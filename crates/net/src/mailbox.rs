//! The in-process match mailbox (old `src/net/room.ts`): the server's lockstep core.
//!
//! Each slot confirms beats; once every slot still connected has confirmed beat `T`, `T`
//! is committed with every slot's actions in player order. Single-player runs its match
//! through this mailbox with no socket (as the old `MemoryChannel` did), so local and
//! networked play feed the simulation the same way; tests use it as a stand-in server.

use crate::wire::{Bundle, Commit, CommitSlot, MAX_BUNDLE_ACTIONS, MAX_BUNDLES, WireAction};
use canopy_sim::Action;
use std::collections::{BTreeMap, BTreeSet};

/// How far past the last commit a confirmation may reach.
pub const MAX_CONFIRM_AHEAD: u32 = 4096;

#[derive(Debug, Clone)]
pub struct Mailbox {
    delay: u32,
    /// Per slot: confirmed through, and held actions by beat.
    slots: BTreeMap<u8, (u32, BTreeMap<u32, Vec<Action>>)>,
    dropped: BTreeSet<u8>,
    committed: u32,
    accepting: bool,
}

impl Mailbox {
    /// A mailbox for `players` with input delay `delay`. With `accepting` false nothing is
    /// committed until [`Mailbox::release`] (the server waits for every player's `ready`).
    pub fn new(players: impl IntoIterator<Item = u8>, delay: u32, accepting: bool) -> Self {
        Self {
            delay,
            slots: players
                .into_iter()
                .map(|p| (p, (0, BTreeMap::new())))
                .collect(),
            dropped: BTreeSet::new(),
            committed: 0,
            accepting,
        }
    }

    /// Continues a match from beat `beat` (a loaded save): everything up to it counts as
    /// committed and confirmed.
    pub fn resume(&mut self, beat: u32) {
        self.committed = self.committed.max(beat);
        for (through, held) in self.slots.values_mut() {
            *through = (*through).max(beat);
            held.retain(|&tick, _| tick > beat);
        }
    }

    /// The last committed beat (0 before the first).
    pub fn tick(&self) -> u32 {
        self.committed
    }

    /// `player` promises nothing more up to `through` and hands in bundles. Malformed
    /// turns are ignored whole, as are turns whose `through` does not rise. Returns the
    /// beats this completes.
    pub fn confirm(&mut self, player: u8, through: u32, bundles: &[Bundle]) -> Vec<Commit> {
        let delay = self.delay;
        let committed = self.committed;
        let Some((confirmed, held)) = self.slots.get_mut(&player) else {
            return Vec::new();
        };
        if through > committed + MAX_CONFIRM_AHEAD || bundles.len() > MAX_BUNDLES {
            return Vec::new();
        }
        let valid = bundles.iter().all(|b| {
            b.tick > 0
                && b.tick <= through + delay + 1
                && b.actions.len() <= MAX_BUNDLE_ACTIONS
                && b.actions.iter().all(|a| a.problem().is_none())
        });
        if !valid || through <= *confirmed {
            return Vec::new();
        }
        *confirmed = through;
        for bundle in bundles {
            if bundle.tick > committed {
                held.entry(bundle.tick)
                    .or_insert_with(|| bundle.actions.clone());
            }
        }
        self.flush()
    }

    /// A slot is gone: its confirmations are no longer awaited; it stays in commits.
    pub fn drop_player(&mut self, player: u8) -> Vec<Commit> {
        if !self.slots.contains_key(&player) {
            return Vec::new();
        }
        self.dropped.insert(player);
        self.flush()
    }

    /// Starts committing (every player is ready).
    pub fn release(&mut self) -> Vec<Commit> {
        self.accepting = true;
        self.flush()
    }

    fn flush(&mut self) -> Vec<Commit> {
        let mut out = Vec::new();
        if !self.accepting {
            return out;
        }
        while self.ready(self.committed + 1) {
            let tick = self.committed + 1;
            let slots = self
                .slots
                .iter_mut()
                .map(|(player, (_, held))| CommitSlot {
                    player: *player,
                    actions: held
                        .remove(&tick)
                        .unwrap_or_default()
                        .into_iter()
                        .map(WireAction::Valid)
                        .collect(),
                })
                .collect();
            self.committed = tick;
            out.push(Commit { tick, slots });
        }
        out
    }

    fn ready(&self, tick: u32) -> bool {
        let mut any = false;
        for (player, (through, _)) in &self.slots {
            if self.dropped.contains(player) {
                continue;
            }
            any = true;
            if *through < tick {
                return false;
            }
        }
        any
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

    fn bundle(tick: u32, actions: Vec<Action>) -> Bundle {
        Bundle { tick, actions }
    }

    fn actions(commit: &Commit, player: u8) -> Vec<WireAction> {
        commit
            .slots
            .iter()
            .find(|s| s.player == player)
            .unwrap()
            .actions
            .clone()
    }

    #[test]
    fn commits_once_every_slot_confirmed() {
        let mut mailbox = Mailbox::new([1, 0], 2, true);
        assert!(
            mailbox
                .confirm(0, 3, &[bundle(2, vec![stop(5)])])
                .is_empty()
        );
        let commits = mailbox.confirm(1, 2, &[]);
        assert_eq!(commits.iter().map(|c| c.tick).collect::<Vec<_>>(), [1, 2]);
        assert_eq!(
            commits[1]
                .slots
                .iter()
                .map(|s| s.player)
                .collect::<Vec<_>>(),
            [0, 1],
            "player order"
        );
        assert_eq!(actions(&commits[1], 0), vec![WireAction::Valid(stop(5))]);
        assert_eq!(mailbox.tick(), 2);
    }

    #[test]
    fn ignores_malformed_and_stale_turns() {
        let mut mailbox = Mailbox::new([0], 2, true);
        assert!(
            mailbox
                .confirm(0, 1, &[bundle(5, vec![stop(1)])])
                .is_empty(),
            "bundle beyond through + delay + 1"
        );
        assert!(
            mailbox
                .confirm(0, 1, &[bundle(1, vec![Action::Stop { actors: vec![] }])])
                .is_empty()
        );
        assert!(mailbox.confirm(0, 5000, &[]).is_empty(), "too far ahead");
        assert_eq!(mailbox.confirm(0, 1, &[]).len(), 1);
        assert!(
            mailbox
                .confirm(0, 1, &[bundle(2, vec![stop(1)])])
                .is_empty(),
            "through did not rise: bundles are ignored too"
        );
        let commits = mailbox.confirm(0, 2, &[bundle(1, vec![stop(9)])]);
        assert!(
            actions(&commits[0], 0).is_empty(),
            "bundles for committed beats are dropped"
        );
    }

    #[test]
    fn dropped_slots_are_not_awaited_but_still_listed() {
        let mut mailbox = Mailbox::new([0, 1], 1, true);
        mailbox.confirm(0, 2, &[]);
        let commits = mailbox.drop_player(1);
        assert_eq!(commits.len(), 2);
        assert_eq!(commits[0].slots.len(), 2);
        let mut alone = Mailbox::new([0], 1, true);
        assert!(
            alone.drop_player(0).is_empty(),
            "no commits without any slot"
        );
    }

    #[test]
    fn holds_commits_until_released() {
        let mut mailbox = Mailbox::new([0], 1, false);
        assert!(mailbox.confirm(0, 3, &[]).is_empty());
        assert_eq!(mailbox.release().len(), 3);
    }

    #[test]
    fn first_bundle_for_a_beat_wins() {
        let mut mailbox = Mailbox::new([0, 1], 4, true);
        mailbox.confirm(0, 1, &[bundle(3, vec![stop(1)])]);
        mailbox.confirm(0, 2, &[bundle(3, vec![stop(2)])]);
        assert_eq!(mailbox.confirm(1, 3, &[]).len(), 2);
        let commits = mailbox.confirm(0, 3, &[bundle(3, vec![stop(3)])]);
        assert_eq!(actions(&commits[0], 0), vec![WireAction::Valid(stop(1))]);
    }
}
