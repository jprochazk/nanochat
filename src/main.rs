use std::process::ExitCode;

use nanochat::app::MainWindow;
use nanochat::window::WindowManager;

#[cfg(not(target_os = "android"))]
pub fn main() -> ExitCode {
  tracing_subscriber::fmt::init();

  if let Err(e) = WindowManager::run(MainWindow::new()) {
    eprintln!("{e}");
    return ExitCode::FAILURE;
  }

  ExitCode::SUCCESS
}

#[cfg(any(target_os = "ios", target_os = "android"))]
fn stop_unwind<F: FnOnce() -> T, T>(f: F) -> T {
  match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
    Ok(t) => t,
    Err(err) => {
      eprintln!("stack unwinded with err: {:?}", err);
      std::process::abort()
    }
  }
}

#[cfg(target_os = "ios")]
fn _start_app() {
  stop_unwind(|| main());
}

#[no_mangle]
#[inline(never)]
#[cfg(target_os = "ios")]
pub extern "C" fn start_app() {
  _start_app();
}

#[allow(dead_code)]
#[cfg(target_os = "android")]
#[no_mangle]
fn android_main(app: winit::platform::android::activity::AndroidApp) {
  use winit::platform::android::EventLoopBuilderExtAndroid;

  android_logger::init_once(
    android_logger::Config::default().with_max_level(log::LevelFilter::Warn),
  );

  let event_loop = EventLoopBuilder::with_user_event()
    .with_android_app(app)
    .build();
  stop_unwind(|| _main(event_loop));
}
