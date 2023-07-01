use std::collections::HashMap;
use std::process::ExitCode;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use winit::event::{Event, WindowEvent};
use winit::event_loop::{
  ControlFlow, EventLoop, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget,
};
use winit::window::Window;

use anyhow::Result;
use winit::window::{WindowBuilder, WindowId};

enum UserEvent {
  RequestRedraw(WindowId),
}

#[derive(Clone)]
struct RepaintSignal(Arc<Mutex<EventLoopProxy<UserEvent>>>);

macro_rules! exit_if {
  ($e:expr, $c:ident) => {
    match $e {
      Ok(v) => v,
      Err(e) => {
        eprintln!("{e}");
        *$c = ControlFlow::ExitWithCode(1);
        return;
      }
    }
  };
}

fn run(event_loop: EventLoop<UserEvent>) -> Result<()> {
  let repaint_signal = RepaintSignal(Arc::new(Mutex::new(event_loop.create_proxy())));
  let mut app_windows = HashMap::new();
  for _ in 0..2 {
    let window = AppWindow::new(&event_loop, repaint_signal.clone())?;
    app_windows.insert(window.id, window);
  }
  let mut window_map = HashMap::<WindowId, AppWindowId>::new();

  event_loop.run(move |event, event_loop, control_flow| match event {
    Event::Resumed => {
      for (_, window) in app_windows.iter_mut() {
        exit_if!(window.on_resume(event_loop, &mut window_map), control_flow);
      }
    }
    Event::Suspended => {
      for (_, window) in app_windows.iter_mut() {
        exit_if!(window.on_suspend(&mut window_map), control_flow);
      }
    }
    Event::RedrawRequested(window_id) => {
      if let Some(window) = window_map
        .get(&window_id)
        .copied()
        .and_then(|id| app_windows.get_mut(&id))
      {
        exit_if!(window.on_redraw_requested(), control_flow);
      }
    }
    Event::UserEvent(UserEvent::RequestRedraw(window_id)) => {
      if let Some(window) = window_map
        .get(&window_id)
        .copied()
        .and_then(|id| app_windows.get_mut(&id))
      {
        exit_if!(window.on_user_event(), control_flow);
      }
    }
    Event::MainEventsCleared => {
      for (_, window) in app_windows.iter_mut() {
        exit_if!(window.on_main_events_cleared(), control_flow);
      }
    }
    Event::WindowEvent { event, window_id } => {
      if let Some(window) = window_map
        .get(&window_id)
        .copied()
        .and_then(|id| app_windows.get_mut(&id))
      {
        exit_if!(
          window.on_window_event(event, control_flow, &mut window_map),
          control_flow
        );
      }
    }
    _ => {}
  });
}

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct AppWindowId(u64);

struct AppWindow {
  id: AppWindowId,
  ctx: egui::Context,
  state: egui_winit::State,
  painter: egui_wgpu::winit::Painter,
  window: Option<Window>,
  repaint_signal: RepaintSignal,
  egui_demo: egui_demo_lib::DemoWindows,
}

impl AppWindow {
  #[tracing::instrument(skip(event_loop, repaint_signal))]
  fn new(event_loop: &EventLoop<UserEvent>, repaint_signal: RepaintSignal) -> Result<Self> {
    static ID: AtomicU64 = AtomicU64::new(0);

    let id = AppWindowId(ID.fetch_add(1, Ordering::SeqCst));

    let ctx = egui::Context::default();
    let state = egui_winit::State::new(&event_loop);

    let mut config = egui_wgpu::WgpuConfiguration {
      supported_backends: wgpu::Backends::PRIMARY,
      ..Default::default()
    };
    if supports_gl_backend() {
      config.supported_backends |= wgpu::Backends::GL;
    }

    let painter = egui_wgpu::winit::Painter::new(config, 1, None, false);

    Ok(Self {
      id,
      ctx,
      state,
      painter,
      window: None,
      repaint_signal,
      egui_demo: egui_demo_lib::DemoWindows::default(),
    })
  }

  fn on_resume(
    &mut self,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    window_map: &mut HashMap<WindowId, AppWindowId>,
  ) -> Result<()> {
    let window = match self.window.as_mut() {
      None => {
        let w = self.create_window(event_loop);
        pollster::block_on(self.painter.set_window(Some(&w)))?;
        let window_id = w.id();
        let repaint_signal = self.repaint_signal.clone();
        self.ctx.set_request_repaint_callback(move |_| {
          let _ = repaint_signal
            .0
            .lock()
            .unwrap()
            .send_event(UserEvent::RequestRedraw(window_id));
        });
        window_map.insert(window_id, self.id);
        self.window = Some(w);
        self.window.as_mut().unwrap()
      }
      Some(window) => window,
    };
    window.request_redraw();
    Ok(())
  }

  fn on_suspend(&mut self, window_map: &mut HashMap<WindowId, AppWindowId>) -> Result<()> {
    if let Some(window) = self.window.as_ref() {
      window_map.remove(&window.id());
    }
    self.window = None;
    Ok(())
  }

  fn on_redraw_requested(&mut self) -> Result<()> {
    if let Some(window) = self.window.as_ref() {
      let raw_input = self.state.take_egui_input(window);
      let output = self.ctx.run(raw_input, |ctx| {
        self.egui_demo.ui(ctx);
      });
      self
        .state
        .handle_platform_output(window, &self.ctx, output.platform_output);
      self.painter.paint_and_update_textures(
        self.state.pixels_per_point(),
        egui::Rgba::default().to_array(),
        &self.ctx.tessellate(output.shapes),
        &output.textures_delta,
        false,
      );
      if output.repaint_after.is_zero() {
        window.request_redraw();
      }
    }
    Ok(())
  }

  fn on_user_event(&mut self) -> Result<()> {
    if let Some(window) = self.window.as_ref() {
      window.request_redraw();
    }
    Ok(())
  }

  fn on_main_events_cleared(&mut self) -> Result<()> {
    if let Some(window) = self.window.as_ref() {
      window.request_redraw();
    }
    Ok(())
  }

  fn on_window_event(
    &mut self,
    event: WindowEvent,
    control_flow: &mut ControlFlow,
    window_map: &mut HashMap<WindowId, AppWindowId>,
  ) -> Result<()> {
    match event {
      WindowEvent::Resized(size) => {
        self.painter.on_window_resized(size.width, size.height);
      }
      WindowEvent::CloseRequested => {
        self.on_suspend(window_map)?;
        if window_map.is_empty() {
          // no more open windows, close the app
          *control_flow = ControlFlow::Exit;
        }
      }
      _ => {}
    }

    let response = self.state.on_event(&self.ctx, &event);
    if response.repaint {
      if let Some(window) = self.window.as_ref() {
        window.request_redraw();
      }
    }
    Ok(())
  }

  fn create_window(&mut self, event_loop: &EventLoopWindowTarget<UserEvent>) -> Window {
    let window = WindowBuilder::new()
      .with_decorations(true)
      .with_resizable(true)
      .with_transparent(false)
      .with_title("egui winit + wgpu example")
      .with_inner_size(winit::dpi::PhysicalSize {
        width: 640,
        height: 640,
      })
      .build(event_loop)
      .unwrap();

    pollster::block_on(self.painter.set_window(Some(&window))).unwrap();

    // NB: calling set_window will lazily initialize render state which
    // means we will be able to query the maximum supported texture
    // dimensions
    if let Some(max_size) = self.painter.max_texture_side() {
      self.state.set_max_texture_side(max_size);
    }

    let pixels_per_point = window.scale_factor() as f32;
    self.state.set_pixels_per_point(pixels_per_point);

    window.request_redraw();

    window
  }
}

#[cfg(target_os = "linux")]
fn supports_gl_backend() -> bool {
  use std::sync::OnceLock;

  static GL_DISABLED: OnceLock<bool> = OnceLock::new();

  let value = *GL_DISABLED.get_or_init(|| {
    // software GL works fine
    if let Ok(software_gl) = std::env::var("LIBGL_ALWAYS_SOFTWARE") {
      match software_gl.to_lowercase().as_str() {
        "1" | "t" | "true" => return false,
        _ => {}
      }
    }

    // WSL2 for some reason doesn't work.
    // if we detect that we're in WSL, we disable the GL backend
    // the detection relies on https://github.com/microsoft/WSL/issues/423#issuecomment-221627364
    let v = std::fs::read_to_string("/proc/version")
      .expect("failed to read `/proc/version`")
      .contains("microsoft");

    if v {
      tracing::info!("WSL detected - GL backend will be unavailable")
    }

    v
  });

  !value
}

#[cfg(not(target_os = "linux"))]
#[inline(always)]
fn supports_gl_backend() -> bool {
  true
}

#[cfg(not(target_os = "android"))]
pub fn main() -> ExitCode {
  tracing_subscriber::fmt::init();

  let event_loop = EventLoopBuilder::with_user_event().build();
  if let Err(e) = run(event_loop) {
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
