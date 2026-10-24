//! Neutral camps (old camp branches of `combat.ts`, `campLoot.ts`, `loot.ts`,
//! `dropPosition.ts`, and the neutral sight of `abilities/gameHost.ts`).
//!
//! A camp's members guard its home: one that strays past the leash drops everything and
//! walks home. Camps whose aggression is `players` are enemies of every player; passive
//! camps fight only when attacked by force. A camp pays once, when its last member dies:
//! its fixed drops, or rolls from its loot pool on the state's random generator.

use crate::{
    catalog::Catalog,
    entity::{Entity, EntityId, Milli},
    spatial::{Spatial, Spot},
    state::State,
};
use canopy_content::{
    Id,
    map::UtcMap,
    placement::{Aggression, Owner, Placement, Point},
};
use serde::{Deserialize, Serialize};

/// Cells within which a neutral ability caster sees its targets (old `gameHost.ts`
/// `visibleEntity` for `"none"`): camp spell casting (M2.6) uses this sight.
pub const CASTER_SIGHT: u32 = 24;
/// How far from home a returning member looks for a free cell.
const HOME_SEARCH: i32 = 5;
/// Ticks between attempts to route home.
pub const RETURN_RETRY_TICKS: u32 = 20;
/// Rings around a death site searched for a free drop cell.
const DROP_RINGS: i32 = 6;

/// A unit's camp.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Member {
    pub id: String,
    /// Walking home after straying past the leash.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub returning: bool,
    /// The player's unit that struck or was found by the camp ([`provoke`]): the member
    /// sees it wherever it stands and fights it when it has nothing else to fight, until
    /// it dies or the leash sends the member home.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alarm: Option<EntityId>,
}

/// A map camp in simulation form.
#[derive(Debug, Clone)]
pub struct Camp {
    pub id: String,
    /// Home in cells (authored; may be fractional).
    pub home: (f64, f64),
    pub home_surface: Option<String>,
    pub aggro_range: u32,
    pub leash: u32,
    pub hostile: bool,
    pub loot_pool: Option<Id>,
    pub fixed_drops: Option<Vec<Id>>,
    pub members: Vec<String>,
}

/// The map's camps, by id.
#[derive(Debug, Clone, Default)]
pub struct Camps {
    camps: Vec<Camp>,
}

impl Camps {
    pub fn new(map: &UtcMap) -> Self {
        let mut camps: Vec<Camp> = map
            .camps
            .iter()
            .map(|c| Camp {
                id: c.id.clone(),
                home: (c.home.x, c.home.y),
                home_surface: c.home.surface.clone(),
                aggro_range: c.aggro_range,
                leash: c.leash,
                hostile: c.aggression == Aggression::Players,
                loot_pool: c.loot_pool.clone(),
                fixed_drops: c.fixed_drops.clone(),
                members: c.members.clone(),
            })
            .collect();
        camps.sort_by(|a, b| a.id.cmp(&b.id));
        Self { camps }
    }

    pub fn get(&self, id: &str) -> Option<&Camp> {
        self.camps
            .binary_search_by(|c| c.id.as_str().cmp(id))
            .ok()
            .map(|i| &self.camps[i])
    }

    /// The camp of a unit.
    pub fn of(&self, e: &Entity) -> Option<&Camp> {
        self.get(&e.unit.as_ref()?.camp.as_ref()?.id)
    }

    /// The camp a placement belongs to.
    pub fn of_placement(&self, placement: &str) -> Option<&Camp> {
        self.camps
            .iter()
            .find(|c| c.members.iter().any(|m| m == placement))
    }

    /// Whether a neutral unit (or one of `camp`) is an enemy of the players.
    pub fn hostile(&self, camp: Option<&str>) -> bool {
        camp.and_then(|c| self.get(c)).is_some_and(|c| c.hostile)
    }
}

/// Makes a newly created unit a member of its placement's camp (map creation, mission
/// spawns).
pub fn enlist(state: &mut State, camps: &Camps, id: EntityId, camp: Option<&str>) {
    let Some(e) = state.entities.get_mut(id) else {
        return;
    };
    let camp = camp.map(str::to_string).or_else(|| {
        e.placement
            .as_deref()
            .and_then(|p| camps.of_placement(p))
            .map(|c| c.id.clone())
    });
    if let (Some(u), Some(id)) = (e.unit.as_mut(), camp) {
        u.camp = Some(Member {
            id,
            returning: false,
            alarm: None,
        });
    }
}

/// Squared distance in cells from a unit to its camp's home.
pub fn home_distance(camp: &Camp, e: &Entity) -> f64 {
    let dx = f64::from(e.position.x) / 1000.0 - camp.home.0;
    let dy = f64::from(e.position.y) / 1000.0 - camp.home.1;
    dx * dx + dy * dy
}

/// The free cell a returning member walks to.
pub fn home_spot(spatial: &Spatial, camp: &Camp, id: EntityId) -> Option<Spot> {
    let round = |v: f64| crate::terrain::round_half_up(v) as i32;
    let deck = camp
        .home_surface
        .as_deref()
        .and_then(|s| spatial.deck_id(s));
    spatial.nearest(
        Spot::new(round(camp.home.0), round(camp.home.1), deck),
        HOME_SEARCH,
        Some(id),
        None,
        None,
        None,
    )
}

/// A uniform integer below `count` from the state's generator, by rejection so every
/// value is equally likely (old `randomBelow`).
pub fn random_below(state: &mut State, count: u32) -> u32 {
    assert!(count > 0, "empty random range");
    let limit = (0x1_0000_0000u64 / u64::from(count)) * u64::from(count);
    loop {
        let value = u64::from(state.random());
        if value < limit {
            return (value % u64::from(count)) as u32;
        }
    }
}

/// Rolls a loot pool (old `rollLoot`): each roll picks an entry by weight; an empty entry
/// drops nothing.
pub fn roll(state: &mut State, pool: &canopy_content::game::LootPool) -> Vec<Id> {
    let total: u32 = pool.entries.iter().map(|e| e.weight).sum();
    let mut items = Vec::new();
    if total == 0 {
        return items;
    }
    for _ in 0..pool.rolls {
        let mut draw = random_below(state, total);
        for entry in &pool.entries {
            if draw < entry.weight {
                if let Some(item) = &entry.item {
                    items.push(item.clone());
                }
                break;
            }
            draw -= entry.weight;
        }
    }
    items
}

/// Pays out a camp when `dead` was its last living member (old `CampLoot.onDeath`):
/// marks it cleared and drops its loot around the body. Returns the dropped items.
pub fn on_death(
    state: &mut State,
    catalog: &Catalog,
    spatial: &Spatial,
    camps: &Camps,
    dead: EntityId,
) -> Vec<EntityId> {
    let Some(e) = state.entities.get(dead) else {
        return Vec::new();
    };
    let Some(camp) = camps.of(e) else {
        return Vec::new();
    };
    if state.cleared_camps.contains(&camp.id) {
        return Vec::new();
    }
    let alive = |e: &Entity| e.hp.is_none_or(|hp| hp > 0);
    if state.entities.iter().any(|e| {
        alive(e)
            && e.unit
                .as_ref()
                .and_then(|u| u.camp.as_ref())
                .is_some_and(|m| m.id == camp.id)
    }) {
        return Vec::new();
    }
    state.cleared_camps.push(camp.id.clone());
    let drops = match (&camp.fixed_drops, &camp.loot_pool) {
        (Some(drops), _) => drops.clone(),
        (None, Some(pool)) => match catalog.content.rules.loot_pools.get(pool) {
            Some(pool) => roll(state, pool),
            None => Vec::new(),
        },
        (None, None) => Vec::new(),
    };
    let origin = {
        let e = state.entities.get(dead).expect("dead");
        let (x, y) = e.position.cell();
        (x, y, e.surface.clone())
    };
    let mut created = Vec::new();
    for item in drops {
        let (x, y) = drop_position(state, spatial, origin.clone(), dead);
        let placement = Placement {
            activation: None,
            id: String::new(),
            map_knowledge: None,
            definition: item,
            position: Point {
                x: f64::from(x),
                y: f64::from(y),
                surface: origin.2.clone(),
            },
            rotation: 0.0,
            owner: Owner::Nobody,
            initial_state: Some(canopy_content::placement::InitialState {
                quantity: Some(1),
                ..Default::default()
            }),
            appearance: None,
        };
        if let Ok(id) = crate::spawn::create(state, catalog, &placement) {
            let tick = state.tick;
            let e = state.entities.get_mut(id).expect("created");
            e.placement = None;
            e.ready_tick = tick + 1;
            created.push(id);
        }
    }
    created
}

/// The nearest free cell around a death site, no item closer than two cells, reachable in
/// a straight line (old `dropPosition`); the site itself when everything is crowded.
pub(crate) fn drop_position(
    state: &State,
    spatial: &Spatial,
    origin: (i32, i32, Option<String>),
    except: EntityId,
) -> (i32, i32) {
    let deck = origin.2.as_deref().and_then(|s| spatial.deck_id(s));
    let from = Spot::new(origin.0, origin.1, deck).fixed();
    let items: Vec<(i32, i32)> = state
        .entities
        .iter()
        .filter(|e| e.item.is_some() && e.surface == origin.2)
        .map(|e| (e.position.x, e.position.y))
        .collect();
    let body = spatial.profiles.navigation_body;
    for r in 0..=DROP_RINGS {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs() + dy.abs() != r {
                    continue;
                }
                let p = Spot::new(origin.0 + dx, origin.1 + dy, deck);
                let crowded = items.iter().any(|&(x, y)| {
                    let (ix, iy) = (
                        i64::from(x) - i64::from(p.x) * 1000,
                        i64::from(y) - i64::from(p.y) * 1000,
                    );
                    ix * ix + iy * iy < 4_000_000
                });
                if spatial.free(p, Some(except), None, None)
                    && spatial.clear_segment(from, p.fixed(), None, &body)
                    && !crowded
                {
                    return (p.x, p.y);
                }
            }
        }
    }
    (origin.0, origin.1)
}

/// A neutral that would not fight `source` unprompted (a lone creature outside any camp,
/// a passive camp's member) answers a player's blow: when idle, it attacks the striker as
/// a forced attack, which ends once the striker is gone or out of its sight.
pub fn answer(
    senses: &crate::combat::Senses,
    state: &mut State,
    source: EntityId,
    target: EntityId,
) {
    let (Some(s), Some(t)) = (state.entities.get(source), state.entities.get(target)) else {
        return;
    };
    let idle = t.unit.as_ref().is_some_and(|u| {
        u.order.is_none()
            && u.target.is_none()
            && u.order_queue.is_empty()
            && !u.camp.as_ref().is_some_and(|c| c.returning)
    });
    if t.owner != Owner::Nobody
        || s.owner == Owner::Nobody
        || !idle
        || t.hp.is_none_or(|hp| hp == 0)
        || s.hp.is_none_or(|hp| hp == 0)
        || senses.weapon(t).is_none()
        || !senses.reaches(t, s)
        || senses.hostile(t, s)
    {
        return;
    }
    crate::orders::issue(
        state,
        target,
        crate::orders::Order::Attack {
            target: source,
            force: true,
        },
        false,
    );
}

/// A camp member struck by a player's unit, or one that found one in its aggro range,
/// calls its whole camp (Warcraft III's creeps answer together): every member not walking
/// home is alarmed by `source` ([`Member::alarm`]), a passive camp's too. A member already alarmed by `source`
/// stops the call, so a fight rings its camp once.
pub fn provoke(
    senses: &crate::combat::Senses,
    state: &mut State,
    member: EntityId,
    source: EntityId,
) {
    let (Some(m), Some(s)) = (state.entities.get(member), state.entities.get(source)) else {
        return;
    };
    let Some(camp) = m.unit.as_ref().and_then(|u| u.camp.as_ref()) else {
        return;
    };
    if camp.alarm == Some(source)
        || !matches!(s.owner, Owner::Player(_))
        || s.hp.is_none_or(|hp| hp == 0)
        || s.unit.is_none()
    {
        return;
    }
    let Some(camp) = senses.camps.get(&camp.id) else {
        return;
    };
    let id = camp.id.as_str();
    for e in state.entities.iter_mut() {
        if e.hp.is_none_or(|hp| hp == 0) {
            continue;
        }
        if let Some(c) = e
            .unit
            .as_mut()
            .and_then(|u| u.camp.as_mut())
            .filter(|c| c.id == id && !c.returning)
        {
            c.alarm = Some(source);
        }
    }
}

/// Whether `e` is a camp member alarmed by `source`: it sees and fights `source` even when
/// its camp is passive.
pub fn alarmed_by(e: &Entity, source: EntityId) -> bool {
    e.unit
        .as_ref()
        .and_then(|u| u.camp.as_ref())
        .is_some_and(|c| c.alarm == Some(source))
}

/// Whether a neutral unit at `eye` sees `target` within `radius` cells over the terrain.
pub fn neutral_sees(
    sight: &crate::vision::Sight,
    eye: &Entity,
    target: &Entity,
    radius: u32,
) -> bool {
    let (dx, dy) = (
        i64::from(eye.position.x - target.position.x),
        i64::from(eye.position.y - target.position.y),
    );
    let r = i64::from(radius) * 1000;
    if dx * dx + dy * dy > r * r {
        return false;
    }
    let from = sight.sight_point(eye);
    let to = sight.sight_point(target);
    if sight.rules.flight_cm(sight.catalog, target).is_some() {
        sight.terrain.shot_clear(from, to)
    } else {
        sight.terrain.visible(from, to)
    }
}

/// A camp's home as a fixed position, for views and tests.
pub fn home_milli(camp: &Camp) -> Milli {
    let m = |v: f64| crate::terrain::round_half_up(v * 1000.0) as i32;
    Milli {
        x: m(camp.home.0),
        y: m(camp.home.1),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_content::game::{LootEntry, LootPool};

    #[test]
    fn rolls_follow_the_generator_and_reject_biased_draws() {
        let mut state = State::new(7);
        let pool = LootPool {
            rolls: 3,
            max_tier: None,
            entries: vec![
                LootEntry {
                    item: Some(Id::new("item.a").unwrap()),
                    weight: 3,
                },
                LootEntry {
                    item: None,
                    weight: 1,
                },
            ],
        };
        let mut again = State::new(7);
        assert_eq!(roll(&mut state, &pool), roll(&mut again, &pool));
        assert_eq!(state.random, again.random);
        // Every value below the bound is reachable and nothing at or above it.
        let mut seen = [false; 6];
        for _ in 0..200 {
            seen[random_below(&mut state, 6) as usize] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }
}
