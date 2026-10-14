//! Under the Canopy: the game executable.

mod app;
mod gpu;
mod options;
mod paths;
mod shot;
mod telemetry;
mod view;
mod world;

use anyhow::Result;
use options::Command;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    telemetry::init();
    let options = options::Options::parse(std::env::args().skip(1))?;
    let paths = paths::Paths::resolve(options.data.clone())?;
    tracing::info!(data = %paths.data.display(), user = %paths.user.display(), "starting");
    match &options.command {
        Command::Shot { out, size } => shot::run(&options, &paths, out, *size),
        Command::Play => {
            let event_loop = EventLoop::new()?;
            let mut app = app::App::new(options, paths);
            event_loop.run_app(&mut app)?;
            app.finish()
        }
    }
}
