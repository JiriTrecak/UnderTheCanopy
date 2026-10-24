//! Scene staging (docs/campaign.md "Scenes"): poses a scene holds until an order, effects
//! and lights at points, the script's stock, and the bounty a kill pays.

mod mission_fixture;

use canopy_content::placement::Owner;
use canopy_sim::{Action, Queued, World, action::CellPoint, entity::Milli};
use mission_fixture::*;
use serde_json::json;

fn animations(world: &World) -> Vec<(String, String)> {
    mission(world)
        .cinema
        .animations
        .iter()
        .map(|a| (a.placement.clone(), a.name.clone()))
        .collect()
}

fn posed(world: &World, placement: &str) -> bool {
    animations(world).iter().any(|(p, _)| p == placement)
}

fn amber(world: &World) -> u32 {
    world
        .state()
        .wallets
        .get(&Owner::Player(0))
        .and_then(|w| w.get("item.amber"))
        .copied()
        .unwrap_or(0)
}

/// Runs until the mission's tick reaches `tick` (the script runs every four ticks).
fn to(world: &mut World, tick: u32) {
    while world.state().tick < tick {
        world.step();
    }
}

#[test]
fn a_held_pose_lasts_until_the_unit_is_ordered() {
    let mut world = Mission::new(
        r#"
function on_start()
  mission.animate("guard", "kneel", true)
  mission.animate("scout", "kneel", true)
  mission.animate("statue", "sleep", true)
end

function on_tick()
  local t = mission.tick()
  if t == 1600 then mission.move("guard", 70, 60) end
  if t == 1700 then
    -- An animation after the order in the same run stands: it plays once the walk ends.
    mission.move("statue", 66, 61)
  end
end
"#,
    )
    .entity(unit("scout", "unit.scout", 58.0, 62.0))
    .world();
    // Held well past the memory of a one-off animation (30 s).
    to(&mut world, 1500);
    assert_eq!(error(&world), None);
    assert!(posed(&world, "guard") && posed(&world, "scout") && posed(&world, "statue"));

    // The script's order ends the guard's pose; the others hold theirs.
    to(&mut world, 1601);
    assert!(!posed(&world, "guard"), "{:?}", animations(&world));
    assert!(posed(&world, "scout") && posed(&world, "statue"));

    // A player's order ends the scout's.
    let scout = entity(&world, "scout").unwrap().id.0;
    let tick = world.tick();
    world
        .enqueue(Queued {
            tick,
            player: 0,
            seq: 0,
            action: Action::Move {
                actors: vec![scout],
                destination: CellPoint {
                    x: 62,
                    y: 66,
                    surface: None,
                },
                attack_move: None,
                append: None,
            },
        })
        .unwrap();
    world.step();
    assert!(!posed(&world, "scout"));
    assert!(
        posed(&world, "statue"),
        "a unit nobody ordered keeps its pose"
    );
}

#[test]
fn an_animation_given_after_an_order_in_the_same_run_stays() {
    let mut world = started(
        r#"
function on_start()
  mission.move("guard", 64, 60)
  mission.animate("guard", "kneel", true)
end
"#,
    );
    steps(&mut world, 200);
    assert_eq!(error(&world), None);
    assert!(posed(&world, "guard"), "it kneels where the walk ends");
}

const LIGHTS: &str = r##"
function on_start()
  mission.light_at("dawn", 60, 60, "#8a5cff", 20, 12, 2)
  mission.effect_at("hum", "effect.glow", 60, 60)
  mission.effect_at("puff", "effect.burst", 61, 60)
end

function on_tick()
  if mission.tick() == 120 then
    mission.light_at("dawn", 60, 60, "#8a5cff", 0, 12, 1)
    mission.stop_effect("hum")
  end
end
"##;

#[test]
fn lights_ramp_and_effects_play_at_points_until_they_end() {
    let mut world = started(LIGHTS);
    let cinema = &mission(&world).cinema;
    let dawn = cinema.lights[0].clone();
    assert_eq!(
        (
            dawn.key.as_str(),
            dawn.color,
            dawn.radius,
            dawn.from,
            dawn.to
        ),
        ("dawn", [0x8a, 0x5c, 0xff], 12_000, 0, 20_000)
    );
    assert_eq!(
        dawn.position,
        Milli {
            x: 60_000,
            y: 60_000
        }
    );
    // Half way up after a second, full after two, exact in the simulation.
    assert_eq!(dawn.at(40), 10_000);
    assert_eq!(dawn.at(500), 20_000);
    let keys: Vec<&str> = cinema.effects.iter().map(|e| e.key.as_str()).collect();
    assert_eq!(keys, ["hum", "puff"]);
    assert_eq!(cinema.effects[0].until, None, "a looping effect is held");
    assert!(
        cinema.effects[1].until.is_some(),
        "a burst plays its length"
    );
    // The picture follows the saved state.
    let view = world.observer_view();
    assert_eq!(view.mission.as_ref().unwrap().cinema.lights.len(), 1);

    let keys = |world: &World| -> Vec<String> {
        mission(world)
            .cinema
            .effects
            .iter()
            .map(|e| e.key.clone())
            .collect()
    };
    // Down again from where it stood, then gone; the glow stopped.
    to(&mut world, 121);
    assert_eq!(keys(&world), ["puff"]);
    let dusk = mission(&world).cinema.lights[0].clone();
    assert_eq!((dusk.from, dusk.to), (20_000, 0));
    to(&mut world, 180);
    assert!(mission(&world).cinema.lights.is_empty());
    // The burst is forgotten once played (its own length and its particles' tails).
    to(&mut world, 440);
    assert!(keys(&world).is_empty());
    assert_eq!(error(&world), None);
}

#[test]
fn a_skipped_scene_leaves_its_light_risen() {
    let mut world = started(
        r#"
function on_start() mission.play("rise") end

function scenes.rise()
  mission.wait(1)
  mission.light_at("glow", 60, 60, "white", 8, 6, 10)
  mission.wait(20)
  mission.set("done", true)
end
"#,
    );
    steps(&mut world, 8);
    let tick = world.tick();
    world
        .enqueue(Queued {
            tick,
            player: 0,
            seq: 0,
            action: Action::SkipScene,
        })
        .unwrap();
    // As a fade does, the rise jumps to its end.
    world.step();
    assert_eq!(variable(&world, "done"), boolean(true));
    let glow = mission(&world).cinema.lights[0].clone();
    assert_eq!((glow.ticks, glow.at(world.state().tick)), (0, 8000));
}

#[test]
fn scripts_name_effects_that_exist() {
    let mut world =
        world(r#"function on_start() mission.effect_at("x", "effect.none", 60, 60) end"#);
    world.step();
    assert!(error(&world).is_some_and(|e| e.contains("Unknown effect")));
}

#[test]
fn a_script_gives_and_takes_stock_but_never_below_nothing() {
    let mut world = started(
        r#"
function on_start()
  mission.set("before", mission.stock("player.1", "item.amber"))
  mission.give_stock("player.1", "item.amber", 40)
end

function on_tick()
  if mission.tick() == 4 then
    mission.set("given", mission.stock("player.1", "item.amber"))
    mission.give_stock("player.1", "item.amber", -100000)
  elseif mission.tick() == 8 then
    mission.set("taken", mission.stock("player.1", "item.amber"))
  end
end
"#,
    );
    steps(&mut world, 12);
    assert_eq!(error(&world), None);
    let before = match variable(&world, "before") {
        Some(canopy_sim::mission::state::Variable::Integer(n)) => n,
        other => panic!("{other:?}"),
    };
    assert_eq!(variable(&world, "given"), int(before + 40));
    assert_eq!(variable(&world, "taken"), int(0));
    assert_eq!(amber(&world), 0);

    let mut world = mission_fixture::world(
        r#"function on_start() mission.give_stock("player.1", "item.wood", 5) end"#,
    );
    world.step();
    assert!(error(&world).is_some_and(|e| e.contains("currency")));
}

#[test]
fn a_kill_pays_the_killers_owner_its_bounty() {
    let mission = Mission::new(
        r#"
function on_start() mission.attack("hunter", "cart") end
"#,
    )
    .entity(unit("hunter", "unit.hunter", 62.0, 64.0))
    .entity(json!({"id": "cart", "definition": "unit.cart", "position": {"x": 64, "y": 64}, "owner": "none"}))
    .camp(json!({"id": "camp.road", "members": ["cart"], "home": {"x": 64, "y": 64}, "aggroRange": 6, "leash": 12, "aggression": "players"}));
    let mut world = mission.world();
    let start = amber(&world);
    let mut paid = None;
    for _ in 0..2000 {
        world.step();
        if entity(&world, "cart").is_none() {
            paid = Some(world.observer_view().deliveries.clone());
            break;
        }
    }
    let deliveries = paid.expect("the hunter breaks the cart");
    assert_eq!(error(&world), None);
    assert_eq!(amber(&world), start + 30);
    let bounty = deliveries
        .iter()
        .find(|d| d.amount == 30)
        .expect("the bounty rises where the cart fell");
    assert_eq!(
        (bounty.owner, bounty.item.as_str(), bounty.carrier),
        (Owner::Player(0), "item.amber", None)
    );
    assert!(
        world
            .state()
            .facts
            .iter()
            .any(|f| f.message == "Bounty" && f.amount == Some(30))
    );
}

#[test]
fn a_death_nobody_caused_pays_no_bounty() {
    let mission = Mission::new(r#"function on_start() mission.damage("cart", 50) end"#)
        .entity(json!({"id": "cart", "definition": "unit.cart", "position": {"x": 64, "y": 64}, "owner": "none"}))
        .camp(json!({"id": "camp.road", "members": ["cart"], "home": {"x": 64, "y": 64}, "aggroRange": 6, "leash": 12, "aggression": "players"}));
    let mut world = mission.world();
    let start = amber(&world);
    steps(&mut world, 10);
    assert!(entity(&world, "cart").is_none());
    assert_eq!(amber(&world), start);
}

/// A unit asleep in a scene pose (`animate(unit, "sleep", true)`) picks no fight of its
/// own, so an easter egg dozing in its flower stays there when enemies pass; awake, the
/// same unit goes for the enemy in its aggro range, and an order or a wound wakes it.
#[test]
fn a_sleeping_unit_picks_no_fight_until_woken() {
    let mission = |pose: &str| {
        Mission::new(&format!("function on_start() {pose} end"))
            .entity(unit("sentry", "unit.soldier", 63.0, 70.0))
            // A camp creature inside the sentry's aggro range that stays home.
            .entity(json!({"id": "bandit", "definition": "unit.bandit", "position": {"x": 67, "y": 70}, "owner": "none"}))
            .camp(json!({"id": "camp.bandit", "members": ["bandit"], "home": {"x": 67, "y": 70}, "aggroRange": 1, "leash": 4, "aggression": "players"}))
    };
    let x = |world: &World| entity(world, "sentry").expect("the sentry").position.x;
    let mut awake = mission("").world();
    steps(&mut awake, 200);
    assert!(
        x(&awake) > 64_000,
        "awake, it goes for the enemy: {}",
        x(&awake)
    );

    let asleep = || {
        let mut world = mission(r#"mission.animate("sentry", "sleep", true)"#).world();
        steps(&mut world, 200);
        assert_eq!(error(&world), None);
        assert_eq!(x(&world), 63_000, "asleep, it stays");
        world
    };
    let mut ordered = asleep();
    let sentry = entity(&ordered, "sentry").unwrap().id.0;
    let tick = ordered.tick();
    ordered
        .enqueue(Queued {
            tick,
            player: 0,
            seq: 0,
            action: Action::Move {
                actors: vec![sentry],
                destination: CellPoint {
                    x: 63,
                    y: 72,
                    surface: None,
                },
                attack_move: Some(true),
                append: None,
            },
        })
        .unwrap();
    steps(&mut ordered, 300);
    assert!(x(&ordered) > 64_000, "an order wakes it: {}", x(&ordered));

    let mut hurt = asleep();
    let id = entity(&hurt, "sentry").unwrap().id;
    hurt.state_mut().entities.get_mut(id).unwrap().hp = Some(90);
    steps(&mut hurt, 300);
    assert!(x(&hurt) > 64_000, "a wound wakes it: {}", x(&hurt));
}
