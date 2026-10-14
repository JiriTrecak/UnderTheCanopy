//! Unit orders (old `UnitOrders`, `src/sim/game/unitOrders.ts`, the order schema of
//! `state.ts`, `combatIntent.ts`, and the navigation half of `Combat.planUnits`):
//! issuing and queueing, advancing the queue, and turning move, patrol, follow and hold
//! orders into routes each tick.

pub mod command;
pub mod plan;

use crate::entity::{Entity, EntityId, Place, Unit};
use serde::{Deserialize, Serialize};

/// Most orders a unit queues behind its current one.
pub const MAX_QUEUED_ORDERS: usize = 16;

/// What a unit was told to do. Combat, work and ability orders join this as their
/// systems arrive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Order {
    /// Walk to a cell; an attack-move engages enemies met on the way (combat).
    #[serde(rename_all = "camelCase")]
    Move {
        destination: Place,
        attack_move: bool,
    },
    /// Stay put and only fight what comes within reach.
    Hold,
    /// Walk back and forth between `origin` (set on the first plan) and `destination`.
    Patrol {
        destination: Place,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        origin: Option<Place>,
    },
    /// Keep close to a friendly unit; escorts may follow units they do not own.
    Follow {
        target: EntityId,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        escort: bool,
    },
    /// Harvest a resource and its neighbours of the same kind (economy).
    Gather { target: EntityId },
    /// Build or finish an owned construction site (economy).
    Construct { target: EntityId },
}

/// A route is a disposable plan, never the unit's position (old `discardNavigation`).
pub fn discard_navigation(u: &mut Unit) {
    u.route.clear();
    u.goal = None;
    u.segment = None;
    u.detour = None;
}

/// Drops the engagement without touching another system's movement plan (old
/// `releaseCombat`; pursuit, attack and charge state arrive with combat).
pub fn release_combat(u: &mut Unit) {
    u.target = None;
}

/// Whether a new order would queue behind current work (old `UnitOrders.busy`): an
/// order, queued orders, or a job, cargo, workplace or delivery-before-move.
pub fn busy(e: &Entity) -> bool {
    e.unit.as_ref().is_some_and(|u| {
        u.order.is_some() || !u.order_queue.is_empty() || !u.work.is_idle()
    })
}

/// Whether the unit accepts an order now (old `canIssue`): the queue has room for an
/// appended one.
pub fn can_issue(e: &Entity, append: bool) -> bool {
    e.unit
        .as_ref()
        .is_some_and(|u| !append || u.order_queue.len() < MAX_QUEUED_ORDERS)
}

/// Cancels current activity: queue, engagement, detours and navigation (the movement part
/// of old `Economy.interrupt`; [`crate::economy::interrupt`] adds jobs and workplaces). A
/// move destination becomes the new order.
pub fn interrupt(e: &mut Entity, destination: Option<Place>) -> bool {
    let Some(u) = e.unit.as_mut() else {
        return false;
    };
    if u.contained.is_some() || u.release.is_some() {
        return false;
    }
    u.order_queue.clear();
    release_combat(u);
    u.detour = None;
    u.last_moved_tick = None;
    u.idle = None;
    u.order = destination.map(|destination| Order::Move {
        destination,
        attack_move: false,
    });
    discard_navigation(u);
    true
}

/// Gives the unit an order, or queues it behind current work when appending (old
/// `UnitOrders.issue`). Replacing work goes through the economy's interrupt, so a carrier
/// mid-trip delivers first and then follows a move.
pub fn issue(state: &mut crate::state::State, id: EntityId, order: Order, append: bool) -> bool {
    let tick = state.tick;
    let Some(e) = state.entities.get_mut(id) else {
        return false;
    };
    if !can_issue(e, append) {
        return false;
    }
    if append && busy(e) {
        e.unit.as_mut().expect("unit").order_queue.push(order);
        return true;
    }
    let destination = match &order {
        Order::Move { destination, .. } => Some(destination.clone()),
        _ => None,
    };
    crate::economy::interrupt(state, id, destination);
    let u = state
        .entities
        .get_mut(id)
        .and_then(|e| e.unit.as_mut())
        .expect("unit");
    if !matches!(order, Order::Move { .. }) {
        u.work.pending_move = None;
    }
    u.order = Some(order);
    u.target = None;
    u.retry_at = tick;
    true
}
