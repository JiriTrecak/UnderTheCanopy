//! The ability stage (docs/editor.md, M2): an ability cast on test targets in the editor's
//! own viewport with the real simulation (`canopy_sim::encounter`), as the old spell
//! studio's encounter stage. The simulation runs on the editor's thread (a few units cost
//! little) and is presented every tick; the viewport interpolates between the last two
//! presentations. Here are the settings, the transport, the timeline and the events; the
//! stage draws the scene ([`crate::stage::EncounterShot`]).

use crate::theme::*;
use crate::widgets::{eyebrow, primary};
use canopy_assets::Pack;
use canopy_content::{Content, abilities::TargetingKind};
use canopy_render::terrain::{LayerKind, TerrainDesc};
use canopy_scene::{
    MapScene,
    live::{Presentation, Presenter},
    world_ui::{AimMark, HealthBars, WorldUiInput},
};
use canopy_sim::encounter::{
    self as sim, Aim, Fixture, Intervention, PRESETS, Preset, Relation, Settings, Span, SpanKind,
    Take,
};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use glam::{Mat4, Vec3, Vec4};
use std::sync::Arc;

/// Ticks a second.
const TICKS: f32 = 40.0;
/// Playback speeds.
const SPEEDS: [(f32, &str); 4] = [(0.25, "¼"), (0.5, "½"), (1.0, "1×"), (2.0, "2×")];
/// The zinc stage's ground, linear (its texture's average is tinted to this).
const GROUND: f32 = 0.05;

/// An orbiting camera over the encounter.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Camera {
    focus: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
}

impl Default for Camera {
    fn default() -> Self {
        let [x, z] = sim::ORIGIN;
        Self {
            focus: Vec3::new(x as f32 + 4.0, 2.0, z as f32),
            distance: 30.0,
            yaw: 0.0,
            pitch: 0.9,
        }
    }
}

impl Camera {
    fn eye(&self) -> Vec3 {
        self.focus
            + self.distance
                * Vec3::new(
                    self.yaw.sin() * self.pitch.cos(),
                    self.pitch.sin(),
                    self.yaw.cos() * self.pitch.cos(),
                )
    }

    /// View and projection for picking (the renderer's own is reverse-Z, same frustum).
    fn matrix(&self, aspect: f32) -> Mat4 {
        let d = self.distance.max(1.0);
        Mat4::perspective_rh(crate::map::view::FOV, aspect.max(0.1), d * 0.02, d * 12.0)
            * Mat4::look_at_rh(self.eye(), self.focus, Vec3::Y)
    }
}

/// The lower panel's page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Timeline,
    Events,
}

/// The next ground version (shared by every stage).
static NEXT_VERSION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// One ability's stage.
pub struct Session {
    pub ability: String,
    settings: Option<Settings>,
    /// The ground's biome; the zinc stage when `None`.
    biome: Option<String>,
    /// The content and settings the fixture was built from.
    built: Option<(Arc<Content>, Settings)>,
    fixture: Option<Arc<Fixture>>,
    /// The ground, the biome it was built for and its version.
    scene: Option<(Option<String>, Arc<MapScene>)>,
    version: u64,
    pack: Option<Arc<Pack>>,
    take: Option<Take>,
    presenter: Option<Presenter>,
    /// The two latest presentations, older first.
    shown: [Option<Arc<Presentation>>; 2],
    playing: bool,
    speed: f32,
    /// Ticks owed to playback (its fraction blends the presentations).
    owed: f32,
    /// Choosing a target in the viewport.
    targeting: bool,
    /// What a click would aim at, under the pointer.
    hover: Option<Aim>,
    camera: Camera,
    /// The camera follows the units until the user moves it.
    fit: bool,
    page: Page,
    error: Option<String>,
    /// Commands from outside the panel (captures, MCP), run at the next frame.
    queued: Vec<Command>,
    /// A caster asked for before the settings exist (the Overview's Stage link).
    caster: Option<String>,
}

/// What can be asked of the stage from outside its panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    /// Cast at the first target (or its place, or the caster).
    Cast,
    Seek(u32),
}

/// What the stage asks of the shell this frame.
#[derive(Default)]
pub struct Out {
    /// The viewport's size in physical pixels.
    pub size: Option<[u32; 2]>,
}

impl Session {
    pub fn new(ability: &str) -> Self {
        Self {
            ability: ability.to_string(),
            settings: None,
            biome: None,
            built: None,
            fixture: None,
            scene: None,
            version: NEXT_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            pack: None,
            take: None,
            presenter: None,
            shown: [None, None],
            playing: false,
            speed: 1.0,
            owed: 0.0,
            targeting: false,
            hover: None,
            camera: Camera::default(),
            fit: true,
            page: Page::Timeline,
            error: None,
            queued: Vec::new(),
            caster: None,
        }
    }

    /// Stands `unit` up as the caster.
    pub fn set_caster(&mut self, unit: &str) {
        match &mut self.settings {
            Some(settings) => settings.caster = unit.to_string(),
            None => self.caster = Some(unit.to_string()),
        }
    }

    /// The biome whose look the ground is built from (the zinc stage borrows the first
    /// biome's rock).
    pub fn look_biome(&self) -> &str {
        self.biome
            .as_deref()
            .unwrap_or(canopy_content::map::BIOMES[0])
    }

    pub fn queue(&mut self, command: Command) {
        self.queued.push(command);
    }

    /// Builds what is out of date: the fixture when the content (an edit) or the
    /// settings changed, the ground when its biome changed, and a take to show. An edit
    /// replays the take to the same tick, so a change shows at the same moment.
    pub fn prepare(&mut self, content: &Arc<Content>, pack: &Arc<Pack>, look: &canopy_scene::Look) {
        self.pack = Some(pack.clone());
        let wanted = self.caster.take();
        let settings = self.settings.get_or_insert_with(|| {
            let mut s = sim::suggest(content, &self.ability);
            if let Some(caster) = wanted {
                s.caster = caster;
            }
            s
        });
        // An ability that lost ranks is cast (and shown) at its highest.
        if let Some(ranks) = content
            .abilities
            .abilities
            .iter()
            .find(|a| a.id.as_str() == self.ability)
            .map(|a| a.ranks.len().max(1) as u32)
        {
            settings.rank = settings.rank.clamp(1, ranks);
        }
        let settings = settings.clone();
        let stale = self
            .built
            .as_ref()
            .is_none_or(|(c, s)| !Arc::ptr_eq(c, content) || *s != settings);
        if stale {
            let edit_only = self.built.as_ref().is_some_and(|(_, s)| *s == settings);
            self.built = Some((content.clone(), settings.clone()));
            let resume = self
                .take
                .as_ref()
                .filter(|_| edit_only)
                .map(|t| (t.aim(), t.tick(), t.interventions().to_vec()));
            match Fixture::new(content, &self.ability, &settings) {
                Ok(fixture) => {
                    let fixture = Arc::new(fixture);
                    self.fixture = Some(fixture.clone());
                    self.error = None;
                    let tick = resume.as_ref().map(|r| r.1);
                    let take = match resume {
                        Some((aim, _, interventions)) => Take::replay(fixture, aim, interventions),
                        None => Take::new(fixture, None),
                    };
                    match take {
                        Ok(take) => {
                            self.begin(take);
                            if let Some(tick) = tick {
                                self.seek(tick);
                            } else {
                                self.playing = false;
                                self.targeting = false;
                            }
                        }
                        Err(error) => self.fail(error),
                    }
                }
                Err(error) => self.fail(error),
            }
        }
        let wanted = self.biome.clone();
        if self.scene.as_ref().is_none_or(|(b, _)| *b != wanted)
            && let (Some(fixture), Some(take)) = (&self.fixture, &self.take)
        {
            match ground(fixture, take, pack, look, wanted.is_none()) {
                Ok(scene) => {
                    self.scene = Some((wanted, Arc::new(scene)));
                    // Versions are unique across stages: one renderer draws them all, and
                    // tells the grounds apart by it.
                    self.version = NEXT_VERSION.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    // The presenter takes the scene's player colours.
                    if let Some(take) = &self.take
                        && let Ok(fresh) = take.restart()
                    {
                        let tick = take.tick();
                        self.begin(fresh);
                        self.seek(tick);
                    }
                }
                Err(error) => self.error = Some(error),
            }
        }
    }

    fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.fixture = None;
        self.take = None;
        self.presenter = None;
        self.shown = [None, None];
        self.playing = false;
        self.targeting = false;
    }

    /// Shows `take` from its start: a presenter of its own, its first presentation.
    fn begin(&mut self, take: Take) {
        self.owed = 0.0;
        self.shown = [None, None];
        let (Some(pack), Some(fixture)) = (&self.pack, &self.fixture) else {
            self.take = Some(take);
            return;
        };
        let colors = self
            .scene
            .as_ref()
            .map(|(_, s)| s.colors.clone())
            .unwrap_or_default();
        self.presenter = Some(Presenter::new(
            take.world(),
            &fixture.map,
            Some(pack.clone()),
            canopy_scene::live::Setup {
                colors,
                tree_slots: Default::default(),
                statics: 0,
            },
        ));
        self.take = Some(take);
        self.present();
    }

    fn present(&mut self) {
        let (Some(take), Some(presenter)) = (&self.take, &mut self.presenter) else {
            return;
        };
        let world = take.world();
        let view = world.observer_view();
        let presentation = presenter.update(world, &view, &|id| {
            world
                .state()
                .entities
                .get(id)
                .and_then(|e| e.unit.as_ref())
                .is_some_and(|u| u.contained.is_some() || u.release.is_some())
        });
        self.shown[0] = self.shown[1].take();
        self.shown[1] = Some(Arc::new(presentation));
    }

    /// One tick of the take, presented.
    fn step(&mut self) {
        if let Some(take) = &mut self.take {
            take.step();
            self.present();
        }
    }

    /// The take at `tick`: forward by stepping, backward by replaying from its start.
    fn seek(&mut self, tick: u32) {
        let Some(take) = &self.take else { return };
        let _span = tracing::trace_span!("encounter_seek").entered();
        let started = std::time::Instant::now();
        let tick = tick.min(take.fixture().duration);
        let back = tick < take.tick();
        if back {
            match take.restart() {
                Ok(fresh) => self.begin(fresh),
                Err(error) => return self.fail(error),
            }
        }
        while self.take.as_ref().is_some_and(|t| t.tick() < tick) {
            self.step();
        }
        self.owed = 0.0;
        if back {
            tracing::debug!(
                tick,
                ms = started.elapsed().as_secs_f32() * 1000.0,
                "editor: encounter replayed"
            );
        }
    }

    /// Starts a take casting at `aim` (or, for passives, just running) and plays it.
    fn fire(&mut self, aim: Option<Aim>) {
        let Some(fixture) = self.fixture.clone() else {
            return;
        };
        self.targeting = false;
        self.hover = None;
        match Take::new(fixture, aim) {
            Ok(take) => {
                self.begin(take);
                self.playing = true;
            }
            Err(error) => self.fail(error),
        }
    }

    /// Cast: self spells and passives at once, the rest after a target is chosen.
    fn cast(&mut self) {
        let Some(fixture) = &self.fixture else { return };
        let ability = &fixture.ability;
        if ability.activation == canopy_content::abilities::Activation::Passive {
            self.fire(None);
        } else if ability.targeting.kind == TargetingKind::Caster {
            self.fire(Some(Aim::Caster));
        } else if self.targeting {
            // Cast again while choosing: the first target.
            let aim = self.take.as_ref().and_then(Take::first_aim);
            self.fire(aim);
        } else {
            self.targeting = true;
            self.playing = false;
        }
    }

    fn reset(&mut self) {
        self.targeting = false;
        if let Some(fixture) = self.fixture.clone() {
            match Take::new(fixture, None) {
                Ok(take) => self.begin(take),
                Err(error) => self.fail(error),
            }
        }
        self.playing = false;
    }

    fn toggle(&mut self) {
        if self.take.as_ref().is_some_and(Take::done) {
            self.seek(0);
            self.playing = true;
        } else {
            self.playing = !self.playing;
        }
    }

    fn interrupt(&mut self) {
        if let Some(take) = &mut self.take
            && take.aim().is_some()
            && !take.done()
        {
            take.intervene(Intervention::Interrupt);
        }
    }

    /// Plays `dt` seconds of the take.
    fn advance(&mut self, dt: f32) {
        if !self.playing {
            return;
        }
        self.owed += dt.min(0.1) * TICKS * self.speed;
        while self.owed >= 1.0 {
            if self.take.as_ref().is_none_or(Take::done) {
                self.playing = false;
                self.owed = 0.0;
                return;
            }
            self.step();
            self.owed -= 1.0;
        }
    }

    /// The camera's view of the units: their bounds, from a little above the game's
    /// angle.
    fn frame_units(&mut self, aspect: f32) {
        let Some(b) = &self.shown[1] else { return };
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for body in b.bodies.iter().filter(|b| b.hp != Some(0)) {
            let r = body.radius.max(0.5);
            lo = lo.min(body.position - Vec3::new(r, 0.0, r));
            hi = hi.max(body.position + Vec3::new(r, body.height, r));
        }
        if lo.x > hi.x {
            return;
        }
        let centre = (lo + hi) * 0.5;
        let across = (hi.x - lo.x)
            .max((hi.z - lo.z) * aspect.max(0.5))
            .max(hi.y - lo.y);
        let half = (crate::map::view::FOV * 0.5).tan();
        self.camera.focus = Vec3::new(centre.x, lo.y, centre.z);
        self.camera.distance = (across / aspect.clamp(0.5, 3.0) / half * 0.85 + 8.0).max(12.0);
    }

    /// The shot the stage draws this frame.
    pub fn shot(&self, size: [u32; 2]) -> Option<crate::stage::EncounterShot> {
        let (biome, scene) = self.scene.as_ref()?;
        let b = self.shown[1].clone()?;
        let presenter = self.presenter.as_ref()?;
        let fixture = self.fixture.as_ref()?;
        let take = self.take.as_ref()?;
        let t = if self.playing {
            self.owed.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let caster = take.caster().0;
        let mut ui = WorldUiInput {
            selected: vec![caster],
            health: HealthBars::Always,
            scale: 1.0,
            ..Default::default()
        };
        if self.targeting {
            ui.hovered = match self.hover {
                Some(Aim::Unit { entity }) => Some(entity),
                _ => None,
            };
            ui.aim = self.aim_mark(fixture, &b, caster);
        }
        Some(crate::stage::EncounterShot {
            scene: scene.clone(),
            version: self.version,
            biome: biome.clone(),
            effects: presenter.effects.effect_table().clone(),
            content: fixture.content.clone(),
            a: self.shown[0].clone(),
            b,
            t,
            eye: self.camera.eye(),
            target: self.camera.focus,
            ui,
            size,
        })
    }

    /// The aim ring and range while choosing a target.
    fn aim_mark(&self, fixture: &Fixture, b: &Presentation, caster: u32) -> Option<AimMark> {
        let ability = &fixture.ability;
        let rank = ability.ranks.get(fixture.rank as usize - 1)?;
        let origin = b.bodies.iter().find(|x| x.id == caster)?.position;
        let point = match self.hover {
            Some(Aim::Unit { entity }) => b.bodies.iter().find(|x| x.id == entity)?.position,
            Some(Aim::Point { x, y }) => Vec3::new(x as f32, 0.0, y as f32),
            _ => return None,
        };
        let range = ability.targeting.range.at(rank).unwrap_or(0) as f32;
        let radius = ability
            .targeting
            .radius
            .as_ref()
            .and_then(|r| r.at(rank))
            .unwrap_or(0) as f32;
        let line = match &ability.delivery {
            Some(canopy_content::abilities::Delivery::Line { length, width, .. }) => {
                Some((*length as f32, *width as f32 / 2.0))
            }
            _ => None,
        };
        let dx = point.x - origin.x;
        let dz = point.z - origin.z;
        Some(AimMark {
            origin: [origin.x, origin.z],
            point: [point.x, point.z],
            radius,
            line,
            range,
            valid: (dx * dx + dz * dz).sqrt() <= range + 0.5,
        })
    }

    /// What a click at `p` in `rect` aims at: the nearest unit body to the pointer, or
    /// the ground under it for point spells.
    fn pick(&self, rect: Rect, p: Pos2) -> Option<Aim> {
        let fixture = self.fixture.as_ref()?;
        let b = self.shown[1].as_ref()?;
        let aspect = rect.width() / rect.height().max(1.0);
        let matrix = self.camera.matrix(aspect);
        let ndc = vec2(
            (p.x - rect.left()) / rect.width() * 2.0 - 1.0,
            1.0 - (p.y - rect.top()) / rect.height() * 2.0,
        );
        let inverse = matrix.inverse();
        let far = inverse * Vec4::new(ndc.x, ndc.y, 1.0, 1.0);
        let near = inverse * Vec4::new(ndc.x, ndc.y, -1.0, 1.0);
        let (far, near) = (far.truncate() / far.w, near.truncate() / near.w);
        let direction = (far - near).normalize_or_zero();
        let floor = self.camera.focus.y;
        let ground = (direction.y < -1e-4).then(|| {
            let t = (floor - near.y) / direction.y;
            near + direction * t
        });
        match fixture.ability.targeting.kind {
            TargetingKind::Point => {
                let g = ground?;
                Some(Aim::Point {
                    x: g.x.round().clamp(0.0, sim::SIZE as f32 - 1.0) as u32,
                    y: g.z.round().clamp(0.0, sim::SIZE as f32 - 1.0) as u32,
                })
            }
            TargetingKind::Unit => {
                // Bodies whose upright column the pointer's ray passes nearest.
                let mut best: Option<(f32, u32)> = None;
                for body in b.bodies.iter().filter(|x| x.hp.is_some_and(|hp| hp > 0)) {
                    let mid = body.position + Vec3::Y * body.height * 0.5;
                    let along = (mid - near).dot(direction).max(0.0);
                    let miss = (near + direction * along - mid).length();
                    let reach = body.radius.max(0.6) + body.height * 0.3;
                    if miss <= reach && best.is_none_or(|(d, _)| miss < d) {
                        best = Some((miss, body.id));
                    }
                }
                best.map(|(_, entity)| Aim::Unit { entity })
            }
            TargetingKind::Caster => Some(Aim::Caster),
        }
    }
}

/// The ground of an encounter: the sandbox map drawn as the game does, or on the zinc
/// stage (`zinc`) in one neutral material (the biome's rock, tinted so it averages zinc)
/// without the forest around it.
fn ground(
    fixture: &Fixture,
    take: &Take,
    pack: &Pack,
    look: &canopy_scene::Look,
    zinc: bool,
) -> Result<MapScene, String> {
    if !zinc {
        return canopy_scene::build(take.world(), &fixture.map, pack, look)
            .map_err(|e| e.to_string());
    }
    let mut map = fixture.map.clone();
    let terrain = &mut map.authoring.terrain;
    let floats = |value: f32, like: &canopy_content::map::grid::Grid| {
        canopy_content::map::grid::Grid(
            std::iter::repeat_n(value.to_le_bytes(), like.0.len() / 4)
                .flatten()
                .collect(),
        )
    };
    terrain.rock = floats(1.0, &terrain.rock);
    terrain.grass = floats(0.0, &terrain.grass);
    let mut scene =
        canopy_scene::build(take.world(), &map, pack, look).map_err(|e| e.to_string())?;
    scene.world.forest = None;
    if let Some(desc) = &mut scene.world.terrain {
        neutral(desc, pack);
    }
    Ok(scene)
}

/// Makes `desc` one material: the layer that covers the most ground becomes the base,
/// tinted so its texture averages a dark neutral grey.
fn neutral(desc: &mut TerrainDesc, pack: &Pack) {
    let main = desc
        .layers
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, l)| matches!(l.kind, LayerKind::Tiled { .. }))
        .max_by_key(|(_, l)| l.mask.iter().map(|&w| u64::from(w)).sum::<u64>())
        .map(|(i, _)| i);
    if let Some(i) = main {
        let layer = desc.layers[i].clone();
        let base = &mut desc.layers[0];
        base.albedo = layer.albedo;
        base.normal = layer.normal;
        base.occlusion = layer.occlusion;
        if let LayerKind::Tiled { tiling, .. } = layer.kind {
            base.kind = LayerKind::Base { tiling };
        }
    }
    let base = &mut desc.layers[0];
    base.tint = average(pack, &base.albedo).map_or([0.35; 3], |mean| {
        [0, 1, 2].map(|c| GROUND / mean[c].max(0.005))
    });
    base.painted = true;
    desc.layers.truncate(1);
    desc.surroundings = None;
}

/// The average linear colour of a texture (its smallest level).
fn average(pack: &Pack, name: &str) -> Option<[f32; 3]> {
    let texture = pack.texture(name).ok()?;
    let level = texture.meta.mips.len().checked_sub(1)?;
    let texels = texture.decode_level(level);
    let linear = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let n = (texels.len() / 4).max(1) as f32;
    let mut sum = [0.0f32; 3];
    for px in texels.chunks_exact(4) {
        for c in 0..3 {
            sum[c] += linear(px[c]);
        }
    }
    Some(sum.map(|s| s / n))
}

fn span_color(kind: SpanKind) -> Color32 {
    match kind {
        SpanKind::Turn => hex(0x3f3f46),
        SpanKind::Prepare => hex(0x4f46e5).gamma_multiply(0.75),
        SpanKind::Channel => hex(0x0284c7).gamma_multiply(0.75),
        SpanKind::Recover => hex(0x52525b),
        SpanKind::Cancelled => hex(0xdc2626).gamma_multiply(0.8),
        SpanKind::Impact => hex(0xd97706).gamma_multiply(0.8),
        SpanKind::Status => hex(0x059669).gamma_multiply(0.75),
    }
}

const fn hex(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

fn seconds(ticks: u32) -> String {
    format!("{:.2} s", ticks as f32 / TICKS)
}

/// The settings panel. Returns whether anything changed.
fn settings_panel(ui: &mut Ui, session: &mut Session, content: Option<&Content>) {
    let Some(content) = content else {
        ui.label(
            RichText::new("Waiting for the check…")
                .color(FAINT)
                .size(11.0),
        );
        return;
    };
    let ranks = session
        .fixture
        .as_ref()
        .map_or(1, |f| f.ability.ranks.len().max(1));
    let Some(settings) = &mut session.settings else {
        return;
    };
    let mut s = settings.clone();
    ui.label(eyebrow("Scene"));
    let current = Preset::of(&s).map_or("Custom", |p| p.name);
    egui::ComboBox::from_id_salt(("encounter-preset", &session.ability))
        .selected_text(current)
        .width(196.0)
        .show_ui(ui, |ui| {
            for preset in &PRESETS {
                if ui
                    .selectable_label(current == preset.name, preset.name)
                    .clicked()
                {
                    s = preset.apply(&s);
                }
            }
        });
    ui.add_space(6.0);
    let units = sim::units(content);
    let name = |id: &str| {
        content
            .definition(id)
            .map_or(id.to_string(), |d| d.name.clone())
    };
    let unit_picker = |ui: &mut Ui, label: &str, value: &mut String| {
        ui.label(RichText::new(label).color(MUTED).size(11.0));
        egui::ComboBox::from_id_salt(("encounter-unit", label, &session.ability))
            .selected_text(name(value))
            .width(196.0)
            .height(320.0)
            .show_ui(ui, |ui| {
                for d in &units {
                    let id = d.id.to_string();
                    ui.selectable_value(value, id.clone(), &d.name)
                        .on_hover_text(&id);
                }
            });
    };
    unit_picker(ui, "Caster", &mut s.caster);
    unit_picker(ui, "Targets", &mut s.target);
    ui.label(RichText::new("Relation").color(MUTED).size(11.0));
    egui::ComboBox::from_id_salt(("encounter-relation", &session.ability))
        .selected_text(s.relation.name())
        .width(196.0)
        .show_ui(ui, |ui| {
            for r in Relation::ALL {
                ui.selectable_value(&mut s.relation, r, r.name());
            }
        });
    ui.add_space(4.0);
    ui.add_enabled(
        s.relation != Relation::Caster,
        egui::Slider::new(&mut s.targets, 0..=sim::MAX_TARGETS).text("Count"),
    );
    ui.add(
        egui::Slider::new(&mut s.distance, 1.0..=30.0)
            .step_by(0.5)
            .text("Distance"),
    );
    ui.add(
        egui::Slider::new(&mut s.spacing, 1.0..=8.0)
            .step_by(0.5)
            .text("Spacing"),
    );
    ui.add_space(6.0);
    ui.label(eyebrow("Cast"));
    egui::ComboBox::from_id_salt(("encounter-rank", &session.ability))
        .selected_text(format!("Rank {}", s.rank))
        .width(196.0)
        .show_ui(ui, |ui| {
            for r in 1..=ranks as u32 {
                ui.selectable_value(&mut s.rank, r, format!("Rank {r}"));
            }
        });
    ui.add(egui::Slider::new(&mut s.mana, 0..=sim::MANA).text("Mana"));
    ui.add_space(6.0);
    ui.label(eyebrow("Health at the start"));
    ui.add(
        egui::Slider::new(&mut s.caster_health, 1..=100)
            .suffix(" %")
            .text("Caster"),
    );
    ui.add(
        egui::Slider::new(&mut s.target_health, 1..=100)
            .suffix(" %")
            .text("Targets"),
    );
    ui.add_space(4.0);
    ui.checkbox(&mut s.combat, "Units move and fight");
    ui.checkbox(&mut s.fallen, "Targets start as corpses");
    if s != *settings {
        *settings = s;
    }
    ui.add_space(6.0);
    ui.label(eyebrow("Ground"));
    let mut biome = session.biome.clone();
    egui::ComboBox::from_id_salt(("encounter-ground", &session.ability))
        .selected_text(biome.as_deref().unwrap_or("Zinc stage"))
        .width(196.0)
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut biome, None, "Zinc stage");
            for b in canopy_content::map::BIOMES {
                ui.selectable_value(&mut biome, Some(b.to_string()), b);
            }
        });
    session.biome = biome;
    if let Some(fixture) = &session.fixture {
        ui.add_space(6.0);
        ui.label(
            RichText::new(format!(
                "Free and ready at once (cost and cooldown off). A take lasts {}.",
                seconds(fixture.duration)
            ))
            .color(FAINT)
            .size(11.0),
        );
    }
    if let Some(error) = &session.error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).color(ERROR).size(11.0));
    }
}

/// The transport: Cast, Interrupt, Reset, Play, a tick on, the speed and the time.
fn transport(ui: &mut Ui, session: &mut Session) {
    let ready = session.fixture.is_some();
    ui.horizontal(|ui| {
        let label = if session.targeting {
            "Choose a target  Q"
        } else {
            "Cast  Q"
        };
        if ui
            .add_enabled(ready, primary(label))
            .on_hover_text("A fresh take: the units as set, then the cast (Q again while choosing aims at the first target)")
            .clicked()
        {
            session.cast();
        }
        let casting = session.take.as_ref().is_some_and(|t| t.aim().is_some() && !t.done());
        if ui
            .add_enabled(casting, egui::Button::new("Interrupt"))
            .on_hover_text("Stops the caster now (the Stop command): a cast before its release is cancelled and refunded, a channel ends")
            .clicked()
        {
            session.interrupt();
        }
        if ui.add_enabled(ready, egui::Button::new("Reset  R")).clicked() {
            session.reset();
        }
        let play = if session.playing { "Pause" } else { "Play" };
        if ui
            .add_enabled(ready, egui::Button::new(format!("{play}  Space")))
            .clicked()
        {
            session.toggle();
        }
        if ui
            .add_enabled(ready, egui::Button::new("+1 tick"))
            .clicked()
        {
            session.playing = false;
            if session.take.as_ref().is_some_and(|t| !t.done()) {
                session.step();
            }
        }
        ui.add_space(6.0);
        for (speed, label) in SPEEDS {
            if ui
                .selectable_label(session.speed == speed, label)
                .clicked()
            {
                session.speed = speed;
            }
        }
        ui.add_space(6.0);
        if ui.button("Fit").on_hover_text("Frame the units (double-click the viewport)").clicked() {
            session.fit = true;
        }
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if let Some(take) = &session.take {
                ui.label(
                    RichText::new(format!(
                        "{} · {} t / {}",
                        seconds(take.tick()),
                        take.tick(),
                        seconds(take.fixture().duration)
                    ))
                    .color(MUTED)
                    .size(11.0)
                    .monospace(),
                );
            }
        });
    });
}

/// The timeline: a playhead to drag and the lanes (animation, impacts, statuses); or the
/// events list.
fn lower(ui: &mut Ui, session: &mut Session) {
    ui.horizontal(|ui| {
        for (page, label) in [(Page::Timeline, "Timeline"), (Page::Events, "Events")] {
            if ui.selectable_label(session.page == page, label).clicked() {
                session.page = page;
            }
        }
        if let Some(take) = &session.take
            && let Some(refusal) = take.refusals().last()
        {
            ui.add_space(12.0);
            ui.label(
                RichText::new(format!(
                    "Refused at {}: {}",
                    seconds(refusal.tick),
                    refusal.message
                ))
                .color(ERROR)
                .size(11.0),
            );
        }
    });
    ui.add_space(4.0);
    let Some(take) = &session.take else {
        return;
    };
    let duration = take.fixture().duration.max(1);
    let now = take.tick();
    let mut seek = None;
    match session.page {
        Page::Events => {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if take.events().is_empty() {
                        ui.label(
                            RichText::new("No events yet: cast, then play.")
                                .color(FAINT)
                                .size(11.0),
                        );
                    }
                    egui::Grid::new(("encounter-events", &session.ability))
                        .num_columns(4)
                        .spacing([16.0, 2.0])
                        .striped(false)
                        .show(ui, |ui| {
                            for e in take.events() {
                                let past = e.tick <= now;
                                let tone = if past { SOFT } else { FAINT };
                                if ui
                                    .add(
                                        egui::Label::new(
                                            RichText::new(format!("{:>4} t", e.tick))
                                                .color(MUTED)
                                                .size(11.0)
                                                .monospace(),
                                        )
                                        .sense(Sense::click()),
                                    )
                                    .on_hover_text("Seek here")
                                    .clicked()
                                {
                                    seek = Some(e.tick);
                                }
                                ui.label(
                                    RichText::new(sim::event_name(e.kind))
                                        .color(DIRTY)
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new(
                                        e.amount.map_or(String::new(), |a| a.to_string()),
                                    )
                                    .color(tone)
                                    .size(11.0),
                                );
                                let about = match (e.target, &e.status) {
                                    (Some(t), Some(s)) => format!("unit {} · {s}", t.0),
                                    (Some(t), None) => format!("unit {}", t.0),
                                    (None, _) => e.reason.clone().unwrap_or_default(),
                                };
                                ui.label(RichText::new(about).color(tone).size(11.0));
                                ui.end_row();
                            }
                        });
                });
        }
        Page::Timeline => {
            let timeline = take.timeline();
            const LABEL: f32 = 86.0;
            const ROW: f32 = 24.0;
            let width = ui.available_width();
            let lanes: [(&str, &Vec<Span>); 3] = [
                ("Animation", &timeline.animation),
                ("Impacts", &timeline.impacts),
                ("Statuses", &timeline.statuses),
            ];
            let height = 22.0 + 14.0 + ROW * lanes.len() as f32;
            let (rect, response) =
                ui.allocate_exact_size(vec2(width, height), Sense::click_and_drag());
            let painter = ui.painter_at(rect);
            let lane_left = rect.left() + LABEL;
            let lane_width = (rect.width() - LABEL - 4.0).max(10.0);
            let x_of =
                |tick: u32| lane_left + lane_width * tick.min(duration) as f32 / duration as f32;
            let tick_of = |x: f32| {
                (((x - lane_left) / lane_width).clamp(0.0, 1.0) * duration as f32).round() as u32
            };
            // The ruler: the playhead's track and the seconds.
            let ruler =
                Rect::from_min_size(pos2(lane_left, rect.top() + 4.0), vec2(lane_width, 12.0));
            painter.text(
                pos2(rect.left(), ruler.center().y),
                egui::Align2::LEFT_CENTER,
                "PLAYHEAD",
                egui::FontId::proportional(9.0),
                FAINT,
            );
            painter.rect_filled(ruler.shrink2(vec2(0.0, 4.0)), 2, SELECTED);
            painter.rect_filled(
                Rect::from_min_max(
                    ruler.left_top() + vec2(0.0, 4.0),
                    pos2(x_of(now), ruler.bottom() - 4.0),
                ),
                2,
                CONTROL,
            );
            for i in 0..=4 {
                let tick = duration * i / 4;
                let x = x_of(tick);
                painter.text(
                    pos2(
                        x.clamp(lane_left + 12.0, lane_left + lane_width - 12.0),
                        rect.top() + 26.0,
                    ),
                    egui::Align2::CENTER_CENTER,
                    format!("{:.1}s", tick as f32 / TICKS),
                    egui::FontId::proportional(9.0),
                    FAINT,
                );
            }
            let top = rect.top() + 36.0;
            let pointer = response.hover_pos();
            let mut hovered: Option<&Span> = None;
            for (i, (name, spans)) in lanes.iter().enumerate() {
                let y = top + ROW * i as f32;
                painter.text(
                    pos2(rect.left(), y + ROW / 2.0),
                    egui::Align2::LEFT_CENTER,
                    *name,
                    egui::FontId::proportional(11.0),
                    MUTED,
                );
                let lane =
                    Rect::from_min_size(pos2(lane_left, y + 2.0), vec2(lane_width, ROW - 4.0));
                painter.rect_filled(lane, 3, RAISED);
                for k in 1..10 {
                    let x = lane.left() + lane.width() * k as f32 / 10.0;
                    painter.line_segment(
                        [pos2(x, lane.top()), pos2(x, lane.bottom())],
                        Stroke::new(1.0, MAIN),
                    );
                }
                for span in spans.iter() {
                    let r = Rect::from_min_max(
                        pos2(x_of(span.start), lane.top() + 1.0),
                        pos2(
                            x_of(span.end).max(x_of(span.start) + 3.0),
                            lane.bottom() - 1.0,
                        ),
                    );
                    let mut fill = span_color(span.kind);
                    if now < span.start {
                        fill = fill.gamma_multiply(0.45);
                    }
                    painter.rect_filled(r, 3, fill);
                    if now >= span.start && now < span.end {
                        painter.rect_stroke(r, 3, Stroke::new(1.0, TEXT), egui::StrokeKind::Inside);
                    }
                    if r.width() > 24.0 {
                        let text = painter.layout_no_wrap(
                            span.label.clone(),
                            egui::FontId::proportional(10.0),
                            TEXT,
                        );
                        let clip = painter.with_clip_rect(r.shrink(1.0));
                        clip.galley(
                            pos2(r.left() + 4.0, r.center().y - text.size().y / 2.0),
                            text,
                            TEXT,
                        );
                    }
                    if pointer.is_some_and(|p| r.contains(p)) {
                        hovered = Some(span);
                    }
                }
            }
            // The playhead.
            let x = x_of(now);
            painter.line_segment(
                [pos2(x, rect.top() + 2.0), pos2(x, rect.bottom())],
                Stroke::new(1.5, TEXT),
            );
            if let Some(span) = hovered {
                let text = format!(
                    "{} · {}–{}",
                    span.label,
                    seconds(span.start),
                    seconds(span.end)
                );
                let response = response.clone().on_hover_text(text);
                if response.clicked() {
                    seek = Some(span.start);
                }
            } else if (response.clicked() || response.dragged())
                && let Some(p) = response.interact_pointer_pos()
            {
                seek = Some(tick_of(p.x));
            }
            if timeline.animation.is_empty() && timeline.impacts.is_empty() {
                let hint = if session.targeting {
                    "Choose a target in the scene to cast. Escape cancels."
                } else {
                    "Cast to start a fresh take. Drag the playhead to inspect any moment."
                };
                painter.text(
                    pos2(lane_left + 12.0, top + ROW * 1.5),
                    egui::Align2::LEFT_CENTER,
                    hint,
                    egui::FontId::proportional(11.5),
                    MUTED,
                );
            }
        }
    }
    if let Some(tick) = seek {
        session.playing = false;
        session.seek(tick);
    }
}

/// The stage view of an ability: settings on the left, the viewport, the transport and
/// the timeline below it.
pub fn show(
    ui: &mut Ui,
    session: &mut Session,
    content: Option<&Content>,
    texture: Option<egui::TextureId>,
    stale: bool,
) -> Out {
    let mut out = Out::default();
    for command in std::mem::take(&mut session.queued) {
        match command {
            Command::Cast => {
                let aim = session.take.as_ref().and_then(Take::first_aim);
                session.fire(aim);
            }
            Command::Seek(tick) => {
                session.playing = false;
                session.seek(tick);
            }
        }
    }
    egui::Panel::left(egui::Id::new(("encounter-settings", &session.ability)))
        .exact_size(220.0)
        .resizable(false)
        .show_separator_line(true)
        .frame(
            egui::Frame::new()
                .fill(PANEL)
                .inner_margin(egui::Margin::same(10)),
        )
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| settings_panel(ui, session, content));
        });
    egui::Panel::bottom(egui::Id::new(("encounter-timeline", &session.ability)))
        .exact_size(214.0)
        .resizable(false)
        .show_separator_line(true)
        .frame(
            egui::Frame::new()
                .fill(SUNK)
                .inner_margin(egui::Margin::symmetric(12, 8)),
        )
        .show(ui, |ui| {
            transport(ui, session);
            ui.add_space(6.0);
            lower(ui, session);
        });
    // Keys while the stage shows and no field is being typed in.
    if !ui.ctx().egui_wants_keyboard_input() {
        let (q, r, space, escape) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::Q),
                i.key_pressed(egui::Key::R),
                i.key_pressed(egui::Key::Space),
                i.key_pressed(egui::Key::Escape),
            )
        });
        if q && session.fixture.is_some() {
            session.cast();
        }
        if r && session.fixture.is_some() {
            session.reset();
        }
        // A focused button takes Space itself.
        let focused = ui.ctx().memory(|m| m.focused().is_some());
        if space && !focused && session.fixture.is_some() {
            session.toggle();
        }
        if escape {
            session.targeting = false;
            session.hover = None;
        }
    }
    session.advance(ui.input(|i| i.stable_dt));

    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, RAISED);
    if session.shown[1].is_none() {
        let text = session
            .error
            .as_deref()
            .unwrap_or("Setting up the encounter…");
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            text,
            egui::FontId::proportional(12.0),
            FAINT,
        );
        return out;
    }
    if let Some(texture) = texture {
        painter.image(
            texture,
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    let scale = ui.ctx().pixels_per_point();
    out.size = Some([
        (rect.width() * scale) as u32,
        (rect.height() * scale) as u32,
    ]);
    let aspect = rect.width() / rect.height().max(1.0);

    // Camera: drag turns, right or middle drag pans, scroll zooms, double-click fits.
    let input = ui.input(|i| i.clone());
    if response.dragged_by(egui::PointerButton::Primary) && response.drag_delta().length() > 0.0 {
        let d = response.drag_delta();
        session.camera.yaw -= d.x * 0.008;
        session.camera.pitch = (session.camera.pitch + d.y * 0.005).clamp(0.2, 1.5);
        session.fit = false;
    }
    if response.dragged_by(egui::PointerButton::Secondary)
        || response.dragged_by(egui::PointerButton::Middle)
    {
        let d = response.drag_delta();
        let k = session.camera.distance / rect.height().max(1.0) * 1.2;
        let (s, c) = session.camera.yaw.sin_cos();
        session.camera.focus.x -= (d.x * c + d.y * s) * k;
        session.camera.focus.z -= (-d.x * s + d.y * c) * k;
        session.fit = false;
    }
    if response.hovered() {
        let scroll = input.smooth_scroll_delta.y;
        if scroll != 0.0 {
            session.camera.distance =
                (session.camera.distance * (-scroll * 0.002).exp()).clamp(4.0, 220.0);
            session.fit = false;
        }
    }
    if response.double_clicked() {
        session.fit = true;
    }
    if session.fit {
        session.frame_units(aspect);
    }
    // Targeting: what is under the pointer, and a click fires.
    if session.targeting {
        session.hover = response.hover_pos().and_then(|p| session.pick(rect, p));
        if response.clicked()
            && let Some(p) = response.interact_pointer_pos()
            && let Some(aim) = session.pick(rect, p)
        {
            session.fire(Some(aim));
        }
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
    }
    // Corpses (the game draws a body only while it falls) as faint rings.
    if let Some(take) = &session.take {
        let matrix = session.camera.matrix(aspect);
        let project = |p: Vec3| {
            let clip = matrix * p.extend(1.0);
            (clip.w > 0.0).then(|| {
                let ndc = clip.truncate() / clip.w;
                pos2(
                    rect.left() + (ndc.x + 1.0) / 2.0 * rect.width(),
                    rect.top() + (1.0 - ndc.y) / 2.0 * rect.height(),
                )
            })
        };
        for corpse in &take.world().state().combat.corpses {
            let (x, z) = (corpse.position.x as f32, corpse.position.y as f32);
            let points: Vec<Pos2> = (0..=24)
                .filter_map(|i| {
                    let a = i as f32 / 24.0 * std::f32::consts::TAU;
                    project(Vec3::new(
                        x + a.cos() * 0.8,
                        session.camera.focus.y + 0.05,
                        z + a.sin() * 0.8,
                    ))
                })
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.0, MUTED.gamma_multiply(0.7)),
            ));
        }
    }
    let note = if session.targeting {
        Some(
            match session.fixture.as_ref().map(|f| f.ability.targeting.kind) {
                Some(TargetingKind::Point) => {
                    "Click the ground to cast there · Q aims at the first target · Escape cancels"
                }
                _ => "Click a unit to cast at it · Q aims at the first target · Escape cancels",
            },
        )
    } else if stale {
        Some("Showing the last version that passed the check")
    } else {
        None
    };
    if let Some(note) = note {
        painter.text(
            pos2(rect.center().x, rect.bottom() - 16.0),
            egui::Align2::CENTER_CENTER,
            note,
            egui::FontId::proportional(11.5),
            SOFT,
        );
    }
    out
}

/// An encounter run headless to the end of its take (MCP `encounter`): the settings, the
/// events, the refusals, the timeline and the units as they end.
pub fn headless(
    content: &Content,
    args: &crate::mcp::EncounterArgs,
) -> Result<serde_json::Value, String> {
    use serde_json::json;
    let mut settings = sim::suggest(content, &args.ability);
    if let Some(id) = &args.preset {
        let preset = Preset::find(id).ok_or_else(|| {
            let ids: Vec<&str> = PRESETS.iter().map(|p| p.id).collect();
            format!("no preset {id} (presets: {})", ids.join(", "))
        })?;
        settings = preset.apply(&settings);
    }
    if let Some(relation) = &args.relation {
        settings.relation = serde_json::from_value(json!(relation))
            .map_err(|_| format!("relation is ally, enemy, self or neutral, not {relation}"))?;
    }
    let s = &mut settings;
    if let Some(v) = &args.caster {
        s.caster = v.clone();
    }
    if let Some(v) = &args.target {
        s.target = v.clone();
    }
    s.targets = args.targets.unwrap_or(s.targets);
    s.distance = args.distance.unwrap_or(s.distance);
    s.spacing = args.spacing.unwrap_or(s.spacing);
    s.rank = args.rank.unwrap_or(s.rank);
    s.caster_health = args.caster_health.unwrap_or(s.caster_health);
    s.target_health = args.target_health.unwrap_or(s.target_health);
    s.mana = args.mana.unwrap_or(s.mana);
    s.combat = args.combat.unwrap_or(s.combat);
    s.fallen = args.fallen.unwrap_or(s.fallen);
    let fixture = Arc::new(Fixture::new(content, &args.ability, &settings)?);
    let idle = Take::new(fixture.clone(), None)?;
    let aim = match (args.unit, args.point) {
        (Some(entity), _) => Some(Aim::Unit { entity }),
        (None, Some([x, y])) => Some(Aim::Point { x, y }),
        (None, None) => idle.first_aim(),
    };
    let mut take = Take::new(fixture.clone(), aim)?;
    if let Some(at) = args.interrupt_at {
        take.seek(at)?;
        take.intervene(Intervention::Interrupt);
    }
    take.finish();
    let world = take.world();
    let state = world.state();
    let events: Vec<_> = take
        .events()
        .iter()
        .map(|e| {
            json!({
                "tick": e.tick,
                "event": sim::event_name(e.kind),
                "amount": e.amount,
                "caster": e.caster.0,
                "target": e.target.map(|t| t.0),
                "status": e.status,
                "reason": e.reason,
                "durationTicks": e.duration_ticks,
            })
        })
        .collect();
    let units: Vec<_> = state
        .entities
        .iter()
        .filter(|e| e.unit.is_some())
        .map(|e| {
            let max = canopy_sim::stats::Stats::of(
                world.catalog(),
                e,
                canopy_sim::stats::research(state, e.owner),
            );
            json!({
                "id": e.id.0,
                "placement": e.placement,
                "definition": world.catalog().id(e.definition).as_str(),
                "owner": e.owner,
                "hp": e.hp,
                "maxHp": max.max_hp,
                "mana": e.caster.as_ref().map(|c| c.mana),
                "statuses": e.statuses.iter().map(|s| world.catalog().spells.status(s.key).name.clone()).collect::<Vec<_>>(),
            })
        })
        .collect();
    let spans = |list: &[Span]| -> Vec<serde_json::Value> {
        list.iter()
            .map(|s| json!({"label": s.label, "start": s.start, "end": s.end}))
            .collect()
    };
    let timeline = take.timeline();
    Ok(json!({
        "settings": settings,
        "rank": fixture.rank,
        "aim": aim,
        "ticks": take.tick(),
        "secondsPerTick": 1.0 / TICKS,
        "events": events,
        "refused": take.refusals().iter().map(|r| json!({"tick": r.tick, "message": r.message})).collect::<Vec<_>>(),
        "timeline": {
            "animation": spans(&timeline.animation),
            "impacts": spans(&timeline.impacts),
            "statuses": spans(&timeline.statuses),
        },
        "units": units,
        "corpses": state.combat.corpses.len(),
    }))
}

#[cfg(test)]
#[path = "../../sim/tests/abilities_fixture/mod.rs"]
mod abilities_fixture;

#[cfg(test)]
mod tests {
    use super::abilities_fixture::*;
    use serde_json::{Value, json};

    fn args(value: Value) -> crate::mcp::EncounterArgs {
        serde_json::from_value(value).unwrap()
    }

    #[test]
    fn mcp_runs_a_take_to_its_end() {
        let mut spec = Spec::default();
        give(
            &mut spec,
            spell(
                "ability.test.bolt",
                "unit",
                &["enemy"],
                12,
                50,
                200,
                10,
                8,
                json!([{"op": "damage", "target": "target", "amount": 30, "damageType": "spell"}]),
            ),
            "bolt",
        );
        let content = content("editor-encounter", &spec);
        let answer =
            super::headless(&content, &args(json!({"ability": "ability.test.bolt"}))).unwrap();
        let damaged: Vec<&Value> = answer["events"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| e["event"] == "damaged")
            .collect();
        assert_eq!(damaged.len(), 1, "{answer}");
        assert_eq!(damaged[0]["amount"], 30);
        assert_eq!(answer["settings"]["caster"], "unit.caster");
        let target = answer["units"]
            .as_array()
            .unwrap()
            .iter()
            .find(|u| u["placement"] == "target-0")
            .unwrap();
        assert_eq!(
            target["hp"].as_u64().unwrap() + 30,
            target["maxHp"].as_u64().unwrap()
        );
        // Stopped before its release, nothing lands; a preset that does not exist is named.
        let stopped = super::headless(
            &content,
            &args(
                json!({"ability": "ability.test.bolt", "preset": "enemy-group", "interruptAt": 2}),
            ),
        )
        .unwrap();
        assert!(
            stopped["events"]
                .as_array()
                .unwrap()
                .iter()
                .all(|e| e["event"] != "damaged")
        );
        assert!(
            super::headless(
                &content,
                &args(json!({"ability": "ability.test.bolt", "preset": "crowd"}))
            )
            .unwrap_err()
            .contains("enemy-single")
        );
    }
}
