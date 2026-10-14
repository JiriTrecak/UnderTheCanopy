//! Movement commands from players (the move/stop/hold/patrol/follow branches of the old
//! `Game.command`, `src/sim/game/game.ts`), with the old validation.

use super::{Order, can_issue, issue};
use crate::{
    action::{Action, Actor, CellPoint},
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
            | Action::Stop { .. }
            | Action::Hold { .. }
            | Action::Patrol { .. }
            | Action::Follow { .. }
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

/// Applies a movement command for `owner`. Errors are the old rejection messages.
pub fn apply(
    state: &mut State,
    spatial: &mut Spatial,
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
        | Action::Follow { actors, append, .. } => (actors, append.unwrap_or(false)),
        Action::Stop { actors } => (actors, false),
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
    let movable = |state: &State, id: EntityId| {
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
                if !movable(state, id) || Some(id) == target {
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
                applied.push(id);
            }
        }
        Action::Stop { .. } => {
            for &id in &eligible {
                if !movable(state, id) {
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
                            && movable(state, id)
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
                if !movable(state, id) {
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
