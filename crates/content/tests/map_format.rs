//! Reading `.utcmap` documents: what the old parser accepts, rejects and returns.

mod map_fixture;

use canopy_content::map::{
    DEFAULT_MAP_NAME, Exploration, HeightMode, StampVariant, UtcMap, WeatherKind,
};
use map_fixture::{map, parse, remove, set, with};
use serde_json::{Value, json};

fn rejected(document: Value) -> String {
    match parse(document) {
        Ok(_) => panic!("accepted an invalid map"),
        Err(errors) => errors.to_string(),
    }
}

#[test]
fn minimal_map_parses_with_defaults() {
    let mut document = map();
    remove(&mut document, "/name");
    remove(&mut document, "/stamps");
    let parsed = parse(document).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(parsed.name, DEFAULT_MAP_NAME);
    assert!(parsed.stamps.is_empty());
    assert!(!parsed.sandbox);
    assert_eq!(parsed.biome(), "vibrant-forest");
    assert_eq!(parsed.exploration(), Exploration::Explored);
    let object = &parsed.authoring.objects[0];
    assert_eq!(
        (object.elevation, object.yaw, object.scale),
        (0.0, 0.0, 1.0)
    );
    assert_eq!(object.height_mode, HeightMode::Terrain);
    assert!(object.visible && !object.locked);
    assert_eq!(parsed.player_start(2).map(|s| s.x), Some(200.5));
}

#[test]
fn normalises_like_the_old_parser() {
    let mut document = map();
    set(&mut document, "/name", json!("  Spaced  "));
    set(&mut document, "/description", json!(" \n "));
    set(&mut document, "/sandbox", json!(false));
    set(
        &mut document,
        "/stamps",
        json!([
            {"id": "s1", "asset": "pebble", "x": 3, "y": 4, "scale": 1, "variant": "slate", "note": "ignored"},
            {"id": "s2", "asset": "pebble", "x": 5, "y": 6, "scale": 2},
        ]),
    );
    set(
        &mut document,
        "/landscape",
        json!({
            "environment": {"hour": 7, "playing": true, "season": "summer", "light": null, "weather": {"kind": "rain"}},
            "water": null,
        }),
    );
    let parsed = parse(document).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(parsed.name, "Spaced");
    assert_eq!(parsed.description, None);
    assert_eq!(parsed.stamps[0].scale, None);
    assert_eq!(parsed.stamps[0].variant, Some(StampVariant::Slate));
    assert_eq!(parsed.stamps[1].scale, Some(2.0));
    let landscape = parsed.landscape.as_ref().expect("landscape");
    assert_eq!(landscape.water, None);
    assert_eq!(
        landscape.environment.weather.as_ref().map(|w| w.kind),
        Some(WeatherKind::Rain)
    );
    let saved = serde_json::to_value(&parsed).expect("serialisable");
    assert_eq!(saved.get("sandbox"), None);
    assert_eq!(
        saved["landscape"],
        json!({"environment": {"hour": 7.0, "playing": true, "weather": {"kind": "rain"}}})
    );
    assert_eq!(saved["stamps"][0].get("note"), None);
}

#[test]
fn round_trips() {
    let mut document = with(
        "/mission",
        json!({
            "campaign": "Test", "title": "One", "order": 1, "script": "function on_start() end",
            "regions": [{"id": "gate", "x": 4, "y": 5, "radius": 3}],
            "objectives": [{"id": "win", "title": "Win", "description": "Win it"}],
        }),
    );
    set(
        &mut document,
        "/playerStarts",
        json!([{"player": 1, "x": 40.5, "z": 40.5, "setup": "setup.test", "mainFort": "fort"}]),
    );
    let parsed = parse(document).unwrap_or_else(|e| panic!("{e}"));
    let mission = parsed.mission.as_ref().expect("mission");
    assert!(!mission.objectives.as_ref().expect("objectives")[0].optional);
    assert_eq!(parsed.exploration(), Exploration::Hidden);
    let again = parse(serde_json::to_value(&parsed).expect("serialisable")).expect("reparses");
    assert_eq!(again, parsed);
}

#[test]
fn packages_round_trip() {
    let folder = std::path::Path::new(env!("CARGO_TARGET_TMPDIR")).join("map-format");
    let _ = std::fs::remove_dir_all(&folder);
    let parsed = parse(map()).expect("parses");
    let path = folder.join("fixture.utcm");
    canopy_content::map::package::write(&parsed, "fixture", "1.0.0", &path).expect("written");
    let loaded = canopy_content::map::package::load(&path).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(loaded, parsed);
    let summary = canopy_content::map::package::summary(&path).unwrap();
    assert_eq!(summary.name, "Fixture");
    assert_eq!(summary.starts.len(), parsed.player_starts.len());
    assert!(canopy_content::map::package::load(&folder.join("missing.utcm")).is_err());
}

#[test]
fn accepts_what_the_old_parser_tolerates() {
    for (pointer, value) in [
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "anything": null}]),
        ),
        (
            "/landscape",
            json!({"environment": {"hour": 0, "playing": false, "extra": null}, "water": {"rippleScale": 9}}),
        ),
        (
            "/landscape",
            json!({"environment": {"hour": 24, "playing": false}, "decals": []}),
        ),
        ("/initialExploration", json!("hidden")),
        ("/biome", json!("frozen-forest")),
        ("/v", json!(3.0)),
    ] {
        if let Err(errors) = parse(with(pointer, value.clone())) {
            panic!("{pointer} = {value}: {errors}");
        }
    }
}

#[test]
fn rejects_what_the_old_parser_rejects() {
    let start = |player: u32, x: f64| json!({"player": player, "x": x, "z": 100, "setup": "s", "mainFort": "f"});
    let entity = |id: &str, x: f64| json!({"id": id, "definition": "unit.a", "position": {"x": x, "y": 3}, "owner": "none"});
    let cases: Vec<(&str, Value)> = vec![
        ("/v", json!(2)),
        ("/size", json!(300)),
        ("/unknown", json!(1)),
        ("/height", json!([])),
        ("/waterLevel", json!(0)),
        ("/biome", json!("lava")),
        ("/biome", Value::Null),
        ("/name", json!(5)),
        ("/name", Value::Null),
        ("/description", json!("x".repeat(1201))),
        ("/description", Value::Null),
        ("/sandbox", json!("yes")),
        ("/initialExploration", json!("partial")),
        ("/playerStarts", json!([start(1, 40.0), start(1, 80.0)])),
        ("/playerStarts", json!([start(1, 40.25), start(2, 80.0)])),
        ("/playerStarts", json!([start(1, 256.0), start(2, 80.0)])),
        ("/playerStarts", json!([start(0, 40.0), start(2, 80.0)])),
        ("/playerStarts/0/rotation", json!(45)),
        ("/playerStarts/0/extra", json!(1)),
        ("/entities", json!([entity("a", 256.0)])),
        ("/entities", json!([entity("", 3.0)])),
        ("/entities", json!([entity("a", 3.25)])),
        (
            "/camps",
            json!([{"id": "c", "members": [], "home": {"x": 1, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players"}]),
        ),
        (
            "/camps",
            json!([{"id": "c", "members": ["m"], "home": {"x": 1.5, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players"}]),
        ),
        (
            "/camps",
            json!([{"id": "c", "members": ["m"], "home": {"x": 1, "y": 1}, "aggroRange": 65, "leash": 8, "aggression": "players"}]),
        ),
        (
            "/camps",
            json!([{"id": "c", "members": ["m"], "home": {"x": 1, "y": 1}, "aggroRange": 4, "leash": 8, "aggression": "players", "lootPool": "loot.a", "fixedDrops": ["item.a"]}]),
        ),
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "pitch": 2}]),
        ),
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "heightScale": 5}]),
        ),
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "elevation": 33}]),
        ),
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "yaw": null}]),
        ),
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "variant": "blue"}]),
        ),
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "walk": {"level": 0}}]),
        ),
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "walk": {"level": 1, "mesh": {"positions": [[0,0,0],[1,0,0],[0,0,1]], "indices": [0, 1, 3]}}}]),
        ),
        (
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "sourceTransform": {"height": 0, "quaternion": [0, 0, 0, 2]}}]),
        ),
        ("/stamps", json!([{"asset": "a", "x": 1, "y": 2}])),
        ("/stamps", json!({})),
        (
            "/landscape",
            json!({"environment": {"hour": 25, "playing": false}}),
        ),
        (
            "/landscape",
            json!({"environment": {"hour": 1, "playing": false}, "strokes": []}),
        ),
        (
            "/landscape",
            json!({"environment": {"hour": 1, "playing": false, "weather": null}}),
        ),
        ("/landscape", json!({"water": {}})),
        ("/landscape", Value::Null),
        (
            "/landscape",
            json!({"environment": {"hour": 1, "playing": false}, "decals": [{"id": "d", "kind": "pebbles", "x": 0, "z": 0, "size": 40, "rotation": 0, "opacity": 1}]}),
        ),
        ("/mission", Value::Null),
        ("/authoring/terrain/size", json!(512)),
        ("/authoring/terrain/version", json!(2)),
        ("/authoring/terrain/heights", json!("AAAA")),
        (
            "/authoring/terrain/waterProfiles",
            json!(["water.a", "water.a"]),
        ),
        (
            "/authoring/terrain/paint",
            json!([{"material": "", "weights": ""}]),
        ),
        (
            "/authoring/terrain/textureGrid",
            json!({"cellSize": 0.5, "origin": [0, 0]}),
        ),
        ("/authoring/layers", json!([])),
        (
            "/authoring/objects/1",
            json!({"id": "rock.1", "asset": "asset.models.rock", "x": 1, "z": 1}),
        ),
        ("/authoring/objects/0/scale", json!(0)),
        ("/authoring/objects/0/id", json!("_rock")),
        ("/authoring/objects/0/x", json!(5000)),
    ];
    for (pointer, value) in cases {
        let mut document = map();
        if pointer.starts_with("/authoring/objects/1") {
            document["authoring"]["objects"]
                .as_array_mut()
                .expect("objects")
                .push(value.clone());
        } else {
            set(&mut document, pointer, value.clone());
        }
        assert!(parse(document).is_err(), "accepted {pointer} = {value}");
    }
}

#[test]
fn mission_rules() {
    let mission =
        || json!({"campaign": "C", "title": "T", "order": 1, "script": "x = 1", "regions": []});
    let one_start = json!([{"player": 1, "x": 40.5, "z": 40.5, "setup": "s", "mainFort": "f"}]);
    let mut base = with("/mission", mission());
    set(&mut base, "/playerStarts", one_start);
    parse(base.clone()).unwrap_or_else(|e| panic!("{e}"));
    for (pointer, value) in [
        ("/mission/order", json!(0)),
        ("/mission/script", json!("")),
        ("/mission/race", json!("Ants")),
        ("/mission/playerRaces", json!({"player.9": "ants"})),
        ("/mission/nextMission", json!("-next")),
        ("/mission/company", json!([])),
        ("/mission/company", json!(["a", "a"])),
        ("/mission/heroLevelCap", json!(51)),
        (
            "/mission/regions",
            json!([{"id": "1a", "x": 1, "y": 1, "radius": 1}]),
        ),
        (
            "/mission/regions",
            json!([{"id": "a", "x": 1, "y": 1, "radius": 0}]),
        ),
        (
            "/mission/regions",
            json!([{"id": "a", "x": 1, "y": 1, "radius": 1}, {"id": "a", "x": 2, "y": 2, "radius": 1}]),
        ),
        (
            "/mission/objectives",
            json!([{"id": "o", "title": "", "description": "d"}]),
        ),
        (
            "/mission/presentation",
            json!({"loadingBackground": "asset.icons.x"}),
        ),
        (
            "/mission/presentation",
            json!({"tips": ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l", "m"]}),
        ),
        ("/mission/extra", json!(1)),
        ("/sandbox", json!(true)),
    ] {
        let mut document = base.clone();
        set(&mut document, pointer, value.clone());
        assert!(parse(document).is_err(), "accepted {pointer} = {value}");
    }
}

#[test]
fn sandbox_rules() {
    let mut document = with("/sandbox", json!(true));
    set(
        &mut document,
        "/playerStarts",
        json!([{"player": 1, "x": 40.5, "z": 40.5, "setup": "s", "mainFort": "f"}]),
    );
    assert!(
        parse(document.clone())
            .unwrap_or_else(|e| panic!("{e}"))
            .sandbox
    );
    set(&mut document, "/playerStarts/0/player", json!(2));
    assert!(rejected(document).contains("testbed uses player 1"));
    assert!(rejected(with("/sandbox", json!(true))).contains("playerStarts"));
}

#[test]
fn errors_name_the_offending_field() {
    assert!(rejected(with("/size", json!(300))).contains("size"));
    assert!(
        rejected(with("/authoring/terrain/heights", json!("AAAA")))
            .contains("authoring.terrain.heights")
    );
    assert!(
        rejected(with(
            "/stamps",
            json!([{"id": "s", "asset": "a", "x": 1, "y": 2, "pitch": 2}])
        ))
        .contains("stamps[0]")
    );
    assert!(parse(json!([])).is_err());
    assert!(UtcMap::parse(b"{", "broken.utcmap").is_err());
}
