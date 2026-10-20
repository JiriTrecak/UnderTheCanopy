//! A map's blockout (D80): the map authored as columns of blocks, 4 × 4 cells each,
//! stacked in whole cliff levels, from which its heights, water and closed cells are made.
//! The simulation never reads the blocks, only what they make, so a map plays the same
//! however the look generated from them changes.
//!
//! `terrain/blocks.grid` holds one 4-byte sample per column (halo included): level, kind,
//! diagonal cut with a ramp's direction in its high half, and the face style its cliffs
//! take. Columns run from world −[`MAP_HALO`], `BLOCK` cells apart; a 480-cell map is
//! 128 × 128 columns.
//!
//! `terrain/block-paint.grid`, optional, paints kinds cell by cell over the columns (one
//! byte per cell: 0 keeps the column's kind, otherwise kind + 1), so an outline that is
//! only a change of kind, such as a shore or a frame's edge, can run at any angle; levels
//! stay whole columns.
//!
//! A ramp is a region: connected ramp columns of one level and one direction slope as one
//! plane, falling one level along their direction across the whole region, so ramps can be
//! wide, long and diagonal.

use super::{DRY_WATER_HEIGHT, MAP_HALO, TerrainData, grid::Grid};

/// Cells per block side (the building lattice).
pub const BLOCK: u32 = 4;
/// Height of one level in metres (one cliff level).
pub const LEVEL_HEIGHT: f32 = 3.0;
/// The highest level (heights stay within the map's range).
pub const MAX_LEVEL: u8 = 7;
/// Ground at level 0 stands at this height, so water can lie below it.
pub const BASE_HEIGHT: f32 = 0.0;

/// What a column is for play.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum BlockKind {
    /// Walkable ground at the column's level.
    #[default]
    Ground = 0,
    /// A slope from the column's level down one level, towards its lower neighbour.
    Ramp = 1,
    /// Impassable forest floor (the dark frame).
    Wall = 2,
    /// Impassable rock.
    Rock = 3,
    /// Water units can wade, its bed just under the level.
    Shallow = 4,
    /// Deep water nothing walks through.
    Deep = 5,
    /// Frozen water: walkable ice just under the column's level.
    Ice = 6,
}

impl BlockKind {
    pub const ALL: [BlockKind; 7] = [
        BlockKind::Ground,
        BlockKind::Ramp,
        BlockKind::Wall,
        BlockKind::Rock,
        BlockKind::Shallow,
        BlockKind::Deep,
        BlockKind::Ice,
    ];

    pub fn from_u8(v: u8) -> Self {
        Self::ALL.get(v as usize).copied().unwrap_or_default()
    }

    pub fn name(self) -> &'static str {
        match self {
            BlockKind::Ground => "Ground",
            BlockKind::Ramp => "Ramp",
            BlockKind::Wall => "Forest floor",
            BlockKind::Rock => "Rock",
            BlockKind::Shallow => "Shallow water",
            BlockKind::Deep => "Deep water",
            BlockKind::Ice => "Ice",
        }
    }

    /// Nothing walks or builds here.
    pub fn closed(self) -> bool {
        matches!(self, BlockKind::Wall | BlockKind::Rock)
    }

    pub fn water(self) -> bool {
        matches!(self, BlockKind::Shallow | BlockKind::Deep)
    }
}

/// Which corner of a column a diagonal cut removes: that triangle takes the level and kind
/// of the neighbours beside it, so outlines can run at 45°.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[repr(u8)]
pub enum Cut {
    #[default]
    None = 0,
    /// −x −z.
    NorthWest = 1,
    /// +x −z.
    NorthEast = 2,
    /// +x +z.
    SouthEast = 3,
    /// −x +z.
    SouthWest = 4,
}

impl Cut {
    pub fn from_u8(v: u8) -> Self {
        match v {
            1 => Cut::NorthWest,
            2 => Cut::NorthEast,
            3 => Cut::SouthEast,
            4 => Cut::SouthWest,
            _ => Cut::None,
        }
    }

    /// The cut corner's direction (−1 or +1 on x and z).
    pub fn corner(self) -> Option<(i32, i32)> {
        match self {
            Cut::None => None,
            Cut::NorthWest => Some((-1, -1)),
            Cut::NorthEast => Some((1, -1)),
            Cut::SouthEast => Some((1, 1)),
            Cut::SouthWest => Some((-1, 1)),
        }
    }

    /// Whether a point at (`u`, `v`) within the column (0 to 1 each) lies in the cut triangle.
    pub fn removes(self, u: f32, v: f32) -> bool {
        let Some((cx, cz)) = self.corner() else {
            return false;
        };
        let a = if cx < 0 { 1.0 - u } else { u };
        let b = if cz < 0 { 1.0 - v } else { v };
        a + b > 1.0
    }
}

/// The eight ways a ramp can fall, clockwise from +x (east); a ramp's direction byte is
/// one of these plus one, or 0 for a ramp that finds its own way down.
pub const RAMP_DIRECTIONS: [(i32, i32); 8] = [
    (1, 0),
    (1, 1),
    (0, 1),
    (-1, 1),
    (-1, 0),
    (-1, -1),
    (0, -1),
    (1, -1),
];

/// The ramp direction byte (1–8) nearest a vector, 0 when it is zero.
pub fn ramp_direction(dx: f32, dz: f32) -> u8 {
    if dx == 0.0 && dz == 0.0 {
        return 0;
    }
    let a = dz.atan2(dx).rem_euclid(std::f32::consts::TAU);
    ((a / std::f32::consts::FRAC_PI_4).round() as u8 % 8) + 1
}

/// One column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Block {
    pub level: u8,
    pub kind: BlockKind,
    pub cut: Cut,
    /// For a ramp: which way it falls (1–8, [`RAMP_DIRECTIONS`]), or 0 to find its own.
    pub ramp: u8,
    /// The face style of the cliffs below this column (0: the biome's cliff; otherwise one
    /// of the biome's faces, from 1).
    pub style: u8,
}

impl Block {
    pub fn ground(level: u8) -> Self {
        Self {
            level,
            ..Self::default()
        }
    }
}

/// A map's blocks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocks {
    /// Columns per side (halo included).
    pub side: u32,
    pub columns: Vec<Block>,
    /// Kinds painted per cell over the columns (`side × BLOCK` per side; 0 keeps the
    /// column's kind, otherwise kind + 1), when the map has any.
    pub paint: Option<Vec<u8>>,
}

/// One ramp region's plane: heights fall one level from `s0` to `s1` along `dir` (in
/// cells from the halo's corner).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RampPlane {
    pub dir: [f32; 2],
    pub s0: f32,
    pub s1: f32,
}

impl RampPlane {
    /// How far down the ramp a point is (0 at the top, 1 at the foot), in cells.
    pub fn fall(&self, cx: f32, cz: f32) -> f32 {
        let s = cx * self.dir[0] + cz * self.dir[1];
        ((s - self.s0) / (self.s1 - self.s0).max(0.01)).clamp(0.0, 1.0)
    }
}

impl Blocks {
    /// Flat ground at level 1 for a map of `size` cells (level 0 stays free for water and
    /// hollows below the plain).
    pub fn flat(size: u32) -> Self {
        let side = (size + 2 * MAP_HALO) / BLOCK;
        Self {
            side,
            columns: vec![Block::ground(1); (side * side) as usize],
            paint: None,
        }
    }

    /// The blocks of a map's terrain, with its painted cells, when it has blocks.
    pub fn of(terrain: &TerrainData) -> Option<Result<Self, String>> {
        let grid = terrain.blocks.as_ref()?;
        Some(Self::decode(grid, terrain.size).and_then(|mut blocks| {
            if let Some(paint) = &terrain.block_paint {
                let cells = (blocks.side * BLOCK) as usize;
                blocks.paint = Some(paint.exactly(cells * cells)?.to_vec());
            }
            Ok(blocks)
        }))
    }

    /// Cells per side of the paint layer.
    pub fn cells(&self) -> u32 {
        self.side * BLOCK
    }

    /// The kind painted on cell (`cx`, `cz`), if any.
    pub fn painted(&self, cx: i32, cz: i32) -> Option<BlockKind> {
        let paint = self.paint.as_ref()?;
        let n = self.cells() as i32;
        let (x, z) = (cx.clamp(0, n - 1), cz.clamp(0, n - 1));
        match paint[(z * n + x) as usize] {
            0 => None,
            k => Some(BlockKind::from_u8(k - 1)),
        }
    }

    /// Paints cell (`cx`, `cz`) a kind, or clears it back to its column's (`None`).
    pub fn paint_cell(&mut self, cx: i32, cz: i32, kind: Option<BlockKind>) {
        let n = self.cells() as i32;
        if cx < 0 || cz < 0 || cx >= n || cz >= n {
            return;
        }
        let paint = self.paint.get_or_insert_with(|| vec![0; (n * n) as usize]);
        paint[(cz * n + cx) as usize] = kind.map_or(0, |k| k as u8 + 1);
    }

    pub fn decode(grid: &Grid, size: u32) -> Result<Self, String> {
        let side = (size + 2 * MAP_HALO) / BLOCK;
        let bytes = grid.exactly((side * side * 4) as usize)?;
        Ok(Self {
            side,
            columns: bytes
                .chunks_exact(4)
                .map(|b| Block {
                    level: b[0].min(MAX_LEVEL),
                    kind: BlockKind::from_u8(b[1]),
                    cut: Cut::from_u8(b[2] & 0x0f),
                    ramp: (b[2] >> 4).min(8),
                    style: b[3],
                })
                .collect(),
            paint: None,
        })
    }

    pub fn encode(&self) -> Grid {
        Grid(
            self.columns
                .iter()
                .flat_map(|b| [b.level, b.kind as u8, b.cut as u8 | (b.ramp << 4), b.style])
                .collect(),
        )
    }

    pub fn get(&self, x: i32, z: i32) -> Option<Block> {
        let side = self.side as i32;
        (x >= 0 && z >= 0 && x < side && z < side).then(|| self.columns[(z * side + x) as usize])
    }

    /// The column at (`x`, `z`), clamped to the edge.
    pub fn at(&self, x: i32, z: i32) -> Block {
        let side = self.side as i32 - 1;
        self.columns[(z.clamp(0, side) * self.side as i32 + x.clamp(0, side)) as usize]
    }

    pub fn set(&mut self, x: i32, z: i32, block: Block) {
        let side = self.side as i32;
        if x >= 0 && z >= 0 && x < side && z < side {
            self.columns[(z * side + x) as usize] = block;
        }
    }

    /// The column under world (`wx`, `wz`).
    pub fn column_at(&self, wx: f32, wz: f32) -> (i32, i32) {
        let halo = MAP_HALO as f32;
        (
            ((wx + halo) / BLOCK as f32).floor() as i32,
            ((wz + halo) / BLOCK as f32).floor() as i32,
        )
    }

    /// The level and kind the cut triangle of column (`x`, `z`) takes: its two neighbours
    /// across the cut corner, the lower of them (or the lower of the three around the
    /// corner when they disagree).
    fn cut_fill(&self, x: i32, z: i32, cut: Cut) -> Block {
        let (cx, cz) = cut.corner().unwrap_or((0, 0));
        let a = self.at(x + cx, z);
        let b = self.at(x, z + cz);
        let fill = if a.level <= b.level { a } else { b };
        Block {
            cut: Cut::None,
            ..fill
        }
    }

    /// The effective column at a point (`u`, `v`) within column (`x`, `z`): itself, or
    /// what its cut triangle takes, with the kind painted on the cell there.
    pub fn sample(&self, x: i32, z: i32, u: f32, v: f32) -> Block {
        let block = self.at(x, z);
        let mut block = if block.cut.removes(u, v) {
            self.cut_fill(x, z, block.cut)
        } else {
            block
        };
        if block.kind != BlockKind::Ramp && self.paint.is_some() {
            let b = BLOCK as i32;
            let side = self.side as i32 - 1;
            let cell =
                |c: i32, f: f32| c.clamp(0, side) * b + ((f * b as f32) as i32).clamp(0, b - 1);
            if let Some(kind) = self.painted(cell(x, u), cell(z, v))
                && kind != BlockKind::Ramp
            {
                block.kind = kind;
            }
        }
        block
    }

    /// Every column's ramp plane (`None` off ramps): connected ramp columns of one level and
    /// one direction share a plane that falls one level across them. A ramp left to find
    /// its own way down falls towards its lowest side, one column at a time.
    pub fn ramp_planes(&self) -> Vec<Option<RampPlane>> {
        let side = self.side as i32;
        let b = BLOCK as f32;
        let mut planes = vec![None; self.columns.len()];
        let mut seen = vec![false; self.columns.len()];
        for start in 0..self.columns.len() {
            let first = self.columns[start];
            if first.kind != BlockKind::Ramp || seen[start] {
                continue;
            }
            // The region: this column alone when it finds its own way, otherwise every
            // ramp column joined to it at its level and direction.
            let mut region = vec![start];
            seen[start] = true;
            if first.ramp != 0 {
                let mut k = 0;
                while k < region.len() {
                    let i = region[k] as i32;
                    let (x, z) = (i % side, i / side);
                    for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
                        let (nx, nz) = (x + dx, z + dz);
                        if nx < 0 || nz < 0 || nx >= side || nz >= side {
                            continue;
                        }
                        let j = (nz * side + nx) as usize;
                        let n = self.columns[j];
                        if !seen[j]
                            && n.kind == BlockKind::Ramp
                            && n.level == first.level
                            && n.ramp == first.ramp
                        {
                            seen[j] = true;
                            region.push(j);
                        }
                    }
                    k += 1;
                }
            }
            let dir = if first.ramp != 0 {
                let (dx, dz) = RAMP_DIRECTIONS[(first.ramp - 1) as usize];
                let l = ((dx * dx + dz * dz) as f32).sqrt();
                [dx as f32 / l, dz as f32 / l]
            } else {
                let i = start as i32;
                let (dx, dz) = self.ramp_down(i % side, i / side);
                [dx as f32, dz as f32]
            };
            // The plane spans the region's ramp ground along its direction: every column's
            // corners but a cut one.
            let (mut s0, mut s1) = (f32::MAX, f32::MIN);
            for &i in &region {
                let (x, z) = ((i as i32 % side) as f32, (i as i32 / side) as f32);
                let cut = self.columns[i].cut.corner();
                for (u, v) in [(0.0, 0.0), (1.0, 0.0), (0.0, 1.0), (1.0, 1.0)] {
                    if cut.is_some_and(|(cx, cz)| (cx > 0) == (u > 0.5) && (cz > 0) == (v > 0.5)) {
                        continue;
                    }
                    let s = (x + u) * b * dir[0] + (z + v) * b * dir[1];
                    s0 = s0.min(s);
                    s1 = s1.max(s);
                }
            }
            let plane = RampPlane { dir, s0, s1 };
            for &i in &region {
                planes[i] = Some(plane);
            }
        }
        planes
    }

    /// A ramp column's height at (`u`, `v`) within it, on its plane.
    pub fn ramp_height(&self, planes: &[Option<RampPlane>], x: i32, z: i32, u: f32, v: f32) -> f32 {
        let block = self.at(x, z);
        let top = BASE_HEIGHT + block.level as f32 * LEVEL_HEIGHT;
        let side = self.side as i32 - 1;
        let (cx, cz) = (x.clamp(0, side), z.clamp(0, side));
        let Some(plane) = planes[(cz * self.side as i32 + cx) as usize] else {
            return top;
        };
        let b = BLOCK as f32;
        top - plane.fall((cx as f32 + u) * b, (cz as f32 + v) * b) * LEVEL_HEIGHT
    }

    /// For a ramp column: the direction (dx, dz) towards its lower side, the lowest
    /// neighbour among the four, preferring the one opposite a neighbour at its own level.
    pub fn ramp_down(&self, x: i32, z: i32) -> (i32, i32) {
        let level = self.at(x, z).level;
        let mut best = (0, 1);
        let mut score = i32::MIN;
        for (dx, dz) in [(1, 0), (-1, 0), (0, 1), (0, -1)] {
            let down = self.at(x + dx, z + dz);
            let up = self.at(x - dx, z - dz);
            let drop = level as i32 - down.level as i32;
            let mut s = if drop == 1 { 10 } else { -10 * drop.abs() };
            if up.level >= level && up.kind != BlockKind::Ramp {
                s += 5;
            }
            if down.kind.closed() {
                s -= 20;
            }
            if s > score {
                score = s;
                best = (dx, dz);
            }
        }
        best
    }

    /// The surface height a column makes at (`u`, `v`) within it (ground, bed under water).
    /// (`su`, `sv`) is the same point nudged inside the column, which decides the cut.
    #[allow(clippy::too_many_arguments)]
    fn surface(
        &self,
        planes: &[Option<RampPlane>],
        x: i32,
        z: i32,
        su: f32,
        sv: f32,
        u: f32,
        v: f32,
    ) -> f32 {
        let block = self.sample(x, z, su, sv);
        let top = BASE_HEIGHT + block.level as f32 * LEVEL_HEIGHT;
        match block.kind {
            BlockKind::Ramp => self.ramp_height(planes, x, z, u, v),
            BlockKind::Shallow => top - 0.7,
            BlockKind::Deep => top - 4.0,
            BlockKind::Ice => top - 0.15,
            _ => top,
        }
    }

    /// The water surface over a water column, if it is one.
    fn water(&self, x: i32, z: i32, u: f32, v: f32) -> Option<f32> {
        let block = self.sample(x, z, u, v);
        let top = BASE_HEIGHT + block.level as f32 * LEVEL_HEIGHT;
        match block.kind {
            BlockKind::Shallow => Some(top - 0.2),
            BlockKind::Deep => Some(top - 0.4),
            _ => None,
        }
    }

    /// Writes the heights, water surfaces and closed cells the blocks make into `terrain`.
    /// A vertex on a column boundary takes the highest surface around it, so a raised
    /// column keeps its whole top and its cliff falls within the lower neighbour's first
    /// cell; inside a column the vertex samples its own column.
    pub fn apply(&self, terrain: &mut TerrainData) {
        let vertices = terrain.vertices();
        let span = terrain.span();
        let mut heights = Vec::with_capacity((vertices * vertices * 4) as usize);
        let mut water = Vec::with_capacity((vertices * vertices * 4) as usize);
        let planes = self.ramp_planes();
        let b = BLOCK as i32;
        for vz in 0..vertices as i32 {
            for vx in 0..vertices as i32 {
                // The columns this vertex touches, and where it lies in each.
                let mut h = f32::MIN;
                let mut w: Option<f32> = None;
                let xs: &[i32] = if vx % b == 0 {
                    &[vx / b - 1, vx / b]
                } else {
                    &[vx / b]
                };
                let zs: &[i32] = if vz % b == 0 {
                    &[vz / b - 1, vz / b]
                } else {
                    &[vz / b]
                };
                for &cz in zs {
                    for &cx in xs {
                        let u = (vx - cx * b) as f32 / b as f32;
                        let v = (vz - cz * b) as f32 / b as f32;
                        let (u, v) = (u.clamp(0.0, 1.0), v.clamp(0.0, 1.0));
                        // Sample just inside the column so a cut's diagonal edge is exact.
                        let (su, sv) = (u.clamp(0.02, 0.98), v.clamp(0.02, 0.98));
                        h = h.max(self.surface(&planes, cx, cz, su, sv, u, v));
                        if let Some(level) = self.water(cx, cz, su, sv) {
                            w = Some(w.map_or(level, |o: f32| o.max(level)));
                        }
                    }
                }
                // A vertex shared with dry ground keeps the dry bank's height under water
                // only where every column around it is water.
                let all_water = zs.iter().all(|&cz| {
                    xs.iter().all(|&cx| {
                        let u = ((vx - cx * b) as f32 / b as f32).clamp(0.02, 0.98);
                        let v = ((vz - cz * b) as f32 / b as f32).clamp(0.02, 0.98);
                        self.sample(cx, cz, u, v).kind.water()
                    })
                });
                if all_water {
                    h = zs
                        .iter()
                        .flat_map(|&cz| {
                            xs.iter().map(move |&cx| {
                                let u = ((vx - cx * b) as f32 / b as f32).clamp(0.0, 1.0);
                                let v = ((vz - cz * b) as f32 / b as f32).clamp(0.0, 1.0);
                                (cx, cz, u, v)
                            })
                        })
                        .map(|(cx, cz, u, v)| {
                            self.surface(
                                &planes,
                                cx,
                                cz,
                                u.clamp(0.02, 0.98),
                                v.clamp(0.02, 0.98),
                                u,
                                v,
                            )
                        })
                        .fold(f32::MAX, f32::min);
                }
                heights.extend_from_slice(&h.to_le_bytes());
                let surface = w.unwrap_or(DRY_WATER_HEIGHT as f32);
                water.extend_from_slice(&surface.to_le_bytes());
            }
        }
        let mut blocked = vec![0u8; (span * span) as usize];
        for cz in 0..span as i32 {
            for cx in 0..span as i32 {
                let (x, z) = (cx / b, cz / b);
                let u = ((cx % b) as f32 + 0.5) / b as f32;
                let v = ((cz % b) as f32 + 0.5) / b as f32;
                if self.sample(x, z, u, v).kind.closed() {
                    blocked[(cz as u32 * span + cx as u32) as usize] = 1;
                }
            }
        }
        terrain.heights = Grid(heights);
        terrain.water_heights = Grid(water);
        terrain.blocked = Some(Grid(blocked));
        terrain.blocks = Some(self.encode());
        terrain.block_paint = self.paint.as_ref().map(|p| Grid(p.clone()));
    }

    /// Blocks that approximate an existing map: each column's level from its mean height,
    /// water where most of it is under water (deep or shallow by depth), closed ground
    /// where most of its cells are closed. Where the map's ground was walkable between two
    /// columns a level apart, the higher becomes a ramp, so gentle slopes stay passable;
    /// every point in `pads` (the starts) gets flat ground at its level within 26 cells (the
    /// fort and its worker formation).
    pub fn from_terrain(terrain: &TerrainData, pads: &[[f32; 2]]) -> Self {
        let size = terrain.size;
        let mut blocks = Self::flat(size);
        let vertices = terrain.vertices() as usize;
        let span = terrain.span() as usize;
        let read = |grid: &Grid, i: usize| {
            let b = &grid.0[i * 4..i * 4 + 4];
            f32::from_le_bytes([b[0], b[1], b[2], b[3]])
        };
        let blocked = terrain.blocked.as_ref().map(|g| g.0.clone());
        let b = BLOCK as usize;
        for cz in 0..blocks.side as usize {
            for cx in 0..blocks.side as usize {
                let (mut sum, mut wet, mut depth, mut n) = (0.0f32, 0, 0.0f32, 0);
                for z in cz * b..=cz * b + b {
                    for x in cx * b..=cx * b + b {
                        let i = z.min(vertices - 1) * vertices + x.min(vertices - 1);
                        let h = read(&terrain.heights, i);
                        let w = read(&terrain.water_heights, i);
                        sum += h;
                        n += 1;
                        if w > h + 0.05 && w > DRY_WATER_HEIGHT as f32 + 0.5 {
                            wet += 1;
                            depth += w - h;
                        }
                    }
                }
                let mean = sum / n as f32;
                let mut closed = 0;
                if let Some(blocked) = &blocked {
                    for z in cz * b..cz * b + b {
                        for x in cx * b..cx * b + b {
                            closed +=
                                usize::from(blocked[z.min(span - 1) * span + x.min(span - 1)] != 0);
                        }
                    }
                }
                let mut block = Block::ground(
                    ((mean - BASE_HEIGHT) / LEVEL_HEIGHT)
                        .round()
                        .clamp(0.0, MAX_LEVEL as f32) as u8,
                );
                if wet * 2 > n {
                    let surface = mean + depth / wet as f32;
                    block.level = ((surface - BASE_HEIGHT) / LEVEL_HEIGHT)
                        .ceil()
                        .clamp(0.0, MAX_LEVEL as f32) as u8;
                    block.kind = if depth / (wet as f32) > 1.2 {
                        BlockKind::Deep
                    } else {
                        BlockKind::Shallow
                    };
                } else if closed * 2 > b * b {
                    block.kind = BlockKind::Wall;
                }
                blocks.columns[cz * blocks.side as usize + cx] = block;
            }
        }
        // Starts stand on flat ground.
        let halo = MAP_HALO as f32;
        for pad in pads {
            let (px, pz) = blocks.column_at(pad[0], pad[1]);
            let level = blocks.at(px, pz).level;
            for z in pz - 8..=pz + 8 {
                for x in px - 8..=px + 8 {
                    let (cx, cz) = (
                        (x as f32 + 0.5) * BLOCK as f32 - halo,
                        (z as f32 + 0.5) * BLOCK as f32 - halo,
                    );
                    if (cx - pad[0]).hypot(cz - pad[1]) <= 26.0 + BLOCK as f32 {
                        blocks.set(x, z, Block::ground(level));
                    }
                }
            }
        }
        // Gentle slopes between levels become ramps.
        let walkable_between = |a: (usize, usize), b: (usize, usize)| {
            let centre = |c: (usize, usize)| (c.0 * BLOCK as usize + 2, c.1 * BLOCK as usize + 2);
            let (pa, pb) = (centre(a), centre(b));
            let steps = pa.0.abs_diff(pb.0).max(pa.1.abs_diff(pb.1));
            let mut previous = None;
            (0..=steps).all(|s| {
                let t = s as f32 / steps.max(1) as f32;
                let x = (pa.0 as f32 + (pb.0 as f32 - pa.0 as f32) * t).round() as usize;
                let z = (pa.1 as f32 + (pb.1 as f32 - pa.1 as f32) * t).round() as usize;
                let h = read(
                    &terrain.heights,
                    z.min(vertices - 1) * vertices + x.min(vertices - 1),
                );
                let ok = previous.is_none_or(|p: f32| (h - p).abs() <= 0.9);
                previous = Some(h);
                ok
            })
        };
        let side = blocks.side as usize;
        let mut ramps = Vec::new();
        for cz in 0..side {
            for cx in 0..side {
                let here = blocks.columns[cz * side + cx];
                if here.kind != BlockKind::Ground {
                    continue;
                }
                for (dx, dz) in [(1i32, 0i32), (-1, 0), (0, 1), (0, -1)] {
                    let (nx, nz) = (cx as i32 + dx, cz as i32 + dz);
                    let Some(next) = blocks.get(nx, nz) else {
                        continue;
                    };
                    if next.kind == BlockKind::Ground
                        && here.level == next.level + 1
                        && walkable_between((cx, cz), (nx as usize, nz as usize))
                    {
                        ramps.push((cx, cz));
                        break;
                    }
                }
            }
        }
        for (cx, cz) in ramps {
            blocks.columns[cz * side + cx].kind = BlockKind::Ramp;
        }
        blocks
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn terrain(size: u32) -> TerrainData {
        let span = size + 2 * MAP_HALO;
        let vertices = (span + 1) as usize;
        let floats = |v: f32| Grid(v.to_le_bytes().repeat(vertices * vertices));
        TerrainData {
            version: crate::map::Version,
            size,
            texture_grid: None,
            heights: floats(0.0),
            water_heights: floats(DRY_WATER_HEIGHT as f32),
            water_flow: Grid([128, 128, 0, 0].repeat((span * span) as usize)),
            water_profiles: Vec::new(),
            grass: floats(0.0),
            rock: floats(0.0),
            paint: Vec::new(),
            shade: None,
            blocked: None,
            overlay: None,
            blocks: None,
            block_paint: None,
        }
    }

    fn height(t: &TerrainData, x: u32, z: u32) -> f32 {
        let i = ((z + MAP_HALO) * t.vertices() + x + MAP_HALO) as usize * 4;
        f32::from_le_bytes(t.heights.0[i..i + 4].try_into().unwrap())
    }

    #[test]
    fn a_raised_column_keeps_its_top_and_falls_as_a_cliff() {
        let mut t = terrain(256);
        let mut blocks = Blocks::flat(256);
        let (x, z) = blocks.column_at(100.0, 100.0);
        blocks.set(x, z, Block::ground(2));
        blocks.apply(&mut t);
        // The column spans cells 100–104 (its vertices included) at level 2.
        let top = BASE_HEIGHT + 2.0 * LEVEL_HEIGHT;
        let below = BASE_HEIGHT + LEVEL_HEIGHT;
        assert_eq!(height(&t, 100, 100), top);
        assert_eq!(height(&t, 104, 102), top);
        assert_eq!(height(&t, 105, 102), below);
        assert_eq!(height(&t, 99, 102), below);
    }

    #[test]
    fn a_ramp_climbs_one_level_across_its_column_gently_enough_to_walk() {
        let mut t = terrain(256);
        let mut blocks = Blocks::flat(256);
        let (x, z) = blocks.column_at(100.0, 100.0);
        // A plateau at level 2 to the west, a ramp column, the plain (level 1) to the east.
        for dz in -2..=2 {
            for dx in -3..=-1 {
                blocks.set(x + dx, z + dz, Block::ground(2));
            }
        }
        blocks.set(
            x,
            z,
            Block {
                level: 2,
                kind: BlockKind::Ramp,
                cut: Cut::None,
                ..Block::default()
            },
        );
        assert_eq!(blocks.ramp_down(x, z), (1, 0));
        blocks.apply(&mut t);
        let mut previous = height(&t, 100, 102);
        for cx in 101..=104 {
            let h = height(&t, cx, 102);
            assert!(
                previous - h <= 0.9,
                "a step of {} m is a cliff",
                previous - h
            );
            previous = h;
        }
        assert_eq!(height(&t, 104, 102), BASE_HEIGHT + LEVEL_HEIGHT);
    }

    #[test]
    fn closed_kinds_close_their_cells_and_a_cut_opens_its_triangle() {
        let mut t = terrain(256);
        let mut blocks = Blocks::flat(256);
        let (x, z) = blocks.column_at(100.0, 100.0);
        blocks.set(
            x,
            z,
            Block {
                level: 1,
                kind: BlockKind::Wall,
                cut: Cut::SouthEast,
                ..Block::default()
            },
        );
        blocks.apply(&mut t);
        let span = t.span();
        let closed = |cx: u32, cz: u32| {
            t.blocked.as_ref().unwrap().0[((cz + MAP_HALO) * span + cx + MAP_HALO) as usize] != 0
        };
        assert!(closed(100, 100), "the column's north-west corner is closed");
        assert!(
            !closed(103, 103),
            "its cut south-east corner is open ground"
        );
    }

    #[test]
    fn a_diagonal_ramp_region_falls_as_one_walkable_plane() {
        let mut t = terrain(256);
        let mut blocks = Blocks::flat(256);
        // A plateau (level 2) north-west of a band of ramps running south-west to
        // north-east, falling south-east to the plain.
        for z in 10..50 {
            for x in 10..50 {
                let d = x + z;
                if d < 58 {
                    blocks.set(x, z, Block::ground(2));
                } else if d < 62 {
                    blocks.set(
                        x,
                        z,
                        Block {
                            level: 2,
                            kind: BlockKind::Ramp,
                            ramp: ramp_direction(1.0, 1.0),
                            ..Block::default()
                        },
                    );
                }
            }
        }
        blocks.apply(&mut t);
        // Along the diagonal through the band's middle, from plateau to plain.
        let at = |k: u32| height(&t, k - MAP_HALO, k - MAP_HALO);
        let (top, foot) = (at(100), at(140));
        assert_eq!(top, BASE_HEIGHT + 2.0 * LEVEL_HEIGHT);
        assert_eq!(foot, BASE_HEIGHT + LEVEL_HEIGHT);
        for k in 100..140 {
            let step = at(k) - at(k + 1);
            assert!((-0.01..=0.9).contains(&step), "a step of {step} m at {k}");
        }
        // Across the band the plane is level: the same fall at two points on one line.
        assert_eq!(height(&t, 110, 130), height(&t, 130, 110));
    }

    #[test]
    fn painted_cells_take_their_kind_and_keep_the_level() {
        let mut t = terrain(256);
        let mut blocks = Blocks::flat(256);
        let (x, z) = blocks.column_at(100.0, 100.0);
        blocks.set(x, z, Block::ground(2));
        let (cx, cz) = ((100 + MAP_HALO) as i32, (100 + MAP_HALO) as i32);
        blocks.paint_cell(cx, cz, Some(BlockKind::Wall));
        assert_eq!(blocks.sample(x, z, 0.1, 0.1).kind, BlockKind::Wall);
        assert_eq!(blocks.sample(x, z, 0.1, 0.1).level, 2);
        assert_eq!(blocks.sample(x, z, 0.6, 0.1).kind, BlockKind::Ground);
        blocks.apply(&mut t);
        let span = t.span();
        let closed = |cx: u32, cz: u32| {
            t.blocked.as_ref().unwrap().0[((cz + MAP_HALO) * span + cx + MAP_HALO) as usize] != 0
        };
        assert!(closed(100, 100));
        assert!(!closed(101, 100));
        let back = Blocks::of(&t).unwrap().unwrap();
        assert_eq!(back.painted(cx, cz), Some(BlockKind::Wall));
    }

    #[test]
    fn blocks_survive_their_grid() {
        let mut blocks = Blocks::flat(256);
        blocks.set(
            3,
            4,
            Block {
                level: 5,
                kind: BlockKind::Shallow,
                cut: Cut::NorthEast,
                ramp: 3,
                style: 2,
            },
        );
        assert_eq!(Blocks::decode(&blocks.encode(), 256).unwrap(), blocks);
    }
}
