//! The block editor (D80): a map is shaped as columns of blocks, 4 × 4 cells each, and
//! its heights, water and closed cells are made from them: raise and lower, set a level,
//! paint a kind, ramps dragged from top to foot, diagonal cuts, kinds painted cell by
//! cell, and the face each cliff takes. Block tools work on a dragged box or along a
//! brush stroke. The Generated view (G) shows the world the biome makes of the blocks.
//!
//! Every edit changes the blocks, makes the map's grids from them (`Blocks::apply`) and
//! becomes one undo step: the blocks and closed cells whole, heights and water as patches
//! over what changed.

pub mod catalog;
pub mod mesh;
pub mod palette;
pub mod settings;
mod strip;

use crate::document::{Command, Sample};
use crate::map::Session;
use crate::map::tools::Picked;
use crate::map::view::{Out, matrices, navigate, pick, project, step};
use crate::theme::*;
use canopy_content::map::MAP_HALO;
use canopy_content::map::blocks::{
    BLOCK, Block, BlockKind, Blocks, Cut, MAX_LEVEL, RAMP_DIRECTIONS, ramp_direction,
};
use egui::{Color32, Rect, RichText, Sense, Stroke as Line, Ui, pos2};
use glam::{Vec2, Vec3};

/// The block editor's tools.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BlockTool {
    /// Each block in the dragged area one level up (Shift: down).
    Raise,
    /// The dragged area to the level of the block the drag began on.
    Level,
    /// The dragged area to the chosen kind.
    Paint,
    /// A ramp dragged from its top to its foot (Shift: back to ground).
    Ramp,
    /// A diagonal cut at the clicked corner (again or Shift: removed).
    Cut,
    /// Kinds painted cell by cell (Shift: back to the block's own).
    Cells,
    /// The face the cliffs under the blocks take (Shift: the biome's own).
    Face,
    /// Starts, amber sites and creep camps, clicked where they go.
    Place,
    /// The corners of the blocks under the brush where a plateau's edge turns, cut at
    /// 45° so outlines round off (Shift: uncut).
    Round,
    /// Picks a start, an amber site, a camp or a placed prop: drag to move it, Delete to
    /// remove it.
    #[default]
    Select,
}

impl BlockTool {
    const ALL: [BlockTool; 10] = [
        BlockTool::Raise,
        BlockTool::Level,
        BlockTool::Paint,
        BlockTool::Ramp,
        BlockTool::Cut,
        BlockTool::Cells,
        BlockTool::Face,
        BlockTool::Place,
        BlockTool::Round,
        BlockTool::Select,
    ];

    pub fn named(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|t| t.name().eq_ignore_ascii_case(name))
    }

    fn name(self) -> &'static str {
        match self {
            BlockTool::Raise => "Raise",
            BlockTool::Level => "Level",
            BlockTool::Paint => "Paint",
            BlockTool::Ramp => "Ramp",
            BlockTool::Cut => "Cut",
            BlockTool::Cells => "Cells",
            BlockTool::Face => "Face",
            BlockTool::Place => "Place",
            BlockTool::Round => "Round",
            BlockTool::Select => "Select",
        }
    }

    fn key(self) -> egui::Key {
        match self {
            BlockTool::Raise => egui::Key::Q,
            BlockTool::Level => egui::Key::W,
            BlockTool::Paint => egui::Key::E,
            BlockTool::Ramp => egui::Key::R,
            BlockTool::Cut => egui::Key::T,
            BlockTool::Cells => egui::Key::C,
            BlockTool::Face => egui::Key::F,
            BlockTool::Place => egui::Key::P,
            BlockTool::Round => egui::Key::O,
            BlockTool::Select => egui::Key::S,
        }
    }

    fn hint(self) -> &'static str {
        match self {
            BlockTool::Raise => "Drag over blocks to raise them a level (Shift lowers)",
            BlockTool::Level => "Drag from a block: the area takes its level",
            BlockTool::Paint => "Drag over blocks to make them the chosen kind",
            BlockTool::Ramp => {
                "Drag from the top of a cliff down to its foot: the ramp falls that way, the brush wide (Shift removes)"
            }
            BlockTool::Cut => "Click near a block's corner to cut it diagonally (again removes)",
            BlockTool::Cells => {
                "Paint kinds cell by cell, for outlines at any angle (Shift clears)"
            }
            BlockTool::Face => {
                "Drag over the blocks above a cliff to choose its face (Shift: the biome's)"
            }
            BlockTool::Place => {
                "Click to place what the picker holds: a building, unit, camp, start, resource node or scenery"
            }
            BlockTool::Round => {
                "Drag over a plateau's edge to cut its corners at 45° (Shift uncuts)"
            }
            BlockTool::Select => {
                "Click to select (Shift adds), drag a rectangle round several; drag the selection to move it, Delete removes it"
            }
        }
    }

    /// Whether the tool works along a stroke rather than a box.
    fn strokes(self, brush: bool) -> bool {
        match self {
            BlockTool::Ramp | BlockTool::Cells => true,
            BlockTool::Cut | BlockTool::Place | BlockTool::Select => false,
            _ => brush,
        }
    }
}

/// What Place puts down (the catalog's entries, `catalog.rs`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placing {
    /// A unit or building: the Place tool's owner's, or no one's when it is neutral.
    Definition(String),
    /// A camp of creeps of this strength.
    Camp(crate::map::tools::Tier),
    /// Player `n`'s start (moved here when the map has one).
    Start(u8),
    /// One amber node.
    Amber,
    /// A piece of scenery.
    Scenery(String),
}

impl Default for Placing {
    fn default() -> Self {
        Placing::Start(1)
    }
}

/// A placement's twin in the mirror, made once the map has taken the first.
#[derive(Debug, Clone)]
pub struct Twin {
    pub placing: Placing,
    pub at: [f32; 2],
    pub owner: u8,
    /// The store generation the map was loaded at when the first was placed.
    pub after: Option<u64>,
}

/// Edits repeated in a mirror of the map, for fair maps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Symmetry {
    #[default]
    None,
    /// Turned half round the map's middle (two players across a diagonal).
    Point,
    /// Mirrored left to right.
    MirrorX,
    /// Mirrored top to bottom.
    MirrorZ,
}

impl Symmetry {
    const ALL: [Symmetry; 4] = [
        Symmetry::None,
        Symmetry::Point,
        Symmetry::MirrorX,
        Symmetry::MirrorZ,
    ];

    fn name(self) -> &'static str {
        match self {
            Symmetry::None => "Off",
            Symmetry::Point => "Point",
            Symmetry::MirrorX => "Left–right",
            Symmetry::MirrorZ => "Top–bottom",
        }
    }

    /// A world point's twin on a map of `size` cells.
    pub fn point(self, p: [f32; 2], size: u32) -> [f32; 2] {
        let s = size as f32;
        match self {
            Symmetry::None => p,
            Symmetry::Point => [s - p[0], s - p[1]],
            Symmetry::MirrorX => [s - p[0], p[1]],
            Symmetry::MirrorZ => [p[0], s - p[1]],
        }
    }

    /// A column's twin among `side` columns per side.
    fn column(self, c: (i32, i32), side: i32) -> (i32, i32) {
        match self {
            Symmetry::None => c,
            Symmetry::Point => (side - 1 - c.0, side - 1 - c.1),
            Symmetry::MirrorX => (side - 1 - c.0, c.1),
            Symmetry::MirrorZ => (c.0, side - 1 - c.1),
        }
    }

    /// `op` done in the mirror.
    fn op(self, op: &BlockOp, size: u32, side: i32) -> BlockOp {
        BlockOp {
            from: self.column(op.from, side),
            to: self.column(op.to, side),
            point: op.point.map(|p| self.point(p, size)),
            path: op.path.iter().map(|p| self.point(*p, size)).collect(),
            ..op.clone()
        }
    }
}

/// A drag in progress: the column it began on, the one it is over, and its stroke.
#[derive(Debug, Clone)]
pub struct Drag {
    pub from: (i32, i32),
    pub to: (i32, i32),
    pub path: Vec<[f32; 2]>,
}

/// The block editor's state in a map's session.
/// What the block view shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum View {
    /// The blocks themselves.
    #[default]
    Blocks,
    /// The world the biome generates from them.
    Generated,
    /// Each cell green where ground units can walk, grey where they cannot.
    Pathing,
    /// The blocks and the generated world side by side, one camera; the world follows
    /// the blocks a moment after the last edit.
    Split,
}

impl View {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "blocks" => Some(View::Blocks),
            "generated" => Some(View::Generated),
            "pathing" => Some(View::Pathing),
            "split" => Some(View::Split),
            _ => None,
        }
    }
}

/// Which panels round the map view show (the maps editor's own layout): the maps
/// library, and the map's panel on the right (its settings, or what is selected).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Panels {
    pub library: bool,
    pub properties: bool,
}

impl Default for Panels {
    fn default() -> Self {
        Self {
            library: false,
            properties: true,
        }
    }
}

impl Panels {
    fn id() -> egui::Id {
        egui::Id::new("map-panels")
    }

    pub fn get(ctx: &egui::Context) -> Self {
        ctx.data_mut(|d| d.get_temp(Self::id()).unwrap_or_default())
    }

    pub fn set(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(Self::id(), self));
    }
}

#[derive(Debug, Clone)]
pub struct State {
    pub tool: BlockTool,
    /// The kind Paint and Cells lay down.
    pub kind: BlockKind,
    pub cell_kind: BlockKind,
    /// The face Face gives.
    pub style: u8,
    /// Block tools follow a brush stroke instead of a box (B).
    pub brush: bool,
    /// Block tools and Cells fill the loop a drag draws (B again).
    pub lasso: bool,
    /// The brush's radius: blocks for block tools (0: one block), cells for Cells.
    pub radius: u8,
    pub cell_radius: u8,
    pub drag: Option<Drag>,
    /// What the view shows: the blocks, the world generated from them (G), or where
    /// ground units can walk (H).
    pub view: View,
    /// The view H and V return to from Pathing and Split.
    pub before: View,
    /// In the split view, whether the pointer works in the right pane (the generated
    /// world); kept through a drag.
    pub right_pane: bool,
    /// The names of the biome's faces (face style 1 onwards).
    pub faces: Vec<String>,
    pub placing: Placing,
    /// Who placed units and buildings belong to: 0 no one, else that player.
    pub owner: u8,
    /// Place and move what may stand anywhere (units, camps, scenery) cell by cell instead
    /// of block by block (X); buildings, starts and nodes keep the building lattice.
    pub fine: bool,
    /// What Place can put down, and where its picker stands: category, group, search.
    pub catalog: catalog::Catalog,
    pub place_category: usize,
    pub place_group: usize,
    pub place_search: String,
    /// What Select holds.
    pub selection: Vec<(Picked, String)>,
    /// The map panel's state, and the biomes its picker offers.
    pub settings: settings::State,
    pub biomes: Vec<settings::BiomeInfo>,
    pub symmetry: Symmetry,
    pub twin: Option<Twin>,
    /// While a drag is under way, the map shows what it will do: the terrain as it was
    /// before the drag, to go back to, and what the preview last showed.
    pub preview: Option<(Box<canopy_content::map::TerrainData>, String)>,
    /// Write the map picker's picture on the next frame (the panel's button).
    pub write_picture: bool,
    /// The pictures of the strip's pickers, for the map's biome.
    pub palette: palette::Palette,
    /// Open the chosen tool's picker on the next frame (captures).
    pub open_picker: bool,
}

impl State {
    /// The view after switching `mode` (Pathing, Split) on, or off back to the view
    /// before it.
    pub fn toggle(&mut self, mode: View) -> View {
        if self.view == mode {
            self.before
        } else {
            if !matches!(self.view, View::Pathing | View::Split) {
                self.before = self.view;
            }
            mode
        }
    }

    /// Shows `view`, rebuilding the scene when it changes.
    pub fn set_view(&mut self, view: View) -> bool {
        let changed = self.view != view;
        self.view = view;
        changed
    }
}

impl Default for State {
    fn default() -> Self {
        Self {
            tool: BlockTool::default(),
            kind: BlockKind::default(),
            cell_kind: BlockKind::Ice,
            style: 1,
            brush: false,
            lasso: false,
            radius: 1,
            cell_radius: 2,
            drag: None,
            view: View::Blocks,
            before: View::Blocks,
            right_pane: false,
            faces: Vec::new(),
            placing: Placing::default(),
            owner: 1,
            fine: false,
            catalog: catalog::Catalog::default(),
            place_category: 0,
            place_group: 0,
            place_search: String::new(),
            selection: Vec::new(),
            settings: settings::State::default(),
            biomes: Vec::new(),
            symmetry: Symmetry::default(),
            twin: None,
            preview: None,
            write_picture: false,
            palette: palette::Palette::default(),
            open_picker: false,
        }
    }
}

/// The map's blocks, when it has them.
pub fn blocks_of(session: &Session) -> Option<Blocks> {
    let map = session.map.as_ref()?;
    Blocks::of(&map.authoring.terrain)?.ok()
}

/// Changes the map's blocks with `change`, makes its grids from them and returns the undo
/// step; `blocks` is what the map holds now (or the blocks to start from).
fn commit(
    session: &mut Session,
    label: &str,
    mut blocks: Blocks,
    change: impl FnOnce(&mut Blocks),
) -> Option<crate::tabs::Edit> {
    let root = session.root.clone();
    let map = session.map.as_mut()?;
    let terrain = &mut map.authoring.terrain;
    let before = (
        terrain.heights.0.clone(),
        terrain.water_heights.0.clone(),
        terrain.blocks.clone(),
        terrain.block_paint.clone(),
    );
    change(&mut blocks);
    blocks.apply(terrain);
    if terrain.blocks == before.2 && terrain.block_paint == before.3 {
        return None;
    }
    let vertices = terrain.vertices();
    let span = terrain.span();
    let columns = span / BLOCK;
    let mut commands = vec![
        Command::Grid {
            document: format!("{root}/terrain/blocks.grid"),
            element: Sample::Rgba8,
            width: columns,
            height: columns,
            bytes: terrain.blocks.clone().unwrap_or_default().0,
        },
        Command::Grid {
            document: format!("{root}/terrain/blocked.grid"),
            element: Sample::U8,
            width: span,
            height: span,
            bytes: terrain.blocked.clone().unwrap_or_default().0,
        },
    ];
    if let Some(paint) = &terrain.block_paint {
        commands.push(Command::Grid {
            document: format!("{root}/terrain/block-paint.grid"),
            element: Sample::U8,
            width: span,
            height: span,
            bytes: paint.0.clone(),
        });
    }
    for (file, old, new) in [
        ("terrain/heights.grid", &before.0, &terrain.heights.0),
        (
            "terrain/water-heights.grid",
            &before.1,
            &terrain.water_heights.0,
        ),
    ] {
        if let Some(patch) = patch(&root, file, old, new, vertices) {
            commands.push(patch);
        }
    }
    session.own = true;
    session.stale = true;
    Some(step(session, label, commands))
}

/// Starts the session's map again from flat ground.
pub fn flatten(session: &mut Session) -> Option<crate::tabs::Edit> {
    let map = session.map.as_ref()?;
    let blocks = Blocks::flat(map.size);
    commit(session, "Flat blocks", blocks, |_| {})
}

/// One block edit, as a drag of a tool does it.
#[derive(Debug, Clone)]
pub struct BlockOp {
    pub tool: BlockTool,
    /// Columns (inclusive) of the dragged area.
    pub from: (i32, i32),
    pub to: (i32, i32),
    /// Shift: lower for Raise, remove for Ramp and Cut.
    pub reverse: bool,
    pub kind: BlockKind,
    /// Level: the level to set (default: the level where the drag began).
    pub level: Option<u8>,
    /// Cut: the world point (x, z) whose nearest corner of `to` is cut.
    pub point: Option<[f32; 2]>,
    /// The stroke in world cells (x, z): a brush tool's line, a ramp from its top to its
    /// foot, the cells painted (or with `fill`, the outline filled).
    pub path: Vec<[f32; 2]>,
    /// Block tools follow `path` instead of the box.
    pub brush: bool,
    /// The brush's radius: blocks for block tools, cells for Cells.
    pub radius: f32,
    /// Cells: fill the polygon `path` instead of painting along it.
    pub fill: bool,
    /// Face: the face style.
    pub style: u8,
}

impl BlockOp {
    /// An op over the box `from`–`to`.
    pub fn new(tool: BlockTool, from: (i32, i32), to: (i32, i32)) -> Self {
        Self {
            tool,
            from,
            to,
            reverse: false,
            kind: BlockKind::Ground,
            level: None,
            point: None,
            path: Vec::new(),
            brush: false,
            radius: 1.0,
            fill: false,
            style: 0,
        }
    }
}

/// The distance from `p` to the polyline `path`.
fn to_path(p: Vec2, path: &[[f32; 2]]) -> f32 {
    let points: Vec<Vec2> = path.iter().map(|q| Vec2::from_array(*q)).collect();
    match points.as_slice() {
        [] => f32::MAX,
        [only] => p.distance(*only),
        _ => points
            .windows(2)
            .map(|w| {
                let (a, b) = (w[0], w[1]);
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-6)).clamp(0.0, 1.0);
                p.distance(a + ab * t)
            })
            .fold(f32::MAX, f32::min),
    }
}

/// Whether `p` lies inside the polygon `path`.
fn inside(p: Vec2, path: &[[f32; 2]]) -> bool {
    let mut odd = false;
    let n = path.len();
    for i in 0..n {
        let (a, b) = (path[i], path[(i + n - 1) % n]);
        if (a[1] > p.y) != (b[1] > p.y) && p.x < (b[0] - a[0]) * (p.y - a[1]) / (b[1] - a[1]) + a[0]
        {
            odd = !odd;
        }
    }
    odd
}

/// The centre of column (`x`, `z`) in world cells.
fn centre(x: i32, z: i32) -> Vec2 {
    let b = BLOCK as f32;
    let halo = MAP_HALO as f32;
    Vec2::new((x as f32 + 0.5) * b - halo, (z as f32 + 0.5) * b - halo)
}

/// The columns an op works on: its box, or those within its brush of its stroke.
pub fn footprint(blocks: &Blocks, op: &BlockOp) -> Vec<(i32, i32)> {
    // An outline filled: every column whose centre is inside it.
    if op.fill && op.path.len() >= 3 && op.tool != BlockTool::Cells {
        let (lo, hi) = op.path.iter().fold(
            (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(Vec2::from_array(*p)), hi.max(Vec2::from_array(*p))),
        );
        let (x0, z0) = blocks.column_at(lo.x, lo.y);
        let (x1, z1) = blocks.column_at(hi.x, hi.y);
        return area((x0, z0), (x1, z1))
            .filter(|&(x, z)| blocks.get(x, z).is_some() && inside(centre(x, z), &op.path))
            .collect();
    }
    if !op.tool.strokes(op.brush) || op.path.is_empty() {
        return area(op.from, op.to).collect();
    }
    let reach = op.radius.max(0.0) * BLOCK as f32 + BLOCK as f32 * 0.5;
    let path = if op.tool == BlockTool::Ramp && op.path.len() > 1 {
        vec![op.path[0], op.path[op.path.len() - 1]]
    } else {
        op.path.clone()
    };
    let (lo, hi) = path.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(Vec2::from_array(*p)), hi.max(Vec2::from_array(*p))),
    );
    let (x0, z0) = blocks.column_at(lo.x - reach, lo.y - reach);
    let (x1, z1) = blocks.column_at(hi.x + reach, hi.y + reach);
    area((x0, z0), (x1, z1))
        .filter(|&(x, z)| blocks.get(x, z).is_some() && to_path(centre(x, z), &path) <= reach)
        .collect()
}

/// The cells Cells paints: within its radius of the stroke, or inside the filled outline.
fn cell_footprint(blocks: &Blocks, op: &BlockOp) -> Vec<(i32, i32)> {
    let halo = MAP_HALO as i32;
    let n = blocks.cells() as i32;
    let reach = op.radius.max(0.5);
    let (lo, hi) = op.path.iter().fold(
        (Vec2::splat(f32::MAX), Vec2::splat(f32::MIN)),
        |(lo, hi), p| (lo.min(Vec2::from_array(*p)), hi.max(Vec2::from_array(*p))),
    );
    if op.path.is_empty() {
        return Vec::new();
    }
    let pad = if op.fill { 0.0 } else { reach + 1.0 };
    let (x0, x1) = ((lo.x - pad).floor() as i32, (hi.x + pad).ceil() as i32);
    let (z0, z1) = ((lo.y - pad).floor() as i32, (hi.y + pad).ceil() as i32);
    let mut out = Vec::new();
    for z in z0..=z1 {
        for x in x0..=x1 {
            let (cx, cz) = (x + halo, z + halo);
            if cx < 0 || cz < 0 || cx >= n || cz >= n {
                continue;
            }
            let p = Vec2::new(x as f32 + 0.5, z as f32 + 0.5);
            let hit = if op.fill {
                inside(p, &op.path)
            } else {
                to_path(p, &op.path) <= reach
            };
            if hit {
                out.push((cx, cz));
            }
        }
    }
    out
}

/// The step `op` makes of the session's blocks (none when nothing changes).
pub fn apply(session: &mut Session, op: &BlockOp) -> Option<crate::tabs::Edit> {
    let blocks = blocks_of(session)?;
    let start = blocks.at(op.from.0, op.from.1);
    let reverse = op.reverse;
    let label = match op.tool {
        BlockTool::Raise if reverse => "Lower blocks",
        BlockTool::Raise => "Raise blocks",
        BlockTool::Level => "Level blocks",
        BlockTool::Paint => "Paint blocks",
        BlockTool::Ramp if reverse => "Remove ramps",
        BlockTool::Ramp => "Ramp",
        BlockTool::Cut => "Cut a block",
        BlockTool::Cells if reverse => "Clear painted cells",
        BlockTool::Cells => "Paint cells",
        BlockTool::Face => "Cliff face",
        BlockTool::Place | BlockTool::Select => return None,
        BlockTool::Round if reverse => "Uncut corners",
        BlockTool::Round => "Round corners",
    };
    let current = blocks.clone();
    let size = session.map.as_ref().map_or(0, |m| m.size);
    let symmetry = session.blocks.symmetry;
    commit(session, label, current, |blocks| {
        change(blocks, op, start);
        if symmetry != Symmetry::None {
            let side = blocks.side as i32;
            let twin = symmetry.op(op, size, side);
            let start = blocks.at(twin.from.0, twin.from.1);
            change(blocks, &twin, start);
        }
    })
}

/// What `op` does to `blocks`; `start` is the block its drag began on.
fn change(blocks: &mut Blocks, op: &BlockOp, start: Block) {
    let reverse = op.reverse;
    let columns = footprint(blocks, op);
    match op.tool {
        BlockTool::Raise => {
            for &(x, z) in &columns {
                if let Some(mut b) = blocks.get(x, z) {
                    b.level = if reverse {
                        b.level.saturating_sub(1)
                    } else {
                        (b.level + 1).min(MAX_LEVEL)
                    };
                    blocks.set(x, z, b);
                }
            }
        }
        BlockTool::Level => {
            let level = op.level.unwrap_or(start.level).min(MAX_LEVEL);
            for &(x, z) in &columns {
                if let Some(mut b) = blocks.get(x, z) {
                    b.level = level;
                    blocks.set(x, z, b);
                }
            }
        }
        BlockTool::Paint => {
            for &(x, z) in &columns {
                if let Some(mut b) = blocks.get(x, z) {
                    b.kind = op.kind;
                    blocks.set(x, z, b);
                }
            }
        }
        BlockTool::Ramp => ramp(blocks, op, &columns),
        BlockTool::Cut => {
            let (x, z) = op.to;
            if let (Some(mut b), Some(p)) = (blocks.get(x, z), op.point) {
                let halo = MAP_HALO as f32;
                let u = (p[0] + halo) / BLOCK as f32 - x as f32;
                let v = (p[1] + halo) / BLOCK as f32 - z as f32;
                let corner = match (u > 0.5, v > 0.5) {
                    (false, false) => Cut::NorthWest,
                    (true, false) => Cut::NorthEast,
                    (true, true) => Cut::SouthEast,
                    (false, true) => Cut::SouthWest,
                };
                b.cut = if reverse || b.cut == corner {
                    Cut::None
                } else {
                    corner
                };
                blocks.set(x, z, b);
            }
        }
        BlockTool::Cells => {
            let b = BLOCK as i32;
            for (cx, cz) in cell_footprint(blocks, op) {
                // A cell painted its own block's kind keeps no paint of its own.
                let own = blocks.at(cx / b, cz / b).kind;
                let kind = (!reverse && op.kind != own).then_some(op.kind);
                blocks.paint_cell(cx, cz, kind);
            }
        }
        BlockTool::Face => {
            for &(x, z) in &columns {
                if let Some(mut b) = blocks.get(x, z) {
                    b.style = if reverse { 0 } else { op.style };
                    blocks.set(x, z, b);
                }
            }
        }
        BlockTool::Place | BlockTool::Select => {}
        BlockTool::Round => round(blocks, &columns, reverse),
    }
}

/// Cuts the convex corners of `columns` at 45°: a corner where both neighbours beside it
/// are lower (and the one across it no higher), so a plateau's outline rounds off; with
/// `reverse`, takes those cuts away. Ramps keep their own.
/// Cuts the teeth of the staircase a diagonal ramp's blocks make, as the Ramp tool does
/// (the importers).
pub(crate) fn ramp_teeth(blocks: &mut Blocks, columns: &[(i32, i32)], level: u8, dir: u8) {
    teeth(blocks, columns, level, dir);
}

/// Cuts the turning corners of the plateaus among `columns` at 45° (the importers).
pub(crate) fn round_columns(blocks: &mut Blocks, columns: &[(i32, i32)]) {
    round(blocks, columns, false);
}

fn round(blocks: &mut Blocks, columns: &[(i32, i32)], reverse: bool) {
    let mut cuts = Vec::new();
    for &(x, z) in columns {
        let Some(b) = blocks.get(x, z) else { continue };
        if b.kind == BlockKind::Ramp {
            continue;
        }
        if reverse {
            cuts.push((x, z, Cut::None));
            continue;
        }
        let lower = |n: Block| {
            n.level < b.level || (n.level == b.level && n.kind.closed() && !b.kind.closed())
        };
        let corners: Vec<Cut> = [
            ((-1, -1), Cut::NorthWest),
            ((1, -1), Cut::NorthEast),
            ((1, 1), Cut::SouthEast),
            ((-1, 1), Cut::SouthWest),
        ]
        .into_iter()
        .filter(|&((dx, dz), _)| {
            let (a, c, d) = (
                blocks.at(x + dx, z),
                blocks.at(x, z + dz),
                blocks.at(x + dx, z + dz),
            );
            lower(a) && lower(c) && d.level <= b.level
        })
        .map(|(_, cut)| cut)
        .collect();
        // One turning corner: cut it; a block sticking out on two sides keeps its shape.
        if let [cut] = corners.as_slice() {
            cuts.push((x, z, *cut));
        }
    }
    for (x, z, cut) in cuts {
        if let Some(mut b) = blocks.get(x, z) {
            b.cut = cut;
            blocks.set(x, z, b);
        }
    }
}

/// Shows what `op` will do while its drag is under way: the map's grids as they would be,
/// none of it in the documents (the drag's end makes the one undo step).
fn live_preview(session: &mut Session, op: &BlockOp) {
    let key = format!(
        "{:?}{:?}{}{:?}{}",
        op.from,
        op.to,
        op.path.len(),
        op.path.last(),
        op.reverse
    );
    if session
        .blocks
        .preview
        .as_ref()
        .is_some_and(|(_, k)| *k == key)
    {
        return;
    }
    restore(session);
    let symmetry = session.blocks.symmetry;
    let Some(map) = session.map.as_mut() else {
        return;
    };
    let size = map.size;
    let terrain = &mut map.authoring.terrain;
    let Some(Ok(mut blocks)) = Blocks::of(terrain) else {
        return;
    };
    let original = Box::new(terrain.clone());
    let start = blocks.at(op.from.0, op.from.1);
    change(&mut blocks, op, start);
    if symmetry != Symmetry::None {
        let twin = symmetry.op(op, size, blocks.side as i32);
        let start = blocks.at(twin.from.0, twin.from.1);
        change(&mut blocks, &twin, start);
    }
    blocks.apply(terrain);
    session.blocks.preview = Some((original, key));
    session.stale = true;
}

/// Puts back the terrain a preview stood in for.
fn restore(session: &mut Session) {
    if let Some((original, _)) = session.blocks.preview.take()
        && let Some(map) = session.map.as_mut()
    {
        map.authoring.terrain = *original;
        session.stale = true;
    }
}

/// A ramp over `columns`: from the stroke's top (its first point) towards its foot, one
/// level down, as one region; a diagonal ramp's outer corners are cut so its ends and
/// sides run straight. A click makes one ramp block that finds its own way down.
fn ramp(blocks: &mut Blocks, op: &BlockOp, columns: &[(i32, i32)]) {
    if op.reverse {
        for &(x, z) in columns {
            if let Some(mut b) = blocks.get(x, z)
                && b.kind == BlockKind::Ramp
            {
                b.kind = BlockKind::Ground;
                b.ramp = 0;
                b.cut = Cut::None;
                blocks.set(x, z, b);
            }
        }
        return;
    }
    let (first, last) = match op.path.as_slice() {
        [first, .., last] => (Vec2::from_array(*first), Vec2::from_array(*last)),
        _ => (centre(op.from.0, op.from.1), centre(op.to.0, op.to.1)),
    };
    let fall = last - first;
    let dir = if fall.length() < BLOCK as f32 * 0.75 {
        0
    } else {
        ramp_direction(fall.x, fall.y)
    };
    let (tx, tz) = blocks.column_at(first.x, first.y);
    let level = op.level.unwrap_or(blocks.at(tx, tz).level).max(1);
    for &(x, z) in columns {
        if let Some(mut b) = blocks.get(x, z) {
            b.kind = BlockKind::Ramp;
            b.level = level;
            b.ramp = dir;
            b.cut = Cut::None;
            blocks.set(x, z, b);
        }
    }
    teeth(blocks, columns, level, dir);
}

fn teeth(blocks: &mut Blocks, columns: &[(i32, i32)], level: u8, dir: u8) {
    if dir == 0 {
        return;
    }
    let (dx, dz) = RAMP_DIRECTIONS[(dir - 1) as usize];
    if dx == 0 || dz == 0 {
        return;
    }
    let ours = |b: &Blocks, x: i32, z: i32| {
        b.get(x, z)
            .is_some_and(|n| n.kind == BlockKind::Ramp && n.level == level && n.ramp == dir)
    };
    let mut cuts = Vec::new();
    for &(x, z) in columns {
        // A corner where neither neighbour beside it is ramp is a tooth of the staircase
        // the blocks make of a diagonal: those up and down the fall first, then the sides.
        for (cx, cz) in [(dx, dz), (-dx, -dz), (dx, -dz), (-dx, dz)] {
            if !ours(blocks, x + cx, z) && !ours(blocks, x, z + cz) {
                let cut = match (cx, cz) {
                    (-1, -1) => Cut::NorthWest,
                    (1, -1) => Cut::NorthEast,
                    (1, 1) => Cut::SouthEast,
                    _ => Cut::SouthWest,
                };
                cuts.push((x, z, cut));
                break;
            }
        }
    }
    for (x, z, cut) in cuts {
        if let Some(mut b) = blocks.get(x, z) {
            b.cut = cut;
            blocks.set(x, z, b);
        }
    }
}

/// The rectangle of 4-byte samples where `new` differs from `old`, as a patch.
fn patch(root: &str, file: &str, old: &[u8], new: &[u8], side: u32) -> Option<Command> {
    let (mut x0, mut z0, mut x1, mut z1) = (u32::MAX, u32::MAX, 0, 0);
    for (i, (a, b)) in old.chunks_exact(4).zip(new.chunks_exact(4)).enumerate() {
        if a != b {
            let (x, z) = (i as u32 % side, i as u32 / side);
            x0 = x0.min(x);
            z0 = z0.min(z);
            x1 = x1.max(x);
            z1 = z1.max(z);
        }
    }
    if x0 == u32::MAX {
        return None;
    }
    let (width, height) = (x1 - x0 + 1, z1 - z0 + 1);
    let mut bytes = Vec::with_capacity((width * height * 4) as usize);
    for z in z0..=z1 {
        let at = (z * side + x0) as usize * 4;
        bytes.extend_from_slice(&new[at..at + width as usize * 4]);
    }
    Some(Command::Patch {
        document: format!("{root}/{file}"),
        x: x0,
        z: z0,
        width,
        height,
        bytes,
    })
}

/// The columns of a drag, inclusive.
fn area(a: (i32, i32), b: (i32, i32)) -> impl Iterator<Item = (i32, i32)> {
    let (x0, x1) = (a.0.min(b.0), a.0.max(b.0));
    let (z0, z1) = (a.1.min(b.1), a.1.max(b.1));
    (z0..=z1).flat_map(move |z| (x0..=x1).map(move |x| (x, z)))
}

/// The block view of `session`; `texture` is the scene's last picture.
/// Where the selection stands (world x, z).
fn selection_at(map: &canopy_content::map::UtcMap, picked: Picked, id: &str) -> Option<(f64, f64)> {
    match picked {
        Picked::Start => map
            .player_starts
            .iter()
            .find(|s| s.player.to_string() == id)
            .map(|s| (s.x, s.z)),
        Picked::Entity => map
            .entities
            .iter()
            .find(|e| e.id == id)
            .map(|e| (e.position.x, e.position.y)),
        Picked::Object => map
            .authoring
            .objects
            .iter()
            .find(|o| o.id == id)
            .map(|o| (o.x, o.z)),
    }
}

/// The commands that move the selection by whole cells (so placements stay on their cell
/// centres): a start; a camp member with its whole camp and its home; an amber crystal
/// with the rest of its site; a prop.
fn move_selection(
    session: &Session,
    docs: &crate::map::view::Docs,
    picked: Picked,
    id: &str,
    delta: (f64, f64),
) -> Vec<Command> {
    let (dx, dz) = (delta.0.round(), delta.1.round());
    if dx == 0.0 && dz == 0.0 {
        return Vec::new();
    }
    let map_doc = session.path("map.json");
    let set = |document: &str, pointer: String, value: serde_json::Value| Command::Set {
        document: document.to_string(),
        pointer,
        value,
        at: None,
    };
    let f = |v: &serde_json::Value| v.as_f64().unwrap_or(0.0);
    let mut out = Vec::new();
    match picked {
        Picked::Start => {
            let starts = docs.map["playerStarts"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            if let Some((i, s)) = starts
                .iter()
                .enumerate()
                .find(|(_, s)| s["player"].as_u64().map(|p| p.to_string()).as_deref() == Some(id))
            {
                out.push(set(
                    &map_doc,
                    format!("/playerStarts/{i}/x"),
                    (f(&s["x"]) + dx).into(),
                ));
                out.push(set(
                    &map_doc,
                    format!("/playerStarts/{i}/z"),
                    (f(&s["z"]) + dz).into(),
                ));
            }
        }
        Picked::Entity => {
            let camps = docs.map["camps"].as_array().cloned().unwrap_or_default();
            let camp = camps.iter().position(|c| {
                c["members"]
                    .as_array()
                    .is_some_and(|m| m.iter().any(|v| v == id))
            });
            // A site's crystals share their id up to the last dot (mine.3.0, mine.3.1…).
            let site = id
                .starts_with("mine.")
                .then(|| id.rsplit_once('.').map(|(head, _)| format!("{head}.")))
                .flatten();
            let together = |e: &serde_json::Value| {
                let eid = e["id"].as_str().unwrap_or_default();
                match (camp, &site) {
                    (Some(c), _) => camps[c]["members"]
                        .as_array()
                        .is_some_and(|m| m.iter().any(|v| v == eid)),
                    (None, Some(site)) => eid.starts_with(site.as_str()),
                    (None, None) => eid == id,
                }
            };
            for (i, e) in docs.map["entities"]
                .as_array()
                .into_iter()
                .flatten()
                .enumerate()
            {
                if together(e) {
                    let p = &e["position"];
                    out.push(set(
                        &map_doc,
                        format!("/entities/{i}/position"),
                        serde_json::json!({"x": f(&p["x"]) + dx, "y": f(&p["y"]) + dz}),
                    ));
                }
            }
            if let Some(c) = camp {
                let home = &camps[c]["home"];
                out.push(set(
                    &map_doc,
                    format!("/camps/{c}/home"),
                    serde_json::json!({"x": f(&home["x"]) + dx, "y": f(&home["y"]) + dz}),
                ));
            }
        }
        Picked::Object => {
            if let Some((i, o)) = docs.scenery["objects"]
                .as_array()
                .and_then(|o| o.iter().enumerate().find(|(_, o)| o["id"] == id))
            {
                let doc = session.path("scenery.json");
                out.push(set(
                    &doc,
                    format!("/objects/{i}/x"),
                    (f(&o["x"]) + dx).into(),
                ));
                out.push(set(
                    &doc,
                    format!("/objects/{i}/z"),
                    (f(&o["z"]) + dz).into(),
                ));
            }
        }
    }
    out
}

/// Everything placed on the map, with where it stands: starts, entities and scenery.
fn placed(map: &canopy_content::map::UtcMap) -> Vec<(Picked, String, f64, f64)> {
    let mut out: Vec<(Picked, String, f64, f64)> = map
        .player_starts
        .iter()
        .map(|s| (Picked::Start, s.player.to_string(), s.x, s.z))
        .collect();
    out.extend(
        map.entities
            .iter()
            .map(|e| (Picked::Entity, e.id.clone(), e.position.x, e.position.y)),
    );
    out.extend(
        map.authoring
            .objects
            .iter()
            .map(|o| (Picked::Object, o.id.clone(), o.x, o.z)),
    );
    out
}

/// The selected item under world point `at`, if any.
fn selection_under(session: &Session, at: [f32; 2]) -> Option<(Picked, String)> {
    let map = session.map.as_ref()?;
    session
        .blocks
        .selection
        .iter()
        .find(|(kind, id)| {
            let reach = if *kind == Picked::Start { 8.0 } else { 3.5 };
            selection_at(map, *kind, id)
                .is_some_and(|(x, z)| (x as f32 - at[0]).hypot(z as f32 - at[1]) < reach)
        })
        .cloned()
}

/// A drag of the selection by `delta`, snapped: whole blocks when a building, a start or
/// a node moves, so they stay on the building lattice; otherwise as fine as Place is set.
fn snapped_delta(
    session: &Session,
    content: Option<&canopy_content::Content>,
    delta: (f64, f64),
) -> (f64, f64) {
    let lattice = session
        .blocks
        .selection
        .iter()
        .any(|(kind, id)| match kind {
            Picked::Start => true,
            Picked::Entity => session
                .map
                .as_ref()
                .and_then(|m| m.entities.iter().find(|e| &e.id == id))
                .and_then(|e| content?.definition(e.definition.as_str()))
                .is_some_and(|d| d.kind == canopy_content::game::Kind::Building),
            Picked::Object => false,
        });
    let step = if lattice || !session.blocks.fine {
        4.0
    } else {
        1.0
    };
    (
        (delta.0 / step).round() * step,
        (delta.1 / step).round() * step,
    )
}

/// What a move of the selection carries along: every selected thing, a camp member's
/// whole camp, an old amber site's every node; each with where it stands.
fn carried(session: &Session) -> Vec<(Picked, String, f64, f64)> {
    let Some(map) = session.map.as_ref() else {
        return Vec::new();
    };
    let selected = &session.blocks.selection;
    let mut ids: std::collections::BTreeSet<String> = selected
        .iter()
        .filter(|(k, _)| *k == Picked::Entity)
        .map(|(_, id)| id.clone())
        .collect();
    for camp in &map.camps {
        if camp.members.iter().any(|m| ids.contains(m)) {
            ids.extend(camp.members.iter().cloned());
        }
    }
    let sites: Vec<String> = ids
        .iter()
        .filter(|id| id.starts_with("mine."))
        .filter_map(|id| id.rsplit_once('.').map(|(head, _)| format!("{head}.")))
        .collect();
    placed(map)
        .into_iter()
        .filter(|(kind, id, _, _)| match kind {
            Picked::Entity => ids.contains(id) || sites.iter().any(|s| id.starts_with(s)),
            other => selected.contains(&(*other, id.clone())),
        })
        .collect()
}

/// The simulation's view of the map's terrain, made again when the map changes.
fn sim_terrain(
    session: &mut Session,
    content: &canopy_content::Content,
) -> Option<std::sync::Arc<canopy_sim::terrain::Terrain>> {
    if let Some((at, terrain)) = &session.sim_terrain
        && *at == session.loaded_at
    {
        return Some(terrain.clone());
    }
    let map = session.map.as_ref()?;
    let terrain = std::sync::Arc::new(canopy_sim::terrain::Terrain::compile(map, content).ok()?);
    session.sim_terrain = Some((session.loaded_at, terrain.clone()));
    Some(terrain)
}

/// The cells taken by buildings and starts (a start: its every fort and worker
/// formation), but for those in `ignore`.
fn taken_cells(
    map: &canopy_content::map::UtcMap,
    content: &canopy_content::Content,
    terrain: &canopy_sim::terrain::Terrain,
    ignore: &[(Picked, String)],
) -> Vec<(String, canopy_sim::terrain::CellBounds)> {
    use canopy_sim::terrain::footprint::{Extent, footprint_cells};
    let skip = |kind: Picked, id: &str| ignore.iter().any(|(k, i)| *k == kind && i == id);
    let mut out = Vec::new();
    for e in &map.entities {
        if skip(Picked::Entity, &e.id) {
            continue;
        }
        if let Some(d) = content.definition(e.definition.as_str())
            && d.kind == canopy_content::game::Kind::Building
        {
            let cells = footprint_cells(
                e.position.x,
                e.position.y,
                Extent::of(d.footprint.as_ref()),
                e.rotation,
            );
            out.push((d.name.clone(), cells));
        }
    }
    for s in &map.player_starts {
        if skip(Picked::Start, &s.player.to_string()) {
            continue;
        }
        if let Some(cells) = canopy_sim::spawn::start_area(map, content, terrain, s) {
            out.push((format!("start {}", s.player), cells));
        }
    }
    out
}

/// Why `placing` may not stand at `land` as the game would check it (for a start, the
/// start's rules and room from the other starts; for a unit or building, open ground in
/// the playable area, a building's flat foundation clear of other buildings and starts),
/// ignoring what is in `ignore` (what a drag moves).
fn problem(
    map: &canopy_content::map::UtcMap,
    content: &canopy_content::Content,
    terrain: &canopy_sim::terrain::Terrain,
    placing: &Placing,
    land: &Landing,
    ignore: &[(Picked, String)],
) -> Option<String> {
    use canopy_sim::terrain::footprint::{Extent, footprint_cells};
    let overlap = |cells: canopy_sim::terrain::CellBounds| {
        taken_cells(map, content, terrain, ignore)
            .into_iter()
            .find(|(_, other)| {
                cells.min_x <= other.max_x
                    && other.min_x <= cells.max_x
                    && cells.min_y <= other.max_y
                    && other.min_y <= cells.max_y
            })
            .map(|(name, _)| format!("Overlaps {name}"))
    };
    let definition = match placing {
        Placing::Definition(id) => content.definition(id),
        Placing::Amber => content.definitions.iter().find(|d| catalog::is_mine(d)),
        Placing::Camp(_) => content
            .definitions
            .iter()
            .find(|d| d.behaviors.camp_defense.is_some()),
        Placing::Start(player) => {
            let setup = content
                .rules
                .races
                .values()
                .filter_map(|r| r.starting_setup.as_ref())
                .next()
                .unwrap_or(&content.rules.starting_setup);
            let start = canopy_content::map::PlayerStart {
                player: *player,
                x: land.x,
                z: land.z,
                rotation: None,
                setup: setup.id.to_string(),
                main_fort: format!("start.player.{player}/main-fort"),
            };
            if let Err(error) = canopy_sim::spawn::start_problem(map, content, terrain, &start) {
                return Some(error);
            }
            let crowded = map.player_starts.iter().any(|s| {
                s.player != *player
                    && !ignore.contains(&(Picked::Start, s.player.to_string()))
                    && (s.x - land.x).hypot(s.z - land.z) < 20.0
            });
            if crowded {
                return Some("Too close to another start".into());
            }
            return canopy_sim::spawn::start_area(map, content, terrain, &start).and_then(overlap);
        }
        Placing::Scenery(_) => return None,
    }?;
    if let Some(error) =
        canopy_sim::spawn::site_problem(map, terrain, definition, land.x, land.z, 0.0)
    {
        return Some(error);
    }
    if definition.kind == canopy_content::game::Kind::Building {
        let cells = footprint_cells(
            land.x,
            land.z,
            Extent::of(definition.footprint.as_ref()),
            0.0,
        );
        return overlap(cells);
    }
    None
}

/// Whether the selection may move by `delta` (already snapped): every start, building
/// and unit it carries must stand where it lands, by [`problem`]'s rules, clear of what
/// does not move.
fn move_fits(
    session: &Session,
    content: &canopy_content::Content,
    terrain: Option<&canopy_sim::terrain::Terrain>,
    delta: (f64, f64),
) -> Result<(), String> {
    let (Some(map), Some(terrain)) = (session.map.as_ref(), terrain) else {
        return Ok(());
    };
    let items = carried(session);
    let ignore: Vec<(Picked, String)> =
        items.iter().map(|(k, id, _, _)| (*k, id.clone())).collect();
    for (kind, id, x, z) in &items {
        let placing = match kind {
            Picked::Start => Placing::Start(id.parse().unwrap_or(1)),
            Picked::Entity => match map.entities.iter().find(|e| &e.id == id) {
                Some(e) => Placing::Definition(e.definition.to_string()),
                None => continue,
            },
            Picked::Object => continue,
        };
        let land = Landing {
            x: x + delta.0,
            z: z + delta.1,
            footprint: None,
        };
        if let Some(why) = problem(map, content, terrain, &placing, &land, &ignore) {
            return Err(why);
        }
    }
    Ok(())
}

/// Why something may not stand where the pointer is: a label beside the pointer.
fn reason(painter: &egui::Painter, at: egui::Pos2, why: &str) {
    let galley = painter.layout_no_wrap(
        why.to_string(),
        egui::FontId::proportional(12.0),
        Color32::from_rgb(255, 150, 140),
    );
    let place = at + egui::vec2(18.0, 16.0);
    let back = Rect::from_min_size(place, galley.size()).expand2(egui::vec2(7.0, 4.0));
    painter.rect(
        back,
        5,
        Color32::from_black_alpha(200),
        Line::new(1.0, Color32::from_rgb(200, 70, 60)),
        egui::StrokeKind::Inside,
    );
    painter.galley(place, galley, Color32::WHITE);
}

/// What `placing` puts down, as a ghost: its model and scale.
pub fn ghost_look(content: &canopy_content::Content, placing: &Placing) -> Option<(String, f64)> {
    match placing {
        Placing::Definition(id) => crate::map::look_of(content, id),
        Placing::Amber => content
            .definitions
            .iter()
            .find(|d| catalog::is_mine(d))
            .and_then(|d| crate::map::look_of(content, d.id.as_str())),
        Placing::Start(_) => {
            let setup = content
                .rules
                .races
                .values()
                .filter_map(|r| r.starting_setup.as_ref())
                .next()
                .unwrap_or(&content.rules.starting_setup);
            crate::map::look_of(content, setup.fort.as_str())
        }
        Placing::Camp(_) | Placing::Scenery(_) => None,
    }
}

/// The commands that move the whole selection by `delta`: each camp and each old amber
/// site once, however many of its members are selected.
fn move_all(
    session: &Session,
    content: Option<&canopy_content::Content>,
    docs: &crate::map::view::Docs,
    delta: (f64, f64),
) -> Vec<Command> {
    let delta = snapped_delta(session, content, delta);
    let camps = docs.map["camps"].as_array().cloned().unwrap_or_default();
    let group = |id: &str| -> String {
        if let Some(c) = camps.iter().position(|c| {
            c["members"]
                .as_array()
                .is_some_and(|m| m.iter().any(|v| v == id))
        }) {
            return format!("camp {c}");
        }
        if id.starts_with("mine.")
            && let Some((head, _)) = id.rsplit_once('.')
        {
            return head.to_string();
        }
        id.to_string()
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut out = Vec::new();
    for (kind, id) in &session.blocks.selection {
        let key = match kind {
            Picked::Entity => format!("e {}", group(id)),
            Picked::Start => format!("s {id}"),
            Picked::Object => format!("o {id}"),
        };
        if seen.insert(key) {
            out.extend(move_selection(session, docs, *kind, id, delta));
        }
    }
    out
}

/// A building's centre on the 4-cell building lattice (its edges at -0.5 + 4k), for a
/// footprint `width` × `depth` cells.
fn on_lattice(x: f64, z: f64, width: u32, depth: u32) -> (f64, f64) {
    let snap = |v: f64, size: u32| {
        let offset = f64::from(size) / 2.0 - 0.5;
        ((v - offset) / 4.0).round() * 4.0 + offset
    };
    (snap(x, width), snap(z, depth))
}

/// Where a placement lands (the simulation's rules: a building's footprint edges on the
/// 4-cell building lattice, units and resources on cell centres), and the footprint to
/// show there. `fine` places what may stand anywhere (units, camps, scenery) cell by cell
/// instead of block by block.
pub struct Landing {
    pub x: f64,
    pub z: f64,
    /// Width and depth in cells, for what has a footprint.
    pub footprint: Option<(u32, u32)>,
}

fn landing(
    content: &canopy_content::Content,
    placing: &Placing,
    at: [f32; 2],
    fine: bool,
) -> Landing {
    use crate::map::tools;
    let (x, z) = (f64::from(at[0]), f64::from(at[1]));
    // A block's middle (4k + 1.5) or a cell's centre; units and camps need a cell centre,
    // so a block's middle rounds to the cell beside it.
    let free = |v: f64| {
        if fine {
            v.round()
        } else {
            ((v - 1.5) / 4.0).round() * 4.0 + 1.5
        }
    };
    let cell = |v: f64| free(v).round();
    let point = |x: f64, z: f64| Landing {
        x,
        z,
        footprint: None,
    };
    let building = |d: &canopy_content::game::Definition| {
        let (w, h) = d.footprint.as_ref().map_or((4, 4), |f| (f.width, f.depth));
        let (x, z) = on_lattice(x, z, w, h);
        Landing {
            x,
            z,
            footprint: Some((w, h)),
        }
    };
    match placing {
        Placing::Definition(id) => match content.definition(id) {
            Some(d) if d.kind == canopy_content::game::Kind::Building => building(d),
            _ => point(cell(x), cell(z)),
        },
        Placing::Amber => match content.definitions.iter().find(|d| catalog::is_mine(d)) {
            Some(d) => building(d),
            None => point(cell(x), cell(z)),
        },
        Placing::Start(_) => {
            let (x, z) = tools::start_position(x, z);
            let setup = content
                .rules
                .races
                .values()
                .filter_map(|r| r.starting_setup.as_ref())
                .next()
                .unwrap_or(&content.rules.starting_setup);
            Landing {
                x,
                z,
                footprint: content
                    .definition(setup.fort.as_str())
                    .and_then(|d| d.footprint.as_ref())
                    .map(|f| (f.width, f.depth)),
            }
        }
        Placing::Camp(_) => point(cell(x), cell(z)),
        Placing::Scenery(_) => point(free(x), free(z)),
    }
}

/// Places what Place puts down at `at`, owned by player `owner` (0: no one) when it can
/// be: a unit or building, a camp, a player's start (moved when it is there already), one
/// amber node, or a piece of scenery. In blocks nothing needs a pad: the blocks are the
/// ground.
fn place(
    session: &mut Session,
    content: &canopy_content::Content,
    docs: &crate::map::view::Docs,
    placing: &Placing,
    owner: u8,
    at: [f32; 2],
) -> Option<crate::tabs::Edit> {
    use crate::map::tools::{self, Placement};
    use serde_json::json;
    let land = landing(content, placing, at, session.blocks.fine);
    let (x, z) = (land.x, land.z);
    let map = session.map.as_ref()?;
    let map_doc = session.path("map.json");
    let list = |v: &serde_json::Value| v.as_array().cloned().unwrap_or_default();
    let entities = list(&docs.map["entities"]);
    let fresh = |prefix: &str, taken: Vec<String>| {
        let n = (1..)
            .find(|n| !taken.iter().any(|t| t == &format!("{prefix}{n}")))
            .unwrap_or(1);
        format!("{prefix}{n}")
    };
    let entity_ids: Vec<String> = entities
        .iter()
        .filter_map(|e| e["id"].as_str().map(str::to_string))
        .collect();
    let insert_entity = |value: serde_json::Value| Command::Insert {
        document: map_doc.clone(),
        pointer: "/entities".into(),
        index: entities.len(),
        value,
    };
    match placing {
        Placing::Camp(tier) => {
            session.tools.tier = *tier;
            let mut edit = crate::map::view::place(session, content, docs, Placement::Camp, x, z)
                .ok()
                .flatten()?;
            edit.commands.retain(
                |c| !matches!(c, Command::Patch { document, .. } if document.ends_with("heights.grid")),
            );
            Some(edit)
        }
        Placing::Start(player) => {
            let setup = content
                .rules
                .races
                .values()
                .filter_map(|r| r.starting_setup.as_ref())
                .next()
                .unwrap_or(&content.rules.starting_setup);
            let rotation = tools::facing_middle(content, setup.fort.as_str(), x, z, map.size);
            let starts = list(&docs.map["playerStarts"]);
            let commands = match starts
                .iter()
                .position(|s| s["player"].as_u64() == Some(u64::from(*player)))
            {
                Some(i) => vec![
                    Command::Set {
                        document: map_doc.clone(),
                        pointer: format!("/playerStarts/{i}/x"),
                        value: x.into(),
                        at: None,
                    },
                    Command::Set {
                        document: map_doc.clone(),
                        pointer: format!("/playerStarts/{i}/z"),
                        value: z.into(),
                        at: None,
                    },
                ],
                None => {
                    let mut start = json!({"mainFort": format!("start.player.{player}/main-fort"), "player": player, "setup": setup.id.as_str(), "x": x, "z": z});
                    if rotation != 0 {
                        start["rotation"] = json!(rotation);
                    }
                    vec![Command::Insert {
                        document: map_doc.clone(),
                        pointer: "/playerStarts".into(),
                        index: starts.len(),
                        value: start,
                    }]
                }
            };
            Some(step(session, &format!("Place start {player}"), commands))
        }
        Placing::Amber => {
            let mine = content.definitions.iter().find(|d| catalog::is_mine(d))?;
            // The mine's numbered looks (`…-1`, `…-2`, …), one picked by the place.
            let stem = mine
                .model
                .as_str()
                .trim_end_matches(|c: char| c.is_ascii_digit());
            let looks: Vec<&str> = content
                .assets
                .iter()
                .map(|a| a.id.as_str())
                .filter(|id| stem.ends_with('-') && id.starts_with(stem))
                .collect();
            let mut node = json!({
                "definition": mine.id.as_str(),
                "id": fresh("amber.", entity_ids),
                "initialState": {"amount": session.tools.amber, "construction": "complete"},
                "owner": "none",
                "position": {"x": x, "y": z},
                "rotation": 0.0,
            });
            if !looks.is_empty() {
                let pick = ((x * 7.0 + z * 13.0).abs() as usize) % looks.len();
                node["appearance"] = json!({"asset": looks[pick]});
            }
            Some(step(
                session,
                "Place an amber node",
                vec![insert_entity(node)],
            ))
        }
        Placing::Definition(id) => {
            let definition = content.definition(id)?;
            let neutral = session
                .blocks
                .catalog
                .find(placing)
                .is_none_or(|e| e.neutral);
            let owner = if neutral || owner == 0 {
                "none".to_string()
            } else {
                format!("player.{owner}")
            };
            let building = definition.kind == canopy_content::game::Kind::Building;
            let short = id.rsplit('.').next().unwrap_or(id);
            let mut entity = json!({
                "definition": id,
                "id": fresh(&format!("{short}."), entity_ids),
                "owner": owner,
                "position": {"x": x, "y": z},
                "rotation": 0.0,
            });
            if building {
                entity["initialState"] = json!({"construction": "complete"});
            }
            Some(step(
                session,
                &format!("Place {}", definition.name),
                vec![insert_entity(entity)],
            ))
        }
        Placing::Scenery(id) => {
            let objects = list(&docs.scenery["objects"]);
            let short = id.rsplit('.').next().unwrap_or(id);
            let taken = objects
                .iter()
                .filter_map(|o| o["id"].as_str().map(str::to_string))
                .collect();
            // A turn picked by the place, so a row of the same piece does not repeat.
            let yaw = ((x * 12.9898 + z * 78.233).sin() * 43758.545).fract().abs()
                * std::f64::consts::TAU;
            let object = json!({
                "id": fresh(&format!("{short}."), taken),
                "asset": id,
                "x": x,
                "z": z,
                "yaw": yaw,
            });
            Some(step(
                session,
                "Place scenery",
                vec![Command::Insert {
                    document: session.path("scenery.json"),
                    pointer: "/objects".into(),
                    index: objects.len(),
                    value: object,
                }],
            ))
        }
    }
}

/// The block view of `session`; `texture` is the scene's last picture.
pub fn show(
    ui: &mut Ui,
    session: &mut Session,
    content: Option<&canopy_content::Content>,
    docs: Option<&crate::map::view::Docs>,
    texture: Option<egui::TextureId>,
    texture_generated: Option<egui::TextureId>,
    package: Option<&std::path::Path>,
) -> Out {
    let mut out = Out::default();
    // The map's one panel: its settings, or what is selected.
    if Panels::get(ui.ctx()).properties {
        let mut edits = Vec::new();
        egui::Panel::right(egui::Id::new(("map-panel", &session.root)))
            .exact_size(300.0)
            .resizable(false)
            .show_separator_line(true)
            .frame(
                egui::Frame::new()
                    .fill(PANEL)
                    .inner_margin(egui::Margin::same(14)),
            )
            .show(ui, |ui| {
                if let Some(error) = &session.error {
                    ui.label(
                        RichText::new(error)
                            .size(11.0)
                            .color(Color32::from_rgb(240, 120, 110)),
                    );
                }
                edits = settings::show(ui, session, content, docs, package);
            });
        out.edits.extend(edits);
    }
    out.preview = std::mem::take(&mut session.blocks.write_picture);
    // Place and Select check what they put down as the game would.
    session.overlay = Default::default();
    let terrain = match (content, session.blocks.tool) {
        (Some(content), BlockTool::Place | BlockTool::Select) => sim_terrain(session, content),
        _ => None,
    };
    let full = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(full, Sense::click_and_drag());
    ui.painter_at(full).rect_filled(full, 0, RAISED);
    // The split view: the blocks on the left, the generated world on the right; the pane
    // under the pointer is the one worked in (kept through a drag).
    let split = session.blocks.view == View::Split;
    let panes = split.then(|| {
        let middle = full.center().x;
        (
            Rect::from_min_max(full.min, pos2(middle - 1.0, full.max.y)),
            Rect::from_min_max(pos2(middle + 1.0, full.min.y), full.max),
        )
    });
    if let Some((_, right)) = panes
        && session.blocks.drag.is_none()
        && !response.dragged()
        && let Some(p) = response.hover_pos()
    {
        session.blocks.right_pane = right.contains(p);
    }
    let rect = match panes {
        Some((left, right)) => {
            if session.blocks.right_pane {
                right
            } else {
                left
            }
        }
        None => full,
    };
    let painter = ui.painter_at(rect);
    let Some(map) = session.map.as_ref() else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "Loading the map…",
            egui::FontId::proportional(12.0),
            FAINT,
        );
        return out;
    };
    let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
    // In the split view, where the overlays (the hover, the drag, the selection, the
    // playable outline) begin among the shapes drawn: they are drawn again in the other
    // pane, which shows the same camera at the same size.
    let mut mirror_from = None;
    let scale = ui.ctx().pixels_per_point();
    let pixels = |r: Rect| [(r.width() * scale) as u32, (r.height() * scale) as u32];
    match panes {
        Some((left, right)) => {
            let full_painter = ui.painter_at(full);
            if let Some(texture) = texture {
                full_painter.image(texture, left, uv, Color32::WHITE);
            }
            if let Some(texture) = texture_generated {
                full_painter.image(texture, right, uv, Color32::WHITE);
            }
            full_painter.vline(full.center().x, full.y_range(), Line::new(2.0, BORDER));
            // The world follows the blocks a moment after the last edit: a spinner while
            // it waits.
            if session.generated_due.is_some() {
                let at = Rect::from_center_size(
                    right.right_bottom() + egui::vec2(-20.0, -20.0),
                    egui::vec2(14.0, 14.0),
                );
                egui::Spinner::new().size(14.0).color(SOFT).paint_at(ui, at);
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(100));
            }
            out.size = Some(pixels(left));
            out.size_generated = Some(pixels(right));
            mirror_from = Some(
                ui.ctx()
                    .graphics(|g| g.get(ui.layer_id()).map_or(0, |l| l.next_idx().0)),
            );
        }
        None => {
            if let Some(texture) = texture {
                painter.image(texture, rect, uv, Color32::WHITE);
            }
            out.size = Some(pixels(rect));
        }
    }
    let map_size = map.size;
    let ground =
        crate::map::tools::ground_at(map, session.camera.focus[0], session.camera.focus[1]);
    let (eye, _, view_projection) = matrices(
        &session.camera,
        ground,
        rect.width() / rect.height().max(1.0),
    );
    let input = ui.input(|i| i.clone());
    navigate(&input, &response, rect, &mut session.camera, map_size);
    // G switches between the blocks and the generated world, H shows where ground units
    // walk and back, V both side by side and back, wherever the pointer is, unless a
    // field is being typed in.
    if !input.modifiers.command && !ui.ctx().egui_wants_keyboard_input() {
        let view = session.blocks.view;
        let next = if input.key_pressed(egui::Key::G) {
            Some(match view {
                View::Generated => View::Blocks,
                _ => View::Generated,
            })
        } else if input.key_pressed(egui::Key::X) {
            session.blocks.fine = !session.blocks.fine;
            None
        } else if let Some(mode) = [(egui::Key::H, View::Pathing), (egui::Key::V, View::Split)]
            .into_iter()
            .find_map(|(key, mode)| input.key_pressed(key).then_some(mode))
        {
            Some(session.blocks.toggle(mode))
        } else {
            None
        };
        if let Some(next) = next
            && session.blocks.set_view(next)
        {
            session.stale = true;
        }
    }
    if response.hovered() && !input.modifiers.command {
        for tool in BlockTool::ALL {
            if input.key_pressed(tool.key()) {
                session.blocks.tool = tool;
            }
        }
        if input.key_pressed(egui::Key::B) {
            // Box, brush, lasso.
            let state = &mut session.blocks;
            (state.brush, state.lasso) = match (state.brush, state.lasso) {
                (false, false) => (true, false),
                (true, false) => (false, true),
                _ => (false, false),
            };
        }
        if input.key_pressed(egui::Key::Y) {
            let all = Symmetry::ALL;
            let i = all
                .iter()
                .position(|s| *s == session.blocks.symmetry)
                .unwrap_or(0);
            session.blocks.symmetry = all[(i + 1) % all.len()];
        }
        let grow = i32::from(input.key_pressed(egui::Key::CloseBracket))
            - i32::from(input.key_pressed(egui::Key::OpenBracket));
        if grow != 0 {
            let state = &mut session.blocks;
            if state.tool == BlockTool::Cells {
                state.cell_radius = (state.cell_radius as i32 + grow).clamp(1, 16) as u8;
            } else {
                state.radius = (state.radius as i32 + grow).clamp(0, 8) as u8;
            }
        }
        for (n, key) in [
            egui::Key::Num1,
            egui::Key::Num2,
            egui::Key::Num3,
            egui::Key::Num4,
            egui::Key::Num5,
            egui::Key::Num6,
            egui::Key::Num7,
        ]
        .iter()
        .enumerate()
        {
            if input.key_pressed(*key) && BlockKind::ALL[n] != BlockKind::Ramp {
                if session.blocks.tool == BlockTool::Cells {
                    session.blocks.cell_kind = BlockKind::ALL[n];
                } else {
                    session.blocks.kind = BlockKind::ALL[n];
                    session.blocks.tool = BlockTool::Paint;
                }
            }
        }
    }
    let Some(blocks) = blocks_of(session) else {
        painter.text(
            rect.center(),
            egui::Align2::CENTER_CENTER,
            "This map has no blocks: its package is broken",
            egui::FontId::proportional(13.0),
            MUTED,
        );
        return out;
    };
    let pointer = response.hover_pos().or(response.interact_pointer_pos());
    // Picking: the ground under the pointer, nudged a little back towards the camera so a
    // block's top is chosen rather than the foot of the cliff below it.
    let hit = pointer
        .and_then(|p| pick(session, view_projection, eye, rect, p))
        .map(|p| {
            let back = (eye - p).normalize_or_zero();
            p + back * 0.15
        });
    let column = hit.map(|p| (blocks.column_at(p.x, p.z), p));
    let shift = input.modifiers.shift;

    // A block's top outline, projected.
    let outline = |(x, z): (i32, i32), (x2, z2): (i32, i32), color: Color32| {
        let b = BLOCK as f32;
        let halo = MAP_HALO as f32;
        let (x0, x1) = (x.min(x2), x.max(x2) + 1);
        let (z0, z1) = (z.min(z2), z.max(z2) + 1);
        let y = area((x, z), (x2, z2))
            .map(|(cx, cz)| mesh::top(blocks.at(cx, cz)))
            .fold(f32::MIN, f32::max)
            + 0.05;
        let corners = [(x0, z0), (x1, z0), (x1, z1), (x0, z1), (x0, z0)];
        let points: Vec<_> = corners
            .iter()
            .filter_map(|&(cx, cz)| {
                project(
                    view_projection,
                    rect,
                    Vec3::new(cx as f32 * b - halo, y, cz as f32 * b - halo),
                )
            })
            .collect();
        if points.len() == 5 {
            painter.add(egui::Shape::line(points, Line::new(2.0, color)));
        }
    };

    let state = &session.blocks;
    let strokes = state.tool.strokes(state.brush || state.lasso);
    let radius = if state.tool == BlockTool::Cells {
        state.cell_radius as f32
    } else {
        state.radius as f32
    };
    // A ring on the ground around `at`, `r` cells across.
    let ring = |at: Vec3, r: f32, color: Color32| {
        let points: Vec<_> = (0..=32)
            .filter_map(|k| {
                let a = k as f32 / 32.0 * std::f32::consts::TAU;
                let p = Vec2::new(at.x, at.z) + Vec2::from_angle(a) * r;
                let y = crate::map::tools::ground_at(map, p.x, p.y) + 0.1;
                project(view_projection, rect, Vec3::new(p.x, y, p.y))
            })
            .collect();
        if points.len() > 2 {
            painter.add(egui::Shape::line(points, Line::new(1.5, color)));
        }
    };
    // A footprint `w` × `h` cells round (`x`, `z`), on the ground.
    let ground_rect = |x: f32, z: f32, w: f32, h: f32, color: Color32| {
        let (hw, hh) = (w / 2.0, h / 2.0);
        let corners = [(-hw, -hh), (hw, -hh), (hw, hh), (-hw, hh), (-hw, -hh)];
        let mut points = Vec::new();
        for pair in corners.windows(2) {
            for k in 0..8 {
                let t = k as f32 / 8.0;
                let px = x + pair[0].0 + (pair[1].0 - pair[0].0) * t;
                let pz = z + pair[0].1 + (pair[1].1 - pair[0].1) * t;
                let y = crate::map::tools::ground_at(map, px, pz) + 0.15;
                points.extend(project(view_projection, rect, Vec3::new(px, y, pz)));
            }
        }
        points.extend(points.first().copied());
        painter.add(egui::Shape::line(points, Line::new(2.0, color)));
    };
    let preview = |op: &BlockOp, color: Color32| {
        if op.tool == BlockTool::Cells {
            return;
        }
        for (x, z) in footprint(&blocks, op) {
            outline((x, z), (x, z), color);
        }
    };
    let symmetry = state.symmetry;
    let side = blocks.side as i32;
    let mut shown: Option<BlockOp> = None;
    let mut live: Option<BlockOp> = None;
    let mut op = BlockOp {
        tool: state.tool,
        reverse: shift,
        kind: if state.tool == BlockTool::Cells {
            state.cell_kind
        } else {
            state.kind
        },
        point: column.map(|(_, p)| [p.x, p.z]),
        brush: state.brush || state.lasso,
        fill: state.lasso
            && !matches!(
                state.tool,
                BlockTool::Ramp | BlockTool::Cut | BlockTool::Place | BlockTool::Select
            ),
        radius,
        style: state.style,
        ..BlockOp::new(state.tool, (0, 0), (0, 0))
    };
    let mut ghost = None;
    let mut moved: Vec<(usize, Vec3)> = Vec::new();
    match (&session.blocks.drag, column) {
        (Some(drag), Some((c, p))) => {
            let mut drag = drag.clone();
            drag.to = c;
            let last = drag.path.last().copied().unwrap_or([p.x, p.z]);
            if Vec2::from_array(last).distance(Vec2::new(p.x, p.z)) > 0.75 {
                drag.path.push([p.x, p.z]);
            }
            op.from = drag.from;
            op.to = drag.to;
            op.path = drag.path.clone();
            if op.tool != BlockTool::Select {
                if strokes {
                    preview(&op, SOFT);
                } else {
                    outline(drag.from, c, SOFT);
                }
                shown = Some(op.clone());
            } else if let (Some(a), Some(content)) = (drag.path.first(), content)
                && selection_under(session, *a).is_some()
            {
                // The selection follows the pointer, snapped as it will land, red where
                // something may not stand.
                let delta = snapped_delta(
                    session,
                    Some(content),
                    (f64::from(p.x - a[0]), f64::from(p.z - a[1])),
                );
                let fits = move_fits(session, content, terrain.as_deref(), delta);
                let color = if fits.is_ok() {
                    Color32::from_rgb(255, 214, 120)
                } else {
                    Color32::from_rgb(240, 90, 80)
                };
                let keys = session
                    .scene
                    .as_ref()
                    .map(|s| s.keys.clone())
                    .unwrap_or_default();
                for (kind, id, x, z) in carried(session) {
                    let (nx, nz) = ((x + delta.0) as f32, (z + delta.1) as f32);
                    let y = crate::map::tools::ground_at(map, nx, nz);
                    let radius = if kind == Picked::Start { 8.0 } else { 3.0 };
                    ring(Vec3::new(nx, y, nz), radius, color);
                    if let Some(i) = keys.iter().position(|k| k.0 == kind && k.1 == id) {
                        moved.push((i, Vec3::new(nx, y, nz)));
                    }
                }
                if let (Err(why), Some(at)) = (&fits, response.hover_pos()) {
                    reason(&painter, at, why);
                }
            }
            if op.tool == BlockTool::Cells {
                for q in &op.path {
                    let y = crate::map::tools::ground_at(map, q[0], q[1]);
                    ring(
                        Vec3::new(q[0], y, q[1]),
                        radius.max(0.5),
                        Color32::from_white_alpha(60),
                    );
                }
            }
            if op.tool == BlockTool::Ramp
                && let (Some(a), Some(b)) = (
                    drag.path.first().and_then(|q| {
                        let y = crate::map::tools::ground_at(map, q[0], q[1]) + 0.3;
                        project(view_projection, rect, Vec3::new(q[0], y, q[1]))
                    }),
                    project(view_projection, rect, p + Vec3::Y * 0.3),
                )
            {
                painter.arrow(a, b - a, Line::new(2.5, Color32::from_rgb(255, 214, 120)));
            }
            session.blocks.drag = Some(drag);
            // The map follows the drag (not for cuts and placements, which are clicks).
            if !matches!(
                op.tool,
                BlockTool::Place | BlockTool::Cut | BlockTool::Select
            ) {
                live = Some(op.clone());
            }
        }
        (None, Some((c, p))) => match state.tool {
            BlockTool::Cells => ring(p, radius.max(0.5), Color32::from_white_alpha(200)),
            // Place shows where it will land and what it puts down (its ghost), red with
            // the reason where it may not stand.
            BlockTool::Place => {
                let gold = Color32::from_rgb(255, 214, 120);
                let red = Color32::from_rgb(240, 90, 80);
                if let Some(content) = content {
                    let land = landing(content, &state.placing, [p.x, p.z], state.fine);
                    let why = terrain
                        .as_deref()
                        .and_then(|t| problem(map, content, t, &state.placing, &land, &[]));
                    let color = if why.is_some() { red } else { gold };
                    let (x, z) = (land.x as f32, land.z as f32);
                    match land.footprint {
                        Some((w, h)) => {
                            ground_rect(x, z, w as f32, h as f32, color);
                        }
                        None => {
                            let y = crate::map::tools::ground_at(map, x, z);
                            ring(Vec3::new(x, y, z), 1.2, color);
                        }
                    }
                    let owner = match &state.placing {
                        Placing::Start(n) => Some(usize::from(n.saturating_sub(1))),
                        Placing::Definition(_)
                            if state.owner > 0
                                && state
                                    .catalog
                                    .find(&state.placing)
                                    .is_some_and(|e| !e.neutral) =>
                        {
                            Some(usize::from(state.owner - 1))
                        }
                        _ => None,
                    };
                    let degrees = match &state.placing {
                        Placing::Start(_) => {
                            let setup = content
                                .rules
                                .races
                                .values()
                                .filter_map(|r| r.starting_setup.as_ref())
                                .next()
                                .unwrap_or(&content.rules.starting_setup);
                            crate::map::tools::facing_middle(
                                content,
                                setup.fort.as_str(),
                                land.x,
                                land.z,
                                map_size,
                            ) as f32
                        }
                        _ => 0.0,
                    };
                    if let Some((_, scale)) = session.scene.as_ref().and_then(|s| s.ghost) {
                        let colors = session.scene.as_ref().map(|s| s.colors.clone());
                        let y = crate::map::tools::ground_at(map, x, z);
                        ghost = Some((
                            glam::Mat4::from_scale_rotation_translation(
                                Vec3::splat(scale as f32),
                                glam::Quat::from_rotation_y(degrees.to_radians()),
                                Vec3::new(x, y, z),
                            ),
                            owner.and_then(|o| colors?.get(o).copied()),
                            why.is_none(),
                        ));
                    }
                    if let (Some(why), Some(at)) = (&why, response.hover_pos()) {
                        reason(&painter, at, why);
                    }
                } else {
                    ring(p, 4.0, gold);
                }
            }
            // Select rings what a click would pick.
            BlockTool::Select => {
                if let Some((kind, id)) = crate::map::view::nearest(session, p.x, p.z)
                    && let Some((x, z)) = selection_at(map, kind, &id)
                {
                    let y = crate::map::tools::ground_at(map, x as f32, z as f32);
                    let radius = if kind == Picked::Start { 8.0 } else { 3.0 };
                    ring(
                        Vec3::new(x as f32, y, z as f32),
                        radius,
                        Color32::from_white_alpha(170),
                    );
                }
            }
            _ if strokes && state.tool != BlockTool::Ramp => {
                op.from = c;
                op.to = c;
                op.path = vec![[p.x, p.z]];
                preview(&op, Color32::from_white_alpha(200));
                shown = Some(op.clone());
            }
            _ => {
                outline(c, c, Color32::from_white_alpha(200));
                op.from = c;
                op.to = c;
                op.path = vec![[p.x, p.z]];
                shown = Some(op.clone());
            }
        },
        _ => {}
    }
    session.overlay.ghost = ghost;
    session.overlay.moved = std::mem::take(&mut moved);
    // The mirror's twin of what the pointer would change.
    if symmetry != Symmetry::None
        && let Some(o) = &shown
    {
        let twin = symmetry.op(o, map_size, side);
        let faint = Color32::from_rgba_unmultiplied(255, 214, 120, 110);
        if twin.tool == BlockTool::Cells {
            for q in &twin.path {
                let y = crate::map::tools::ground_at(map, q[0], q[1]);
                ring(Vec3::new(q[0], y, q[1]), radius.max(0.5), faint);
            }
        } else if strokes {
            preview(&twin, faint);
        } else {
            outline(twin.from, twin.to, faint);
        }
    }

    if response.drag_started_by(egui::PointerButton::Primary)
        && let Some((c, p)) = column
    {
        // A drag begins where the button went down, not where it has moved to by the
        // time it counts as a drag: what was pressed on is what moves.
        let (c, p) = input
            .pointer
            .press_origin()
            .and_then(|o| pick(session, view_projection, eye, rect, o))
            .map_or((c, p), |o| (blocks.column_at(o.x, o.z), o));
        session.blocks.drag = Some(Drag {
            from: c,
            to: c,
            path: vec![[p.x, p.z]],
        });
    }
    let mut finished = None;
    if response.drag_stopped_by(egui::PointerButton::Primary) {
        finished = session.blocks.drag.take();
    } else if response.clicked_by(egui::PointerButton::Primary)
        && let Some((c, p)) = column
    {
        finished = Some(Drag {
            from: c,
            to: c,
            path: vec![[p.x, p.z]],
        });
    }
    if session.blocks.drag.is_none() && finished.is_none() {
        restore(session);
    }
    if let Some(drag) = finished {
        restore(session);
        op.from = drag.from;
        op.to = drag.to;
        op.path = drag.path;
        if op.tool == BlockTool::Select {
            let moved = match (op.path.first(), op.path.last()) {
                (Some(a), Some(b)) if op.path.len() > 1 => {
                    Some((f64::from(b[0] - a[0]), f64::from(b[1] - a[1])))
                }
                _ => None,
            };
            let adding = input.modifiers.shift;
            let on_selection = op
                .path
                .first()
                .is_some_and(|a| selection_under(session, *a).is_some());
            match (moved, docs) {
                // A drag that begins on the selection moves all of it.
                (Some(delta), Some(docs)) if on_selection => {
                    let snapped = snapped_delta(session, content, delta);
                    let fits = match content {
                        Some(content) => move_fits(session, content, terrain.as_deref(), snapped),
                        None => Ok(()),
                    };
                    // Where something may not stand, the move does not happen.
                    if fits.is_ok() {
                        let commands = move_all(session, content, docs, delta);
                        if !commands.is_empty() {
                            out.edits.push(step(session, "Move", commands));
                        }
                    }
                }
                // Any other drag selects what stands in its rectangle.
                (Some(_), _) => {
                    if let (Some(a), Some(b), Some(map)) =
                        (op.path.first(), op.path.last(), session.map.as_ref())
                    {
                        let corner = |p: &[f32; 2]| {
                            let y = crate::map::tools::ground_at(map, p[0], p[1]);
                            project(view_projection, rect, Vec3::new(p[0], y, p[1]))
                        };
                        if let (Some(a), Some(b)) = (corner(a), corner(b)) {
                            let area = Rect::from_two_pos(a, b);
                            let inside = placed(map)
                                .into_iter()
                                .filter(|(_, _, x, z)| {
                                    let y = crate::map::tools::ground_at(map, *x as f32, *z as f32);
                                    project(
                                        view_projection,
                                        rect,
                                        Vec3::new(*x as f32, y, *z as f32),
                                    )
                                    .is_some_and(|p| area.contains(p))
                                })
                                .map(|(kind, id, _, _)| (kind, id));
                            if !adding {
                                session.blocks.selection.clear();
                            }
                            for item in inside.collect::<Vec<_>>() {
                                if !session.blocks.selection.contains(&item) {
                                    session.blocks.selection.push(item);
                                }
                            }
                        }
                    }
                }
                // A click picks what is nearest; Shift adds it or takes it away.
                _ => {
                    if let Some(at) = op.path.first() {
                        let picked = crate::map::view::nearest(session, at[0], at[1]);
                        let selection = &mut session.blocks.selection;
                        match picked {
                            Some(item) if adding => match selection.iter().position(|s| *s == item)
                            {
                                Some(i) => {
                                    selection.remove(i);
                                }
                                None => selection.push(item),
                            },
                            Some(item) => *selection = vec![item],
                            None if !adding => selection.clear(),
                            None => {}
                        }
                    }
                }
            }
        } else if op.tool == BlockTool::Place {
            if let (Some(content), Some(docs), Some(&at)) = (content, docs, op.path.first()) {
                let placing = session.blocks.placing.clone();
                let owner = session.blocks.owner;
                let loaded = session.loaded_at;
                // Where it may not stand, nothing is placed (the ghost says why).
                let land = landing(content, &placing, at, session.blocks.fine);
                let fits = match (session.map.as_ref(), terrain.as_deref()) {
                    (Some(map), Some(t)) => {
                        problem(map, content, t, &placing, &land, &[]).is_none()
                    }
                    _ => true,
                };
                if fits {
                    out.edits
                        .extend(place(session, content, docs, &placing, owner, at));
                }
                if fits && symmetry != Symmetry::None {
                    // The mirror's twin: the next player's start, or the next player's
                    // unit or building.
                    let next = |n: u8| n % 8 + 1;
                    session.blocks.twin = Some(Twin {
                        placing: match placing {
                            Placing::Start(n) => Placing::Start(next(n)),
                            other => other,
                        },
                        at: symmetry.point(at, map_size),
                        owner: if owner == 0 { 0 } else { next(owner) },
                        after: loaded,
                    });
                }
            }
        } else {
            out.edits.extend(apply(session, &op));
        }
    }
    // A placement's twin, once the map has taken the first.
    if let Some(twin) = session.blocks.twin.clone()
        && let (Some(content), Some(docs)) = (content, docs)
        && session.loaded_at != twin.after
        && out.edits.is_empty()
    {
        session.blocks.twin = None;
        let land = landing(content, &twin.placing, twin.at, session.blocks.fine);
        let terrain = sim_terrain(session, content);
        let fits = match (session.map.as_ref(), terrain.as_deref()) {
            (Some(map), Some(t)) => problem(map, content, t, &twin.placing, &land, &[]).is_none(),
            _ => true,
        };
        if fits {
            out.edits.extend(place(
                session,
                content,
                docs,
                &twin.placing,
                twin.owner,
                twin.at,
            ));
        }
    }
    // The rectangle a Select drag is drawing.
    if session.blocks.tool == BlockTool::Select
        && let (Some(drag), Some(map), Some(pointer)) = (
            session.blocks.drag.as_ref(),
            session.map.as_ref(),
            response.hover_pos().or(response.interact_pointer_pos()),
        )
        && let Some(a) = drag.path.first()
        && selection_under(session, *a).is_none()
    {
        let y = crate::map::tools::ground_at(map, a[0], a[1]);
        if let Some(start) = project(view_projection, rect, Vec3::new(a[0], y, a[1])) {
            let area = Rect::from_two_pos(start, pointer);
            painter.rect(
                area,
                2,
                Color32::from_rgba_unmultiplied(255, 214, 120, 18),
                Line::new(1.0, Color32::from_rgb(255, 214, 120)),
                egui::StrokeKind::Inside,
            );
        }
    }
    if let Some((c, p)) = column
        && let Some(b) = blocks.get(c.0, c.1)
    {
        let halo = MAP_HALO as f32;
        let cell = ((p.x + halo).floor() as i32, (p.z + halo).floor() as i32);
        let mut text = format!("level {}  ·  {}", b.level, b.kind.name());
        if let Some(kind) = blocks.painted(cell.0, cell.1) {
            text += &format!("  ·  cell {}", kind.name());
        }
        if b.kind == BlockKind::Ramp && b.ramp > 0 {
            const NAMES: [&str; 8] = [
                "east",
                "south-east",
                "south",
                "south-west",
                "west",
                "north-west",
                "north",
                "north-east",
            ];
            text += &format!("  ·  falls {}", NAMES[(b.ramp - 1) as usize]);
        }
        if b.style > 0 {
            let name = session
                .blocks
                .faces
                .get(b.style as usize - 1)
                .cloned()
                .unwrap_or_else(|| format!("face {}", b.style));
            text += &format!("  ·  {name}");
        }
        painter.text(
            rect.left_top() + egui::vec2(12.0, 12.0),
            egui::Align2::LEFT_TOP,
            text,
            egui::FontId::proportional(12.0),
            TEXT,
        );
    }
    // The selection: a ring round each, the name of one alone; Delete takes it away (a
    // camp member its camp).
    if session.blocks.tool == BlockTool::Select && !session.blocks.selection.is_empty() {
        let gold = Color32::from_rgb(255, 214, 120);
        let single = session.blocks.selection.len() == 1;
        let mut kept = Vec::new();
        if let Some(map) = session.map.as_ref() {
            for (kind, id) in &session.blocks.selection {
                let Some((x, z)) = selection_at(map, *kind, id) else {
                    continue;
                };
                kept.push((*kind, id.clone()));
                let y = crate::map::tools::ground_at(map, x as f32, z as f32);
                let radius = if *kind == Picked::Start { 8.0 } else { 3.0 };
                let points: Vec<_> = (0..=32)
                    .filter_map(|k| {
                        let a = k as f32 / 32.0 * std::f32::consts::TAU;
                        let p = Vec2::new(x as f32, z as f32) + Vec2::from_angle(a) * radius;
                        project(view_projection, rect, Vec3::new(p.x, y + 0.3, p.y))
                    })
                    .collect();
                painter.add(egui::Shape::line(points, Line::new(2.5, gold)));
                if single
                    && let Some(p) = project(
                        view_projection,
                        rect,
                        Vec3::new(x as f32, y + 0.3, z as f32),
                    )
                {
                    painter.text(
                        p + egui::vec2(0.0, -14.0),
                        egui::Align2::CENTER_BOTTOM,
                        match kind {
                            Picked::Start => format!("start of player {id}"),
                            _ => id.clone(),
                        },
                        egui::FontId::proportional(11.0),
                        gold,
                    );
                }
            }
        }
        session.blocks.selection = kept;
        if response.hovered()
            && (input.key_pressed(egui::Key::Delete) || input.key_pressed(egui::Key::Backspace))
            && let Some(docs) = docs
        {
            let commands = crate::map::view::delete_all(session, docs, &session.blocks.selection);
            if !commands.is_empty() {
                out.edits.push(step(session, "Delete", commands));
                session.blocks.selection.clear();
            }
        }
    }
    // The playable area: a strong line round it, on the ground, and its size at a corner.
    if let Some(map) = session.map.as_ref() {
        let area = map.playable_area();
        let (x0, z0) = (area.x as f32, area.z as f32);
        let (x1, z1) = ((area.x + area.width) as f32, (area.z + area.height) as f32);
        let corners = [(x0, z0), (x1, z0), (x1, z1), (x0, z1), (x0, z0)];
        let mut line = Vec::new();
        let mut lines = Vec::new();
        for pair in corners.windows(2) {
            let ((ax, az), (bx, bz)) = (pair[0], pair[1]);
            let steps = ((bx - ax).abs() + (bz - az).abs()).max(1.0) as usize;
            for k in 0..=steps {
                let t = k as f32 / steps as f32;
                let (x, z) = (ax + (bx - ax) * t, az + (bz - az) * t);
                let y = crate::map::tools::ground_at(map, x, z) + 0.25;
                match project(view_projection, rect, Vec3::new(x, y, z)) {
                    Some(p) if rect.expand(200.0).contains(p) => line.push(p),
                    _ => {
                        if line.len() > 1 {
                            lines.push(std::mem::take(&mut line));
                        }
                        line.clear();
                    }
                }
            }
        }
        if line.len() > 1 {
            lines.push(line);
        }
        for l in lines {
            painter.add(egui::Shape::line(
                l.clone(),
                Line::new(5.0, Color32::from_black_alpha(150)),
            ));
            painter.add(egui::Shape::line(
                l,
                Line::new(2.5, Color32::from_rgb(255, 214, 120)),
            ));
        }
        if let Some(p) = project(
            view_projection,
            rect,
            Vec3::new(x0, crate::map::tools::ground_at(map, x0, z0) + 0.25, z0),
        ) && rect.contains(p)
        {
            painter.text(
                p + egui::vec2(6.0, -6.0),
                egui::Align2::LEFT_BOTTOM,
                format!("playable {} × {}", area.width, area.height),
                egui::FontId::proportional(11.0),
                Color32::from_rgb(255, 214, 120),
            );
        }
    }
    // The tool strips, floating over the foot of the view.
    if let (Some(start), Some((left, right))) = (mirror_from, panes) {
        let other = if session.blocks.right_pane {
            left
        } else {
            right
        };
        let delta = other.min - rect.min;
        let layer = ui.layer_id();
        ui.ctx().graphics_mut(|g| {
            let list = g.entry(layer);
            let copies: Vec<egui::Shape> = list
                .all_entries()
                .skip(start)
                .map(|c| {
                    let mut shape = c.shape.clone();
                    shape.translate(delta);
                    shape
                })
                .collect();
            list.extend(other, copies);
        });
    }
    out.edits.extend(strip::show(ui.ctx(), session, full, docs));
    strip::view_bar(ui.ctx(), session, full);
    if let Some(op) = live
        && session.blocks.drag.is_some()
    {
        live_preview(session, &op);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn op(tool: BlockTool, from: (i32, i32), to: (i32, i32)) -> BlockOp {
        BlockOp::new(tool, from, to)
    }

    #[test]
    fn a_point_mirrored_edit_leaves_the_blocks_symmetric() {
        let mut blocks = Blocks::flat(256);
        let side = blocks.side as i32;
        let raise = op(BlockTool::Raise, (10, 12), (14, 15));
        let mut ramp = op(BlockTool::Ramp, (0, 0), (0, 0));
        ramp.path = vec![[40.0, 30.0], [60.0, 50.0]];
        ramp.level = Some(2);
        for edit in [raise, ramp] {
            let start = blocks.at(edit.from.0, edit.from.1);
            change(&mut blocks, &edit, start);
            let twin = Symmetry::Point.op(&edit, 256, side);
            let start = blocks.at(twin.from.0, twin.from.1);
            change(&mut blocks, &twin, start);
        }
        for z in 0..side {
            for x in 0..side {
                let (a, b) = (blocks.at(x, z), blocks.at(side - 1 - x, side - 1 - z));
                assert_eq!((a.level, a.kind), (b.level, b.kind), "at {x}, {z}");
                // The twin ramp falls the other way, half a turn round.
                if a.kind == BlockKind::Ramp {
                    assert_eq!((a.ramp + 3) % 8 + 1, b.ramp);
                }
            }
        }
    }

    #[test]
    fn round_cuts_the_turning_corners_of_a_plateau_only() {
        let mut blocks = Blocks::flat(256);
        for z in 20..26 {
            for x in 20..26 {
                blocks.set(x, z, Block::ground(2));
            }
        }
        let all = op(BlockTool::Round, (18, 18), (28, 28));
        let start = blocks.at(18, 18);
        change(&mut blocks, &all, start);
        assert_eq!(blocks.at(20, 20).cut, Cut::NorthWest);
        assert_eq!(blocks.at(25, 20).cut, Cut::NorthEast);
        assert_eq!(blocks.at(25, 25).cut, Cut::SouthEast);
        assert_eq!(blocks.at(20, 25).cut, Cut::SouthWest);
        // Along the straight edges and inside, nothing is cut.
        assert_eq!(blocks.at(22, 20).cut, Cut::None);
        assert_eq!(blocks.at(22, 22).cut, Cut::None);
    }
}
