//! Soft bodies, as in StarCraft II (docs/decisions.md D100): after every unit has moved,
//! bodies that overlap are pushed apart by who may move whom. A unit on the move pushes
//! an idle ally out of its way and that ally pushes the next, so a crowd of one's own
//! units opens up; allies on the move squeeze past each other; planted units (holding,
//! fighting, casting, building) and every unit of another side are never pushed, so body
//! blocking stays. Pushes never cross terrain.
//!
//! Every pair is read from the same positions and the corrections applied together
//! (Jacobi), a few rounds a tick, in id order: every seat moves identically.

use super::{Motion, alive, place, ready};
use crate::{
    entity::{Entity, EntityId},
    orders::Order,
    spatial::{Fixed, geometry::length_ceil},
    state::State,
};
use canopy_content::placement::Owner;
use std::collections::BTreeMap;

/// Buckets of the pair search, milli-cells.
const BUCKET: i32 = 8_000;
/// A body in a bucket of the pair search: where it stands this round.
#[derive(Debug, Clone, Copy)]
struct Member {
    x: i64,
    y: i64,
    radius: i64,
    index: u32,
}

/// How a body takes a push.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stance {
    /// On its way somewhere: pushes idle allies and squeezes past moving ones.
    Moving,
    /// Standing with nothing to do: gives way to allies.
    Idle,
    /// Holding, fighting, casting, building, stunned, rooted or unable to walk: nothing
    /// moves it.
    Planted,
}

#[derive(Debug, Clone, Copy)]
struct Body {
    id: EntityId,
    owner: Owner,
    at: Fixed,
    radius: i64,
    air: bool,
    stance: Stance,
    /// Where a moving body is heading (towards its next waypoint), milli-cells.
    heading: (i64, i64),
}

/// Whether `a` and `b` are on one side (one owner, or allies).
fn friends(motion: &Motion, a: Owner, b: Owner) -> bool {
    a == b || motion.teams.allied(a, b)
}

impl Motion {
    /// How `e` takes a push now.
    pub fn stance(&self, state: &State, e: &Entity) -> Stance {
        let Some(u) = e.unit.as_ref() else {
            return Stance::Planted;
        };
        let walks = self
            .spatial
            .profiles
            .get(e.definition)
            .motion
            .is_some_and(|m| m.speed > 0.0);
        // Standing at a target in reach is fighting; one waiting out of reach (a second row)
        // gives way like any idle unit.
        let fighting = u.route.is_empty()
            && u.target
                .and_then(|t| state.entities.get(t))
                .is_some_and(|t| {
                    let range = self
                        .catalog
                        .get(e.definition)
                        .behaviors
                        .combat
                        .as_ref()
                        .map_or(0.0, |c| c.range);
                    self.spatial.body_range(e, t) <= (range + 0.5) * (range + 0.5)
                });
        let planted = !walks
            || matches!(u.order, Some(Order::Hold))
            || u.combat.attack.is_some()
            || fighting
            || u.work.job.is_some()
            || u.garrison.is_some()
            || e.caster.as_ref().is_some_and(|c| c.pending.is_some())
            || crate::abilities::statuses::stunned(&self.catalog, e)
            || crate::abilities::statuses::rooted(&self.catalog, e)
            || crate::abilities::deliveries::charging(&self.catalog, state, e.id);
        if planted {
            Stance::Planted
        } else if u.route.is_empty() {
            Stance::Idle
        } else {
            Stance::Moving
        }
    }

    /// Whether `other` is a wall to a unit of `owner`'s steering: a unit of another side,
    /// or a planted ally. Allies that walk or stand idle are pushed instead.
    pub(super) fn solid_to(&self, state: &State, owner: Owner, other: EntityId) -> bool {
        state.entities.get(other).is_none_or(|b| {
            !friends(self, owner, b.owner) || self.stance(state, b) == Stance::Planted
        })
    }

    /// Pushes overlapping bodies apart (the module's rules).
    pub(super) fn push_apart(
        &mut self,
        state: &mut State,
        start: &BTreeMap<EntityId, crate::entity::Milli>,
    ) {
        let _span = tracing::trace_span!("push").entered();
        let tick = state.tick;
        let mut bodies: Vec<Body> = Vec::new();
        for e in state.entities.iter() {
            if e.unit.is_none() || !ready(e, tick) || !alive(e) {
                continue;
            }
            let Some(record) = self.spatial.bodies.get(e.id) else {
                continue;
            };
            if !record.solid() {
                continue;
            }
            let heading = e
                .unit
                .as_ref()
                .and_then(|u| u.route.first())
                .map(|&n| {
                    let p = self.spatial.point(n);
                    (i64::from(p.x - e.position.x), i64::from(p.y - e.position.y))
                })
                .unwrap_or((0, 0));
            bodies.push(Body {
                id: e.id,
                owner: e.owner,
                at: Fixed::on(e.position.x, e.position.y, record.deck),
                radius: i64::from(record.radius),
                air: record.air,
                stance: self.stance(state, e),
                heading,
            });
        }
        if bodies.len() < 2 {
            return;
        }
        let tuning = self.spatial.tuning;
        let mut moved = vec![(0i64, 0i64); bodies.len()];
        // Buckets over a grid padded by one bucket round the bodies, each bucket's bodies
        // side by side with what the pair test reads. Corrections are integer sums, so the
        // order pairs are met in does not matter.
        let mut starts: Vec<u32> = Vec::new();
        let mut members: Vec<Member> = Vec::new();
        let mut slots: Vec<usize> = vec![0; bodies.len()];
        let mut cells = vec![(0i32, 0i32); bodies.len()];
        let mut fill: Vec<u32> = Vec::new();
        let mut delta: Vec<(i64, i64)> = Vec::new();
        // Every pair that can overlap is in neighbouring buckets once a bucket is as wide
        // as the widest reach, so any such width finds the same pairs as `BUCKET`; a crowd
        // of small bodies then tests far fewer.
        let widest = 2 * bodies.iter().map(|b| b.radius).max().unwrap_or(0) + tuning.push_room;
        let side = if widest > i64::from(BUCKET) {
            BUCKET
        } else {
            (widest as i32).max(1000)
        };
        for _ in 0..tuning.push_rounds {
            let at = |i: usize, moved: &[(i64, i64)]| {
                (
                    i64::from(bodies[i].at.x) + moved[i].0,
                    i64::from(bodies[i].at.y) + moved[i].1,
                )
            };
            let (mut lo, mut hi) = ((i32::MAX, i32::MAX), (i32::MIN, i32::MIN));
            for (i, cell) in cells.iter_mut().enumerate() {
                let (x, y) = at(i, &moved);
                *cell = ((x as i32).div_euclid(side), (y as i32).div_euclid(side));
                lo = (lo.0.min(cell.0), lo.1.min(cell.1));
                hi = (hi.0.max(cell.0), hi.1.max(cell.1));
            }
            let width = (hi.0 - lo.0 + 3) as usize;
            let height = (hi.1 - lo.1 + 3) as usize;
            let bucket = |(bx, by): (i32, i32), air: bool| {
                ((by - lo.1 + 1) as usize * width + (bx - lo.0 + 1) as usize) * 2 + usize::from(air)
            };
            // Counting sort into the buckets.
            starts.clear();
            starts.resize(width * height * 2 + 1, 0);
            for (i, body) in bodies.iter().enumerate() {
                let b = bucket(cells[i], body.air);
                slots[i] = b;
                starts[b + 1] += 1;
            }
            for b in 1..starts.len() {
                starts[b] += starts[b - 1];
            }
            members.clear();
            members.resize(
                bodies.len(),
                Member {
                    x: 0,
                    y: 0,
                    radius: 0,
                    index: 0,
                },
            );
            fill.clear();
            fill.extend_from_slice(&starts);
            for (i, body) in bodies.iter().enumerate() {
                let (x, y) = at(i, &moved);
                let at = &mut fill[slots[i]];
                members[*at as usize] = Member {
                    x,
                    y,
                    radius: body.radius,
                    index: i as u32,
                };
                *at += 1;
            }
            delta.clear();
            delta.resize(bodies.len(), (0, 0));
            let mut any = false;
            for i in 0..bodies.len() {
                let (ax, ay) = at(i, &moved);
                let (bx, by) = cells[i];
                let (air, own_reach) = (bodies[i].air, bodies[i].radius + tuning.push_room);
                for nx in bx - 1..=bx + 1 {
                    for ny in by - 1..=by + 1 {
                        let b = bucket((nx, ny), air);
                        // A bucket holds its bodies in index order: only those after `i`
                        // make new pairs.
                        let bucket = &members[starts[b] as usize..starts[b + 1] as usize];
                        let after = bucket.partition_point(|m| m.index as usize <= i);
                        for m in &bucket[after..] {
                            let j = m.index as usize;
                            let (cx, cy) = (m.x, m.y);
                            let (mut dx, mut dy) = (ax - cx, ay - cy);
                            let reach = own_reach + m.radius;
                            if dx.abs() >= reach
                                || dy.abs() >= reach
                                || bodies[i].at.deck != bodies[j].at.deck
                            {
                                continue;
                            }
                            let mut d = length_ceil(dx, dy);
                            if d >= reach {
                                continue;
                            }
                            if d == 0 {
                                // Stacked exactly: part along a direction fixed by ids.
                                let k = (bodies[i].id.0 ^ bodies[j].id.0) % 8;
                                let dirs = [
                                    (1000, 0),
                                    (707, 707),
                                    (0, 1000),
                                    (-707, 707),
                                    (-1000, 0),
                                    (-707, -707),
                                    (0, -1000),
                                    (707, -707),
                                ];
                                (dx, dy) = dirs[k as usize];
                                d = 1000;
                            }
                            let overlap = reach - d;
                            let (si, sj) = self.shares(&bodies[i], &bodies[j]);
                            if si == 0 && sj == 0 {
                                continue;
                            }
                            any = true;
                            // Shares are in hundredths of the overlap. An idle ally shoved by
                            // a mover steps aside out of its path rather than ahead of it.
                            let push = |share: i64, (px, py): (i64, i64)| {
                                let amount = (overlap * share + 99) / 100;
                                let l = length_ceil(px, py).max(1);
                                (px * amount / l, py * amount / l)
                            };
                            let away = (dx, dy);
                            let aside = |mover: &Body, (ox, oy): (i64, i64)| {
                                let (hx, hy) = mover.heading;
                                let h = length_ceil(hx, hy);
                                if h == 0 {
                                    return (ox, oy);
                                }
                                // The offset's part across the mover's heading, to the side
                                // the pushed body already stands on (right when dead ahead).
                                let (sx, sy) = (-hy, hx);
                                let lateral = (ox * sx + oy * sy) / h;
                                let side = if lateral >= 0 { 1 } else { -1 };
                                let forward = (ox * hx + oy * hy) / h;
                                (
                                    side * sx * 1000 / h + hx * forward.max(0) / h / 4,
                                    side * sy * 1000 / h + hy * forward.max(0) / h / 4,
                                )
                            };
                            let shoved = |pushed: &Body, mover: &Body, offset: (i64, i64)| {
                                if tuning.aside
                                    && pushed.stance == Stance::Idle
                                    && mover.stance == Stance::Moving
                                    && friends(self, pushed.owner, mover.owner)
                                {
                                    aside(mover, offset)
                                } else {
                                    offset
                                }
                            };
                            let pi = push(si, shoved(&bodies[i], &bodies[j], away));
                            let pj = push(sj, shoved(&bodies[j], &bodies[i], (-away.0, -away.1)));
                            let pj = (-pj.0, -pj.1);
                            delta[i].0 += pi.0;
                            delta[i].1 += pi.1;
                            delta[j].0 -= pj.0;
                            delta[j].1 -= pj.1;
                        }
                    }
                }
            }
            if !any {
                break;
            }
            for (m, d) in moved.iter_mut().zip(&delta) {
                m.0 += d.0;
                m.1 += d.1;
                let l = length_ceil(m.0, m.1);
                if l > tuning.push_most {
                    m.0 = m.0 * tuning.push_most / l;
                    m.1 = m.1 * tuning.push_most / l;
                }
            }
        }
        for (b, &(mx, my)) in bodies.iter().zip(&moved) {
            if mx == 0 && my == 0 {
                continue;
            }
            let Some(e) = state.entities.get(b.id) else {
                continue;
            };
            let body = self.spatial.body_of(e);
            let from = b.at;
            // Along the push, else sliding along whatever terrain is in the way.
            let candidates = [(mx, my), (mx, 0), (0, my)];
            let to = candidates.iter().find_map(|&(x, y)| {
                if x == 0 && y == 0 {
                    return None;
                }
                let to = Fixed::on(from.x + x as i32, from.y + y as i32, from.deck);
                // Clear from here, and from where it stood when the tick began (drawn as
                // one straight move).
                let began = start
                    .get(&b.id)
                    .map_or(from, |m| Fixed::on(m.x, m.y, from.deck));
                (self.spatial.clear_segment(from, to, None, &body)
                    && (began == from || self.spatial.clear_segment(began, to, None, &body)))
                .then_some(to)
            });
            if let Some(to) = to {
                let e = state.entities.get_mut(b.id).expect("pushed");
                place(&self.spatial, e, to);
                self.spatial.update_body(e);
            }
        }
    }

    /// How much of an overlap each of two bodies takes, in hundredths.
    fn shares(&self, a: &Body, b: &Body) -> (i64, i64) {
        use Stance::*;
        if !friends(self, a.owner, b.owner) {
            // Another side: nobody is pushed. A mover that walked into a standing foe is
            // put back; two moving foes part evenly.
            return match (a.stance, b.stance) {
                (Moving, Moving) | (Idle, Idle) => (50, 50),
                (Moving, _) | (Idle, Planted) => (100, 0),
                (_, Moving) | (Planted, Idle) => (0, 100),
                (Planted, Planted) => (0, 0),
            };
        }
        match (a.stance, b.stance) {
            (Planted, Planted) => (0, 0),
            (Planted, _) => (0, 100),
            (_, Planted) => (100, 0),
            // A moving unit shoves an idle ally out of its way.
            (Moving, Idle) => (0, 100),
            (Idle, Moving) => (100, 0),
            (Idle, Idle) => (50, 50),
            // Allies on the move squeeze: half the overlap a tick.
            (Moving, Moving) => {
                let half = self.spatial.tuning.squeeze / 2;
                (half, half)
            }
        }
    }
}
