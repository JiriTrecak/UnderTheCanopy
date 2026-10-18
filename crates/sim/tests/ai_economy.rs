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
