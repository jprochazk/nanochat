#[macro_use]
mod macros;

use std::cell::RefCell;
use std::collections::{HashMap, VecDeque};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use winit::event::{Event as WinitEvent, WindowEvent};
use winit::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy, EventLoopWindowTarget};
use winit::window::Window as WinitWindow;

use anyhow::Result;
use winit::window::{WindowBuilder, WindowId};

pub struct WindowManager<E: 'static> {
  id_map: IdMap,
  windows: WindowMap<E>,
  children: WindowChildren,
  repaint_signal: RepaintSignal,
  event_queue: EventQueue<E>,
}

pub struct Context<'a, E: 'static> {
  id: Id,
  ui: &'a egui::Context,
  event_queue: &'a EventQueue<E>,
}

impl<'a, E: 'static> Context<'a, E> {
  fn new(id: Id, ui: &'a egui::Context, event_queue: &'a EventQueue<E>) -> Self {
    Self {
      id,
      ui,
      event_queue,
    }
  }

  pub fn id(&self) -> Id {
    self.id
  }

  pub fn ui(&self) -> &'a egui::Context {
    self.ui
  }

  pub fn notify(&self, recipient: Id, event: E) {
    self
      .event_queue
      .send(Event::notify(self.id, recipient, event));
  }

  pub fn create_window(&self, handler: impl Handler<Event = E> + 'static) -> Id {
    let id = next_id();
    self
      .event_queue
      .send(Event::create(self.id, id, Box::new(handler)));
    id
  }
}

fn next_id() -> Id {
  static ID: AtomicU64 = AtomicU64::new(0);

  Id(ID.fetch_add(1, Ordering::SeqCst))
}

struct Event<E: 'static> {
  from: Id,
  kind: EventKind<E>,
}

enum EventKind<E: 'static> {
  Notify(Id, E),
  Create(Id, Box<dyn Handler<Event = E>>),
}

impl<E: 'static> Event<E> {
  fn notify(from: Id, to: Id, event: E) -> Self {
    Self {
      from,
      kind: EventKind::Notify(to, event),
    }
  }

  fn create(from: Id, new_window_id: Id, handler: Box<dyn Handler<Event = E>>) -> Self {
    Self {
      from,
      kind: EventKind::Create(new_window_id, handler),
    }
  }
}

pub trait Handler {
  type Event: 'static;
  fn on_event(&mut self, from: Id, event: Self::Event) -> bool;
  fn update_and_draw(&mut self, ctx: Context<'_, Self::Event>);
}

type IdMap = HashMap<WindowId, Id>;
type WindowMap<E> = HashMap<Id, Window<E>>;
type WindowChildren = HashMap<Id, Vec<Id>>;

struct EventQueue<E: 'static>(RefCell<VecDeque<Event<E>>>);

impl<E: 'static> EventQueue<E> {
  fn new() -> Self {
    Self(RefCell::new(VecDeque::new()))
  }

  fn is_empty(&self) -> bool {
    self.0.borrow().is_empty()
  }

  fn send(&self, event: Event<E>) {
    self.0.borrow_mut().push_back(event);
  }

  fn recv(&self) -> Option<Event<E>> {
    self.0.borrow_mut().pop_front()
  }
}

impl<E: 'static> WindowManager<E> {
  pub fn run(main: impl Handler<Event = E> + 'static) -> Result<()> {
    let event_loop = EventLoopBuilder::with_user_event().build();
    let mut manager = Self {
      id_map: IdMap::new(),
      windows: WindowMap::new(),
      children: WindowChildren::new(),
      repaint_signal: RepaintSignal(Arc::new(Mutex::new(event_loop.create_proxy()))),
      event_queue: EventQueue::new(),
    };

    manager.create_window(next_id(), &event_loop, None, Box::new(main))?;

    event_loop.run(move |event, event_loop, control_flow| match event {
      WinitEvent::Resumed => {
        exit_if!(manager.on_resume(event_loop), control_flow)
      }
      WinitEvent::Suspended => {
        exit_if!(manager.on_suspend(), control_flow)
      }
      WinitEvent::RedrawRequested(window_id) => {
        exit_if!(
          manager.on_redraw_requested(event_loop, window_id),
          control_flow
        )
      }
      WinitEvent::UserEvent(UserEvent::RequestRedraw(window_id)) => {
        exit_if!(manager.on_user_event(window_id), control_flow)
      }
      WinitEvent::MainEventsCleared => {
        exit_if!(manager.on_main_events_cleared(), control_flow)
      }
      WinitEvent::WindowEvent { event, window_id } => {
        exit_if!(
          manager.on_window_event(window_id, event, control_flow),
          control_flow
        )
      }
      _ => {}
    });
  }

  fn on_resume(&mut self, event_loop: &EventLoopWindowTarget<UserEvent>) -> Result<()> {
    for (_, window) in self.windows.iter_mut() {
      window.on_resume(event_loop, &mut self.id_map)?;
    }
    Ok(())
  }

  fn on_suspend(&mut self) -> Result<()> {
    for (_, window) in self.windows.iter_mut() {
      window.on_suspend(&mut self.id_map)?;
    }
    Ok(())
  }

  fn on_redraw_requested(
    &mut self,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    window_id: WindowId,
  ) -> Result<()> {
    if let Some(window) = self
      .id_map
      .get(&window_id)
      .copied()
      .and_then(|id| self.windows.get_mut(&id))
    {
      window.on_redraw_requested(&self.event_queue)?;
    }

    self.handle_events(event_loop)?;

    Ok(())
  }

  fn on_user_event(&mut self, window_id: WindowId) -> Result<()> {
    if let Some(window) = self
      .id_map
      .get(&window_id)
      .copied()
      .and_then(|id| self.windows.get_mut(&id))
    {
      window.on_user_event()?;
    }
    Ok(())
  }

  fn on_main_events_cleared(&mut self) -> Result<()> {
    for (_, window) in self.windows.iter_mut() {
      window.on_main_events_cleared()?;
    }
    Ok(())
  }

  fn on_window_event(
    &mut self,
    window_id: WindowId,
    event: WindowEvent,
    control_flow: &mut ControlFlow,
  ) -> Result<()> {
    if let Some(window) = self
      .id_map
      .get(&window_id)
      .copied()
      .and_then(|id| self.windows.get_mut(&id))
    {
      let closed = window.on_window_event(event, &mut self.id_map)?;
      if closed {
        // TODO: fully close children (parent stays suspended only)
      }
    }

    // TODO: this is a bit wrong, we shouldn't close immediately when everything is suspended on macos
    if self.id_map.is_empty() {
      // no more open windows, close the app
      *control_flow = ControlFlow::Exit;
    }

    Ok(())
  }

  fn create_window(
    &mut self,
    id: Id,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    parent: Option<Id>,
    handler: Box<dyn Handler<Event = E>>,
  ) -> Result<()> {
    let mut window = Window::new(id, event_loop, self.repaint_signal.clone(), parent, handler)?;
    // resume the window so that it is live, then add it to the window graph
    window.on_resume(event_loop, &mut self.id_map)?;
    if let Some(parent) = parent {
      self
        .children
        .entry(parent)
        .and_modify(|v| v.push(window.id))
        .or_insert_with(Vec::new);
    }
    self.windows.insert(window.id, window);

    Ok(())
  }

  fn handle_events(&mut self, event_loop: &EventLoopWindowTarget<UserEvent>) -> Result<()> {
    if !self.event_queue.is_empty() {
      while let Some(Event { from, kind }) = self.event_queue.recv() {
        match kind {
          EventKind::Notify(to, event) => {
            if let Some(window) = self.windows.get_mut(&to) {
              if window.handler.on_event(from, event) {
                if let Some(window) = window.window.as_ref() {
                  self
                    .repaint_signal
                    .0
                    .lock()
                    .unwrap()
                    .send_event(UserEvent::RequestRedraw(window.id()))?;
                }
              };
            }
          }
          EventKind::Create(id, handler) => {
            self.create_window(id, event_loop, Some(from), handler)?;
          }
        }
      }
    }

    Ok(())
  }
}

#[derive(Debug)]
enum UserEvent {
  RequestRedraw(WindowId),
}

#[derive(Clone)]
struct RepaintSignal(Arc<Mutex<EventLoopProxy<UserEvent>>>);

#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord, Debug)]
#[repr(transparent)]
pub struct Id(u64);

struct Window<E: 'static> {
  parent: Option<Id>,
  id: Id,
  ctx: egui::Context,
  state: egui_winit::State,
  painter: egui_wgpu::winit::Painter,
  window: Option<WinitWindow>,
  repaint_signal: RepaintSignal,
  handler: Box<dyn Handler<Event = E>>,
}

impl<E: 'static> Window<E> {
  #[tracing::instrument(skip(event_loop, repaint_signal, handler))]
  fn new(
    id: Id,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    repaint_signal: RepaintSignal,
    parent: Option<Id>,
    handler: Box<dyn Handler<Event = E>>,
  ) -> Result<Self> {
    let ctx = egui::Context::default();
    let state = egui_winit::State::new(&event_loop);
    let mut config = egui_wgpu::WgpuConfiguration {
      supported_backends: wgpu::Backends::PRIMARY,
      ..Default::default()
    };
    if SUPPORTS_GL_BACKEND {
      config.supported_backends |= wgpu::Backends::GL;
    }
    let painter = egui_wgpu::winit::Painter::new(config, 1, None, false);

    Ok(Self {
      parent,
      id,
      ctx,
      state,
      painter,
      window: None,
      repaint_signal,
      handler,
    })
  }

  fn on_resume(
    &mut self,
    event_loop: &EventLoopWindowTarget<UserEvent>,
    id_map: &mut HashMap<WindowId, Id>,
  ) -> Result<()> {
    let window = match self.window.as_mut() {
      None => {
        let w = self.recreate(event_loop);
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
        id_map.insert(window_id, self.id);
        self.window = Some(w);
        self.window.as_mut().unwrap()
      }
      Some(window) => window,
    };
    window.request_redraw();
    Ok(())
  }

  fn on_suspend(&mut self, id_map: &mut HashMap<WindowId, Id>) -> Result<()> {
    if let Some(window) = self.window.as_ref() {
      id_map.remove(&window.id());
    }
    self.window = None;
    Ok(())
  }

  fn on_redraw_requested(&mut self, event_queue: &EventQueue<E>) -> Result<()> {
    if let Some(window) = self.window.as_ref() {
      let raw_input = self.state.take_egui_input(window);

      let output = self.ctx.run(raw_input, |ui| {
        self
          .handler
          .update_and_draw(Context::new(self.id, ui, event_queue))
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

  fn on_window_event(&mut self, event: WindowEvent, id_map: &mut IdMap) -> Result<bool> {
    match event {
      WindowEvent::Resized(size) => {
        self.painter.on_window_resized(size.width, size.height);
      }
      WindowEvent::CloseRequested => {
        self.on_suspend(id_map)?;
        return Ok(true);
      }
      _ => {}
    }

    let response = self.state.on_event(&self.ctx, &event);
    if response.repaint {
      if let Some(window) = self.window.as_ref() {
        window.request_redraw();
      }
    }

    Ok(false)
  }

  fn recreate(&mut self, event_loop: &EventLoopWindowTarget<UserEvent>) -> WinitWindow {
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

    if let Some(max_size) = self.painter.max_texture_side() {
      self.state.set_max_texture_side(max_size);
    }

    let pixels_per_point = window.scale_factor() as f32;
    self.state.set_pixels_per_point(pixels_per_point);

    window.request_redraw();

    window
  }
}

// this is probably a bug in egui-wgpu
const SUPPORTS_GL_BACKEND: bool = cfg!(not(target_os = "linux"));
