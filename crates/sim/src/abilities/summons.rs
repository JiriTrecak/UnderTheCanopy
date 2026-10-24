//! Summons (old `summons.ts`): groups of units around a point or on consumed corpses,
//! replacement and caps, lifetimes and links to their source.

use super::{
    Ctx,
    actor::{self, Actor},
    corpses,
    library::{self, AbilityId},
    state::{Summoned, UNTIL_DEATH},
};
use crate::{
    catalog::Def,
    entity::{EntityId, Milli},
    spatial::Spot,
    state::State,
};
use canopy_content::{
    abilities::Summon,
    placement::{InitialState, Owner, Placement, Point},
};

/// A created group (old summon result).
#[derive(Debug, Clone)]
pub struct Group {
    pub point: Milli,
    pub units: Vec<EntityId>,
}

/// Creates a unit at `at` that takes part from the next tick (old `context.create`).
pub fn spawn(
    ctx: &mut Ctx,
    state: &mut State,
    def: Def,
    owner: Owner,
    at: Spot,
    rotation: i32,
) -> EntityId {
    let catalog = ctx.catalog();
    let placement = Placement {
        activation: None,
        id: String::new(),
        map_knowledge: None,
        definition: catalog.id(def).clone(),
        position: Point {
            x: f64::from(at.x),
            y: f64::from(at.y),
            surface: at.deck.map(|d| ctx.spatial.deck_name(d)),
        },
        rotation: f64::from(rotation) / 1000.0,
        owner,
        initial_state: Some(InitialState::default()),
        appearance: None,
    };
    let id = crate::spawn::create(state, catalog, &placement).expect("summon definition");
    let tick = state.tick;
    let e = state.entities.get_mut(id).expect("created");
    e.placement = None;
    e.ready_tick = tick + 1;
    e.rotation = rotation;
    ctx.spatial.update_body(e);
    id
}

fn owned_summons(
    state: &State,
    owner: Owner,
    source: EntityId,
    ability: AbilityId,
) -> Vec<EntityId> {
    state
        .entities
        .iter()
        .filter(|e| {
            e.unit.is_some()
                && actor::alive(e)
                && e.owner == owner
                && e.links
                    .as_ref()
                    .and_then(|l| l.summoned.as_ref())
                    .is_some_and(|s| s.source == source && s.ability == ability)
        })
        .map(|e| e.id)
        .collect()
}

/// The groups of a summon operation at `point` (old `groupsAt` / `fromCorpses`).
#[allow(clippy::too_many_arguments)]
pub fn groups(
    ctx: &mut Ctx,
    state: &mut State,
    caster: &Actor,
    ability: AbilityId,
    rank: u8,
    cast: u32,
    op: &Summon,
    point: Milli,
) -> Vec<Group> {
    let spell = ctx.ability(ability);
    let centers: Vec<(Milli, Option<EntityId>)>;
    let limit: usize;
    match &op.corpses {
        Some(q) => {
            let visible = corpses::visible(ctx, state, caster);
            let chosen = corpses::select(
                &visible,
                library::value(spell, rank, &q.radius),
                &q.relations,
                q.filter.as_ref(),
                q.order,
                point,
            );
            centers = chosen.iter().map(|c| (c.position, Some(c.id))).collect();
            limit = library::value(spell, rank, &q.max_targets).max(0) as usize;
        }
        None => {
            centers = vec![(point, None)];
            limit = 1;
        }
    }
    create(ctx, state, caster, ability, rank, cast, op, &centers, limit)
}

#[allow(clippy::too_many_arguments)]
fn create(
    ctx: &mut Ctx,
    state: &mut State,
    caster: &Actor,
    ability: AbilityId,
    rank: u8,
    cast: u32,
    op: &Summon,
    centers: &[(Milli, Option<EntityId>)],
    limit: usize,
) -> Vec<Group> {
    let spell = ctx.ability(ability);
    let owner = caster.owner;
    let ends = op.ends_with_caster == Some(true);
    if ends {
        let parent_ok = state.entities.get(caster.id).is_some_and(|p| {
            actor::alive(p)
                && p.owner == owner
                && (owner != Owner::Nobody || actor::camp(p).map(str::to_string) == caster.camp)
        });
        if !parent_ok {
            return Vec::new();
        }
    }
    let Some(def) = ctx.catalog().find(library::reference(&op.definition, rank)) else {
        return Vec::new();
    };
    let amount = library::value(spell, rank, &op.amount).max(0) as usize;
    let max_active = op
        .max_active
        .as_ref()
        .map(|m| library::value(spell, rank, m).max(0) as usize);
    let duration = op
        .duration_ticks
        .as_ref()
        .map(|d| library::value(spell, rank, d).max(0) as u32);
    let existing = owned_summons(state, owner, caster.id, ability);
    let previous: Vec<EntityId> = if op.replace {
        existing.clone()
    } else {
        Vec::new()
    };
    let mut retained = if op.replace { 0 } else { existing.len() };
    let mut replaced = false;
    let rotation = state.entities.get(caster.id).map_or(0, |e| e.rotation);
    let body = ctx.spatial.profiles.get(def).body;
    let mut results: Vec<Group> = Vec::new();
    for &(center, corpse) in centers {
        if results.len() >= limit {
            break;
        }
        let tick = state.tick;
        if let Some(c) = corpse
            && !state
                .combat
                .corpses
                .iter()
                .any(|x| x.id == c && x.expires > tick)
        {
            continue;
        }
        let mut units = Vec::new();
        for i in 0..amount {
            if max_active.is_some_and(|m| retained >= m) {
                break;
            }
            let s = super::world_ops::supply(ctx, state, owner);
            let replaceable = if replaced {
                0
            } else {
                previous
                    .iter()
                    .filter(|&&id| {
                        state
                            .entities
                            .get(id)
                            .is_some_and(|e| e.owner == owner && e.hp.is_some_and(|hp| hp > 0))
                    })
                    .count() as u32
            };
            if (s.units + s.queued_units).saturating_sub(replaceable) >= s.unit_limit {
                break;
            }
            let angle = i as f64 * 2.0 * std::f64::consts::PI / amount as f64;
            let origin = Spot::new(
                crate::terrain::round_half_up(
                    f64::from(center.x) / 1000.0 + crate::math::cos(angle) * op.radius,
                ) as i32,
                crate::terrain::round_half_up(
                    f64::from(center.y) / 1000.0 + crate::math::sin(angle) * op.radius,
                ) as i32,
                None,
            );
            let Some(at) = ctx.spatial.nearest(
                origin,
                op.radius.floor() as i32,
                None,
                Some(&body),
                None,
                None,
            ) else {
                continue;
            };
            if !replaced {
                for &old in &previous {
                    if let Some(e) = state.entities.get_mut(old)
                        && e.hp.is_some_and(|hp| hp > 0)
                    {
                        e.hp = Some(0);
                        crate::combat::deaths::die(ctx, state, old);
                    }
                }
                replaced = true;
            }
            let id = spawn(ctx, state, def, owner, at, rotation);
            let now = state.tick;
            let e = state.entities.get_mut(id).expect("summon");
            if owner == Owner::Nobody
                && let (Some(camp), Some(u)) = (&caster.camp, e.unit.as_mut())
            {
                u.camp = Some(crate::camps::Member {
                    id: camp.clone(),
                    returning: false,
                    alarm: None,
                });
            }
            super::links(e).summoned = Some(Summoned {
                source: caster.id,
                ability,
                rank,
                cast,
                started: now,
                expires: duration.map_or(UNTIL_DEATH, |d| now.saturating_add(d)),
                split: None,
                link: ends.then(|| (owner, caster.camp.clone())),
                reanimated_from: None,
            });
            retained += 1;
            units.push(id);
        }
        if !units.is_empty() {
            if let Some(c) = corpse {
                corpses::consume(state, c);
            }
            results.push(Group {
                point: center,
                units,
            });
        }
    }
    results
}

/// Summon lifetimes and source links (old `SpellSummons.tick`): every dependent of a
/// doomed summon goes too.
pub fn tick(ctx: &mut Ctx, state: &mut State) {
    let tick = state.tick;
    let mut doomed: Vec<EntityId> = Vec::new();
    let mut children: Vec<(EntityId, EntityId)> = Vec::new();
    for e in state.entities.iter() {
        if e.unit.is_none() || !actor::alive(e) {
            continue;
        }
        let Some(s) = e.links.as_ref().and_then(|l| l.summoned.as_ref()) else {
            continue;
        };
        if s.split.is_none() && s.expires <= tick {
            doomed.push(e.id);
        }
        if let Some((owner, camp)) = &s.link {
            children.push((s.source, e.id));
            let source_ok = state.entities.get(s.source).is_some_and(|src| {
                actor::alive(src)
                    && src.owner == *owner
                    && (src.owner != Owner::Nobody || actor::camp(src).map(str::to_string) == *camp)
            });
            if !source_ok && !doomed.contains(&e.id) {
                doomed.push(e.id);
            }
        }
    }
    let mut i = 0;
    while i < doomed.len() {
        let parent = doomed[i];
        for &(source, child) in &children {
            if source == parent && !doomed.contains(&child) {
                doomed.push(child);
            }
        }
        i += 1;
    }
    doomed.sort_unstable();
    for id in doomed {
        if let Some(e) = state.entities.get_mut(id)
            && e.hp.is_some_and(|hp| hp > 0)
        {
            e.hp = Some(0);
            crate::combat::deaths::die(ctx, state, id);
        }
    }
}
