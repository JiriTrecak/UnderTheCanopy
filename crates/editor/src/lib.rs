//! The editor (docs/editor.md): one native editor for maps, units and heroes, abilities,
//! effects, items and buildings, races and rules, and assets. The game opens it as a mode
//! (the main menu's Editor entry, `canopy edit`); until then nothing of it exists, so
//! players pay nothing for it.
//!
//! [`Editor`] owns its egui context and draws the whole window: the game hands it window
//! events while it is open, calls [`Editor::run`] once a frame and [`Editor::paint`] to
//! record the frame. The look is [`theme`] (Tailwind's zinc, as the old spell studio).

mod checker;
mod document;
mod forms;
mod icons;
mod inspector;
mod library;
mod map;
mod mcp;
mod overview;
mod references;
mod shell;
mod stage;
mod tabs;
pub mod theme;
mod watch;
mod widgets;

pub use document::Command;
pub use library::Kind;
use std::path::PathBuf;
use winit::{event::WindowEvent, window::Window};

/// What the editor needs from the game.
pub struct Setup {
    /// The data folder (the repository): its cooked pack and lighting draw the viewport.
    pub data: PathBuf,
    /// The base package's folder.
    pub base: PathBuf,
    /// Font files for the interface, first choice first (egui's own stay as fallbacks).
    pub fonts: Vec<Vec<u8>>,
    /// The user folder: the MCP token and the editor's settings.
    pub user: Option<PathBuf>,
}

/// What the editor asks of the game after a frame.
#[derive(Debug, Default)]
pub struct Output {
    /// Back to the game's menus.
    pub close: bool,
    /// Quit the game (the window was closed and unsaved changes are settled).
    pub quit: bool,
    /// Test in game: play this content (unsaved edits included).
    pub test: Option<Test>,
}

/// What Test in game plays.
#[derive(Debug, Clone)]
pub struct Test {
    pub content: std::sync::Arc<canopy_content::Content>,
    /// The open map, when the maps editor shows one.
    pub map: Option<String>,
    /// A unit trying its abilities near the player's start, instead of a skirmish.
    pub encounter: Option<String>,
}

pub struct Editor {
    ctx: egui::Context,
    /// Window input; `None` for an editor rendered offscreen ([`Editor::headless`]).
    state: Option<egui_winit::State>,
    renderer: egui_wgpu::Renderer,
    jobs: Vec<egui::ClippedPrimitive>,
    screen: egui_wgpu::ScreenDescriptor,
    shell: shell::Shell,
    /// The viewport's renderer, made when a viewport first shows (or why it could not be).
    stage: Option<Result<stage::Stage, String>>,
    data: PathBuf,
}

impl Editor {
    /// An editor in `window`, drawing into `format` targets (the non-sRGB view of the
    /// surface).
    pub fn new(
        window: &Window,
        gpu: &canopy_render::Gpu,
        format: wgpu::TextureFormat,
        setup: Setup,
    ) -> Self {
        let mut editor = Self::headless(gpu, format, setup);
        editor.shell.autostart_mcp();
        editor.state = Some(egui_winit::State::new(
            editor.ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        ));
        editor
    }

    /// An editor without a window, for offscreen captures ([`Editor::run_headless`]).
    pub fn headless(gpu: &canopy_render::Gpu, format: wgpu::TextureFormat, setup: Setup) -> Self {
        let ctx = egui::Context::default();
        theme::apply(&ctx, &setup.fonts);
        let renderer =
            egui_wgpu::Renderer::new(&gpu.device, format, egui_wgpu::RendererOptions::default());
        let package =
            canopy_content::package::Package::open(&setup.base).map_err(|e| e.to_string());
        if let Err(error) = &package {
            tracing::warn!(%error, "editor: base package");
        }
        Self {
            ctx,
            state: None,
            renderer,
            jobs: Vec::new(),
            screen: egui_wgpu::ScreenDescriptor {
                size_in_pixels: [1, 1],
                pixels_per_point: 1.0,
            },
            shell: shell::Shell::new(package, setup.user, Some(setup.data.clone())),
            stage: None,
            data: setup.data,
        }
    }

    /// Feeds a window event.
    pub fn event(&mut self, window: &Window, event: &WindowEvent) {
        if let Some(state) = &mut self.state {
            let _ = state.on_window_event(window, event);
        }
    }

    /// Builds this frame's interface and uploads what it draws.
    pub fn run(&mut self, window: &Window, gpu: &canopy_render::Gpu) -> Output {
        let Some(state) = &mut self.state else {
            return Output::default();
        };
        let input = state.take_egui_input(window);
        let size = window.inner_size();
        let (output, platform) = self.frame(input, [size.width, size.height], gpu);
        if let Some(state) = &mut self.state {
            state.handle_platform_output(window, platform);
        }
        output
    }

    /// A frame of `size` physical pixels at `scale` without a window (captures).
    pub fn run_headless(&mut self, size: [u32; 2], scale: f32, gpu: &canopy_render::Gpu) -> Output {
        let mut input = egui::RawInput {
            screen_rect: Some(egui::Rect::from_min_size(
                egui::Pos2::ZERO,
                egui::vec2(size[0] as f32, size[1] as f32) / scale,
            )),
            ..Default::default()
        };
        input
            .viewports
            .entry(egui::ViewportId::ROOT)
            .or_default()
            .native_pixels_per_point = Some(scale);
        self.frame(input, size, gpu).0
    }

    /// The window is closing: true when it may close now, false when the editor first asks
    /// about unsaved changes (its answer comes as [`Output::quit`]).
    pub fn close_requested(&mut self) -> bool {
        self.shell.leave(shell::Leave::Quit);
        self.shell.close.take() == Some(shell::Leave::Quit)
    }

    /// The document of the current editor's focused tab.
    pub fn active_document(&mut self) -> Option<String> {
        self.shell.active_document()
    }

    /// Runs `commands` as one undo step in the current editor (captures; MCP later).
    pub fn run_step(&mut self, label: &str, commands: Vec<Command>) -> Result<(), String> {
        self.shell.run_step(label, commands)
    }

    /// Whether the check of the latest edits is still running (captures wait for it).
    pub fn checking(&self) -> bool {
        self.shell.checking()
    }

    /// The game data of the last check that passed (Test in game; captures).
    pub fn checked_content(&self) -> Option<std::sync::Arc<canopy_content::Content>> {
        self.shell.checked_content()
    }

    /// Takes back every step (captures).
    pub fn undo_all(&mut self) {
        self.shell.undo_all();
    }

    /// Opens or closes the problems list (captures).
    pub fn show_problems(&mut self, open: bool) {
        self.shell.show_problems(open);
    }

    /// Switches editors (captures).
    pub fn select(&mut self, kind: Kind) {
        self.shell.select(kind);
    }

    /// Opens the current editor's first `n` elements (captures).
    pub fn open_first(&mut self, n: usize) {
        self.shell.open_first(n);
    }

    pub fn set_assistant(&mut self, open: bool) {
        self.shell.set_assistant(open);
    }

    fn frame(
        &mut self,
        input: egui::RawInput,
        size: [u32; 2],
        gpu: &canopy_render::Gpu,
    ) -> (Output, egui::PlatformOutput) {
        let (device, queue) = (&gpu.device, &gpu.queue);
        // The viewport shows last frame's picture; this frame's is drawn below.
        self.shell.viewport_texture = match &self.stage {
            Some(Ok(stage)) => stage.texture(),
            _ => None,
        };
        self.shell.viewport_error = match &self.stage {
            Some(Err(error)) => Some(error.clone()),
            _ => None,
        };
        let shell = &mut self.shell;
        let output = self.ctx.run_ui(input, |ui| shell.ui(ui));
        let (shot, scene) = (self.shell.shot.take(), self.shell.scene.take());
        if shot.is_some() || scene.is_some() {
            let stage = self
                .stage
                .get_or_insert_with(|| stage::Stage::new(gpu, &self.data));
            if let Ok(stage) = stage {
                if let Some(scene) = scene {
                    stage.draw_scene(gpu, &mut self.renderer, &scene);
                } else if let Some(shot) = shot {
                    stage.draw(gpu, &mut self.renderer, &shot);
                }
            }
        }
        let egui::FullOutput {
            platform_output,
            mut textures_delta,
            shapes,
            pixels_per_point,
            ..
        } = output;
        self.screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size[0].max(1), size[1].max(1)],
            pixels_per_point,
        };
        self.jobs = self.ctx.tessellate(shapes, pixels_per_point);
        // Every delta must be taken (epaint checks on drop).
        let set: Vec<_> = textures_delta.set.drain().collect();
        for (id, deltas) in &set {
            for delta in deltas {
                self.renderer.update_texture(device, queue, *id, delta);
            }
        }
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("editor upload"),
        });
        let extra =
            self.renderer
                .update_buffers(device, queue, &mut encoder, &self.jobs, &self.screen);
        queue.submit(extra.into_iter().chain([encoder.finish()]));
        for id in std::mem::take(&mut textures_delta.free) {
            self.renderer.free_texture(&id);
        }
        let close = self.shell.close.take();
        let output = Output {
            close: close == Some(shell::Leave::Menu),
            quit: close == Some(shell::Leave::Quit),
            test: self.shell.test.take(),
        };
        (output, platform_output)
    }

    /// Records the frame into `view`, clearing it first.
    pub fn paint(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let [r, g, b, _] = theme::BG.to_array();
        let channel = |c: u8| f64::from(c) / 255.0;
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("editor"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    // The view is not sRGB: the palette's values go in as they are.
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: channel(r),
                        g: channel(g),
                        b: channel(b),
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        });
        let mut pass = pass.forget_lifetime();
        self.renderer.render(&mut pass, &self.jobs, &self.screen);
    }
}

/// Runs the editor without a window for agents (`canopy mcp`): the same documents, check,
/// file watch and MCP server; never returns unless the server cannot start.
pub fn serve_headless(setup: Setup) -> Result<(), String> {
    let package = canopy_content::package::Package::open(&setup.base).map_err(|e| e.to_string())?;
    let mut shell = shell::Shell::new(Ok(package), setup.user, Some(setup.data));
    shell.start_mcp();
    let port = shell
        .mcp_port()
        .ok_or("the MCP server did not start (is the port taken?)")?;
    println!("Under the Canopy editor: MCP on 127.0.0.1:{port}. Add it to Claude Code with:");
    println!("  {}", shell.claude_command());
    loop {
        shell.tick();
        std::thread::sleep(std::time::Duration::from_millis(15));
    }
}
