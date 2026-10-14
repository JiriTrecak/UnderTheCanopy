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
    /// Engage one entity until it dies or is lost; `force` also attacks friends and
    /// passive neutrals (combat).
    Attack { target: EntityId, force: bool },
    /// Walk into a friendly lookout and hold there (garrisons).
    Garrison { target: EntityId },
}

impl Order {
    /// Orders another system carries out on its own (old `planUnits`: work, pickup and
    /// garrison orders): combat stands down while one runs. The economy and item steps
    /// add their orders here.
    pub fn stands_down(&self) -> bool {
        matches!(self, Self::Garrison { .. })
    }
}

/// A route is a disposable plan, never the unit's position (old `discardNavigation`).
pub fn discard_navigation(u: &mut Unit) {
    u.route.clear();
    u.goal = None;
    u.segment = None;
    u.detour = None;
}

/// Drops the engagement without touching another system's movement plan (old
/// `releaseCombat`): target, pursuit and weapon swing end; a spent charge keeps its
/// cooldown.
pub fn release_combat(u: &mut Unit) {
    u.target = None;
    u.combat.pursuit = None;
    u.combat.attack = None;
    if let Some(charge) = &mut u.combat.charge {
        charge.target = None;
    }
}

/// Suspends the engagement but keeps orders (old `suspendCombat`): a unit that plans its
/// own navigation for the fight drops that route and replans at once.
pub fn suspend_combat(u: &mut Unit, tick: u32) {
    let engaged = u.target.is_some()
        || u.combat.pursuit.is_some()
        || u.combat.attack.is_some()
        || u.combat.charge.as_ref().is_some_and(|c| c.target.is_some())
        || matches!(u.order, Some(Order::Attack { .. }));
    let returning = u.camp.as_ref().is_some_and(|c| c.returning);
    let owns_navigation = !returning
        && match &u.order {
            None | Some(Order::Attack { .. } | Order::Hold | Order::Patrol { .. }) => true,
            Some(Order::Move { attack_move, .. }) => *attack_move,
            Some(_) => false,
        };
    if engaged && owns_navigation {
        discard_navigation(u);
        u.retry_at = tick;
    }
    release_combat(u);
}

/// Completes an explicit attack order; queued orders, attack-moves and patrols go on
/// (old `finishCombat`).
pub fn finish_combat(u: &mut Unit, tick: u32) {
    suspend_combat(u, tick);
    if matches!(u.order, Some(Order::Attack { .. })) {
        u.order = None;
    }
}

/// Whether a new order would queue behind current work (old `UnitOrders.busy`).
pub fn busy(e: &Entity) -> bool {
    e.unit
        .as_ref()
        .is_some_and(|u| u.order.is_some() || !u.order_queue.is_empty())
}

/// Whether the unit accepts an order now (old `canIssue`): the queue has room for an
/// appended one.
pub fn can_issue(e: &Entity, append: bool) -> bool {
    e.unit
        .as_ref()
        .is_some_and(|u| !append || u.order_queue.len() < MAX_QUEUED_ORDERS)
}

/// Cancels current activity: queue, engagement, detours and navigation (the movement part
/// of old `Economy.interrupt`). A move destination becomes the new order.
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
/// `UnitOrders.issue`).
pub fn issue(e: &mut Entity, order: Order, append: bool, tick: u32) -> bool {
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
    let u = e.unit.as_mut().expect("unit");
    // A lookout keeps its archer only for holding and attacking.
    let leaves = u.garrison.is_some() && !matches!(order, Order::Hold | Order::Attack { .. });
    // Reaffirming the current target keeps a charge already under way.
    let charge = match &order {
        Order::Attack { target, .. }
            if u.combat.charge.as_ref().and_then(|c| c.target) == Some(*target) =>
        {
            Some(*target)
        }
        _ => None,
    };
    interrupt(e, destination);
    let u = e.unit.as_mut().expect("unit");
    if leaves {
        crate::garrisons::leave(u);
    }
    u.target = match &order {
        Order::Attack { target, .. } => Some(*target),
        _ => None,
    };
    if charge.is_some()
        && u.target == charge
        && let Some(c) = &mut u.combat.charge
    {
        c.target = charge;
    }
    u.order = Some(order);
    u.retry_at = tick;
    true
}
