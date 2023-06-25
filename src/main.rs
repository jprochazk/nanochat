use std::process::ExitCode;

use mimalloc::MiMalloc;

#[global_allocator]
static GLOBAL: MiMalloc = MiMalloc;

type Result<T, E = Box<dyn std::error::Error + Send + Sync + 'static>> =
  ::core::result::Result<T, E>;

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
  if let Err(e) = try_main().await {
    eprintln!("{e}");
    return ExitCode::FAILURE;
  }

  ExitCode::SUCCESS
}

async fn try_main() -> Result<()> {
  Ok(())
}
