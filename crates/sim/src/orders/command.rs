//! Unit commands from players (the move/attack/stop/hold/patrol/follow and
//! garrison/unload branches of the old `Game.command`, `src/sim/game/game.ts`), with the
//! old validation.

use super::{Order, can_issue, issue};
use crate::{
    action::{Action, Actor, CellPoint},
    combat::Senses,
    entity::{EntityId, Place},
    movement::formation::{Member, formation_destinations},
    spatial::{Body, Spatial, Spot},
    state::State,
};
use canopy_content::placement::Owner;
use std::collections::BTreeMap;

/// Whether `action` is one of the commands this module applies.
pub fn handles(action: &Action) -> bool {
    matches!(
        action,
        Action::Move { .. }
            | Action::Attack { .. }
            | Action::Stop { .. }
            | Action::Hold { .. }
            | Action::Patrol { .. }
            | Action::Follow { .. }
            | Action::Garrison { .. }
            | Action::Unload { .. }
    )
}

fn alive(e: &crate::entity::Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0)
}

/// The spot an action point names, if it is on the map (and on a known deck).
pub fn spot(spatial: &Spatial, point: &CellPoint) -> Result<Spot, String> {
    let deck = match &point.surface {
        None => None,
        Some(name) => Some(
            spatial
                .deck_id(name)
                .ok_or("Destination is not on a declared walk surface")?,
        ),
    };
    let spot = Spot::new(point.x as i32, point.y as i32, deck);
    if !spatial.valid_spot(spot) {
        return Err("Destination is not on a declared walk surface".into());
    }
    Ok(spot)
}

fn place(point: &CellPoint) -> Place {
    Place {
        x: point.x as i32,
        y: point.y as i32,
        surface: point.surface.clone(),
    }
}

/// Applies a unit command for `owner`. Errors are the old rejection messages.
pub fn apply(
    state: &mut State,
    spatial: &mut Spatial,
    senses: &Senses,
    owner: Owner,
    action: &Action,
) -> Result<Vec<EntityId>, String> {
    if state.outcome.is_some() {
        return Err("Match has ended".into());
    }
    let destination = match action {
        Action::Move { destination, .. } | Action::Patrol { destination, .. } => {
            Some(spot(spatial, destination)?)
        }
        _ => None,
    };
    let (actors, append): (&[Actor], bool) = match action {
        Action::Move { actors, append, .. }
        | Action::Hold { actors, append }
        | Action::Patrol { actors, append, .. }
        | Action::Follow { actors, append, .. }
        | Action::Attack { actors, append, .. }
        | Action::Garrison { actors, append, .. } => (actors, append.unwrap_or(false)),
        Action::Stop { actors } => (actors, false),
        Action::Unload { actor } => (std::slice::from_ref(actor), false),
        _ => return Err("Invalid request".into()),
    };
    let mut ids: Vec<EntityId> = actors.iter().map(|&a| EntityId(a)).collect();
    ids.sort_unstable();
    if ids
        .iter()
        .any(|&id| state.entities.get(id).is_some_and(|e| e.owner != owner))
    {
        return Err("Not your actors".into());
    }
    let eligible: Vec<EntityId> = ids
        .into_iter()
        .filter(|&id| {
            state.entities.get(id).is_some_and(|e| {
                alive(e)
                    && spatial.profiles.get(e.definition).player_control
                    && e.unit
                        .as_ref()
                        .is_none_or(|u| u.contained.is_none() && u.release.is_none())
            })
        })
        .collect();
    if eligible.is_empty() {
        return Err("No eligible controlled actors".into());
    }
    if matches!(action, Action::Garrison { .. } | Action::Unload { .. }) {
        let seen = |state: &State, e: &crate::entity::Entity| senses.sight(state).seen(owner, e);
        return crate::garrisons::command(state, senses.catalog, spatial, &eligible, action, &seen);
    }
    let movable = |state: &State, spatial: &Spatial, id: EntityId| {
        state.entities.get(id).is_some_and(|e| {
            e.unit.is_some() && spatial.profiles.get(e.definition).motion.is_some()
        })
    };
    let mut applied = Vec::new();
    match action {
        Action::Hold { .. } | Action::Patrol { .. } | Action::Follow { .. } => {
            let target = match action {
                Action::Follow { target, .. } => {
                    let target = EntityId(*target);
                    let valid = state.entities.get(target).is_some_and(|t| {
                        alive(t)
                            && t.owner == owner
                            && t.unit
                                .as_ref()
                                .is_some_and(|u| u.contained.is_none() && u.release.is_none())
                    });
                    if !valid {
                        return Err("Follow requires a visible friendly unit".into());
                    }
                    Some(target)
                }
                _ => None,
            };
            for &id in &eligible {
                if !movable(state, spatial, id) || Some(id) == target {
                    continue;
                }
                if !can_issue(state.entities.get(id).expect("eligible"), append) {
                    continue;
                }
                let order = match action {
                    Action::Hold { .. } => Order::Hold,
                    Action::Follow { .. } => Order::Follow {
                        target: target.expect("follow target"),
                        escort: false,
                    },
                    Action::Patrol { destination, .. } => Order::Patrol {
                        destination: place(destination),
                        origin: None,
                    },
                    _ => unreachable!(),
                };
                issue(state, id, order, append);
                // A unit ordered out of a lookout steps outside at once.
                crate::garrisons::settle(spatial, state.entities.get_mut(id).expect("eligible"));
                applied.push(id);
            }
        }
        Action::Attack { target, force, .. } => {
            let force = force.unwrap_or(false);
            let target = EntityId(*target);
            let valid = state.entities.get(target).is_some_and(|t| {
                t.hp.is_some_and(|hp| hp > 0)
                    && senses.sight(state).seen(owner, t)
                    && t.unit.as_ref().is_none_or(|u| {
                        u.contained.is_none() && u.release.is_none() && u.garrison.is_none()
                    })
            });
            if !valid {
                return Err("Target is not visible and damageable".into());
            }
            for &id in &eligible {
                if !movable(state, spatial, id) {
                    continue;
                }
                let (e, t) = (
                    state.entities.get(id).expect("eligible"),
                    state.entities.get(target).expect("target"),
                );
                if !can_issue(e, append)
                    || !senses.combat.reaches(e.definition, t.definition)
                    || id == target
                    || (!force && !senses.hostile(e, t))
                {
                    continue;
                }
                issue(state, id, Order::Attack { target, force }, append);
                applied.push(id);
            }
        }
        Action::Stop { .. } => {
            for &id in &eligible {
                if !movable(state, spatial, id) {
                    continue;
                }
                crate::economy::interrupt(state, id, None);
                applied.push(id);
            }
        }
        Action::Move { attack_move, .. } => {
            let attack_move = attack_move.unwrap_or(false);
            let destination = destination.expect("move destination");
            let mut slots: BTreeMap<EntityId, Spot> = BTreeMap::new();
            for air in [false, true] {
                let group: Vec<EntityId> = eligible
                    .iter()
                    .copied()
                    .filter(|&id| {
                        let e = state.entities.get(id).expect("eligible");
                        let profile = spatial.profiles.get(e.definition);
                        profile.body.air == air
                            && movable(state, spatial, id)
                            && can_issue(e, append)
                            && (!attack_move || profile.combat)
                    })
                    .collect();
                if group.is_empty() {
                    continue;
                }
                let mut body = Body {
                    radius: 0.0,
                    height: 0.0,
                    spacing: 0.0,
                    air,
                };
                let members: Vec<Member> = group
                    .iter()
                    .map(|&id| {
                        let e = state.entities.get(id).expect("eligible");
                        let b = spatial.profiles.get(e.definition).body;
                        body.radius = body.radius.max(b.radius);
                        body.height = body.height.max(b.height);
                        body.spacing = body.spacing.max(b.spacing);
                        Member {
                            id,
                            x: e.position.x,
                            y: e.position.y,
                            deck: spatial.deck_of(e),
                        }
                    })
                    .collect();
                let target = if air {
                    Spot::new(destination.x, destination.y, None)
                } else {
                    destination
                };
                let spatial_ref: &Spatial = spatial;
                let found = formation_destinations(
                    &members,
                    target,
                    spatial_ref.size(),
                    |p| spatial_ref.unit_walkable(p, &body),
                    |a, b| spatial_ref.clear_segment(a, b.fixed(), None, &body),
                    body.spacing,
                );
                slots.extend(found);
            }
            for &id in &eligible {
                if !movable(state, spatial, id) {
                    continue;
                }
                let e = state.entities.get(id).expect("eligible");
                if !can_issue(e, append)
                    || (attack_move && !spatial.profiles.get(e.definition).combat)
                {
                    continue;
                }
                let Some(slot) = slots.get(&id) else {
                    continue;
                };
                let destination = Place {
                    x: slot.x,
                    y: slot.y,
                    surface: slot.deck.map(|d| spatial.deck_name(d)),
                };
                issue(
                    state,
                    id,
                    Order::Move {
                        destination,
                        attack_move,
                    },
                    append,
                );
                crate::garrisons::settle(spatial, state.entities.get_mut(id).expect("eligible"));
                applied.push(id);
            }
        }
        _ => unreachable!(),
    }
    if applied.is_empty() {
        return Err("No actors support that order".into());
    }
    Ok(applied)
}
