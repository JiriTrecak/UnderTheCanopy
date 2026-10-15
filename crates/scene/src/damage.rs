//! Fires on damaged buildings (D46, new — not in the old game): below
//! [`FIRE_THRESHOLD`] of its health a built building in sight starts burning at one spot on
//! its roof; more spots ignite and every flame grows, with denser and darker smoke, as its
//! health falls. Repairs put the fires out in reverse order; a destroyed building has none.
//!
//! The spots are chosen once per building from its entity id: points spread over the
//! model's plan, dropped onto its own mesh from above, kept only on the upper part of the
//! building and apart from each other. They never move. This is presentation only: it
//! reads the view's health and never touches the simulation.

use crate::{effects::Fire, obstruction::Hull};
use glam::{Mat4, Quat, Vec3};
use std::{collections::BTreeMap, sync::Arc};

/// Buildings at or above this share of their health do not burn.
pub const FIRE_THRESHOLD: f32 = 0.75;
/// At or below this share the fires are at their worst.
pub const FIRE_WORST: f32 = 0.15;
/// Fire spots of the largest buildings (the smallest have two).
pub const MAX_SPOTS: usize = 6;
/// A spot only on the upper part of the model (share of its height above the base).
const UPPER: f32 = 0.4;
/// Candidate points spread over the plan.
const CANDIDATES: u32 = 32;
/// Severity a burning building gains or loses at most per tick (a heavy blow flares up in
/// half a second; a repair dies down over a little more than a second).
const RISE_PER_TICK: f32 = 0.05;
const FALL_PER_TICK: f32 = 0.02;

/// How badly a building at `health` (share of its maximum) burns: 0 at
/// [`FIRE_THRESHOLD`] and above, 1 at [`FIRE_WORST`] and below.
pub fn severity(health: f32) -> f32 {
    ((FIRE_THRESHOLD - health) / (FIRE_THRESHOLD - FIRE_WORST)).clamp(0.0, 1.0)
}

/// Spots a building of `extent` metres across (its larger side) burns at, at worst.
pub fn spots(extent: f32) -> usize {
    ((extent / 2.4).round() as usize).clamp(2, MAX_SPOTS)
}

/// The flame size of a building `extent` metres across, at full strength.
fn flame_size(extent: f32) -> f32 {
    (0.12 * extent).clamp(0.5, 2.2)
}

/// The severity at which spot `k` of `n` ignites (the first at once).
fn ignites(k: usize, n: usize) -> f32 {
    0.9 * k as f32 / n as f32
}

/// Visual-only randomness from a seed (never the simulation's stream).
fn random(seed: u32, salt: u32) -> f32 {
    let mut n = seed ^ salt.wrapping_add(1).wrapping_mul(0x85eb_ca6b);
    n = (n ^ (n >> 16)).wrapping_mul(0x7feb_352d);
    n = (n ^ (n >> 15)).wrapping_mul(0x846c_a68b);
    ((n ^ (n >> 16)) >> 8) as f32 / 16_777_216.0
}

/// Up to `count` fire spots in model space on a model with bind-pose bounds `min`–`max`
/// (and its `hull`, when it has one), in the order they ignite. Deterministic in `seed`.
pub fn anchors(hull: Option<&Hull>, min: Vec3, max: Vec3, seed: u32, count: usize) -> Vec<Vec3> {
    let centre = (min + max) * 0.5;
    let half = (max - min) * 0.5;
    let height = (max.y - min.y).max(1e-3);
    let turn = random(seed, 0) * std::f32::consts::TAU;
    // A Vogel disc over the inner plan, each candidate dropped onto the mesh.
    let mut candidates: Vec<(f32, Vec3)> = (0..CANDIDATES)
        .filter_map(|i| {
            let r = ((i as f32 + 0.5) / CANDIDATES as f32).sqrt() * 0.75;
            let a = turn + i as f32 * 2.399_963;
            let (x, z) = (
                centre.x + a.cos() * r * half.x,
                centre.z + a.sin() * r * half.z,
            );
            let top = max.y + 1.0;
            let y = match hull {
                Some(hull) => {
                    let t = hull.hit(
                        &Mat4::IDENTITY,
                        Vec3::new(x, top, z),
                        Vec3::NEG_Y,
                        height + 2.0,
                    )?;
                    top - t
                }
                // Without a mesh: under the top of the bounds.
                None => max.y - height * 0.15,
            };
            ((y - min.y) / height >= UPPER).then(|| (random(seed, i + 1), Vec3::new(x, y, z)))
        })
        .collect();
    candidates.sort_by(|a, b| a.0.total_cmp(&b.0));
    // Apart from each other: relaxed until enough are found.
    let mut chosen: Vec<Vec3> = Vec::with_capacity(count);
    let mut gap = half.x.min(half.z) * 0.8;
    while chosen.len() < count && gap > 1e-3 {
        for &(_, p) in &candidates {
            if chosen.len() == count {
                break;
            }
            let apart = chosen.iter().all(|c| {
                let d = *c - p;
                d.x * d.x + d.z * d.z >= gap * gap
            });
            if apart && !chosen.contains(&p) {
                chosen.push(p);
            }
        }
        gap *= 0.6;
    }
    chosen
}

/// A built building in sight with its health and pose this tick.
#[derive(Debug, Clone, Copy)]
pub struct Damaged {
    pub entity: u32,
    /// Its model in the live model catalog.
    pub model: u32,
    /// Share of its maximum health.
    pub health: f32,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: f32,
}

#[derive(Debug, Clone)]
struct Burning {
    model: u32,
    /// Model space, in ignition order.
    anchors: Vec<Vec3>,
    extent: f32,
    level: f32,
}

/// The fires of the buildings in sight, from tick to tick.
#[derive(Default)]
pub struct Fires {
    burning: BTreeMap<u32, Burning>,
}

impl Fires {
    /// The fire spots of this tick. `shape(model)` gives a model's bind-pose bounds and
    /// hull (asked once per burning building).
    pub fn update(
        &mut self,
        buildings: &[Damaged],
        mut shape: impl FnMut(u32) -> (Vec3, Vec3, Option<Arc<Hull>>),
    ) -> Vec<Fire> {
        let mut fires = Vec::new();
        let mut seen: Vec<u32> = Vec::with_capacity(buildings.len());
        for b in buildings {
            let target = severity(b.health);
            if target <= 0.0 && !self.burning.contains_key(&b.entity) {
                continue;
            }
            seen.push(b.entity);
            let burning = match self.burning.get_mut(&b.entity) {
                Some(burning) if burning.model == b.model => {
                    let step = (target - burning.level).clamp(-FALL_PER_TICK, RISE_PER_TICK);
                    burning.level += step;
                    burning
                }
                _ => {
                    // First seen (or rebuilt in another look): as it stands, no flare-up.
                    let (min, max, hull) = shape(b.model);
                    let extent = (max.x - min.x).max(max.z - min.z) * b.scale;
                    let seed = b.entity.wrapping_mul(0x9e37_79b1);
                    let anchors = anchors(hull.as_deref(), min, max, seed, spots(extent));
                    self.burning.insert(
                        b.entity,
                        Burning {
                            model: b.model,
                            anchors,
                            extent,
                            level: target,
                        },
                    );
                    self.burning.get_mut(&b.entity).expect("inserted")
                }
            };
            let level = burning.level;
            if level <= 0.0 {
                continue;
            }
            let n = burning.anchors.len();
            let size = flame_size(burning.extent);
            for (k, anchor) in burning.anchors.iter().enumerate() {
                let from = ignites(k, n);
                if level <= from {
                    break;
                }
                let grown = ((level - from) / 0.3).min(1.0);
                let seed = b.entity.wrapping_mul(0x9e37_79b1) ^ (k as u32).wrapping_mul(7919);
                // Sunk a little so the flames grow out of the roof.
                let local = (*anchor - Vec3::Y * 0.08) * b.scale;
                fires.push(Fire {
                    entity: b.entity,
                    position: b.position + b.rotation * local,
                    size: size
                        * (0.45 + 0.55 * grown)
                        * (0.75 + 0.5 * level)
                        * (0.85 + 0.3 * random(seed, 99)),
                    severity: level,
                    seed,
                });
            }
        }
        // Out of sight, repaired and put out, or destroyed: forgotten.
        self.burning
            .retain(|id, b| seen.contains(id) && b.level > 0.0);
        fires
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 4 × 4 m block 2.5 m high with a 1 × 1 m tower 5 m high on one corner.
    fn keep() -> Hull {
        let mut positions = Vec::new();
        let mut indices = Vec::new();
        let mut cuboid = |min: Vec3, max: Vec3| {
            let base = positions.len() as u32;
            for i in 0..8u32 {
                positions.push(Vec3::new(
                    if i & 1 == 0 { min.x } else { max.x },
                    if i & 2 == 0 { min.y } else { max.y },
                    if i & 4 == 0 { min.z } else { max.z },
                ));
            }
            // Top and the four sides (the solver sees both faces).
            for q in [
                [2, 3, 7, 6],
                [0, 1, 3, 2],
                [4, 5, 7, 6],
                [0, 2, 6, 4],
                [1, 3, 7, 5],
            ] {
                indices.extend([q[0], q[1], q[2], q[0], q[2], q[3]].map(|k: u32| base + k));
            }
        };
        cuboid(Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 2.5, 2.0));
        cuboid(Vec3::new(1.0, 0.0, 1.0), Vec3::new(2.0, 5.0, 2.0));
        Hull::from_triangles(positions, indices)
    }

    #[test]
    fn spots_lie_on_the_upper_roof_apart_and_stay_put() {
        let hull = keep();
        let (min, max) = (Vec3::new(-2.0, 0.0, -2.0), Vec3::new(2.0, 5.0, 2.0));
        let spots = anchors(Some(&hull), min, max, 77, 4);
        assert_eq!(spots.len(), 4);
        for p in &spots {
            // On a top face: the block's roof (2.5 m) or the tower's (5 m), never in the air.
            let on_tower = p.x >= 1.0 && p.z >= 1.0;
            assert!((p.y - if on_tower { 5.0 } else { 2.5 }).abs() < 1e-4, "{p}");
            assert!(p.y >= UPPER * 5.0 - 1e-4, "upper part: {p}");
        }
        for (i, a) in spots.iter().enumerate() {
            for b in &spots[i + 1..] {
                assert!((a.x - b.x).hypot(a.z - b.z) > 0.3, "{a} {b}");
            }
        }
        assert_eq!(spots, anchors(Some(&hull), min, max, 77, 4));
        assert_ne!(spots, anchors(Some(&hull), min, max, 78, 4));
    }

    #[test]
    fn fires_grow_with_damage_and_go_out_on_repair() {
        let hull = Arc::new(keep());
        let shape = |_| {
            (
                Vec3::new(-2.0, 0.0, -2.0),
                Vec3::new(2.0, 5.0, 2.0),
                Some(hull.clone()),
            )
        };
        let at = |health| Damaged {
            entity: 9,
            model: 1,
            health,
            position: Vec3::new(10.0, 0.0, 10.0),
            rotation: Quat::IDENTITY,
            scale: 1.0,
        };
        let mut fires = Fires::default();
        assert!(fires.update(&[at(0.9)], shape).is_empty(), "healthy");
        let light = fires.update(&[at(0.7)], shape);
        let heavy = Fires::default().update(&[at(0.2)], shape);
        assert_eq!(light.len(), 1);
        assert!(heavy.len() > light.len());
        let largest = |f: &[Fire]| f.iter().map(|f| f.size).fold(0.0, f32::max);
        assert!(largest(&heavy) > largest(&light));
        // The first spot burns in the same place at every level.
        assert_eq!(light[0].position, heavy[0].position);
        // Repaired above the threshold: it dies down over a few ticks, then is gone.
        let mut ticks = 0;
        while !fires.update(&[at(0.95)], shape).is_empty() {
            ticks += 1;
            assert!(ticks < 100);
        }
        assert!(fires.burning.is_empty());
        // Out of sight: nothing burns.
        let mut fires = Fires::default();
        fires.update(&[at(0.2)], shape);
        assert!(fires.update(&[], shape).is_empty());
    }
}
