//! Autoplay (docs/campaign.md "Autoplay"): a plan steers player 1 with ordinary commands,
//! so its run replays exactly from those commands alone.

mod mission_fixture;

use canopy_content::placement::Owner;
use canopy_sim::{
    Queued, World,
    autoplay::{Autoplay, Status},
};
use mission_fixture::*;
use serde_json::json;
use std::sync::Arc;

/// The hero and the guard must walk to the far gate, then put the bandit down; the script
/// wins when it falls.
const SCRIPT: &str = r#"
function on_start() mission.begin_objective("reach") end
function on_tick()
  if mission.in_region("hero", "far") and not mission.get("reached") then
    mission.set("reached", true)
    mission.complete_objective("reach")
  end
  if mission.get("reached") and not mission.alive("bandit") then mission.win() end
end"#;

const PLAN: &str = r#"
say("off to the far gate")
hero{"hero"}
local arrived = go(100, 58)
assert(arrived, "never arrived")
assert(wait_objective("reach", 5), "not reached")
assert(alive("bandit") == true, "the bandit should be in sight")
click("statue", 2)
attack("bandit")
say("army", army())
wait_until(function() return false end, 30)
"#;

fn far_gate() -> Mission {
    let mut mission = Mission::new(SCRIPT);
    // The player's own: a captain and a soldier it commands.
    mission.entities[0]["definition"] = json!("unit.captain");
    mission.entities[1]["definition"] = json!("unit.soldier");
    let mut mission = mission
        .entity(json!({"id": "bandit", "definition": "unit.bandit", "position": {"x": 104, "y": 60}, "owner": "none"}))
        .camp(json!({"id": "camp.far", "members": ["bandit"], "home": {"x": 104, "y": 60}, "aggroRange": 4, "leash": 12, "aggression": "players"}));
    mission
        .regions
        .push(json!({"id": "far", "x": 100, "y": 58, "radius": 6}));
    mission
}

/// Steps `world`, asking `autoplay` after every step and queueing its commands for the
/// next tick as a session would; returns every command sent.
fn play(world: &mut World, autoplay: &mut Autoplay, limit: u32) -> Vec<Queued> {
    let mut sent = Vec::new();
    while world.tick() < limit && world.state().outcome.is_none() {
        world.step();
        let tick = world.tick();
        for (seq, action) in autoplay.tick(world).into_iter().enumerate() {
            let command = Queued {
                tick,
                player: 0,
                seq: seq as u32,
                action,
            };
            world
                .enqueue(command.clone())
                .expect("a well-formed command");
            sent.push(command);
        }
    }
    sent
}

#[test]
fn a_plan_wins_with_a_players_commands_and_replays_from_them() {
    let fixture = far_gate();
    let map = Arc::new(fixture.map());
    let mut world = fixture.world();
    let mut autoplay = Autoplay::new(PLAN, map).unwrap_or_else(|e| panic!("{e}"));
    let sent = play(&mut world, &mut autoplay, 40 * 240);
    let lines = autoplay.drain_lines().join("\n");
    assert_eq!(autoplay.status(), &Status::Running, "{lines}");
    let outcome = world.state().outcome.clone().expect("decided");
    assert_eq!(outcome.winner, Some(Owner::Player(0)), "{lines}");
    assert!(lines.contains("off to the far gate"), "{lines}");
    assert!(lines.contains("go 100,58"), "{lines}");
    assert_eq!(mission(&world).clicks.get("statue"), Some(&2));
    // Nothing but the commands: the same match without the plan ends the same.
    let mut again = fixture.world();
    for command in sent {
        while again.tick() < command.tick {
            again.step();
        }
        again.enqueue(command).unwrap();
    }
    while again.tick() < world.tick() {
        again.step();
    }
    assert_eq!(again.fingerprint(), world.fingerprint());
    assert_eq!(again.state().outcome, world.state().outcome);
}

fn failure(plan: &str) -> String {
    let fixture = far_gate();
    let mut world = fixture.world();
    let mut autoplay = Autoplay::new(plan, Arc::new(fixture.map())).unwrap();
    play(&mut world, &mut autoplay, 400);
    match autoplay.status() {
        Status::Failed(error) => error.clone(),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_plan_reaches_no_clock_randomness_or_unordered_tables() {
    for name in [
        "os",
        "io",
        "pairs",
        "next",
        "require",
        "load",
        "coroutine",
        "print",
    ] {
        let error = failure(&format!("assert({name} == nil, '{name}')\nerror('done')"));
        assert!(error.ends_with("done"), "{name}: {error}");
    }
    let error = failure("assert(math.random == nil and math.floor(2.5) == 2)\nerror('done')");
    assert!(error.ends_with("done"), "{error}");
}

#[test]
fn plan_errors_name_their_line_and_endless_loops_stop() {
    assert_eq!(
        failure("wait(0.1)\nattack('nobody')"),
        "autoplay.lua:2: attack: the map places no nobody"
    );
    let error = failure("while true do end");
    assert!(error.contains("two million instructions"), "{error}");
    assert!(Autoplay::new("go(", Arc::new(far_gate().map())).is_err());
}

/// While a cinematic holds the match, the plan waits and sends nothing.
#[test]
fn the_plan_waits_out_a_cinematic() {
    let script = r#"
function on_start() mission.play("intro") end
function scenes.intro()
  mission.cinematic(true)
  mission.wait(2)
  mission.cinematic(false)
end"#;
    let mut fixture = Mission::new(script);
    fixture.entities[0]["definition"] = json!("unit.captain");
    let mut world = fixture.world();
    let mut autoplay = Autoplay::new("go(80, 60)", Arc::new(fixture.map())).unwrap();
    world.step();
    let mut first = None;
    for _ in 0..200 {
        world.step();
        let actions = autoplay.tick(&mut world);
        if !actions.is_empty() && first.is_none() {
            first = Some(world.tick());
        }
    }
    assert!(first.is_some_and(|t| t >= 80), "{first:?}");
}
