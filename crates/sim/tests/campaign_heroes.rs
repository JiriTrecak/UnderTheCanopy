//! Carried heroes (docs/campaign.md "What carries over"): every hero of player 1 leaves a
//! won mission with its experience, learned abilities, items and permanent bonuses, and
//! the next mission's placed hero of the same definition takes them on, within the hero
//! level cap. A campaign world saves, restores and replays like any other. Test-owned
//! content only.

mod ai_fixture;

use ai_fixture::*;
use canopy_content::{Id, map::UtcMap};
use canopy_sim::{
    MatchSetup, Slot, SlotKind, World,
    campaign::{CampaignStart, CarriedHero},
    items::Bonuses,
};
use std::collections::BTreeMap;

/// A map placing only a hero for player 1 (no starts, no computer bases).
fn map() -> UtcMap {
    let mut map = skirmish(
        vec![place_for("hero", HERO, "player.1", 60.0, 60.0)],
        vec![],
    );
    map.player_starts.clear();
    map.sandbox = true;
    map
}

fn setup() -> MatchSetup {
    MatchSetup {
        seed: 5,
        slots: vec![Slot {
            difficulty: Default::default(),
            player: 0,
            kind: SlotKind::Human,
            name: None,
            team: None,
            hero: None,
            race: None,
        }],
    }
}

fn mission(start: &CampaignStart) -> World {
    World::new_campaign(content(), &map(), setup(), start).unwrap_or_else(|e| panic!("{e}"))
}

fn hero(world: &World) -> &canopy_sim::entity::Entity {
    world
        .state()
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some("hero"))
        .expect("the hero")
}

/// The first mission: the hero reaches level 3, learns two abilities, carries the charm
/// and drank a power-up.
fn veteran() -> CarriedHero {
    let mut world = mission(&CampaignStart::default());
    let id = hero(&world).id;
    let catalog_charm = world.catalog().find(CHARM).expect("charm");
    let e = world.state_mut().entities.get_mut(id).unwrap();
    e.progression.as_mut().unwrap().experience = 200;
    e.caster.as_mut().unwrap().ranks = vec![1, 0, 1];
    let q = canopy_sim::items::equipment(e);
    q.slots[0] = Some(catalog_charm);
    q.bonuses = Some(Bonuses {
        max_hp: 10,
        damage: 0,
        armor: 0,
        max_mana: 0,
    });
    world.step();
    let carried = world.carried_heroes(0);
    assert_eq!(carried.len(), 1);
    carried[0].clone()
}

#[test]
fn a_won_mission_saves_its_heroes() {
    let carried = veteran();
    assert_eq!(carried.definition.as_str(), HERO);
    assert_eq!(carried.experience, 200);
    let learned: Vec<(&str, u8)> = carried
        .learned
        .iter()
        .map(|(b, &r)| (b.as_str(), r))
        .collect();
    assert_eq!(learned, vec![("rush", 0), ("smite", 1), ("ward", 1)]);
    assert_eq!(
        carried.items[0].as_ref().map(|i| i.item.as_str()),
        Some(CHARM)
    );
    assert_eq!(carried.items.len(), 2);
    assert_eq!(carried.bonuses.map(|b| b.max_hp), Some(10));
    // It travels as JSON in the player's progress.
    let text = serde_json::to_string(&carried).unwrap();
    assert_eq!(serde_json::from_str::<CarriedHero>(&text).unwrap(), carried);
}

#[test]
fn the_next_mission_hero_takes_on_the_carried_progress() {
    let carried = veteran();
    let world = mission(&CampaignStart {
        heroes: vec![carried.clone()],
        ..CampaignStart::default()
    });
    let e = hero(&world);
    assert_eq!(e.progression.as_ref().unwrap().experience, 200);
    assert_eq!(e.caster.as_ref().unwrap().ranks, vec![1, 0, 1]);
    assert_eq!(
        e.equipment.as_ref().unwrap().slots[0].map(|d| world.catalog().id(d).as_str()),
        Some(CHARM)
    );
    let stats = canopy_sim::stats::Stats::of(world.catalog(), e, &[]);
    assert_eq!(stats.level, 3);
    assert_eq!(e.hp, Some(stats.max_hp), "arrives at full health");
    assert_eq!(stats.max_hp, 420 + 10, "the power-up's bonus stays");
    // The world carries the same hero on again.
    assert_eq!(world.carried_heroes(0), vec![carried]);
}

#[test]
fn the_level_cap_holds_carried_heroes_back() {
    let carried = veteran();
    // Level 2: experience stops at its threshold; both abilities are still allowed.
    let world = mission(&CampaignStart {
        heroes: vec![carried.clone()],
        hero_level_cap: Some(2),
        ..CampaignStart::default()
    });
    let e = hero(&world);
    assert_eq!(e.progression.as_ref().unwrap().experience, 80);
    assert_eq!(e.caster.as_ref().unwrap().ranks, vec![1, 0, 1]);
    // Level 1: the second ability needs level 2, so the points are given back.
    let world = mission(&CampaignStart {
        heroes: vec![carried],
        hero_level_cap: Some(1),
        ..CampaignStart::default()
    });
    let e = hero(&world);
    assert_eq!(e.progression.as_ref().unwrap().experience, 0);
    assert_eq!(e.caster.as_ref().unwrap().ranks, vec![0, 0, 0]);
    assert_eq!(
        world.state().campaign.as_ref().unwrap().hero_level_cap,
        Some(1)
    );
}

#[test]
fn heroes_of_other_definitions_stay_as_placed() {
    let mut other = veteran();
    other.definition = Id::new("unit.test.someone-else").unwrap();
    let world = mission(&CampaignStart {
        heroes: vec![other],
        ..CampaignStart::default()
    });
    assert_eq!(hero(&world).progression.as_ref().unwrap().experience, 0);
}

#[test]
fn a_campaign_mission_saves_and_replays_identically() {
    let start = CampaignStart {
        difficulty: Default::default(),
        heroes: vec![veteran()],
        hero_level_cap: Some(2),
        locked: [Id::new(SOLDIER).unwrap()].into_iter().collect(),
        variables: BTreeMap::new(),
        player: 0,
    };
    let mut a = mission(&start);
    // A replay of a campaign mission starts from the save of its first tick.
    let first = a.save();
    let mut b = World::restore(
        content(),
        &map(),
        serde_json::from_str(&serde_json::to_string(&first).unwrap()).unwrap(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    for _ in 0..120 {
        a.step();
        b.step();
        assert_eq!(a.fingerprint(), b.fingerprint());
    }
    assert_eq!(
        serde_json::to_value(a.save()).unwrap(),
        serde_json::to_value(b.save()).unwrap()
    );
    // Starting twice from the same campaign start gives the same world.
    let again = mission(&start);
    assert_eq!(
        serde_json::to_value(again.save()).unwrap(),
        serde_json::to_value(first).unwrap()
    );
}

#[test]
fn a_skirmish_computer_player_keeps_to_the_mission_tech() {
    let map = mission_map(
        vec![
            place_for("hero", HERO, "player.1", 60.0, 60.0),
            place_for("p2-hall", HALL, "player.2", 179.5, 199.5),
            place_for("p3-hall", HALL, "player.3", 59.5, 199.5),
        ],
        serde_json::json!([]),
        serde_json::json!([
            {"player": 2, "team": "enemy", "ai": {"behaviour": "skirmish"}},
            {"player": 3, "team": "enemy", "ai": {"behaviour": "base"}}
        ]),
        serde_json::json!([]),
        "x = 1",
    );
    let start = CampaignStart {
        locked: [Id::new(SOLDIER).unwrap()].into_iter().collect(),
        ..CampaignStart::default()
    };
    let world = World::new_campaign(campaign_content(), &map, setup(), &start)
        .unwrap_or_else(|e| panic!("{e}"));
    let state = world.state();
    assert!(!canopy_sim::campaign::allowed(state, P1, SOLDIER));
    assert!(!canopy_sim::campaign::allowed(state, P2, SOLDIER));
    // Behaviours that cost nothing are not held back.
    let p3 = canopy_content::placement::Owner::Player(2);
    assert!(canopy_sim::campaign::allowed(state, p3, SOLDIER));
}
