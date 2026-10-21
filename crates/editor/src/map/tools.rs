//! What maps are furnished with: starts, amber sites and creep camps, placed as the block
//! editor's Place and MCP's `map_place` do (and the Warcraft importer); a start's level
//! pad as patches over the heights; the ground's height; the scenery that counts as trees
//! and as props. (The brush editor of M3 was removed with D81: maps are made in blocks.)

use crate::document::Command;
use canopy_content::{
    Content,
    game::Kind,
    map::{MAP_HALO, UtcMap, grid::Grid},
};
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// A step between neighbouring vertices above this is a cliff (the simulation's 90 cm).
pub const CLIFF_STEP: f32 = 0.9;

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

/// What can be placed with a click.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Placement {
    /// The next player's start, its amber 26 cells towards the middle.
    Start,
    Amber,
    Camp,
}

/// The placement settings and the selection of a map's session.
#[derive(Debug, Clone)]
pub struct Tools {
    /// The strength of new camps.
    pub tier: Tier,
    /// Amber per node of a new site.
    pub amber: u32,
    /// The selected start, entity or object: (kind, id).
    pub selected: Option<(Picked, String)>,
}

impl Default for Tools {
    fn default() -> Self {
        Self {
            tier: Tier::Easy,
            amber: 2500,
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

/// A change to the terrain's grids in progress (a start's pad): the grids as they were,
/// and the vertices it touched.
#[derive(Debug, Clone, Default)]
pub struct Stroke {
    /// Grid file → its bytes before the change.
    pub before: BTreeMap<String, Vec<u8>>,
    /// Touched vertices: min x, min z, max x, max z (inclusive).
    pub area: Option<[u32; 4]>,
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

impl Stroke {
    pub fn new() -> Self {
        Self::default()
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

/// The scenery the tree brush can plant: the package's trees, harvestable or (D62: trees
/// are no resource in the base game) foliage tree models whose trunks close the ground.
pub fn tree_kinds(content: &Content) -> Vec<String> {
    use canopy_content::assets::{SceneryCategory, landscape::AssetKind};
    canopy_content::assets::scenery(&content.manifest)
        .filter(|s| {
            content.landscape.iter().any(|a| {
                a.scenery
                    .as_ref()
                    .is_some_and(|i| i.as_str() == s.id.as_str())
                    && (a.harvesting.is_some()
                        || (a.kind == AssetKind::Tree
                            && s.category == SceneryCategory::Foliage
                            && (s.blocker.is_some() || s.blockers.is_some())))
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
    // On a ring wide enough that neighbours' bodies stay apart (positions round to cells).
    let widest = members
        .iter()
        .filter_map(|m| content.definition(m).and_then(|d| d.dimensions.as_ref()))
        .map(|d| d.radius)
        .fold(0.5, f64::max);
    let ring = (widest + 0.8) / (std::f64::consts::PI / count).sin();
    let entities: Vec<Value> = members
        .iter()
        .enumerate()
        .map(|(k, definition)| {
            let angle = k as f64 / count * std::f64::consts::TAU;
            let r = if members.len() == 1 { 0.0 } else { ring };
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

/// The quarter turn (degrees) that points the door of building `fort` standing at world
/// (`x`, `z`) most nearly at the middle of a map of `size`.
pub fn facing_middle(content: &Content, fort: &str, x: f64, z: f64, size: u32) -> u32 {
    let Some((ox, oy)) = content
        .definition(fort)
        .and_then(|d| d.entrance.as_ref())
        .map(|e| (e.x, e.y))
    else {
        return 0;
    };
    let middle = f64::from(size) / 2.0;
    let (dx, dz) = (middle - x, middle - z);
    // The simulation's quarter turns (`economy::sites::entrance`).
    [(0, ox, oy), (90, oy, -ox), (180, -ox, -oy), (270, -oy, ox)]
        .into_iter()
        .max_by(|a, b| (a.1 * dx + a.2 * dz).total_cmp(&(b.1 * dx + b.2 * dz)))
        .map_or(0, |(r, _, _)| r)
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
    fn starts_snap_to_the_hall_lattice() {
        assert_eq!(start_position(117.0, 226.4), (117.5, 225.5));
        assert_eq!(start_position(500.0, 500.0), (501.5, 501.5));
    }
}
