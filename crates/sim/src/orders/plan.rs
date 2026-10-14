//! Turning orders into routes each tick: the navigation half of the old order planning
//! pass (`Combat.planUnits`, `moveOrder`, the patrol, follow and hold branches in
//! `src/sim/game/combat.ts`) and the queue advance (`UnitOrders.advance`).
//!
//! The per-unit planning loop itself lives in [`crate::combat::plan`], which interleaves
//! these with target acquisition and attack approach exactly as the old pass did.

use super::{Order, discard_navigation, release_combat};
use crate::{
    action::{Action, CellPoint},
    combat::Senses,
    entity::{EntityId, Place},
    movement::Motion,
    spatial::Spot,
    state::{FactKind, State},
};

fn at(e: &crate::entity::Entity, spot: Spot, deck: Option<crate::terrain::DeckId>) -> bool {
    deck == spot.deck && e.position.x == spot.x * 1000 && e.position.y == spot.y * 1000
}

/// Step 4 (old `UnitOrders.advance`): idle units start their next queued order, replayed
/// through the command rules; a rejected one is dropped with its error fact.
pub fn advance(motion: &mut Motion, state: &mut State, senses: &Senses) {
    let _span = tracing::trace_span!("orders advance").entered();
    let tick = state.tick;
    let units: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| {
            e.hp.is_none_or(|hp| hp > 0) && e.ready_tick <= tick && crate::spatial::present(e)
        })
        .map(|e| e.id)
        .collect();
    for id in units {
        while let Some(e) = state.entities.get_mut(id) {
            let u = e.unit.as_mut().expect("unit");
            if u.order.is_some() || u.order_queue.is_empty() {
                break;
            }
            let order = u.order_queue.remove(0);
            // Activation replaces orders; keep the already authorised tail.
            let remaining = std::mem::take(&mut u.order_queue);
            let owner = e.owner;
            let actors = vec![id.0];
            let point = |p: &Place| CellPoint {
                x: p.x.max(0) as u32,
                y: p.y.max(0) as u32,
                surface: p.surface.clone(),
            };
            let action = match &order {
                Order::Move {
                    destination,
                    attack_move,
                } => Action::Move {
                    actors,
                    destination: point(destination),
                    attack_move: Some(*attack_move),
                    append: None,
                },
                Order::Hold => Action::Hold {
                    actors,
                    append: None,
                },
                Order::Patrol { destination, .. } => Action::Patrol {
                    actors,
                    destination: point(destination),
                    append: None,
                },
                Order::Follow { target, .. } => Action::Follow {
                    actors,
                    target: target.0,
                    append: None,
                },
                Order::Attack { target, force } => Action::Attack {
                    actors,
                    target: target.0,
                    force: Some(*force),
                    append: None,
                },
                Order::Garrison { target } => Action::Garrison {
                    actors,
                    target: target.0,
                    append: None,
                },
            };
            let accepted =
                super::command::apply(state, &mut motion.spatial, senses, owner, &action);
            if let Err(reason) = &accepted {
                state.fact(owner, FactKind::Error, reason.clone());
            }
            if let Some(u) = state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) {
                u.order_queue = remaining;
            }
            if accepted.is_ok() {
                break;
            }
        }
    }
}

/// Patrols: walks to `destination`; on arrival the legs swap (old patrol branch of
/// `planUnits`). `origin` was set when the patrol was first planned.
pub(crate) fn patrol(
    spatial: &mut crate::spatial::Spatial,
    state: &mut State,
    senses: &Senses,
    id: EntityId,
    destination: Place,
    origin: Option<Place>,
) {
    let tick = state.tick;
    let e = state.entities.get(id).expect("unit");
    let (cx, cy) = e.position.cell();
    let origin = origin.unwrap_or(Place::new(cx, cy));
    if move_order(spatial, state, senses, id, &destination) {
        let u = state
            .entities
            .get_mut(id)
            .expect("unit")
            .unit
            .as_mut()
            .expect("unit");
        u.order = Some(Order::Patrol {
            destination: origin,
            origin: Some(destination),
        });
        u.retry_at = tick + 1;
    }
}

/// Walks towards `destination`; returns whether the unit has arrived (old `moveOrder`).
/// An unreachable destination ends the order with an error fact.
pub(crate) fn move_order(
    spatial: &mut crate::spatial::Spatial,
    state: &mut State,
    senses: &Senses,
    id: EntityId,
    destination: &Place,
) -> bool {
    let tick = state.tick;
    let e = state.entities.get(id).expect("unit");
    let u = e.unit.as_ref().expect("unit");
    let deck = spatial.deck_of(e);
    let target = Spot::new(
        destination.x,
        destination.y,
        destination
            .surface
            .as_deref()
            .and_then(|s| spatial.deck_id(s)),
    );
    let arrived = u.route.is_empty()
        && (at(e, target, deck) || u.goal.is_some_and(|g| at(e, spatial.spot(g), deck)));
    if arrived {
        let u = state
            .entities
            .get_mut(id)
            .expect("unit")
            .unit
            .as_mut()
            .expect("unit");
        u.order = None;
        discard_navigation(u);
        return true;
    }
    if u.route.is_empty() && u.retry_at <= tick {
        let known = |b: &crate::spatial::BodyRecord| {
            state
                .entities
                .get(b.id)
                .is_some_and(|other| senses.perceives(state, e, other))
        };
        let goal = spatial.nearest(target, 8, Some(id), None, None, Some(&known));
        let owner = e.owner;
        if let Some(goal) = goal {
            let e = state.entities.get_mut(id).expect("unit");
            let routed = spatial.route_default(e, goal);
            if !routed {
                let body = spatial.body_of(e);
                let reachable = match (spatial.entity_node(e), spatial.node(goal)) {
                    (Some(from), Some(to)) => spatial
                        .find_path(from, to, None, i64::MAX, &body, None)
                        .is_some(),
                    _ => false,
                };
                if !reachable {
                    let u = e.unit.as_mut().expect("unit");
                    u.order = None;
                    discard_navigation(u);
                    spatial.update_body(e);
                    state.fact(owner, FactKind::Error, "Move destination is unreachable");
                }
            }
        }
        if let Some(u) = state.entities.get_mut(id).and_then(|e| e.unit.as_mut()) {
            u.retry_at = tick + 20;
        }
    }
    false
}

/// Keeps close to a friendly unit (old follow branch of `planUnits`).
pub(crate) fn follow(
    spatial: &mut crate::spatial::Spatial,
    state: &mut State,
    id: EntityId,
    target: EntityId,
    escort: bool,
) {
    let tick = state.tick;
    let e = state.entities.get(id).expect("unit");
    let leader = state.entities.get(target);
    let valid = leader.is_some_and(|l| {
        l.hp.is_none_or(|hp| hp > 0)
            && l.unit.as_ref().is_none_or(|u| u.contained.is_none())
            && (escort || l.owner == e.owner)
    });
    let Some(leader) = leader.filter(|_| valid) else {
        let e = state.entities.get_mut(id).expect("unit");
        let u = e.unit.as_mut().expect("unit");
        release_combat(u);
        u.order = None;
        discard_navigation(u);
        spatial.update_body(e);
        return;
    };
    let close = spatial.body_range(e, leader) <= 4.0;
    let (lx, ly) = leader.position.cell();
    let origin = Spot::new(lx, ly, spatial.deck_of(leader));
    let retry = e.unit.as_ref().expect("unit").retry_at <= tick;
    let e = state.entities.get_mut(id).expect("unit");
    let u = e.unit.as_mut().expect("unit");
    release_combat(u);
    if close {
        discard_navigation(u);
        spatial.update_body(e);
    } else if retry {
        if let Some(goal) = spatial.nearest(origin, 8, Some(id), None, None, None) {
            spatial.route_default(e, goal);
        }
        e.unit.as_mut().expect("unit").retry_at = tick + 12;
    }
}
