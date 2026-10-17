//! AI operations on test-owned content: scouting without cheating, assaults, camp
//! clearing, retreats, and the command limits.

mod ai_fixture;

use ai_fixture::*;
use canopy_content::placement::Owner;
use canopy_sim::{
    Action, World,
    ai::state::{CampStatus, Mission, MissionKind, Stage},
    entity::{EntityId, Place},
    orders::Order,
};

fn order(world: &World, id: u32) -> Option<Order> {
    get(world, id).unit.as_ref()?.order.clone()
}

fn alive(world: &World, id: u32) -> bool {
    world
        .state()
        .entities
        .get(EntityId(id))
        .is_some_and(|e| e.hp != Some(0))
}

fn enemy_hall(world: &World) -> u32 {
    all_of(world, P2, HALL)[0]
}

#[test]
fn an_unseen_enemy_base_is_scouted_not_assaulted() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    let soldiers: Vec<u32> = (0..4)
        .map(|i| {
            spawn(
                &mut world,
                &format!("s{i}"),
                SOLDIER,
                P1,
                40.0 + f64::from(i),
                60.0,
            )
        })
        .collect();
    run(&mut world, 30);
    let m = memory(&world, 0).mission.clone().expect("an operation");
    assert_eq!(
        m.kind,
        MissionKind::Scout,
        "it knows only the possible starts"
    );
    assert!(soldiers.iter().all(|&s| alive(&world, s)));
}

#[test]
fn a_seen_enemy_building_is_assaulted_and_attacked() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    let soldiers: Vec<u32> = (0..6)
        .map(|i| {
            spawn(
                &mut world,
                &format!("s{i}"),
                SOLDIER,
                P1,
                196.0 + 2.0 * f64::from(i),
                202.0,
            )
        })
        .collect();
    let hall = enemy_hall(&world);
    run(&mut world, 30);
    let m = memory(&world, 0).mission.clone().expect("an operation");
    assert_eq!(
        (m.kind, m.key.clone()),
        (MissionKind::Assault, hall.to_string())
    );
    let full = get(&world, hall).hp.expect("hp");
    run_until(&mut world, 3000, |w| {
        w.state()
            .entities
            .get(EntityId(hall))
            .is_none_or(|h| h.hp.is_some_and(|hp| hp < full))
    });
    assert!(soldiers.iter().any(|&s| alive(&world, s)));
}

#[test]
fn a_camp_is_cleared_for_hero_experience_and_remembered() {
    let map = skirmish(
        vec![
            place_for("creep-1", CREEP, "none", 75.0, 36.0),
            place_for("creep-2", CREEP, "none", 76.0, 38.0),
        ],
        vec![camp("camp-east", (75.0, 37.0), &["creep-1", "creep-2"])],
    );
    let mut world = world(&map, false);
    let creeps = [
        world
            .state()
            .entities
            .iter()
            .find(|e| e.placement.as_deref() == Some("creep-1"))
            .unwrap()
            .id
            .0,
        world
            .state()
            .entities
            .iter()
            .find(|e| e.placement.as_deref() == Some("creep-2"))
            .unwrap()
            .id
            .0,
    ];
    let hero = all_of(&world, P1, HERO)[0];
    run(&mut world, 30);
    let m = memory(&world, 0).mission.clone().expect("an operation");
    assert_eq!((m.kind, m.key.as_str()), (MissionKind::Camp, "camp-east"));
    run_until(&mut world, 6000, |w| {
        memory(w, 0)
            .camps
            .get("camp-east")
            .is_some_and(|c| c.status == CampStatus::Cleared)
    });
    assert!(creeps.iter().all(|&c| !alive(&world, c)));
    assert!(
        get(&world, hero)
            .progression
            .as_ref()
            .is_some_and(|p| p.experience > 0),
        "the hero earned the camp's experience"
    );
}

#[test]
fn a_squad_withdraws_from_a_visibly_stronger_force() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    let squad: Vec<u32> = (0..3)
        .map(|i| {
            spawn(
                &mut world,
                &format!("s{i}"),
                SOLDIER,
                P1,
                128.0 + f64::from(i),
                128.0,
            )
        })
        .collect();
    for i in 0..12 {
        spawn(
            &mut world,
            &format!("e{i}"),
            SOLDIER,
            P2,
            128.0 + f64::from(i % 4),
            140.0 + f64::from(i / 4),
        );
    }
    let tick = world.tick();
    let target = Place::new(211, 211);
    memory_mut(&mut world, 0).mission = Some(Mission {
        kind: MissionKind::Assault,
        key: enemy_hall(&world).to_string(),
        point: target.clone(),
        started: tick,
        until: tick + 2400,
        stage: Stage::Travel,
        progress_point: Place::new(128, 128),
        progress_tick: tick,
        members: squad.clone(),
    });
    // The strategy would otherwise reconsider at once.
    memory_mut(&mut world, 0).next_strategy = tick + 1000;
    run_until(&mut world, 60, |w| {
        memory(w, 0)
            .mission
            .as_ref()
            .is_some_and(|m| m.kind == MissionKind::Regroup)
    });
    // The survivors head home rather than towards the enemy.
    run(&mut world, 60);
    let home = (HOME.0, HOME.1);
    for &s in squad.iter().filter(|&&s| alive(&world, s)) {
        let towards_home = match order(&world, s) {
            Some(Order::Move { destination, .. }) => {
                f64::from(destination.x) < 128.0 && f64::from(destination.y) < 128.0
            }
            _ => false,
        };
        let p = get(&world, s).position;
        let moved_home =
            f64::from(p.x) / 1000.0 < 128.0 - 2.0 || f64::from(p.y) / 1000.0 < 128.0 - 2.0;
        assert!(
            towards_home || moved_home,
            "soldier {s} withdraws towards {home:?}"
        );
    }
}

/// Distance in cells from an entity to a point.
fn distance_to(world: &World, id: u32, to: (f64, f64)) -> f64 {
    let p = get(world, id).position;
    let (dx, dy) = (
        f64::from(p.x) / 1000.0 - to.0,
        f64::from(p.y) / 1000.0 - to.1,
    );
    (dx * dx + dy * dy).sqrt()
}

#[test]
fn a_member_that_cannot_get_out_does_not_hold_the_squad() {
    // One soldier is boxed in by four houses; the rest of the squad, the hero first, is in
    // the open. The old safe point needed every member to reach it, found none and sent
    // the squad to where the hero already stood, for good.
    let mut world = world(&skirmish(vec![], vec![]), false);
    let hero = all_of(&world, P1, HERO)[0];
    let mut squad = vec![hero];
    squad.extend((0..3).map(|i| {
        spawn(
            &mut world,
            &format!("s{i}"),
            SOLDIER,
            P1,
            38.0 + 2.0 * f64::from(i),
            22.0,
        )
    }));
    for (i, (x, y)) in [(58.0, 20.0), (63.0, 20.0), (60.0, 18.0), (60.0, 23.0)]
        .into_iter()
        .enumerate()
    {
        spawn(&mut world, &format!("house-{i}"), HOUSE, P1, x, y);
    }
    let boxed = spawn(&mut world, "boxed", SOLDIER, P1, 60.0, 20.0);
    run(&mut world, 1);
    let spatial = world.spatial();
    let body = spatial.body_of(get(&world, boxed));
    let from = canopy_sim::spatial::Fixed::cell(60, 20, None);
    assert!(
        (59..=61)
            .flat_map(|x| (19..=21).map(move |y| (x, y)))
            .filter(|&c| c != (60, 20))
            .all(|(x, y)| !spatial.clear_segment(
                from,
                canopy_sim::spatial::Fixed::cell(x, y, None),
                None,
                &body
            )),
        "the houses box the soldier in"
    );
    squad.push(boxed);
    let tick = world.tick();
    let target = Place::new(AWAY.0 as i32, AWAY.1 as i32);
    memory_mut(&mut world, 0).mission = Some(Mission {
        kind: MissionKind::Assault,
        key: enemy_hall(&world).to_string(),
        point: target.clone(),
        started: tick,
        until: tick + 2400,
        stage: Stage::Travel,
        progress_point: target,
        progress_tick: tick,
        members: squad.clone(),
    });
    memory_mut(&mut world, 0).next_strategy = tick + 2000;
    let before = distance_to(&world, hero, AWAY);
    run(&mut world, 400);
    let after = distance_to(&world, hero, AWAY);
    assert!(
        after < before - 20.0,
        "the hero advanced from {before:.1} to {after:.1} cells"
    );
    assert!(distance_to(&world, boxed, (60.0, 20.0)) < 1.0);
}

#[test]
fn a_badly_wounded_fighter_is_pulled_back() {
    let mut world = world(&skirmish(vec![], vec![]), false);
    let soldier = spawn(&mut world, "wounded", SOLDIER, P1, 90.0, 90.0);
    spawn(&mut world, "enemy", SOLDIER, P2, 96.0, 90.0);
    world
        .state_mut()
        .entities
        .get_mut(EntityId(soldier))
        .unwrap()
        .hp = Some(20);
    run_until(&mut world, 40, |w| {
        memory(w, 0)
            .trace
            .iter()
            .any(|t| t.reason == "Preserve a badly wounded fighter")
    });
    let pending: Vec<&Action> = world
        .pending()
        .iter()
        .filter(|q| q.player == 0)
        .map(|q| &q.action)
        .collect();
    let retreating = pending.iter().any(|a| {
        matches!(a, Action::Move { actors, destination, attack_move: None, .. }
            if actors.contains(&soldier) && destination.x < 90 && destination.y < 90)
    }) || matches!(
        order(&world, soldier),
        Some(Order::Move {
            attack_move: false,
            ..
        })
    );
    assert!(retreating, "{pending:?}");
}

#[test]
fn commands_respect_the_beat_limits_and_sequence_scheme() {
    let mut world = world(&skirmish(vec![], vec![]), true);
    let mut last_seq = [0u32; 2];
    let mut issued = [0u32; 2];
    for _ in 0..2500 {
        let clock = world.tick();
        world.step();
        for player in 0..2u8 {
            let fresh: Vec<_> = world
                .pending()
                .iter()
                .filter(|q| q.player == player && q.tick == clock + 1)
                .collect();
            assert!(fresh.len() <= 4, "at most four commands per decision");
            let spending = fresh
                .iter()
                .filter(|q| {
                    matches!(
                        q.action,
                        Action::Build { .. }
                            | Action::Produce { .. }
                            | Action::Upgrade { .. }
                            | Action::Research { .. }
                            | Action::Revive { .. }
                    )
                })
                .count();
            assert!(spending <= 1, "at most one spending command per decision");
            let mut actors: Vec<u32> = fresh
                .iter()
                .flat_map(|q| canopy_sim::ai::actors(&q.action))
                .collect();
            let count = actors.len();
            actors.sort_unstable();
            actors.dedup();
            assert_eq!(actors.len(), count, "one command per actor");
            for q in fresh {
                assert!(q.seq > 1_000_000 && q.seq > last_seq[player as usize]);
                last_seq[player as usize] = q.seq;
                issued[player as usize] += 1;
            }
            assert!(memory(&world, player).pending.len() <= 32);
        }
    }
    assert!(issued.iter().all(|&n| n > 20), "{issued:?}");
    // Every issued command came back with a receipt or is still queued.
    for player in 0..2u8 {
        let m = memory(&world, player);
        let queued = world
            .pending()
            .iter()
            .filter(|q| q.player == player)
            .count() as u32;
        assert_eq!(
            m.metrics.issued,
            m.metrics.accepted + m.metrics.rejected + queued
        );
    }
}

#[test]
fn ai_matches_resume_identically_after_a_save() {
    let map = skirmish(
        vec![place_for("creep-1", CREEP, "none", 75.0, 36.0)],
        vec![camp("camp-east", (75.0, 37.0), &["creep-1"])],
    );
    let start = || world(&map, true);
    let fingerprints = |world: &mut World, ticks: u32| -> Vec<u32> {
        (0..ticks)
            .map(|_| {
                world.step();
                world.fingerprint()
            })
            .collect()
    };
    let mut first = start();
    let reference = fingerprints(&mut first, 1600);
    let mut halfway = start();
    let mut resumed = fingerprints(&mut halfway, 800);
    let save = serde_json::to_string(&halfway.save()).unwrap();
    let mut restored = World::restore(content(), &map, serde_json::from_str(&save).unwrap())
        .unwrap_or_else(|e| panic!("{e}"));
    resumed.extend(fingerprints(&mut restored, 800));
    assert_eq!(resumed, reference);
    assert_eq!(restored.state(), first.state(), "AI memories included");
    assert!(memory(&first, 0).metrics.issued > 0 && memory(&first, 1).metrics.issued > 0);
}

#[test]
fn a_save_with_a_tampered_ai_memory_is_refused() {
    let map = skirmish(vec![], vec![]);
    let mut world = world(&map, true);
    run(&mut world, 200);
    let mut save = world.save();
    save.state.ai.retain(|m| m.player != 1);
    assert!(World::restore(content(), &map, save).is_err());
    let mut save = world.save();
    if let Some(m) = save.state.ai.first_mut() {
        m.briefing ^= 1;
    }
    assert!(World::restore(content(), &map, save).is_err());
}

#[test]
fn the_observer_view_shows_what_each_ai_is_doing() {
    let mut world = world(&skirmish(vec![], vec![]), true);
    run(&mut world, 100);
    let view = world.observer_view();
    assert_eq!(view.ai.len(), 2);
    assert!(
        view.ai
            .iter()
            .all(|s| !s.mission.is_empty() && !s.trace.is_empty())
    );
    assert!(
        world.view(P1).ai.is_empty(),
        "players do not see the AI's plans"
    );
    let _ = Owner::Nobody;
}
