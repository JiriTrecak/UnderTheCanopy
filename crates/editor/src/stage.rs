//! The viewport (docs/editor.md): the game renderer draws a scene into a texture sized to
//! the panel, and egui shows it. One renderer serves whichever viewport shows. For now the
//! scene is one model on the zinc stage (the open element's look), turned with an orbit
//! camera; the map editor and the ability stage build on it.

use canopy_assets::Pack;
use canopy_render::{CameraView, Gpu, Renderer, World, actors::ActorInstance};
use glam::{Mat4, Vec3};
use std::path::Path;
use std::sync::Arc;

/// The zinc stage behind previews (`#18181b`, linear).
const STAGE: Vec3 = Vec3::new(0.0091, 0.0091, 0.0109);
const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;

/// What a viewport wants drawn this frame.
#[derive(Debug, Clone, PartialEq)]
pub struct Shot {
    /// The model's pack entry.
    pub model: String,
    /// Its uniform scale (the element's look).
    pub scale: f32,
    /// Physical pixels.
    pub size: [u32; 2],
    pub orbit: Orbit,
}

/// An orbit camera around the model: angles in radians, distance in model heights.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
}

impl Default for Orbit {
    fn default() -> Self {
        Self {
            yaw: 0.6,
            pitch: 0.45,
            distance: 2.4,
        }
    }
}

pub struct Stage {
    renderer: Renderer,
    pack: Arc<Pack>,
    target: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
    /// The model the renderer has loaded, and its rest height (model metres).
    loaded: Option<(String, f32)>,
    texture: Option<egui::TextureId>,
    started: std::time::Instant,
}

fn target(gpu: &Gpu, size: [u32; 2]) -> (wgpu::Texture, wgpu::TextureView) {
    let texture = gpu.device.create_texture(&wgpu::TextureDescriptor {
        label: Some("editor viewport"),
        size: wgpu::Extent3d {
            width: size[0],
            height: size[1],
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    let view = texture.create_view(&wgpu::TextureViewDescriptor::default());
    (texture, view)
}

impl Stage {
    /// A stage for the data folder `data` (its cooked pack and lighting).
    pub fn new(gpu: &Gpu, data: &Path) -> Result<Self, String> {
        let pack_path =
            canopy_assets::find_pack(data).ok_or("no cooked asset pack (run `just cook`)")?;
        let pack = Arc::new(Pack::open(&pack_path).map_err(|e| e.to_string())?);
        let look = canopy_scene::Look::load(data, canopy_content::map::BIOMES[0])
            .map_err(|e| e.to_string())?;
        let mut environment =
            canopy_render::Environment::new(&look.lighting, &look.biome, 12.0, None);
        environment.lighting.background = STAGE;
        let size = [256, 256];
        let renderer = Renderer::new(gpu, FORMAT, size, environment);
        let (target, view) = target(gpu, size);
        Ok(Self {
            renderer,
            pack,
            target,
            view,
            size,
            loaded: None,
            texture: None,
            started: std::time::Instant::now(),
        })
    }

    /// The picture of the last [`Stage::draw`], for egui.
    pub fn texture(&self) -> Option<egui::TextureId> {
        self.texture
    }

    /// Draws `shot` into the viewport's texture.
    pub fn draw(&mut self, gpu: &Gpu, egui: &mut egui_wgpu::Renderer, shot: &Shot) {
        let size = [shot.size[0].clamp(16, 4096), shot.size[1].clamp(16, 4096)];
        if size != self.size || self.texture.is_none() {
            self.size = size;
            (self.target, self.view) = target(gpu, size);
            self.renderer.resize(gpu, size);
            match self.texture {
                Some(id) => egui.update_egui_texture_from_wgpu_texture(
                    &gpu.device,
                    &self.view,
                    wgpu::FilterMode::Linear,
                    id,
                ),
                None => {
                    self.texture = Some(egui.register_native_texture(
                        &gpu.device,
                        &self.view,
                        wgpu::FilterMode::Linear,
                    ))
                }
            }
        }
        if self.loaded.as_ref().map(|(m, _)| m) != Some(&shot.model) {
            let world = World {
                terrain: None,
                statics: Vec::new(),
                glows: Default::default(),
                point_lights: Vec::new(),
                actor_models: vec![shot.model.clone()],
                water: None,
                water_textures: Default::default(),
                forest: None,
            };
            if let Err(error) = self.renderer.load_world(gpu, &self.pack, &world) {
                tracing::warn!(%error, model = %shot.model, "editor: viewport model");
            }
            let height = canopy_scene::models::ModelInfo::read(&self.pack, &shot.model).height();
            self.loaded = Some((shot.model.clone(), height.max(0.1)));
        }
        let height = self.loaded.as_ref().map_or(1.0, |(_, h)| *h) * shot.scale;
        self.renderer.set_actors(&[ActorInstance::rest(
            0,
            Mat4::from_scale(Vec3::splat(shot.scale)),
        )]);
        let target = Vec3::new(0.0, height * 0.45, 0.0);
        let o = shot.orbit;
        let distance = height.max(0.5) * o.distance;
        let eye = target
            + distance
                * Vec3::new(
                    o.yaw.sin() * o.pitch.cos(),
                    o.pitch.sin(),
                    o.yaw.cos() * o.pitch.cos(),
                );
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let camera = CameraView::look_at(eye, target, 0.6, aspect, distance * 0.05, distance * 8.0);
        let seconds = self.started.elapsed().as_secs_f32();
        self.renderer
            .render(gpu, &camera, seconds, &self.view, None);
    }
}
