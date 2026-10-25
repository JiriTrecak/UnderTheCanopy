//! AI-versus-AI matches on the shipped maps, headless: the M2 finish line. Every match runs
//! twice, once straight through and once saved and restored half way; both must produce
//! the same fingerprint every tick and the same final state. CI runs the short match on
//! macOS and Windows, so it also checks that both platforms simulate identically.
//!
//! The full matches take minutes and are ignored; run them in release:
//!
//! `cargo test --release -p canopy-sim --test ai_match -- --ignored --nocapture`
#![allow(clippy::disallowed_methods)] // wall-clock timing of the long matches

use canopy_content::{Content, map, map::UtcMap};
use canopy_sim::{MatchSetup, Slot, SlotKind, World, state::Outcome};
use std::{path::Path, sync::Arc, time::Instant};

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

fn content() -> Arc<Content> {
    Arc::new(Content::load(data()).unwrap_or_else(|errors| panic!("{errors}")))
}

/// The shipped maps that can hold a match (not testbeds or missions), fewest starts first.
fn maps() -> Vec<UtcMap> {
    let mut maps: Vec<UtcMap> = map::library(data())
        .unwrap_or_else(|errors| panic!("{errors}"))
        .iter()
        .map(|file| file.load().unwrap_or_else(|errors| panic!("{errors}")))
        .filter(|map| !map.sandbox && map.mission.is_none())
        .collect();
    maps.sort_by(|a, b| (a.player_starts.len(), &a.name).cmp(&(b.player_starts.len(), &b.name)));
    maps
}

/// `map` with only the starts of `players` (one-based), so a duel can be played on a map
/// with more starts.
fn with_starts(map: &UtcMap, players: &[u8]) -> UtcMap {
    let mut map = map.clone();
    map.player_starts.retain(|s| players.contains(&s.player));
    map
}

/// The two starts farthest apart.
fn farthest(map: &UtcMap) -> [u8; 2] {
    let starts = &map.player_starts;
    let mut best = (0.0, [starts[0].player, starts[1].player]);
    for a in starts {
        for b in starts {
            let d = (a.x - b.x).hypot(a.z - b.z);
            if d > best.0 {
                best = (d, [a.player, b.player]);
            }
        }
    }
    best.1
}

fn setup(map: &UtcMap) -> MatchSetup {
    MatchSetup {
        seed: 0x5eed,
        slots: map
            .player_starts
            .iter()
            .map(|s| Slot {
                difficulty: Default::default(),
                player: s.player - 1,
                kind: SlotKind::Ai,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    }
}

/// Plays until an outcome or `cap` ticks, saving and restoring after `save_at` ticks when
/// given. Returns every tick's fingerprint and the final world.
fn play(content: &Arc<Content>, map: &UtcMap, cap: u32, save_at: Option<u32>) -> (Vec<u32>, World) {
    let mut world = World::new(content.clone(), map, setup(map)).unwrap_or_else(|e| panic!("{e}"));
    let mut fingerprints = Vec::new();
    for tick in 0..cap {
        if Some(tick) == save_at {
            let save = serde_json::to_string(&world.save()).unwrap();
            world = World::restore(content.clone(), map, serde_json::from_str(&save).unwrap())
                .unwrap_or_else(|e| panic!("{e}"));
        }
        world.step();
        fingerprints.push(world.fingerprint());
        if world.state().outcome.is_some() {
            break;
        }
    }
    (fingerprints, world)
}

/// Plays `map` straight and with a save half way; both must agree.
fn check(content: &Arc<Content>, map: &UtcMap, cap: u32) -> (u32, Option<Outcome>) {
    let started = Instant::now();
    let (reference, world) = play(content, map, cap, None);
    let ticks = reference.len() as u32;
    let elapsed = started.elapsed();
    let (resumed, restored) = play(content, map, cap, Some(ticks / 2));
    assert_eq!(
        resumed, reference,
        "{}: save/restore changes the match",
        map.name
    );
    assert_eq!(
        restored.state(),
        world.state(),
        "{}: final states differ",
        map.name
    );
    for summary in world.ai_summaries() {
        assert!(
            summary.metrics.issued > 0,
            "{}: player {} idle",
            map.name,
            summary.player
        );
        println!(
            "  player {}: {} | {} | {:?}",
            summary.player, summary.mission, summary.plan.economy, summary.metrics
        );
    }
    println!(
        "{} with {} AI players: {} after {ticks} ticks ({:.1} game minutes) in {:.1} s",
        map.name,
        map.player_starts.len(),
        world
            .state()
            .outcome
            .as_ref()
            .map_or("no outcome".into(), |o| format!("{o:?}")),
        f64::from(ticks) / 40.0 / 60.0,
        elapsed.as_secs_f64(),
    );
    (ticks, world.state().outcome.clone())
}

#[test]
fn an_ai_duel_on_a_shipped_map_is_deterministic_and_resumable() {
    let content = content();
    let map = maps().into_iter().next().expect("a shipped map");
    let duel = with_starts(&map, &farthest(&map));
    let (ticks, _) = check(&content, &duel, 1200);
    assert_eq!(ticks, 1200);
}

/// Cap for the full matches: an hour of game time.
const CAP: u32 = 144_000;

#[test]
#[ignore = "long; run in release with --ignored --nocapture"]
fn ai_duels_on_every_shipped_map() {
    let content = content();
    for map in maps() {
        let duel = with_starts(&map, &farthest(&map));
        check(&content, &duel, CAP);
    }
}

#[test]
#[ignore = "long; run in release with --ignored --nocapture"]
fn ai_free_for_all_with_every_start_taken() {
    let content = content();
    let map = maps().into_iter().last().expect("a shipped map");
    check(&content, &map, CAP);
}

#[test]
#[ignore = "long; run in release with --ignored --nocapture"]
fn computer_ants_found_mounds_and_travel_through_them() {
    let content = content();
    let map = maps()
        .into_iter()
        .find(|m| m.player_starts.len() == 2)
        .expect("a duel map");
    let mut world =
        World::new(content.clone(), &map, setup(&map)).unwrap_or_else(|e| panic!("{e}"));
    let mut underground = 0;
    for _ in 0..36_000 {
        world.step();
        underground = underground.max(
            world
                .state()
                .entities
                .iter()
                .filter(|e| {
                    e.unit
                        .as_ref()
                        .and_then(|u| u.tunnel.as_ref())
                        .is_some_and(|t| t.arrives.is_some())
                })
                .count(),
        );
    }
    println!("at most {underground} ants underground at once");
    assert!(underground > 0, "no ant ever used a tunnel");
}
