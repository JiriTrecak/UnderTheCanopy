//! The `mission` API, function by function.

mod mission_fixture;

use canopy_content::{Id, placement::Owner};
use canopy_sim::{
    World,
    entity::{Entity, Milli},
    mission::{
        self, Current, MissionEffect, Systems,
        state::{CameraMode, ObjectiveState, Variable},
    },
    state::State,
};
use mission_fixture::*;
use serde_json::json;

/// Runs `body` as `on_start` and returns the world after the run.
fn on_start(body: &str) -> World {
    started(&format!("function on_start()\n{body}\nend"))
}

/// The error `body` stops the script with.
fn fails(body: &str) -> String {
    let mut world = world(&format!("function on_start()\n{body}\nend"));
    world.step();
    let message = error(&world)
        .unwrap_or_else(|| panic!("{body} succeeded"))
        .to_string();
    let prefix = "[string \"mission.lua\"]:2: ";
    message.strip_prefix(prefix).unwrap_or(&message).to_string()
}

fn effects(world: &World) -> Vec<MissionEffect> {
    world.mission().unwrap().effects().to_vec()
}

fn id(world: &World, placement: &str) -> canopy_sim::entity::EntityId {
    entity(world, placement).unwrap().id
}

#[test]
fn tick_and_variables() {
    let world = on_start(
        r#"mission.set("int", 3) mission.set("float", 2.5) mission.set("text", "é")
           mission.set("yes", true) mission.set("gone", 1) mission.set("gone", nil)
           mission.set("unset", 1) mission.set("unset")
           mission.set("copy", mission.get("text") .. mission.get("int") .. mission.tick())
           mission.set("missing", mission.get("nothing") == nil)"#,
    );
    assert_eq!(variable(&world, "int"), int(3));
    assert_eq!(variable(&world, "float"), Some(Variable::Number(2.5)));
    assert_eq!(variable(&world, "yes"), boolean(true));
    assert_eq!(variable(&world, "gone"), None);
    assert_eq!(variable(&world, "unset"), None);
    // Integers stay integers: the old runtime returned every number as a float ("é30.0").
    assert_eq!(variable(&world, "copy"), string("é30"));
    assert_eq!(variable(&world, "missing"), boolean(true));
    assert_eq!(fails("mission.set('', 1)"), "Expected a nonempty string");
    assert_eq!(
        fails(&format!("mission.get('{}')", "k".repeat(121))),
        "Expected a nonempty string"
    );
}

#[test]
fn count_alive_and_position() {
    let world = on_start(
        r#"mission.set("warriors", mission.count("player.1", "unit.warrior"))
           mission.set("halls", mission.count("player.1", "building.hall"))
           mission.set("neutral", mission.count("none", "unit.settler"))
           mission.set("alive", mission.alive("guard"))
           local x, y = mission.position("hero") mission.set("hero", x .. "," .. y)
           local hx, hy = mission.position("hall") mission.set("hall", hx .. "," .. hy)
           local sx, sy = mission.position("sleeper") mission.set("unspawned", sx == nil and sy == nil)
           mission.set("dead", mission.alive("sleeper"))"#,
    );
    assert_eq!(variable(&world, "warriors"), int(1));
    assert_eq!(variable(&world, "halls"), int(1));
    assert_eq!(variable(&world, "neutral"), int(1));
    assert_eq!(variable(&world, "alive"), boolean(true));
    assert_eq!(variable(&world, "hero"), string("60,60"));
    assert_eq!(variable(&world, "hall"), string("41.5,41.5"));
    assert_eq!(variable(&world, "unspawned"), boolean(true));
    assert_eq!(variable(&world, "dead"), boolean(false));
    assert_eq!(
        fails("mission.count('player.1', 'item.amber')"),
        "Count requires a unit or building definition"
    );
    assert_eq!(
        fails("mission.count('player.9', 'unit.warrior')"),
        "Expected an owner: none or player.1 to player.8"
    );
    assert_eq!(
        fails("mission.alive('missing')"),
        "Unknown entity ID: missing"
    );
}

#[test]
fn stock_reads_wallet_currencies() {
    let world = world(
        "function on_start() mission.set('amber', mission.stock('player.1', 'item.amber')) end",
    );
    let mut state = world.state().clone();
    state
        .wallets
        .entry(Owner::Player(0))
        .or_default()
        .insert(Id::new("item.amber").unwrap(), 42);
    run(&world, &mut state, &Current);
    assert_eq!(variables(&state)["amber"], Variable::Integer(42));
    assert_eq!(
        fails("mission.stock('player.1', 'item.wood')"),
        "Stock requires an economy currency item"
    );
}

#[test]
fn regions_and_arrival() {
    let world = on_start(
        r#"mission.set("hero", mission.in_region("hero", "gate"))
           mission.set("guard", mission.in_region("guard", "gate"))
           mission.set("here", mission.arrived("hero", 60, 60))
           mission.set("near", mission.arrived("hero", 60.4, 60))
           mission.set("there", mission.arrived("hero", 61, 60))
           mission.set("hall", mission.arrived("hall", 40.5, 40.5))"#,
    );
    assert_eq!(variable(&world, "hero"), boolean(true), "on the edge");
    assert_eq!(variable(&world, "guard"), boolean(false));
    assert_eq!(variable(&world, "here"), boolean(true));
    assert_eq!(variable(&world, "near"), boolean(true));
    assert_eq!(variable(&world, "there"), boolean(false));
    assert_eq!(
        variable(&world, "hall"),
        boolean(false),
        "only units arrive"
    );
    assert_eq!(
        fails("mission.in_region('hero', 'moat')"),
        "Unknown region: moat"
    );
    assert_eq!(
        fails("mission.arrived('hero', 256, 0)"),
        "Expected a number in 0..255"
    );
}

#[test]
fn facing_turns_units_every_tick() {
    let script = r#"
function on_start() mission.face("hero", 70, 60) mission.set("done", mission.facing_done()) end
function on_tick() mission.set("done", mission.facing_done()) end"#;
    let mut world = world(script);
    world.step();
    assert_eq!(
        effects(&world),
        [MissionEffect::Face {
            placement: "hero".into()
        }]
    );
    assert_eq!(variable(&world, "done"), boolean(false));
    // 360°/s is 9° per tick; a quarter turn takes ten ticks.
    steps(&mut world, 9);
    assert_eq!(entity(&world, "hero").unwrap().rotation, 81_000);
    assert_eq!(mission(&world).facing.len(), 1);
    world.step();
    assert_eq!(entity(&world, "hero").unwrap().rotation, 90_000);
    assert!(mission(&world).facing.is_empty());
    steps(&mut world, 2);
    assert_eq!(variable(&world, "done"), boolean(true));
    assert_eq!(fails("mission.face('statue', 1, 1)"), "statue cannot turn");
}

#[test]
fn a_new_facing_replaces_the_old_one() {
    let world = on_start(
        r#"mission.face("hero", 70, 60) mission.face("guard", 1, 1) mission.face("hero", 50, 60)"#,
    );
    let facing = &mission(&world).facing;
    assert_eq!(facing.len(), 2);
    assert_eq!(
        (facing[1].placement.as_str(), facing[1].target),
        (
            "hero",
            Milli {
                x: 50_000,
                y: 60_000
            }
        )
    );
}

#[test]
fn orders_become_effects() {
    let world = on_start(
        r#"mission.move("guard", 70.4, 60.5) mission.attack_move("hero", 1, 2)
           mission.follow("volunteer", "hero") mission.attack("guard", "volunteer")"#,
    );
    assert_eq!(
        effects(&world),
        [
            MissionEffect::Move {
                placement: "guard".into(),
                destination: Milli {
                    x: 70_000,
                    y: 61_000
                },
                attack_move: false,
            },
            MissionEffect::Move {
                placement: "hero".into(),
                destination: Milli { x: 1000, y: 2000 },
                attack_move: true,
            },
            MissionEffect::Follow {
                placement: "volunteer".into(),
                target: "hero".into(),
            },
            MissionEffect::Attack {
                placement: "guard".into(),
                target: "volunteer".into(),
            },
        ]
    );
    assert_eq!(fails("mission.move('statue', 1, 1)"), "statue cannot move");
    assert_eq!(
        fails("mission.move('hero', -1, 1)"),
        "Expected a number in 0..255"
    );
    assert_eq!(
        fails("mission.attack_move('volunteer', 1, 1)"),
        "volunteer cannot attack-move"
    );
    assert_eq!(
        fails("mission.follow('statue', 'hero')"),
        "statue cannot follow"
    );
    assert_eq!(
        fails("mission.attack('hero', 'missing')"),
        "Unknown entity ID: missing"
    );
}

#[test]
fn damage_and_recovery_become_effects() {
    let world = on_start(
        r#"mission.set("hit", mission.damage("guard", 30))
           mission.set("sourced", mission.damage("volunteer", 5, "hero"))
           mission.set("unspawned", mission.damage("sleeper", 5))
           mission.set("by_unspawned", mission.damage("guard", 5, "sleeper"))
           mission.recover("hero")"#,
    );
    assert_eq!(variable(&world, "hit"), boolean(true));
    assert_eq!(variable(&world, "sourced"), boolean(true));
    assert_eq!(variable(&world, "unspawned"), boolean(false));
    assert_eq!(variable(&world, "by_unspawned"), boolean(false));
    assert_eq!(
        effects(&world),
        [
            MissionEffect::Damage {
                target: id(&world, "guard"),
                source: None,
                amount: 30,
            },
            MissionEffect::Damage {
                target: id(&world, "volunteer"),
                source: Some(id(&world, "hero")),
                amount: 5,
            },
            MissionEffect::Recover {
                placement: "hero".into()
            },
        ]
    );
    assert_eq!(
        fails("mission.damage('guard', 2.5)"),
        "Damage must be an integer"
    );
    assert_eq!(
        fails("mission.damage('guard', 0)"),
        "Expected a number in 1..100000"
    );
    assert_eq!(fails("mission.recover('hall')"), "Recovery requires a unit");
    let mut world = mission_fixture::world(&format!(
        "function on_start() for i = 1, {} do mission.damage('guard', 1) end end",
        mission::MAX_DAMAGE + 1
    ));
    world.step();
    assert_eq!(
        error(&world),
        Some("Mission queued more than 64 damage events")
    );
}

fn ambush() -> Mission {
    Mission::new(
        r#"function on_start() mission.spawn("ambusher") mission.move("ambusher", 90, 90)
           mission.set("now", mission.count("none", "unit.bandit")) end
           function on_tick() mission.set("later", mission.count("none", "unit.bandit")) end"#,
    )
    .entity(json!({"id": "ambusher", "definition": "unit.bandit", "position": {"x": 80, "y": 80}, "owner": "none", "activation": "script"}))
    .camp(json!({"id": "camp.woods", "members": ["ambusher"], "home": {"x": 80, "y": 80}, "aggroRange": 6, "leash": 12, "aggression": "players"}))
}

#[test]
fn spawn_creates_scripted_placements_once() {
    let mut world = ambush().world();
    assert!(entity(&world, "ambusher").is_none());
    world.step();
    assert_eq!(error(&world), None);
    let spawned = entity(&world, "ambusher").unwrap();
    assert_eq!(
        spawned.position,
        Milli {
            x: 80_000,
            y: 80_000
        }
    );
    assert_eq!(spawned.ready_tick, 1, "takes part from the next tick");
    assert_eq!(mission(&world).spawned, ["ambusher"]);
    assert_eq!(
        effects(&world),
        [
            MissionEffect::Spawn {
                placement: "ambusher".into(),
                camp: Some("camp.woods".into()),
            },
            MissionEffect::Move {
                placement: "ambusher".into(),
                destination: Milli {
                    x: 90_000,
                    y: 90_000
                },
                attack_move: false,
            },
        ]
    );
    steps(&mut world, 4);
    assert_eq!(
        variable(&world, "now"),
        int(0),
        "the run sees the world as it began"
    );
    assert_eq!(variable(&world, "later"), int(1));

    let twice = ambush();
    for (script, expected) in [
        (
            "mission.spawn('ambusher') mission.spawn('ambusher')",
            "ambusher was already spawned",
        ),
        ("mission.spawn('guard')", "guard is not a scripted spawn"),
    ] {
        let mut fixture = Mission::new(&format!("function on_start()\n{script}\nend"));
        fixture.entities = twice.entities.clone();
        fixture.camps = twice.camps.clone();
        let mut world = fixture.world();
        world.step();
        assert!(error(&world).unwrap().ends_with(expected), "{script}");
    }
    let mut fixture = Mission::new(
        "function on_start() mission.spawn('ambusher') end function on_tick() mission.spawn('ambusher') end",
    );
    fixture.entities = twice.entities;
    let mut world = fixture.world();
    steps(&mut world, 5);
    assert!(
        error(&world)
            .unwrap()
            .ends_with("ambusher was already spawned")
    );
    assert_eq!(
        world
            .state()
            .entities
            .iter()
            .filter(|e| e.placement.as_deref() == Some("ambusher"))
            .count(),
        1
    );
}

#[test]
fn transfer_changes_owners() {
    let world = on_start(r#"mission.transfer("guard", "none")"#);
    assert_eq!(entity(&world, "guard").unwrap().owner, Owner::Nobody);
    let world = on_start(r#"mission.transfer("guard", "player.2")"#);
    assert_eq!(entity(&world, "guard").unwrap().owner, Owner::Player(1));
    assert_eq!(
        fails("mission.transfer('guard', 'player.3')"),
        "Missing owner slot"
    );
    assert_eq!(
        fails("mission.transfer('volunteer', 'player.1')"),
        "Transfer requires a non-camp military unit"
    );
    assert_eq!(
        fails("mission.transfer('hall', 'player.1')"),
        "Transfer requires a non-camp military unit"
    );
}

fn wounded(script: &str) -> Mission {
    let mut fixture = Mission::new(script);
    fixture.entities[2]["initialState"] = json!({"health": 50});
    fixture.camp(json!({"id": "camp.ambush", "members": ["sleeper"], "home": {"x": 64, "y": 62}, "aggroRange": 6, "leash": 12, "aggression": "players"}))
}

#[test]
fn transform_keeps_identity_and_health_fraction() {
    let mut world = wounded(
        "function on_start() mission.set('joined', mission.transform('volunteer', 'unit.brute', 'player.1')) end",
    )
    .world();
    let before = id(&world, "volunteer");
    world.step();
    assert_eq!(error(&world), None);
    assert_eq!(variable(&world, "joined"), boolean(true));
    let volunteer = entity(&world, "volunteer").unwrap();
    assert_eq!(volunteer.id, before);
    assert_eq!(
        world.catalog().id(volunteer.definition).as_str(),
        "unit.brute"
    );
    assert_eq!(volunteer.owner, Owner::Player(0));
    assert_eq!(volunteer.hp, Some(150), "half of 300");

    let mut world = wounded(
        "function on_start() mission.set('ok', mission.transform('volunteer', 'unit.bandit', 'none', 'camp.ambush')) end",
    )
    .world();
    world.step();
    assert_eq!(variable(&world, "ok"), boolean(true));
    let MissionEffect::Transform { owner, camp, .. } = &effects(&world)[0] else {
        panic!("a transform");
    };
    assert_eq!(
        (*owner, camp.as_deref()),
        (Owner::Nobody, Some("camp.ambush"))
    );

    let mut world = Mission::new("function on_start() mission.set('ok', mission.transform('ambusher', 'unit.warrior', 'player.1')) end")
        .entity(json!({"id": "ambusher", "definition": "unit.settler", "position": {"x": 80, "y": 80}, "owner": "none", "activation": "script"}))
        .world();
    world.step();
    assert_eq!(variable(&world, "ok"), boolean(false), "not spawned yet");

    let target = "Transformation target must be an ordinary unit with a matching camp assignment";
    for (call, expected) in [
        ("'volunteer', 'unit.bandit', 'none'", target),
        ("'volunteer', 'unit.marshal', 'player.1'", target),
        ("'volunteer', 'building.hall', 'player.1'", target),
        (
            "'volunteer', 'unit.warrior', 'player.1', 'camp.ambush'",
            "Transformation requires an uncleared neutral camp",
        ),
        (
            "'volunteer', 'unit.bandit', 'none', 'camp.missing'",
            "Transformation requires an uncleared neutral camp",
        ),
        (
            "'hero', 'unit.warrior', 'player.1'",
            "Transformation requires an ordinary uncontained unit",
        ),
        (
            "'volunteer', 'unit.warrior', 'player.5'",
            "Missing owner slot",
        ),
    ] {
        let mut world = wounded(&format!(
            "function on_start()\nmission.transform({call})\nend"
        ))
        .world();
        world.step();
        assert!(
            error(&world).unwrap().ends_with(expected),
            "{call}: {:?}",
            error(&world)
        );
    }
}

#[test]
fn scenes_and_cameras() {
    let world = on_start(r#"mission.begin_scene(12.5, 30)"#);
    let scene = mission(&world).scene.clone().unwrap();
    assert_eq!(
        (scene.position, scene.camera),
        (
            Milli {
                x: 12_500,
                y: 30_000
            },
            None
        )
    );
    let world = on_start(r#"mission.begin_scene(1, 1) mission.end_scene()"#);
    assert_eq!(mission(&world).scene, None);

    let world = on_start(r#"mission.camera("third-person", "hero", "guard", 6, 1.8, 60, 0.5)"#);
    let scene = mission(&world).scene.clone().unwrap();
    assert_eq!(
        scene.position,
        Milli {
            x: 60_000,
            y: 60_000
        }
    );
    let camera = scene.camera.unwrap();
    assert_eq!(camera.mode, CameraMode::ThirdPerson);
    assert_eq!(
        (camera.entity.as_str(), camera.look_at.as_deref()),
        ("hero", Some("guard"))
    );
    assert_eq!(
        (
            camera.distance,
            camera.height,
            camera.fov,
            camera.transition_ms
        ),
        (Some(6000), Some(1800), Some(60_000), 500)
    );
    let world = on_start(r#"mission.camera("rts", "guard")"#);
    let camera = mission(&world).scene.clone().unwrap().camera.unwrap();
    assert_eq!(
        (camera.look_at, camera.distance, camera.transition_ms),
        (None, None, 350)
    );

    assert_eq!(
        fails(r#"mission.camera("first-person", "missing")"#),
        "Unknown entity ID: missing"
    );
    assert_eq!(
        fails(r#"mission.camera("first-person", "hall")"#),
        "Camera subject must be a unit"
    );
    assert_eq!(
        fails(r#"mission.camera("first-person", "hero", "hall")"#),
        "Camera look-at must be a unit"
    );
    assert_eq!(
        fails(r#"mission.camera("orbit", "hero")"#),
        "Camera mode must be rts, third-person or first-person"
    );
    assert_eq!(
        fails(r#"mission.camera("rts", "hero", nil, 6, 1.8, 200)"#),
        "Camera field of view must be a number in 25..90"
    );
    assert_eq!(
        fails(r#"mission.camera("rts", "hero", nil, 6, 0.1)"#),
        "Camera height must be a number in 0.2..30"
    );
    assert_eq!(
        fails(r#"mission.camera("rts", "hero", nil, nil, nil, nil, 6)"#),
        "Expected a number in 0..5"
    );
}

#[test]
fn dialogue_lines() {
    let world = on_start(
        r#"mission.say("Marshal", "unit.marshal", "First.", 1) mission.say("Guard", "unit.warrior", "Second.", 1.5, false, "guard")"#,
    );
    let line = mission(&world).dialogue.clone().unwrap();
    assert_eq!(line.id, 2);
    assert_eq!(mission(&world).next_dialogue, 3);
    assert_eq!(
        (
            line.speaker.as_str(),
            line.portrait.as_str(),
            line.text.as_str()
        ),
        ("Guard", "unit.warrior", "Second.")
    );
    assert_eq!(
        (line.duration_ticks, line.until, line.cinematic),
        (60, 60, false)
    );
    assert_eq!(line.actor, Some(id(&world, "guard")));

    let actor = "Dialogue actor must be a living unit matching the portrait";
    assert_eq!(
        fails(r#"mission.say("M", "unit.marshal", "x", 1, true, "guard")"#),
        actor
    );
    assert_eq!(
        fails(r#"mission.say("M", "unit.marshal", "x", 1, true, "missing")"#),
        "Unknown entity ID: missing"
    );
    assert_eq!(
        fails(r#"mission.say("M", "unit.missing", "x", 1)"#),
        "Portrait must be an entity definition ID"
    );
    assert_eq!(
        fails(r#"mission.say("M", "unit.marshal", "x", 1, "yes")"#),
        "Cinematic must be a boolean"
    );
    assert_eq!(
        fails(r#"mission.say("M", "unit.marshal", "x", 0.5)"#),
        "Expected a number in 1..60"
    );
    assert_eq!(
        fails(r#"mission.say("M", "unit.marshal", "", 1)"#),
        "Expected a nonempty string"
    );
    assert_eq!(
        fails(&format!(
            r#"mission.say("{}", "unit.marshal", "x", 1)"#,
            "m".repeat(81)
        )),
        "Expected a nonempty string"
    );
}

#[test]
fn objectives() {
    let world = on_start(
        r#"mission.begin_objective("reach") mission.begin_objective("loot") mission.complete_objective("reach") mission.begin_objective("hold") mission.fail_objective("loot")"#,
    );
    let states = &mission(&world).objective_states;
    assert_eq!(states["reach"], ObjectiveState::Completed);
    assert_eq!(states["hold"], ObjectiveState::Active);
    assert_eq!(states["loot"], ObjectiveState::Failed);
    assert_eq!(mission(&world).objective, "Hold the gate");
    let world = on_start(r#"mission.begin_objective("reach") mission.objective("Custom text")"#);
    assert_eq!(mission(&world).objective, "Custom text");

    assert_eq!(
        fails(r#"mission.begin_objective("reach") mission.begin_objective("hold")"#),
        "Complete the current primary objective before starting another"
    );
    assert_eq!(
        fails(r#"mission.begin_objective("loot") mission.begin_objective("loot")"#),
        "Objective already started: loot"
    );
    assert_eq!(
        fails(r#"mission.complete_objective("reach")"#),
        "Objective is not active: reach"
    );
    assert_eq!(
        fails(
            r#"mission.begin_objective("reach") mission.fail_objective("reach") mission.complete_objective("reach")"#
        ),
        "Objective is not active: reach"
    );
    assert_eq!(
        fails(r#"mission.begin_objective("escape")"#),
        "Unknown objective: escape"
    );
    assert_eq!(
        fails(&format!(r#"mission.objective("{}")"#, "o".repeat(501))),
        "Expected a nonempty string"
    );
}

#[test]
fn win_and_lose_return_nothing() {
    let world = on_start(r#"mission.set("n", mission.win() == nil)"#);
    assert_eq!(variable(&world, "n"), boolean(true));
    assert_eq!(effects(&world), [MissionEffect::Win]);
}

/// Systems with a two-slot hero inventory, as M2.6 will provide.
struct Equipped(Vec<Option<Id>>);

impl Systems for Equipped {
    fn has_order(&self, entity: &Entity) -> bool {
        Current.has_order(entity)
    }
    fn equipment(&self, entity: &Entity) -> Option<Vec<Option<Id>>> {
        (entity.placement.as_deref() == Some("hero")).then(|| self.0.clone())
    }
    fn contained(&self, entity: &Entity) -> bool {
        Current.contained(entity)
    }
    fn fallen(&self, entity: &Entity) -> bool {
        Current.fallen(entity)
    }
    fn max_hp(&self, catalog: &canopy_sim::catalog::Catalog, entity: &Entity) -> u32 {
        Current.max_hp(catalog, entity)
    }
}

fn run(world: &World, state: &mut State, systems: &dyn Systems) -> Option<Vec<MissionEffect>> {
    let script = world.mission().unwrap().script.clone();
    mission::tick(state, world.catalog(), &script, systems)
}

fn variables(state: &State) -> &std::collections::BTreeMap<String, Variable> {
    &state.mission.as_ref().unwrap().variables
}

#[test]
fn item_exchanges_read_their_own_writes() {
    let script = r#"function on_start()
  mission.set("gave", mission.give_item("hero", "item.charm"))
  mission.set("has", mission.has_item("hero", "item.charm"))
  mission.set("taken", mission.take_item("hero", "item.charm"))
  mission.set("twice", mission.take_item("hero", "item.charm"))
  mission.set("a", mission.give_item("hero", "item.charm"))
  mission.set("b", mission.give_item("hero", "item.charm"))
  mission.set("full", mission.give_item("hero", "item.charm"))
  mission.set("guard", mission.give_item("guard", "item.charm"))
end"#;
    let world = world(script);
    let mut state = world.state().clone();
    let effects = run(&world, &mut state, &Equipped(vec![None, None])).unwrap();
    let vars = variables(&state);
    for (key, value) in [
        ("gave", true),
        ("has", true),
        ("taken", true),
        ("twice", false),
        ("a", true),
        ("b", true),
        ("full", false),
        ("guard", false),
    ] {
        assert_eq!(vars[key], Variable::Boolean(value), "{key}");
    }
    let hero = world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some("hero"))
        .unwrap()
        .id;
    let charm = Some(Id::new("item.charm").unwrap());
    let equip = |slot, item: Option<Id>| MissionEffect::Equip {
        entity: hero,
        slot,
        item,
    };
    assert_eq!(
        effects,
        [
            equip(0, charm.clone()),
            equip(0, None),
            equip(0, charm.clone()),
            equip(1, charm)
        ]
    );

    // Without an inventory system every exchange is refused.
    let mut state = world.state().clone();
    run(&world, &mut state, &Current).unwrap();
    assert_eq!(variables(&state)["gave"], Variable::Boolean(false));

    assert_eq!(
        fails("mission.give_item('hero', 'item.amber')"),
        "Expected a hero inventory item"
    );
    assert_eq!(
        fails("mission.has_item('hero', 3)"),
        "Expected a hero inventory item"
    );
    assert_eq!(
        fails("mission.take_item('missing', 'item.charm')"),
        "Unknown entity ID: missing"
    );
}

#[test]
fn every_function_is_exposed() {
    assert_eq!(mission::NAMES.len(), 34);
    let calls: Vec<String> = mission::NAMES
        .iter()
        .map(|n| format!("mission.{n} ~= nil"))
        .collect();
    let world = on_start(&format!("mission.set('all', {})", calls.join(" and ")));
    assert_eq!(variable(&world, "all"), boolean(true));
}
