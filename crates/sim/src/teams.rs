//! Teams: the only source of who is an ally (docs/decisions.md D20).
//!
//! Every player has a team. Slots that don't choose one get their own team at match start,
//! so rules (sight, spells that affect allies, auras, AI) only ever compare teams and never
//! special-case owners.

use crate::setup::MatchSetup;
use canopy_content::placement::Owner;

/// How two owners stand to each other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Relation {
    /// The same owner.
    Own,
    /// Different players on one team.
    Ally,
    /// Players on different teams.
    Enemy,
    /// A player and the neutral side (camps, unowned resources).
    Neutral,
}

/// The resolved teams of a match.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Teams {
    /// `(player, team)`, ascending by player.
    slots: Vec<(u8, u8)>,
    /// Distinct teams, ascending; a team's position here is its dense index.
    teams: Vec<u8>,
    /// Slot owners, ascending.
    owners: Vec<Owner>,
}

impl Teams {
    /// Keeps chosen teams and gives every slot without one its own team: the smallest
    /// number no other slot uses, in player order.
    pub fn new(setup: &MatchSetup) -> Self {
        let mut slots: Vec<(u8, Option<u8>)> =
            setup.slots.iter().map(|s| (s.player, s.team)).collect();
        slots.sort_unstable();
        let mut used: Vec<u8> = slots.iter().filter_map(|s| s.1).collect();
        let mut free = 0u8;
        let slots: Vec<(u8, u8)> = slots
            .into_iter()
            .map(|(player, team)| {
                let team = team.unwrap_or_else(|| {
                    while used.contains(&free) {
                        free += 1;
                    }
                    used.push(free);
                    free
                });
                (player, team)
            })
            .collect();
        let mut teams: Vec<u8> = slots.iter().map(|s| s.1).collect();
        teams.sort_unstable();
        teams.dedup();
        let owners = slots.iter().map(|s| Owner::Player(s.0)).collect();
        Self {
            slots,
            teams,
            owners,
        }
    }

    /// The owner's team; `None` for the neutral side and owners not in the match.
    pub fn team_of(&self, owner: Owner) -> Option<u8> {
        let Owner::Player(player) = owner else {
            return None;
        };
        self.slots
            .binary_search_by_key(&player, |s| s.0)
            .ok()
            .map(|i| self.slots[i].1)
    }

    /// Dense index of the owner's team among [`Teams::teams`].
    pub fn index(&self, owner: Owner) -> Option<usize> {
        let team = self.team_of(owner)?;
        self.teams.binary_search(&team).ok()
    }

    pub fn teams(&self) -> &[u8] {
        &self.teams
    }

    /// The players of the match, ascending.
    pub fn owners(&self) -> &[Owner] {
        &self.owners
    }

    pub fn relation(&self, a: Owner, b: Owner) -> Relation {
        if a == b {
            return Relation::Own;
        }
        match (self.team_of(a), self.team_of(b)) {
            (Some(x), Some(y)) if x == y => Relation::Ally,
            (Some(_), Some(_)) => Relation::Enemy,
            _ => Relation::Neutral,
        }
    }

    /// The same owner or the same team.
    pub fn allied(&self, a: Owner, b: Owner) -> bool {
        matches!(self.relation(a, b), Relation::Own | Relation::Ally)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup::{Slot, SlotKind};

    fn setup(teams: &[Option<u8>]) -> MatchSetup {
        let slots = teams
            .iter()
            .enumerate()
            .map(|(player, &team)| Slot {
                player: player as u8,
                kind: SlotKind::Ai,
                name: None,
                team,
                hero: None,
                race: None,
            })
            .collect();
        MatchSetup { seed: 1, slots }
    }

    #[test]
    fn slots_without_a_team_play_alone() {
        // Player 0 chose team 1; player 1 chose none and must not join team 1.
        let teams = Teams::new(&setup(&[Some(1), None, Some(1), None]));
        let p = Owner::Player;
        assert_eq!(teams.relation(p(0), p(2)), Relation::Ally);
        assert_eq!(teams.relation(p(0), p(1)), Relation::Enemy);
        assert_eq!(teams.relation(p(1), p(3)), Relation::Enemy);
        assert_eq!(teams.relation(p(1), p(1)), Relation::Own);
        assert_eq!(teams.relation(p(1), Owner::Nobody), Relation::Neutral);
        assert_eq!(teams.teams().len(), 3);
        assert_eq!(teams.team_of(p(1)), Some(0));
        assert_eq!(teams.team_of(p(3)), Some(2));
    }

    #[test]
    fn free_for_all_gives_everyone_a_team() {
        let teams = Teams::new(&setup(&[None, None, None]));
        assert_eq!(teams.teams(), [0, 1, 2]);
        assert!(!teams.allied(Owner::Player(0), Owner::Player(1)));
    }
}
