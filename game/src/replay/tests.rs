//! A headless match on the simulation's test-owned AI fixture: a computer player against
//! scripted human commands, recorded through the real simulation-thread path, written as
//! a replay, read back and played again.

use super::{
    MatchInfo, build,
    format::{Mode, Player, Replay},
    playback::{Keyframes, Origin, Playback, Script},
    record::Recorder,
};
use crate::sim::{Driver, Seat, advance_with, ai_fixture};
use canopy_net::Session;
use canopy_scene::live::{Presenter, Setup};
use canopy_sim::{Action, World, action::CellPoint};
use std::sync::{Arc, atomic::AtomicBool};

const TICKS: u32 = 2600;

fn info() -> MatchInfo {
    MatchInfo {
        mode: Mode::Skirmish,
        map_name: "Fixture".into(),
        identity: crate::saves::MatchIdentity {
            map_id: "fixture".into(),
            map_digest: "00".into(),
            content_digest: "00".into(),
        },
        map_package: None,
        players: (0..2)
            .map(|slot| Player {
                slot,
                name: format!("P{slot}"),
                computer: slot == 0,
                race: None,
                hero: None,
                team: None,
                color: usize::from(slot),
            })
            .collect(),
        recorder: Some(1),
        colors: None,
    }
}

/// Plays the fixture match with the human's scripted commands; returns the replay and the
/// world it ended with.
fn record() -> (Replay, World, Arc<canopy_content::map::UtcMap>) {
    let map = Arc::new(ai_fixture::skirmish(vec![], vec![]));
    let setup = ai_fixture::setup(false);
    let mut session = Session::local(setup.clone(), Some(1));
    let mut world = World::new(ai_fixture::content(), &map, setup).unwrap();
    let recorder = Recorder::new(&world);
    let mut driver = Driver::Live {
        recorder: Some(recorder.clone()),
        autoplay: None,
    };
    let mut presenter = Presenter::new(
        &world,
        &map,
        None,
        Setup {
            colors: vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
            tree_slots: Default::default(),
            statics: 0,
        },
    );
    let commands = session.commands();
    let me = canopy_content::placement::Owner::Player(1);
    while world.tick() < TICKS {
        let tick = world.tick();
        let workers = ai_fixture::all_of(&world, me, ai_fixture::WORKER);
        let hero = ai_fixture::all_of(&world, me, ai_fixture::HERO);
        let hall = ai_fixture::all_of(&world, me, ai_fixture::HALL);
        let to = |x: u32, y: u32| CellPoint {
            x,
            y,
            surface: None,
        };
        if tick == 20 {
            commands.send(Action::Produce {
                actor: hall[0],
                definition: canopy_content::Id::new(ai_fixture::WORKER).unwrap(),
            });
        }
        if tick == 40 || tick == 1300 {
            commands.send(Action::Move {
                actors: hero.clone(),
                destination: to(128, 128),
                attack_move: Some(true),
                append: None,
            });
        }
        if tick == 600 && workers.len() > 1 {
            commands.send(Action::Move {
                actors: workers[..2].to_vec(),
                destination: to(60, 60),
                attack_move: None,
                append: None,
            });
        }
        if tick == 900 {
            commands.send(Action::Stop {
                actors: hero.clone(),
            });
        }
        if advance_with(
            &mut world,
            &mut session,
            &mut driver,
            &mut presenter,
            Seat::Player(1),
            None,
        )
        .is_none()
        {
            panic!("the local session always has the next beat");
        }
    }
    let recording = recorder.snapshot();
    assert!(
        recording.commands.len() >= 4,
        "the human's commands are recorded: {:?}",
        recording.commands
    );
    assert!(
        recording.commands.iter().all(|c| c.player == 1),
        "computer players are not recorded"
    );
    assert_eq!(recording.tick, TICKS);
    assert_eq!(recording.fingerprints.len(), (TICKS / 400) as usize);
    let replay = build(&recording, &info(), "Unfinished", 1).unwrap();
    (replay, world, map)
}

#[test]
fn a_recorded_match_replays_identically() {
    let (replay, world, map) = record();
    let bytes = replay.to_bytes().unwrap();
    let back = Replay::from_bytes(&bytes).unwrap();
    assert_eq!(back, replay);

    // Played headless: every fingerprint and the final state agree.
    let script = Script::new(&back.body, back.header.start_tick, back.header.end_tick);
    let origin = Origin::new(ai_fixture::content(), map.clone(), &back);
    let keyframes = Keyframes::default();
    keyframes.analyse(&origin, &script, &AtomicBool::new(false));
    let analysis = keyframes.analysis();
    assert!(analysis.done && analysis.failed.is_none());
    assert_eq!(analysis.analysed, TICKS);
    assert_eq!(analysis.desync, None);
    assert_eq!(
        analysis.outcome.map(|o| o.0),
        world.state().outcome.clone(),
        "the same outcome"
    );
    assert!(analysis.keyframes >= (TICKS / 400) as usize);

    // The viewer's playback, seeking back and forth, ends where the match ended.
    let mut playback = Playback::new(&back, ai_fixture::content(), map);
    while !playback.analysis().done {
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    let mut shown = playback.first_world().unwrap();
    while shown.tick() < 1500 {
        playback.advance(&mut shown);
    }
    playback.seek(&mut shown, 500).unwrap();
    assert_eq!(shown.tick(), 500);
    playback.seek(&mut shown, 2000).unwrap();
    playback.seek(&mut shown, 1210).unwrap();
    while playback.advance(&mut shown).is_some() {}
    assert_eq!(shown.tick(), TICKS);
    assert_eq!(shown.fingerprint(), world.fingerprint());
    assert_eq!(shown.save(), world.save());
    assert_eq!(playback.desync, None);
}

#[test]
fn a_changed_replay_is_out_of_sync() {
    let (mut replay, _, map) = record();
    // Without the human's first order the match plays differently.
    replay.body.commands.remove(0);
    let script = Script::new(&replay.body, 0, replay.header.end_tick);
    let origin = Origin::new(ai_fixture::content(), map, &replay);
    let keyframes = Keyframes::default();
    keyframes.analyse(&origin, &script, &AtomicBool::new(false));
    let desync = keyframes.analysis().desync.expect("found out of sync");
    assert_eq!(desync % 400, 0, "at a recorded fingerprint");
}
