//! Every shipped map runs identically twice and across save/restore. CI runs this on macOS
//! and Windows, so it also checks that both platforms simulate identically.

use canopy_content::{Content, map};
use canopy_sim::{MatchSetup, Slot, SlotKind, World};
use std::{path::Path, sync::Arc};

const TICKS: u32 = 400;

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn setup(players: usize) -> MatchSetup {
    let slots = (0..players as u8)
        .map(|player| Slot {
            player,
            kind: SlotKind::Ai,
            name: None,
            team: None,
            hero: None,
            race: None,
        })
        .collect();
    MatchSetup {
        seed: 0x5eed,
        slots,
    }
}

/// Orders every player's units towards the map centre, so movement takes part.
fn march(world: &mut World) {
    let centre = world.terrain().size() / 2;
    for slot in world.setup().slots.clone() {
        let owner = canopy_content::placement::Owner::Player(slot.player);
        let actors: Vec<u32> = world
            .state()
            .entities
            .iter()
            .filter(|e| e.owner == owner && e.unit.is_some())
            .map(|e| e.id.0)
            .take(canopy_sim::action::MAX_ACTORS)
            .collect();
        if actors.is_empty() {
            continue;
        }
        let action = canopy_sim::Action::Move {
            actors,
            destination: canopy_sim::action::CellPoint {
                x: centre,
                y: centre,
                surface: None,
            },
            attack_move: None,
            append: None,
        };
        world
            .enqueue(canopy_sim::Queued {
                tick: 0,
                player: slot.player,
                seq: 0,
                action,
            })
            .unwrap();
    }
}

/// Fingerprints of every tick.
fn run(world: &mut World, ticks: u32) -> Vec<u32> {
    (0..ticks)
        .map(|_| {
            world.step();
            world.fingerprint()
        })
        .collect()
}

#[test]
fn shipped_maps_simulate_deterministically() {
    let content = Arc::new(Content::load(data()).unwrap_or_else(|errors| panic!("{errors}")));
    let maps = map::library(data()).unwrap_or_else(|errors| panic!("{errors}"));
    assert!(!maps.is_empty());
    for file in maps {
        let map = file.load().unwrap_or_else(|errors| panic!("{errors}"));
        // A mission seats player 1 with or without a start.
        let setup = setup(
            map.player_starts
                .len()
                .max(usize::from(map.mission.is_some())),
        );
        let start = || {
            let mut world = World::new(content.clone(), &map, setup.clone())
                .unwrap_or_else(|e| panic!("{}: {e}", file.id));
            march(&mut world);
            world
        };

        let mut first = start();
        assert!(
            !first.state().entities.is_empty(),
            "{}: no entities",
            file.id
        );
        let reference = run(&mut first, TICKS);
        assert_eq!(
            run(&mut start(), TICKS),
            reference,
            "{}: runs differ",
            file.id
        );

        let mut halfway = start();
        let mut resumed_fingerprints = run(&mut halfway, TICKS / 2);
        let save = serde_json::to_string(&halfway.save()).unwrap();
        let mut resumed =
            World::restore(content.clone(), &map, serde_json::from_str(&save).unwrap()).unwrap();
        resumed_fingerprints.extend(run(&mut resumed, TICKS / 2));
        assert_eq!(
            resumed_fingerprints, reference,
            "{}: save/restore changes the simulation",
            file.id
        );
    }
}
