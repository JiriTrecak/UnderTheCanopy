//! The map editor's tools (docs/editor.md, M3): terrain brushes that change the map's
//! grids directly (Warcraft III style), and tools that place trees, amber sites, creep
//! camps and starts. A brush stroke works on the session's map; when it ends, every grid
//! it touched becomes one patch over the touched rectangle, in one undo step.

use crate::document::Command;
use canopy_content::{
    Content,
    game::Kind,
    map::{DRY_WATER_HEIGHT, HEIGHT_MAX, HEIGHT_MIN, MAP_HALO, UtcMap, grid::Grid},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// A step between neighbouring vertices above this is a cliff (the simulation's 90 cm).
pub const CLIFF_STEP: f32 = 0.9;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tool {
    Select,
    Raise,
    Lower,
    Flatten,
    Smooth,
    Cliff,
    Ramp,
    Paint,
    Water,
    Trees,
    Props,
    Amber,
    Camp,
    Start,
}

impl Tool {
    pub const ALL: [Tool; 14] = [
        Tool::Select,
        Tool::Raise,
        Tool::Lower,
        Tool::Flatten,
        Tool::Smooth,
        Tool::Cliff,
        Tool::Ramp,
        Tool::Paint,
        Tool::Water,
        Tool::Trees,
        Tool::Props,
        Tool::Amber,
        Tool::Camp,
        Tool::Start,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Tool::Select => "Select",
            Tool::Raise => "Raise",
            Tool::Lower => "Lower",
            Tool::Flatten => "Flatten",
            Tool::Smooth => "Smooth",
            Tool::Cliff => "Cliff",
            Tool::Ramp => "Ramp",
            Tool::Paint => "Paint",
            Tool::Water => "Water",
            Tool::Trees => "Trees",
            Tool::Props => "Props",
            Tool::Amber => "Amber",
            Tool::Camp => "Camp",
            Tool::Start => "Start",
        }
    }

    pub fn hint(self) -> &'static str {
        match self {
            Tool::Select => "Click a start, mine, creep or tree; Delete removes it",
            Tool::Raise => "Drag to raise the ground (Shift lowers)",
            Tool::Lower => "Drag to lower the ground",
            Tool::Flatten => "Drag to level the ground to where the stroke began",
            Tool::Smooth => "Drag to soften slopes (makes ramps between cliff levels)",
            Tool::Cliff => "Drag to lift ground one cliff level (Shift lowers one level)",
            Tool::Ramp => {
                "Drag from the top of a cliff to the ground below: a walkable ramp as wide as the brush"
            }
            Tool::Paint => "Drag to paint the chosen ground",
            Tool::Water => "Drag to flood to the water level (Shift drains)",
            Tool::Trees => "Drag to plant trees (Shift clears them)",
            Tool::Props => "Drag to scatter rocks, logs, mushrooms and flowers (Shift clears them)",
            Tool::Amber => "Click to place an amber site facing the nearest start",
            Tool::Camp => "Click to place a creep camp of the chosen strength",
            Tool::Start => "Click to place a player start with its amber",
        }
    }

    /// Whether dragging paints continuously (the others act on a click).
    pub fn is_brush(self) -> bool {
        matches!(
            self,
            Tool::Raise
                | Tool::Lower
                | Tool::Flatten
                | Tool::Smooth
                | Tool::Cliff
                | Tool::Ramp
                | Tool::Paint
                | Tool::Water
                | Tool::Trees
                | Tool::Props
        )
    }
}

/// What the paint brush lays down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ground {
    /// The biome's bare soil: removes grass, rock and paint.
    Soil,
    /// The biome's grass.
    Grass,
    /// The biome's rock.
    Rock,
    /// A painted material (`terrain.paint[n]`).
    Paint(usize),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tier {
    Easy,
    Medium,
    Hard,
}

impl Tier {
    pub const ALL: [Tier; 3] = [Tier::Easy, Tier::Medium, Tier::Hard];

    pub fn name(self) -> &'static str {
        match self {
            Tier::Easy => "easy",
            Tier::Medium => "medium",
            Tier::Hard => "hard",
        }
    }

    /// The camp's summed creep levels (the Warcraft importer's tiers: up to 9 easy,
    /// 10 to 19 medium, 20 and more hard).
    fn levels(self) -> (u32, u32) {
        match self {
            Tier::Easy => (5, 8),
            Tier::Medium => (12, 17),
            Tier::Hard => (21, 27),
        }
    }
}

/// The tools' settings and the stroke in progress.
#[derive(Debug, Clone)]
pub struct Tools {
    pub tool: Tool,
    /// Brush radius, cells.
    pub radius: f32,
    /// Brush strength, 0 to 1.
    pub strength: f32,
    pub ground: Ground,
    /// The water surface the water brush floods to.
    pub water_level: f32,
    /// Height of one cliff level.
    pub cliff_level: f32,
    pub tier: Tier,
    /// Scenery the tree brush plants (a harvestable tree), and trees per 100 cells.
    pub tree: String,
    pub density: f32,
    /// Scenery the props brush scatters (decoration that blocks nothing).
    pub prop: String,
    /// Amber per node of a new site.
    pub amber: u32,
    pub stroke: Option<Stroke>,
    /// The selected start, entity or object: (kind, id).
    pub selected: Option<(Picked, String)>,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            tool: Tool::Raise,
            radius: 6.0,
            strength: 0.5,
            ground: Ground::Grass,
            water_level: -1.0,
            cliff_level: 3.0,
            tier: Tier::Easy,
            tree: String::new(),
            density: 6.0,
            prop: String::new(),
            amber: 2500,
            stroke: None,
            selected: None,
        }
    }
}

/// What the select tool picked.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Picked {
    Start,
    Entity,
    Object,
}

/// A brush stroke in progress: the grids it changed as they were, and the vertices it
/// touched.
#[derive(Debug, Clone)]
pub struct Stroke {
    pub tool: Tool,
    /// Grid file → its bytes before the stroke.
    pub before: BTreeMap<String, Vec<u8>>,
    /// Touched vertices: min x, min z, max x, max z (inclusive).
    pub area: Option<[u32; 4]>,
    /// The flatten height, and the cliff level the stroke lifts to.
    pub target: f32,
    pub lower: bool,
    /// Trees planted (scenery objects) and tree ids cleared.
    pub planted: Vec<Value>,
    pub cleared: Vec<String>,
    /// For trees: the cells already tried.
    pub tried: std::collections::HashSet<(i32, i32)>,
    /// For ramps: where the drag began, and where it is now.
    pub from: Option<[f32; 2]>,
    pub to: Option<[f32; 2]>,
}

/// The grids of a map's terrain by file, mutably.
pub fn grid_mut<'a>(map: &'a mut UtcMap, file: &str) -> Option<&'a mut Grid> {
    let terrain = &mut map.authoring.terrain;
    match file {
        "terrain/heights.grid" => Some(&mut terrain.heights),
        "terrain/water-heights.grid" => Some(&mut terrain.water_heights),
        "terrain/grass.grid" => Some(&mut terrain.grass),
        "terrain/rock.grid" => Some(&mut terrain.rock),
        _ => {
            let n: usize = file
                .strip_prefix("terrain/paint-")?
                .strip_suffix("-weights.grid")?
                .parse()
                .ok()?;
            terrain
                .paint
                .get_mut(n.checked_sub(1)?)
                .map(|l| &mut l.weights)
        }
    }
}

fn read(bytes: &[u8], i: usize) -> f32 {
    f32::from_le_bytes([
        bytes[i * 4],
        bytes[i * 4 + 1],
        bytes[i * 4 + 2],
        bytes[i * 4 + 3],
    ])
}

fn write(bytes: &mut [u8], i: usize, v: f32) {
    bytes[i * 4..i * 4 + 4].copy_from_slice(&v.to_le_bytes());
}

/// The cliff paint layer: the first painted material whose asset projects onto cliffs.
pub fn cliff_layer(map: &UtcMap, content: &Content) -> Option<usize> {
    map.authoring.terrain.paint.iter().position(|layer| {
        content
            .landscape
            .iter()
            .find(|a| a.id.as_str() == layer.material)
            .and_then(|a| a.terrain.as_ref())
            .is_some_and(|t| t.projection.is_some())
    })
}

impl Stroke {
    pub fn new(tool: Tool, target: f32, lower: bool) -> Self {
        Self {
            tool,
            before: BTreeMap::new(),
            area: None,
            target,
            lower,
            planted: Vec::new(),
            cleared: Vec::new(),
            tried: Default::default(),
            from: None,
            to: None,
        }
    }

    /// Remembers `file` as it was before its first change in this stroke.
    fn keep(&mut self, map: &mut UtcMap, file: &str) {
        if !self.before.contains_key(file)
            && let Some(grid) = grid_mut(map, file)
        {
            self.before.insert(file.to_string(), grid.0.clone());
        }
    }

    fn touch(&mut self, x0: u32, z0: u32, x1: u32, z1: u32) {
        self.area = Some(match self.area {
            None => [x0, z0, x1, z1],
            Some([a, b, c, d]) => [a.min(x0), b.min(z0), c.max(x1), d.max(z1)],
        });
    }

    /// The patches that turn the grids as they were into what the stroke made, over the
    /// touched rectangle; the session's map holds the result.
    pub fn patches(&self, map: &mut UtcMap, root: &str) -> Vec<Command> {
        let Some([x0, z0, x1, z1]) = self.area else {
            return Vec::new();
        };
        let side = map.authoring.terrain.vertices();
        let (width, height) = (x1 - x0 + 1, z1 - z0 + 1);
        let mut out = Vec::new();
        for (file, before) in &self.before {
            let Some(grid) = grid_mut(map, file) else {
                continue;
            };
            if grid.0 == *before {
                continue;
            }
            let mut bytes = Vec::with_capacity((width * height * 4) as usize);
            for z in z0..=z1 {
                let at = (z * side + x0) as usize * 4;
                bytes.extend_from_slice(&grid.0[at..at + width as usize * 4]);
            }
            out.push(Command::Patch {
                document: format!("{root}/{file}"),
                x: x0,
                z: z0,
                width,
                height,
                bytes,
            });
        }
        out
    }
}

/// Vertices under a brush: (index, x, z, falloff), and their bounds.
type Footprint = (Vec<(usize, u32, u32, f32)>, [u32; 4]);

/// The vertices within `radius` of world (`cx`, `cz`) and their falloff (1 at the centre,
/// 0 at the rim).
fn footprint(map: &UtcMap, cx: f32, cz: f32, radius: f32) -> Footprint {
    let side = map.authoring.terrain.vertices();
    let halo = MAP_HALO as f32;
    let (vx, vz) = (cx + halo, cz + halo);
    let r = radius.max(0.5);
    let x0 = ((vx - r).floor().max(0.0)) as u32;
    let z0 = ((vz - r).floor().max(0.0)) as u32;
    let x1 = ((vx + r).ceil() as u32).min(side - 1);
    let z1 = ((vz + r).ceil() as u32).min(side - 1);
    let mut out = Vec::new();
    for z in z0..=z1 {
        for x in x0..=x1 {
            let d = ((x as f32 - vx).powi(2) + (z as f32 - vz).powi(2)).sqrt() / r;
            if d < 1.0 {
                let f = (1.0 - d * d).powi(2);
                out.push(((z * side + x) as usize, x, z, f));
            }
        }
    }
    (out, [x0, z0, x1, z1])
}

/// The ground height at world (`x`, `z`), bilinear between vertices.
pub fn ground_at(map: &UtcMap, x: f32, z: f32) -> f32 {
    let side = map.authoring.terrain.vertices() as i64;
    let bytes = &map.authoring.terrain.heights.0;
    let (vx, vz) = (x + MAP_HALO as f32, z + MAP_HALO as f32);
    let (ix, iz) = (vx.floor() as i64, vz.floor() as i64);
    let (fx, fz) = (vx - ix as f32, vz - iz as f32);
    let at = |x: i64, z: i64| {
        let (x, z) = (x.clamp(0, side - 1), z.clamp(0, side - 1));
        read(bytes, (z * side + x) as usize)
    };
    let top = at(ix, iz) * (1.0 - fx) + at(ix + 1, iz) * fx;
    let bottom = at(ix, iz + 1) * (1.0 - fx) + at(ix + 1, iz + 1) * fx;
    top * (1.0 - fz) + bottom * fz
}

/// One brush application at world (`cx`, `cz`) over `dt` seconds.
pub fn brush(
    map: &mut UtcMap,
    tools: &Tools,
    stroke: &mut Stroke,
    cx: f32,
    cz: f32,
    dt: f32,
    cliff: Option<usize>,
) {
    let (cells, [x0, z0, x1, z1]) = footprint(map, cx, cz, tools.radius);
    if cells.is_empty() {
        return;
    }
    let rate = (tools.strength * dt).clamp(0.0, 1.0);
    let (lo, hi) = (HEIGHT_MIN as f32, HEIGHT_MAX as f32);
    match stroke.tool {
        Tool::Raise | Tool::Lower => {
            stroke.keep(map, "terrain/heights.grid");
            let sign = if (stroke.tool == Tool::Lower) != stroke.lower {
                -1.0
            } else {
                1.0
            };
            let grid = &mut map.authoring.terrain.heights.0;
            for &(i, _, _, f) in &cells {
                let h = read(grid, i) + sign * 8.0 * rate * f;
                write(grid, i, h.clamp(lo, hi));
            }
        }
        Tool::Flatten | Tool::Cliff => {
            stroke.keep(map, "terrain/heights.grid");
            let hard = stroke.tool == Tool::Cliff;
            let grid = &mut map.authoring.terrain.heights.0;
            for &(i, _, _, f) in &cells {
                let h = read(grid, i);
                let next = if hard {
                    if f > 0.15 { stroke.target } else { h }
                } else {
                    h + (stroke.target - h) * (4.0 * rate * f).min(1.0)
                };
                write(grid, i, next.clamp(lo, hi));
            }
        }
        Tool::Smooth => {
            stroke.keep(map, "terrain/heights.grid");
            let side = map.authoring.terrain.vertices() as usize;
            let grid = &mut map.authoring.terrain.heights.0;
            let old = grid.clone();
            for &(i, x, z, f) in &cells {
                let (x, z) = (x as usize, z as usize);
                let mut sum = 0.0;
                let mut n = 0.0;
                for dz in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (nx, nz) = (x as i64 + dx, z as i64 + dz);
                        if nx >= 0 && nz >= 0 && (nx as usize) < side && (nz as usize) < side {
                            sum += read(&old, nz as usize * side + nx as usize);
                            n += 1.0;
                        }
                    }
                }
                let h = read(&old, i);
                write(grid, i, h + (sum / n - h) * (6.0 * rate * f).min(1.0));
            }
        }
        Tool::Water => {
            stroke.keep(map, "terrain/water-heights.grid");
            let level = if stroke.lower {
                DRY_WATER_HEIGHT as f32
            } else {
                tools.water_level
            };
            let grid = &mut map.authoring.terrain.water_heights.0;
            for &(i, _, _, f) in &cells {
                if f > 0.1 {
                    write(grid, i, level.clamp(DRY_WATER_HEIGHT as f32, hi));
                }
            }
        }
        Tool::Paint => paint(map, tools.ground, stroke, &cells, rate, cliff),
        _ => return,
    }
    stroke.touch(x0, z0, x1, z1);
    if matches!(
        stroke.tool,
        Tool::Raise | Tool::Lower | Tool::Flatten | Tool::Smooth | Tool::Cliff
    ) && let Some(layer) = cliff
    {
        mark_cliffs(map, stroke, layer, [x0, z0, x1, z1]);
    }
}

/// A straight ramp from world `a` at height `ha` to `b` at `hb`, `width` cells across:
/// the ground along it is set to the slope between them (blending into its sides over four
/// cells), and the cliff faces it cuts through are cleared.
#[allow(clippy::too_many_arguments)]
pub fn ramp(
    map: &mut UtcMap,
    stroke: &mut Stroke,
    a: [f32; 2],
    ha: f32,
    b: [f32; 2],
    hb: f32,
    width: f32,
    cliff: Option<usize>,
) {
    let side = map.authoring.terrain.vertices();
    let halo = MAP_HALO as f32;
    let half = width / 2.0;
    let reach = half + 4.0;
    let (dx, dz) = (b[0] - a[0], b[1] - a[1]);
    let length2 = (dx * dx + dz * dz).max(1e-6);
    let lo = |v: f32| ((v - reach + halo).floor().max(0.0)) as u32;
    let hi = |v: f32| ((v + reach + halo).ceil().max(0.0) as u32).min(side - 1);
    let (x0, x1) = (lo(a[0].min(b[0])), hi(a[0].max(b[0])));
    let (z0, z1) = (lo(a[1].min(b[1])), hi(a[1].max(b[1])));
    if x0 > x1 || z0 > z1 {
        return;
    }
    stroke.keep(map, "terrain/heights.grid");
    let grid = &mut map.authoring.terrain.heights.0;
    for z in z0..=z1 {
        for x in x0..=x1 {
            let (wx, wz) = (x as f32 - halo, z as f32 - halo);
            let t = (((wx - a[0]) * dx + (wz - a[1]) * dz) / length2).clamp(0.0, 1.0);
            let (px, pz) = (a[0] + dx * t, a[1] + dz * t);
            let d = ((wx - px).powi(2) + (wz - pz).powi(2)).sqrt();
            if d > reach {
                continue;
            }
            let f = if d <= half {
                1.0
            } else {
                1.0 - (d - half) / 4.0
            };
            let i = (z * side + x) as usize;
            let h = read(grid, i);
            write(grid, i, h + (ha + (hb - ha) * t - h) * f);
        }
    }
    stroke.touch(x0, z0, x1, z1);
    if let Some(layer) = cliff {
        mark_cliffs(map, stroke, layer, [x0, z0, x1, z1]);
    }
}

/// Paints `ground` over `cells`: its own weight rises, and what would cover it falls.
fn paint(
    map: &mut UtcMap,
    ground: Ground,
    stroke: &mut Stroke,
    cells: &[(usize, u32, u32, f32)],
    rate: f32,
    cliff: Option<usize>,
) {
    let layers = map.authoring.terrain.paint.len();
    // Paint layers other than the cliff faces (those follow the slopes).
    let covering: Vec<usize> = (0..layers)
        .filter(|&n| Some(n) != cliff)
        .filter(|&n| match ground {
            Ground::Paint(k) => n > k,
            _ => true,
        })
        .collect();
    let mut up: Vec<String> = Vec::new();
    let mut down: Vec<String> = covering
        .iter()
        .map(|n| format!("terrain/paint-{}-weights.grid", n + 1))
        .collect();
    match ground {
        Ground::Paint(k) => up.push(format!("terrain/paint-{}-weights.grid", k + 1)),
        Ground::Grass => {
            up.push("terrain/grass.grid".into());
            down.push("terrain/rock.grid".into());
        }
        Ground::Rock => up.push("terrain/rock.grid".into()),
        Ground::Soil => {
            down.push("terrain/grass.grid".into());
            down.push("terrain/rock.grid".into());
        }
    }
    for (file, rising) in up
        .iter()
        .map(|f| (f, true))
        .chain(down.iter().map(|f| (f, false)))
    {
        stroke.keep(map, file);
        let Some(grid) = grid_mut(map, file) else {
            continue;
        };
        for &(i, _, _, f) in cells {
            let a = (6.0 * rate * f).min(1.0);
            let w = read(&grid.0, i);
            let next = if rising {
                w + (1.0 - w) * a
            } else {
                w * (1.0 - a)
            };
            write(&mut grid.0, i, next.clamp(0.0, 1.0));
        }
    }
}

/// Cliff faces follow the ground: the cliff layer is full where a neighbouring step is
/// a cliff and empty elsewhere, within `area` (one vertex wider).
pub fn mark_cliffs(map: &mut UtcMap, stroke: &mut Stroke, layer: usize, area: [u32; 4]) {
    let file = format!("terrain/paint-{}-weights.grid", layer + 1);
    stroke.keep(map, &file);
    let side = map.authoring.terrain.vertices();
    let heights = map.authoring.terrain.heights.0.clone();
    let [x0, z0, x1, z1] = [
        area[0].saturating_sub(1),
        area[1].saturating_sub(1),
        (area[2] + 1).min(side - 1),
        (area[3] + 1).min(side - 1),
    ];
    let Some(grid) = grid_mut(map, &file) else {
        return;
    };
    for z in z0..=z1 {
        for x in x0..=x1 {
            let i = (z * side + x) as usize;
            let h = read(&heights, i);
            let mut steep = false;
            for (dx, dz) in [(-1i64, 0i64), (1, 0), (0, -1), (0, 1)] {
                let (nx, nz) = (x as i64 + dx, z as i64 + dz);
                if nx < 0 || nz < 0 || nx >= side as i64 || nz >= side as i64 {
                    continue;
                }
                if (read(&heights, (nz * side as i64 + nx) as usize) - h).abs() > CLIFF_STEP {
                    steep = true;
                }
            }
            write(&mut grid.0, i, if steep { 1.0 } else { 0.0 });
        }
    }
    stroke.touch(x0, z0, x1, z1);
}

/// A small deterministic hash in 0–1.
fn noise(a: i64, b: i64, salt: u64) -> f32 {
    let mut x = (a as u64).wrapping_mul(0x9e37_79b9_7f4a_7c15)
        ^ (b as u64).wrapping_mul(0xc2b2_ae3d_27d4_eb4f)
        ^ salt.wrapping_mul(0x1656_67b1_9e37_79f9);
    x ^= x >> 31;
    x = x.wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x ^= x >> 29;
    (x & 0xffff) as f32 / 65535.0
}

/// The next free id `prefix<n>` among `taken`.
fn next_id(prefix: &str, taken: impl Iterator<Item = String>) -> u64 {
    taken
        .filter_map(|id| {
            id.strip_prefix(prefix)?
                .split('.')
                .next()?
                .parse::<u64>()
                .ok()
        })
        .max()
        .map_or(0, |n| n + 1)
}

/// The scenery the tree brush can plant: the package's harvestable trees.
pub fn tree_kinds(content: &Content) -> Vec<String> {
    canopy_content::assets::scenery(&content.manifest)
        .filter(|s| {
            content.landscape.iter().any(|a| {
                a.scenery
                    .as_ref()
                    .is_some_and(|i| i.as_str() == s.id.as_str())
                    && a.harvesting.is_some()
            })
        })
        .map(|s| s.id.to_string())
        .collect()
}

/// The scenery the props brush can scatter: decoration that is no tree, no bridge deck and
/// not retired, and blocks nothing.
pub fn prop_kinds(content: &Content) -> Vec<String> {
    let trees = tree_kinds(content);
    canopy_content::assets::scenery(&content.manifest)
        .filter(|s| {
            s.deck.is_none()
                && s.blocker.is_none()
                && s.blockers.is_none()
                && s.editor_hidden != Some(true)
                && !s.id.as_str().contains("missing")
                && !trees.iter().any(|t| t == s.id.as_str())
        })
        .map(|s| s.id.to_string())
        .collect()
}

/// Plants trees (or, for the props brush, decoration) over the brush, one per cell,
/// spaced, on dry flat-enough ground; or clears them (Shift). The session's map shows them
/// at once.
pub fn trees(map: &mut UtcMap, tools: &Tools, stroke: &mut Stroke, cx: f32, cz: f32) {
    let r = tools.radius;
    let props = stroke.tool == Tool::Props;
    if stroke.lower {
        let before = map.authoring.objects.len();
        let mut cleared = Vec::new();
        map.authoring.objects.retain(|o| {
            let inside = (o.x as f32 - cx).powi(2) + (o.z as f32 - cz).powi(2) <= r * r
                && o.id.starts_with("prop.") == props;
            if inside {
                cleared.push(o.id.clone());
            }
            !inside
        });
        if map.authoring.objects.len() != before {
            stroke.cleared.extend(cleared);
        }
        return;
    }
    let (asset, prefix, spacing) = if props {
        (&tools.prop, "prop.", 12.0)
    } else {
        (&tools.tree, "tree.", 3.0)
    };
    if asset.is_empty() {
        return;
    }
    let size = map.size as f32;
    let mut next = next_id(
        prefix,
        map.authoring.objects.iter().map(|o| o.id.clone()).chain(
            stroke
                .planted
                .iter()
                .filter_map(|v| v["id"].as_str().map(str::to_string)),
        ),
    );
    let chance = (tools.density / 100.0).clamp(0.0, 1.0) * 4.0;
    for z in (cz - r).floor() as i32..=(cz + r).ceil() as i32 {
        for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
            if (x as f32 - cx).powi(2) + (z as f32 - cz).powi(2) > r * r
                || !stroke.tried.insert((x, z))
                || x < 2
                || z < 2
                || x as f32 > size - 3.0
                || z as f32 > size - 3.0
                || noise(x as i64, z as i64, if props { 17 } else { 7 }) > chance.min(1.0) * 0.25
            {
                continue;
            }
            let (fx, fz) = (x as f32, z as f32);
            // Not in water, not on a cliff, not next to another tree.
            let ground = ground_at(map, fx, fz);
            let water = water_at(map, fx, fz);
            let steep = [(1.0, 0.0), (-1.0, 0.0), (0.0, 1.0), (0.0, -1.0)]
                .iter()
                .any(|(dx, dz)| (ground_at(map, fx + dx, fz + dz) - ground).abs() > CLIFF_STEP);
            let crowded = map
                .authoring
                .objects
                .iter()
                .any(|o| (o.x as f32 - fx).powi(2) + (o.z as f32 - fz).powi(2) < spacing);
            if water > ground - 0.2 || steep || crowded {
                continue;
            }
            let object = json!({
                "asset": asset,
                "elevation": 0.0,
                "heightMode": "terrain",
                "id": format!("{prefix}{next}"),
                "locked": false,
                "scale": 0.82 + 0.2 * noise(x as i64, z as i64, 11),
                "visible": true,
                "x": fx,
                "yaw": (noise(x as i64, z as i64, 13) * std::f32::consts::TAU) as f64,
                "z": fz,
            });
            next += 1;
            if let Ok(o) = serde_json::from_value(object.clone()) {
                map.authoring.objects.push(o);
            }
            stroke.planted.push(object);
        }
    }
}

/// The water surface at world (`x`, `z`) (nearest vertex).
pub fn water_at(map: &UtcMap, x: f32, z: f32) -> f32 {
    let side = map.authoring.terrain.vertices() as i64;
    let vx = ((x + MAP_HALO as f32).round() as i64).clamp(0, side - 1);
    let vz = ((z + MAP_HALO as f32).round() as i64).clamp(0, side - 1);
    read(
        &map.authoring.terrain.water_heights.0,
        (vz * side + vx) as usize,
    )
}

/// The commands that keep a tree stroke: planted trees appended to `scenery.json`,
/// cleared ones removed (by their place in the document `objects`).
pub fn tree_commands(root: &str, stroke: &Stroke, objects: &[Value]) -> Vec<Command> {
    let document = format!("{root}/scenery.json");
    let mut out = Vec::new();
    let mut indices: Vec<usize> = stroke
        .cleared
        .iter()
        .filter_map(|id| objects.iter().position(|o| o["id"] == *id))
        .collect();
    indices.sort_unstable_by(|a, b| b.cmp(a));
    indices.dedup();
    for i in &indices {
        out.push(Command::Remove {
            document: document.clone(),
            pointer: format!("/objects/{i}"),
        });
    }
    for (at, object) in (objects.len() - indices.len()..).zip(&stroke.planted) {
        out.push(Command::Insert {
            document: document.clone(),
            pointer: "/objects".into(),
            index: at,
            value: object.clone(),
        });
    }
    out
}

/// A building position on its 0.5-cell lattice.
/// Five amber nodes in an arc around world (`cx`, `cz`), its hollow facing (`fx`, `fz`)
/// (the hall), as `map.json` entities.
pub fn amber_site(
    map: &UtcMap,
    content: &Content,
    cx: f64,
    cz: f64,
    toward: (f64, f64),
    amount: u32,
) -> Vec<Value> {
    let Some(mine) = content
        .definitions
        .iter()
        .find(|d| d.kind == Kind::Building && d.harvesting.is_some() && d.resource_yield.is_some())
    else {
        return Vec::new();
    };
    // The mine's look and its numbered variants (`…-1`, `…-2`, …).
    let stem = mine
        .model
        .as_str()
        .trim_end_matches(|c: char| c.is_ascii_digit());
    let crystals: Vec<String> = content
        .assets
        .iter()
        .map(|a| a.id.as_str().to_string())
        .filter(|id| stem.ends_with('-') && id.starts_with(stem))
        .collect();
    // On the building lattice, the hollow turned to the nearest of eight directions
    // towards the hall: an arc across it when straight, a bent line when diagonal.
    let (cx, cz) = start_position(cx, cz);
    let angle = (toward.1 - cz).atan2(toward.0 - cx);
    let octant = ((angle / std::f64::consts::FRAC_PI_4).round() as i64).rem_euclid(8);
    let offsets = site_offsets(octant as usize);
    let site = next_id("mine.", map.entities.iter().map(|e| e.id.clone()));
    offsets
        .iter()
        .enumerate()
        .map(|(k, (a, b))| {
            let (x, z) = (cx + a, cz + b);
            let mut node = json!({
                "definition": mine.id.as_str(),
                "id": format!("mine.{site}.{k}"),
                "initialState": {"amount": amount, "construction": "complete"},
                "owner": "none",
                "position": {"x": x, "y": z},
                "rotation": 0.0,
            });
            if !crystals.is_empty() {
                node["appearance"] = json!({"asset": crystals[k % crystals.len()]});
            }
            node
        })
        .collect()
}

/// The five nodes of an amber site around its centre, in cells, for the hall in direction
/// `octant` (0 east, counting towards +z in eighths of a turn): multiples of the 4-cell
/// building lattice, an arc across a straight direction and a bent line across a diagonal.
fn site_offsets(octant: usize) -> Vec<(f64, f64)> {
    let (sx, sz) = [
        (1, 0),
        (1, 1),
        (0, 1),
        (-1, 1),
        (-1, 0),
        (-1, -1),
        (0, -1),
        (1, -1),
    ][octant % 8];
    let (sx, sz) = (f64::from(sx), f64::from(sz));
    // u = (sx, sz) points at the hall; p across it.
    let (px, pz) = (-sz, sx);
    if sx == 0.0 || sz == 0.0 {
        [
            (-8.0, 0.0),
            (-4.0, -4.0),
            (0.0, -4.0),
            (4.0, -4.0),
            (8.0, 0.0),
        ]
        .iter()
        .map(|(a, b)| (px * a + sx * b, pz * a + sz * b))
        .collect()
    } else {
        vec![
            (-8.0 * px, -8.0 * pz + 4.0 * sz),
            (-4.0 * px, -4.0 * pz),
            (0.0, 0.0),
            (4.0 * px, 4.0 * pz),
            (8.0 * px + 4.0 * sx, 8.0 * pz),
        ]
    }
}

/// A creep camp of `tier` at world (`cx`, `cz`): creeps (units that defend camps)
/// whose levels add up to the tier's range, picked by the place for variety.
pub fn camp(map: &UtcMap, content: &Content, cx: f64, cz: f64, tier: Tier) -> (Vec<Value>, Value) {
    let mut creeps: Vec<(&str, u32)> = content
        .definitions
        .iter()
        .filter(|d| d.kind == Kind::Unit && d.behaviors.camp_defense.is_some())
        .map(|d| (d.id.as_str(), d.level.unwrap_or(1).max(1)))
        .collect();
    creeps.sort_by_key(|c| (c.1, c.0));
    let (low, high) = tier.levels();
    let seed = (cx as i64) * 31 + cz as i64;
    // A leader as strong as the tier allows, then followers of its level or lower.
    let leaders: Vec<&(&str, u32)> = creeps.iter().filter(|c| c.1 <= high / 2 + 1).collect();
    let mut members: Vec<&str> = Vec::new();
    let mut total = 0;
    if let Some(leader) = leaders
        .iter()
        .rev()
        .nth((noise(seed, 1, 3) * leaders.len().min(4) as f32) as usize % leaders.len().max(1))
    {
        members.push(leader.0);
        total += leader.1;
        let followers: Vec<&(&str, u32)> = creeps.iter().filter(|c| c.1 <= leader.1).collect();
        let follower =
            followers[(noise(seed, 2, 5) * followers.len() as f32) as usize % followers.len()];
        while total < low && members.len() < 5 {
            let pick = if total + follower.1 <= high {
                follower
            } else {
                &creeps[0]
            };
            members.push(pick.0);
            total += pick.1;
        }
    }
    let n = next_id("camp.", map.camps.iter().map(|c| c.id.clone()));
    let id = format!("camp.{n}");
    let count = members.len().max(1) as f64;
    let entities: Vec<Value> = members
        .iter()
        .enumerate()
        .map(|(k, definition)| {
            let angle = k as f64 / count * std::f64::consts::TAU;
            let r = if members.len() == 1 { 0.0 } else { 2.5 };
            json!({
                "definition": definition,
                "id": format!("{id}/{}", k + 1),
                "owner": "none",
                "position": {"x": (cx + angle.cos() * r).round(), "y": (cz + angle.sin() * r).round()},
                "rotation": (angle.to_degrees() + 180.0).rem_euclid(360.0).round(),
            })
        })
        .collect();
    let pool = content
        .rules
        .loot_pools
        .keys()
        .find(|p| p.as_str().ends_with(tier.name()))
        .or_else(|| content.rules.loot_pools.keys().next())
        .map(|p| p.to_string());
    let mut camp = json!({
        "aggression": "players",
        "aggroRange": 8,
        "home": {"x": cx.round(), "y": cz.round()},
        "id": id,
        "leash": 18,
        "members": entities.iter().map(|e| e["id"].clone()).collect::<Vec<_>>(),
    });
    if let Some(pool) = pool {
        camp["lootPool"] = json!(pool);
    }
    (entities, camp)
}

/// A start position on the hall's lattice (its 20-cell footprint's corner on the 4-cell
/// building grid anchored at -0.5).
pub fn start_position(x: f64, z: f64) -> (f64, f64) {
    let snap = |v: f64| ((v - 1.5) / 4.0).round() * 4.0 + 1.5;
    (snap(x), snap(z))
}

/// The cells within `radius` of world (`cx`, `cz`) levelled to their mean height: a
/// start's pad, so the hall and its workers stand on buildable ground.
pub fn level_pad(map: &mut UtcMap, stroke: &mut Stroke, cx: f32, cz: f32, radius: f32) {
    let (cells, area) = footprint(map, cx, cz, radius);
    if cells.is_empty() {
        return;
    }
    stroke.keep(map, "terrain/heights.grid");
    let grid = &mut map.authoring.terrain.heights.0;
    let mean = cells.iter().map(|&(i, ..)| read(grid, i)).sum::<f32>() / cells.len() as f32;
    for &(i, _, _, f) in &cells {
        let h = read(grid, i);
        write(grid, i, h + (mean - h) * (f * 3.0).min(1.0));
    }
    stroke.touch(area[0], area[1], area[2], area[3]);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A flat 256-cell map at height `h`, dry.
    fn flat(h: f32) -> UtcMap {
        let mut map: UtcMap = serde_json::from_value(json!({
            "v": canopy_content::map::UTCMAP_VERSION,
            "name": "Test",
            "size": 256,
            "playerStarts": [],
            "entities": [],
            "camps": [],
            "authoring": {"version": 1, "objects": [], "terrain": {
                "version": 1, "size": 256, "heights": "", "waterHeights": "", "waterFlow": "",
                "waterProfiles": [], "grass": "", "rock": "", "paint": [
                    {"material": "cliff", "weights": ""}
                ]
            }}
        }))
        .expect("a test map");
        let side = map.authoring.terrain.vertices() as usize;
        let floats = |v: f32| {
            Grid(
                std::iter::repeat_n(v, side * side)
                    .flat_map(f32::to_le_bytes)
                    .collect(),
            )
        };
        map.authoring.terrain.heights = floats(h);
        map.authoring.terrain.water_heights = floats(DRY_WATER_HEIGHT as f32);
        map.authoring.terrain.grass = floats(0.0);
        map.authoring.terrain.rock = floats(0.0);
        map.authoring.terrain.paint[0].weights = floats(0.0);
        map
    }

    #[test]
    fn amber_sites_stay_on_the_lattice_without_overlapping_and_open_to_the_hall() {
        for octant in 0..8 {
            let nodes = site_offsets(octant);
            assert_eq!(nodes.len(), 5);
            for (i, a) in nodes.iter().enumerate() {
                assert_eq!((a.0 % 4.0, a.1 % 4.0), (0.0, 0.0), "octant {octant}: {a:?}");
                for b in &nodes[i + 1..] {
                    // 4×4 footprints: apart by a whole footprint on some axis.
                    assert!(
                        (a.0 - b.0).abs() >= 4.0 || (a.1 - b.1).abs() >= 4.0,
                        "octant {octant}"
                    );
                }
            }
            // The middle of the site lies behind its ends, seen from the hall.
            let angle = octant as f64 * std::f64::consts::FRAC_PI_4;
            let along = |p: &(f64, f64)| p.0 * angle.cos() + p.1 * angle.sin();
            assert!(along(&nodes[2]) < along(&nodes[0]) && along(&nodes[2]) < along(&nodes[4]));
        }
    }

    #[test]
    fn props_and_trees_are_planted_apart_and_cleared_apart() {
        let mut map = flat(1.0);
        let tools = Tools {
            tree: "scenery.pine".into(),
            prop: "scenery.boulder".into(),
            density: 25.0,
            radius: 12.0,
            ..Tools::default()
        };
        let plant = |map: &mut UtcMap, tool: Tool, lower: bool, x: f32| {
            let mut stroke = Stroke::new(tool, 0.0, lower);
            trees(map, &tools, &mut stroke, x, 100.0);
            stroke
        };
        // Trees at x 100, props beside them at x 120: the brushes overlap in between.
        let planted = plant(&mut map, Tool::Trees, false, 100.0).planted.len();
        let scattered = plant(&mut map, Tool::Props, false, 120.0).planted.len();
        assert!(
            planted > 0 && scattered > 0,
            "{planted} trees, {scattered} props"
        );
        // Each brush clears only its own kind, over both.
        assert!(
            plant(&mut map, Tool::Trees, true, 110.0)
                .cleared
                .iter()
                .all(|i| i.starts_with("tree."))
        );
        let cleared = plant(&mut map, Tool::Props, true, 110.0).cleared;
        assert!(!cleared.is_empty() && cleared.iter().all(|i| i.starts_with("prop.")));
    }

    #[test]
    fn a_raise_stroke_becomes_one_patch_per_grid_it_changed() {
        let mut map = flat(1.0);
        let tools = Tools::default();
        let mut stroke = Stroke::new(Tool::Raise, 0.0, false);
        for _ in 0..10 {
            brush(&mut map, &tools, &mut stroke, 100.0, 100.0, 0.1, None);
        }
        assert!(ground_at(&map, 100.0, 100.0) > 3.0);
        assert!(
            (ground_at(&map, 130.0, 130.0) - 1.0).abs() < 1e-6,
            "outside the brush"
        );
        let patches = stroke.patches(&mut map, "maps/t.utcm");
        assert_eq!(patches.len(), 1);
        let Command::Patch {
            document, x, width, ..
        } = &patches[0]
        else {
            panic!("a patch")
        };
        assert_eq!(document, "maps/t.utcm/terrain/heights.grid");
        // The brush's 6-cell radius around vertex 116 (world 100 + the 16-cell halo).
        assert_eq!((*x, *width), (110, 13));
    }

    #[test]
    fn a_cliff_stroke_lifts_a_level_and_paints_its_faces() {
        let mut map = flat(0.0);
        let tools = Tools::default();
        let mut stroke = Stroke::new(Tool::Cliff, 3.0, false);
        brush(&mut map, &tools, &mut stroke, 100.0, 100.0, 0.1, Some(0));
        assert_eq!(ground_at(&map, 100.0, 100.0), 3.0);
        let side = map.authoring.terrain.vertices() as usize;
        let weights = &map.authoring.terrain.paint[0].weights.0;
        let at = |x: usize, z: usize| read(weights, z * side + x);
        // The centre is flat (no face); the rim, where the level drops, is a face.
        assert_eq!(at(116, 116), 0.0);
        let rim = (100..132).any(|x| at(x, 116) == 1.0);
        assert!(rim, "cliff faces along the edge");
        assert_eq!(stroke.patches(&mut map, "m").len(), 2);
    }

    #[test]
    fn painting_raises_its_layer_and_lowers_what_covers_it() {
        let mut map = flat(0.0);
        let tools = Tools {
            ground: Ground::Grass,
            strength: 1.0,
            ..Tools::default()
        };
        let mut stroke = Stroke::new(Tool::Paint, 0.0, false);
        for _ in 0..20 {
            brush(&mut map, &tools, &mut stroke, 50.0, 50.0, 0.1, Some(0));
        }
        let side = map.authoring.terrain.vertices() as usize;
        assert!(read(&map.authoring.terrain.grass.0, 66 * side + 66) > 0.9);
        // The cliff layer is not paint the brush covers.
        assert!(!stroke.before.contains_key("terrain/paint-1-weights.grid"));
    }

    #[test]
    fn starts_snap_to_the_hall_lattice() {
        assert_eq!(start_position(117.0, 226.4), (117.5, 225.5));
        assert_eq!(start_position(500.0, 500.0), (501.5, 501.5));
    }
}
