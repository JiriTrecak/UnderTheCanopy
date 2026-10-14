//! Under the Canopy: the game executable.

mod app;
mod gpu;
mod options;
mod paths;
mod telemetry;

use anyhow::Result;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    telemetry::init();
    let options = options::Options::parse(std::env::args().skip(1))?;
    let paths = paths::Paths::resolve(options.data.clone())?;
    tracing::info!(data = %paths.data.display(), user = %paths.user.display(), "starting");

    let event_loop = EventLoop::new()?;
    let mut app = app::App::new(options);
    event_loop.run_app(&mut app)?;
    app.finish()
}
