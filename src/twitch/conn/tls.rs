use std::fmt::Display;
use std::io;
use std::sync::Arc;

use tokio_rustls::rustls;
use tokio_rustls::rustls::{ClientConfig, RootCertStore, ServerName};

#[derive(Debug, Clone)]
pub struct TlsConfig {
  config: Arc<ClientConfig>,
  server_name: ServerName,
}

impl TlsConfig {
  pub fn load(server_name: ServerName) -> Result<Self, TlsConfigError> {
    tracing::debug!("loading native certificates");
    let mut root_store = RootCertStore::empty();
    let native_certs = rustls_native_certs::load_native_certs()?;
    for cert in native_certs {
      root_store.add(&rustls::Certificate(cert.0))?;
    }
    let config = rustls::ClientConfig::builder()
      .with_safe_defaults()
      .with_root_certificates(root_store)
      .with_no_client_auth();
    Ok(Self {
      config: Arc::new(config),
      server_name,
    })
  }

  pub fn client(&self) -> Arc<ClientConfig> {
    self.config.clone()
  }

  pub fn server_name(&self) -> ServerName {
    self.server_name.clone()
  }
}

#[derive(Debug)]
pub enum TlsConfigError {
  Io(io::Error),
  Tls(rustls::Error),
}

impl From<io::Error> for TlsConfigError {
  fn from(value: io::Error) -> Self {
    Self::Io(value)
  }
}

impl From<rustls::Error> for TlsConfigError {
  fn from(value: rustls::Error) -> Self {
    Self::Tls(value)
  }
}

impl Display for TlsConfigError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      TlsConfigError::Io(e) => write!(f, "tls config error: {e}"),
      TlsConfigError::Tls(e) => write!(f, "tls config error: {e}"),
    }
  }
}

impl std::error::Error for TlsConfigError {}
