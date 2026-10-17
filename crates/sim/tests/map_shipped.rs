//! Whatever maps the repository ships must start a match and pass the editor's playability
//! checks: room and flat dry ground for every start, and lanes wide enough for armies to
//! every start, camp and amber site. No test here names a shipped map.

use canopy_content::{Content, map};
use canopy_sim::{MatchSetup, Slot, SlotKind, World};
use std::{path::Path, sync::Arc};

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

#[test]
fn shipped_maps_are_playable_and_armies_reach_everything() {
    let content = Arc::new(Content::load(data()).unwrap_or_else(|errors| panic!("{errors}")));
    for file in map::library(data()).unwrap_or_else(|errors| panic!("{errors}")) {
        let map = file.load().unwrap_or_else(|errors| panic!("{errors}"));
        let setup = MatchSetup {
            seed: 1,
            slots: map
                .player_starts
                .iter()
                .map(|s| Slot {
                    player: s.player - 1,
                    kind: SlotKind::Ai,
                    name: None,
                    team: None,
                    hero: None,
                    race: None,
                })
                .collect(),
        };
        let world = World::new(content.clone(), &map, setup)
            .unwrap_or_else(|error| panic!("{}: {}", file.file, error.0));
        canopy_sim::spawn::playable(&map, world.catalog(), world.terrain())
            .unwrap_or_else(|error| panic!("{}: {error}", file.file));
        canopy_sim::spawn::connected(&map, world.terrain())
            .unwrap_or_else(|error| panic!("{}: {error}", file.file));
    }
}
