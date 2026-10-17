//! Starting a match on compiled terrain: projected resources, starting formations and the
//! placement checks, on test-owned content and maps.

mod terrain_fixture;

use canopy_sim::{World, entity::Milli};
use serde_json::{Value, json};
use terrain_fixture::{content, map, object, parse, setup, terrain};

/// Starts a match and runs the map's playability checks (an authoring gate, not part of
/// starting a match).
fn start(document: Value, players: u8) -> Result<World, String> {
    let map = parse(document);
    let world = World::new(content(), &map, setup(players)).map_err(|e| e.0)?;
    canopy_sim::spawn::playable(&map, world.catalog(), world.terrain())?;
    Ok(world)
}

fn fails(document: Value, expected: &str) {
    match start(document, 2) {
        Ok(_) => panic!("expected {expected:?}"),
        Err(error) => assert!(
            error.contains(expected),
            "expected {expected:?}, got {error:?}"
        ),
    }
}

fn with_objects(objects: Value) -> Value {
    let mut document = map();
    document["authoring"]["objects"] = objects;
    document
}

fn with_entities(entities: Value) -> Value {
    let mut document = map();
    document["entities"] = entities;
    document
}

/// Placement id → (position, rotation in millidegrees) of the started match.
fn spawned(world: &World) -> Vec<(String, Milli, i32)> {
    world
        .state()
        .entities
        .iter()
        .map(|e| (e.placement.clone().unwrap(), e.position, e.rotation))
        .collect()
}

fn milli(x: i32, y: i32) -> Milli {
    Milli {
        x: x * 1000,
        y: y * 1000,
    }
}

#[test]
fn resources_spawn_and_formations_face_the_nearest_tree() {
    // Two trees equally far from player 1's hall; the lower id (east) wins the tie.
    let world = start(
        with_objects(json!([
            object("tree.b", "asset.tree", 41.0, 55.0),
            object("tree.a", "asset.tree", 55.0, 42.0),
        ])),
        2,
    )
    .unwrap();
    let entities = spawned(&world);
    let ids: Vec<&str> = entities.iter().map(|(id, ..)| id.as_str()).collect();
    assert_eq!(
        ids,
        [
            "start.player.1/main-fort",
            "start.player.1/unit-001",
            "start.player.1/unit-002",
            "start.player.2/main-fort",
            "start.player.2/unit-001",
            "start.player.2/unit-002",
            "tree.a",
            "tree.b",
        ]
    );
    // Player 1's workers turn a quarter towards the east tree; the hall keeps its facing.
    assert_eq!(entities[0].2, 0);
    assert_eq!(entities[1].1, milli(46, 42));
    assert_eq!(entities[2].1, milli(46, 40));
    assert_eq!(entities[1].2, 90_000);
    // Player 2 has no tree in reach.
    assert_eq!(entities[4].1, milli(201, 206));
    assert_eq!(entities[5].1, milli(203, 206));
    assert_eq!(entities[4].2, 0);
    let tree = world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some("tree.a"))
        .unwrap();
    assert_eq!(tree.position, milli(55, 42));
    assert_eq!(tree.resource.as_ref().unwrap().amount, 10);
    assert_eq!(world.terrain().scene().resources.len(), 2);
}

#[test]
fn sandbox_maps_keep_their_scenery_resources_without_starts() {
    let mut document = with_objects(json!([object("tree.a", "asset.tree", 55.0, 42.0)]));
    document["sandbox"] = json!(true);
    document["playerStarts"].as_array_mut().unwrap().truncate(1);
    let world = start(document, 1).unwrap();
    let ids: Vec<String> = spawned(&world).into_iter().map(|(id, ..)| id).collect();
    assert_eq!(ids, ["tree.a"]);
}

#[test]
fn restored_matches_rebuild_the_terrain() {
    let document = with_objects(json!([object("tree.a", "asset.tree", 55.0, 42.0)]));
    let map = parse(document.clone());
    let mut world = start(document, 2).unwrap();
    world.step();
    let save = world.save();
    let restored = World::restore(content(), &map, save).unwrap();
    assert_eq!(restored.terrain().scene(), world.terrain().scene());
    assert_eq!(restored.terrain().heights(), world.terrain().heights());
    assert_eq!(restored.fingerprint(), world.fingerprint());
}

#[test]
fn starting_halls_keep_a_lane_to_resources() {
    fails(
        with_objects(json!([object("tree.a", "asset.tree", 44.0, 41.0)])),
        "Player 1: building.hall is too close to resource.tree; leave at least 4.5 cells horizontally or 4.5 cells vertically between centers.",
    );
}

#[test]
fn starting_bodies_cannot_overlap_footprints() {
    fails(
        with_entities(json!([{"id": "post", "definition": "building.hall",
            "position": {"x": 41.5, "y": 45.5}, "owner": "none",
            "initialState": {"construction": "complete"}}])),
        "start.player.1/unit-001: body overlaps post",
    );
    fails(
        with_entities(json!([{"id": "post", "definition": "building.hall",
            "position": {"x": 41.5, "y": 41.5}, "owner": "none",
            "initialState": {"construction": "complete"}}])),
        "start.player.1/main-fort: overlaps post",
    );
    fails(
        with_entities(json!([{"id": "a.worker", "definition": "unit.worker",
            "position": {"x": 41, "y": 46}, "owner": "none"}])),
        "start.player.1/unit-001: overlaps a.worker",
    );
}

#[test]
fn starts_need_the_lattice_dry_ground_and_gentle_slopes() {
    let mut off_lattice = map();
    off_lattice["playerStarts"][0]["x"] = json!(42.5);
    fails(
        off_lattice,
        "start.player.1/main-fort: Align the building footprint to the building grid",
    );
    let mut edge = map();
    edge["playerStarts"][0]["x"] = json!(5.5);
    fails(
        edge,
        "Player 1 needs a building-grid-aligned start at least 8 navigation cells inside the map.",
    );
    let mut flooded = map();
    flooded["authoring"]["terrain"] = terrain(|_, _| 0.0, |x, _| if x < 60 { 0.5 } else { -1.0 });
    fails(
        flooded,
        "Player 1 needs dry ground for the fort and workers.",
    );
    let mut cliff = map();
    cliff["authoring"]["terrain"] = terrain(|x, _| if x >= 43 { 3.0 } else { 0.0 }, |_, _| -1.0);
    fails(
        cliff,
        "Player 1 needs flatter ground for the fort and workers.",
    );
}

#[test]
fn placements_need_cell_centres_unique_ids_and_known_surfaces() {
    let unit = |extra: Value| {
        let mut p = json!({"id": "scout", "definition": "unit.worker",
            "position": {"x": 100, "y": 100}, "owner": "none"});
        for (key, value) in extra.as_object().unwrap() {
            p[key] = value.clone();
        }
        p
    };
    fails(
        with_entities(json!([unit(json!({"position": {"x": 100.5, "y": 100}}))])),
        "scout: Place units and resources on navigation cell centers",
    );
    fails(
        with_entities(json!([unit(
            json!({"position": {"x": 100, "y": 100, "surface": "nowhere"}})
        )])),
        "scout: unknown or out-of-bounds walk surface",
    );
    let mut duplicate = with_entities(json!([unit(json!({"id": "tree.a"}))]));
    duplicate["authoring"]["objects"] = json!([object("tree.a", "asset.tree", 120.0, 120.0)]);
    fails(duplicate, "Duplicate placement tree.a");
    fails(
        with_entities(json!([{"id": "hut", "definition": "building.hall",
            "position": {"x": 41.5, "y": 41.5}, "rotation": 45, "owner": "none"}])),
        "hut: Buildings rotate in 90-degree steps",
    );
}

#[test]
fn seats_nobody_plays_get_no_base_and_lose_their_placements() {
    let scout = |owner: &str| {
        json!({"id": format!("scout.{owner}"), "definition": "unit.worker",
            "position": {"x": 100, "y": 100}, "owner": owner})
    };
    // Skirmish: only player 1 is seated; player 2's start and scout are left out.
    let world = start(with_entities(json!([scout("player.2")])), 1).unwrap();
    let ids: Vec<String> = spawned(&world).into_iter().map(|(id, ..)| id).collect();
    assert!(ids.iter().any(|id| id.starts_with("start.player.1/")));
    assert!(
        !ids.iter()
            .any(|id| id.starts_with("start.player.2/") || id == "scout.player.2")
    );
    // A sandbox (like a mission) needs every owner it places seated.
    let mut sandbox = with_entities(json!([scout("player.2")]));
    sandbox["sandbox"] = json!(true);
    sandbox["playerStarts"].as_array_mut().unwrap().truncate(1);
    assert_eq!(
        start(sandbox, 1).err().as_deref(),
        Some("Placement owner player.2 has no match slot")
    );
}

#[test]
fn units_may_stand_on_bridge_decks() {
    let mut document = with_entities(json!([{"id": "scout", "definition": "unit.worker",
        "position": {"x": 120, "y": 120, "surface": "bridge.1"}, "owner": "none"}]));
    document["stamps"] =
        json!([{"id": "bridge.1", "asset": "scenery.asset.bridge-prop", "x": 120, "y": 120}]);
    let world = start(document, 2).unwrap();
    let scout = world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some("scout"))
        .unwrap();
    assert_eq!(scout.surface.as_deref(), Some("bridge.1"));
}

#[test]
fn starts_must_reach_each_other_on_foot() {
    let walled = |gap: bool| {
        let mut document = map();
        document["authoring"]["terrain"] = terrain_fixture::terrain(
            move |x, z| {
                let wall = (120..=130).contains(&x) && !(gap && (100..=112).contains(&z));
                if wall { 5.0 } else { 0.0 }
            },
            |_, _| -1.0,
        );
        let map = parse(document);
        let world = World::new(content(), &map, setup(2))
            .map_err(|e| e.0)
            .unwrap();
        canopy_sim::spawn::connected(&map, world.terrain())
    };
    let error = walled(false).unwrap_err();
    assert!(error.contains("cannot be reached"), "{error}");
    walled(true).unwrap();
}

#[test]
fn nothing_walks_outside_the_playable_area_but_its_trees_are_resources() {
    let mut document = map();
    document["playable"] = json!({"x": 16, "z": 16, "width": 224, "height": 224});
    document["authoring"]["objects"] = json!([
        object("tree.inside", "asset.tree", 100.0, 100.0),
        object("tree.outside", "asset.tree", 4.0, 4.0),
    ]);
    let parsed = parse(document);
    let terrain = canopy_sim::terrain::Terrain::compile(&parsed, &content()).unwrap();
    let walkable = |x: i64, z: i64| terrain.walkable(terrain.cell(x, z).unwrap());
    assert!(walkable(100, 100) && walkable(16, 16));
    assert!(!walkable(15, 100) && !walkable(100, 240) && !walkable(4, 4));
    // Trees are resources wherever they stand; those at the edge are cut from inside.
    let resources: Vec<&str> = terrain
        .scene()
        .resources
        .iter()
        .map(|r| r.id.as_str())
        .collect();
    assert_eq!(resources, ["tree.inside", "tree.outside"]);
    // Starts must stand well inside it.
    let mut document = map();
    document["playable"] = json!({"x": 40, "z": 40, "width": 180, "height": 180});
    fails(document, "inside the map");
}
