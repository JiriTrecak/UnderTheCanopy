//! Deaths in combat settle the economy: a carrier's goods are lost and its trip ends, and a
//! fallen hero can be revived.

mod economy_fixture;

use canopy_sim::{economy::Cargo, entity::EntityId, state::FactKind};
use economy_fixture::*;
use serde_json::json;

#[test]
fn a_carrier_killed_on_its_way_loses_its_goods() {
    let mut world = world(&sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        place("carrier", WORKER, 60.0, 60.0),
        place_for("raider", SOLDIER, "player.2", 61.0, 60.0),
    ]));
    let carrier = id(&world, "carrier");
    world
        .state_mut()
        .entities
        .get_mut(EntityId(carrier))
        .unwrap()
        .unit
        .as_mut()
        .unwrap()
        .work
        .cargo = Some(Cargo {
        item: canopy_content::Id::new(WOOD).unwrap(),
        amount: 5,
    });
    run_until(&mut world, 200, |w| {
        w.state().entities.get(EntityId(carrier)).is_none()
    });
    let lost = world
        .state()
        .facts
        .iter()
        .find(|f| f.message == "Carrier cargo lost")
        .expect("loss recorded");
    assert_eq!(
        (lost.kind, lost.owner, lost.amount),
        (FactKind::Lost, P1, Some(5))
    );
    assert_eq!(world.state().accounting.lost.get(WOOD).copied(), Some(5));
    assert!(
        world.state().economy.jobs.is_empty(),
        "the delivery ended with it"
    );
    world.verify_economy().unwrap();
}

#[test]
fn a_hero_fallen_in_combat_is_revived() {
    let mut world = world(&sandbox(vec![
        place("sanctuary", SANCTUARY, 19.5, 19.5),
        place("house", HOUSE, 13.5, 33.5),
        place("hero", HERO, 60.0, 60.0),
        place_for("raider", SOLDIER, "player.2", 61.0, 60.0),
    ]));
    fund(&mut world, P1, AMBER, 100);
    let (sanctuary, hero) = (id(&world, "sanctuary"), id(&world, "hero"));
    run_until(&mut world, 200, |w| get(w, hero).fallen);
    assert_eq!(get(&world, hero).hp, Some(0));
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "produce", "actor": sanctuary, "definition": HERO})
        ),
        Some("This hero already belongs to your roster. Revive fallen heroes.".into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "revive", "actor": sanctuary, "hero": hero})
        ),
        None
    );
    run_until(&mut world, 100, |w| !get(w, hero).fallen);
    let e = get(&world, hero);
    assert_eq!(e.hp, Some(300));
    let (x, y) = e.position.cell();
    assert!(
        (x - 20).abs() <= 3 && (y - 24).abs() <= 3,
        "beside the doorway: {x},{y}"
    );
}
