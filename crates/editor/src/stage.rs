//! The viewport (docs/editor.md): the game renderer draws a scene into a texture sized to
//! the panel, and egui shows it. One renderer serves whichever viewport shows: one model on
//! the zinc stage (the open element's look) turned with an orbit camera, a map's scene, or
//! an ability encounter's live scene (its presentations interpolated, effects and health
//! bars as the game draws them).

use canopy_assets::Pack;
use canopy_render::{CameraView, Gpu, Renderer, World, actors::ActorInstance};
use canopy_scene::effects::frame::EffectEvaluator;
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
    /// The animation clip playing (its index in the model and its length), or the rest
    /// pose.
    pub clip: Option<(u16, f32)>,
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

/// What the stage has loaded: one model, a map's scene (its map and version), or an
/// encounter's ground (its version).
#[derive(Debug, Clone, PartialEq)]
enum Loaded {
    Model(String, f32),
    Scene(String, u64),
    Encounter(u64),
}

/// An ability encounter to draw this frame (`crate::encounter`).
pub struct EncounterShot {
    /// The ground and the version it was built at.
    pub scene: Arc<canopy_scene::MapScene>,
    pub version: u64,
    /// The light: `None` for the zinc stage's, else a biome's.
    pub biome: Option<String>,
    pub effects: Arc<canopy_assets::EffectTable>,
    /// The encounter's content (the health bars' badge icons).
    pub content: Arc<canopy_content::Content>,
    /// The two presentations around the shown moment, `a` older; `t` blends them.
    pub a: Option<Arc<canopy_scene::live::Presentation>>,
    pub b: Arc<canopy_scene::live::Presentation>,
    pub t: f32,
    pub eye: Vec3,
    pub target: Vec3,
    /// Health bars, the selected caster, the aim while choosing a target.
    pub ui: canopy_scene::world_ui::WorldUiInput,
    pub size: [u32; 2],
}

/// The encounter's live drawing state (the game's `LiveScene`, cut down).
#[derive(Default)]
struct Live {
    /// The effect table the evaluator reads, and the evaluator.
    evaluator: Option<(Arc<canopy_assets::EffectTable>, EffectEvaluator)>,
    effects: canopy_render::effects::EffectFrame,
    instances: Vec<ActorInstance>,
    /// The models the renderer has, in the presenter's order.
    models: Arc<Vec<String>>,
    world_ui: Option<canopy_scene::world_ui::WorldUi>,
    world_ui_frame: canopy_render::world_ui::WorldUiFrame,
}

/// What the view draws over a map's scene this frame.
#[derive(Debug, Clone, Default)]
pub struct Overlay {
    /// Actors (by their index in the scene) standing elsewhere: a drag under way.
    pub moved: Vec<(usize, glam::Vec3)>,
    /// The ghost of what Place would put down: where (model space to world), its owner's
    /// colour, and whether it may stand there.
    pub ghost: Option<(glam::Mat4, Option<[f32; 3]>, bool)>,
}

/// A map's scene to draw this frame.
pub struct SceneShot {
    pub root: String,
    pub version: u64,
    pub scene: Arc<crate::map::Scene>,
    pub camera: crate::map::Camera,
    /// Ground height under the camera's focus.
    pub ground: f32,
    pub size: [u32; 2],
    pub overlay: Overlay,
}

pub struct Stage {
    renderer: Renderer,
    pack: Arc<Pack>,
    data: std::path::PathBuf,
    /// The zinc stage's lighting, and which lighting the renderer has ("stage" or a biome).
    stage_environment: canopy_render::Environment,
    environment: String,
    target: wgpu::Texture,
    view: wgpu::TextureView,
    size: [u32; 2],
    loaded: Option<Loaded>,
    texture: Option<egui::TextureId>,
    started: std::time::Instant,
    live: Live,
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
        let renderer = Renderer::new(gpu, FORMAT, size, environment.clone());
        let (target, view) = target(gpu, size);
        Ok(Self {
            renderer,
            pack,
            data: data.to_path_buf(),
            stage_environment: environment,
            environment: "stage".into(),
            target,
            view,
            size,
            loaded: None,
            texture: None,
            started: std::time::Instant::now(),
            live: Live::default(),
        })
    }

    /// The picture of the last [`Stage::draw`], for egui.
    pub fn texture(&self) -> Option<egui::TextureId> {
        self.texture
    }

    /// Sizes the target and the egui texture to `size`.
    fn fit(&mut self, gpu: &Gpu, egui: &mut egui_wgpu::Renderer, size: [u32; 2]) -> [u32; 2] {
        let size = [size[0].clamp(16, 4096), size[1].clamp(16, 4096)];
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
        size
    }

    /// Draws a map's scene in its biome's light.
    pub fn draw_scene(&mut self, gpu: &Gpu, egui: &mut egui_wgpu::Renderer, shot: &SceneShot) {
        let size = self.fit(gpu, egui, shot.size);
        self.leave_encounter();
        let scene = &shot.scene;
        self.light(gpu, Some(&scene.biome), scene.hour, scene.weather);
        let loaded = Loaded::Scene(shot.root.clone(), shot.version);
        if self.loaded.as_ref() != Some(&loaded) {
            if let Err(error) = self.renderer.load_world(gpu, &self.pack, &scene.world) {
                tracing::warn!(%error, map = %shot.root, "editor: map scene");
            }
            self.loaded = Some(loaded);
        }
        if shot.overlay.moved.is_empty() && shot.overlay.ghost.is_none() {
            self.renderer.set_actors(&scene.actors);
        } else {
            let mut actors = scene.actors.clone();
            for &(i, at) in &shot.overlay.moved {
                if let Some(actor) = actors.get_mut(i) {
                    actor.transform.w_axis = at.extend(1.0);
                }
            }
            if let (Some((model, _)), Some((transform, owner, fits))) =
                (scene.ghost, shot.overlay.ghost)
            {
                let mut ghost = canopy_render::actors::ActorInstance::rest(model, transform);
                ghost.owner_color = owner;
                ghost.opacity = 0.6;
                // Red where it may not stand.
                if !fits {
                    ghost.tint = [1.0, 0.35, 0.3, 1.0];
                }
                actors.push(ghost);
            }
            self.renderer.set_actors(&actors);
        }
        let (eye, target) = shot.camera.eye_target(shot.ground);
        let distance = shot.camera.distance.max(1.0);
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let camera = CameraView::look_at(
            eye,
            target,
            crate::map::view::FOV,
            aspect,
            distance * 0.02,
            distance * 6.0,
        );
        let seconds = self.started.elapsed().as_secs_f32();
        self.renderer
            .render(gpu, &camera, seconds, &self.view, None);
    }

    /// Clears what only an encounter draws (health bars) when another scene follows it.
    fn leave_encounter(&mut self) {
        if matches!(self.loaded, Some(Loaded::Encounter(_))) {
            self.renderer
                .set_world_ui(&mut canopy_render::world_ui::WorldUiFrame::default());
            self.loaded = None;
        }
    }

    /// Lights the stage as `biome` at `hour` under `weather`, or as the zinc stage.
    fn light(
        &mut self,
        gpu: &Gpu,
        biome: Option<&str>,
        hour: f32,
        weather: canopy_render::environment::Weather,
    ) {
        let Some(biome) = biome else {
            if self.environment != "stage" {
                self.renderer
                    .set_environment(gpu, self.stage_environment.clone());
                self.environment = "stage".into();
            }
            return;
        };
        let key = format!("{biome}@{hour}@{weather:?}");
        if self.environment == key {
            return;
        }
        match canopy_scene::Look::load(&self.data, biome) {
            Ok(look) => {
                let environment =
                    canopy_render::Environment::new(&look.lighting, &look.biome, hour, None)
                        .with_weather(weather);
                self.renderer.set_environment(gpu, environment);
                self.environment = key;
            }
            Err(error) => tracing::warn!(%error, %biome, "editor: biome lighting"),
        }
    }

    /// Draws an ability encounter: its ground, its units and effects between the two
    /// presentations, health bars and the aim.
    pub fn draw_encounter(
        &mut self,
        gpu: &Gpu,
        egui: &mut egui_wgpu::Renderer,
        shot: &EncounterShot,
    ) {
        let size = self.fit(gpu, egui, shot.size);
        let hour = shot.scene.hour;
        self.light(
            gpu,
            shot.biome.as_deref(),
            hour,
            canopy_render::environment::Weather::Clear,
        );
        let b = &shot.b;
        let live = &mut self.live;
        // A new presenter may list its models in another order: the world loads again.
        let loaded = Loaded::Encounter(shot.version);
        if self.loaded.as_ref() != Some(&loaded) || !b.models.starts_with(&live.models) {
            let mut world = shot.scene.world.clone();
            world.actor_models = b.models.to_vec();
            if let Err(error) = self.renderer.load_world(gpu, &self.pack, &world) {
                tracing::warn!(%error, "editor: encounter scene");
            }
            live.models = b.models.clone();
            self.loaded = Some(loaded);
        } else if b.models.len() > live.models.len() {
            self.renderer.sync_actor_models(gpu, &self.pack, &b.models);
            live.models = b.models.clone();
        }
        if live
            .evaluator
            .as_ref()
            .is_none_or(|(table, _)| !Arc::ptr_eq(table, &shot.effects))
        {
            let evaluator = EffectEvaluator::new(shot.effects.clone());
            self.renderer
                .load_effect_textures(gpu, &self.pack, evaluator.textures());
            self.renderer.load_effect_curves(gpu, evaluator.curves());
            live.evaluator = Some((shot.effects.clone(), evaluator));
        }
        let a = shot.a.as_deref();
        canopy_scene::interpolate::interpolate(a, b, shot.t, &mut live.instances);
        let tick = match a {
            Some(a) => f64::from(a.tick) + f64::from(shot.t) * f64::from(b.tick - a.tick),
            None => f64::from(b.tick),
        };
        if let Some((_, evaluator)) = &mut live.evaluator {
            evaluator.frame(
                a.map(|a| &a.effects),
                &b.effects,
                shot.t,
                tick,
                Some(shot.eye),
                &mut live.effects,
                &mut live.instances,
            );
            self.renderer.set_effects(&mut live.effects);
        }
        self.renderer.set_actors(&live.instances);
        let distance = shot.eye.distance(shot.target).max(1.0);
        let aspect = size[0] as f32 / size[1].max(1) as f32;
        let camera = CameraView::look_at(
            shot.eye,
            shot.target,
            crate::map::view::FOV,
            aspect,
            distance * 0.02,
            distance * 12.0,
        );
        let seconds = self.started.elapsed().as_secs_f32();
        // Health bars and circles where the frame draws the units.
        let ui = live.world_ui.get_or_insert_with(|| {
            canopy_scene::world_ui::WorldUi::new(
                &self.data,
                Some(&self.pack),
                &shot.content,
                shot.scene.colors.clone(),
            )
        });
        if ui.atlases_dirty {
            let (glyphs, icons) = ui.atlases(shot.ui.scale);
            self.renderer.set_world_ui_atlases(gpu, glyphs, icons);
        }
        let instances = &live.instances;
        let position = |key: u64| {
            b.actors
                .binary_search_by_key(&key, |a| a.key)
                .ok()
                .and_then(|i| instances.get(i))
                .map(|x| x.transform.w_axis.truncate())
        };
        ui.build(
            &b.bodies,
            &position,
            &shot.ui,
            &camera,
            self.renderer.size(),
            &shot.scene.water_field(),
            seconds,
            &mut live.world_ui_frame,
        );
        if ui.atlases_dirty {
            let (glyphs, icons) = ui.atlases(shot.ui.scale);
            self.renderer.set_world_ui_atlases(gpu, glyphs, icons);
        }
        self.renderer.set_world_ui(&mut live.world_ui_frame);
        self.renderer
            .render(gpu, &camera, seconds, &self.view, None);
    }

    /// Draws `shot` into the viewport's texture.
    pub fn draw(&mut self, gpu: &Gpu, egui: &mut egui_wgpu::Renderer, shot: &Shot) {
        let size = self.fit(gpu, egui, shot.size);
        self.leave_encounter();
        self.light(gpu, None, 12.0, canopy_render::environment::Weather::Clear);
        if !matches!(&self.loaded, Some(Loaded::Model(m, _)) if *m == shot.model) {
            let world = World {
                terrain: None,
                statics: Vec::new(),
                glows: Default::default(),
                point_lights: Vec::new(),
                actor_models: vec![shot.model.clone()],
                water: None,
                water_textures: Default::default(),
                forest: None,
                blockout: None,
                dressing: None,
            };
            if let Err(error) = self.renderer.load_world(gpu, &self.pack, &world) {
                tracing::warn!(%error, model = %shot.model, "editor: viewport model");
            }
            let height = canopy_scene::models::ModelInfo::read(&self.pack, &shot.model).height();
            self.loaded = Some(Loaded::Model(shot.model.clone(), height.max(0.1)));
        }
        let height = match &self.loaded {
            Some(Loaded::Model(_, h)) => *h,
            _ => 1.0,
        } * shot.scale;
        let mut actor = ActorInstance::rest(0, Mat4::from_scale(Vec3::splat(shot.scale)));
        if let Some((clip, duration)) = shot.clip {
            actor.poses[0] = canopy_render::actors::Pose {
                clip,
                time: self.started.elapsed().as_secs_f32() % duration.max(0.05),
                weight: 1.0,
            };
            actor.pose_count = 1;
        }
        self.renderer.set_actors(&[actor]);
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
