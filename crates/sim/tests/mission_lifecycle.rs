//! Mission lifecycle: run timing, transactions, limits, the cinematic pause, saves and
//! determinism.

mod mission_fixture;

use canopy_content::placement::Owner;
use canopy_sim::{
    Action, Queued, World,
    mission::{self, sandbox},
    state::FactKind,
};
use mission_fixture::*;

const COUNTER: &str = r#"
function on_start() mission.set("start", mission.tick()) end
function on_tick()
  mission.set("runs", (mission.get("runs") or 0) + 1)
  mission.set("last", mission.tick())
end"#;

#[test]
fn on_start_runs_once_then_on_tick_every_four_ticks() {
    let mut world = world(COUNTER);
    world.step();
    assert_eq!(variable(&world, "start"), int(0));
    assert_eq!(variable(&world, "runs"), None);
    assert!(mission(&world).started);
    steps(&mut world, 4);
    assert_eq!(variable(&world, "runs"), int(1));
    assert_eq!(variable(&world, "last"), int(4));
    steps(&mut world, 8);
    assert_eq!(variable(&world, "runs"), int(3));
    assert_eq!(variable(&world, "last"), int(12));
    assert_eq!(variable(&world, "start"), int(0));
}

#[test]
fn a_script_without_callbacks_is_harmless() {
    let mut world = world("local x = 1");
    steps(&mut world, 9);
    assert_eq!(error(&world), None);
    assert!(mission(&world).started);
}

#[test]
fn a_failed_run_applies_nothing_and_stops_the_script_for_good() {
    let script = r#"
function on_start()
  mission.set("before", true)
  mission.begin_objective("reach")
  mission.begin_scene(10, 10)
  mission.spawn("ambusher")
  mission.transfer("guard", "none")
  mission.win()
  mission.alive("missing")
end
function on_tick() mission.set("ran", true) end"#;
    let mut world = Mission::new(script)
        .entity(serde_json::json!({"id": "ambusher", "definition": "unit.warrior", "position": {"x": 80, "y": 80}, "owner": "none", "activation": "script"}))
        .world();
    let entities = world.state().entities.len();
    steps(&mut world, 12);
    let state = mission(&world);
    assert_eq!(
        state.error.as_deref(),
        Some("[string \"mission.lua\"]:9: Unknown entity ID: missing")
    );
    assert!(state.variables.is_empty());
    assert!(state.objective_states.is_empty());
    assert!(state.spawned.is_empty());
    assert_eq!(state.scene, None);
    assert!(!state.started);
    assert_eq!(world.state().entities.len(), entities);
    assert_eq!(entity(&world, "guard").unwrap().owner, Owner::Player(0));
    assert_eq!(world.state().outcome, None);
    let fact = world.state().facts.last().unwrap();
    assert_eq!(fact.kind, FactKind::Error);
    assert_eq!(fact.owner, Owner::Player(0));
    assert_eq!(
        fact.message,
        "Mission script stopped: [string \"mission.lua\"]:9: Unknown entity ID: missing"
    );
    assert_eq!(world.state().facts.len(), 1, "reported once");
}

#[test]
fn an_error_clears_the_scene_and_turning_but_keeps_progress() {
    let script = r#"
function on_start() mission.set("kept", 1) mission.begin_scene(5, 5) mission.face("hero", 70, 60) end
function on_tick() mission.get(function() end) end"#;
    let mut world = world(script);
    world.step();
    assert!(mission(&world).scene.is_some());
    assert_eq!(mission(&world).facing.len(), 1);
    steps(&mut world, 4);
    let state = mission(&world);
    assert!(
        state
            .error
            .as_deref()
            .unwrap()
            .ends_with("Mission API accepts only strings, numbers, booleans and tables of options")
    );
    assert_eq!(state.scene, None);
    assert!(state.facing.is_empty());
    assert_eq!(variable(&world, "kept"), int(1));
}

#[test]
fn runs_are_checked_when_they_end() {
    let many = |count: u32| {
        format!("function on_start() for i = 1, {count} do mission.set('v' .. i, i) end end")
    };
    let world = started(&many(128));
    assert_eq!(mission(&world).variables.len(), 128);
    let mut world = world_of(&many(129));
    world.step();
    assert_eq!(error(&world), Some("Mission state exceeds 128 variables"));
    assert!(mission(&world).variables.is_empty());

    // A value is judged by what the run leaves behind.
    let long = "local s = 'x' for i = 1, 11 do s = s .. s end";
    started(&format!(
        "function on_start() {long} mission.set('s', s) mission.set('s', nil) end"
    ));
    let mut world = world_of(&format!(
        "function on_start() {long} mission.set('s', s) end"
    ));
    world.step();
    assert!(
        error(&world)
            .unwrap()
            .starts_with("Mission variable s must be")
    );
    let mut world = world_of("function on_start() mission.set('n', 0/0) end");
    world.step();
    assert!(
        error(&world)
            .unwrap()
            .starts_with("Mission variable n must be")
    );
}

fn world_of(script: &str) -> World {
    world(script)
}

#[test]
fn limits_stop_the_script() {
    for (script, expected) in [
        (
            "function on_start() while true do end end",
            "Mission exceeded 20,000 instructions".to_string(),
        ),
        (
            "function on_start() local s = 'x' for i = 1, 17 do s = s .. s end end",
            "Mission string exceeds 64 KiB".to_string(),
        ),
        (
            "mission.tick()",
            "[string \"mission.lua\"]:1: Call the mission API inside on_start/on_tick, not at script top level".to_string(),
        ),
        (
            "function on_start() for i = 1, 257 do mission.tick() end end",
            "[string \"mission.lua\"]:1: Mission API call budget exceeded".to_string(),
        ),
        (
            "on_start = 1",
            "on_start must be a function".to_string(),
        ),
    ] {
        let mut world = world(script);
        world.step();
        assert_eq!(error(&world), Some(expected.as_str()), "{script}");
    }
    started("function on_start() for i = 1, 256 do mission.tick() end end");
}

#[test]
fn scripts_must_compile_when_the_map_loads() {
    let error = World::new(
        content(),
        &Mission::new("function on_start(").map(),
        setup(),
    )
    .err()
    .expect("a syntax error");
    assert!(
        error.to_string().contains("[string \"mission.lua\"]:1:"),
        "{error}"
    );
}

#[test]
fn instruction_counts_are_fixed() {
    // Pinned: CI runs this on every platform, so the hook count must agree everywhere.
    let script = "local function f(x) return x * 2 end\nfunction on_tick() local s = 0 for i = 1, 10 do s = s + f(i) end mission.echo(s) end";
    let mut seen = Vec::new();
    let ran = sandbox::run(script, "on_tick", &["echo"], &mut |_, args| {
        seen.push(args);
        Ok(Vec::new())
    })
    .unwrap();
    assert_eq!(seen, [vec![sandbox::Scalar::Integer(110)]]);
    assert_eq!(ran.instructions, 84);
}

fn ping(world: &mut World) {
    let tick = world.tick();
    world
        .enqueue(Queued {
            tick,
            player: 0,
            seq: 0,
            action: Action::Ping { position: None },
        })
        .unwrap();
}

fn refusals(world: &World) -> usize {
    world
        .state()
        .facts
        .iter()
        .filter(|f| f.message == "Cinematic dialogue is playing")
        .count()
}

#[test]
fn cinematic_dialogue_pauses_the_game_but_not_the_clock() {
    let script = r#"
function on_start() mission.say("Marshal", "unit.marshal", "Hold.", 2, true, "hero") end
function on_tick() mission.set("busy", mission.dialogue_busy()) mission.set("at", mission.tick()) end"#;
    let mut world = world(script);
    world.step();
    let line = mission(&world).dialogue.clone().unwrap();
    assert_eq!(
        (line.duration_ticks, line.remaining, line.until),
        (80, 80, 80)
    );
    assert_eq!(line.actor, Some(entity(&world, "hero").unwrap().id));
    for paused in 1..=80 {
        ping(&mut world);
        world.step();
        assert_eq!(world.state().tick, 1, "game time stands still");
        assert_eq!(world.tick(), 1 + paused, "the clock runs on");
    }
    assert_eq!(refusals(&world), 80);
    let line = mission(&world).dialogue.clone().unwrap();
    assert_eq!((line.remaining, line.until), (0, 1));
    assert_eq!(mission(&world).paused_ticks, 80);
    ping(&mut world);
    steps(&mut world, 4);
    assert_eq!(refusals(&world), 80, "commands flow again");
    assert_eq!(world.state().tick, 5);
    assert_eq!(variable(&world, "at"), int(4));
    assert_eq!(variable(&world, "busy"), boolean(false));
}

#[test]
fn ordinary_dialogue_does_not_pause() {
    let script = r#"
function on_start() mission.say("Guard", "unit.warrior", "Over here.", 1) end
function on_tick() if not mission.get("first") then mission.set("first", mission.dialogue_busy()) end mission.set("busy", mission.dialogue_busy()) end"#;
    let mut world = world(script);
    world.step();
    let line = mission(&world).dialogue.clone().unwrap();
    assert!(!line.cinematic);
    assert_eq!(
        (line.duration_ticks, line.remaining, line.until),
        (40, 0, 40)
    );
    steps(&mut world, 44);
    assert_eq!(world.state().tick, 45);
    assert_eq!(variable(&world, "first"), boolean(true));
    assert_eq!(variable(&world, "busy"), boolean(false));
}

#[test]
fn scenes_refuse_commands_until_they_end() {
    let script = r#"
function on_start() mission.begin_scene(30, 30) end
function on_tick() if mission.tick() >= 8 then mission.end_scene() end end"#;
    let mut world = world(script);
    world.step();
    ping(&mut world);
    world.step();
    assert_eq!(refusals(&world), 1);
    steps(&mut world, 8);
    ping(&mut world);
    world.step();
    assert_eq!(refusals(&world), 1);
}

#[test]
fn winning_ends_the_script() {
    let script = r#"
function on_start() end
function on_tick() mission.set("ran", mission.tick()) if mission.tick() == 8 then mission.win() end end"#;
    let mut world = world(script);
    steps(&mut world, 20);
    let outcome = world.state().outcome.clone().unwrap();
    assert_eq!(outcome.winner, Some(Owner::Player(0)));
    assert_eq!(variable(&world, "ran"), int(8));
    let mut world = world_of("function on_start() mission.lose() end");
    world.step();
    let outcome = world.state().outcome.clone().unwrap();
    assert_eq!(
        (outcome.winner, outcome.defeated),
        (None, vec![Owner::Player(0)])
    );
}

const BUSY: &str = r#"
function on_start()
  mission.begin_objective("reach")
  mission.face("hero", 70, 70)
  mission.set("f", 0.1 + 0.2)
end
function on_tick()
  local t = mission.tick()
  mission.set("t" .. (t % 3), t / 7)
  if t == 12 then mission.say("Marshal", "unit.marshal", "Onward.", 1, true) end
  if t == 40 then mission.complete_objective("reach") mission.begin_objective("hold") mission.spawn("ambusher") end
  if t == 60 then mission.camera("third-person", "hero", "guard", 6, 1.8, 60, 0.5) end
end"#;

fn busy() -> Mission {
    Mission::new(BUSY).entity(serde_json::json!({"id": "ambusher", "definition": "unit.warrior", "position": {"x": 80, "y": 80}, "owner": "none", "activation": "script"}))
}

fn snapshot(world: &World) -> String {
    serde_json::to_string(&world.save()).unwrap()
}

#[test]
fn missions_are_deterministic_and_survive_save_and_restore() {
    let fixture = busy();
    let mut reference = fixture.world();
    let mut trace = Vec::new();
    for _ in 0..200 {
        reference.step();
        trace.push(snapshot(&reference));
    }
    assert_eq!(error(&reference), None);
    assert_eq!(mission(&reference).paused_ticks, 40);
    assert!(entity(&reference, "ambusher").is_some());
    assert!(mission(&reference).scene.is_some());

    let mut again = fixture.world();
    for expected in &trace {
        again.step();
        assert_eq!(&snapshot(&again), expected);
    }

    // Mid-pause (tick 30), mid-turn (tick 3) and mid-mission (tick 100).
    for at in [3, 30, 100] {
        let mut first = fixture.world();
        steps(&mut first, at);
        let mut resumed = reload(&first, &fixture);
        assert_eq!(snapshot(&resumed), trace[at as usize - 1]);
        for expected in &trace[at as usize..] {
            resumed.step();
            assert_eq!(&snapshot(&resumed), expected, "resumed at {at}");
        }
    }
}

#[test]
fn saves_belong_to_their_map() {
    let world = started(COUNTER);
    let save = world.save();
    let mut plain = Mission::new(COUNTER).map();
    plain.mission = None;
    assert!(World::restore(content(), &plain, save).is_err());
}

#[test]
fn the_runtime_reports_its_latest_effects() {
    let mut world = world("function on_start() mission.win() end");
    world.step();
    assert_eq!(
        world.mission().unwrap().effects(),
        [mission::MissionEffect::Win]
    );
}
