//! Memory, strategy and army operations (old `observe`, `strategy`, `mission` and
//! `operations` of `ai/playerAI.ts`): what the AI remembers between decisions, which
//! operation the army takes on (assault, camp, scout, regroup), and how it carries it
//! out (scouting, home guard, defence, assembly, travel, engagement and retreat).

use super::{
    Beat,
    frame::{Frame, Ix, Known},
    geography::{Pt, dist},
    powers,
    state::{CampStatus, Memory, Mission, MissionKind, Spotted, Stage},
};
use crate::{action::Action, teams::Relation};
use canopy_content::{game::Kind, placement::Owner};

fn start_key(p: Pt) -> String {
    format!("start:{}:{}", p.x, p.y)
}

/// Updates memory from the frame (old `observe`).
pub fn observe(f: &Frame, s: &mut Memory) {
    let tick = f.tick;
    let reaction = f.rules.reaction_ticks;
    for &i in &f.own {
        let e = f.at(i);
        let hp = e.hp.unwrap_or(0);
        let old = s.damage.get(&e.id).copied();
        let react_at = match old {
            Some(old) if hp < old.hp => {
                if old.react_at > tick {
                    old.react_at.min(tick + reaction)
                } else {
                    tick + reaction
                }
            }
            Some(old) => old.react_at,
            None => tick,
        };
        s.damage.insert(
            e.id,
            super::state::Damage {
                hp,
                seen: tick,
                react_at,
            },
        );
    }
    let mut index: std::collections::BTreeMap<u32, usize> = s
        .sightings
        .iter()
        .enumerate()
        .map(|(i, e)| (e.id, i))
        .collect();
    for &i in &f.hostiles {
        let e = f.at(i);
        s.contacts.entry(e.id).or_insert(tick);
        if e.unit {
            let entry = Spotted {
                id: e.id,
                definition: e.def,
                point: f.geo.place(e.p),
                hp: e.hp.unwrap_or(0),
                seen: tick,
            };
            match index.get(&e.id) {
                Some(&at) => s.sightings[at] = entry,
                None => {
                    index.insert(e.id, s.sightings.len());
                    s.sightings.push(entry);
                }
            }
        }
    }
    s.sightings.retain(|e| {
        tick - e.seen < 800 && (!f.visible(f.geo.from_place(&e.point)) || f.get(e.id).is_some())
    });
    let excess = s
        .sightings
        .len()
        .saturating_sub(super::state::MAX_SIGHTINGS);
    s.sightings.drain(..excess);
    for site in &f.geo.camps {
        let known = s.camps.entry(site.id.clone()).or_default();
        let visible: Vec<u32> = f
            .hostiles
            .iter()
            .map(|&i| f.at(i))
            .filter(|e| e.owner == Owner::Nobody && dist(e.p, site.point) <= site.radius)
            .map(|e| e.id)
            .collect();
        if !visible.is_empty() {
            known.status = CampStatus::Active;
            known.seen = tick;
            for id in visible {
                if !known.seen_ids.contains(&id) {
                    known.seen_ids.push(id);
                }
            }
        } else if known.status != CampStatus::Cleared
            && [(0.0, 0.0), (6.0, 0.0), (-6.0, 0.0), (0.0, 6.0), (0.0, -6.0)]
                .iter()
                .all(|&(dx, dy)| f.visible(Pt::new(site.point.x + dx, site.point.y + dy)))
        {
            known.status = CampStatus::Empty;
            known.seen = tick;
            known.retry = tick + 2400;
        }
        for death in f.deaths {
            let id = death.id.0;
            if known.seen_ids.contains(&id) && !known.killed.contains(&id) {
                known.killed.push(id);
            }
        }
        if known.seen_ids.len() == site.composition.len()
            && !known.seen_ids.is_empty()
            && known.seen_ids.iter().all(|id| known.killed.contains(id))
        {
            known.status = CampStatus::Cleared;
        }
    }
    for &p in &f.geo.starts {
        if dist(p, f.home) > 20.0 && f.visible(p) {
            s.inspected.insert(start_key(p), tick);
        }
    }
    // Every memory is bounded or expires; debug history is never unbounded match state.
    let stale: Vec<u32> = s
        .damage
        .iter()
        .filter(|(_, d)| tick - d.seen > 800)
        .map(|(&id, _)| id)
        .collect();
    for id in stale {
        s.damage.remove(&id);
        s.orders.remove(&id);
        s.worker_returns.remove(&id);
    }
    s.failed_sites.retain(|_, until| *until > tick);
    s.inspected.retain(|key, value| {
        !((key.starts_with("reject:")
            || key.starts_with("item:")
            || key.starts_with("replacement:")
            || key.starts_with("stalled:"))
            && *value <= tick)
    });
    let sightings = &s.sightings;
    s.contacts
        .retain(|id, _| f.get(*id).is_some() || sightings.iter().any(|e| e.id == *id));
}

fn mission(f: &Frame, s: &mut Memory, kind: MissionKind, key: String, point: Pt, duration: u32) {
    let place = f.geo.place(point);
    s.mission = Some(Mission {
        kind,
        key,
        point: place.clone(),
        started: f.tick,
        until: f.tick + duration,
        stage: Stage::Assemble,
        progress_point: place,
        progress_tick: f.tick,
        members: Vec::new(),
    });
}

/// An enemy player's building, seen or remembered.
fn enemy_building(f: &Frame, e: &Known) -> bool {
    e.owner != Owner::Nobody
        && f.input.senses().teams.relation(f.me, e.owner) == Relation::Enemy
        && f.def(e).kind == Kind::Building
}

/// Where the army waits, regroups and falls back to, `reach` cells out of the base towards
/// the nearest known enemy building (else the map's centre), at most halfway there: out of
/// the workers' way and between them and the threat, instead of on the hall's doorstep.
pub(super) fn staging(f: &Frame, reach: f64) -> Pt {
    let home = f.home;
    let toward = f
        .all
        .iter()
        .filter(|e| enemy_building(f, e))
        .map(|e| e.p)
        .min_by(|a, b| dist(*a, home).total_cmp(&dist(*b, home)))
        .unwrap_or_else(|| {
            let c = f64::from(f.geo.size) / 2.0;
            Pt::new(c, c)
        });
    let (dx, dy) = (toward.x - home.x, toward.y - home.y);
    let d = (dx * dx + dy * dy).sqrt();
    if d < 1.0 {
        return home;
    }
    let reach = reach.min(d * 0.5);
    let point = Pt {
        x: home.x + dx / d * reach,
        y: home.y + dy / d * reach,
        deck: home.deck,
    }
    .rounded();
    if f.geo.connected(home, point) {
        point
    } else {
        home
    }
}

/// An operation whose squad gets no closer for this long is given up.
const STALL_TICKS: u32 = 600;
/// A given-up target is not chosen again for this long.
const STALL_RETRY_TICKS: u32 = 2400;

/// The memory key of a given-up operation target (`inspected`, until a tick).
fn stalled_key(key: &str) -> String {
    format!("stalled:{key}")
}

/// How far out the army stages, and the home guard stands.
pub(super) const STAGING: f64 = 12.0;
const GUARD_POST: f64 = 6.0;
/// Reinforcements that set out together.
const REINFORCEMENT_GROUP: usize = 3;

/// Chooses the army's next operation (old `strategy`).
pub fn strategy(b: &mut Beat) {
    let f = b.f;
    let tick = f.tick;
    let catalog = f.catalog();
    let hero = f.army.iter().map(|&i| f.at(i)).find(|e| f.def(e).is_hero());
    let level = hero.and_then(|h| f.stats(h)).map_or(1, |s| s.level);
    let xp = hero
        .and_then(|h| f.own_entity(h))
        .and_then(|e| e.progression.as_ref())
        .map_or(0, |p| p.experience);
    let thresholds = hero.and_then(|h| f.def(h).behaviors.progression.as_ref());
    let s = &mut *b.s;
    s.plan.hero_level = level;
    s.plan.hero_xp_needed = thresholds
        .and_then(|p| p.levels.get(level as usize))
        .map_or(xp, |l| l.experience)
        .saturating_sub(xp);
    if let Some(m) = &s.mission
        && tick < m.until
    {
        let point = f.geo.from_place(&m.point);
        let done = match m.kind {
            // A camp found empty before the mission began is worth another look: counting
            // it as done would choose it again at once, every strategy beat, and the army
            // would assemble at home for good.
            MissionKind::Camp => s.camps.get(&m.key).is_some_and(|c| {
                c.status == CampStatus::Cleared
                    || (c.status == CampStatus::Empty && c.seen > m.started)
            }),
            MissionKind::Scout => f.visible(point),
            MissionKind::Assault => {
                f.visible(point) && !f.hostiles.iter().any(|&i| dist(f.at(i).p, point) < 15.0)
            }
            _ => false,
        };
        if !done {
            return;
        }
        s.mission = None;
    }
    let rules = f.rules;
    let power: f64 = f.army.iter().map(|&i| f.power(f.at(i))).sum();
    // Targets the army could not get closer to lately wait, and ground the army cannot
    // walk to is no target.
    let stalled = |key: &str| {
        s.inspected
            .get(&stalled_key(key))
            .is_some_and(|&t| t > tick)
    };
    let mut buildings: Vec<&Known> = f
        .all
        .iter()
        .filter(|e| {
            enemy_building(f, e)
                && e.hostile != Some(false)
                && f.geo.connected(f.home, e.p)
                && !stalled(&e.id.to_string())
        })
        .collect();
    let dropoff = |e: &Known| {
        f.def(e)
            .behaviors
            .storage
            .as_ref()
            .is_some_and(|s| s.dropoff == Some(true))
    };
    buildings.sort_by(|a, b| {
        dropoff(b)
            .cmp(&dropoff(a))
            .then(dist(a.p, f.home).total_cmp(&dist(b.p, f.home)))
            .then(a.id.cmp(&b.id))
    });
    // Ready to attack: enough units for the stage of the match (the minimum, plus one a
    // minute, up to half the maximum) and clearly stronger than the enemy army seen lately;
    // a near-full army goes regardless.
    let minutes = tick / 2400;
    // After twenty minutes the numbers alone decide, so matches come to a head.
    let late = minutes >= 20;
    let wanted =
        (rules.army.minimum + minutes).min((rules.army.maximum / 2).max(rules.army.minimum));
    // Late on, an army that stopped growing is as big as it gets: the economy fields no
    // more than its own target (which follows the workers), and nothing in training means
    // it cannot afford more. Waiting for the numbers would farm camps for the rest of the
    // match, so such an army goes when clearly stronger than the enemy seen near the target.
    let training = f.buildings.iter().any(|&i| !f.queue(f.at(i)).is_empty());
    let stalled = late && (f.army.len() as u32 >= s.plan.army || !training);
    // The army lately seen around the target (not every opponent's, in a free-for-all).
    let enemy_army = |target: Pt| -> f64 {
        s.sightings
            .iter()
            .filter(|e| {
                let d = f.definition(e.definition);
                d.behaviors.camp_defense.is_none()
                    && d.behaviors.combat.is_some()
                    && dist(f.geo.from_place(&e.point), target) < 40.0
            })
            .map(|e| {
                f.nominal_power(
                    f.definition(e.definition),
                    Some(f64::from(e.hp)),
                    None,
                    None,
                    None,
                ) * (1.0 - f64::from(tick.saturating_sub(e.seen)) / 2400.0).max(0.0)
            })
            .sum()
    };
    let full = f.army.len() as u32 * 5 >= rules.army.maximum * 3;
    let ready = |target: &Known| {
        let count = f.army.len() as u32;
        let stronger = || power >= enemy_army(target.p) * 1.2;
        full || (count >= wanted && (late || stronger()))
            || (stalled && count >= rules.army.minimum && stronger())
    };
    let contested: Vec<crate::catalog::Def> = catalog
        .content
        .definitions
        .iter()
        .filter(|d| {
            d.behaviors
                .storage
                .as_ref()
                .is_some_and(|s| s.dropoff == Some(true))
                && !f
                    .buildings
                    .iter()
                    .any(|&i| catalog.id(f.at(i).def) == &d.id)
        })
        .filter_map(|d| f.find(d.placement_near.as_ref()?.source.as_str()))
        .collect();
    let xp_needed = s.plan.hero_xp_needed;
    struct Candidate<'c> {
        site: &'c super::geography::CampSite,
        strength: f64,
        score: f64,
        unlocks: bool,
    }
    let origin = hero
        .or_else(|| f.army.first().map(|&i| f.at(i)))
        .map_or(f.home, |e| e.p);
    let mut camps: Vec<Candidate> =
        f.geo
            .camps
            .iter()
            .filter(|c| {
                let memory = s.camps.get(&c.id);
                f.geo.connected(f.home, c.point)
                    && memory.is_none_or(|m| m.status != CampStatus::Cleared)
                    && memory.map_or(0, |m| m.retry) <= tick
            })
            .map(|c| {
                let seen: Vec<&Known> = f
                    .hostiles
                    .iter()
                    .map(|&i| f.at(i))
                    .filter(|e| e.owner == Owner::Nobody && dist(e.p, c.point) < c.radius)
                    .collect();
                let strength = if seen.is_empty() {
                    c.composition
                        .iter()
                        .map(|&d| f.nominal_power(f.definition(d), None, None, None, None))
                        .sum()
                } else {
                    seen.iter().map(|e| f.power(e)).sum()
                };
                let yield_xp: u32 = c
                    .composition
                    .iter()
                    .map(|&d| f.definition(d).experience_yield.unwrap_or(0))
                    .sum();
                let useful = f64::from(xp_needed.min(yield_xp));
                let risk: f64 = s
                    .sightings
                    .iter()
                    .filter(|e| {
                        f.definition(e.definition).behaviors.camp_defense.is_none()
                            && dist(f.geo.from_place(&e.point), c.point) < 24.0
                    })
                    .map(|e| {
                        f.nominal_power(
                            f.definition(e.definition),
                            Some(f64::from(e.hp)),
                            None,
                            None,
                            None,
                        ) * (1.0 - f64::from(tick - e.seen) / 800.0).max(0.0)
                    })
                    .sum();
                let unlocks =
                    f.geo.resources.iter().any(|r| {
                        contested.contains(&r.definition) && dist(r.point, c.point) < 16.0
                    });
                let empty = s
                    .camps
                    .get(&c.id)
                    .is_some_and(|m| m.status == CampStatus::Empty);
                let score = dist(origin, c.point) + strength * 0.6 + risk * 2.0
                    - useful * 0.08
                    - if unlocks { 65.0 } else { 0.0 }
                    + if empty { 60.0 } else { 0.0 };
                Candidate {
                    site: c,
                    strength,
                    score,
                    unlocks,
                }
            })
            .filter(|c| c.strength * f64::from(rules.army.camp_permille) <= power * 1000.0)
            .collect();
    camps.sort_by(|a, b| a.score.total_cmp(&b.score).then(a.site.id.cmp(&b.site.id)));
    let best = camps.first();
    let assault = |s: &mut Memory, target: &Known| {
        mission(
            f,
            s,
            MissionKind::Assault,
            target.id.to_string(),
            target.p,
            2400,
        );
    };
    if let Some(&target) = buildings.first()
        && ready(target)
        && !best.is_some_and(|c| c.unlocks)
        && (level >= 3 || best.is_none() || xp_needed == 0)
    {
        assault(s, target);
        s.plan.reason = "Use the trained army to pressure known enemy infrastructure".into();
        return;
    }
    if hero.is_some()
        && let Some(c) = best
        && (xp_needed > 0 || c.unlocks)
    {
        mission(
            f,
            s,
            MissionKind::Camp,
            c.site.id.clone(),
            c.site.point,
            2400,
        );
        s.plan.reason = if c.unlocks {
            format!("Secure a contested resource at {}", c.site.id)
        } else {
            format!("Earn useful hero experience at {}", c.site.id)
        };
        return;
    }
    if let Some(&target) = buildings.first()
        && ready(target)
    {
        assault(s, target);
        return;
    }
    let mut starts: Vec<Pt> = f
        .geo
        .starts
        .iter()
        .copied()
        .filter(|&p| dist(p, f.home) > 24.0 && f.geo.connected(f.home, p))
        .collect();
    let inspected = |p: &Pt| {
        s.inspected
            .get(&start_key(*p))
            .map_or(-1, |&t| i64::from(t))
    };
    starts.sort_by(|a, b| {
        inspected(a)
            .cmp(&inspected(b))
            .then(dist(*a, f.home).total_cmp(&dist(*b, f.home)))
            .then(a.x.total_cmp(&b.x))
            .then(a.y.total_cmp(&b.y))
    });
    if let Some(&p) = starts.first() {
        mission(f, s, MissionKind::Scout, start_key(p), p, 2400);
        s.plan.reason = "Locate the opponent using possible starting sites".into();
    } else {
        mission(
            f,
            s,
            MissionKind::Regroup,
            "home".into(),
            staging(f, STAGING),
            400,
        );
        s.plan.reason = "Reinforce and recover before another operation".into();
    }
}

fn move_to(f: &Frame, actors: &[&Known], to: Pt, attack_move: Option<bool>) -> Action {
    Action::Move {
        actors: actors.iter().map(|e| e.id).collect(),
        destination: f.geo.cell_point(to),
        attack_move,
        append: None,
    }
}

fn indices(f: &Frame, group: &[&Known]) -> Vec<Ix> {
    group.iter().filter_map(|e| f.index_of(e.id)).collect()
}

/// Carries out the operation: scouting, the home guard, defence, then the mission
/// (old `operations`).
pub fn operations(b: &mut Beat) {
    let _span = tracing::trace_span!("ai operations").entered();
    let f = b.f;
    let tick = f.tick;
    let rules = f.rules;
    let catalog = f.catalog();
    let home = f.home;
    let near = |a: &Known, d: f64| f.hostiles.iter().any(|&h| dist(f.at(h).p, a.p) < d);
    let army: Vec<&Known> = f.army.iter().map(|&i| f.at(i)).collect();
    let threats: Vec<&Known> = f
        .hostiles
        .iter()
        .map(|&i| f.at(i))
        .filter(|e| e.unit && dist(e.p, home) < 30.0)
        .collect();
    let threat_power: f64 = threats.iter().map(|e| f.power(e)).sum();
    let local: Vec<&Known> = army
        .iter()
        .copied()
        .filter(|e| dist(e.p, home) < 32.0)
        .collect();
    let local_power: f64 = local.iter().map(|e| f.power(e)).sum();
    let total_power: f64 = army.iter().map(|e| f.power(e)).sum();
    let small_raid = rules
        .composition
        .iter()
        .filter_map(|c| f.find(c.definition.as_str()))
        .map(|d| f.nominal_power(catalog.get(d), None, None, None, None))
        .fold(f64::INFINITY, f64::min)
        * 2.0;
    let hall_danger = f.stores.iter().map(|&i| f.at(i)).any(|h| {
        f64::from(h.hp.unwrap_or(0)) < f64::from(f.max_hp(h)) * 0.55
            && threats.iter().any(|e| dist(e.p, h.p) < 12.0)
    });
    let severe =
        threat_power > (local_power * 0.85).max(total_power * 0.3).max(small_raid) || hall_danger;
    let enemy_known = f.all.iter().any(|e| enemy_building(f, e));
    if let Some(scout) = b.s.scout
        && !army.iter().any(|e| e.id == scout)
    {
        b.s.scout = None;
        b.s.next_scout = tick + 800;
    }
    if enemy_known {
        b.s.scout = None;
    }
    if b.s.scout.is_none() && !enemy_known && army.len() >= 5 && tick >= b.s.next_scout {
        let guards = &b.s.guards;
        b.s.scout = army
            .iter()
            .filter(|e| {
                !f.def(e).is_hero()
                    && !guards.contains(&e.id)
                    && f64::from(e.hp.unwrap_or(0)) > f64::from(f.max_hp(e)) * 0.7
            })
            .map(|e| e.id)
            .min();
    }
    if let Some(scout) = b.s.scout.and_then(|id| f.get(id)) {
        let s = &*b.s;
        let inspected = |p: &Pt| {
            s.inspected
                .get(&start_key(*p))
                .map_or(-1, |&t| i64::from(t))
        };
        let mut sites: Vec<Pt> = f
            .geo
            .starts
            .iter()
            .copied()
            .filter(|&p| dist(p, home) > 24.0 && f.geo.connected(scout.p, p))
            .collect();
        sites.sort_by(|a, b| {
            inspected(a)
                .cmp(&inspected(b))
                .then(dist(*a, scout.p).total_cmp(&dist(*b, scout.p)))
                .then(a.x.total_cmp(&b.x))
                .then(a.y.total_cmp(&b.y))
        });
        if let Some(&target) = sites.first() {
            let threatened = f
                .hostiles
                .iter()
                .map(|&i| f.at(i))
                .any(|e| f.def(e).behaviors.combat.is_some() && dist(e.p, scout.p) < 8.0);
            let goal = if threatened { home } else { target };
            let destination = f.nearest_safe(goal, &indices(f, &[scout]));
            b.emit(
                move_to(f, &[scout], destination, None),
                if threatened {
                    "Preserve the scout after contact"
                } else {
                    "Scout the opponent while the hero develops"
                },
            );
        }
    }
    // Health never returns, so a wounded unit is pulled out only while it is fighting;
    // away from a fight it stays in the army instead of idling in the base for good.
    let retreat = f64::from(rules.army.retreat_health_permille);
    let wounded_in_fight = |e: &Known| {
        f64::from(e.hp.unwrap_or(0)) * 1000.0 < f64::from(f.max_hp(e)) * retreat && near(e, 14.0)
    };
    let healthy: Vec<&Known> = army
        .iter()
        .copied()
        .filter(|e| {
            !wounded_in_fight(e)
                && !f.casting(e)
                && !f.stunned(e)
                && powers::pickup_target(f, e).is_none()
        })
        .collect();
    let pulled: Vec<&Known> = army
        .iter()
        .copied()
        .filter(|e| wounded_in_fight(e) && !f.def(e).is_hero())
        .collect();
    if !pulled.is_empty() {
        let destination = f.nearest_safe(staging(f, STAGING), &indices(f, &pulled));
        b.emit(
            move_to(f, &pulled, destination, Some(false)),
            "Pull badly wounded units out of the fight",
        );
    }
    let scout = b.s.scout;
    b.s.guards
        .retain(|&id| Some(id) != scout && healthy.iter().any(|e| e.id == id));
    let guard_count = if healthy.len() > 6 {
        rules.army.home_guard as usize
    } else {
        0
    };
    let mut candidates: Vec<&Known> = healthy
        .iter()
        .copied()
        .filter(|e| !f.def(e).is_hero() && Some(e.id) != scout && !b.s.guards.contains(&e.id))
        .collect();
    candidates.sort_by(|a, b| {
        dist(a.p, home)
            .total_cmp(&dist(b.p, home))
            .then(a.id.cmp(&b.id))
    });
    for e in candidates {
        if b.s.guards.len() >= guard_count {
            break;
        }
        b.s.guards.push(e.id);
    }
    if guard_count == 0 {
        b.s.guards.clear();
    }
    let guards: Vec<&Known> = healthy
        .iter()
        .copied()
        .filter(|e| b.s.guards.contains(&e.id))
        .collect();
    if !threats.is_empty() {
        let responders: Vec<&Known> = if severe {
            healthy.clone()
        } else if !guards.is_empty() {
            guards.clone()
        } else if !local.is_empty() {
            local.clone()
        } else {
            let mut rest: Vec<&Known> = healthy
                .iter()
                .copied()
                .filter(|e| !f.def(e).is_hero() && Some(e.id) != scout)
                .collect();
            rest.sort_by(|a, b| {
                dist(a.p, home)
                    .total_cmp(&dist(b.p, home))
                    .then(a.id.cmp(&b.id))
            });
            rest.truncate(2);
            rest
        };
        if !responders.is_empty() {
            let target = threats
                .iter()
                .min_by(|a, b| {
                    dist(a.p, home)
                        .total_cmp(&dist(b.p, home))
                        .then(a.id.cmp(&b.id))
                })
                .expect("threat");
            let destination = f.nearest_safe(target.p, &indices(f, &responders));
            b.emit(
                move_to(f, &responders, destination, Some(true)),
                if severe {
                    "Defend the economy against a credible incursion"
                } else {
                    "Contain harassment with the home guard"
                },
            );
        }
        if severe {
            b.s.plan.reason = "Defend productive workers and the main hall".into();
            return;
        }
    }
    let Some(mut m) = b.s.mission.clone() else {
        return;
    };
    let available: Vec<&Known> = healthy
        .iter()
        .copied()
        .filter(|e| !guards.iter().any(|g| g.id == e.id) && Some(e.id) != scout)
        .collect();
    m.members.retain(|id| available.iter().any(|e| e.id == *id));
    if m.members.is_empty() {
        let anchor = available
            .iter()
            .find(|e| f.def(e).is_hero())
            .or_else(|| available.first())
            .map_or(home, |e| e.p);
        let mut members: Vec<&Known> = available
            .iter()
            .copied()
            .filter(|e| dist(e.p, anchor) < 32.0)
            .collect();
        members.sort_by(|a, b| {
            f.def(b)
                .is_hero()
                .cmp(&f.def(a).is_hero())
                .then(a.id.cmp(&b.id))
        });
        m.members = members.iter().take(24).map(|e| e.id).collect();
    }
    let mut squad: Vec<&Known> = available
        .iter()
        .copied()
        .filter(|e| m.members.contains(&e.id))
        .collect();
    if squad.is_empty() {
        b.s.mission = Some(m);
        return;
    }
    let leader = *squad
        .iter()
        .find(|e| f.def(e).is_hero())
        .unwrap_or(&squad[0]);
    let reinforcements: Vec<&Known> = available
        .iter()
        .copied()
        .filter(|e| !m.members.contains(&e.id))
        .collect();
    for &e in &reinforcements {
        if dist(e.p, leader.p) < super::continuity::REINFORCEMENT_JOIN_RADIUS
            && m.members.len() < 24
        {
            m.members.push(e.id);
            squad.push(e);
        }
    }
    let joining: Vec<&Known> = reinforcements
        .iter()
        .copied()
        .filter(|e| {
            !m.members.contains(&e.id)
                && !super::continuity::approaching_army(f, e, leader.p)
                && !near(e, 14.0)
        })
        .collect();
    b.s.mission = Some(m.clone());
    if !joining.is_empty() {
        // New units go out together, three or more at a time, unless the army fights near
        // home; alone they would be picked off on the way.
        let near_home = dist(leader.p, home) < 30.0;
        if joining.len() >= REINFORCEMENT_GROUP || near_home {
            let destination = f.nearest_safe(leader.p, &indices(f, &joining));
            b.emit(
                move_to(f, &joining, destination, Some(true)),
                "Send reinforcements to the active army",
            );
        } else {
            let destination = f.nearest_safe(staging(f, STAGING), &indices(f, &joining));
            b.emit(
                move_to(f, &joining, destination, Some(true)),
                "Gather reinforcements before sending them out",
            );
        }
    }
    let n = squad.len() as f64;
    let center = Pt::new(
        squad.iter().fold(0.0, |acc, e| acc + e.p.x) / n,
        squad.iter().fold(0.0, |acc, e| acc + e.p.y) / n,
    );
    let point = f.geo.from_place(&m.point);
    let destination = f.nearest_safe(point, &indices(f, &squad));
    // Progress is getting closer to the operation's point, or fighting on the way. The
    // squad's centre alone jitters as members shuffle and reinforcements join, and the
    // safe point may be where the squad already stands, so neither proves progress.
    let fighting = f.hostiles.iter().any(|&h| dist(f.at(h).p, center) < 18.0);
    let reference = f.geo.from_place(&m.progress_point);
    if m.progress_point == m.point || fighting || dist(center, point) + 3.0 < dist(reference, point)
    {
        m.progress_point = f.geo.place(center);
        m.progress_tick = tick;
    } else if tick - m.progress_tick > STALL_TICKS && dist(center, point) > 12.0 {
        if m.kind == MissionKind::Camp
            && let Some(camp) = b.s.camps.get_mut(&m.key)
        {
            camp.retry = tick + 1600;
        }
        if m.kind == MissionKind::Scout {
            // Scouting moves on to the start inspected longest ago.
            b.s.inspected.insert(m.key.clone(), tick);
        }
        b.s.inspected
            .insert(stalled_key(&m.key), tick + STALL_RETRY_TICKS);
        b.s.mission = None;
        b.s.next_strategy = tick;
        return;
    }
    b.s.mission = Some(m.clone());
    if m.kind == MissionKind::Scout && squad.len() > 3 {
        let scout = squad
            .iter()
            .copied()
            .filter(|e| !f.def(e).is_hero())
            .min_by(|a, b| {
                dist(a.p, destination)
                    .total_cmp(&dist(b.p, destination))
                    .then(a.id.cmp(&b.id))
            });
        if let Some(scout) = scout
            && !near(scout, 12.0)
        {
            b.emit(
                move_to(f, &[scout], destination, Some(false)),
                "Scout a possible opponent location",
            );
        }
        let rest: Vec<&Known> = squad
            .iter()
            .copied()
            .filter(|e| Some(e.id) != scout.map(|s| s.id) && !near(e, 14.0))
            .collect();
        if !rest.is_empty() {
            let destination = f.nearest_safe(staging(f, STAGING), &indices(f, &rest));
            b.emit(
                move_to(f, &rest, destination, Some(true)),
                "Gather reinforcements at home while the scout travels",
            );
        }
        return;
    }
    let visible: Vec<&Known> = f
        .hostiles
        .iter()
        .map(|&i| f.at(i))
        .filter(|e| dist(e.p, center) < 18.0)
        .collect();
    let enemy_power: f64 = visible.iter().map(|e| f.power(e)).sum();
    let our_power: f64 = squad.iter().map(|e| f.power(e)).sum();
    if enemy_power * f64::from(rules.army.engage_permille) > our_power * 1000.0
        && dist(center, home) > 25.0
    {
        let destination = f.nearest_safe(staging(f, STAGING), &indices(f, &squad));
        b.emit(
            move_to(f, &squad, destination, None),
            "Withdraw from a visibly unfavorable fight",
        );
        if m.kind == MissionKind::Camp
            && let Some(camp) = b.s.camps.get_mut(&m.key)
        {
            camp.retry = tick + 1200;
        }
        mission(
            f,
            b.s,
            MissionKind::Regroup,
            "home".into(),
            staging(f, STAGING),
            400,
        );
        return;
    }
    let gathered = squad.iter().filter(|e| dist(e.p, center) <= 14.0).count() as f64 >= n * 0.7;
    if m.stage == Stage::Assemble
        && tick - m.started < 400
        && visible.is_empty()
        && !gathered
        && dist(center, destination) > 20.0
    {
        let target = f.nearest_safe(center, &indices(f, &squad));
        b.emit(
            move_to(f, &squad, target, Some(true)),
            "Assemble the army before entering the next fight",
        );
        return;
    }
    m.stage = if visible.is_empty() {
        Stage::Travel
    } else {
        Stage::Engage
    };
    b.s.mission = Some(m.clone());
    let travelers: Vec<&Known> = squad.iter().copied().filter(|e| !near(e, 14.0)).collect();
    if !travelers.is_empty() {
        b.emit(
            move_to(f, &travelers, destination, Some(true)),
            if m.kind == MissionKind::Camp {
                "Approach the selected camp as a group"
            } else {
                "Advance the operation as a group"
            },
        );
    }
    // Once in reach of an observed building, target it explicitly to finish the match.
    let building = f
        .hostiles
        .iter()
        .map(|&i| f.at(i))
        .find(|e| f.def(e).kind == Kind::Building && dist(e.p, point) < 12.0);
    if let Some(building) = building {
        let attackers: Vec<u32> = squad
            .iter()
            .filter(|e| {
                dist(e.p, building.p) < 18.0
                    && !f
                        .hostiles
                        .iter()
                        .map(|&h| f.at(h))
                        .any(|h| h.unit && dist(h.p, e.p) < 10.0)
            })
            .map(|e| e.id)
            .collect();
        if !attackers.is_empty() {
            b.emit(
                Action::Attack {
                    actors: attackers,
                    target: building.id,
                    force: None,
                    append: None,
                },
                "Finish the exposed enemy building",
            );
        }
    }
    if threats.is_empty() && !guards.is_empty() && !guards.iter().any(|g| near(g, 14.0)) {
        let destination = f.nearest_safe(staging(f, GUARD_POST), &indices(f, &guards));
        b.emit(
            move_to(f, &guards, destination, Some(true)),
            "Keep a small guard near the workers",
        );
    }
}
