pub mod tls;

use std::fmt::Display;
use std::io;

use tokio::net::TcpStream;
use tokio_rustls::client::TlsStream;
use tokio_rustls::TlsConnector;

use self::tls::TlsConfig;

pub const HOST: &str = "irc.chat.twitch.tv";
pub const PORT: u16 = 6697;

pub type Stream = TlsStream<TcpStream>;

pub async fn open(config: TlsConfig) -> Result<Stream, OpenStreamError> {
  tracing::debug!(?config, "opening tls stream to twitch");
  Ok(
    TlsConnector::from(config.client())
      .connect(
        config.server_name(),
        TcpStream::connect((HOST, PORT)).await?,
      )
      .await?,
  )
}

#[derive(Debug)]
pub enum OpenStreamError {
  Io(io::Error),
}

impl From<io::Error> for OpenStreamError {
  fn from(value: io::Error) -> Self {
    Self::Io(value)
  }
}

impl Display for OpenStreamError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      OpenStreamError::Io(e) => write!(f, "failed to open tls stream: {e}"),
    }
  }
}

impl std::error::Error for OpenStreamError {}
