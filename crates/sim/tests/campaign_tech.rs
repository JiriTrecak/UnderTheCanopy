//! A campaign's tech in a mission (docs/campaign.md "Tech"): locked tech is refused for
//! building, training, research and upgrades; `mission.allow` and `mission.forbid` change
//! it mid-mission; a computer player can be held back the same way; campaign variables
//! travel through `campaign_get` / `campaign_set`. Test-owned content only.

mod economy_fixture;
mod mission_fixture;

use canopy_content::{Id, map::Mission};
use canopy_sim::{World, campaign::CampaignStart, mission::state::Variable};
use economy_fixture::*;
use serde_json::{Value, json};
use std::collections::BTreeSet;

const REFUSED: &str = "message.not-available-in-mission";

fn ids(list: &[&str]) -> BTreeSet<Id> {
    list.iter().map(|i| Id::new(*i).unwrap()).collect()
}

/// A mission on the economy fixture: a hall, a forge and a worker for player 1, `script`
/// running, no starts.
fn mission(script: &str) -> canopy_content::map::UtcMap {
    let mut map = sandbox(vec![
        place("hall", HALL, 19.5, 19.5),
        place("forge", FORGE, 41.5, 25.5),
        place("worker", WORKER, 26.0, 30.0),
    ]);
    map.sandbox = false;
    map.player_starts.clear();
    map.mission = Some(Mission {
        objectives: None,
        script: script.into(),
        regions: Vec::new(),
        players: Vec::new(),
        cameras: None,
        autoplay: None,
    });
    map
}

fn campaign_world(script: &str, locked: &[&str]) -> World {
    let start = CampaignStart {
        locked: ids(locked),
        ..CampaignStart::default()
    };
    let mut world = World::new_campaign(content(), &mission(script), setup(2), &start)
        .unwrap_or_else(|e| panic!("{e}"));
    fund(&mut world, P1, AMBER, 500);
    fund(&mut world, P1, WOOD, 500);
    world
}

fn produce(building: u32, definition: &str) -> Value {
    json!({"type": "produce", "actor": building, "definition": definition})
}

#[test]
fn locked_tech_is_refused_everywhere_and_shown_in_the_view() {
    let mut world = campaign_world("function on_start() end", &[WORKER, TOOLS, HOUSE, KEEP]);
    let (hall, forge, worker) = (
        id(&world, "hall"),
        id(&world, "forge"),
        id(&world, "worker"),
    );
    assert_eq!(
        try_command(&mut world, produce(hall, WORKER)),
        Some(REFUSED.into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "research", "actor": forge, "research": TOOLS})
        ),
        Some(REFUSED.into())
    );
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "build", "actors": [worker], "definition": HOUSE, "position": {"x": 29.5, "y": 29.5}})
        ),
        Some(REFUSED.into())
    );
    assert_eq!(
        try_command(&mut world, json!({"type": "upgrade", "actor": hall})),
        Some(REFUSED.into())
    );
    let view = world.view(P1);
    let locked: Vec<&str> = view
        .colony
        .as_ref()
        .unwrap()
        .locked
        .iter()
        .map(Id::as_str)
        .collect();
    assert_eq!(locked, vec![HOUSE, KEEP, TOOLS, WORKER]);
    // Player 2 has no limit.
    assert!(world.view(P2).colony.unwrap().locked.is_empty());
    // A plain match has none either.
    let mut plain = economy_fixture::world(&sandbox(vec![place("hall", HALL, 19.5, 19.5)]));
    fund(&mut plain, P1, AMBER, 100);
    let hall = id(&plain, "hall");
    assert_eq!(try_command(&mut plain, produce(hall, WORKER)), None);
}

#[test]
fn scripts_allow_and_forbid_tech_mid_mission() {
    let script = r#"
function on_start() end
function on_tick()
  if mission.tick() == 8 then
    mission.allow("unit.test.worker")
    mission.forbid("research.test.tools")
  end
end"#;
    let mut world = campaign_world(script, &[WORKER]);
    let (hall, forge) = (id(&world, "hall"), id(&world, "forge"));
    assert_eq!(
        try_command(&mut world, produce(hall, WORKER)),
        Some(REFUSED.into())
    );
    run(&mut world, 10);
    assert_eq!(try_command(&mut world, produce(hall, WORKER)), None);
    assert_eq!(
        try_command(
            &mut world,
            json!({"type": "research", "actor": forge, "research": TOOLS})
        ),
        Some(REFUSED.into())
    );
    // The limits are saved with the state.
    let save = world.save();
    let text = serde_json::to_string(&save).unwrap();
    let restored = World::restore(
        content(),
        &mission(script),
        serde_json::from_str(&text).unwrap(),
    )
    .unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(restored.state().tech, world.state().tech);
    // An unknown id stops the script.
    let mut broken = campaign_world(
        "function on_start() mission.allow('unit.test.ghost') end",
        &[],
    );
    broken.step();
    let error = broken.state().mission.as_ref().unwrap().error.clone();
    assert!(error.is_some_and(|e| e.contains("Unknown tech: unit.test.ghost")));
}

#[test]
fn a_computer_player_can_be_held_back_to_a_mission_tech() {
    let mut world = economy_fixture::world(&sandbox(vec![place_for(
        "hall-2", HALL, "player.2", 19.5, 19.5,
    )]));
    fund(&mut world, P2, AMBER, 100);
    world.limit_tech(1, ids(&[WORKER]));
    let hall = id(&world, "hall-2");
    command_for(&mut world, 1, produce(hall, WORKER));
    world.step();
    let refused = world
        .state()
        .facts
        .iter()
        .any(|f| f.owner == P2 && f.message == REFUSED);
    assert!(refused);
    assert!(canopy_sim::campaign::allowed(world.state(), P1, WORKER));
    assert!(!canopy_sim::campaign::allowed(world.state(), P2, WORKER));
}

#[test]
fn campaign_variables_carry_in_and_out_of_a_mission() {
    use mission_fixture::Mission as Fixture;
    let script = r#"
function on_start()
  mission.set("before", mission.campaign_get("rescued"))
  mission.campaign_set("rescued", mission.campaign_get("rescued") + 1)
  mission.set("after", mission.campaign_get("rescued"))
  mission.campaign_set("ally", "moths")
  mission.campaign_set("gone", nil)
end"#;
    let fixture = Fixture::new(script);
    let start = CampaignStart {
        variables: [
            ("rescued".to_string(), Variable::Integer(2)),
            ("gone".to_string(), Variable::Boolean(true)),
        ]
        .into_iter()
        .collect(),
        ..CampaignStart::default()
    };
    let mut world = World::new_campaign(
        mission_fixture::content(),
        &fixture.map(),
        mission_fixture::setup(),
        &start,
    )
    .unwrap_or_else(|e| panic!("{e}"));
    world.step();
    assert_eq!(mission_fixture::error(&world), None);
    assert_eq!(
        mission_fixture::variable(&world, "before"),
        mission_fixture::int(2)
    );
    assert_eq!(
        mission_fixture::variable(&world, "after"),
        mission_fixture::int(3),
        "a run reads its own sets"
    );
    let campaign = world.state().campaign.as_ref().unwrap();
    assert_eq!(
        campaign.variables.get("rescued"),
        Some(&Variable::Integer(3))
    );
    assert_eq!(
        campaign.variables.get("ally"),
        Some(&Variable::String("moths".into()))
    );
    assert!(!campaign.variables.contains_key("gone"));
    // A failed run leaves the campaign as it was.
    let failing =
        Fixture::new("function on_start() mission.campaign_set('rescued', 9) error('stop') end");
    let mut world = World::new_campaign(
        mission_fixture::content(),
        &failing.map(),
        mission_fixture::setup(),
        &start,
    )
    .unwrap();
    world.step();
    assert!(mission_fixture::error(&world).is_some());
    assert_eq!(
        world
            .state()
            .campaign
            .as_ref()
            .unwrap()
            .variables
            .get("rescued"),
        Some(&Variable::Integer(2))
    );
    // Played on its own, a mission keeps no campaign variables.
    let alone = mission_fixture::started("function on_start() mission.campaign_set('x', 1) end");
    assert!(alone.state().campaign.is_none());
}
