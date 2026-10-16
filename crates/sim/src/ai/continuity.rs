//! Keeping orders the AI already gave (old `ai/orderContinuity.ts`): a unit making
//! progress on the same intention is not reordered, so paths stay intact; a stalled one is
//! retried. Uses only saved memory and the unit's own observed order.

use super::{
    frame::{Frame, Known},
    geography::{Pt, dist},
    state::OrderMemo,
};
use crate::{action::Action, orders::Order};

/// Reinforcements within this many cells of the leader join the army.
pub const REINFORCEMENT_JOIN_RADIUS: f64 = 16.0;

/// A moving reinforcement already heading into the army's join area need not restart its
/// route for each small movement of the leader.
pub fn approaching_army(f: &Frame, e: &Known, leader: Pt) -> bool {
    let Some(Order::Move {
        destination,
        attack_move: true,
    }) = f.order(e)
    else {
        return false;
    };
    e.moving
        && f.geo.deck_id(destination.surface.as_deref()) == leader.deck
        && dist(f.geo.from_place(destination), leader) < REINFORCEMENT_JOIN_RADIUS
}

/// The command without its recipients: what a squad's members share. Keys are sorted at
/// every level: the text is saved state and must not follow serde_json's map order.
pub fn intent(action: &Action) -> String {
    let mut value = serde_json::to_value(action).expect("action serializes");
    if let Some(map) = value.as_object_mut() {
        map.remove("actors");
    }
    sorted(value).to_string()
}

fn sorted(value: serde_json::Value) -> serde_json::Value {
    use serde_json::Value;
    match value {
        Value::Object(map) => {
            let mut entries: Vec<_> = map.into_iter().collect();
            entries.sort_by(|a, b| a.0.cmp(&b.0));
            Value::Object(entries.into_iter().map(|(k, v)| (k, sorted(v))).collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sorted).collect()),
        other => other,
    }
}

/// The command as a whole, for rejection cooldowns.
pub fn canonical(action: &Action) -> String {
    serde_json::to_string(action).expect("action serializes")
}

/// Whether `e` is still carrying out the remembered order that `action` would repeat.
pub fn continues(
    f: &Frame,
    e: &Known,
    old: Option<&mut OrderMemo>,
    action: &Action,
    signature: &str,
    tick: u32,
    interval: u32,
) -> bool {
    let Some(old) = old else {
        return false;
    };
    if old.signature != signature {
        return false;
    }
    let order = f.order(e);
    match action {
        Action::Move { destination, .. } => {
            let deck = f.geo.deck_id(destination.surface.as_deref());
            let to = Pt {
                x: f64::from(destination.x),
                y: f64::from(destination.y),
                deck,
            };
            if dist(e.p, to) < 2.0 && e.p.deck == deck {
                return true;
            }
        }
        Action::Gather { target, .. } => {
            if matches!(order, Some(Order::Gather { target: t }) if t.0 == *target) {
                return true;
            }
        }
        Action::Attack { target, .. } => {
            if matches!(order, Some(Order::Attack { target: t, .. }) if t.0 == *target) {
                return true;
            }
        }
        Action::Pickup { target, .. }
            if powers::pickup_target(f, e) == Some(*target) && tick - old.tick < 400 =>
        {
            return true;
        }
        _ => {}
    }
    if e.moving && dist(f.geo.from_place(&old.point), e.p) > 1.0 {
        old.point = f.geo.place(e.p);
        old.tick = tick;
        return true;
    }
    tick - old.tick < interval.max(160)
}

use super::powers;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::CellPoint;

    fn walk(actors: Vec<u32>) -> Action {
        Action::Move {
            actors,
            destination: CellPoint {
                x: 10,
                y: 20,
                surface: None,
            },
            attack_move: Some(true),
            append: None,
        }
    }

    #[test]
    fn a_squad_shares_its_intention_whoever_carries_it_out() {
        assert_eq!(intent(&walk(vec![1, 2])), intent(&walk(vec![3])));
        assert_ne!(canonical(&walk(vec![1, 2])), canonical(&walk(vec![3])));
        assert!(!intent(&walk(vec![1])).contains("actors"));
    }
}
