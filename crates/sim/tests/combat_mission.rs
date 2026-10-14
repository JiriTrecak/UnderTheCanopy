//! Mission effects that belong to units: orders, facing interruptions, damage, recovery.

#[path = "mission_fixture/mod.rs"]
mod mission_fixture;

use canopy_sim::{
    entity::{EntityId, Place},
    orders::Order,
};
use mission_fixture::*;

fn order(world: &canopy_sim::World, placement: &str) -> Option<Order> {
    entity(world, placement)?.unit.as_ref()?.order.clone()
}

fn hp(world: &canopy_sim::World, placement: &str) -> u32 {
    entity(world, placement).and_then(|e| e.hp).unwrap_or(0)
}

#[test]
fn scripted_damage_lands_at_the_next_resolution_as_spell_damage() {
    let mut world = world(r#"function on_start() mission.damage("guard", 30, "hero") end"#);
    world.step();
    assert_eq!(hp(&world, "guard"), 100);
    assert_eq!(world.state().combat.scripted.len(), 1);
    world.step();
    // Spell damage ignores armour; the rules' spell row is 100 % here.
    assert_eq!(hp(&world, "guard"), 70);
    assert!(world.state().combat.scripted.is_empty());
}

#[test]
fn scripted_damage_can_kill_and_credits_nobody() {
    let mut world = world(r#"function on_start() mission.damage("guard", 500) end"#);
    steps(&mut world, 2);
    assert!(entity(&world, "guard").is_none());
    // A kill credited to nobody pays no experience.
    let hero = entity(&world, "hero").unwrap();
    assert_eq!(hero.progression.as_ref().unwrap().experience, 0);
}

#[test]
fn recovery_restores_full_health_for_the_level() {
    let mut world = world(
        r#"function on_start() mission.damage("hero", 40) end
           function on_tick() if mission.tick() == 4 then mission.recover("hero") end end"#,
    );
    steps(&mut world, 2);
    assert_eq!(hp(&world, "hero"), 60);
    steps(&mut world, 3);
    assert_eq!(hp(&world, "hero"), 100);
}

#[test]
fn scripted_orders_become_unit_orders() {
    let mut world = world(
        r#"function on_start()
             mission.move("guard", 70, 60)
             mission.attack_move("hero", 50, 50)
             mission.follow("volunteer", "hero")
           end"#,
    );
    world.step();
    assert_eq!(
        order(&world, "guard"),
        Some(Order::Move {
            destination: Place::new(70, 60),
            attack_move: false
        })
    );
    assert_eq!(
        order(&world, "hero"),
        Some(Order::Move {
            destination: Place::new(50, 50),
            attack_move: true
        })
    );
    let hero = EntityId(entity(&world, "hero").unwrap().id.0);
    assert_eq!(
        order(&world, "volunteer"),
        Some(Order::Follow {
            target: hero,
            escort: true
        })
    );
    let mut world = world_with_attack();
    world.step();
    let guard = EntityId(entity(&world, "guard").unwrap().id.0);
    assert_eq!(
        order(&world, "hero"),
        Some(Order::Attack {
            target: guard,
            force: true
        })
    );
    // Facing interrupts the order; the turn itself is mission state.
    steps(&mut world, 4);
    assert_eq!(order(&world, "hero"), None);
}

fn world_with_attack() -> canopy_sim::World {
    world(
        r#"function on_start() mission.attack("hero", "guard") end
           function on_tick() if mission.tick() == 4 then mission.face("hero", 60, 70) end end"#,
    )
}
