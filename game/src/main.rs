//! Under the Canopy: the game executable.

// First, so `tr!` reaches every module below (D96).
#[macro_use]
mod text;
mod allocs;
mod app;
mod bench;
mod campaign;
mod clipboard;
mod data;
mod feedback;
mod files;
mod gpu;
mod live;
mod options;
mod paths;
mod play;
mod replay;
mod saves;
mod screens;
mod settings;
mod shot;
mod sim;
mod telemetry;
mod update;
mod view;
mod world;

use anyhow::Result;
use options::Command;
use winit::event_loop::EventLoop;

fn main() -> Result<()> {
    telemetry::init();
    // The simulation's worker threads (`canopy_sim::parallel`) work inside ticks.
    if let Err(error) = rayon::ThreadPoolBuilder::new()
        .start_handler(|_| {
            canopy_render::priority::raise_current_thread();
        })
        .build_global()
    {
        tracing::debug!(%error, "worker threads left at their priority");
    }
    if std::env::args().nth(1).as_deref() == Some("bench-render") {
        let result = bench::run(std::env::args().skip(2));
        telemetry::print_span_times();
        return result;
    }
    let options = options::Options::parse(std::env::args().skip(1))?;
    let paths = paths::Paths::resolve(options.data.clone())?;
    tracing::info!(data = %paths.data.display(), user = %paths.user.display(), "starting");
    if matches!(options.command, Command::Edit | Command::Mcp) && paths.repository().is_none() {
        anyhow::bail!(
            "the editor edits a repository checkout's base package; {} is not one (run `just edit` in the repository, or pass --data <checkout>)",
            paths.data.display()
        );
    }
    let result = match &options.command {
        Command::Shot { out, size } => shot::run(&options, &paths, out, *size),
        Command::CaptureScreens {
            out,
            size,
            scale,
            only,
        } => screens::capture::run(&paths, out, *size, *scale, only.as_deref()),
        Command::Replay {
            file,
            record: Some((map, ticks)),
            ..
        } => replay::verify::record_stand_in(&paths, map, *ticks, file),
        Command::Replay {
            file, verify: true, ..
        } => replay::verify::run(&paths, file),
        Command::Mcp => {
            canopy_editor::serve_headless(app::editor_setup(&paths)).map_err(anyhow::Error::msg)
        }
        Command::Play | Command::Edit | Command::Replay { .. } => {
            // The interface's system fonts scan while the window and the GPU start.
            canopy_ui::text::preload_system_fonts();
            let event_loop = EventLoop::new()?;
            let mut app = app::App::new(options, paths);
            event_loop.run_app(&mut app)?;
            app.finish()
        }
    };
    telemetry::print_span_times();
    result
}
