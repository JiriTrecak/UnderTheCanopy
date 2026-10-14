//! The window and its event loop.

use crate::{gpu::Gpu, options::Options, telemetry};
use anyhow::Result;
use std::sync::Arc;
use winit::{
    application::ApplicationHandler,
    dpi::LogicalSize,
    event::WindowEvent,
    event_loop::ActiveEventLoop,
    window::{Window, WindowId},
};

pub struct App {
    options: Options,
    window: Option<Arc<Window>>,
    gpu: Option<Gpu>,
    error: Option<anyhow::Error>,
}

impl App {
    pub fn new(options: Options) -> Self {
        Self {
            options,
            window: None,
            gpu: None,
            error: None,
        }
    }

    /// The error that stopped the game, if any.
    pub fn finish(self) -> Result<()> {
        self.error.map_or(Ok(()), Err)
    }

    fn open(&mut self, event_loop: &ActiveEventLoop) -> Result<()> {
        let attributes = Window::default_attributes()
            .with_title("Under the Canopy")
            .with_inner_size(LogicalSize::new(1600, 900))
            .with_min_inner_size(LogicalSize::new(1024, 600));
        let window = Arc::new(event_loop.create_window(attributes)?);
        self.gpu = Some(Gpu::new(window.clone(), self.options.vsync)?);
        window.request_redraw();
        self.window = Some(window);
        Ok(())
    }
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        if let Err(error) = self.open(event_loop) {
            self.error = Some(error);
            event_loop.exit();
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _: WindowId, event: WindowEvent) {
        let (Some(window), Some(gpu)) = (&self.window, &mut self.gpu) else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => gpu.resize(size),
            WindowEvent::RedrawRequested => {
                gpu.render();
                telemetry::frame_mark();
                window.request_redraw();
            }
            _ => {}
        }
    }
}
