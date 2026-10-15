//! One player's outbox and confirmations (old `src/net/lockstep.ts`).
//!
//! Commands wait in the outbox; [`Outbox::confirm`] promises the server that this player
//! sends nothing more for beats up to `through` and ships the waiting commands as one
//! bundle. A beat is committed once every connected player has confirmed it.

use crate::wire::{Bundle, ClientMsg, MAX_BUNDLE_ACTIONS};
use canopy_sim::Action;

#[derive(Debug, Clone, Default)]
pub struct Outbox {
    pending: Vec<Action>,
    sent_through: u32,
}

impl Outbox {
    /// Queues a command. `noop` is never sent.
    pub fn send(&mut self, action: Action) {
        if action != Action::Noop {
            self.pending.push(action);
        }
    }

    /// Confirms beats up to `through`, carrying up to 64 waiting commands at `bundle_tick`
    /// (normally `through + 1`). A bundle for a beat already confirmed moves to the first
    /// unconfirmed one, and `through` then rises to cover it: the server drops bundles at or
    /// below what it has committed and whole turns whose `through` does not rise. Returns
    /// the `turn` to send, or nothing when `through` did not rise and nothing waits.
    pub fn confirm(&mut self, through: u32, bundle_tick: u32) -> Option<ClientMsg> {
        let count = self.pending.len().min(MAX_BUNDLE_ACTIONS);
        let actions: Vec<Action> = self.pending.drain(..count).collect();
        let mut at = bundle_tick;
        if !actions.is_empty() && at <= self.sent_through {
            at = self.sent_through + 1;
        }
        let send_through = if actions.is_empty() {
            through
        } else {
            through.max(at)
        };
        if send_through <= self.sent_through && actions.is_empty() {
            return None;
        }
        self.sent_through = self.sent_through.max(send_through);
        let bundles = if actions.is_empty() {
            Vec::new()
        } else {
            vec![Bundle { tick: at, actions }]
        };
        Some(ClientMsg::Turn {
            through: self.sent_through,
            bundles,
        })
    }

    /// Continues from beat `beat` (a loaded save): it counts as confirmed.
    pub fn resume(&mut self, beat: u32) {
        self.sent_through = self.sent_through.max(beat);
    }

    /// The highest beat confirmed so far.
    pub fn sent(&self) -> u32 {
        self.sent_through
    }

    /// Commands not yet sent.
    pub fn waiting(&self) -> &[Action] {
        &self.pending
    }

    /// Drops waiting commands (a desync: the old client stops sending).
    pub fn clear(&mut self) {
        self.pending.clear();
    }
}

/// The old runtime's `pulseConfirm` for a networked player: keep confirmations `delay`
/// beats ahead of the simulation clock (at least one), so the server can commit in real
/// time, carrying any waiting commands. `clock` is the last simulated beat (the old
/// `tickIndex`; `World::tick()` in this simulation).
pub fn pulse(outbox: &mut Outbox, clock: u32, delay: u32) -> Option<ClientMsg> {
    let through = clock + delay.max(1);
    if outbox.sent() <= through {
        outbox.confirm(through, through + 1)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stop(actor: u32) -> Action {
        Action::Stop {
            actors: vec![actor],
        }
    }

    fn turn(message: Option<ClientMsg>) -> (u32, Vec<Bundle>) {
        match message {
            Some(ClientMsg::Turn { through, bundles }) => (through, bundles),
            other => panic!("expected a turn, got {other:?}"),
        }
    }

    #[test]
    fn empty_confirms_only_when_through_rises() {
        let mut outbox = Outbox::default();
        assert_eq!(turn(outbox.confirm(8, 9)), (8, vec![]));
        assert!(outbox.confirm(8, 9).is_none(), "a repeat is not a new turn");
        assert!(outbox.confirm(5, 6).is_none());
        assert_eq!(turn(outbox.confirm(9, 10)).0, 9);
    }

    #[test]
    fn commands_land_after_the_confirmed_beats() {
        let mut outbox = Outbox::default();
        outbox.confirm(8, 9);
        outbox.send(stop(1));
        outbox.send(Action::Noop);
        let (through, bundles) = turn(outbox.confirm(8, 9));
        assert_eq!(through, 9, "through rises to cover the bundle");
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].tick, 9);
        assert_eq!(bundles[0].actions, vec![stop(1)], "noop is never sent");
        // A bundle aimed at a beat already confirmed moves past it.
        outbox.send(stop(2));
        let (through, bundles) = turn(outbox.confirm(3, 4));
        assert_eq!((through, bundles[0].tick), (10, 10));
    }

    #[test]
    fn at_most_64_commands_per_turn() {
        let mut outbox = Outbox::default();
        for i in 1..=70 {
            outbox.send(stop(i));
        }
        let (_, bundles) = turn(outbox.confirm(1, 2));
        assert_eq!(bundles[0].actions.len(), 64);
        assert_eq!(outbox.waiting().len(), 6);
        let (_, bundles) = turn(outbox.confirm(1, 2));
        assert_eq!(bundles[0].actions.len(), 6);
    }

    #[test]
    fn pulse_runs_delay_ahead_of_the_clock() {
        let mut outbox = Outbox::default();
        assert_eq!(turn(pulse(&mut outbox, 0, 8)), (8, vec![]));
        assert!(pulse(&mut outbox, 0, 8).is_none());
        assert_eq!(turn(pulse(&mut outbox, 1, 8)).0, 9);
        // A click between beats: the old client confirms at once, one beat past the
        // buffer (the bundle sits at through + 1, and through rises to it).
        outbox.send(stop(1));
        let (through, bundles) = turn(pulse(&mut outbox, 1, 8));
        assert_eq!((through, bundles[0].tick), (10, 10));
        assert!(
            pulse(&mut outbox, 2, 8).is_none(),
            "already confirmed beyond"
        );
        assert_eq!(
            turn(pulse(&mut outbox, 10, 0)).0,
            11,
            "delay is at least one"
        );
    }
}
