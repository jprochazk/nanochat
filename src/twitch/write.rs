use std::fmt::Display;

use tokio::io;
use tokio::io::{AsyncWriteExt, WriteHalf};

use super::{conn, Client};

pub type WriteStream = WriteHalf<conn::Stream>;

impl Client {
  pub async fn send(&mut self, s: &str) -> Result<(), WriteError> {
    self.writer.write_all(s.as_bytes()).await?;
    Ok(())
  }
}

#[derive(Debug)]
pub enum WriteError {
  Io(io::Error),
  StreamClosed,
}

impl From<io::Error> for WriteError {
  fn from(value: io::Error) -> Self {
    Self::Io(value)
  }
}

impl Display for WriteError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      WriteError::Io(e) => write!(f, "failed to write message: {e}"),
      WriteError::StreamClosed => write!(f, "failed to write message: stream closed"),
    }
  }
}

impl std::error::Error for WriteError {}
