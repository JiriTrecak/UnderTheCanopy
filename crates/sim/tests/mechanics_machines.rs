//! Machine-age mechanics: chaining missiles, minimum weapon range, tower crews, field
//! barriers, pulls, delayed zones with their own pulse, research that unlocks abilities
//! and speeds units up or follows earlier research, and one-at-a-time units.
//! Test-owned content only.

mod abilities_fixture;

use abilities_fixture::*;
use canopy_content::{Content, testing::DataFolder};
use canopy_sim::{World, abilities::EventKind};
use serde_json::{Value, json};
use std::sync::Arc;

/// Loads a spec after `patch` edits its game document.
fn content_with(name: &str, spec: &Spec, patch: impl FnOnce(&mut Value)) -> Arc<Content> {
    let mut game = game(spec);
    patch(&mut game);
    let mut folder = DataFolder::new(game, records());
    folder.abilities = library(spec);
    Arc::new(
        folder
            .load(name)
            .unwrap_or_else(|errors| panic!("{errors}")),
    )
}

fn world_with(content: Arc<Content>, entities: Vec<Value>) -> World {
    World::new(content, &map(entities), setup(false)).unwrap_or_else(|e| panic!("{e}"))
}

fn definition_mut<'a>(game: &'a mut Value, id: &str) -> &'a mut Value {
    game["definitions"]
        .as_array_mut()
        .expect("definitions")
        .iter_mut()
        .find(|d| d["id"] == id)
        .expect("definition")
}

fn attack(world: &mut World, player: u8, actor: u32, target: u32) {
    command(
        world,
        player,
        json!({"type": "attack", "actors": [actor], "target": target, "force": null, "append": null}),
    );
}

#[test]
fn chaining_missiles_jump_to_nearby_enemies_with_less_damage() {
    let spec = Spec::default();
    let content = content_with("chain", &spec, |g| {
        definition_mut(g, "unit.archer")["behaviors"]["combat"]["chain"] =
            json!({"bounces": 2, "radius": 4.0, "retentionPermille": 500});
    });
    let mut world = world_with(
        content,
        vec![
            placed("a", "unit.archer", 100.0, 100.0, 1),
            placed("b", "unit.golem", 108.0, 100.0, 2),
            placed("c", "unit.golem", 110.0, 101.0, 2),
            placed("d", "unit.golem", 112.0, 100.0, 2),
            placed("e", "unit.golem", 130.0, 100.0, 2),
        ],
    );
    let (a, b) = (id(&world, "a"), id(&world, "b"));
    let golems: Vec<u32> = ["b", "c", "d", "e"].iter().map(|p| id(&world, p)).collect();
    command(
        &mut world,
        2,
        json!({"type": "hold", "actors": golems, "append": null}),
    );
    attack(&mut world, 1, a, b);
    // One shot: the first missile and its two jumps land within a second and a half.
    run(&mut world, 75);
    let lost = |w: &World, p: &str| 200 - hp(w, p);
    assert!(lost(&world, "b") > 0, "the target is hit");
    assert!(
        lost(&world, "c") > 0 && lost(&world, "d") > 0,
        "both jumps land"
    );
    assert!(
        lost(&world, "c") < lost(&world, "b"),
        "each jump keeps less damage"
    );
    assert_eq!(lost(&world, "e"), 0, "nobody out of reach is hit");
}

#[test]
fn siege_weapons_cannot_hit_targets_inside_their_minimum_range() {
    let spec = Spec::default();
    let content = content_with("min-range", &spec, |g| {
        let combat = &mut definition_mut(g, "unit.archer")["behaviors"]["combat"];
        combat["minRange"] = json!(4.0);
        combat["aggroRange"] = json!(12);
    });
    let mut world = world_with(
        content,
        vec![
            placed("a", "unit.archer", 100.0, 100.0, 1),
            placed("near", "unit.golem", 102.0, 100.0, 2),
            placed("far", "unit.golem", 108.0, 100.0, 2),
        ],
    );
    let (a, near, far) = (id(&world, "a"), id(&world, "near"), id(&world, "far"));
    // Golems stand still: hold them so the test sees the archer alone choose.
    command(
        &mut world,
        2,
        json!({"type": "hold", "actors": [near, far], "append": null}),
    );
    run(&mut world, 160);
    assert_eq!(hp(&world, "near"), 200, "too close to shoot");
    assert!(hp(&world, "far") < 200, "it shoots the one further out");
    // An explicit order on the close target does nothing either.
    attack(&mut world, 1, a, near);
    run(&mut world, 80);
    assert_eq!(hp(&world, "near"), 200);
}

#[test]
fn a_tower_crews_itself_fights_and_goes_with_its_tower() {
    let mut spec = Spec::default();
    spec.definitions.push(definition(
        "unit.gunner",
        "unit",
        json!({"body": {"maxHp": 50, "armor": 0, "armorType": "armor.light"}, "supplyCost": 0, "vision": 16,
            "selectable": false, "unitNature": "mechanical",
            "dimensions": {"radius": 0.4, "height": 1.5, "formationSpacing": 1.2},
            "behaviors": {"combat": {"damage": 9, "damageType": "damage.pierce", "range": 10.0, "cooldownTicks": 40,
                "aggroRange": 12, "attack": {"windupTicks": 6, "recoveryTicks": 6, "rangeBuffer": 0.75},
                "projectile": {"speed": 30}}}}),
    ));
    spec.definitions.push(definition(
        "building.tower",
        "building",
        json!({"body": {"maxHp": 300, "armor": 0, "armorType": "armor.structure"}, "footprint": {"width": 4, "depth": 4},
            "entrance": {"x": 0.5, "y": 2.5}, "vision": 14,
            "garrison": {"capacity": 1, "accepts": [], "height": 3.0, "crew": "unit.gunner"}}),
    ));
    let content = content_with("crew", &spec, |_| {});
    let mut world = world_with(
        content,
        vec![
            placed("t", "building.tower", 101.5, 101.5, 1),
            placed("w", "unit.golem", 108.0, 102.0, 2),
        ],
    );
    let w = id(&world, "w");
    command(
        &mut world,
        2,
        json!({"type": "hold", "actors": [w], "append": null}),
    );
    run(&mut world, 3);
    let crew: Vec<u32> = world
        .state()
        .entities
        .iter()
        .filter(|e| world.catalog().id(e.definition).as_str() == "unit.gunner")
        .map(|e| e.id.0)
        .collect();
    assert_eq!(crew.len(), 1, "one gunner stands in the tower");
    // It takes no orders.
    command(&mut world, 1, json!({"type": "stop", "actors": [crew[0]]}));
    run(&mut world, 120);
    assert!(hp(&world, "w") < 200, "the crew shoots from the tower");
    let tower = canopy_sim::entity::EntityId(id(&world, "t"));
    world.state_mut().entities.get_mut(tower).expect("tower").hp = Some(1);
    attack(&mut world, 2, w, tower.0);
    run(&mut world, 200);
    assert!(!alive(&world, "t"));
    assert!(
        world
            .state()
            .entities
            .get(canopy_sim::entity::EntityId(crew[0]))
            .is_none_or(|e| e.hp == Some(0)),
        "the crew goes with its tower"
    );
}

/// A self spell giving the caster a frontal barrier for `ticks`.
fn barrier_spec() -> Spec {
    let mut spec = Spec::default();
    give(
        &mut spec,
        spell(
            "ability.test.field",
            "self",
            &["ally"],
            0,
            0,
            10,
            0,
            0,
            json!([{"op": "status", "target": "target", "id": "field", "amount": 400, "polarity": "positive",
                "dispel": false, "barrier": {"radius": 2.0, "arcDegrees": 140.0}}]),
        ),
        "field",
    );
    spec
}

#[test]
fn a_field_barrier_stops_missiles_from_the_front_only() {
    // The caster faces +y (rotation 0): an archer north of it shoots into the pane.
    for (archer_y, blocked) in [(108.0, true), (92.0, false)] {
        let spec = barrier_spec();
        let mut world = world(
            "barrier",
            &spec,
            vec![
                placed("c", "unit.caster", 100.0, 100.0, 1),
                placed("x", "unit.archer", 100.0, archer_y, 2),
            ],
        );
        let (c, x) = (id(&world, "c"), id(&world, "x"));
        command(
            &mut world,
            1,
            json!({"type": "hold", "actors": [c], "append": null}),
        );
        run(&mut world, 2);
        cast_at(&mut world, 1, c, "field", c);
        run(&mut world, 4);
        command(
            &mut world,
            1,
            json!({"type": "hold", "actors": [c], "append": null}),
        );
        run(&mut world, 1);
        assert_eq!(statuses(&world, "c"), vec!["field".to_string()]);
        attack(&mut world, 2, x, c);
        run(&mut world, 200);
        let shot = world.state().combat.missiles.iter().any(|m| m.blocked);
        if blocked {
            assert_eq!(hp(&world, "c"), 150, "the pane takes every shot");
        } else {
            assert!(hp(&world, "c") < 150, "shots from behind pass");
            assert!(!shot);
        }
    }
}

#[test]
fn shells_burst_against_a_field_barrier() {
    for shielded in [true, false] {
        let spec = barrier_spec();
        let content = content_with(
            if shielded { "pane-shell" } else { "open-shell" },
            &spec,
            |g| {
                let combat = &mut definition_mut(g, "unit.archer")["behaviors"]["combat"];
                combat.as_object_mut().expect("combat").remove("projectile");
                combat["shell"] =
                    json!({"flightTicks": 30, "radius": 1.2, "slowPermille": 0, "slowTicks": 0});
                combat["damageType"] = json!("damage.blade");
            },
        );
        let mut world = world_with(
            content,
            vec![
                placed("c", "unit.caster", 100.0, 100.0, 1),
                placed("x", "unit.archer", 100.0, 108.0, 2),
            ],
        );
        let (c, x) = (id(&world, "c"), id(&world, "x"));
        command(
            &mut world,
            1,
            json!({"type": "hold", "actors": [c], "append": null}),
        );
        run(&mut world, 2);
        if shielded {
            cast_at(&mut world, 1, c, "field", c);
            run(&mut world, 4);
            command(
                &mut world,
                1,
                json!({"type": "hold", "actors": [c], "append": null}),
            );
        }
        run(&mut world, 1);
        attack(&mut world, 2, x, c);
        run(&mut world, 200);
        if shielded {
            assert_eq!(hp(&world, "c"), 150, "every shell bursts on the pane");
        } else {
            assert!(hp(&world, "c") < 150, "without the pane the shells land");
        }
    }
}

#[test]
fn a_vortex_pulls_enemies_toward_its_caster() {
    let mut spec = Spec::default();
    let mut vortex = spell(
        "ability.test.vortex",
        "self",
        &["ally"],
        0,
        0,
        10,
        0,
        0,
        json!([{"op": "pull", "target": "target", "query": {"center": "caster", "radius": 16, "relations": ["enemy"]},
            "amount": 40, "toward": "caster", "minDistance": 2.0}]),
    );
    vortex["persistent"] = json!({"durationTicks": 200, "intervalTicks": 4, "anchor": "caster"});
    give(&mut spec, vortex, "vortex");
    let mut world = world(
        "vortex",
        &spec,
        vec![
            placed("c", "unit.caster", 100.0, 100.0, 1),
            placed("w", "unit.golem", 110.0, 100.0, 2),
        ],
    );
    let (c, w) = (id(&world, "c"), id(&world, "w"));
    command(
        &mut world,
        2,
        json!({"type": "hold", "actors": [w], "append": null}),
    );
    command(
        &mut world,
        1,
        json!({"type": "hold", "actors": [c], "append": null}),
    );
    run(&mut world, 1);
    cast_at(&mut world, 1, c, "vortex", c);
    run(&mut world, 30);
    let x = entity(&world, "w").position.x;
    assert!(x < 110_000, "pulled in: {x}");
    run(&mut world, 120);
    // The caster may step in to fight; the pull brings the golem to it.
    let (p, q) = (entity(&world, "w").position, entity(&world, "c").position);
    let gap2 = (i64::from(p.x - q.x).pow(2) + i64::from(p.y - q.y).pow(2)) as f64 / 1e6;
    assert!(gap2 <= 2.6 * 2.6, "pulled in to the caster: {gap2}");
}

fn falling_zone() -> Spec {
    let mut spec = Spec::default();
    let mut fall = spell(
        "ability.test.fall",
        "point",
        &["enemy"],
        20,
        0,
        400,
        0,
        0,
        json!([{"op": "damage", "target": "target", "amount": 50, "damageType": "spell"}]),
    );
    fall["targeting"]["radius"] = json!(3);
    fall["persistent"] = json!({"durationTicks": 40, "intervalTicks": 10, "anchor": "point", "endsWithCaster": false,
        "delayTicks": 30,
        "pulse": [{"op": "damage", "target": "target", "amount": 5, "damageType": "spell"}]});
    give(&mut spec, fall, "fall");
    spec
}

#[test]
fn a_delayed_zone_lands_after_its_warning_then_burns_with_its_pulse() {
    let script = Script::new(
        "delayed-zone",
        falling_zone(),
        vec![
            placed("c", "unit.caster", 100.0, 100.0, 1),
            placed("w", "unit.golem", 110.0, 100.0, 2),
        ],
        120,
        vec![
            (0, 2, json!({"type": "hold", "actors": [2], "append": null})),
            (1, 1, cast_at_point(1, "fall", 110, 100)),
        ],
    );
    let played = script.play();
    let released = played.of(EventKind::Released)[0].tick;
    let impact = played.of(EventKind::Impact);
    assert_eq!(impact.len(), 1, "{}", played.dump());
    assert_eq!(impact[0].tick, released + 30, "lands after the warning");
    let hits: Vec<(u32, i64)> = played
        .at(EventKind::Damaged, "w")
        .iter()
        .map(|e| (e.tick, e.amount.unwrap_or(0)))
        .collect();
    assert_eq!(hits[0], (released + 30, 50), "the blow lands first");
    assert_eq!(hits.len(), 4, "then three burning pulses: {hits:?}");
    assert!(hits[1..].iter().all(|&(_, d)| d == 5));
    // A save in the middle of the warning plays on the same.
    let again = script.play_reloading(Some(15));
    assert_eq!(again.prints, played.prints);
}

#[test]
fn research_follows_research_unlocks_abilities_and_speeds_units() {
    let mut spec = Spec::default();
    let shout = spell(
        "ability.test.shout",
        "self",
        &["ally"],
        0,
        0,
        10,
        0,
        0,
        json!([{"op": "heal", "target": "target", "amount": 1}]),
    );
    let mut shout = with(shout, json!({"ranks": [{}, {}]}));
    shout["id"] = json!("ability.test.shout");
    spec.abilities.push(shout);
    spec.warrior.push(bind("shout", "ability.test.shout", 0));
    spec.definitions.push(definition(
        "building.lab",
        "building",
        json!({"body": {"maxHp": 500, "armor": 0, "armorType": "armor.structure"}, "footprint": {"width": 4, "depth": 4},
            "entrance": {"x": 0.5, "y": 2.5}, "vision": 10,
            "behaviors": {"playerControl": {}, "research": {"outputs": ["research.test.one", "research.test.two"], "queueCapacity": 2}}}),
    ));
    let content = content_with("research-chain", &spec, |g| {
        g["rules"]["research"] = json!({
            "research.test.one": {"name": "One", "description": "", "icon": "asset.icon", "priority": 1, "items": [], "workTicks": 10,
                "effects": [{"units": ["unit.warrior"], "abilityRanks": {"shout": 1}, "attackSpeedPermille": 500}]},
            "research.test.two": {"name": "Two", "description": "", "icon": "asset.icon", "priority": 1, "items": [], "workTicks": 10,
                "requires": ["research.test.one"],
                "effects": [{"units": ["unit.warrior"], "abilityRanks": {"shout": 2}, "moveSpeedPermille": 250}]}
        });
    });
    let mut world = world_with(
        content,
        vec![
            placed("hall", "building.hall", 81.5, 81.5, 1),
            placed("lab", "building.lab", 101.5, 101.5, 1),
            placed("w", "unit.warrior", 110.0, 110.0, 1),
        ],
    );
    let lab = id(&world, "lab");
    let rank = |w: &World| entity(w, "w").caster.as_ref().map_or(0, |c| c.ranks[0]);
    assert_eq!(rank(&world), 0);
    let before = canopy_sim::stats::Stats::of(world.catalog(), entity(&world, "w"), &[]);
    // The second level waits for the first.
    command(
        &mut world,
        1,
        json!({"type": "research", "actor": lab, "research": "research.test.two"}),
    );
    run(&mut world, 2);
    assert!(
        errors(&world).iter().any(|e| e == "Requires One"),
        "{:?}",
        errors(&world)
    );
    command(
        &mut world,
        1,
        json!({"type": "research", "actor": lab, "research": "research.test.one"}),
    );
    run(&mut world, 30);
    assert_eq!(rank(&world), 1, "the research unlocks the ability");
    let research = world.state().economy.researched(player(1)).to_vec();
    let after = canopy_sim::stats::Stats::of(world.catalog(), entity(&world, "w"), &research);
    assert!(
        after.cooldown_ticks < before.cooldown_ticks,
        "attacks come faster"
    );
    command(
        &mut world,
        1,
        json!({"type": "research", "actor": lab, "research": "research.test.two"}),
    );
    run(&mut world, 30);
    assert_eq!(rank(&world), 2);
    let research = world.state().economy.researched(player(1)).to_vec();
    assert_eq!(
        canopy_sim::stats::move_speed_permille(world.catalog(), entity(&world, "w"), &research),
        1250
    );
}

#[test]
fn a_unit_limited_to_one_at_a_time_refuses_a_second_order() {
    let spec = Spec::default();
    let content = content_with("max-owned", &spec, |g| {
        definition_mut(g, "unit.warrior")["maxOwned"] = json!(1);
        definition_mut(g, "building.barracks")["behaviors"]["playerControl"] = json!({});
    });
    let mut world = world_with(
        content,
        vec![
            placed("hall", "building.hall", 81.5, 81.5, 1),
            placed("b", "building.barracks", 101.5, 101.5, 1),
        ],
    );
    let b = id(&world, "b");
    let produce = json!({"type": "produce", "actor": b, "definition": "unit.warrior"});
    command(&mut world, 1, produce.clone());
    run(&mut world, 2);
    command(&mut world, 1, produce.clone());
    run(&mut world, 2);
    assert!(
        errors(&world).iter().any(|e| e.starts_with("Only 1")),
        "{:?}",
        errors(&world)
    );
    run(&mut world, 80);
    command(&mut world, 1, produce);
    run(&mut world, 2);
    let warriors = world
        .state()
        .entities
        .iter()
        .filter(|e| world.catalog().id(e.definition).as_str() == "unit.warrior")
        .count();
    assert_eq!(warriors, 1, "still one while the first lives");
}
