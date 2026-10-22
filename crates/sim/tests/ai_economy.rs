//! The AI colony planner on test-owned content: build order, placement and recovery.

mod ai_fixture;

use ai_fixture::*;
use canopy_sim::{
    World,
    terrain::footprint::{Extent, footprint_cells},
};

fn complete(world: &World, id: u32) -> bool {
    get(world, id).construction.is_none()
}

#[test]
fn a_colony_trains_workers_then_builds_production_and_an_army() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    let workers = all_of(&world, P1, WORKER).len();
    run_until(&mut world, 6000, |w| {
        all_of(w, P1, BARRACKS).iter().any(|&b| complete(w, b))
            && all_of(w, P1, SOLDIER).len() + all_of(w, P1, ARCHER).len() >= 3
    });
    assert!(all_of(&world, P1, WORKER).len() > workers, "trains workers");
    let m = memory(&world, 0);
    assert!(
        m.metrics.builds >= 1 && m.metrics.recruits >= 3,
        "{:?}",
        m.metrics
    );
    assert_eq!(
        m.metrics.rejected, 0,
        "every command it gave was legal: {:?}",
        m.trace
    );
    // The human colony beside it did nothing.
    assert!(all_of(&world, P2, BARRACKS).is_empty());
}

#[test]
fn supply_is_expanded_before_the_army_is_blocked() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    run_until(&mut world, 12_000, |w| !all_of(w, P1, HOUSE).is_empty());
    let supply = world.view(P1).colony.expect("colony").supply;
    assert!(
        supply.committed + 4 > supply.capacity || supply.capacity >= supply.limit,
        "a house is built once supply runs short: {supply:?}"
    );
}

#[test]
fn foundations_keep_deposit_access_and_a_lane_between_buildings() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    run_until(&mut world, 12_000, |w| {
        all_of(w, P1, BARRACKS).len() + all_of(w, P1, HOUSE).len() >= 2
    });
    let catalog = world.catalog();
    let bounds = |id: u32| {
        let e = get(&world, id);
        let d = catalog.get(e.definition);
        footprint_cells(
            f64::from(e.position.x) / 1000.0,
            f64::from(e.position.y) / 1000.0,
            Extent::of(d.footprint.as_ref()),
            f64::from(e.rotation) / 1000.0,
        )
    };
    let mines = all_of(&world, canopy_content::placement::Owner::Nobody, MINE);
    let mut built: Vec<u32> = all_of(&world, P1, BARRACKS);
    built.extend(all_of(&world, P1, HOUSE));
    let mut buildings = built.clone();
    buildings.extend(all_of(&world, P1, HALL));
    for &b in &built {
        let a = bounds(b);
        for &mine in &mines {
            let m = bounds(mine);
            let clearance = 2;
            let touches = a.min_x <= m.max_x + clearance
                && a.max_x >= m.min_x - clearance
                && a.min_y <= m.max_y + clearance
                && a.max_y >= m.min_y - clearance;
            assert!(
                !touches,
                "building {b} blocks the access around deposit {mine}"
            );
        }
        for &other in buildings.iter().filter(|&&o| o != b) {
            let o = bounds(other);
            // The widest trainable ground body is 0.5 cells: a one-cell lane.
            let lane = 1;
            let crowded = a.min_x <= o.max_x + lane
                && a.max_x >= o.min_x - lane
                && a.min_y <= o.max_y + lane
                && a.max_y >= o.min_y - lane;
            assert!(!crowded, "buildings {b} and {other} leave no lane");
        }
    }
}

#[test]
fn a_lost_hall_is_rebuilt_while_another_building_stands() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    run_until(&mut world, 12_000, |w| {
        all_of(w, P1, HOUSE)
            .iter()
            .chain(&all_of(w, P1, BARRACKS))
            .any(|&b| complete(w, b))
    });
    for hall in all_of(&world, P1, HALL) {
        world.remove_entity(canopy_sim::entity::EntityId(hall));
    }
    // Pay for the replacement, as a recovering colony would have to.
    let amber = canopy_content::Id::new(AMBER).unwrap();
    world
        .state_mut()
        .wallets
        .entry(P1)
        .or_default()
        .insert(amber, 200);
    run_until(&mut world, 8000, |w| {
        all_of(w, P1, HALL).iter().any(|&h| complete(w, h))
    });
    assert!(world.state().outcome.is_none());
}

#[test]
fn a_colony_builds_a_well_on_a_deposit_and_gathers_through_it() {
    let spring = place_for("spring", SPRING, "none", 67.5, 35.5);
    let mut world = world(&skirmish(vec![spring], vec![]), false);
    let held = |w: &World| {
        w.state()
            .wallets
            .get(&P1)
            .and_then(|s| s.get(ESSENCE))
            .copied()
            .unwrap_or(0)
    };
    run_until(&mut world, 30_000, |w| held(w) >= 20);
    let well = all_of(&world, P1, WELL);
    assert_eq!(well.len(), 1, "one well");
    let spring = world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some("spring"))
        .expect("spring");
    assert_eq!(
        get(&world, well[0]).position,
        spring.position,
        "on the deposit"
    );
    let gathering = |e: &canopy_sim::entity::Entity| {
        e.unit.as_ref().is_some_and(|u| {
            matches!(u.order, Some(canopy_sim::orders::Order::Gather { target }) if target == spring.id)
        })
    };
    let gatherers = world
        .state()
        .entities
        .iter()
        .filter(|e| gathering(e))
        .count();
    assert!(
        (1..=2).contains(&gatherers),
        "staffed by about its two recommended workers: {gatherers}"
    );
    let m = memory(&world, 0);
    assert_eq!(m.metrics.rejected, 0, "{:?}", m.trace);
}

#[test]
fn a_colony_builds_a_pumping_well_and_leaves_it_to_earn_alone() {
    let spring = place_for("spring", SPRING, "none", 67.5, 35.5);
    let mut world = pumping_world(&skirmish(vec![spring], vec![]), false);
    let held = |w: &World| {
        w.state()
            .wallets
            .get(&P1)
            .and_then(|s| s.get(ESSENCE))
            .copied()
            .unwrap_or(0)
    };
    run_until(&mut world, 30_000, |w| held(w) >= 20);
    let well = all_of(&world, P1, WELL);
    assert_eq!(well.len(), 1, "one well");
    // Nobody is sent to the spring or the well: the well pumps by itself.
    let spring = world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some("spring"))
        .expect("spring")
        .id
        .0;
    let sent = world
        .state()
        .entities
        .iter()
        .filter_map(|e| match e.unit.as_ref()?.order {
            Some(canopy_sim::orders::Order::Gather { target }) => Some(target.0),
            _ => None,
        })
        .filter(|&t| t == spring || t == well[0])
        .count();
    assert_eq!(sent, 0, "no worker gathers at the well");
    let m = memory(&world, 0);
    assert_eq!(m.metrics.rejected, 0, "{:?}", m.trace);
}

#[test]
fn a_networked_colony_builds_inside_its_network_and_its_well_on_the_deposit() {
    // Outside the hall's disc: the well, a source, needs no power to stand there.
    let spring = place_for("spring", SPRING, "none", 67.5, 35.5);
    let mut world = networked_world(&skirmish(vec![spring], vec![]), false);
    let held = |w: &World| {
        w.state()
            .wallets
            .get(&P1)
            .and_then(|s| s.get(ESSENCE))
            .copied()
            .unwrap_or(0)
    };
    run_until(&mut world, 30_000, |w| held(w) >= 20);
    assert_eq!(all_of(&world, P1, WELL).len(), 1, "one well");
    let barracks = all_of(&world, P1, BARRACKS);
    assert!(!barracks.is_empty());
    for b in barracks {
        let p = get(&world, b).position;
        assert!(
            world
                .economy()
                .network()
                .covers(P1, i64::from(p.x), i64::from(p.y)),
            "a barracks stands inside the network"
        );
    }
    let m = memory(&world, 0);
    assert_eq!(m.metrics.rejected, 0, "{:?}", m.trace);
}

#[test]
fn a_colony_whose_field_runs_low_builds_a_hall_at_the_next_field() {
    let far = vec![
        place_for("far-1", MINE, "none", 113.5, 53.5),
        place_for("far-2", MINE, "none", 121.5, 53.5),
    ];
    let mut world = world(&skirmish(far, vec![]), false);
    // The home field is nearly spent.
    for e in world.state_mut().entities.iter_mut() {
        if matches!(e.placement.as_deref(), Some("mine-1" | "mine-2"))
            && let Some(r) = e.resource.as_mut()
        {
            r.amount = 400;
        }
    }
    let field = (117.5, 53.5);
    let at_field = |w: &World, id: u32| {
        let p = get(w, id).position;
        let (dx, dy) = (
            f64::from(p.x) / 1000.0 - field.0,
            f64::from(p.y) / 1000.0 - field.1,
        );
        complete(w, id) && dx * dx + dy * dy < 30.0 * 30.0
    };
    for _ in 0..20_000 {
        if all_of(&world, P1, HALL)
            .iter()
            .any(|&h| at_field(&world, h))
        {
            break;
        }
        world.step();
    }
    let m = memory(&world, 0);
    assert!(
        all_of(&world, P1, HALL)
            .iter()
            .any(|&h| at_field(&world, h)),
        "a second hall stands at the far field: {:?} {:?} {:?}",
        world.state().outcome,
        world.tick(),
        m.trace
    );
    assert_eq!(m.metrics.rejected, 0, "{:?}", m.trace);
}

#[test]
fn a_colony_with_no_room_near_home_builds_where_its_open_ground_goes_on() {
    // Home is a pocket with no room for a barracks; a lane too narrow to build in leads
    // east to a meadow further off than the rings searched around home reach.
    let open = |x: i32, z: i32| {
        ((24..=48).contains(&x) && (24..=58).contains(&z))
            || ((49..=111).contains(&x) && (33..=35).contains(&z))
            || ((112..=200).contains(&x) && (12..=70).contains(&z))
            || ((185..=240).contains(&x) && (180..=240).contains(&z))
    };
    let mut world = world(&closed_skirmish(vec![], open), false);
    run_until(&mut world, 20_000, |w| !all_of(w, P1, BARRACKS).is_empty());
    let barracks = get(&world, all_of(&world, P1, BARRACKS)[0]);
    assert!(barracks.position.x > 112_000, "{:?}", barracks.position);
}

#[test]
fn a_colony_held_to_a_mission_tech_never_plans_what_it_forbids() {
    let army = |w: &World| all_of(w, P1, SOLDIER).len() + all_of(w, P1, ARCHER).len();
    // Free to, the colony upgrades its hall once it has an army.
    let mut free = keep_world(&skirmish(vec![], vec![]), false);
    run_until(&mut free, 30_000, |w| !all_of(w, P1, KEEP).is_empty());
    // Held to a tech without the keep, it never asks for one and spends on its army.
    let mut held = keep_world(&skirmish(vec![], vec![]), false);
    held.limit_tech(0, [canopy_content::Id::new(KEEP).unwrap()]);
    run_until(&mut held, 30_000, |w| army(w) >= 8);
    assert!(all_of(&held, P1, KEEP).is_empty());
    let m = memory(&held, 0);
    assert_eq!(m.metrics.rejected, 0, "{:?}", m.trace);
}
