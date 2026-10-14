//! Abilities and items for AI players (old `abilityActions` in `ai/tactics.ts` with the
//! scoring of `abilities/ai.ts`): what its units are doing with spells and items, hero
//! skill learning, and which spell to cast at what.
//!
//! Scoring is the ability runtime's (`abilities::ai`), which autocast and camp casters
//! share: the release program is predicted in order on copies of the units the player
//! observes. For the player AI every operation chooses its recipients from the units as
//! the earlier operations left them; the old scorer did that only for operations with a
//! query and chose everyone else from the units as they were when the cast began, so a
//! later operation was judged against a world the earlier ones had already changed
//! (docs/decisions.md D26).

use super::{
    Beat,
    frame::{Frame, Known},
    geography::{Pt, dist, round},
};
use crate::{
    abilities::{
        self, Caster,
        actor::{self, Actor, Side},
        cast, library,
    },
    action::{Action, CastTarget, PlainPoint},
    catalog::Def,
    entity::{Entity, EntityId, Milli},
    orders::Order,
};
use canopy_content::abilities::{AiIntent, Control, TargetingKind};

/// A hero's carried items as its player knows them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Gear {
    /// Item definition per slot.
    pub slots: Vec<Option<Def>>,
    /// First tick each slot's active use is ready again.
    pub ready: Vec<u32>,
    pub mana: u32,
    /// Ticks at which its abilities come off cooldown.
    pub cooldowns: Vec<u32>,
}

/// Whether an own unit is casting: a cast whose preparation has begun.
pub fn casting(f: &Frame, e: &Entity) -> bool {
    e.caster
        .as_ref()
        .and_then(|c| c.pending.as_ref())
        .is_some_and(|p| p.start_tick <= f.tick)
}

/// Whether an own unit is stunned.
pub fn stunned(f: &Frame, e: &Entity) -> bool {
    abilities::statuses::stunned(f.catalog(), e)
}

/// An own hero's carried items, when it can carry any.
pub fn gear(f: &Frame, hero: &Known) -> Option<Gear> {
    let e = f.own_entity(hero)?;
    let equipment = e.equipment.as_ref().filter(|q| !q.slots.is_empty())?;
    let caster = e.caster.as_deref();
    Some(Gear {
        slots: equipment.slots.clone(),
        ready: (0..equipment.slots.len())
            .map(|i| {
                equipment
                    .runtime
                    .get(i)
                    .and_then(Option::as_ref)
                    .map_or(0, |r| r.ready_tick)
            })
            .collect(),
        mana: caster.map_or(0, |c| c.mana),
        cooldowns: caster.map_or_else(Vec::new, |c| c.cooldowns.values().copied().collect()),
    })
}

/// The item an own unit's current order picks up.
pub fn pickup_target(f: &Frame, e: &Known) -> Option<u32> {
    match f.order(e)? {
        Order::Pickup { target } => Some(target.0),
        _ => None,
    }
}

/// A unit as the scorer sees it, from what its player knows: an enemy's mana is private,
/// so it counts as full (as the old AI read the missing value).
fn observed(f: &Frame, k: &Known) -> Option<Actor> {
    let e = f.input.state.entities.get(EntityId(k.id))?;
    let mut a = actor::of(&f.input.spells.senses, f.input.state, e);
    if k.owner != f.me {
        a.mana = a.max_mana;
    }
    Some(a)
}

/// Learning and casting for every own caster, in id order, before anything else claims
/// the units (old `abilityActions`).
pub fn ability_actions(b: &mut Beat) {
    let f = b.f;
    let catalog = f.catalog();
    let spells = f.input.spells;
    // What the player observes: its own living units and buildings and the hostiles in
    // sight.
    let mut seen: Vec<&Known> = f
        .own
        .iter()
        .chain(&f.hostiles)
        .map(|&i| f.at(i))
        .filter(|k| !k.remembered && k.hp.is_some_and(|hp| hp > 0))
        .collect();
    seen.sort_by_key(|k| k.id);
    seen.dedup_by_key(|k| k.id);
    let mut actors: Option<Vec<Actor>> = None;
    for &i in &f.own {
        let k = f.at(i);
        let Some(e) = f.own_entity(k) else {
            continue;
        };
        let (Some(caster_state), Some(policy)) =
            (e.caster.as_deref(), catalog.spells.caster(e.definition))
        else {
            continue;
        };
        if caster_state.pending.is_some() || caster_state.weapon_order.is_some() || stunned(f, e) {
            continue;
        }
        let level = f.stats(k).map_or(1, |s| s.level);
        let rank_of = |index: usize| caster_state.ranks.get(index).copied().unwrap_or(0);
        // Learn through the ordinary command: newly unlocked ranks first, then breadth,
        // then the race's declared preference; no ability names in the planner.
        let spent: u32 = policy
            .bindings
            .iter()
            .enumerate()
            .map(|(index, d)| u32::from(rank_of(index).saturating_sub(d.initial_rank)))
            .sum();
        let preference = |ability: library::AbilityId| {
            let id = &spells.ability(ability).id;
            f.rules
                .skill_preference
                .iter()
                .position(|p| p == id)
                .unwrap_or(usize::MAX)
        };
        let learn = policy
            .bindings
            .iter()
            .enumerate()
            .filter_map(|(index, d)| {
                let required = *d.learning.as_ref()?.get(usize::from(rank_of(index)))?;
                (d.allows(Control::Ai) && required <= level).then_some((index, d, required))
            })
            .min_by(|a, b| {
                b.2.cmp(&a.2)
                    .then(rank_of(a.0).cmp(&rank_of(b.0)))
                    .then(preference(a.1.ability).cmp(&preference(b.1.ability)))
                    .then(a.0.cmp(&b.0))
            });
        if spent < level
            && let Some((_, d, _)) = learn
            && b.emit(
                Action::LearnAbility {
                    actor: k.id,
                    ability: d.id.clone(),
                },
                "Learn an available hero ability",
            )
        {
            continue;
        }
        let actors =
            actors.get_or_insert_with(|| seen.iter().filter_map(|k| observed(f, k)).collect());
        cast_one(b, k, caster_state, policy, actors);
    }
}

/// Casts the first useful AI-controlled spell of one caster; true when one was issued.
fn cast_one(
    b: &mut Beat,
    k: &Known,
    caster_state: &Caster,
    policy: &library::CasterPolicy,
    observed: &[Actor],
) -> bool {
    let f = b.f;
    let spells = f.input.spells;
    let state = f.input.state;
    let Some(me) = observed.iter().find(|a| a.id.0 == k.id) else {
        return false;
    };
    let side = |t: &Actor| {
        if f.get(t.id.0).is_some_and(|k| k.hostile == Some(true)) {
            Side::Enemy
        } else {
            Side::Ally
        }
    };
    let here = Pt::of(me.position, None);
    for (index, d) in policy.bindings.iter().enumerate() {
        let Some(ai) = d.ai.as_ref().filter(|_| d.allows(Control::Ai)) else {
            continue;
        };
        let spell = spells.ability(d.ability);
        if library::passive(spell) || spell.autocast.is_some() {
            continue;
        }
        let rank = caster_state.ranks.get(index).copied().unwrap_or(0);
        let cost = library::value(spell, rank, &spell.cast.cost.amount).max(0) as u32;
        if rank == 0
            || caster_state
                .cooldowns
                .get(&d.ability)
                .is_some_and(|&t| t > f.tick)
            || caster_state.mana < cost
        {
            continue;
        }
        // An active toggle stays on.
        if spell.persistent.as_ref().is_some_and(|p| p.toggle)
            && state
                .abilities
                .instances
                .iter()
                .any(|i| i.source.0 == k.id && i.ability == d.ability)
        {
            continue;
        }
        let range = library::value(spell, rank, &spell.targeting.range) as f64;
        if let Some(intent) = ai.intent.filter(|&i| i != AiIntent::Utility) {
            let size = f.geo.size;
            // An escape lands on known open ground.
            let usable = |x: i32, y: i32| {
                intent != AiIntent::Escape || {
                    let cell = y as u32 * size + x as u32;
                    f.land(cell) && !f.blocked(cell)
                }
            };
            let aims = abilities::ai::player_strategic(
                spells, state, intent, d.ability, rank, me, observed, &usable,
            );
            if let Some(&aim) = aims.first()
                && b.emit(
                    cast_action(k.id, &d.id, aim),
                    "Use a declared tactical ability",
                )
            {
                return true;
            }
            continue;
        }
        let corpses = abilities::corpses::visible(spells, state, me);
        let score = |aim: &abilities::program::Aim| {
            abilities::ai::player_score(
                spells,
                state,
                me,
                d.ability,
                rank,
                aim,
                observed,
                ai.preference,
                &corpses,
            )
        };
        let t = &spell.targeting;
        let pool: Vec<&Actor> = if t.kind == TargetingKind::Caster {
            vec![me]
        } else {
            observed.iter().collect()
        };
        let mut candidates: Vec<(u32, Milli, f64)> = pool
            .into_iter()
            .filter(|e| {
                let s = side(e);
                (t.kind == TargetingKind::Point
                    || (cast::matches_target(spell, e, me, s) && cast::accepts(spell, e, s)))
                    && (e.unit || t.include_buildings == Some(true))
                    && (e.id != me.id || t.allow_self || t.kind == TargetingKind::Caster)
                    && dist(Pt::of(e.position, None), here) <= range
                    && (t.kind == TargetingKind::Caster || s.is(&t.relations))
            })
            .map(|e| {
                let (aim, at) = match t.kind {
                    TargetingKind::Point => {
                        let p = Milli {
                            x: round(e.x()) as i32 * 1000,
                            y: round(e.y()) as i32 * 1000,
                        };
                        (abilities::program::Aim::Point(p), p)
                    }
                    _ => (
                        abilities::program::Aim::Unit(Box::new(e.clone())),
                        e.position,
                    ),
                };
                (e.id.0, at, score(&aim))
            })
            .filter(|c| c.2 > 0.0)
            .collect();
        if t.kind == TargetingKind::Point {
            for (id, point) in abilities::ai::corpse_points(spell, &corpses) {
                let p = Pt::of(point, None);
                if dist(p, here) > range || !f.geo.inside(p) || (t.visible && !f.visible(p)) {
                    continue;
                }
                let value = score(&abilities::program::Aim::Point(point));
                if value > 0.0 {
                    candidates.push((id.0, point, value));
                }
            }
        }
        candidates.sort_by(|a, b| b.2.total_cmp(&a.2).then(a.0.cmp(&b.0)));
        let Some(&(id, at, _)) = candidates.first() else {
            continue;
        };
        let (aim, reason) = if t.kind == TargetingKind::Point {
            (
                cast::Aim::Point(Milli {
                    x: round(f64::from(at.x) / 1000.0) as i32 * 1000,
                    y: round(f64::from(at.y) / 1000.0) as i32 * 1000,
                }),
                "Cast a declared ability at a visible useful area",
            )
        } else {
            (
                cast::Aim::Unit(EntityId(id)),
                "Cast a declared ability on a visible useful target",
            )
        };
        if b.emit(cast_action(k.id, &d.id, aim), reason) {
            return true;
        }
    }
    false
}

fn cast_action(actor: u32, binding: &str, aim: cast::Aim) -> Action {
    Action::CastAbility {
        actor,
        binding: binding.to_string(),
        target: match aim {
            cast::Aim::Unit(id) => CastTarget::Unit { entity: id.0 },
            cast::Aim::Point(p) => CastTarget::Point {
                position: PlainPoint {
                    x: (p.x / 1000).max(0) as u32,
                    y: (p.y / 1000).max(0) as u32,
                },
            },
        },
    }
}
