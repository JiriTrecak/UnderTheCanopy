//! A map's blockout (D80): the map authored as columns of blocks, 4 × 4 cells each,
//! stacked in whole cliff levels, from which its heights, water and closed cells are made.
//! The simulation never reads the blocks, only what they make, so a map plays the same
//! however the look generated from them changes.
//!
//! `terrain/blocks.grid` holds one 4-byte sample per column (halo included): level, kind,
//! diagonal cut, and a byte kept zero. Columns run from world −[`MAP_HALO`], `BLOCK` cells
//! apart; a 480-cell map is 128 × 128 columns.

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

/// One column.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Block {
    pub level: u8,
    pub kind: BlockKind,
    pub cut: Cut,
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
}

impl Blocks {
    /// Flat ground at level 1 for a map of `size` cells (level 0 stays free for water and
    /// hollows below the plain).
    pub fn flat(size: u32) -> Self {
        let side = (size + 2 * MAP_HALO) / BLOCK;
        Self {
            side,
            columns: vec![Block::ground(1); (side * side) as usize],
        }
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
                    cut: Cut::from_u8(b[2]),
                })
                .collect(),
        })
    }

    pub fn encode(&self) -> Grid {
        Grid(
            self.columns
                .iter()
                .flat_map(|b| [b.level, b.kind as u8, b.cut as u8, 0])
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
    /// what its cut triangle takes.
    pub fn sample(&self, x: i32, z: i32, u: f32, v: f32) -> Block {
        let block = self.at(x, z);
        if block.cut.removes(u, v) {
            self.cut_fill(x, z, block.cut)
        } else {
            block
        }
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
    fn surface(&self, x: i32, z: i32, su: f32, sv: f32, u: f32, v: f32) -> f32 {
        let block = self.sample(x, z, su, sv);
        let top = BASE_HEIGHT + block.level as f32 * LEVEL_HEIGHT;
        match block.kind {
            BlockKind::Ramp => {
                let (dx, dz) = self.ramp_down(x, z);
                // 0 at the high edge, 1 at the low edge.
                let t = if dx != 0 {
                    if dx > 0 { u } else { 1.0 - u }
                } else if dz > 0 {
                    v
                } else {
                    1.0 - v
                };
                top - t * LEVEL_HEIGHT
            }
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
                        h = h.max(self.surface(cx, cz, su, sv, u, v));
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
                            self.surface(cx, cz, u.clamp(0.02, 0.98), v.clamp(0.02, 0.98), u, v)
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
    fn blocks_survive_their_grid() {
        let mut blocks = Blocks::flat(256);
        blocks.set(
            3,
            4,
            Block {
                level: 5,
                kind: BlockKind::Shallow,
                cut: Cut::NorthEast,
            },
        );
        assert_eq!(Blocks::decode(&blocks.encode(), 256).unwrap(), blocks);
    }
}
