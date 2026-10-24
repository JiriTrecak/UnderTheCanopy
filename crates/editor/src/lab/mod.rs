//! The combat lab (docs/editor.md, "The Combat Lab"): scenarios on a flat grid of open and
//! closed cells, with units and buildings for any player, named groups and the orders they
//! get, run with the real simulation and measured, with what movement decided drawn over
//! the units. Scenarios are files beside the package (`labs/*.lab.json`), mounted into the
//! editor's documents as `@labs/…`, so they share its undo history and Cmd+S but never ship.
//!
//! The lab is its own editor: it shares only the generic stage pieces (`crate::sandbox`).

mod mesh;
mod view;

pub use view::{show, starter};

use crate::library::{Element, Library, Node};
use canopy_assets::Pack;
use canopy_content::{
    Content,
    lab::{EXTENSION, LABS_FOLDER, Lab},
};
use canopy_scene::{
    MapScene,
    live::{Presentation, Presenter},
};
use canopy_sim::lab::Run;
use glam::{Mat4, Vec3};
use std::{path::Path, sync::Arc, time::Instant};

/// The name the store mounts the scenario folder under.
pub const MOUNT_NAME: &str = LABS_FOLDER;
/// What scenario documents' names start with.
pub const PREFIX: &str = "@labs/";
/// Ticks a second.
const TICKS: f32 = canopy_content::lab::TICKS as f32;
/// The height of the sandbox ground, world units.
const FLOOR: f32 = 2.0;

/// The document of scenario `id`.
pub fn document(id: &str) -> String {
    format!("{PREFIX}{id}{EXTENSION}")
}

/// Whether `document` is a scenario.
pub fn is_lab(document: &str) -> bool {
    document.starts_with(PREFIX) && document.ends_with(EXTENSION)
}

/// The lab's library: the scenarios in `data`'s `labs/` folder, by name.
pub fn library(data: Option<&Path>) -> Library {
    let mut library = Library::default();
    let Some(data) = data else {
        return library;
    };
    let mut nodes: Vec<Node> = canopy_content::lab::library(data)
        .into_iter()
        .map(|(id, path)| {
            let name = canopy_content::lab::read(&path).map_or_else(|_| id.clone(), |l| l.name);
            Node::Element(Element {
                document: document(&id),
                path: document(&id),
                id,
                name,
            })
        })
        .collect();
    nodes.sort_by(|a, b| match (a, b) {
        (Node::Element(a), Node::Element(b)) => a.name.cmp(&b.name),
        _ => std::cmp::Ordering::Equal,
    });
    library.count = nodes.len();
    library.nodes = nodes;
    library
}

/// A free id for a new scenario named `name`.
pub fn new_id(data: &Path, name: &str) -> String {
    let base: String = name
        .to_lowercase()
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-");
    let base = if base.is_empty() {
        "scenario".into()
    } else {
        base
    };
    let taken = canopy_content::lab::library(data);
    let mut id = base.clone();
    let mut n = 2;
    while taken.iter().any(|(t, _)| *t == id) {
        id = format!("{base}-{n}");
        n += 1;
    }
    id
}

/// What a tool does in the viewport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    /// Paint cells closed (Shift opens them).
    Close,
    Open,
    Place,
    /// Click the ground to give the chosen group an order there.
    Order,
}

impl Tool {
    pub const ALL: [Self; 5] = [
        Self::Select,
        Self::Close,
        Self::Open,
        Self::Place,
        Self::Order,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::Select => "Select",
            Self::Close => "Close cells",
            Self::Open => "Open cells",
            Self::Place => "Place",
            Self::Order => "Order",
        }
    }

    pub fn key(self) -> egui::Key {
        match self {
            Self::Select => egui::Key::S,
            Self::Close => egui::Key::C,
            Self::Open => egui::Key::O,
            Self::Place => egui::Key::P,
            Self::Order => egui::Key::G,
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Self::Select => {
                "Click or drag a box to select units; drag them to move; Delete removes"
            }
            Self::Close => "Paint closed cells (brush or box); Shift opens",
            Self::Open => "Paint open cells (brush or box)",
            Self::Place => "Click to place the chosen unit, as many as the count, packed",
            Self::Order => "Click the ground: the group gets the order there, at the tick",
        }
    }
}

/// The debug layers drawn over the run.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Layers {
    /// Each unit's route, its waypoints and goal.
    pub routes: bool,
    /// The steering vectors: desired, separation, avoidance and what it asked for.
    pub vectors: bool,
    /// The headings tried when the straight step was blocked, by why.
    pub tried: bool,
    /// What each unit is doing: blocked, negotiating, detouring, settled.
    pub states: bool,
    /// Detour paths round bodies.
    pub detours: bool,
    /// The navigation sectors (16 cells).
    pub sectors: bool,
    /// The orders' points.
    pub orders: bool,
    /// Each unit's collision body (D102).
    pub bodies: bool,
}

impl Default for Layers {
    fn default() -> Self {
        Self {
            routes: true,
            vectors: true,
            tried: false,
            states: true,
            detours: true,
            sectors: false,
            orders: true,
            bodies: false,
        }
    }
}

/// How long ticks took to step, wall time (the editor's build, not the game's).
#[derive(Debug, Clone, Copy, Default)]
struct Timing {
    last: f32,
    worst: f32,
    total: f32,
    steps: u32,
}

/// An orbiting camera over the grid.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Camera {
    focus: Vec3,
    distance: f32,
    yaw: f32,
    pitch: f32,
}

impl Camera {
    fn over(size: u32) -> Self {
        let half = size as f32 / 2.0;
        Self {
            focus: Vec3::new(half, FLOOR, half),
            distance: size as f32 * 0.9,
            yaw: 0.0,
            pitch: 1.1,
        }
    }

    fn eye(&self) -> Vec3 {
        self.focus
            + self.distance
                * Vec3::new(
                    self.yaw.sin() * self.pitch.cos(),
                    self.pitch.sin(),
                    self.yaw.cos() * self.pitch.cos(),
                )
    }

    /// View and projection for overlays and picking (the renderer's own is reverse-Z,
    /// with the same frustum).
    fn matrix(&self, aspect: f32) -> Mat4 {
        let d = self.distance.max(1.0);
        Mat4::perspective_rh(crate::map::view::FOV, aspect.max(0.1), d * 0.02, d * 12.0)
            * Mat4::look_at_rh(self.eye(), self.focus, Vec3::Y)
    }
}

/// One open scenario.
pub struct Session {
    pub document: String,
    tool: Tool,
    /// Brush width in cells.
    brush: u32,
    /// Cell tools paint a box instead of a brush.
    boxed: bool,
    /// What Place puts down, for whom, how many, into which group.
    placing: Option<String>,
    owner: u8,
    count: u32,
    group: String,
    search: String,
    /// Units selected, by index in the scenario.
    selected: Vec<usize>,
    /// The order Order gives, and to which group.
    order: canopy_content::lab::OrderKind,
    order_group: String,
    order_target: String,
    /// The scenario as last read, with the store revision it was read at.
    lab: Option<(u64, Lab)>,
    /// The movement constants this session's runs play by (the game's unless changed).
    tuning: canopy_sim::movement::tuning::Tuning,
    /// The content, scenario and tuning the run was built from.
    built: Option<(Arc<Content>, Lab, canopy_sim::movement::tuning::Tuning)>,
    run: Option<Run>,
    /// The grid's ground and its version; built for this scenario's cells.
    scene: Option<(Lab, Arc<MapScene>)>,
    version: u64,
    pack: Option<Arc<Pack>>,
    presenter: Option<Presenter>,
    shown: [Option<Arc<Presentation>>; 2],
    playing: bool,
    speed: f32,
    owed: f32,
    timing: Timing,
    camera: Option<Camera>,
    layers: Layers,
    /// The unit whose trace the panel shows.
    inspected: Option<u32>,
    error: Option<String>,
    catalog: Option<crate::world::catalog::Catalog>,
    palette: Option<crate::world::palette::Palette>,
    /// The drag under way in the viewport.
    drag: Option<view::Drag>,
    /// A left click gives the selection an attack-move (A).
    armed: bool,
    /// Where hand orders went, until when (seconds of the editor's clock).
    marks: Vec<view::Mark>,
}

impl Session {
    pub fn new(document: &str) -> Self {
        Self {
            document: document.to_string(),
            tool: Tool::Select,
            brush: 2,
            boxed: false,
            placing: None,
            owner: 1,
            count: 1,
            group: "red".into(),
            search: String::new(),
            selected: Vec::new(),
            order: canopy_content::lab::OrderKind::Move,
            order_group: String::new(),
            order_target: String::new(),
            lab: None,
            tuning: Default::default(),
            built: None,
            run: None,
            scene: None,
            version: crate::sandbox::next_version(),
            pack: None,
            presenter: None,
            shown: [None, None],
            playing: false,
            speed: 1.0,
            owed: 0.0,
            timing: Timing::default(),
            camera: None,
            layers: Layers::default(),
            inspected: None,
            error: None,
            catalog: None,
            palette: None,
            drag: None,
            armed: false,
            marks: Vec::new(),
        }
    }

    /// The scenario as the store holds it now.
    fn read(&mut self, store: &mut crate::document::Store) -> Option<&Lab> {
        let revision = store.revision(&self.document);
        let fresh = self.lab.as_ref().is_none_or(|(r, _)| *r != revision);
        if fresh {
            let value = store.get(&self.document).ok().flatten()?.clone();
            match serde_json::from_value::<Lab>(value) {
                Ok(lab) => {
                    self.lab = Some((revision, lab));
                    self.error = None;
                }
                Err(e) => {
                    self.error = Some(format!("The scenario cannot be read: {e}"));
                    self.lab = None;
                }
            }
        }
        self.lab.as_ref().map(|(_, l)| l)
    }

    /// Builds what is out of date: a fresh run (at its first tick) when the content or
    /// the scenario changed, and the ground when the grid did.
    fn prepare(&mut self, content: &Arc<Content>, pack: &Arc<Pack>, look: &canopy_scene::Look) {
        self.pack = Some(pack.clone());
        let Some((_, lab)) = &self.lab else {
            return;
        };
        let lab = lab.clone();
        if self.catalog.is_none() {
            self.catalog = Some(crate::world::catalog::Catalog::new(content));
        }
        if self.palette.is_none() {
            self.palette = Some(crate::world::palette::Palette::new(
                canopy_content::map::BIOMES[0],
                pack,
                look,
            ));
        }
        let stale = self
            .built
            .as_ref()
            .is_none_or(|(c, l, t)| !Arc::ptr_eq(c, content) || *l != lab || *t != self.tuning);
        if !stale {
            return;
        }
        self.built = Some((content.clone(), lab.clone(), self.tuning));
        self.playing = false;
        self.owed = 0.0;
        self.timing = Timing::default();
        let problems = lab.problems(content);
        if !problems.is_empty() {
            self.fail(problems.join("; "));
            return;
        }
        let mut run = match Run::tuned(content.clone(), &lab, self.tuning) {
            Ok(run) => run,
            Err(e) => {
                self.fail(e.0);
                return;
            }
        };
        run.world.set_probe(true);
        let resized = self.scene.as_ref().is_none_or(|(l, _)| l.size != lab.size);
        let grid_changed = self
            .scene
            .as_ref()
            .is_none_or(|(l, _)| l.size != lab.size || l.closed != lab.closed);
        if grid_changed {
            let map = canopy_sim::lab::map(&lab);
            match crate::sandbox::zinc_ground(&run.world, &map, pack, look) {
                Ok(mut scene) => {
                    scene.world.blockout = Some(mesh::cells(&lab));
                    self.scene = Some((lab.clone(), Arc::new(scene)));
                    self.version = crate::sandbox::next_version();
                }
                Err(e) => {
                    self.fail(e);
                    return;
                }
            }
        }
        if self.camera.is_none() || resized {
            self.camera = Some(Camera::over(lab.size));
        }
        let colors = self
            .scene
            .as_ref()
            .map(|(_, s)| s.colors.clone())
            .unwrap_or_default();
        let map = canopy_sim::lab::map(&lab);
        self.presenter = Some(Presenter::new(
            &run.world,
            &map,
            Some(pack.clone()),
            canopy_scene::live::Setup {
                colors,
                tree_slots: Default::default(),
                statics: 0,
            },
        ));
        self.shown = [None, None];
        self.run = Some(run);
        self.error = None;
        self.present();
    }

    fn fail(&mut self, error: String) {
        self.error = Some(error);
        self.run = None;
        self.presenter = None;
        self.shown = [None, None];
        self.playing = false;
    }

    fn present(&mut self) {
        let (Some(run), Some(presenter)) = (&self.run, &mut self.presenter) else {
            return;
        };
        let world = &run.world;
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

    /// One tick, timed and presented.
    fn step(&mut self) {
        let Some(run) = &mut self.run else {
            return;
        };
        if run.finished() {
            self.playing = false;
            return;
        }
        let started = Instant::now();
        run.step();
        let ms = started.elapsed().as_secs_f32() * 1000.0;
        let t = &mut self.timing;
        t.last = ms;
        t.worst = t.worst.max(ms);
        t.total += ms;
        t.steps += 1;
        self.present();
    }

    /// Plays `dt` seconds.
    fn advance(&mut self, dt: f32) {
        if !self.playing {
            return;
        }
        self.owed += dt.min(0.1) * TICKS * self.speed;
        // Never fall further behind than a few frames: a slow scenario plays slower.
        let mut budget = 8;
        while self.owed >= 1.0 && budget > 0 {
            if self.run.as_ref().is_none_or(Run::finished) {
                self.playing = false;
                self.owed = 0.0;
                return;
            }
            self.step();
            self.owed -= 1.0;
            budget -= 1;
        }
        if budget == 0 {
            self.owed = self.owed.min(1.0);
        }
    }

    /// Back to the first tick.
    fn restart(&mut self) {
        self.built = None;
        self.playing = false;
    }

    /// Runs `ticks` ticks at once (captures).
    pub fn run_ticks(&mut self, ticks: u32) {
        self.playing = false;
        for _ in 0..ticks {
            self.step();
        }
    }

    /// Turns the debug layer `name` on or off (captures): routes, vectors, tried,
    /// states, detours, sectors, orders.
    pub fn set_layer(&mut self, name: &str, on: bool) {
        let l = &mut self.layers;
        match name {
            "routes" => l.routes = on,
            "vectors" => l.vectors = on,
            "tried" => l.tried = on,
            "states" => l.states = on,
            "detours" => l.detours = on,
            "sectors" => l.sectors = on,
            "orders" => l.orders = on,
            "bodies" => l.bodies = on,
            _ => {}
        }
    }

    /// Selects group `name` and walks it to cell (`x`, `z`) by hand (captures).
    pub fn hand_move(&mut self, name: &str, x: f32, z: f32) {
        view::hand_move_group(self, name, [x, z]);
    }

    /// Inspects the unit held up longest right now (captures), and selects it.
    pub fn inspect_held_up(&mut self) {
        let Some(run) = &self.run else {
            return;
        };
        let Some(probe) = run.world.probe() else {
            return;
        };
        let held = probe
            .units
            .iter()
            .max_by_key(|(id, t)| (t.stalled, std::cmp::Reverse(id.0)))
            .map(|(id, _)| *id);
        self.inspected = held.map(|id| id.0);
        self.selected = held
            .and_then(|id| run.entities.iter().position(|e| *e == Some(id)))
            .into_iter()
            .collect();
    }

    /// Points the camera at cell (`x`, `z`) from `distance` (captures).
    pub fn look_at(&mut self, x: f32, z: f32, distance: f32, pitch: f32) {
        if let Some(c) = &mut self.camera {
            c.focus = Vec3::new(x, FLOOR, z);
            c.distance = distance;
            c.pitch = pitch;
        }
    }

    /// The shot the stage draws this frame.
    pub fn shot(&self, size: [u32; 2]) -> Option<crate::stage::EncounterShot> {
        let (_, scene) = self.scene.as_ref()?;
        let b = self.shown[1].clone()?;
        let presenter = self.presenter.as_ref()?;
        let (content, _, _) = self.built.as_ref()?;
        let run = self.run.as_ref()?;
        let camera = self.camera?;
        let t = if self.playing {
            self.owed.clamp(0.0, 1.0)
        } else {
            1.0
        };
        let selected = self
            .selected
            .iter()
            .filter_map(|&i| run.entities.get(i).copied().flatten())
            .map(|id| id.0)
            .collect();
        Some(crate::stage::EncounterShot {
            scene: scene.clone(),
            version: self.version,
            biome: None,
            effects: presenter.effects.effect_table().clone(),
            content: content.clone(),
            a: self.shown[0].clone(),
            b,
            t,
            eye: camera.eye(),
            target: camera.focus,
            ui: canopy_scene::world_ui::WorldUiInput {
                selected,
                hovered: self.inspected,
                // Damaged and selected units only: bars on everyone hide the debug layers.
                health: canopy_scene::world_ui::HealthBars::Damaged,
                scale: 1.0,
                ..Default::default()
            },
            size,
        })
    }
}
