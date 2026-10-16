//! The editor (docs/editor.md): one native editor for maps, units and heroes, abilities,
//! effects, items and buildings, races and rules, and assets. The game opens it as a mode
//! (the main menu's Editor entry, `canopy edit`); until then nothing of it exists, so
//! players pay nothing for it.
//!
//! [`Editor`] owns its egui context and draws the whole window: the game hands it window
//! events while it is open, calls [`Editor::run`] once a frame and [`Editor::paint`] to
//! record the frame. The look is [`theme`] (Tailwind's zinc, as the old spell studio).

mod icons;
mod library;
mod shell;
pub mod theme;

pub use library::Kind;
use std::path::PathBuf;
use winit::{event::WindowEvent, window::Window};

/// What the editor needs from the game.
pub struct Setup {
    /// The base package's folder.
    pub base: PathBuf,
    /// Font files for the interface, first choice first (egui's own stay as fallbacks).
    pub fonts: Vec<Vec<u8>>,
}

/// What the editor asks of the game after a frame.
#[derive(Debug, Default)]
pub struct Output {
    /// Back to the game's menus.
    pub close: bool,
}

pub struct Editor {
    ctx: egui::Context,
    /// Window input; `None` for an editor rendered offscreen ([`Editor::headless`]).
    state: Option<egui_winit::State>,
    renderer: egui_wgpu::Renderer,
    jobs: Vec<egui::ClippedPrimitive>,
    screen: egui_wgpu::ScreenDescriptor,
    shell: shell::Shell,
}

impl Editor {
    /// An editor in `window`, drawing into `format` targets (the non-sRGB view of the
    /// surface).
    pub fn new(
        window: &Window,
        device: &wgpu::Device,
        format: wgpu::TextureFormat,
        setup: Setup,
    ) -> Self {
        let mut editor = Self::headless(device, format, setup);
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
    pub fn headless(device: &wgpu::Device, format: wgpu::TextureFormat, setup: Setup) -> Self {
        let ctx = egui::Context::default();
        theme::apply(&ctx, &setup.fonts);
        let renderer =
            egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());
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
            shell: shell::Shell::new(package),
        }
    }

    /// Feeds a window event.
    pub fn event(&mut self, window: &Window, event: &WindowEvent) {
        if let Some(state) = &mut self.state {
            let _ = state.on_window_event(window, event);
        }
    }

    /// Builds this frame's interface and uploads what it draws.
    pub fn run(&mut self, window: &Window, device: &wgpu::Device, queue: &wgpu::Queue) -> Output {
        let Some(state) = &mut self.state else {
            return Output::default();
        };
        let input = state.take_egui_input(window);
        let size = window.inner_size();
        let (output, platform) = self.frame(input, [size.width, size.height], device, queue);
        if let Some(state) = &mut self.state {
            state.handle_platform_output(window, platform);
        }
        output
    }

    /// A frame of `size` physical pixels at `scale` without a window (captures).
    pub fn run_headless(
        &mut self,
        size: [u32; 2],
        scale: f32,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Output {
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
        self.frame(input, size, device, queue).0
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
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> (Output, egui::PlatformOutput) {
        let shell = &mut self.shell;
        let output = self.ctx.run_ui(input, |ui| shell.ui(ui));
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
        let output = Output {
            close: std::mem::take(&mut self.shell.close),
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
