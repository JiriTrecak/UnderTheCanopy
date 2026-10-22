//! The campaign AI (docs/campaign.md "Campaign AI"): a mission's computer players from
//! `mission.players`, each behaviour, waves, allies, dormant players, the script API,
//! saves and determinism. Test-owned content (the AI fixture with an altar).

#[path = "ai_fixture/mod.rs"]
mod ai_fixture;

use ai_fixture::*;
use canopy_content::{map::UtcMap, placement::Owner};
use canopy_sim::{
    MatchSetup, Slot, SlotKind, World,
    entity::{Entity, EntityId, Milli},
    orders::Order,
};
use serde_json::{Value, json};

const P3: Owner = Owner::Player(2);
const P4: Owner = Owner::Player(3);

/// Player 1's hall and a soldier in the south-west; player 2's barracks, house and two
/// guards in the north-east.
fn cast() -> Vec<Value> {
    vec![
        place_for("p1-hall", HALL, "player.1", 39.5, 39.5),
        place_for("p1-a", SOLDIER, "player.1", 40.0, 60.0),
        place_for("p2-barracks", BARRACKS, "player.2", 199.5, 199.5),
        place_for("p2-house", HOUSE, "player.2", 213.5, 189.5),
        place_for("p2-g1", SOLDIER, "player.2", 196.0, 188.0),
        place_for("p2-g2", SOLDIER, "player.2", 204.0, 188.0),
    ]
}

fn regions() -> Value {
    json!([
        {"id": "edge", "x": 230, "y": 128, "radius": 6},
        {"id": "west", "x": 40, "y": 128, "radius": 6},
        {"id": "gate", "x": 128, "y": 128, "radius": 4},
    ])
}

fn map(extra: Vec<Value>, players: Value, script: &str) -> UtcMap {
    let mut entities = cast();
    entities.extend(extra);
    let script = if script.is_empty() { "x = 1" } else { script };
    mission_map(entities, regions(), players, json!([]), script)
}

/// A player 2 enemy playing `ai`.
fn enemy(ai: Value) -> Value {
    json!({"player": 2, "team": "enemy", "ai": ai})
}

/// Only player 1 is set up by the caller; the mission adds its computer players.
fn start(map: &UtcMap) -> World {
    let setup = MatchSetup {
        seed: 11,
        slots: vec![Slot {
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    };
    World::new(campaign_content(), map, setup).unwrap_or_else(|e| panic!("{e}"))
}

fn placed<'a>(world: &'a World, placement: &str) -> Option<&'a Entity> {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(placement) && e.hp != Some(0))
}

/// Living units of `owner` and `definition` the AI made (no placement).
fn made(world: &World, owner: Owner, definition: &str) -> Vec<EntityId> {
    world
        .state()
        .entities
        .iter()
        .filter(|e| {
            e.owner == owner
                && e.placement.is_none()
                && e.hp != Some(0)
                && world.catalog().id(e.definition).as_str() == definition
        })
        .map(|e| e.id)
        .collect()
}

fn order(world: &World, id: EntityId) -> Option<Order> {
    world.state().entities.get(id)?.unit.as_ref()?.order.clone()
}

/// The cell an attack-move heads for.
fn heading(world: &World, id: EntityId) -> Option<(i32, i32)> {
    match order(world, id)? {
        Order::Move {
            destination,
            attack_move: true,
        } => Some((destination.x, destination.y)),
        _ => None,
    }
}

fn near(cell: Option<(i32, i32)>, x: i32, y: i32, reach: i32) -> bool {
    cell.is_some_and(|(cx, cy)| (cx - x).abs() <= reach && (cy - y).abs() <= reach)
}

fn cell_of(world: &World, id: EntityId) -> (i32, i32) {
    world
        .state()
        .entities
        .get(id)
        .expect("entity")
        .position
        .cell()
}

#[test]
fn missions_seat_the_players_they_list() {
    // Content without races plays everyone as the rules' default race.
    let race = campaign_content().rules.default_race.clone();
    let players = json!([
        {"player": 2, "team": "enemy", "race": race, "ai": {"behaviour": "passive"}},
        {"player": 3, "team": "ally", "ai": {"behaviour": "base"}},
    ]);
    let world = start(&map(Vec::new(), players, ""));
    let slots: Vec<(u8, SlotKind, Option<u8>)> = world
        .setup()
        .slots
        .iter()
        .map(|s| (s.player, s.kind, s.team))
        .collect();
    assert_eq!(
        slots,
        [
            (0, SlotKind::Human, Some(0)),
            (1, SlotKind::Ai, Some(1)),
            (2, SlotKind::Ai, Some(0)),
        ]
    );
    assert_eq!(world.setup().slots[1].race, Some(race));
}

#[test]
fn waves_are_trained_on_time_and_march_on_the_base() {
    let line = json!({"after": 5, "every": 20, "units": [{"unit": SOLDIER, "count": 2}]});
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "waves", "waves": [line]}))]),
        "",
    ));
    // Due at tick 200; the barracks trains them one after the other (30 ticks each).
    run(&mut world, 229);
    assert!(made(&world, P2, SOLDIER).is_empty());
    run(&mut world, 33);
    let wave = made(&world, P2, SOLDIER);
    assert_eq!(wave.len(), 2);
    // Launched at the next decision once all are out: toward player 1's hall.
    run(&mut world, 40);
    for &id in &wave {
        assert!(
            near(heading(&world, id), 40, 40, 1),
            "{:?}",
            order(&world, id)
        );
    }
    // The next wave 20 seconds after the first.
    run(&mut world, 1062 - 302);
    assert_eq!(made(&world, P2, SOLDIER).len(), 4);
}

#[test]
fn waves_grow_and_lines_end() {
    let line = json!({"after": 1, "every": 1, "until": 3.6, "units": [{"unit": SOLDIER, "count": 1}],
        "grow": {"every": 1, "unit": SOLDIER, "count": 1}, "from": {"region": "edge"}});
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "waves", "waves": [line]}))]),
        "",
    ));
    run(&mut world, 81);
    assert_eq!(made(&world, P2, SOLDIER).len(), 1 + 2);
    run(&mut world, 400);
    // Waves at 1, 2 and 3 seconds of 1, 2 and 3 soldiers; none after 3.6 seconds.
    assert_eq!(made(&world, P2, SOLDIER).len(), 6);
}

#[test]
fn destroying_production_stops_the_waves() {
    let line = json!({"after": 5, "every": 5, "units": [{"unit": SOLDIER, "count": 2}]});
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "waves", "waves": [line]}))]),
        "",
    ));
    // The barracks falls while the first wave trains.
    run(&mut world, 215);
    let barracks = placed(&world, "p2-barracks").expect("barracks").id;
    world.remove_entity(barracks);
    run(&mut world, 800);
    assert!(made(&world, P2, SOLDIER).is_empty());
    let memory = &world.state().campaign_ai[0];
    assert!(memory.training.is_empty() && memory.waves.is_empty());
    assert!(memory.lines[0].sent >= 3);
}

#[test]
fn waves_arrive_at_regions_and_go_where_they_are_sent() {
    let to_gate = json!({"after": 1, "units": [{"unit": SOLDIER, "count": 3}],
        "from": {"region": "edge"}, "target": {"region": "gate"}});
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "waves", "waves": [to_gate]}))]),
        "",
    ));
    run(&mut world, 41);
    let wave = made(&world, P2, SOLDIER);
    assert_eq!(wave.len(), 3);
    for &id in &wave {
        let (x, y) = cell_of(&world, id);
        assert!((x - 230).abs() <= 6 && (y - 128).abs() <= 6, "{x},{y}");
        assert!(near(heading(&world, id), 128, 128, 0));
    }

    // `nearest`: player 1's nearest unit, not its hall.
    let nearest = json!({"after": 1, "units": [{"unit": SOLDIER, "count": 1}],
        "from": {"region": "west"}, "target": "nearest"});
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "waves", "waves": [nearest]}))]),
        "",
    ));
    run(&mut world, 41);
    let id = made(&world, P2, SOLDIER)[0];
    assert!(near(heading(&world, id), 40, 60, 0));
}

#[test]
fn scripts_steer_pause_and_send_waves() {
    let base = json!({"after": 1, "every": 10, "units": [{"unit": SOLDIER, "count": 1}], "from": {"region": "edge"}});
    let script = r#"
        function on_start() mission.ai_target(2, "gate") end
        function on_tick()
          if mission.tick() == 60 then mission.ai_waves("player.2", false) end
          if mission.tick() == 1000 then mission.ai_waves(2, true) end
          if mission.tick() == 1200 then mission.ai_attack_now(2) end
        end"#;
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "waves", "waves": [base]}))]),
        script,
    ));
    run(&mut world, 41);
    let first = made(&world, P2, SOLDIER);
    assert_eq!(first.len(), 1);
    assert!(near(heading(&world, first[0]), 128, 128, 0));
    // Paused at 1.5 s: the wave due at 11 s waits until the script resumes at 25 s.
    run(&mut world, 1000 - 41);
    assert_eq!(made(&world, P2, SOLDIER).len(), 1);
    run(&mut world, 2);
    assert_eq!(made(&world, P2, SOLDIER).len(), 2);
    // Sent now at 30 s, ahead of the 35 s timer.
    run(&mut world, 200);
    assert_eq!(made(&world, P2, SOLDIER).len(), 3);
}

#[test]
fn cinematic_mode_holds_the_waves_back() {
    let line = json!({"after": 5, "every": 5, "units": [{"unit": SOLDIER, "count": 1}], "from": {"region": "edge"}});
    let script = r#"
        function on_start() mission.play("opening") end
        function on_tick()
          if mission.tick() == 1100 then mission.play("later") end
        end
        function scenes.opening() mission.cinematic(true) mission.wait(10) mission.cinematic(false) end
        function scenes.later() mission.cinematic(true) mission.wait(20) mission.cinematic(false) end"#;
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "waves", "waves": [line]}))]),
        script,
    ));
    // Five seconds count from the end of the ten-second opening, not from the start.
    run(&mut world, 590);
    assert!(made(&world, P2, SOLDIER).is_empty());
    run(&mut world, 20);
    assert_eq!(made(&world, P2, SOLDIER).len(), 1);
    // Waves at 15, 20 and 25 seconds; the scene at 27.5 holds the fourth and those under way.
    run(&mut world, 1101 - 610);
    assert_eq!(made(&world, P2, SOLDIER).len(), 3);
    let at = world
        .state()
        .entities
        .get(made(&world, P2, SOLDIER)[0])
        .unwrap()
        .position;
    run(&mut world, 785);
    assert_eq!(made(&world, P2, SOLDIER).len(), 3);
    let held = world
        .state()
        .entities
        .get(made(&world, P2, SOLDIER)[0])
        .unwrap();
    assert_eq!(held.position, at);
    // Then it falls due 2.5 seconds after the scene, as it would have without it.
    run(&mut world, 135);
    assert_eq!(made(&world, P2, SOLDIER).len(), 4);
}

#[test]
fn scripts_switch_behaviours_and_name_only_computer_players() {
    let line = json!({"after": 1, "every": 100, "units": [{"unit": SOLDIER, "count": 1}], "from": {"region": "edge"}});
    let script = r#"function on_tick() if mission.tick() == 200 then mission.ai_behaviour(2, "waves") end end"#;
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "base", "waves": [line]}))]),
        script,
    ));
    run(&mut world, 199);
    assert!(made(&world, P2, SOLDIER).is_empty());
    // The overdue line sends at once.
    run(&mut world, 2);
    assert_eq!(made(&world, P2, SOLDIER).len(), 1);

    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({}))]),
        r#"function on_start() mission.ai_waves(1, true) end"#,
    ));
    world.step();
    let error = world.state().mission.as_ref().unwrap().error.clone();
    assert!(error.unwrap().contains("computer player"));
}

#[test]
fn allies_march_on_the_enemy_and_share_their_sight() {
    let line =
        json!({"after": 1, "units": [{"unit": SOLDIER, "count": 2}], "from": {"region": "west"}});
    let ally = |vision: bool| {
        json!([
            enemy(json!({"behaviour": "passive"})),
            {"player": 3, "team": "ally", "vision": vision, "ai": {"behaviour": "waves", "waves": [line]}},
        ])
    };
    let scout = place_for("p3-scout", SOLDIER, "player.3", 150.0, 236.0);
    let lurker = place_for("p2-lurker", SOLDIER, "player.2", 150.0, 240.0);
    let mut world = start(&map(vec![scout.clone(), lurker.clone()], ally(true), ""));
    run(&mut world, 41);
    let wave = made(&world, P3, SOLDIER);
    assert_eq!(wave.len(), 2);
    // Player 1's hall is nearer, but allies go for the enemy's buildings.
    for &id in &wave {
        assert!(
            near(heading(&world, id), 200, 200, 1),
            "{:?}",
            order(&world, id)
        );
    }
    let lurker_id = placed(&world, "p2-lurker").unwrap().id;
    let seen = |world: &World| {
        world
            .view(P1)
            .entities
            .iter()
            .any(|e| e.id() == lurker_id && !e.remembered)
    };
    assert!(seen(&world));
    let mut world = start(&map(vec![scout, lurker], ally(false), ""));
    run(&mut world, 41);
    assert!(!seen(&world));
}

#[test]
fn dormant_players_arrive_when_woken() {
    let line = json!({"after": 1, "every": 50, "units": [{"unit": SOLDIER, "count": 1}], "from": {"region": "gate"}});
    let players = json!([
        enemy(json!({"behaviour": "passive"})),
        {"player": 4, "team": "enemy", "ai": {"behaviour": "waves", "dormant": true, "waves": [line]}},
    ]);
    let mut arrival = place_for("p4-a", SOLDIER, "player.4", 128.0, 20.0);
    arrival["activation"] = json!("script");
    let script = r#"function on_tick() if mission.tick() == 100 then mission.ai_wake(4) end end"#;
    let mut world = start(&map(vec![arrival], players, script));
    run(&mut world, 100);
    assert!(placed(&world, "p4-a").is_none());
    assert!(made(&world, P4, SOLDIER).is_empty());
    run(&mut world, 1);
    assert!(placed(&world, "p4-a").is_some());
    // Its timers start when it arrives: the first wave a second later.
    run(&mut world, 39);
    assert!(made(&world, P4, SOLDIER).is_empty());
    run(&mut world, 2);
    assert_eq!(made(&world, P4, SOLDIER).len(), 1);
}

#[test]
fn bases_replace_defenders_and_rebuild_for_free() {
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "base"}))]),
        "",
    ));
    run(&mut world, 10);
    let guard = placed(&world, "p2-g1").unwrap().id;
    let house = placed(&world, "p2-house").unwrap().id;
    world.remove_entity(guard);
    world.remove_entity(house);
    // Noticed at the decision on tick 20, trained by tick 50, sent home.
    run(&mut world, 45);
    let replacement = made(&world, P2, SOLDIER);
    assert_eq!(replacement.len(), 1);
    run(&mut world, 400);
    let (x, y) = cell_of(&world, replacement[0]);
    assert!((x - 196).abs() <= 3 && (y - 188).abs() <= 3, "{x},{y}");
    // The house rises again 30 seconds after it fell, over its construction time.
    let house_at = |world: &World| {
        world
            .state()
            .entities
            .iter()
            .find(|e| e.owner == P2 && world.catalog().id(e.definition).as_str() == HOUSE)
            .map(|e| (e.construction.is_some(), e.hp))
    };
    run(&mut world, 1225 - 455);
    assert_eq!(house_at(&world).map(|h| h.0), Some(true));
    run(&mut world, 40);
    assert_eq!(house_at(&world), Some((false, Some(200))));
    // Nothing of this cost anything.
    assert!(world.state().wallets[&P2].values().all(|&v| v == 0));
}

#[test]
fn passive_players_hold_their_ground_and_replace_nothing() {
    let mut world = start(&map(
        Vec::new(),
        json!([enemy(json!({"behaviour": "passive"}))]),
        "",
    ));
    run(&mut world, 1);
    let guard = placed(&world, "p2-g1").unwrap().id;
    world.state_mut().entities.get_mut(guard).unwrap().position = Milli {
        x: 186_000,
        y: 180_000,
    };
    world.remove_entity(placed(&world, "p2-house").unwrap().id);
    world.remove_entity(placed(&world, "p2-g2").unwrap().id);
    run(&mut world, 400);
    let (x, y) = cell_of(&world, guard);
    assert!((x - 196).abs() <= 3 && (y - 188).abs() <= 3, "{x},{y}");
    run(&mut world, 1400);
    assert!(made(&world, P2, SOLDIER).is_empty());
    assert!(
        !world
            .state()
            .entities
            .iter()
            .any(|e| e.owner == P2 && world.catalog().id(e.definition).as_str() == HOUSE)
    );
}

#[test]
fn heroes_join_waves_and_revive_at_their_altar() {
    let extra = vec![
        place_for("p2-hero", HERO, "player.2", 190.0, 200.0),
        place_for("p2-altar", ALTAR, "player.2", 189.5, 213.5),
    ];
    let line = json!({"after": 1, "units": [{"unit": SOLDIER, "count": 1}], "from": {"region": "edge"},
        "target": {"region": "gate"}, "hero": true});
    let ai = json!({"behaviour": "waves", "waves": [line]});
    let mut world = start(&map(extra.clone(), json!([enemy(ai.clone())]), ""));
    run(&mut world, 41);
    let hero = placed(&world, "p2-hero").unwrap().id;
    assert!(near(heading(&world, hero), 128, 128, 0));

    let script = r#"function on_start() mission.damage("p2-hero", 5000) end"#;
    let mut world = start(&map(
        extra,
        json!([enemy(json!({"behaviour": "base"}))]),
        script,
    ));
    run(&mut world, 3);
    let hero = placed(&world, "p2-hero").map(|e| e.id);
    let fallen = |world: &World| {
        world
            .state()
            .entities
            .iter()
            .find(|e| e.placement.as_deref() == Some("p2-hero"))
            .is_some_and(|e| e.fallen)
    };
    assert!(hero.is_none() && fallen(&world));
    // Found fallen at the decision on tick 20; the altar takes 100 ticks.
    run(&mut world, 115);
    assert!(fallen(&world));
    run(&mut world, 5);
    assert!(!fallen(&world));
    assert_eq!(placed(&world, "p2-hero").unwrap().hp, Some(300));
}

#[test]
fn skirmish_players_play_only_while_skirmish() {
    let extra = vec![
        place_for("p2-hall", HALL, "player.2", 179.5, 199.5),
        place_for("p2-w1", WORKER, "player.2", 180.0, 210.0),
        place_for("p2-w2", WORKER, "player.2", 182.0, 210.0),
        place_for("p2-mine", MINE, "none", 169.5, 209.5),
    ];
    let skirmish = json!({"behaviour": "skirmish",
        "skirmish": {"style": "tech", "armyCap": 4, "pace": 2, "income": 0.5}});
    let mut world = start(&map(extra.clone(), json!([enemy(skirmish)]), ""));
    run(&mut world, 400);
    assert!(memory(&world, 1).metrics.issued > 0);
    assert_eq!(world.economy().tables().income.get(&P2), Some(&500));
    let mut world = start(&map(
        extra,
        json!([enemy(json!({"behaviour": "base"}))]),
        "",
    ));
    run(&mut world, 400);
    assert_eq!(memory(&world, 1).metrics.issued, 0);
}

/// Enemy waves from trained production and a region, an allied base and a dormant ally
/// that arrives: everything the campaign AI does, meeting in battle.
fn battle() -> UtcMap {
    let trained = json!({"after": 3, "every": 6, "units": [{"unit": SOLDIER, "count": 2}, {"unit": ARCHER, "count": 1}],
        "grow": {"every": 2, "unit": SOLDIER, "count": 1}, "hero": true, "target": "nearest"});
    let marching = json!({"after": 8, "every": 15, "units": [{"unit": SOLDIER, "count": 3}],
        "from": {"region": "edge"}, "target": "base"});
    let rescue = json!({"after": 2, "every": 10, "units": [{"unit": ARCHER, "count": 2}],
        "from": {"region": "west"}});
    let players = json!([
        enemy(json!({"behaviour": "waves", "waves": [trained, marching]})),
        {"player": 3, "team": "ally", "ai": {"behaviour": "base"}},
        {"player": 4, "team": "ally", "ai": {"behaviour": "waves", "dormant": true, "waves": [rescue]}},
    ]);
    let mut arrival = place_for("p4-a", SOLDIER, "player.4", 60.0, 128.0);
    arrival["activation"] = json!("script");
    let extra = vec![
        place_for("p2-hero", HERO, "player.2", 190.0, 200.0),
        place_for("p2-altar", ALTAR, "player.2", 189.5, 213.5),
        place_for("p3-barracks", BARRACKS, "player.3", 59.5, 91.5),
        place_for("p3-g1", SOLDIER, "player.3", 60.0, 100.0),
        place_for("p3-g2", ARCHER, "player.3", 64.0, 100.0),
        arrival,
    ];
    let script = r#"function on_tick() if mission.tick() == 400 then mission.ai_wake(4) end end"#;
    map(extra, players, script)
}

#[test]
fn saves_mid_wave_play_on_identically() {
    let map = battle();
    let mut world = start(&map);
    run(&mut world, 130);
    assert!(!world.state().campaign_ai[0].training.is_empty());
    let text = serde_json::to_string(&world.save()).unwrap();
    let mut restored = World::restore(
        campaign_content(),
        &map,
        serde_json::from_str(&text).unwrap(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..1200 {
        world.step();
        restored.step();
        assert_eq!(world.fingerprint(), restored.fingerprint());
    }
    assert_eq!(world.save(), restored.save());
}

#[test]
fn campaign_matches_are_deterministic() {
    let map = battle();
    let mut a = start(&map);
    let mut b = start(&map);
    for _ in 0..2400 {
        a.step();
        b.step();
        assert_eq!(a.fingerprint(), b.fingerprint());
    }
    assert_eq!(a.save(), b.save());
    // The battle happened: waves were sent and units died on both sides.
    let enemy = &a.state().campaign_ai[0];
    assert!(enemy.lines.iter().all(|l| l.sent >= 2));
    assert!(placed(&a, "p4-a").is_some() || !made(&a, P4, ARCHER).is_empty());
}
