//! The egui debug overlay (renderer §6: egui is for debug and tuning only): frame rate,
//! CPU and per-pass GPU timings, the simulation's tick cost, what the computer players
//! are doing, and switches for render features. F9 (or Ctrl+F3, the old binding)
//! toggles it.

use canopy_render::{FrameReport, HazeQuality, Quality};
use std::sync::Arc;
use winit::{event::WindowEvent, window::Window};

/// What the overlay shows this frame.
#[derive(Default)]
pub struct Stats {
    pub fps: f32,
    pub frame_ms: f32,
    pub frame_max_ms: f32,
    pub report: FrameReport,
    pub live_ms: f32,
    pub ui_ms: f32,
    pub tick: u32,
    /// Mean step, view and present cost of the snapshots at hand, ms.
    pub tick_cost: [f32; 3],
    pub slowdowns: u32,
    pub decided: bool,
    pub late_frames: u32,
    pub units: u32,
    pub buildings: u32,
    pub selected: usize,
    pub ai: Option<Arc<Vec<canopy_sim::ai::Summary>>>,
    pub sent: Vec<String>,
}

pub struct Debug {
    pub open: bool,
    ctx: egui::Context,
    state: egui_winit::State,
    renderer: egui_wgpu::Renderer,
    jobs: Vec<egui::ClippedPrimitive>,
    screen: egui_wgpu::ScreenDescriptor,
}

impl Debug {
    /// An overlay drawing into `format` targets (the non-sRGB view of the surface).
    pub fn new(window: &Window, device: &wgpu::Device, format: wgpu::TextureFormat) -> Self {
        let ctx = egui::Context::default();
        let state = egui_winit::State::new(
            ctx.clone(),
            egui::ViewportId::ROOT,
            window,
            Some(window.scale_factor() as f32),
            None,
            None,
        );
        let renderer =
            egui_wgpu::Renderer::new(device, format, egui_wgpu::RendererOptions::default());
        Self {
            open: false,
            ctx,
            state,
            renderer,
            jobs: Vec::new(),
            screen: egui_wgpu::ScreenDescriptor {
                size_in_pixels: [1, 1],
                pixels_per_point: 1.0,
            },
        }
    }

    /// Feeds a window event; returns whether egui wants it (only while open).
    pub fn event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        if !self.open {
            return false;
        }
        self.state.on_window_event(window, event).consumed
    }

    /// Builds the overlay for this frame; returns new quality settings if a switch
    /// changed. `ai` asks the simulation for AI summaries while the section is open.
    pub fn run(
        &mut self,
        window: &Window,
        gpu: &canopy_render::Gpu,
        stats: &Stats,
        quality: Quality,
    ) -> (Option<Quality>, bool) {
        self.jobs.clear();
        if !self.open {
            return (None, false);
        }
        let input = self.state.take_egui_input(window);
        let mut changed = None;
        let mut wants_ai = false;
        let output = self.ctx.run_ui(input, |ui| {
            let ctx = ui.ctx().clone();
            egui::Window::new("Debug (F9)")
                .default_pos([12.0, 120.0])
                .default_width(360.0)
                .show(&ctx, |ui| {
                    ui.heading(format!("{:.0} fps", stats.fps));
                    ui.label(format!(
                        "frame {:.2} ms (max {:.1}) · render CPU {:.2} ms · live {:.2} ms · ui {:.2} ms",
                        stats.frame_ms, stats.frame_max_ms, stats.report.cpu_total_ms, stats.live_ms, stats.ui_ms
                    ));
                    ui.label(format!(
                        "draws {} · actors {} visible {} · shadow casters {}",
                        stats.report.draws,
                        stats.report.actors.instances,
                        stats.report.actors.visible,
                        stats.report.actors.shadow_casters
                    ));
                    egui::CollapsingHeader::new("Passes").default_open(true).show(ui, |ui| {
                        egui::Grid::new("passes").striped(true).show(ui, |ui| {
                            ui.label("pass");
                            ui.label("CPU ms");
                            ui.label("GPU ms");
                            ui.end_row();
                            for (i, name) in stats.report.passes.iter().enumerate() {
                                ui.label(*name);
                                ui.label(format!("{:.3}", stats.report.cpu_ms.get(i).copied().unwrap_or(0.0)));
                                ui.label(
                                    stats
                                        .report
                                        .gpu_ms
                                        .as_ref()
                                        .and_then(|g| g.get(i))
                                        .map_or("–".to_owned(), |ms| format!("{ms:.3}")),
                                );
                                ui.end_row();
                            }
                        });
                    });
                    egui::CollapsingHeader::new("Simulation").default_open(true).show(ui, |ui| {
                        let [step, view, present] = stats.tick_cost;
                        ui.label(format!(
                            "tick {} · step {step:.2} ms · view {view:.2} ms · present {present:.2} ms",
                            stats.tick
                        ));
                        ui.label(format!(
                            "slowdowns {} · late frames {} · units {} · buildings {} · selected {}{}",
                            stats.slowdowns,
                            stats.late_frames,
                            stats.units,
                            stats.buildings,
                            stats.selected,
                            if stats.decided { " · decided" } else { "" }
                        ));
                    });
                    egui::CollapsingHeader::new("Render features").default_open(true).show(ui, |ui| {
                        let mut q = quality;
                        ui.checkbox(&mut q.contact_shadows, "Contact shadows");
                        ui.checkbox(&mut q.bloom, "Bloom");
                        egui::ComboBox::from_label("Haze")
                            .selected_text(format!("{:?}", q.haze))
                            .show_ui(ui, |ui| {
                                for h in [HazeQuality::Off, HazeQuality::Low, HazeQuality::Medium, HazeQuality::High] {
                                    ui.selectable_value(&mut q.haze, h, format!("{h:?}"));
                                }
                            });
                        ui.add(
                            egui::Slider::new(&mut q.render_scale, canopy_render::quality::MIN_RENDER_SCALE..=1.0)
                                .text("Render scale"),
                        );
                        if q != quality {
                            changed = Some(q);
                        }
                    });
                    let ai = egui::CollapsingHeader::new("Computer players").show(ui, |ui| {
                        match &stats.ai {
                            None => {
                                ui.label("waiting for the simulation…");
                            }
                            Some(list) => {
                                for s in list.iter() {
                                    ui.label(format!(
                                        "P{} · {} · workers {} army {} · hero {} · {}",
                                        s.player + 1,
                                        s.mission,
                                        s.plan.workers,
                                        s.plan.army,
                                        s.plan.hero_level,
                                        s.plan.reason
                                    ));
                                    for t in s.trace.iter().rev().take(4) {
                                        ui.small(format!(
                                            "  {} {} {}{}",
                                            t.tick,
                                            t.action,
                                            t.reason,
                                            match t.accepted {
                                                Some(false) => " (refused)",
                                                _ => "",
                                            }
                                        ));
                                    }
                                }
                            }
                        }
                    });
                    wants_ai = ai.body_returned.is_some();
                    egui::CollapsingHeader::new("Commands sent").show(ui, |ui| {
                        for line in stats.sent.iter().rev().take(12) {
                            ui.small(line);
                        }
                    });
                });
        });
        self.state
            .handle_platform_output(window, output.platform_output);
        let size = window.inner_size();
        self.screen = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [size.width.max(1), size.height.max(1)],
            pixels_per_point: output.pixels_per_point,
        };
        self.jobs = self.ctx.tessellate(output.shapes, output.pixels_per_point);
        for (id, deltas) in &output.textures_delta.set {
            for delta in deltas {
                self.renderer
                    .update_texture(&gpu.device, &gpu.queue, *id, delta);
            }
        }
        let mut encoder = gpu
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("egui upload"),
            });
        let extra = self.renderer.update_buffers(
            &gpu.device,
            &gpu.queue,
            &mut encoder,
            &self.jobs,
            &self.screen,
        );
        gpu.queue
            .submit(extra.into_iter().chain([encoder.finish()]));
        for id in &output.textures_delta.free {
            self.renderer.free_texture(id);
        }
        (changed, wants_ai)
    }

    /// Records the overlay into `view` (call after the frame and the HUD).
    pub fn paint(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        if self.jobs.is_empty() {
            return;
        }
        let pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("debug"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Load,
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
