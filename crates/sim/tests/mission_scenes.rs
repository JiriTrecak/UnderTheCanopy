//! Mission scenes (docs/campaign.md "Scenes"): statements and waits, cinematic mode,
//! cameras, fades, lines and animations, the skip, saves and determinism.

mod mission_fixture;

use canopy_sim::{Action, Queued, World, entity::Milli, mission::sandbox};
use mission_fixture::*;
use serde_json::json;

/// A hero walks in on a fade from black, the camera cuts to the road and blends to the
/// gate; at the gate the hero turns to the guard and speaks, the guard salutes, and
/// after a beat the player gets the game back.
const INTRO: &str = r#"
function on_start()
  mission.play("intro")
end

function on_tick()
  if not mission.playing() then mission.set("free", mission.tick()) end
end

function scenes.intro()
  mission.cinematic(true)
  mission.fade_out(0)
  mission.fade_in(1)
  mission.camera_to("road", 0)
  mission.move("hero", 70, 60)
  mission.camera_to("gate", 2)
  mission.wait_arrived("hero")
  mission.set("arrived", mission.tick())
  mission.face("hero", 70, 66)
  mission.wait_facing("hero")
  mission.speak("hero", "Open the gate.", 1)
  mission.animate("guard", "attack")
  mission.wait(0.5)
  mission.cinematic(false)
  mission.camera_reset(1)
  mission.set("ended", true)
end
"#;

fn cameras(mission: Mission) -> Mission {
    mission
        .camera(json!({"id": "road", "x": 60, "y": 60, "distance": 30, "pitch": 40, "yaw": 20, "fov": 40}))
        .camera(json!({"id": "gate", "x": 70.5, "y": 61, "height": 1.5, "distance": 12, "pitch": 15, "yaw": -30, "fov": 35, "roll": 2}))
}

fn intro() -> Mission {
    let mut mission = cameras(Mission::new(INTRO));
    // The guard stands at the gate, out of the hero's way.
    mission.entities[1]["position"] = json!({"x": 70, "y": 66});
    mission
}

fn command(world: &mut World, action: Action) {
    let tick = world.tick();
    world
        .enqueue(Queued {
            tick,
            player: 0,
            seq: 0,
            action,
        })
        .unwrap();
}

fn refusals(world: &World) -> usize {
    world
        .state()
        .facts
        .iter()
        .filter(|f| f.message == "A cinematic is playing")
        .count()
}

/// Steps until `done` holds, at most `limit` ticks; the ticks taken.
fn until(world: &mut World, limit: u32, done: impl Fn(&World) -> bool) -> u32 {
    for taken in 0..limit {
        if done(world) {
            return taken;
        }
        world.step();
    }
    panic!("not done in {limit} ticks: {:?}", mission(world).cinema);
}

#[test]
fn a_scene_plays_its_statements_and_waits_in_order() {
    let mut world = intro().world();
    world.step();
    assert_eq!(error(&world), None);
    let cinema = &mission(&world).cinema;
    assert!(cinema.on);
    let play = cinema.scene.as_ref().expect("the intro plays");
    assert_eq!((play.name.as_str(), play.done), ("intro", 6));
    // Black at once, then a second's fade in.
    let fade = cinema.fade.clone().unwrap();
    assert_eq!(
        (fade.from, fade.to, fade.start, fade.ticks),
        (1000, 0, 0, 40)
    );
    // A cut to the road, then a blend to the gate.
    let shot = cinema.shot.clone().unwrap();
    assert_eq!(shot.from.camera.as_deref(), Some("road"));
    assert_eq!(shot.to.camera.as_deref(), Some("gate"));
    assert_eq!(shot.ticks, 80);

    // The player's commands wait; the hero walks.
    command(&mut world, Action::Ping { position: None });
    world.step();
    assert_eq!(refusals(&world), 1);
    let walked = until(&mut world, 2000, |w| variable(w, "arrived").is_some());
    assert!(walked > 100, "{walked}");
    let hero = entity(&world, "hero").unwrap();
    assert!((hero.position.x - 70_000).abs() <= 500);

    // A turn, then the line, which holds the scene for its second.
    until(&mut world, 400, |w| {
        w.state().mission.as_ref().unwrap().dialogue.is_some()
    });
    let line = mission(&world).dialogue.clone().unwrap();
    assert_eq!(line.text, "Open the gate.");
    assert_eq!(line.speaker, "unit.marshal");
    assert_eq!(line.actor, Some(entity(&world, "hero").unwrap().id));
    assert!(!line.cinematic, "a scene's line never pauses the game");
    let spoke = world.state().tick;
    until(&mut world, 400, |w| {
        !w.state()
            .mission
            .as_ref()
            .unwrap()
            .cinema
            .animations
            .is_empty()
    });
    assert!(world.state().tick >= spoke + 40);
    let animation = &mission(&world).cinema.animations[0];
    assert_eq!(
        (animation.placement.as_str(), animation.name.as_str()),
        ("guard", "attack")
    );
    let view = world.observer_view();
    let shown = &view.mission.as_ref().unwrap().animations;
    assert_eq!(shown[0].0, entity(&world, "guard").unwrap().id);

    // Half a second later the game is the player's again.
    until(&mut world, 200, |w| variable(w, "ended").is_some());
    let cinema = &mission(&world).cinema;
    assert!(!cinema.on);
    assert_eq!(cinema.scene, None);
    let shot = cinema.shot.clone().unwrap();
    assert_eq!(shot.to.camera, None, "back to the game's camera");
    assert_eq!(
        shot.to.target,
        Some(Milli {
            x: 70_500,
            y: 61_000
        })
    );
    steps(&mut world, 8);
    assert!(variable(&world, "free").is_some());
    command(&mut world, Action::Ping { position: None });
    world.step();
    assert_eq!(refusals(&world), 1);
    assert_eq!(cinema_fade_at(&world), 0);
}

fn cinema_fade_at(world: &World) -> u16 {
    mission(world)
        .cinema
        .fade
        .as_ref()
        .map_or(0, |f| f.at(world.state().tick))
}

#[test]
fn a_skip_leaves_the_world_as_the_scene_would() {
    let mut world = intro().world();
    steps(&mut world, 30);
    assert!(mission(&world).cinema.scene.is_some());
    command(&mut world, Action::SkipScene);
    world.step();
    assert_eq!(error(&world), None);
    let cinema = &mission(&world).cinema;
    assert_eq!(cinema.scene, None);
    assert!(!cinema.on);
    assert_eq!(cinema.skipped, Some(30));
    assert!(cinema.animations.is_empty(), "the salute was skipped");
    assert_eq!(mission(&world).dialogue, None, "the line was dropped");
    assert_eq!(variable(&world, "ended"), boolean(true));
    assert!(variable(&world, "arrived").is_some());
    // The walk ended at the gate, facing the guard; the camera cut back.
    let hero = entity(&world, "hero").unwrap();
    assert_eq!(
        hero.position,
        Milli {
            x: 70_000,
            y: 60_000
        }
    );
    assert_eq!(hero.rotation, 0);
    assert!(hero.unit.as_ref().unwrap().order.is_none());
    let shot = cinema.shot.clone().unwrap();
    assert_eq!((shot.to.camera.clone(), shot.ticks), (None, 0));
    // The game goes on from there.
    steps(&mut world, 40);
    assert_eq!(
        entity(&world, "hero").unwrap().position,
        Milli {
            x: 70_000,
            y: 60_000
        }
    );
    command(&mut world, Action::Ping { position: None });
    world.step();
    assert_eq!(refusals(&world), 0);
}

#[test]
fn only_skippable_scenes_skip() {
    let script = INTRO.replace(
        r#"mission.play("intro")"#,
        r#"mission.play("intro", false)"#,
    );
    let mut world = cameras(Mission::new(&script)).world();
    steps(&mut world, 10);
    command(&mut world, Action::SkipScene);
    steps(&mut world, 5);
    let play = mission(&world).cinema.scene.clone().expect("still playing");
    assert!(!play.skipping);
    assert_eq!(variable(&world, "ended"), None);
}

#[test]
fn scenes_refuse_what_they_cannot_do() {
    let run = |body: &str| {
        let script =
            format!("function on_start() mission.play('s') end\nfunction scenes.s()\n{body}\nend");
        let mut world = cameras(Mission::new(&script)).world();
        steps(&mut world, 5);
        error(&world).map(str::to_string)
    };
    assert_eq!(run("mission.cinematic(true)"), None);
    assert!(
        run("mission.camera_to('nowhere')")
            .unwrap()
            .ends_with("Unknown camera: nowhere")
    );
    assert!(
        run("mission.camera_pan(10, 10)")
            .unwrap()
            .contains("camera_to first")
    );
    assert!(
        run("mission.play('again')")
            .unwrap()
            .ends_with("A scene is already playing")
    );
    assert!(
        run("mission.fade_out(1, 'red')")
            .unwrap()
            .contains("Fade colour")
    );
    assert!(
        run("mission.animate('hall', 'idle')")
            .unwrap()
            .ends_with("Only units play animations")
    );
    assert!(
        run("mission.speak('sleeper', 'Hi')")
            .unwrap()
            .ends_with("The speaker must be a living unit")
    );
    let mut world = world("function on_start() mission.wait(1) end");
    world.step();
    assert!(
        error(&world)
            .unwrap()
            .ends_with("mission.wait works only inside a scene")
    );
    let mut world = world_without_scene();
    world.step();
    assert_eq!(error(&world), Some("scenes.missing must be a function"));
}

fn world_without_scene() -> World {
    world("function on_start() mission.play('missing') end")
}

#[test]
fn a_script_error_hands_the_game_back() {
    let script = r#"
function on_start() mission.play("s") end
function on_tick() if mission.tick() == 8 then mission.alive("nobody") end end
function scenes.s() mission.cinematic(true) mission.fade_out(0) mission.wait(60) end"#;
    let mut world = Mission::new(script).world();
    world.step();
    assert!(mission(&world).cinema.on);
    steps(&mut world, 10);
    assert!(error(&world).is_some());
    assert!(mission(&world).cinema.is_empty());
    command(&mut world, Action::Ping { position: None });
    world.step();
    assert_eq!(refusals(&world), 0);
}

#[test]
fn replayed_statements_do_not_spend_the_call_budget() {
    // 200 statements before a long wait, replayed every run.
    let script = r#"
function on_start() mission.play("s") end
function scenes.s()
  for i = 1, 200 do mission.set("v", i) end
  mission.wait(5)
  mission.set("done", true)
end"#;
    let mut world = Mission::new(script).world();
    steps(&mut world, 240);
    assert_eq!(error(&world), None);
    assert_eq!(variable(&world, "v"), int(200));
    assert_eq!(variable(&world, "done"), boolean(true));
}

#[test]
fn a_scene_does_not_hang_on_a_walk_that_cannot_go_on() {
    // A crowd the scene holds still stands all round the hero: its walk can never start.
    let script = r#"
function on_start() mission.play("s") end
function scenes.s()
  mission.cinematic(true)
  mission.move("hero", 80, 60)
  mission.wait_arrived("hero")
  mission.cinematic(false)
  mission.set("over", mission.tick())
end"#;
    let mut crowded = Mission::new(script);
    crowded.entities.retain(|e| e["id"] == "hero");
    let around = (-2..=2).flat_map(|dx| (-2..=2).map(move |dy| (dx, dy)));
    for (i, (dx, dy)) in around.filter(|&d| d != (0, 0)).enumerate() {
        crowded = crowded.entity(unit(
            &format!("crowd.{i}"),
            "unit.warrior",
            f64::from(60 + dx),
            f64::from(60 + dy),
        ));
    }
    let mut world = crowded.world();
    let taken = until(&mut world, 2400, |w| variable(w, "over").is_some());
    assert_eq!(error(&world), None);
    assert!(taken > 100, "the walk is given its time first: {taken}");
    assert!(!mission(&world).cinema.on);
}

fn snapshot(world: &World) -> String {
    serde_json::to_string(&world.save()).unwrap()
}

#[test]
fn scenes_are_deterministic_and_survive_save_and_restore() {
    let fixture = intro();
    let play = |skip_at: Option<u32>| {
        let mut world = fixture.world();
        let mut trace = Vec::new();
        for tick in 0..700 {
            if Some(tick) == skip_at {
                command(&mut world, Action::SkipScene);
            }
            world.step();
            trace.push(snapshot(&world));
        }
        (world, trace)
    };
    let (reference, trace) = play(None);
    assert_eq!(error(&reference), None);
    assert_eq!(variable(&reference, "ended"), boolean(true));
    assert_eq!(play(None).1, trace);
    let (skipped, skipped_trace) = play(Some(50));
    assert_eq!(variable(&skipped, "ended"), boolean(true));
    assert_eq!(play(Some(50)).1, skipped_trace);

    // Mid-blend (10), mid-walk (150) and mid-line: each resumes as if never saved.
    let spoke = trace
        .iter()
        .position(|s| s.contains("Open the gate."))
        .expect("the line") as u32
        + 10;
    for at in [10, 150, spoke] {
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
fn a_wait_ends_the_run_without_failing_it() {
    let mut calls = Vec::new();
    struct Host<'a>(&'a mut Vec<String>);
    impl sandbox::Host for Host<'_> {
        fn call(
            &mut self,
            name: &'static str,
            _: Vec<sandbox::Scalar>,
        ) -> Result<Vec<sandbox::Scalar>, String> {
            self.0.push(name.to_string());
            if name == "wait" {
                Err(sandbox::SUSPEND.into())
            } else {
                Ok(Vec::new())
            }
        }
        fn scene(&mut self) -> Option<String> {
            Some("s".into())
        }
    }
    let ran = sandbox::execute(
        "function scenes.s() mission.a() mission.wait() mission.b() end",
        None,
        &["a", "wait", "b"],
        &mut Host(&mut calls),
    )
    .unwrap();
    assert_eq!(ran.scene, Some(false));
    assert_eq!(calls, ["a", "wait"]);
}

#[test]
fn clicks_on_placed_units_reach_the_script() {
    let script = r#"
function on_tick()
  local n = mission.clicks("volunteer")
  mission.set("clicks", n)
  if n >= 3 and not mission.get("said") then
    mission.set("said", true)
    mission.say("Volunteer", "unit.settler", "Stop poking me!", 2)
  end
end"#;
    let fixture = Mission::new(script);
    let mut world = fixture.world();
    let volunteer = entity(&world, "volunteer").unwrap().id.0;
    world.step();
    for _ in 0..3 {
        command(&mut world, Action::Clicked { actor: volunteer });
        world.step();
    }
    steps(&mut world, 4);
    assert_eq!(variable(&world, "clicks"), int(3));
    assert_eq!(variable(&world, "said"), boolean(true));
    assert_eq!(mission(&world).clicks.get("volunteer"), Some(&3));
    // Saved with the mission; clicks during a cinematic do not count.
    let again = reload(&world, &fixture);
    assert_eq!(mission(&again).clicks.get("volunteer"), Some(&3));
    let mut world = started("function on_start() mission.cinematic(true) end");
    let volunteer = entity(&world, "volunteer").unwrap().id.0;
    command(&mut world, Action::Clicked { actor: volunteer });
    world.step();
    assert!(mission(&world).clicks.is_empty());
}

/// A bandit beats the hero at the gate; at tick 120 a scene turns cinematic mode on for
/// five seconds, in which the script walks the guard away.
const HOLD: &str = r#"
function on_tick()
  if mission.tick() >= 120 and not mission.get("began") then
    mission.set("began", mission.tick())
    mission.play("hold")
  end
end

function scenes.hold()
  mission.cinematic(true)
  mission.wait(1)
  mission.move("guard", 52, 56)
  mission.wait(4)
  mission.cinematic(false)
  mission.set("over", mission.tick())
end
"#;

fn brawl() -> Mission {
    let mut mission = Mission::new(HOLD)
        .entity(json!({"id": "raider", "definition": "unit.brute", "position": {"x": 62, "y": 60}, "owner": "none"}))
        .camp(json!({"id": "camp.gate", "members": ["raider"], "home": {"x": 62, "y": 60}, "aggroRange": 6, "leash": 12, "aggression": "players"}));
    mission.entities[1]["position"] = json!({"x": 52, "y": 62});
    mission
}

fn hp(world: &World, placement: &str) -> u32 {
    entity(world, placement).and_then(|e| e.hp).unwrap_or(0)
}

#[test]
fn cinematic_mode_holds_the_battle_and_the_cast_still_acts() {
    let mut world = brawl().world();
    steps(&mut world, 120);
    assert_eq!(error(&world), None);
    let (hero, raider) = (hp(&world, "hero"), hp(&world, "raider"));
    assert!(hero < 100, "the raider fights the hero: {hero} {raider}");
    // The scene starts this tick; a swing already released may still land.
    steps(&mut world, 12);
    assert!(mission(&world).cinema.on);
    let (hero, raider) = (hp(&world, "hero"), hp(&world, "raider"));
    let hero_at = entity(&world, "hero").unwrap().position;
    // A second of stillness: nobody the script does not direct fights or moves.
    steps(&mut world, 28);
    assert_eq!(hp(&world, "hero"), hero);
    assert_eq!(hp(&world, "raider"), raider);
    assert_eq!(entity(&world, "hero").unwrap().position, hero_at);
    // The guard, sent by the script, walks; the raider still holds its swing.
    let guard_at = entity(&world, "guard").unwrap().position;
    steps(&mut world, 40);
    assert_ne!(entity(&world, "guard").unwrap().position, guard_at);
    assert_eq!(mission(&world).cinema.actors, ["guard"]);
    assert_eq!((hp(&world, "hero"), hp(&world, "raider")), (hero, raider));
    // When the mode ends, the cast is dismissed and the fight goes on by itself.
    until(&mut world, 200, |w| variable(w, "over").is_some());
    assert!(mission(&world).cinema.actors.is_empty());
    until(&mut world, 200, |w| hp(w, "hero") < hero);
}

#[test]
fn a_skipped_hold_hands_the_battle_back() {
    let mut world = brawl().world();
    steps(&mut world, 130);
    assert!(mission(&world).cinema.on);
    let hero = hp(&world, "hero");
    command(&mut world, Action::SkipScene);
    world.step();
    let cinema = &mission(&world).cinema;
    assert!(!cinema.on && cinema.actors.is_empty() && cinema.scene.is_none());
    // The guard stands where the scene sent it, and the fight resumes.
    assert_eq!(
        entity(&world, "guard").unwrap().position,
        Milli {
            x: 52_000,
            y: 56_000
        }
    );
    until(&mut world, 200, |w| hp(w, "hero") < hero);
}

#[test]
fn a_held_battle_is_deterministic_across_saves() {
    let fixture = brawl();
    let mut reference = fixture.world();
    let mut trace = Vec::new();
    for _ in 0..500 {
        reference.step();
        trace.push(snapshot(&reference));
    }
    for at in [125, 170, 260] {
        let mut first = fixture.world();
        steps(&mut first, at);
        let mut resumed = reload(&first, &fixture);
        for expected in &trace[at as usize..] {
            resumed.step();
            assert_eq!(&snapshot(&resumed), expected, "resumed at {at}");
        }
    }
}

/// The hero walks to the gate and speaks over the guard's shoulder; a single on the
/// guard answers; the scene ends on a wide shot of the gate's region and hands the
/// camera back where that shot looked.
const PRESETS: &str = r#"
function on_start()
  mission.play("talk")
end

function scenes.talk()
  mission.cinematic(true)
  mission.shot("wide", "hero", { distance = 30, seconds = 2 })
  mission.move("hero", 66, 60)
  mission.wait_arrived("hero")
  mission.shot("over", "guard", "hero", { side = "left", fov = 28 })
  mission.speak("hero", "Open the gate.", 1)
  mission.shot("single", "guard", { height = 1.2, roll = -2 })
  mission.speak("guard", "Who asks?", 1)
  mission.shot("two", "hero", "guard")
  mission.shot("wide", "gate", { seconds = 3 })
  mission.wait(1)
  mission.cinematic(false)
  mission.camera_reset(1)
  mission.set("ended", true)
end
"#;

fn preset(world: &World) -> Option<canopy_sim::mission::cinema::Preset> {
    mission(world).cinema.shot.as_ref()?.to.preset.clone()
}

/// Presets keep the actors and what the scene asked for, not a camera: the picture
/// frames the actors wherever their walk left them.
#[test]
fn shot_presets_frame_actors_wherever_they_stand() {
    use canopy_sim::mission::cinema::{Pose, PresetKind, Side};
    let mut world = Mission::new(PRESETS).world();
    world.step();
    assert_eq!(error(&world), None);
    // A wide shot on the hero, blended in, at the distance asked for.
    let wide = preset(&world).expect("a preset");
    assert_eq!(wide.kind, PresetKind::Wide);
    assert_eq!(wide.actors, ["hero"]);
    assert_eq!(wide.distance, Some(30_000));
    assert_eq!(mission(&world).cinema.shot.as_ref().unwrap().ticks, 80);

    // Over the guard's left shoulder onto the hero, cut.
    until(&mut world, 2000, |w| {
        preset(w).is_some_and(|p| p.kind == PresetKind::Over)
    });
    let over = preset(&world).unwrap();
    assert_eq!(over.actors, ["guard", "hero"]);
    assert_eq!((over.side, over.fov), (Some(Side::Left), Some(28_000)));
    assert_eq!(mission(&world).cinema.shot.as_ref().unwrap().ticks, 0);
    until(&mut world, 400, |w| {
        preset(w).is_some_and(|p| p.kind == PresetKind::Single)
    });
    let single = preset(&world).unwrap();
    assert_eq!((single.height, single.roll), (Some(1_200), Some(-2_000)));
    // The region's wide shot keeps its centre and radius.
    until(&mut world, 400, |w| {
        preset(w).is_some_and(|p| p.kind == PresetKind::Wide && p.actors.is_empty())
    });
    let region = preset(&world).unwrap().region.expect("a region");
    assert_eq!(
        region,
        (
            Milli {
                x: 60_000,
                y: 64_000
            },
            4_000
        )
    );
    // Back to the game's camera over where the gate's shot looked.
    until(&mut world, 400, |w| variable(w, "ended").is_some());
    let shot = mission(&world).cinema.shot.clone().unwrap();
    assert_eq!(
        shot.to,
        Pose {
            target: Some(Milli {
                x: 60_000,
                y: 64_000
            }),
            ..Pose::default()
        }
    );
}

#[test]
fn a_skip_cuts_past_presets_and_presets_resume_alike() {
    let fixture = Mission::new(PRESETS);
    let mut world = fixture.world();
    steps(&mut world, 20);
    command(&mut world, Action::SkipScene);
    world.step();
    assert_eq!(error(&world), None);
    assert_eq!(variable(&world, "ended"), boolean(true));
    let shot = mission(&world).cinema.shot.clone().unwrap();
    assert_eq!(shot.ticks, 0, "a skip cuts");
    assert_eq!(shot.to.preset, None, "the camera is the player's again");

    // Saved with a preset in place, a world resumes as if never saved.
    let mut reference = fixture.world();
    let mut trace = Vec::new();
    for _ in 0..400 {
        reference.step();
        trace.push(snapshot(&reference));
    }
    assert!(trace.iter().any(|s| s.contains("\"preset\"")));
    for at in [5, 200] {
        let mut first = fixture.world();
        steps(&mut first, at);
        let mut resumed = reload(&first, &fixture);
        for expected in &trace[at as usize..] {
            resumed.step();
            assert_eq!(&snapshot(&resumed), expected, "resumed at {at}");
        }
    }
}

#[test]
fn shot_presets_refuse_what_they_cannot_frame() {
    let run = |body: &str| {
        let script =
            format!("function on_start() mission.play('s') end\nfunction scenes.s()\n{body}\nend");
        let mut world = Mission::new(&script).world();
        steps(&mut world, 3);
        error(&world).map(str::to_string)
    };
    assert_eq!(run("mission.shot('single', 'hero')"), None);
    assert_eq!(
        run("mission.shot('over', 'hero', 'guard', {side = 'right'})"),
        None
    );
    for (body, refusal) in [
        ("mission.shot('close', 'hero')", "single, over, two or wide"),
        (
            "mission.shot('single', 'hall')",
            "A shot frames units: hall",
        ),
        (
            "mission.shot('single', 'nobody')",
            "Unknown entity ID: nobody",
        ),
        ("mission.shot('two', 'hero', 'hero')", "frames two units"),
        (
            "mission.shot('single', 'hero', {zoom = 2})",
            "Unknown shot option: zoom",
        ),
        (
            "mission.shot('single', 'hero', {side = 'up'})",
            "left or right",
        ),
        (
            "mission.shot('single', 'hero', {fov = 5})",
            "Shot fov must be",
        ),
        (
            "mission.shot('single', 'hero', 3)",
            "Shot options are a table",
        ),
        (
            "mission.shot('single', 'hero', {seconds = -1})",
            "Expected a number",
        ),
    ] {
        let error = run(body).unwrap_or_default();
        assert!(error.contains(refusal), "{body}: {error}");
    }
}
