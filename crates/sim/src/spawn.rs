//! Turning a map into initial placements and placements into entities.

use crate::{
    catalog::{Catalog, Def},
    entity::{Construction, Entity, EntityId, LooseItem, Milli, Resource, Unit},
    setup::MatchSetup,
    state::State,
};
use canopy_content::{
    game::Kind,
    map::UtcMap,
    placement::{InitialConstruction, InitialState, Owner, Placement, Point},
};

/// Every placement the match starts with, in placement-id order: the map's own entities
/// plus each player's starting setup (fort and starting units).
pub fn placements(
    map: &UtcMap,
    catalog: &Catalog,
    setup: &MatchSetup,
) -> Result<Vec<Placement>, String> {
    let rules = &catalog.content.rules;
    let mut result = map.entities.clone();
    if map.mission.is_none() && !map.sandbox {
        for start in &map.player_starts {
            let slot = setup
                .slots
                .iter()
                .find(|s| u32::from(s.player) + 1 == u32::from(start.player));
            let setup_rules = if rules.races.is_empty() {
                &rules.starting_setup
            } else {
                let race = slot
                    .and_then(|s| s.race.as_deref())
                    .unwrap_or(&rules.default_race);
                rules
                    .race(race)
                    .ok_or_else(|| format!("unknown race {race}"))?
                    .starting_setup
            };
            let owner = Owner::Player(start.player - 1);
            let prefix = format!("start.player.{}", start.player);
            let rotation = f64::from(start.rotation.unwrap_or(0));
            result.push(Placement {
                activation: None,
                id: format!("{prefix}/main-fort"),
                map_knowledge: None,
                definition: setup_rules.fort.clone(),
                position: Point {
                    x: start.x,
                    y: start.z,
                    surface: None,
                },
                rotation,
                owner,
                initial_state: Some(InitialState {
                    construction: Some(InitialConstruction::Complete),
                    inventory: Some(setup_rules.inventory.clone()),
                    ..InitialState::default()
                }),
                appearance: None,
            });
            let hero = match (&setup_rules.hero, slot.and_then(|s| s.hero.as_ref())) {
                (Some(policy), selected) => {
                    let id = selected.map_or(policy.default.as_str(), String::as_str);
                    if !policy.choices.iter().any(|c| c == id) {
                        return Err(format!("unavailable starting hero {id}"));
                    }
                    Some((id.to_string(), policy.offset.clone()))
                }
                (None, Some(_)) => return Err("this setup has no starting hero choice".into()),
                (None, None) => None,
            };
            let units = setup_rules
                .units
                .iter()
                .map(|u| (u.definition.to_string(), u.offset.clone()))
                .chain(hero);
            for (index, (definition, offset)) in units.enumerate() {
                let (x, y) =
                    starting_unit_position(start.x, start.z, rotation, offset.x, offset.y)?;
                result.push(Placement {
                    activation: None,
                    id: format!("{prefix}/unit-{:03}", index + 1),
                    map_knowledge: None,
                    definition: definition.parse_id()?,
                    position: Point {
                        x,
                        y,
                        surface: None,
                    },
                    rotation,
                    owner,
                    initial_state: None,
                    appearance: None,
                });
            }
        }
    }
    result.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(result)
}

trait ParseId {
    fn parse_id(&self) -> Result<canopy_content::Id, String>;
}

impl ParseId for String {
    fn parse_id(&self) -> Result<canopy_content::Id, String> {
        canopy_content::Id::new(self.clone())
    }
}

/// A starting unit's cell: the offset rotated in quarter turns around the start, with the
/// navigation-centre bias rotating with the formation (old `startingUnitPosition`).
fn starting_unit_position(
    x: f64,
    z: f64,
    rotation: f64,
    ox: i32,
    oy: i32,
) -> Result<(f64, f64), String> {
    let quarter = rotation / 90.0;
    if quarter.fract() != 0.0 {
        return Err("starting formations rotate in 90-degree steps".into());
    }
    let round = |v: f64| (v + 0.5).floor();
    let dx = f64::from(ox) + round(x) - x;
    let dy = f64::from(oy) + round(z) - z;
    let (rx, ry) = match (quarter as i64).rem_euclid(4) {
        0 => (dx, dy),
        1 => (dy, -dx),
        2 => (-dx, -dy),
        _ => (-dy, dx),
    };
    Ok((x + rx, z + ry))
}

/// Creates the entity for a placement and adds it to the state.
pub fn create(
    state: &mut State,
    catalog: &Catalog,
    placement: &Placement,
) -> Result<EntityId, String> {
    let def: Def = catalog.find(placement.definition.as_str()).ok_or_else(|| {
        format!(
            "{}: unknown definition {}",
            placement.id, placement.definition
        )
    })?;
    let definition = catalog.get(def);
    let initial = placement.initial_state.clone().unwrap_or_default();
    let id = EntityId(state.counters.entity);
    state.counters.entity += 1;
    let milli = |v: f64| (v * f64::from(Milli::CELL)).round() as i32;
    let entity = Entity {
        id,
        ready_tick: state.tick,
        placement: Some(placement.id.clone()),
        definition: def,
        owner: placement.owner,
        position: Milli {
            x: milli(placement.position.x),
            y: milli(placement.position.y),
        },
        surface: placement.position.surface.clone(),
        rotation: (placement.rotation * 1000.0).round() as i32,
        hp: initial
            .health
            .or(definition.body.as_ref().map(|b| b.max_hp)),
        inventory: initial.inventory.clone().unwrap_or_default(),
        unit: (definition.kind == Kind::Unit).then(Box::<Unit>::default),
        construction: (definition.kind == Kind::Building && initial.construction.is_none())
            .then_some(Construction {
                progress: 0,
                supported_hp: 1,
            }),
        resource: definition.resource_yield.map(|full| Resource {
            amount: initial.amount.unwrap_or(full),
            initial_amount: Some(full),
            growing_until: None,
        }),
        item: (definition.kind == Kind::Item).then(|| LooseItem {
            quantity: initial.quantity.unwrap_or(1),
        }),
    };
    state.entities.insert(entity);
    Ok(id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formations_rotate_in_quarter_turns() {
        assert_eq!(
            starting_unit_position(10.0, 20.0, 0.0, 2, 3).unwrap(),
            (12.0, 23.0)
        );
        assert_eq!(
            starting_unit_position(10.0, 20.0, 90.0, 2, 3).unwrap(),
            (13.0, 18.0)
        );
        assert_eq!(
            starting_unit_position(10.0, 20.0, 180.0, 2, 3).unwrap(),
            (8.0, 17.0)
        );
        assert_eq!(
            starting_unit_position(10.0, 20.0, 270.0, 2, 3).unwrap(),
            (7.0, 22.0)
        );
        // A half-cell hall centre: the bias rotates with the formation.
        assert_eq!(
            starting_unit_position(10.5, 20.5, 0.0, 1, 0).unwrap(),
            (12.0, 21.0)
        );
        assert_eq!(
            starting_unit_position(10.5, 20.5, 180.0, 1, 0).unwrap(),
            (9.0, 20.0)
        );
        assert!(starting_unit_position(10.0, 20.0, 45.0, 1, 0).is_err());
    }
}
