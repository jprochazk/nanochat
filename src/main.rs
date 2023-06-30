use std::process::ExitCode;
use std::time::Duration;
use tokio::{select, signal};

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

type Result<T, E = Box<dyn std::error::Error + Send + Sync + 'static>> =
  ::core::result::Result<T, E>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  tracing_subscriber::fmt::init();

  if let Err(e) = try_main().await {
    eprintln!("{e}");
    return ExitCode::FAILURE;
  }

  ExitCode::SUCCESS
}

async fn try_main() -> Result<()> {
  use nanochat::twitch::*;

  let mut client = Client::connect(ChatConfig::anon(), Duration::from_secs(10)).await?;

  client.send("JOIN #moscowwbish\r\n").await?;

  loop {
    select! {
      _ = signal::ctrl_c() => {
        break;
      }
      message = client.message() => {
        let message = message?;
        println!("{message:?}");
      }
    }
  }

  Ok(())
}
