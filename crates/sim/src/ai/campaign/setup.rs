//! A mission's players: player 1 and the computer players `mission.players` lists, whether
//! or not the map gives them a start.

use crate::setup::{MatchSetup, Slot, SlotKind};
use canopy_content::map::{Team, UtcMap};

/// Player 1's team; allies join it.
pub const PLAYER_TEAM: u8 = 0;
/// The team every enemy computer player shares.
pub const ENEMY_TEAM: u8 = 1;

/// The setup of a mission on `map` from any setup the caller made: player 1's slot (its
/// kind, name, race and hero) is kept; every other slot is replaced by the computer players
/// the mission lists (race, team: allies on player 1's team, enemies together on another).
/// Maps whose mission lists no players, and maps without a mission, keep `setup` as it is.
pub fn mission_setup(map: &UtcMap, setup: MatchSetup) -> MatchSetup {
    let Some(mission) = map.mission.as_ref().filter(|m| !m.players.is_empty()) else {
        return setup;
    };
    let first = setup
        .slots
        .iter()
        .find(|s| s.player == 0)
        .cloned()
        .unwrap_or(Slot {
            difficulty: Default::default(),
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        });
    let mut slots = vec![Slot {
        team: Some(PLAYER_TEAM),
        ..first
    }];
    let mut players = mission.players.clone();
    players.sort_by_key(|p| p.player);
    slots.extend(players.iter().map(|p| Slot {
        difficulty: Default::default(),
        player: p.player.saturating_sub(1),
        kind: SlotKind::Ai,
        name: None,
        team: Some(match p.team {
            Team::Ally => PLAYER_TEAM,
            Team::Enemy => ENEMY_TEAM,
        }),
        hero: None,
        race: p.race.clone(),
    }));
    MatchSetup {
        seed: setup.seed,
        slots,
    }
}
