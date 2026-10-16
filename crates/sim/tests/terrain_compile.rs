//! Compiling a map into simulation terrain, on test-owned content and maps.

mod terrain_fixture;

use canopy_content::{map::HeightMode, placement::Owner};
use canopy_sim::terrain::{SurfacePoint, Terrain, TerrainPoint, WADING_DEPTH_CM};
use serde_json::{Value, json};
use std::f64::consts::FRAC_PI_2;
use terrain_fixture::{content, map, object, parse, terrain};

fn compile(document: Value) -> Terrain {
    Terrain::compile(&parse(document), &content()).unwrap_or_else(|e| panic!("{e}"))
}

fn with(pointer: &str, value: Value) -> Value {
    let mut document = map();
    *document.pointer_mut(pointer).expect("pointer") = value;
    document
}

fn cell(t: &Terrain, x: i64, y: i64) -> usize {
    t.cell(x, y).expect("on the map")
}

#[test]
fn heights_and_water_round_to_centimetres_and_shallow_water_is_wadeable() {
    // Ground falls 5 cm per cell from x = 100 into water standing at 0.
    let ground = |x: i32, _| {
        if x > 100 {
            -0.05 * (x - 100) as f32
        } else {
            0.004
        }
    };
    let t = compile(with("/authoring/terrain", terrain(ground, |_, _| 0.0)));
    assert_eq!(t.height_cm(cell(&t, 10, 10)), 0);
    assert_eq!(t.height_cm(cell(&t, 150, 3)), -250);
    assert_eq!(t.water_cm(cell(&t, 150, 3)), 0);
    // Wading: ground up to 60 cm below the surface stays walkable, deeper does not.
    assert_eq!(WADING_DEPTH_CM, 60);
    assert_eq!(t.height_cm(cell(&t, 112, 0)), -60);
    assert!(t.walkable(cell(&t, 112, 0)));
    assert!(!t.walkable(cell(&t, 113, 0)));
    // Between vertices the field interpolates; off the halo the ground is 0.
    assert!((t.field().sample(150.5, 3.0) + 2.525).abs() < 1e-6);
    assert_eq!(t.field().sample(-17.0, 3.0), 0.0);
    assert_eq!(t.field().water_at(-100.0, 3.0), 0.0);
    assert!(t.field().wet(150.0, 3.0));
}

#[test]
fn harvestable_scenery_becomes_resources_first_object_per_cell() {
    let mut tree = object("tree.a", "asset.tree", 60.4, 70.5);
    tree["yaw"] = json!(FRAC_PI_2);
    tree["scale"] = json!(1.5);
    let mut absolute = object("tree.absolute", "asset.tree", 80.0, 80.0);
    absolute["heightMode"] = json!("absolute");
    absolute["elevation"] = json!(2.0);
    let mut hidden = object("tree.hidden", "asset.tree", 90.0, 90.0);
    hidden["visible"] = json!(false);
    let objects = json!([
        tree,
        object("tree.b", "asset.tree", 59.6, 70.6),
        object("tree.halo", "asset.tree", -5.0, 20.0),
        absolute,
        hidden,
        object("rock", "asset.rock", 30.0, 30.0),
    ]);
    let t = compile(with("/authoring/objects", objects));
    let scene = t.scene();
    let [resource] = scene.resources.as_slice() else {
        panic!("one resource: {:?}", scene.resources)
    };
    assert_eq!(resource.id, "tree.a");
    assert_eq!(resource.definition.as_str(), "resource.tree");
    assert_eq!((resource.position.x, resource.position.y), (60.0, 71.0));
    assert_eq!(resource.rotation, 90.0);
    assert_eq!(resource.owner, Owner::Nobody);
    let appearance = resource.appearance.as_ref().unwrap();
    assert_eq!(
        appearance.asset.as_ref().unwrap().as_str(),
        "scenery.asset.tree-prop"
    );
    assert_eq!(appearance.scale, Some(1.5));
    // The losing tree vanishes; the halo tree, the absolute tree and the rock stay scenery.
    let stamps: Vec<&str> = scene.stamps.iter().map(|s| s.id.as_str()).collect();
    assert_eq!(stamps, ["tree.halo", "tree.absolute", "rock"]);
    let placed = &scene.stamps[1];
    assert_eq!(placed.asset, "scenery.asset.tree-prop");
    assert_eq!(placed.source_transform.as_ref().unwrap().height, 2.0);
    assert_eq!(placed.elevation, None);
    assert_eq!(scene.stamps[2].elevation, Some(0.0));
    assert_eq!(HeightMode::default(), HeightMode::Terrain);
}

#[test]
fn objects_need_a_published_scenery_binding() {
    let error = Terrain::compile(
        &parse(with(
            "/authoring/objects",
            json!([object("x", "asset.missing", 5.0, 5.0)]),
        )),
        &content(),
    )
    .unwrap_err();
    assert_eq!(
        error.to_string(),
        "Asset asset.missing has no published scenery binding"
    );
}

#[test]
fn scenery_blockers_close_ground_from_map_stamps_and_objects() {
    let mut wall = object("wall.object", "asset.wall", 120.0, 60.0);
    wall["yaw"] = json!(FRAC_PI_2);
    let mut document = with("/authoring/objects", json!([wall]));
    document["stamps"] =
        json!([{"id": "wall.stamp", "asset": "scenery.asset.wall-prop", "x": 100, "y": 50}]);
    let t = compile(document);
    // 6 × 1 plus margin: x 97–103 on row 50.
    assert!((97..=103).all(|x| !t.walkable(cell(&t, x, 50))));
    assert!(t.walkable(cell(&t, 96, 50)) && t.walkable(cell(&t, 100, 49)));
    // Turned a quarter: the object's wall runs along y.
    assert!((57..=63).all(|y| !t.walkable(cell(&t, 120, y))));
    assert!(t.walkable(cell(&t, 121, 60)));
}

#[test]
fn bridge_stamps_add_walk_surfaces() {
    let mut document = map();
    document["stamps"] = json!([{"id": "bridge.1", "asset": "scenery.asset.bridge-prop", "x": 120, "y": 120, "elevation": 1}]);
    let t = compile(document);
    let surfaces = t.surfaces().expect("decks");
    let deck = t.deck_id("bridge.1").unwrap();
    assert_eq!(surfaces.deck(deck).base, 1.0);
    // Deck top: base 1 + height 0.4.
    assert_eq!(t.floor(120.5, 120.5, Some("bridge.1")), Some(1.4));
    assert_eq!(t.floor(130.5, 120.5, Some("bridge.1")), None);
    assert_eq!(t.floor(120.5, 120.5, Some("bridge.2")), None);
    assert_eq!(t.floor(120.5, 120.5, None), Some(0.0));
    assert!(t.under_deck(cell(&t, 120, 120)) && !t.under_deck(cell(&t, 130, 120)));
    let on_deck = SurfacePoint {
        x: 120.0,
        y: 120.0,
        deck: Some(deck),
        elevation: 0.0,
    };
    assert_eq!(t.height(on_deck), Some(1.4));
    assert_eq!(t.air_floor(120.0, 120.0), 1.4);
    assert_eq!(t.air_floor(10.0, 10.0), 0.0);
    assert!(surfaces.walkable(surfaces.node_at(120, 120, Some(deck)).unwrap()));
}

#[test]
fn tactical_queries_follow_the_ground() {
    // A 5 m plateau for x ≥ 100.
    let ground = |x: i32, _| if x >= 100 { 5.0 } else { 0.0 };
    let t = compile(with("/authoring/terrain", terrain(ground, |_, _| -17.0)));
    let low = TerrainPoint::new(90.0, 50.0);
    let high = TerrainPoint::new(110.0, 50.0);
    assert!(t.tactical().visible(high, low));
    assert!(!t.tactical().visible(low, high));
    assert!(!t.tactical().melee_clear(low, high));
    assert!(
        t.tactical()
            .shot_clear(high, TerrainPoint::new(120.0, 80.0))
    );
    let point = |p: TerrainPoint| SurfacePoint {
        x: p.x,
        y: p.y,
        deck: None,
        elevation: p.elevation,
    };
    assert_eq!(t.height(point(high)), Some(5.0));
    assert!(!t.visible(point(low), point(high)));
    let spans = t
        .tactical()
        .visible_spans(TerrainPoint::new(97.0, 50.0), 5.0);
    assert!(spans.iter().all(|c| c % 256 < 100));
    assert!(t.steep_edges().entered(100, 50, 1, 0));
}
