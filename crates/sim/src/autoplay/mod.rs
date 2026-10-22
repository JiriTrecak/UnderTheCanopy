//! Autoplay (docs/campaign.md "Autoplay"): a mission's plan, `autoplay.lua` beside its
//! `mission.lua`, steering player 1 to victory. It is a player, not a cheat:
//!
//! - it reads only what player 1 could know (its own units, what it sees or remembers, the
//!   objectives and scenes on its screen, the map's placements where the map puts them);
//! - it answers with ordinary [`Action`]s for player 1, which the caller sends through the
//!   normal queue (headless: `World::enqueue` for the next tick; in the game: the session),
//!   so an autoplay run is an ordinary replay;
//! - it decides the same on every run: the plan's Lua has no clock or randomness, every
//!   collection here is ordered, and it is asked once after every step.
//!
//! Its economy helper is the skirmish AI's (`World::assistant`), held to the mission's
//! tech, deciding outside the world and sending its commands as player 1's own.
//!
//! The plan waits while a cinematic plays (its commands would be refused) and never skips
//! a scene.

mod know;
pub mod lua;

use crate::{
    Action, World, abilities,
    action::{CastTarget, CellPoint, FoundationPoint, HalfCells, MAX_ACTORS, PlainPoint},
    ai, mission,
    state::State,
    stats,
};
use canopy_content::{Id, abilities::TargetingKind, map::UtcMap};
use know::{ME, alive, cells, distance};
use mlua::{IntoLuaMulti, Lua, MultiValue, Table, Value, Variadic};
use std::{collections::BTreeMap, sync::Arc};

/// The player a plan steers: player 1.
pub const PLAYER: u8 = 0;
const SECOND: u32 = 40;
/// How often standing orders are given again (fights and scenes interrupt them).
const REISSUE: u32 = 2 * SECOND;
/// A walk or attack that has not ended after this long is given up.
const STEP_LIMIT: u32 = 180 * SECOND;
/// Within this many cells of its goal, a walk has arrived.
const ARRIVED: f64 = 10.0;
/// How often, and how many cells at a time, a walk's goal comes nearer when nobody can
/// reach it.
const SHIFTS: u32 = 6;
const SHIFT: f64 = 4.0;
/// `pickup()` gathers the items lying this near the hero, asking for each at most so
/// often.
const LOOT_REACH: f64 = 14.0;
const LOOT_TRIES: u32 = 4;

/// Where a plan stands.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Running,
    /// The plan returned; its standing orders (push, hero, economy) go on.
    Finished,
    /// The plan stopped with an error; autoplay does nothing more.
    Failed(String),
}

pub struct Autoplay {
    plan: lua::Plan,
    map: Arc<UtcMap>,
    /// Where the map places each placement (public knowledge of the map).
    places: BTreeMap<String, (f64, f64)>,
    standing: Standing,
    status: Status,
    lines: Vec<String>,
    label: String,
    brain: Option<Brain>,
    memory: Memory,
}

/// What player 1 remembers beyond the fog's own memory: the placements it saw fall.
#[derive(Default)]
struct Memory {
    /// Placements by entity id (refreshed; only ever read for deaths it witnessed).
    names: BTreeMap<u32, String>,
    /// Placements player 1 saw die.
    fallen: std::collections::BTreeSet<String>,
    /// The map's buildings: what stands where the map puts it.
    buildings: Option<std::collections::BTreeSet<String>>,
}

/// The skirmish AI running player 1's economy.
struct Brain {
    brains: ai::Brains,
    memories: Vec<ai::Memory>,
    /// Commands sent after the last step, awaiting their receipts: (AI sequence, action).
    sent: Vec<(u32, Action)>,
}

#[derive(Default)]
struct Standing {
    step: Option<Step>,
    push: Option<Push>,
    heroes: Vec<HeroPlan>,
    economy: bool,
    /// `keep(unit, x, z)`: units held apart from the army, waiting at a point.
    kept: Vec<(String, (f64, f64))>,
    /// Their entity ids this tick, sorted.
    kept_ids: Vec<u32>,
}

/// A step that takes time.
#[derive(Debug, Clone)]
enum Step {
    Go {
        x: f64,
        z: f64,
        attack: bool,
        /// `go(x, z, "hero")`: the leading hero walks alone.
        alone: Option<u32>,
        began: u32,
        idle_since: Option<u32>,
        /// Times the goal moved nearer because nobody could walk there.
        shifted: u32,
        /// The army stood idle when the walk was last given again.
        stalled: bool,
        /// Units idle short of the goal when the walk was last given again, and those
        /// idle twice running: they cannot get there and are left out of it.
        idle_before: Vec<u32>,
        left: Vec<u32>,
    },
    Attack {
        id: String,
        began: u32,
        last: Option<(f64, f64)>,
        /// The entity once seen: the army keeps after it through the fog.
        target: Option<u32>,
        /// Where the army walks while it has not seen it: its place, brought nearer while
        /// nobody can walk there.
        approach: Option<(f64, f64)>,
        stalled: bool,
    },
    Pickup {
        id: String,
        unit: String,
        began: u32,
    },
    /// Every item lying in sight near the hero.
    Loot {
        unit: String,
        began: u32,
        /// Items asked for, and how often: one the hero cannot reach is given up.
        tried: BTreeMap<u32, u32>,
    },
    Wait {
        until: u32,
    },
    Objective {
        id: String,
        until: Option<u32>,
    },
    Clock {
        tick: u32,
    },
    Mass {
        n: usize,
        until: Option<u32>,
    },
}

#[derive(Debug, Clone)]
struct Push {
    x: f64,
    z: f64,
    /// The army attacks only while it is at least this many.
    least: usize,
}

/// `hero{…}`: what one hero does by itself.
#[derive(Debug, Clone, Default)]
struct HeroPlan {
    unit: String,
    learn: Vec<String>,
    heal: Option<String>,
    heal_below: f64,
    shield: Option<String>,
    shield_below: f64,
    potions: Vec<String>,
    potion_below: f64,
    /// Spells cast whenever the fight is on near the hero: on itself, or at its foe.
    fight: Vec<String>,
    /// When each spell was last cast: a refused one is not tried again at once.
    cast_at: BTreeMap<String, u32>,
}

impl Autoplay {
    /// A plan on `map`; nothing runs until the first [`Autoplay::tick`].
    pub fn new(source: &str, map: Arc<UtcMap>) -> Result<Self, String> {
        let plan = lua::Plan::new(source)?;
        let places = map
            .entities
            .iter()
            .map(|p| (p.id.clone(), (p.position.x, p.position.y)))
            .collect();
        Ok(Self {
            plan,
            map,
            places,
            standing: Standing::default(),
            status: Status::Running,
            lines: Vec::new(),
            label: String::new(),
            brain: None,
            memory: Memory::default(),
        })
    }

    pub fn status(&self) -> &Status {
        &self.status
    }

    /// The step the plan is on (`go 300,100`), for the screen.
    pub fn label(&self) -> &str {
        &self.label
    }

    /// The plan's lines since the last call: its `say`s, its errors and its end.
    pub fn drain_lines(&mut self) -> Vec<String> {
        std::mem::take(&mut self.lines)
    }

    /// After every step: plays the plan's turn and returns player 1's commands for the
    /// next tick, in order.
    pub fn tick(&mut self, world: &mut World) -> Vec<Action> {
        let _span = tracing::trace_span!("autoplay").entered();
        let mut out = Vec::new();
        self.receipts(world);
        if self.memory.buildings.is_none() {
            let catalog = world.catalog();
            self.memory.buildings = Some(
                self.map
                    .entities
                    .iter()
                    .filter(|p| {
                        catalog.find(p.definition.as_str()).is_some_and(|d| {
                            catalog.kind(d) == canopy_content::game::Kind::Building
                        })
                    })
                    .map(|p| p.id.clone())
                    .collect(),
            );
        }
        if world.tick().is_multiple_of(SECOND) {
            self.memory.names = world
                .state()
                .entities
                .iter()
                .filter_map(|e| Some((e.id.0, e.placement.clone()?)))
                .collect();
        }
        for death in world.sight().observed_deaths(ME) {
            if let Some(name) = self.memory.names.get(&death.id.0) {
                self.memory.fallen.insert(name.clone());
            }
        }
        if world.state().outcome.is_some() || matches!(self.status, Status::Failed(_)) {
            return out;
        }
        // A cinematic holds the plan: commands would be refused, and scenes are watched.
        if mission::blocks_commands(world.state()).is_some() {
            return out;
        }
        self.standing.kept_ids = self
            .standing
            .kept
            .iter()
            .filter_map(|(unit, _)| know::own(world, unit).map(|e| e.id.0))
            .collect();
        self.standing.kept_ids.sort_unstable();
        if self.status == Status::Running {
            let mut turn = Turn {
                world,
                places: &self.places,
                standing: &mut self.standing,
                out: &mut out,
                lines: &mut self.lines,
                label: &mut self.label,
                memory: &mut self.memory,
            };
            match self
                .plan
                .resume(&mut |lua, name, args| turn.call(lua, name, args))
            {
                Ok(true) => {
                    self.status = Status::Finished;
                    self.lines.push(line(world.state(), "the plan is done"));
                }
                Ok(false) => {}
                Err(error) => {
                    self.lines
                        .push(line(world.state(), &format!("the plan stopped: {error}")));
                    self.status = Status::Failed(error);
                    return out;
                }
            }
        }
        standing(world, &self.places, &mut self.standing, &mut out);
        if self.standing.economy {
            let claimed = claimed(world, &self.standing);
            self.economy(world, &claimed, &mut out);
        }
        out
    }

    /// Where the camera follows the plan: its fight, else its leading hero, else its army.
    pub fn focus(&self, world: &World) -> Option<(f32, f32)> {
        let army = know::army(world, &self.standing.kept_ids);
        let fighting: Vec<_> = army
            .iter()
            .copied()
            .filter(|e| e.unit.as_ref().is_some_and(|u| u.target.is_some()))
            .collect();
        let point = know::middle(&fighting)
            .or_else(|| leader(world, &self.standing).map(cells))
            .or_else(|| know::middle(&army))?;
        Some((point.0 as f32, point.1 as f32))
    }

    /// The economy's receipts: which of the commands it sent were applied.
    fn receipts(&mut self, world: &World) {
        let Some(brain) = &mut self.brain else {
            return;
        };
        if brain.sent.is_empty() {
            return;
        }
        let tick = world.tick();
        for applied in world.applied().iter().filter(|a| a.player == PLAYER) {
            if let Some(i) = brain.sent.iter().position(|(_, a)| *a == applied.action) {
                let (seq, _) = brain.sent.remove(i);
                ai::Brains::receipt(&mut brain.memories, PLAYER, seq, tick, applied.accepted);
            }
        }
        // Whatever never arrived was dropped on the way: refused.
        for (seq, _) in std::mem::take(&mut brain.sent) {
            ai::Brains::receipt(&mut brain.memories, PLAYER, seq, tick, false);
        }
    }

    fn economy(&mut self, world: &mut World, claimed: &[u32], out: &mut Vec<Action>) {
        if self.brain.is_none() {
            let (brains, memories) = world.assistant(&self.map, PLAYER);
            self.brain = Some(Brain {
                brains,
                memories,
                sent: Vec::new(),
            });
        }
        let Some(brain) = &mut self.brain else {
            return;
        };
        // The mission may unlock tech as it goes (`mission.allow`).
        if world.tick().is_multiple_of(10 * SECOND) {
            brain
                .brains
                .forbid(PLAYER, crate::campaign::locked(world.state(), know::ME));
        }
        for command in world.assist(&brain.brains, &mut brain.memories) {
            // The plan's walks and pushes keep their units: the AI's orders for them are
            // not sent (and count as refused).
            let Some(action) = unclaimed(command.action, claimed) else {
                ai::Brains::receipt(
                    &mut brain.memories,
                    PLAYER,
                    command.seq,
                    world.tick(),
                    false,
                );
                continue;
            };
            brain.sent.push((command.seq, action.clone()));
            out.push(action);
        }
    }
}

/// The units the plan commands now: the army while it walks, attacks or pushes (at its
/// strength), the hero fetching an item.
fn claimed(world: &World, standing: &Standing) -> Vec<u32> {
    let army = know::army(world, &standing.kept_ids);
    let mut ids = match &standing.step {
        Some(Step::Go {
            alone: Some(hero), ..
        }) => vec![*hero],
        Some(Step::Go { .. } | Step::Attack { .. }) => know::ids(&army),
        Some(Step::Pickup { unit, .. } | Step::Loot { unit, .. }) => know::own(world, unit)
            .map(|e| vec![e.id.0])
            .unwrap_or_default(),
        _ => match &standing.push {
            Some(push) if army.len() >= push.least => know::ids(&army),
            _ => Vec::new(),
        },
    };
    // A kept unit badly hurt is the AI's again: it pulls back and heals as it knows how.
    ids.extend(standing.kept_ids.iter().copied().filter(|&id| {
        world
            .state()
            .entities
            .get(crate::entity::EntityId(id))
            .is_none_or(|e| know::health(world, e) >= KEPT_HURT)
    }));
    ids.sort_unstable();
    ids.dedup();
    ids
}

/// Below this share of health a kept unit is no longer held at its point.
const KEPT_HURT: f64 = 0.5;

/// An AI command without the units the plan has claimed; `None` when nothing is left of
/// it. Orders only: training, building, research and spells pass.
fn unclaimed(mut action: Action, claimed: &[u32]) -> Option<Action> {
    let mine = |id: &u32| claimed.binary_search(id).is_ok();
    match &mut action {
        Action::Move { actors, .. }
        | Action::Attack { actors, .. }
        | Action::Stop { actors }
        | Action::Hold { actors, .. }
        | Action::Patrol { actors, .. }
        | Action::Follow { actors, .. }
        | Action::Garrison { actors, .. } => {
            actors.retain(|id| !mine(id));
            (!actors.is_empty()).then_some(action)
        }
        Action::Pickup { actor, .. } if mine(actor) => None,
        _ => Some(action),
    }
}

/// A plan's line, with the mission clock in front.
fn line(state: &State, text: &str) -> String {
    let s = state.tick / SECOND;
    format!("{:2}:{:02}  {text}", s / 60, s % 60)
}

/// The hero the plan leads with: the first of its `hero{}`s alive, else any of its heroes.
fn hero_lead<'a>(world: &'a World, standing: &Standing) -> Option<&'a crate::entity::Entity> {
    standing
        .heroes
        .iter()
        .find_map(|h| know::own(world, &h.unit))
        .or_else(|| {
            know::army(world, &[])
                .into_iter()
                .find(|e| know::hero(world, e))
        })
}

/// The hero the army walks with: the plan's lead unless it is kept apart, else any hero
/// in the army.
fn leader<'a>(world: &'a World, standing: &Standing) -> Option<&'a crate::entity::Entity> {
    let kept = &standing.kept_ids;
    standing
        .heroes
        .iter()
        .filter_map(|h| know::own(world, &h.unit))
        .find(|e| kept.binary_search(&e.id.0).is_err())
        .or_else(|| {
            know::army(world, kept)
                .into_iter()
                .find(|e| know::hero(world, e))
        })
}

/// Where the army stands: its leader, else its middle.
fn front(world: &World, standing: &Standing) -> Option<(f64, f64)> {
    leader(world, standing)
        .map(cells)
        .or_else(|| know::middle(&know::army(world, &standing.kept_ids)))
}

fn cell(x: f64, z: f64) -> CellPoint {
    CellPoint {
        x: x.max(0.0).round() as u32,
        y: z.max(0.0).round() as u32,
        surface: None,
    }
}

/// Moves `actors` (attack-moving when `attack`), in groups of at most 160.
fn move_all(out: &mut Vec<Action>, actors: &[u32], x: f64, z: f64, attack: bool) {
    for group in actors.chunks(MAX_ACTORS) {
        out.push(Action::Move {
            actors: group.to_vec(),
            destination: cell(x, z),
            attack_move: Some(attack),
            append: None,
        });
    }
}

fn attack_all(out: &mut Vec<Action>, actors: &[u32], target: u32) {
    for group in actors.chunks(MAX_ACTORS) {
        out.push(Action::Attack {
            actors: group.to_vec(),
            target,
            force: Some(true),
            append: None,
        });
    }
}

/// Who walks: the army, or the one hero walking alone.
fn walkers<'a>(
    world: &'a World,
    alone: Option<u32>,
    kept: &[u32],
) -> Vec<&'a crate::entity::Entity> {
    match alone {
        Some(hero) => know::army(world, &[])
            .into_iter()
            .filter(|e| e.id.0 == hero)
            .collect(),
        None => know::army(world, kept),
    }
}

/// Within this many cells of its goal, a hero walking alone has arrived.
const ARRIVED_ALONE: f64 = 3.0;

/// Nobody in the army has an order.
fn idle(army: &[&crate::entity::Entity]) -> bool {
    army.iter()
        .all(|e| e.unit.as_ref().is_some_and(|u| u.order.is_none()))
}

/// The standing orders of one tick: the step's commands again, the push, the heroes.
fn standing(
    world: &World,
    places: &BTreeMap<String, (f64, f64)>,
    standing: &mut Standing,
    out: &mut Vec<Action>,
) {
    let tick = world.tick();
    let kept = standing.kept_ids.clone();
    let kept = kept.as_slice();
    let front = front(world, standing);
    // The leader stands without an order (its walk was refused, or it is done).
    let leader_idle =
        leader(world, standing).is_some_and(|e| e.unit.as_ref().is_some_and(|u| u.order.is_none()));
    let leader_id = leader(world, standing).map(|e| e.id.0);
    let busy = match &mut standing.step {
        Some(Step::Go {
            x,
            z,
            attack,
            began,
            shifted,
            stalled,
            idle_before,
            left,
            alone,
            ..
        }) => {
            if tick > *began && (tick - *began).is_multiple_of(REISSUE) {
                let army = walkers(world, *alone, kept);
                let front = match alone {
                    Some(_) => army.first().map(|e| cells(e)),
                    None => front,
                };
                let still = idle(&army);
                let short: Vec<u32> = army
                    .iter()
                    .filter(|e| e.unit.as_ref().is_some_and(|u| u.order.is_none()))
                    .filter(|e| distance(cells(e), (*x, *z)) > ARRIVED)
                    .map(|e| e.id.0)
                    .collect();
                // Everyone stood still short of the goal twice running, the walk given
                // in between: the ground there cannot be walked to, so aim a little
                // nearer, as a player clicks again.
                if let Some(at) = front
                    && *shifted < SHIFTS
                    && still
                    && *stalled
                    && distance(at, (*x, *z)) > ARRIVED
                {
                    let d = distance(at, (*x, *z));
                    let step = SHIFT.min(d - ARRIVED / 2.0);
                    *x += (at.0 - *x) / d * step;
                    *z += (at.1 - *z) / d * step;
                    *shifted += 1;
                }
                *stalled = still;
                if !still {
                    // The leader is never left behind: the walk is measured by it.
                    let lead = leader_id;
                    left.extend(
                        short
                            .iter()
                            .filter(|id| idle_before.contains(id) && Some(**id) != lead),
                    );
                    left.sort_unstable();
                    left.dedup();
                }
                *idle_before = short;
                let walking: Vec<u32> = know::ids(&army)
                    .into_iter()
                    .filter(|id| left.binary_search(id).is_err())
                    .collect();
                move_all(out, &walking, *x, *z, *attack);
            }
            true
        }
        Some(Step::Attack {
            id,
            began,
            last,
            target,
            approach,
            stalled,
        }) => {
            if tick > *began && (tick - *began).is_multiple_of(REISSUE) {
                let still = leader_idle;
                let unseen =
                    know::placement(world, id).is_none() && still_there(world, *target).is_none();
                if unseen
                    && still
                    && *stalled
                    && let (Some(at), Some(goal)) = (
                        front,
                        approach
                            .or(*last)
                            .or_else(|| places.get(id.as_str()).copied()),
                    )
                {
                    let d = distance(at, goal);
                    if d > ARRIVED {
                        *approach = Some((
                            goal.0 + (at.0 - goal.0) / d * SHIFT,
                            goal.1 + (at.1 - goal.1) / d * SHIFT,
                        ));
                    }
                }
                *stalled = still;
                order_attack(world, places, id, approach.or(*last), target, kept, out);
            }
            true
        }
        Some(Step::Pickup { id, unit, began }) => {
            if tick > *began && (tick - *began).is_multiple_of(REISSUE) {
                order_pickup(world, places, id, unit, out);
            }
            false
        }
        Some(Step::Loot { unit, began, tried }) => {
            if tick > *began && (tick - *began).is_multiple_of(REISSUE) {
                order_loot(world, unit, tried, out);
            }
            false
        }
        _ => false,
    };
    if !busy
        && let Some(push) = &standing.push
        && tick.is_multiple_of(REISSUE)
    {
        let army = know::army(world, kept);
        if army.len() >= push.least {
            move_all(out, &know::ids(&army), push.x, push.z, true);
        }
    }
    // Units kept apart walk back to their point when they stray from it.
    if tick.is_multiple_of(REISSUE) {
        for (unit, at) in &standing.kept {
            if let Some(e) = know::own(world, unit)
                && distance(cells(e), *at) > 4.0
                && know::health(world, e) >= KEPT_HURT
                && e.unit.as_ref().is_some_and(|u| u.order.is_none())
            {
                move_all(out, &[e.id.0], at.0, at.1, false);
            }
        }
    }
    if tick % REISSUE == REISSUE / 2 {
        for plan in &mut standing.heroes {
            let before = out.len();
            hero_turn(world, plan, out);
            for action in &out[before..] {
                if let Action::CastAbility { binding, .. } = action {
                    plan.cast_at.insert(binding.clone(), tick);
                }
            }
        }
    }
}

/// The entity an attack once saw, while it lives.
fn still_there(world: &World, target: Option<u32>) -> Option<&crate::entity::Entity> {
    let id = crate::entity::EntityId(target?);
    world.state().entities.get(id).filter(|e| alive(e))
}

fn order_attack(
    world: &World,
    places: &BTreeMap<String, (f64, f64)>,
    id: &str,
    last: Option<(f64, f64)>,
    target: &mut Option<u32>,
    kept: &[u32],
    out: &mut Vec<Action>,
) {
    let army = know::ids(&know::army(world, kept));
    if army.is_empty() {
        return;
    }
    if let Some(seen) = know::placement(world, id).filter(|e| alive(e)) {
        *target = Some(seen.id.0);
    }
    match still_there(world, *target) {
        Some(e) => attack_all(out, &army, e.id.0),
        None => {
            if let Some((x, z)) = last.or_else(|| places.get(id).copied()) {
                move_all(out, &army, x, z, true);
            }
        }
    }
}

fn order_pickup(
    world: &World,
    places: &BTreeMap<String, (f64, f64)>,
    id: &str,
    unit: &str,
    out: &mut Vec<Action>,
) {
    let Some(hero) = know::own(world, unit) else {
        return;
    };
    // The hero walks up to it first: a pickup from afar is routed through the crowd.
    match know::placement(world, id).filter(|e| e.item.is_some()) {
        Some(item) if distance(cells(item), cells(hero)) <= 5.0 => out.push(Action::Pickup {
            actor: hero.id.0,
            target: item.id.0,
            append: None,
        }),
        Some(item) => {
            let (x, z) = cells(item);
            move_all(out, &[hero.id.0], x, z, false);
        }
        None => {
            if let Some(&(x, z)) = places.get(id) {
                move_all(out, &[hero.id.0], x, z, false);
            }
        }
    }
}

/// The nearest item lying in sight within reach of `hero`.
fn loose_item<'a>(
    world: &'a World,
    hero: &crate::entity::Entity,
    tried: &BTreeMap<u32, u32>,
) -> Option<&'a crate::entity::Entity> {
    let at = cells(hero);
    world
        .state()
        .entities
        .iter()
        .filter(|e| e.item.is_some() && world.sight().seen(ME, e))
        .filter(|e| tried.get(&e.id.0).is_none_or(|&n| n < LOOT_TRIES))
        .map(|e| (distance(cells(e), at), e))
        .filter(|(d, _)| *d <= LOOT_REACH)
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.id.cmp(&b.1.id)))
        .map(|(_, e)| e)
}

fn order_loot(world: &World, unit: &str, tried: &mut BTreeMap<u32, u32>, out: &mut Vec<Action>) {
    let Some(hero) = know::own(world, unit) else {
        return;
    };
    if let Some(item) = loose_item(world, hero, tried) {
        *tried.entry(item.id.0).or_default() += 1;
        out.push(Action::Pickup {
            actor: hero.id.0,
            target: item.id.0,
            append: None,
        });
    }
}

/// Whether the hero of `plan` can cast `binding` now, and has not just tried.
fn ready_for(world: &World, plan: &HeroPlan, e: &crate::entity::Entity, binding: &str) -> bool {
    let recent = plan
        .cast_at
        .get(binding)
        .is_some_and(|&t| world.tick() < t + 3 * REISSUE);
    !recent && ready(world, e, binding)
}

/// Whether `e` can cast its ability `binding` now: learned, off cooldown, with the mana,
/// and not already holding it up (a stance or aura it keeps).
fn ready(world: &World, e: &crate::entity::Entity, binding: &str) -> bool {
    let catalog = world.catalog();
    let (Some(caster), Some(policy)) = (e.caster.as_deref(), catalog.spells.caster(e.definition))
    else {
        return false;
    };
    let Some((index, d)) = policy.binding(binding) else {
        return false;
    };
    let rank = caster.ranks.get(index).copied().unwrap_or(0);
    if rank == 0 || caster.pending.is_some() {
        return false;
    }
    let spell = &catalog.content.abilities.abilities[usize::from(d.ability.0)];
    if abilities::library::passive(spell) {
        return false;
    }
    let cost = abilities::library::value(spell, rank, &spell.cast.cost.amount).max(0) as u32;
    let cooling = caster
        .cooldowns
        .get(&d.ability)
        .is_some_and(|&t| t > world.tick());
    let held = world
        .state()
        .abilities
        .instances
        .iter()
        .any(|i| i.source == e.id && i.ability == d.ability);
    !cooling && !held && caster.mana >= cost
}

/// A hero's own turn: learns, shields itself, heals the most wounded near it, drinks.
fn hero_turn(world: &World, plan: &HeroPlan, out: &mut Vec<Action>) {
    let Some(hero) = know::own(world, &plan.unit) else {
        return;
    };
    let catalog = world.catalog();
    let actor = hero.id.0;
    if let (Some(caster), Some(policy)) = (
        hero.caster.as_deref(),
        catalog.spells.caster(hero.definition),
    ) {
        let level = stats::level(catalog, hero);
        let rank = |index: usize| caster.ranks.get(index).copied().unwrap_or(0);
        let spent: u32 = policy
            .bindings
            .iter()
            .enumerate()
            .map(|(i, d)| u32::from(rank(i).saturating_sub(d.initial_rank)))
            .sum();
        if spent < level {
            // The list in order; a name listed twice is learned to rank two.
            let next = plan.learn.iter().enumerate().find(|&(at, name)| {
                let wanted = plan.learn[..=at].iter().filter(|n| *n == name).count();
                policy.binding(name).is_some_and(|(index, d)| {
                    usize::from(rank(index)) < wanted
                        && d.learning
                            .as_ref()
                            .and_then(|l| l.get(usize::from(rank(index))))
                            .is_some_and(|&required| required <= level)
                })
            });
            if let Some((_, name)) = next {
                out.push(Action::LearnAbility {
                    actor,
                    ability: name.clone(),
                });
                return;
            }
        }
    }
    let health = know::health(world, hero);
    if let Some(binding) = &plan.shield
        && health < plan.shield_below
        && ready_for(world, plan, hero, binding)
    {
        out.push(Action::CastAbility {
            actor,
            binding: binding.clone(),
            target: CastTarget::Unit { entity: actor },
        });
        return;
    }
    if health < plan.potion_below
        && let Some(equipment) = &hero.equipment
    {
        let slot = equipment.slots.iter().enumerate().find(|(i, item)| {
            item.is_some_and(|d| plan.potions.iter().any(|p| p == catalog.id(d).as_str()))
                && equipment
                    .runtime
                    .get(*i)
                    .and_then(Option::as_ref)
                    .is_none_or(|r| r.ready_tick <= world.tick())
        });
        if let Some((slot, _)) = slot {
            out.push(Action::UseItem {
                actor,
                slot: slot as u32,
            });
            return;
        }
    }
    let at = cells(hero);
    let army = know::army(world, &[]);
    if let Some(binding) = &plan.heal
        && ready_for(world, plan, hero, binding)
    {
        // The healer's own light never touches itself.
        let wounded = army
            .iter()
            .filter(|e| e.id != hero.id && distance(cells(e), at) <= 14.0)
            .map(|e| (know::health(world, e), e.id.0))
            .filter(|(h, _)| *h < plan.heal_below)
            .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        if let Some((_, target)) = wounded {
            out.push(Action::CastAbility {
                actor,
                binding: binding.clone(),
                target: CastTarget::Unit { entity: target },
            });
            return;
        }
    }
    // The fight near the hero: the foe of the nearest of its own that is fighting.
    let foe = army
        .iter()
        .filter(|e| distance(cells(e), at) <= 12.0)
        .filter_map(|e| {
            let target = e.unit.as_ref()?.target?;
            let foe = world.state().entities.get(target)?;
            (alive(foe) && world.sight().seen(ME, foe))
                .then(|| (distance(cells(e), at), e.id.0, foe))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)))
        .map(|(_, _, foe)| foe);
    let Some(foe) = foe else {
        return;
    };
    for binding in &plan.fight {
        if !ready_for(world, plan, hero, binding) {
            continue;
        }
        let Some((index, d)) = catalog
            .spells
            .caster(hero.definition)
            .and_then(|p| p.binding(binding))
        else {
            continue;
        };
        let spell = &catalog.content.abilities.abilities[usize::from(d.ability.0)];
        let rank = hero
            .caster
            .as_ref()
            .and_then(|c| c.ranks.get(index).copied())
            .unwrap_or(0);
        let range = abilities::library::value(spell, rank, &spell.targeting.range) as f64;
        let (fx, fz) = cells(foe);
        let target = match spell.targeting.kind {
            TargetingKind::Caster => CastTarget::Unit { entity: actor },
            _ if distance((fx, fz), at) > range + 1.0 => continue,
            TargetingKind::Unit => CastTarget::Unit { entity: foe.id.0 },
            TargetingKind::Point => CastTarget::Point {
                position: PlainPoint {
                    x: fx.round() as u32,
                    y: fz.round() as u32,
                },
            },
        };
        out.push(Action::CastAbility {
            actor,
            binding: binding.clone(),
            target,
        });
        return;
    }
}

/// One resume's view of the world and what the plan's calls change.
struct Turn<'a> {
    world: &'a World,
    places: &'a BTreeMap<String, (f64, f64)>,
    standing: &'a mut Standing,
    out: &'a mut Vec<Action>,
    lines: &'a mut Vec<String>,
    label: &'a mut String,
    memory: &'a mut Memory,
}

fn runtime(text: impl Into<String>) -> mlua::Error {
    mlua::Error::runtime(text.into())
}

fn number(args: &[Value], i: usize) -> Option<f64> {
    match args.get(i) {
        Some(Value::Integer(n)) => Some(*n as f64),
        Some(Value::Number(n)) => Some(*n),
        _ => None,
    }
}

fn need_number(args: &[Value], i: usize, what: &str) -> mlua::Result<f64> {
    number(args, i).ok_or_else(|| runtime(format!("{what} needs a number")))
}

fn text(args: &[Value], i: usize) -> Option<String> {
    match args.get(i) {
        Some(Value::String(s)) => s.to_str().ok().map(|s| s.to_string()),
        _ => None,
    }
}

fn need_text(args: &[Value], i: usize, what: &str) -> mlua::Result<String> {
    text(args, i).ok_or_else(|| runtime(format!("{what} needs a name")))
}

fn ticks(seconds: f64) -> u32 {
    (seconds.max(0.0) * f64::from(SECOND)).round() as u32
}

impl Turn<'_> {
    fn say(&mut self, text: &str) {
        self.lines.push(line(self.world.state(), text));
    }

    fn call(&mut self, lua: &Lua, name: &str, args: Variadic<Value>) -> mlua::Result<MultiValue> {
        let world = self.world;
        let tick = world.tick();
        match name {
            "begin" => {
                let kind = need_text(&args, 0, "a step")?;
                self.begin(&kind, &args[1..])?;
                ().into_lua_multi(lua)
            }
            "poll" => match self.poll() {
                Some(result) => {
                    self.standing.step = None;
                    (true, result).into_lua_multi(lua)
                }
                None => false.into_lua_multi(lua),
            },
            "ticks" => tick.into_lua_multi(lua),
            "say" => {
                let parts: Vec<String> = args
                    .iter()
                    .map(|v| match v {
                        Value::String(s) => s.to_string_lossy(),
                        Value::Integer(n) => n.to_string(),
                        Value::Number(n) => format!("{n}"),
                        Value::Boolean(b) => b.to_string(),
                        _ => "nil".into(),
                    })
                    .collect();
                self.say(&parts.join(" "));
                ().into_lua_multi(lua)
            }
            "hero" => {
                let Some(Value::Table(spec)) = args.first() else {
                    return Err(runtime("hero needs a table: hero{\"queen\", learn = {…}}"));
                };
                let plan = hero_plan(spec)?;
                self.standing.heroes.retain(|h| h.unit != plan.unit);
                self.standing.heroes.push(plan);
                ().into_lua_multi(lua)
            }
            "keep" => {
                let unit = need_text(&args, 0, "keep")?;
                self.standing.kept.retain(|(u, _)| *u != unit);
                match (number(&args, 1), number(&args, 2)) {
                    (Some(x), Some(z)) => {
                        if let Some(e) = know::own(world, &unit) {
                            move_all(self.out, &[e.id.0], x, z, false);
                            self.standing.kept_ids.push(e.id.0);
                            self.standing.kept_ids.sort_unstable();
                        }
                        self.say(&format!("{unit} waits at {x},{z}"));
                        self.standing.kept.push((unit, (x, z)));
                    }
                    _ => {
                        if let Some(e) = know::own(world, &unit) {
                            self.standing.kept_ids.retain(|id| *id != e.id.0);
                        }
                        self.say(&format!("{unit} rejoins the army"));
                    }
                }
                ().into_lua_multi(lua)
            }
            "economy" => {
                let on = !matches!(args.first(), Some(Value::Boolean(false)));
                if on != self.standing.economy {
                    self.say(if on {
                        "economy: the colony is the skirmish AI's"
                    } else {
                        "economy: off"
                    });
                }
                self.standing.economy = on;
                ().into_lua_multi(lua)
            }
            "push" => {
                self.standing.push = match (number(&args, 0), number(&args, 1)) {
                    (Some(x), Some(z)) => {
                        let least = number(&args, 2).unwrap_or(0.0).max(0.0) as usize;
                        self.say(&format!("push {x},{z}"));
                        *self.label = format!("push {x},{z}");
                        Some(Push { x, z, least })
                    }
                    _ => None,
                };
                ().into_lua_multi(lua)
            }
            "click" => {
                let id = need_text(&args, 0, "click")?;
                let times = number(&args, 1).unwrap_or(1.0).clamp(1.0, 20.0) as u32;
                let target = know::placement(world, &id).filter(|e| alive(e));
                if let Some(e) = target {
                    for _ in 0..times {
                        self.out.push(Action::Clicked { actor: e.id.0 });
                    }
                    self.say(&format!("click {id} ×{times}"));
                }
                target.is_some().into_lua_multi(lua)
            }
            "train" => {
                let unit = need_text(&args, 0, "train")?;
                let count = number(&args, 1).unwrap_or(1.0).clamp(1.0, 20.0) as usize;
                let n = self.train(&unit, count);
                n.into_lua_multi(lua)
            }
            "research" => {
                let id = need_text(&args, 0, "research")?;
                let catalog = world.catalog();
                let building = world.state().entities.iter().find(|e| {
                    e.owner == ME
                        && alive(e)
                        && e.construction.is_none()
                        && catalog
                            .get(e.definition)
                            .behaviors
                            .research
                            .as_ref()
                            .is_some_and(|r| r.outputs.iter().any(|o| o.as_str() == id))
                });
                if let (Some(b), Ok(research)) = (building, Id::new(&id)) {
                    self.out.push(Action::Research {
                        actor: b.id.0,
                        research,
                    });
                }
                building.is_some().into_lua_multi(lua)
            }
            "build" => {
                let definition = need_text(&args, 0, "build")?;
                let x = need_number(&args, 1, "build")?;
                let z = need_number(&args, 2, "build")?;
                let catalog = world.catalog();
                let builds = |e: &&crate::entity::Entity| {
                    catalog
                        .get(e.definition)
                        .behaviors
                        .work
                        .as_ref()
                        .is_some_and(|w| w.builds.iter().any(|b| b.as_str() == definition))
                };
                let mut workers: Vec<_> = world
                    .state()
                    .entities
                    .iter()
                    .filter(|e| e.owner == ME && alive(e) && e.unit.is_some())
                    .filter(builds)
                    .collect();
                workers.sort_by(|a, b| {
                    distance(cells(a), (x, z))
                        .total_cmp(&distance(cells(b), (x, z)))
                        .then(a.id.cmp(&b.id))
                });
                let worker = workers.first().map(|e| e.id.0);
                if let (Some(worker), Ok(definition)) = (worker, Id::new(&definition)) {
                    self.out.push(Action::Build {
                        actors: vec![worker],
                        definition,
                        position: FoundationPoint {
                            x: HalfCells((x.max(0.0) * 2.0).round() as u32),
                            y: HalfCells((z.max(0.0) * 2.0).round() as u32),
                            surface: None,
                        },
                        rotation: None,
                        append: None,
                    });
                }
                worker.is_some().into_lua_multi(lua)
            }
            "cast" => {
                let unit = need_text(&args, 0, "cast")?;
                let binding = need_text(&args, 1, "cast")?;
                let Some(caster) = know::own(world, &unit) else {
                    return false.into_lua_multi(lua);
                };
                let target = match (text(&args, 2), number(&args, 2), number(&args, 3)) {
                    (Some(id), _, _) => match know::placement(world, &id).filter(|e| alive(e)) {
                        Some(e) => CastTarget::Unit { entity: e.id.0 },
                        None => return false.into_lua_multi(lua),
                    },
                    (None, Some(x), Some(z)) => CastTarget::Point {
                        position: PlainPoint {
                            x: x.max(0.0).round() as u32,
                            y: z.max(0.0).round() as u32,
                        },
                    },
                    _ => CastTarget::Unit {
                        entity: caster.id.0,
                    },
                };
                let ok = ready(world, caster, &binding);
                if ok {
                    self.out.push(Action::CastAbility {
                        actor: caster.id.0,
                        binding,
                        target,
                    });
                }
                ok.into_lua_multi(lua)
            }
            "use_item" => {
                let item = need_text(&args, 0, "use_item")?;
                let unit = text(&args, 1).or_else(|| self.leader_id());
                let holder = unit.as_deref().and_then(|u| know::own(world, u));
                let slot = holder.and_then(|h| {
                    let catalog = world.catalog();
                    h.equipment
                        .as_ref()?
                        .slots
                        .iter()
                        .position(|s| s.is_some_and(|d| catalog.id(d).as_str() == item))
                });
                if let (Some(h), Some(slot)) = (holder, slot) {
                    self.out.push(Action::UseItem {
                        actor: h.id.0,
                        slot: slot as u32,
                    });
                }
                slot.is_some().into_lua_multi(lua)
            }
            "learn" => {
                let unit = need_text(&args, 0, "learn")?;
                let ability = need_text(&args, 1, "learn")?;
                let hero = know::own(world, &unit);
                if let Some(h) = hero {
                    self.out.push(Action::LearnAbility {
                        actor: h.id.0,
                        ability,
                    });
                }
                hero.is_some().into_lua_multi(lua)
            }
            // Queries.
            "army" => know::army(world, &self.standing.kept_ids)
                .len()
                .into_lua_multi(lua),
            "army_center" => match know::middle(&know::army(world, &self.standing.kept_ids)) {
                Some((x, z)) => (x, z).into_lua_multi(lua),
                None => ().into_lua_multi(lua),
            },
            "alive" => {
                let id = need_text(&args, 0, "alive")?;
                self.alive(&id).into_lua_multi(lua)
            }
            "position" => {
                let id = need_text(&args, 0, "position")?;
                match know::placement(world, &id).filter(|e| alive(e)) {
                    Some(e) => cells(e).into_lua_multi(lua),
                    None => ().into_lua_multi(lua),
                }
            }
            "hp" => {
                let id = need_text(&args, 0, "hp")?;
                match know::placement(world, &id).filter(|e| alive(e)) {
                    Some(e) => know::health(world, e).into_lua_multi(lua),
                    None => ().into_lua_multi(lua),
                }
            }
            "level" => {
                let id = need_text(&args, 0, "level")?;
                match know::own(world, &id) {
                    Some(e) => stats::level(world.catalog(), e).into_lua_multi(lua),
                    None => ().into_lua_multi(lua),
                }
            }
            "objective" => {
                let id = need_text(&args, 0, "objective")?;
                let state = world
                    .state()
                    .mission
                    .as_ref()
                    .and_then(|m| m.objective_states.get(&id))
                    .map(|s| objective_name(*s));
                state.into_lua_multi(lua)
            }
            "stock" => {
                let item = need_text(&args, 0, "stock")?;
                let amount = world
                    .state()
                    .wallets
                    .get(&ME)
                    .and_then(|w| w.iter().find(|(k, _)| k.as_str() == item))
                    .map_or(0, |(_, v)| *v);
                amount.into_lua_multi(lua)
            }
            "count" => {
                let definition = need_text(&args, 0, "count")?;
                let catalog = world.catalog();
                let n = world
                    .state()
                    .entities
                    .iter()
                    .filter(|e| e.owner == ME && alive(e) && e.construction.is_none())
                    .filter(|e| catalog.id(e.definition).as_str() == definition)
                    .count();
                n.into_lua_multi(lua)
            }
            "scene" => world
                .state()
                .mission
                .as_ref()
                .and_then(|m| m.cinema.scene.as_ref())
                .map(|s| s.name.clone())
                .into_lua_multi(lua),
            "clock" => (f64::from(tick) / f64::from(SECOND)).into_lua_multi(lua),
            other => Err(runtime(format!("the plan has no function {other}"))),
        }
    }

    /// Whether a placement lives as player 1 knows: true or false, or nil when it cannot
    /// tell (never seen, or out of sight since).
    fn alive(&mut self, id: &str) -> Option<bool> {
        if let Some(e) = know::placement(self.world, id) {
            return Some(alive(e));
        }
        // A building gone from where the map put it, that ground in sight: it fell.
        if !self.memory.fallen.contains(id)
            && self
                .memory
                .buildings
                .as_ref()
                .is_some_and(|b| b.contains(id))
            && let Some(&at) = self.places.get(id)
            && know::in_sight(self.world, at)
        {
            self.memory.fallen.insert(id.to_string());
        }
        self.memory.fallen.contains(id).then_some(false)
    }

    fn leader_id(&self) -> Option<String> {
        hero_lead(self.world, self.standing).and_then(|e| e.placement.clone())
    }

    /// Queues `count` of `unit` at player 1's buildings that train it, the least busy first.
    fn train(&mut self, unit: &str, count: usize) -> usize {
        let world = self.world;
        let catalog = world.catalog();
        let Ok(definition) = Id::new(unit) else {
            return 0;
        };
        let mut buildings: Vec<(usize, u32)> = world
            .state()
            .entities
            .iter()
            .filter(|e| e.owner == ME && alive(e) && e.construction.is_none())
            .filter(|e| {
                catalog
                    .get(e.definition)
                    .behaviors
                    .production
                    .as_ref()
                    .is_some_and(|p| p.outputs.iter().any(|o| o.as_str() == unit))
            })
            .map(|e| {
                let queued = e.production.as_ref().map_or(0, |p| p.queue.len());
                (queued, e.id.0)
            })
            .collect();
        if buildings.is_empty() {
            return 0;
        }
        for _ in 0..count {
            buildings.sort_unstable();
            let first = &mut buildings[0];
            first.0 += 1;
            self.out.push(Action::Produce {
                actor: first.1,
                definition: definition.clone(),
            });
        }
        count
    }

    fn begin(&mut self, kind: &str, args: &[Value]) -> mlua::Result<()> {
        let world = self.world;
        let tick = world.tick();
        let step = match kind {
            "go" => {
                let x = need_number(args, 0, "go")?;
                let z = need_number(args, 1, "go")?;
                let how = text(args, 2);
                let attack = how.is_none();
                let alone = match how.as_deref() {
                    None | Some("walk") => None,
                    // No hero standing: nobody walks, and the step ends false.
                    Some("hero") => {
                        Some(hero_lead(world, self.standing).map_or(u32::MAX, |e| e.id.0))
                    }
                    Some(other) => {
                        return Err(runtime(format!(
                            "go: \"{other}\" is neither \"walk\" nor \"hero\""
                        )));
                    }
                };
                let kept = self.standing.kept_ids.clone();
                move_all(
                    self.out,
                    &know::ids(&walkers(world, alone, &kept)),
                    x,
                    z,
                    attack,
                );
                *self.label = match alone {
                    Some(_) => format!("hero to {x},{z}"),
                    None => format!("go {x},{z}"),
                };
                Step::Go {
                    x,
                    z,
                    attack,
                    alone,
                    began: tick,
                    idle_since: None,
                    shifted: 0,
                    stalled: false,
                    idle_before: Vec::new(),
                    left: Vec::new(),
                }
            }
            "attack" => {
                let id = need_text(args, 0, "attack")?;
                if !self.places.contains_key(&id) && know::placement(world, &id).is_none() {
                    return Err(runtime(format!("attack: the map places no {id}")));
                }
                let mut target = None;
                let kept = self.standing.kept_ids.clone();
                order_attack(world, self.places, &id, None, &mut target, &kept, self.out);
                *self.label = format!("attack {id}");
                Step::Attack {
                    id,
                    began: tick,
                    last: None,
                    target,
                    approach: None,
                    stalled: false,
                }
            }
            "pickup" => {
                let unit = text(args, 1)
                    .or_else(|| self.leader_id())
                    .ok_or_else(|| runtime("pickup: no hero to carry it"))?;
                match text(args, 0) {
                    Some(id) => {
                        order_pickup(world, self.places, &id, &unit, self.out);
                        *self.label = format!("pick up {id}");
                        Step::Pickup {
                            id,
                            unit,
                            began: tick,
                        }
                    }
                    None => {
                        let mut tried = BTreeMap::new();
                        order_loot(world, &unit, &mut tried, self.out);
                        *self.label = "pick up what lies near".into();
                        Step::Loot {
                            unit,
                            began: tick,
                            tried,
                        }
                    }
                }
            }
            "wait" => {
                let seconds = need_number(args, 0, "wait")?;
                *self.label = format!("wait {seconds} s");
                Step::Wait {
                    until: tick + ticks(seconds),
                }
            }
            "objective" => {
                let id = need_text(args, 0, "wait_objective")?;
                *self.label = format!("wait for {id}");
                Step::Objective {
                    id,
                    until: number(args, 1).map(|s| tick + ticks(s)),
                }
            }
            "clock" => {
                let minutes = need_number(args, 0, "wait_clock")?;
                *self.label = format!("wait until {minutes} min");
                Step::Clock {
                    tick: ticks(minutes * 60.0),
                }
            }
            "mass" => {
                let n = need_number(args, 0, "mass")?.max(0.0) as usize;
                *self.label = format!("gather {n}");
                Step::Mass {
                    n,
                    until: number(args, 1).map(|s| tick + ticks(s)),
                }
            }
            other => return Err(runtime(format!("no step {other}"))),
        };
        self.say(&self.label.clone());
        self.standing.step = Some(step);
        Ok(())
    }

    /// The step's result once it is done.
    fn poll(&mut self) -> Option<Value> {
        let world = self.world;
        let tick = world.tick();
        let front = front(world, self.standing);
        // What player 1 knows of an attack's target having fallen.
        let fallen = match &self.standing.step {
            Some(Step::Attack { id, .. }) => {
                let id = id.clone();
                self.alive(&id) == Some(false)
            }
            _ => false,
        };
        let step = self.standing.step.as_mut()?;
        let done = |ok: bool| Some(Value::Boolean(ok));
        match step {
            Step::Go {
                x,
                z,
                began,
                idle_since,
                shifted,
                alone,
                ..
            } => {
                let army = walkers(world, *alone, &self.standing.kept_ids);
                let front = match alone {
                    Some(_) => army.first().map(|e| cells(e)),
                    None => front,
                };
                let Some(at) = front.filter(|_| !army.is_empty()) else {
                    return done(false);
                };
                let d = distance(at, (*x, *z));
                if d <= if alone.is_some() {
                    ARRIVED_ALONE
                } else {
                    ARRIVED
                } {
                    return done(true);
                }
                // Standing still near the goal, or after every shift: as far as it goes.
                if idle(&army) && (d <= ARRIVED || *shifted >= SHIFTS) && tick > *began + REISSUE {
                    let since = *idle_since.get_or_insert(tick);
                    if tick >= since + 3 * SECOND {
                        return done(d <= 2.0 * ARRIVED);
                    }
                } else {
                    *idle_since = None;
                }
                (tick > *began + STEP_LIMIT).then_some(Value::Boolean(false))
            }
            Step::Attack {
                id,
                began,
                last,
                target,
                ..
            } => {
                match know::placement(world, id) {
                    Some(e) if alive(e) => {
                        *last = Some(cells(e));
                    }
                    Some(_) => return done(true),
                    None if fallen => return done(true),
                    // Once seen, the army follows it until it falls.
                    None if target.is_some() => {
                        if still_there(world, *target).is_none() {
                            return done(true);
                        }
                    }
                    None => {
                        // Unseen where it stood: gone.
                        let place = last.or_else(|| self.places.get(id.as_str()).copied());
                        if let (Some(at), Some(place)) = (front, place)
                            && distance(at, place) <= 6.0
                        {
                            return done(true);
                        }
                    }
                }
                (tick > *began + STEP_LIMIT).then_some(Value::Boolean(false))
            }
            Step::Pickup { id, unit, began } => {
                let item = world
                    .state()
                    .entities
                    .iter()
                    .find(|e| e.placement.as_deref() == Some(id.as_str()) && e.item.is_some());
                let carrier = know::own(world, unit);
                match (item, carrier) {
                    (_, None) => done(false),
                    (None, Some(h)) => {
                        let place = self.places.get(id.as_str()).copied();
                        let near = place.is_none_or(|p| distance(cells(h), p) <= 8.0);
                        if near {
                            done(true)
                        } else {
                            (tick > *began + STEP_LIMIT).then_some(Value::Boolean(false))
                        }
                    }
                    (Some(_), Some(_)) => {
                        (tick > *began + STEP_LIMIT).then_some(Value::Boolean(false))
                    }
                }
            }
            Step::Loot { unit, began, tried } => match know::own(world, unit) {
                None => done(false),
                Some(hero) if loose_item(world, hero, tried).is_none() => done(true),
                Some(_) => (tick > *began + 60 * SECOND).then_some(Value::Boolean(false)),
            },
            Step::Wait { until } => (tick >= *until).then_some(Value::Boolean(true)),
            Step::Objective { id, until } => {
                let state = world
                    .state()
                    .mission
                    .as_ref()
                    .and_then(|m| m.objective_states.get(id.as_str()))
                    .copied();
                match state {
                    Some(mission::state::ObjectiveState::Completed) => done(true),
                    Some(mission::state::ObjectiveState::Failed) => done(false),
                    _ => until.filter(|u| tick >= *u).map(|_| Value::Boolean(false)),
                }
            }
            Step::Clock { tick: at } => (tick >= *at).then_some(Value::Boolean(true)),
            Step::Mass { n, until } => {
                if know::army(world, &self.standing.kept_ids).len() >= *n {
                    done(true)
                } else {
                    until.filter(|u| tick >= *u).map(|_| Value::Boolean(false))
                }
            }
        }
    }
}

fn objective_name(state: mission::state::ObjectiveState) -> &'static str {
    match state {
        mission::state::ObjectiveState::Active => "active",
        mission::state::ObjectiveState::Completed => "completed",
        mission::state::ObjectiveState::Failed => "failed",
    }
}

/// `hero{"queen", learn = {…}, heal = "…", heal_below = 0.6, shield = "…",
/// shield_below = 0.4, potions = {…}, potion_below = 0.35, fight = {…}}`.
fn hero_plan(spec: &Table) -> mlua::Result<HeroPlan> {
    let unit: String = spec
        .raw_get::<Option<String>>(1)?
        .ok_or_else(|| runtime("hero{…} needs the hero's placement first: hero{\"queen\", …}"))?;
    let list = |key: &str| -> mlua::Result<Vec<String>> {
        Ok(spec
            .raw_get::<Option<Vec<String>>>(key)?
            .unwrap_or_default())
    };
    let share = |key: &str, default: f64| -> mlua::Result<f64> {
        Ok(spec.raw_get::<Option<f64>>(key)?.unwrap_or(default))
    };
    Ok(HeroPlan {
        unit,
        learn: list("learn")?,
        heal: spec.raw_get("heal")?,
        heal_below: share("heal_below", 0.6)?,
        shield: spec.raw_get("shield")?,
        shield_below: share("shield_below", 0.4)?,
        potions: list("potions")?,
        potion_below: share("potion_below", 0.35)?,
        fight: list("fight")?,
        cast_at: BTreeMap::new(),
    })
}
