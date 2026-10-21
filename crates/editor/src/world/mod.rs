//! The block editor (D80): a map is shaped as columns of blocks, 4 × 4 cells each, and
//! its heights, water and closed cells are made from them: raise and lower, set a level,
//! paint a kind, ramps dragged from top to foot, diagonal cuts, kinds painted cell by
//! cell, and the face each cliff takes. Block tools work on a dragged box or along a
//! brush stroke. The Generated view (G) shows the world the biome makes of the blocks.
//!
//! Every edit changes the blocks, makes the map's grids from them (`Blocks::apply`) and
//! becomes one undo step: the blocks and closed cells whole, heights and water as patches
//! over what changed.

pub mod mesh;
pub mod palette;
mod strip;

use crate::document::{Command, Sample};
use crate::map::Session;
use crate::map::tools::Picked;
use crate::map::view::{Out, matrices, navigate, pick, project, step};
use crate::theme::*;
use crate::widgets::eyebrow;
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
    #[default]
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
                "Click to place a start (with its amber), an amber site or a creep camp"
            }
            BlockTool::Round => {
                "Drag over a plateau's edge to cut its corners at 45° (Shift uncuts)"
            }
            BlockTool::Select => {
                "Click a start, amber site, camp or prop; drag it to move it, Delete removes it"
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

/// What Place puts down.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Placing {
    #[default]
    Start,
    Amber,
    Camp,
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
    /// Show the world generated from the blocks instead of the blocks (G).
    pub generated: bool,
    /// The names of the biome's faces (face style 1 onwards).
    pub faces: Vec<String>,
    pub placing: Placing,
    pub symmetry: Symmetry,
    /// A placement's twin, waiting for the map to take the first (a start's player
    /// number comes from the starts already there): what, where, and how many starts
    /// the map has once the first is in.
    pub twin: Option<(Placing, [f32; 2], usize)>,
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
            generated: false,
            faces: Vec::new(),
            placing: Placing::default(),
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

/// Blocks out the session's map from its heights, water and closed ground, or as flat
/// level-1 ground (`flat`).
pub fn block_out(session: &mut Session, flat: bool) -> Option<crate::tabs::Edit> {
    let map = session.map.as_ref()?;
    if flat {
        let blocks = Blocks::flat(map.size);
        return commit(session, "Flat blocks", blocks, |_| {});
    }
    let starts: Vec<[f32; 2]> = map
        .player_starts
        .iter()
        .map(|s| [s.x as f32, s.z as f32])
        .collect();
    let blocks = Blocks::from_terrain(&map.authoring.terrain, &starts);
    commit(session, "Block out the map", blocks, |_| {})
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

/// The smallest playable square the map rules allow (cells).
const MIN_PLAYABLE: u32 = 64;

/// The map's playable area as borders (cells) left, top, right and bottom.
fn borders(map: &canopy_content::map::UtcMap) -> [u32; 4] {
    let a = map.playable_area();
    [
        a.x,
        a.z,
        map.size - a.x - a.width,
        map.size - a.z - a.height,
    ]
}

/// The step that gives the map these borders (none when they are its own): the whole map
/// playable leaves the field out.
fn set_borders(session: &Session, b: [u32; 4]) -> Option<crate::tabs::Edit> {
    let map = session.map.as_ref()?;
    if borders(map) == b {
        return None;
    }
    let document = session.path("map.json");
    let command = if b == [0; 4] {
        map.playable.is_some().then(|| Command::Remove {
            document: document.clone(),
            pointer: "/playable".into(),
        })?
    } else {
        Command::Set {
            document,
            pointer: "/playable".into(),
            value: serde_json::json!({
                "x": b[0],
                "z": b[1],
                "width": map.size - b[0] - b[2],
                "height": map.size - b[1] - b[3],
            }),
            at: None,
        }
    };
    let mut edit = step(session, "Playable area", vec![command]);
    // A dragged border is one undo step.
    edit.merge = Some(format!("{}/playable", session.root));
    Some(edit)
}

/// The playable area's controls: one border all round, or each side on its own, in steps
/// of a block so the outline runs on block edges.
fn playable_panel(ui: &mut Ui, session: &mut Session) -> Option<crate::tabs::Edit> {
    let map = session.map.as_ref()?;
    let size = map.size;
    let mut b = borders(map);
    let before = b;
    let most = (size - MIN_PLAYABLE) / 2;
    ui.label(
        RichText::new(format!(
            "{} × {} cells playable; the border around it is forest nobody enters.",
            size - b[0] - b[2],
            size - b[1] - b[3]
        ))
        .size(11.0)
        .color(MUTED),
    );
    let mut all = *b.iter().max().unwrap_or(&0);
    if ui
        .add(
            egui::Slider::new(&mut all, 0..=most)
                .step_by(BLOCK as f64)
                .text("border, all round"),
        )
        .changed()
    {
        b = [all; 4];
    }
    egui::CollapsingHeader::new(RichText::new("Each side").size(11.0))
        .id_salt(("playable-sides", &session.root))
        .show(ui, |ui| {
            egui::Grid::new(("playable-grid", &session.root))
                .num_columns(4)
                .spacing([6.0, 4.0])
                .show(ui, |ui| {
                    for (k, name) in ["Left", "Top", "Right", "Bottom"].iter().enumerate() {
                        ui.label(RichText::new(*name).size(11.0).color(MUTED));
                        let opposite = b[(k + 2) % 4];
                        let room = size.saturating_sub(MIN_PLAYABLE + opposite);
                        ui.add(
                            egui::DragValue::new(&mut b[k])
                                .range(0..=room)
                                .speed(1.0)
                                .suffix(" cells"),
                        );
                        if k % 2 == 1 {
                            ui.end_row();
                        }
                    }
                });
        });
    // On block edges, and never smaller than the rules allow.
    for side in &mut b {
        *side = (*side + BLOCK / 2) / BLOCK * BLOCK;
    }
    for k in 0..2 {
        while size.saturating_sub(b[k] + b[k + 2]) < MIN_PLAYABLE && b[k] + b[k + 2] > 0 {
            if b[k] >= b[k + 2] {
                b[k] -= BLOCK.min(b[k]);
            } else {
                b[k + 2] -= BLOCK.min(b[k + 2]);
            }
        }
    }
    if b == before {
        return None;
    }
    set_borders(session, b)
}

/// The left panel; returns an edit when one of its buttons made one.
fn panel(ui: &mut Ui, session: &mut Session) -> Option<crate::tabs::Edit> {
    let mut edit = None;
    ui.label(eyebrow("Blocks"));
    ui.label(
        RichText::new(
            "Shape the map in 4 × 4-cell blocks; heights, cliffs, water and closed ground follow.",
        )
        .size(11.0)
        .color(MUTED),
    );
    ui.add_space(8.0);
    let has = blocks_of(session).is_some();
    if !has {
        ui.label(
            RichText::new("This map has no blocks yet.")
                .size(12.0)
                .color(TEXT),
        );
        ui.add_space(4.0);
        if ui.button("Block out this map").on_hover_text("Blocks from the map's heights, water and closed ground; the map's heights then follow the blocks").clicked() {
            edit = block_out(session, false);
        }
        if ui.button("Start from flat ground").clicked() {
            edit = block_out(session, true);
        }
        ui.add_space(10.0);
    }
    ui.add_space(12.0);
    ui.label(eyebrow("Playable area"));
    edit = edit.or(playable_panel(ui, session));
    ui.add_space(12.0);
    ui.label(eyebrow("Symmetry"));
    ui.horizontal_wrapped(|ui| {
        for symmetry in Symmetry::ALL {
            if ui
                .selectable_label(session.blocks.symmetry == symmetry, symmetry.name())
                .on_hover_text("Every edit and placement is made in the mirror too (Y cycles)")
                .clicked()
            {
                session.blocks.symmetry = symmetry;
            }
        }
    });
    ui.add_space(12.0);
    ui.label(
        RichText::new(
            "B box · brush · lasso  ·  [ ] size  ·  Y symmetry  ·  G view  ·  Shift reverses",
        )
        .size(10.5)
        .color(FAINT),
    );
    ui.add_space(12.0);
    ui.label(eyebrow("Map"));
    if ui
        .button("Write map picture")
        .on_hover_text("The map picker's picture (preview.webp), from the map as it is now; cook to see it in the game")
        .clicked()
    {
        session.blocks.write_picture = true;
    }
    if let Some(error) = &session.error {
        ui.add_space(6.0);
        ui.label(RichText::new(error).color(ERROR).size(11.0));
    }
    edit
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

/// Places what Place puts down at `at` (a start with its amber, an amber site or a camp),
/// as MCP's `map_place` does; in blocks a start needs no pad, the blocks are its ground.
fn place(
    session: &mut Session,
    content: &canopy_content::Content,
    docs: &crate::map::view::Docs,
    placing: Placing,
    at: [f32; 2],
) -> Option<crate::tabs::Edit> {
    use crate::map::tools::Placement;
    let tool = match placing {
        Placing::Start => Placement::Start,
        Placing::Amber => Placement::Amber,
        Placing::Camp => Placement::Camp,
    };
    let mut edit = crate::map::view::place(
        session,
        content,
        docs,
        tool,
        f64::from(at[0]),
        f64::from(at[1]),
    )
    .ok()
    .flatten()?;
    edit.commands.retain(
        |c| !matches!(c, Command::Patch { document, .. } if document.ends_with("heights.grid")),
    );
    Some(edit)
}

/// The block view of `session`; `texture` is the scene's last picture.
pub fn show(
    ui: &mut Ui,
    session: &mut Session,
    content: Option<&canopy_content::Content>,
    docs: Option<&crate::map::view::Docs>,
    texture: Option<egui::TextureId>,
) -> Out {
    let mut out = Out::default();
    let mut panel_edit = None;
    egui::Panel::left(egui::Id::new(("block-tools", &session.root)))
        .exact_size(214.0)
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
                .show(ui, |ui| panel_edit = panel(ui, session));
        });
    out.edits.extend(panel_edit);
    out.preview = std::mem::take(&mut session.blocks.write_picture);
    let rect = ui.available_rect_before_wrap();
    let response = ui.allocate_rect(rect, Sense::click_and_drag());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0, RAISED);
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
    // G switches the view wherever the pointer is, unless a field is being typed in.
    if input.key_pressed(egui::Key::G)
        && !input.modifiers.command
        && !ui.ctx().egui_wants_keyboard_input()
    {
        session.blocks.generated = !session.blocks.generated;
        session.stale = true;
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
            "Block out the map, or start from flat ground (left)",
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
            if strokes {
                preview(&op, SOFT);
            } else {
                outline(drag.from, c, SOFT);
            }
            shown = Some(op.clone());
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
            BlockTool::Place => ring(p, 4.0, Color32::from_rgb(255, 214, 120)),
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
            // A drag that begins on the selection moves it; anything else selects.
            let on_selection = |session: &Session| {
                let (Some((kind, id)), Some(a)) = (session.tools.selected.clone(), op.path.first())
                else {
                    return false;
                };
                session
                    .map
                    .as_ref()
                    .and_then(|m| selection_at(m, kind, &id))
                    .is_some_and(|(x, z)| (x as f32 - a[0]).hypot(z as f32 - a[1]) < 6.0)
            };
            match (moved, docs) {
                (Some(delta), Some(docs)) if on_selection(session) => {
                    if let Some((kind, id)) = session.tools.selected.clone() {
                        let commands = move_selection(session, docs, kind, &id, delta);
                        if !commands.is_empty() {
                            out.edits.push(step(session, "Move", commands));
                        }
                    }
                }
                _ => {
                    if let Some(at) = op.path.first() {
                        session.tools.selected = crate::map::view::nearest(session, at[0], at[1]);
                    }
                }
            }
        } else if op.tool == BlockTool::Place {
            if let (Some(content), Some(docs), Some(&at)) = (content, docs, op.path.first()) {
                let placing = session.blocks.placing;
                let starts = session.map.as_ref().map_or(0, |m| m.player_starts.len());
                out.edits.extend(place(session, content, docs, placing, at));
                if symmetry != Symmetry::None {
                    let twin = symmetry.point(at, map_size);
                    session.blocks.twin = Some((placing, twin, starts + 1));
                }
            }
        } else {
            out.edits.extend(apply(session, &op));
        }
    }
    // A placement's twin, once the map has taken the first.
    if let Some((placing, at, starts)) = session.blocks.twin
        && let (Some(content), Some(docs)) = (content, docs)
        && session
            .map
            .as_ref()
            .is_some_and(|m| placing != Placing::Start || m.player_starts.len() >= starts)
        && out.edits.is_empty()
    {
        session.blocks.twin = None;
        out.edits.extend(place(session, content, docs, placing, at));
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
    // The selection: a ring round it; Delete takes it away (a camp member its camp).
    if session.blocks.tool == BlockTool::Select
        && let Some((kind, id)) = session.tools.selected.clone()
    {
        match session
            .map
            .as_ref()
            .and_then(|m| selection_at(m, kind, &id))
        {
            Some((x, z)) => {
                if let Some(map) = session.map.as_ref() {
                    let y = crate::map::tools::ground_at(map, x as f32, z as f32);
                    let radius = if kind == Picked::Start { 8.0 } else { 3.0 };
                    let points: Vec<_> = (0..=32)
                        .filter_map(|k| {
                            let a = k as f32 / 32.0 * std::f32::consts::TAU;
                            let p = Vec2::new(x as f32, z as f32) + Vec2::from_angle(a) * radius;
                            project(view_projection, rect, Vec3::new(p.x, y + 0.3, p.y))
                        })
                        .collect();
                    painter.add(egui::Shape::line(
                        points,
                        Line::new(2.5, Color32::from_rgb(255, 214, 120)),
                    ));
                    if let Some(p) = project(
                        view_projection,
                        rect,
                        Vec3::new(x as f32, y + 0.3, z as f32),
                    ) {
                        painter.text(
                            p + egui::vec2(0.0, -14.0),
                            egui::Align2::CENTER_BOTTOM,
                            match kind {
                                Picked::Start => format!("start of player {id}"),
                                _ => id.clone(),
                            },
                            egui::FontId::proportional(11.0),
                            Color32::from_rgb(255, 214, 120),
                        );
                    }
                }
                if response.hovered()
                    && (input.key_pressed(egui::Key::Delete)
                        || input.key_pressed(egui::Key::Backspace))
                    && let Some(docs) = docs
                {
                    let commands = crate::map::view::delete(session, docs, kind, &id);
                    if !commands.is_empty() {
                        out.edits.push(step(session, "Delete", commands));
                        session.tools.selected = None;
                    }
                }
            }
            None => session.tools.selected = None,
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
    out.edits.extend(strip::show(ui.ctx(), session, rect, docs));
    strip::view_bar(ui.ctx(), session, rect);
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
