//! AI heroes learn and cast abilities on test-owned content.

mod ai_fixture;

use ai_fixture::*;
use canopy_sim::{Action, World, action::CastTarget, entity::EntityId};

const SMITE_BINDING: usize = 0;
const RUSH_BINDING: usize = 1;
const WARD_BINDING: usize = 2;

fn ranks(world: &World, hero: u32) -> Vec<u8> {
    get(world, hero)
        .caster
        .as_ref()
        .expect("caster")
        .ranks
        .clone()
}

fn set_experience(world: &mut World, hero: u32, experience: u32) {
    let e = world
        .state_mut()
        .entities
        .get_mut(EntityId(hero))
        .expect("hero");
    e.progression.as_mut().expect("progression").experience = experience;
}

#[test]
fn heroes_learn_new_ranks_first_then_by_the_race_preference() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    let hero = all_of(&world, P1, HERO)[0];
    // Level 1: two abilities open at the same level and rank; the preference picks.
    run_until(&mut world, 100, |w| ranks(w, hero).iter().any(|&r| r > 0));
    assert_eq!(ranks(&world, hero), [0, 1, 0]);
    run(&mut world, 60);
    assert_eq!(ranks(&world, hero), [0, 1, 0], "one rank per level");
    // Level 2 unlocks the ward: newly unlocked ranks come before older ones.
    set_experience(&mut world, hero, 80);
    run_until(&mut world, 100, |w| ranks(w, hero)[WARD_BINDING] == 1);
    assert_eq!(ranks(&world, hero)[SMITE_BINDING], 0);
    set_experience(&mut world, hero, 200);
    run_until(&mut world, 100, |w| ranks(w, hero)[SMITE_BINDING] == 1);
    assert_eq!(ranks(&world, hero)[RUSH_BINDING], 1);
}

#[test]
fn a_spell_is_aimed_by_its_whole_release_program() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    let hero = all_of(&world, P1, HERO)[0];
    {
        let e = world
            .state_mut()
            .entities
            .get_mut(EntityId(hero))
            .expect("hero");
        e.caster.as_mut().expect("caster").ranks = vec![1, 1, 1];
    }
    let at = get(&world, hero).position;
    let (x, y) = (f64::from(at.x) / 1000.0, f64::from(at.y) / 1000.0);
    // Both in reach. The first hit alone is worth the same against either; the blow
    // that follows is worth something only against the healthy one.
    let frail = spawn(&mut world, "frail", SOLDIER, P2, x + 4.0, y);
    let sturdy = spawn(&mut world, "sturdy", SOLDIER, P2, x, y + 4.0);
    world
        .state_mut()
        .entities
        .get_mut(EntityId(frail))
        .unwrap()
        .hp = Some(1);
    let mut target = None;
    for _ in 0..60 {
        world.step();
        target = world.pending().iter().find_map(|q| match &q.action {
            Action::CastAbility {
                actor,
                binding,
                target: CastTarget::Unit { entity },
            } if *actor == hero && binding == "smite" => Some(*entity),
            _ => None,
        });
        if target.is_some() {
            break;
        }
    }
    assert_eq!(target, Some(sturdy));
    assert!(memory(&world, 0).metrics.issued > 0);
}

#[test]
fn a_hero_collects_a_nearby_item() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    let hero = all_of(&world, P1, HERO)[0];
    let at = get(&world, hero).position;
    let (x, y) = (f64::from(at.x) / 1000.0, f64::from(at.y) / 1000.0);
    let charm = spawn(
        &mut world,
        "charm",
        CHARM,
        canopy_content::placement::Owner::Nobody,
        (x + 5.0).round(),
        y.round(),
    );
    run_until(&mut world, 400, |w| {
        get(w, hero)
            .equipment
            .as_ref()
            .is_some_and(|q| q.slots.iter().any(Option::is_some))
    });
    assert!(
        world
            .state()
            .entities
            .get(EntityId(charm))
            .is_none_or(|e| e.item.is_none())
    );
    assert!(memory(&world, 0).metrics.pickups >= 1);
}
