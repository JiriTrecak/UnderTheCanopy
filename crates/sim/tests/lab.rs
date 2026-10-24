//! The combat lab: a scenario runs in the real simulation, its closed cells closed and its
//! orders given on their ticks, and what the groups did is measured.

mod ai_fixture;

use ai_fixture::*;
use canopy_content::{
    Id,
    lab::{Lab, LabOrder, LabUnit, OrderKind},
    placement::Owner,
};
use canopy_sim::lab::Run;

fn soldier(x: f64, y: f64, owner: Owner, group: &str) -> LabUnit {
    LabUnit {
        definition: Id::new(SOLDIER).unwrap(),
        owner,
        x,
        y,
        facing: 0.0,
        group: Some(group.into()),
    }
}

fn walled(gap: std::ops::Range<u32>) -> Lab {
    let mut lab = Lab::new("Wall", 64);
    let mut cells = lab.closed_cells();
    for y in 0..64 {
        if !gap.contains(&y) {
            cells[y as usize * 64 + 32] = true;
            cells[y as usize * 64 + 33] = true;
        }
    }
    lab.set_closed_cells(&cells);
    lab.duration = 1600;
    lab
}

#[test]
fn a_group_crosses_a_wall_through_its_gap_and_is_measured() {
    let mut lab = walled(28..36);
    for i in 0..6 {
        lab.units.push(soldier(
            10.0,
            26.0 + 2.0 * f64::from(i),
            Owner::Player(0),
            "left",
        ));
    }
    lab.orders.push(LabOrder {
        tick: 0,
        group: "left".into(),
        order: OrderKind::Move,
        x: Some(54.0),
        y: Some(32.0),
        target: None,
    });
    assert!(
        lab.problems(&content()).is_empty(),
        "{:?}",
        lab.problems(&content())
    );
    let mut run = Run::new(content(), &lab).unwrap();
    while !run.finished() && run.measures().groups[0].arrived < 6 {
        run.step();
    }
    let m = &run.measures().groups[0];
    assert_eq!((m.units, m.alive, m.arrived), (6, 6, 6), "{m:?}");
    assert!(m.first_arrival.unwrap() <= m.last_arrival.unwrap());
    for id in run.group("left") {
        let x = run.world.state().entities.get(id).unwrap().position.x;
        assert!(x > 34_000, "through the wall: {x}");
    }
    // The wall is closed: no body stands in it.
    let state = run.world.state();
    assert!(state.entities.iter().all(|e| e.unit.is_none()
        || !(32_000..34_000).contains(&e.position.x)
        || (28_000..36_000).contains(&e.position.y)));
}

#[test]
fn orders_from_two_sides_meet_in_a_fight() {
    let mut lab = Lab::new("Fight", 64);
    lab.duration = 1200;
    for i in 0..4 {
        let y = 28.0 + 2.0 * f64::from(i);
        lab.units.push(soldier(16.0, y, Owner::Player(0), "west"));
        lab.units.push(soldier(48.0, y, Owner::Player(1), "east"));
    }
    for (group, x) in [("west", 48.0), ("east", 16.0)] {
        lab.orders.push(LabOrder {
            tick: 0,
            group: group.into(),
            order: OrderKind::AttackMove,
            x: Some(x),
            y: Some(32.0),
            target: None,
        });
    }
    let mut run = Run::new(content(), &lab).unwrap();
    while !run.finished() {
        run.step();
    }
    let m = run.measures();
    assert!(m.groups.iter().all(|g| g.damage_taken > 0), "{m:?}");
    assert!(m.groups.iter().any(|g| g.alive < g.units), "{m:?}");
}

#[test]
fn recording_the_steering_changes_nothing() {
    let mut lab = walled(30..34);
    for i in 0..12 {
        lab.units.push(soldier(
            8.0 + 2.0 * f64::from(i % 4),
            26.0 + 3.0 * f64::from(i / 4),
            Owner::Player(0),
            "crowd",
        ));
    }
    lab.orders.push(LabOrder {
        tick: 0,
        group: "crowd".into(),
        order: OrderKind::Move,
        x: Some(56.0),
        y: Some(32.0),
        target: None,
    });
    lab.duration = 600;
    let run = |probe: bool| {
        let mut run = Run::new(content(), &lab).unwrap();
        run.world.set_probe(probe);
        let mut traced = 0;
        while !run.finished() {
            run.step();
            traced += run.world.probe().map_or(0, |p| p.units.len());
        }
        (run.world.fingerprint(), run.measures().clone(), traced)
    };
    let (plain, measures, none) = run(false);
    let (probed, probed_measures, traced) = run(true);
    assert_eq!(plain, probed);
    assert_eq!(measures, probed_measures);
    assert_eq!(none, 0);
    assert!(traced > 0);
}

#[test]
fn a_hand_order_walks_a_unit_through_idle_allies() {
    let mut lab = Lab::new("Through", 64);
    lab.duration = 1200;
    for i in 0..16 {
        lab.units.push(soldier(
            28.0 + 2.0 * f64::from(i % 4),
            26.0 + 2.0 * f64::from(i / 4),
            Owner::Player(0),
            "idle",
        ));
    }
    lab.units
        .push(soldier(10.0, 29.0, Owner::Player(0), "walker"));
    let mut run = Run::new(content(), &lab).unwrap();
    let walker = run.group("walker")[0];
    assert!(run.command(
        0,
        canopy_sim::Action::Move {
            actors: vec![walker.0],
            destination: canopy_sim::action::CellPoint {
                x: 54,
                y: 29,
                surface: None,
            },
            attack_move: None,
            append: None,
        },
    ));
    while !run.finished() && run.world.state().entities.get(walker).unwrap().position.x < 50_000 {
        run.step();
    }
    assert!(
        run.world.state().entities.get(walker).unwrap().position.x >= 50_000,
        "the walker gets through the idle block"
    );
}

#[test]
fn every_melee_approach_brings_a_group_into_reach() {
    use canopy_sim::movement::tuning::{Approach, Tuning};
    let mut lab = Lab::new("Ring", 64);
    lab.duration = 400;
    for i in 0..10 {
        lab.units.push(soldier(
            12.0 + 2.0 * f64::from(i % 2),
            24.0 + 2.0 * f64::from(i / 2),
            Owner::Player(0),
            "pack",
        ));
    }
    let mut target = soldier(24.0, 28.0, Owner::Player(1), "target");
    target.definition = Id::new(HERO).unwrap();
    lab.units.push(target);
    lab.orders.push(LabOrder {
        tick: 0,
        group: "pack".into(),
        order: OrderKind::Attack,
        x: None,
        y: None,
        target: Some("target".into()),
    });
    lab.orders.push(LabOrder {
        tick: 0,
        group: "target".into(),
        order: OrderKind::Hold,
        x: None,
        y: None,
        target: None,
    });
    for approach in Approach::ALL {
        let tuning = Tuning {
            approach,
            ..Tuning::default()
        };
        let mut run = Run::tuned(content(), &lab, tuning).unwrap();
        while !run.finished() {
            run.step();
        }
        let pack = &run.measures().groups[0];
        assert!(pack.most_in_reach >= 4, "{approach:?}: {pack:?}");
    }
}
