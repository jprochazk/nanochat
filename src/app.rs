use std::time::{Duration, Instant};

use crate::window;

pub enum Event {
  Test,
}

pub struct MainWindow {
  test: Instant,
  floating_button: Option<window::Id>,
}

impl MainWindow {
  pub fn new() -> Self {
    MainWindow {
      test: Instant::now(),
      floating_button: None,
    }
  }

  pub fn draw(&mut self, ctx: window::Context<'_, Event>) {
    egui::CentralPanel::default().show(ctx.ui(), |ui| {
      if self.test.elapsed() < Duration::from_secs(1) {
        ui.label("Test");
      }

      if self.floating_button.is_none() && ui.button("open test button window").clicked() {
        self.floating_button = Some(ctx.create_window(FloatingButton::new(ctx.id())));
      }
    });
  }
}

impl window::Handler for MainWindow {
  type Event = Event;

  fn on_event(&mut self, _: window::Id, event: Self::Event) -> bool {
    match event {
      Event::Test => self.test = Instant::now(),
    }

    true
  }

  fn update_and_draw(&mut self, ctx: window::Context<'_, Self::Event>) {
    self.draw(ctx)
  }
}

pub struct FloatingButton {
  parent: window::Id,
}

impl FloatingButton {
  pub fn new(parent: window::Id) -> Self {
    Self { parent }
  }

  pub fn draw(&mut self, ctx: window::Context<'_, Event>) {
    egui::CentralPanel::default().show(ctx.ui(), |ui| {
      if ui.button("Test").clicked() {
        ctx.notify(self.parent, Event::Test);
      }
    });
  }
}

impl window::Handler for FloatingButton {
  type Event = Event;

  fn on_event(&mut self, _: window::Id, _: Self::Event) -> bool {
    false
  }

  fn update_and_draw(&mut self, ctx: window::Context<'_, Self::Event>) {
    self.draw(ctx)
  }
}
