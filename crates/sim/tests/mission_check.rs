//! The mission check (docs/campaign.md "Checking a mission") on test-owned maps: stranded
//! units, wave regions on buildings, routes judged by the real pathfinder with their
//! narrowest width, scripted crates walling a corridor, and every scene dry-run skipped.

mod mission_fixture;

use canopy_content::map::{UtcMap, grid::Grid};
use canopy_sim::mission::check::{self, Report};
use mission_fixture::*;
use serde_json::json;

/// A valid scene: the hero walks, turns, speaks; the battle comes back.
const QUIET: &str = r#"
function on_start()
  mission.begin_objective("reach")
end

function scenes.greeting()
  mission.cinematic(true)
  mission.camera_to("road", 0)
  mission.move("hero", 70, 60)
  mission.wait_arrived("hero")
  mission.face("hero", 70, 66)
  mission.speak("hero", "Open the gate.", 1)
  mission.cinematic(false)
  mission.camera_reset(1)
end
"#;

fn road(mission: Mission) -> Mission {
    mission.camera(
        json!({"id": "road", "x": 60, "y": 60, "distance": 30, "pitch": 40, "yaw": 20, "fov": 40}),
    )
}

/// Closes cells (`x0..=x1`, `y0..=y1`) of the map's ground, as a painted frame does.
fn close(map: &mut UtcMap, x0: i64, y0: i64, x1: i64, y1: i64) {
    let terrain = &mut map.authoring.terrain;
    let span = i64::from(terrain.span());
    let mut cells = terrain
        .blocked
        .take()
        .map_or_else(|| vec![0u8; (span * span) as usize], |g| g.0);
    for y in y0..=y1 {
        for x in x0..=x1 {
            cells[((y + 16) * span + x + 16) as usize] = 1;
        }
    }
    terrain.blocked = Some(Grid(cells));
}

/// A wall down the whole map at x 150..=152 with a gap of `gap` cells from y 60.
fn wall(map: &mut UtcMap, gap: i64) {
    close(map, 150, 0, 152, 59);
    close(map, 150, 60 + gap, 152, 255);
}

fn run(map: &UtcMap) -> Report {
    check::check(&content(), map, "test")
}

fn found(report: &Report, needle: &str) -> bool {
    report.findings.iter().any(|f| f.contains(needle))
}

#[test]
fn an_intended_layout_is_quiet() {
    let report = run(&road(Mission::new(QUIET)).map());
    assert_eq!(report.findings, Vec::<String>::new());
    assert_eq!(report.warnings, Vec::<String>::new());
    assert_eq!(report.scenes, ["greeting"]);
    let gate = report
        .routes
        .iter()
        .find(|r| r.to == "region `gate`")
        .unwrap();
    assert!(gate.reached && gate.large);
    assert!(gate.width.is_some_and(|(w, _)| w > 20), "{gate:?}");
}

#[test]
fn units_that_cannot_walk_out_are_found() {
    let mission = Mission::new(QUIET)
        // Shut in a pocket of 5 × 5 cells.
        .entity(unit("boxed", "unit.warrior", 120.0, 120.0))
        // Standing in closed ground.
        .entity(unit("buried", "unit.warrior", 200.0, 120.0))
        // A scripted spawn shut in too.
        .entity(json!({"id": "late", "definition": "unit.warrior", "position": {"x": 121, "y": 121}, "owner": "none", "activation": "script"}));
    let mut map = road(mission).map();
    close(&mut map, 115, 115, 125, 117);
    close(&mut map, 115, 123, 125, 125);
    close(&mut map, 115, 118, 117, 122);
    close(&mut map, 123, 118, 125, 122);
    close(&mut map, 195, 115, 205, 125);
    let report = run(&map);
    assert!(
        found(
            &report,
            "`boxed` (unit.warrior at 120,120) can walk to only"
        ),
        "{:#?}",
        report.findings
    );
    assert!(found(
        &report,
        "`late` (unit.warrior, a scripted spawn, at 121,121) can walk to only"
    ));
    assert!(found(
        &report,
        "`buried` (unit.warrior at 200,120) cannot take a single step"
    ));
    assert!(!found(&report, "`hero`"));
}

#[test]
fn routes_are_walked_by_the_real_pathfinder_and_measured() {
    let far = |gap: i64| {
        let mut mission = road(Mission::new(QUIET));
        mission
            .regions
            .push(json!({"id": "far", "x": 200, "y": 62, "radius": 4}));
        let mut map = mission.map();
        wall(&mut map, gap);
        let report = run(&map);
        let route = report
            .routes
            .iter()
            .find(|r| r.to == "region `far`")
            .cloned()
            .unwrap();
        (report, route)
    };
    // A gap no body fits through: the ground touches, the pathfinder finds no way.
    let (report, route) = far(0);
    assert!(!route.reached);
    assert!(
        found(
            &report,
            "region `far` cannot be reached from player 1's `hero`"
        ),
        "{:#?}",
        report.findings
    );
    // Wide enough for the army, too narrow for a large unit: flagged, not failed.
    let (report, route) = far(4);
    assert!(route.reached && !route.large, "{route:?}");
    // The narrowest point lies at the gap.
    let (width, (x, _)) = route.width.unwrap();
    assert!(width == 3 && (146..=152).contains(&x), "{route:?}");
    assert!(report.findings.is_empty(), "{:#?}", report.findings);
    assert!(
        report.warnings[0]
            .contains("region `far` cannot be entered from player 1's `hero` by a large unit")
    );
    // Wide open.
    let (report, route) = far(16);
    assert!(route.reached && route.large);
    assert!(report.findings.is_empty() && report.warnings.is_empty());
}

#[test]
fn wave_regions_on_buildings_and_crates_in_corridors_are_found() {
    let mut mission = road(Mission::new(QUIET))
        // Scripted crates: one where the corridor is 5 cells wide, one in the open.
        .entity(json!({"id": "crate.narrow", "definition": "building.hall", "position": {"x": 201.5, "y": 101.5}, "owner": "none", "activation": "script"}))
        .entity(json!({"id": "crate.open", "definition": "building.hall", "position": {"x": 101.5, "y": 181.5}, "owner": "none", "activation": "script"}));
    // The hall stands on the camp's centre.
    mission
        .regions
        .push(json!({"id": "camp", "x": 41, "y": 41, "radius": 20}));
    mission
        .regions
        .push(json!({"id": "landing", "x": 100, "y": 220, "radius": 6}));
    let mut map = mission.map();
    close(&mut map, 160, 0, 255, 98);
    close(&mut map, 160, 104, 255, 255);
    let players = json!([{"player": 2, "team": "enemy", "ai": {"behaviour": "waves", "waves": [
        {"after": 60, "units": [{"unit": "unit.warrior", "count": 2}], "from": {"region": "landing"}, "target": {"region": "camp"}},
    ]}}]);
    map.mission.as_mut().unwrap().players = serde_json::from_value(players).unwrap();
    // Player 2 plays the map's own placements: no base of its own.
    map.player_starts.clear();
    let report = run(&map);
    assert!(
        found(
            &report,
            "region `camp`, which waves use, has its centre (41,41) inside a building's footprint"
        ),
        "{:#?}",
        report.findings
    );
    assert!(found(
        &report,
        "`crate.narrow` (building.hall, a scripted spawn, at 202,102) spawns in a corridor 5 cells wide"
    ));
    assert!(!found(&report, "`crate.open`"));
    let wave = report
        .routes
        .iter()
        .find(|r| r.from == "player 2's wave line 1 from region `landing`")
        .unwrap();
    assert!(wave.reached && !wave.walled, "{wave:?}");
}

#[test]
fn every_scene_plays_skipped_with_its_cast_in_place() {
    let script = r#"
function on_start()
  mission.play("opening")
end

-- Ends held: the player would never get the game back.
function scenes.opening()
  mission.cinematic(true)
  mission.camera_to("road", 0)
  mission.move("hero", 70, 60)
  mission.wait_arrived("hero")
end

-- A key with a dash is refused, and stops the script.
function scenes.closing()
  mission.cinematic(true)
  mission.effect_at("gate-glow", "effect.glow", 60, 60)
  mission.cinematic(false)
  mission.win()
end

-- Spawns a cast member already standing, ends an objective another scene began, and
-- reads where a trigger saw the hero.
function scenes.found()
  mission.cinematic(true)
  mission.spawn("sleeper")
  mission.move("sleeper", mission.get("hx") + 4, mission.get("hy"))
  mission.complete_objective("reach")
  mission.cinematic(false)
end

function on_tick()
  if not mission.playing() and mission.get("seen") == nil then
    local x, y = mission.position("hero")
    mission.set("hx", x)
    mission.set("hy", y)
    mission.set("seen", true)
  end
end
"#;
    let report = run(&road(Mission::new(script)).map());
    assert_eq!(report.scenes, ["closing", "found", "opening"]);
    assert!(
        found(&report, "scene `opening` ends with cinematic mode on"),
        "{:#?}",
        report.findings
    );
    assert!(found(
        &report,
        "scene `closing`, skipped with its cast in place, stops the script"
    ));
    assert!(found(&report, "Effect key"));
    assert!(!found(&report, "scene `found`"), "{:#?}", report.findings);
}
