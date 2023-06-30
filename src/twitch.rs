pub mod conn;
mod read;
mod write;

use std::fmt::{Display, Write};
use std::future::Future;
use std::io;
use std::time::Duration;

use futures_util::StreamExt;
use tokio_stream::wrappers::LinesStream;

use rand::{thread_rng, Rng};
use tokio::io::{AsyncWriteExt, BufReader};
use tokio_rustls::rustls::client::InvalidDnsNameError;
use tokio_rustls::rustls::ServerName;

use tokio::io::AsyncBufReadExt;

use crate::util::Timeout;

use self::conn::tls::{TlsConfig, TlsConfigError};
use self::conn::OpenStreamError;
use self::read::{ReadError, ReadStream};
use self::write::WriteStream;

pub struct ChatConfig {
  pub nick: String,
  pub pass: String,
}

impl ChatConfig {
  pub fn new(nick: impl ToString, pass: impl ToString) -> Self {
    Self {
      nick: nick.to_string(),
      pass: pass.to_string(),
    }
  }

  pub fn anon() -> Self {
    Self {
      pass: "just_a_lil_guy".into(),
      nick: format!("justinfan{}", thread_rng().gen_range(10000u32..99999u32)),
    }
  }

  pub fn connect(self, timeout: Duration) -> impl Future<Output = Result<Client, ConnectionError>> {
    Client::connect(self, timeout)
  }
}

pub struct Client {
  reader: ReadStream,
  writer: WriteStream,

  scratch: String,
  tls: TlsConfig,
  config: ChatConfig,
}

impl Client {
  pub async fn connect(config: ChatConfig, timeout: Duration) -> Result<Client, ConnectionError> {
    tracing::debug!("connecting");
    let tls = TlsConfig::load(ServerName::try_from(conn::HOST)?)?;
    tracing::debug!("opening connection to twitch");
    let stream = conn::open(tls.clone()).timeout(timeout).await??;
    let (reader, writer) = split(stream);
    let mut chat = Client {
      reader,
      writer,
      scratch: String::with_capacity(1024),
      tls,
      config,
    };
    chat.handshake().timeout(timeout).await??;
    Ok(chat)
  }

  pub async fn reconnect(&mut self, timeout: Duration) -> Result<(), ConnectionError> {
    tracing::debug!("reconnecting");

    let mut tries = 10;
    let mut delay = Duration::from_secs(3);

    while tries != 0 {
      tokio::time::sleep(delay).await;
      tries -= 1;
      delay *= 3;

      tracing::debug!("opening connection to twitch");
      let stream = match conn::open(self.tls.clone()).timeout(timeout).await? {
        Ok(stream) => stream,
        Err(OpenStreamError::Io(_)) => continue,
      };

      (self.reader, self.writer) = split(stream);

      if let Err(e) = self.handshake().timeout(timeout).await? {
        if e.should_retry() {
          continue;
        } else {
          return Err(e);
        }
      };

      return Ok(());
    }

    Err(ConnectionError::Reconnect)
  }

  async fn handshake(&mut self) -> Result<(), ConnectionError> {
    tracing::debug!("performing handshake");

    const CAP: &str = "twitch.tv/commands twitch.tv/tags";
    tracing::debug!("CAP REQ {CAP}; NICK {}; PASS ***", self.config.nick);

    write!(&mut self.scratch, "CAP REQ :{CAP}\r\n").unwrap();
    write!(&mut self.scratch, "NICK {}\r\n", self.config.nick).unwrap();
    write!(&mut self.scratch, "PASS {}\r\n", self.config.pass).unwrap();

    self.writer.write_all(self.scratch.as_bytes()).await?;
    self.writer.flush().await?;
    self.scratch.clear();

    tracing::debug!("waiting for CAP * ACK");
    let message = self.message().timeout(Duration::from_secs(5)).await??;
    tracing::debug!(?message, "received message");

    match message.command() {
      twitch::Command::Capability => {
        if message.params().is_some_and(|v| v.starts_with("* ACK")) {
          tracing::debug!("received CAP * ACK")
        } else {
          return Err(ConnectionError::InvalidAuth);
        }
      }
      _ => {
        tracing::debug!("unexpected message");
        return Err(ConnectionError::InvalidFirstMessage(message));
      }
    }

    tracing::debug!("waiting for NOTICE 001");
    let message = self.message().timeout(Duration::from_secs(5)).await??;
    tracing::debug!(?message, "received message");

    match message.command() {
      twitch::Command::RplWelcome => {
        tracing::debug!("connected");
      }
      twitch::Command::Notice => {
        if message
          .params()
          .map(|v| v.contains("authentication failed"))
          .unwrap_or(false)
        {
          tracing::debug!("invalid credentials");
          return Err(ConnectionError::InvalidAuth);
        } else {
          tracing::debug!("unrecognized error");
          return Err(ConnectionError::Notice(message));
        }
      }
      _ => {
        tracing::debug!("first message not recognized");
        return Err(ConnectionError::InvalidFirstMessage(message));
      }
    }

    Ok(())
  }
}

fn split(stream: conn::Stream) -> (ReadStream, WriteStream) {
  let (reader, writer) = tokio::io::split(stream);

  (
    LinesStream::new(BufReader::new(reader).lines()).fuse(),
    writer,
  )
}

#[derive(Debug)]
pub enum ConnectionError {
  Read(ReadError),
  Io(io::Error),
  Dns(InvalidDnsNameError),
  Tls(TlsConfigError),
  Open(OpenStreamError),
  Timeout(tokio::time::error::Elapsed),
  InvalidFirstMessage(twitch::Message),
  InvalidAuth,
  Notice(twitch::Message),
  Reconnect,
}

impl ConnectionError {
  fn should_retry(&self) -> bool {
    matches!(self, Self::Open(OpenStreamError::Io(_)) | Self::Io(_))
  }
}

impl From<ReadError> for ConnectionError {
  fn from(value: ReadError) -> Self {
    Self::Read(value)
  }
}

impl From<io::Error> for ConnectionError {
  fn from(value: io::Error) -> Self {
    Self::Io(value)
  }
}

impl From<InvalidDnsNameError> for ConnectionError {
  fn from(value: InvalidDnsNameError) -> Self {
    Self::Dns(value)
  }
}

impl From<TlsConfigError> for ConnectionError {
  fn from(value: TlsConfigError) -> Self {
    Self::Tls(value)
  }
}

impl From<OpenStreamError> for ConnectionError {
  fn from(value: OpenStreamError) -> Self {
    Self::Open(value)
  }
}

impl From<tokio::time::error::Elapsed> for ConnectionError {
  fn from(value: tokio::time::error::Elapsed) -> Self {
    Self::Timeout(value)
  }
}

impl Display for ConnectionError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      ConnectionError::Read(e) => write!(f, "failed to connect: {e}"),
      ConnectionError::Io(e) => write!(f, "failed to connect: {e}"),
      ConnectionError::Dns(e) => write!(f, "failed to connect: {e}"),
      ConnectionError::Tls(e) => write!(f, "failed to connect: {e}"),
      ConnectionError::Open(e) => write!(f, "failed to connect: {e}"),
      ConnectionError::Timeout(e) => write!(f, "failed to connect: connection timed out, {e}"),
      ConnectionError::InvalidFirstMessage(msg) => write!(
        f,
        "failed to connect: expected `NOTICE` or `001` as first message, instead received: {msg:?}"
      ),
      ConnectionError::InvalidAuth => write!(f, "failed to connect: invalid credentials"),
      ConnectionError::Notice(msg) => write!(
        f,
        "failed to connect: received unrecognized notice: {msg:?}"
      ),
      ConnectionError::Reconnect => write!(f, "failed to connect: reconnect attempt failed"),
    }
  }
}

impl std::error::Error for ConnectionError {}
