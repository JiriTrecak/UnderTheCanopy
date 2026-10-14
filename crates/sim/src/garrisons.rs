//! Single-slot lookouts (old `garrisons.ts` and the `garrison`/`unload` commands of
//! `game.ts`).
//!
//! A unit ordered into a friendly lookout walks to a free cell beside its entrance, then
//! stands inside on a hold order: it sees and shoots from the lookout's height, cannot be
//! targeted, and ghosts through other bodies. Any order other than hold or attack, an
//! `unload`, or the lookout's loss puts it back outside at the entrance.

use crate::{
    action::Action,
    catalog::Catalog,
    entity::{Entity, EntityId, Place, Unit},
    orders::{Order, can_issue, issue},
    spatial::{Spatial, Spot},
    state::State,
    terrain::round_half_up,
};
use serde::{Deserialize, Serialize};

/// Ticks between attempts to route to a lookout's entrance.
const ENTRY_RETRY_TICKS: u32 = 12;
/// How far from its release point a unit leaving a lookout may be placed.
const RELEASE_SEARCH: i32 = 12;

/// A unit standing in a lookout.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Lookout {
    pub building: EntityId,
    /// Eye height above the floor, centimetres.
    pub height_cm: i32,
    /// Where the unit steps out: the lookout's entrance.
    pub exit: Place,
}

/// Whether a garrisoned unit ghosts through bodies (it stands inside a building).
pub fn ghost(e: &Entity) -> bool {
    e.unit.as_ref().is_some_and(|u| u.garrison.is_some())
}

/// A lookout's entrance cell (old `Spatial.entrance`): the authored offset turned with the
/// building in quarter turns.
pub fn entrance(catalog: &Catalog, host: &Entity) -> Place {
    let offset = catalog
        .get(host.definition)
        .entrance
        .as_ref()
        .map_or((0.0, 0.0), |o| (o.x, o.y));
    let quarter = (round_half_up(f64::from(host.rotation) / 90_000.0) as i64).rem_euclid(4);
    let (x, y) = match quarter {
        0 => (offset.0, offset.1),
        1 => (offset.1, -offset.0),
        2 => (-offset.0, -offset.1),
        _ => (-offset.1, offset.0),
    };
    let (hx, hy) = (
        f64::from(host.position.x) / 1000.0,
        f64::from(host.position.y) / 1000.0,
    );
    // Halls sit on half cells and their doorways are authored on half cells, so the
    // entrance is a whole cell; rounding only guards odd content.
    Place {
        x: round_half_up(hx + x) as i32,
        y: round_half_up(hy + y) as i32,
        surface: host.surface.clone(),
    }
}

/// Leaves the lookout: the unit's engagement and plans end and it waits to be placed at
/// the exit (old `GameContext.release`); [`settle`] or movement places it.
pub fn leave(u: &mut Unit) {
    let Some(lookout) = u.garrison.take() else {
        return;
    };
    crate::orders::release_combat(u);
    u.detour = None;
    u.idle = None;
    u.segment = None;
    u.contained = None;
    u.order = None;
    crate::orders::discard_navigation(u);
    u.release = Some(lookout.exit);
}

/// Places a unit waiting for release on the nearest free cell around its release point,
/// if there is one (old `release`); it keeps waiting otherwise.
pub fn settle(spatial: &mut Spatial, e: &mut Entity) -> bool {
    let Some(at) = e.unit.as_ref().and_then(|u| u.release.clone()) else {
        return false;
    };
    let deck = at.surface.as_deref().and_then(|s| spatial.deck_id(s));
    let body = spatial.body_of(e);
    let Some(p) = spatial.nearest(
        Spot::new(at.x, at.y, deck),
        RELEASE_SEARCH,
        Some(e.id),
        Some(&body),
        None,
        None,
    ) else {
        return false;
    };
    e.position = crate::entity::Milli {
        x: p.x * 1000,
        y: p.y * 1000,
    };
    e.surface = p.deck.map(|d| spatial.deck_name(d));
    e.unit.as_mut().expect("unit").release = None;
    spatial.update_body(e);
    true
}

/// Releases every unit standing in `host` (it is being removed).
pub fn evict(state: &mut State, spatial: &mut Spatial, host: EntityId) {
    let occupants: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| {
            e.hp.is_none_or(|hp| hp > 0)
                && e.unit
                    .as_ref()
                    .and_then(|u| u.garrison.as_ref())
                    .is_some_and(|g| g.building == host)
        })
        .map(|e| e.id)
        .collect();
    for id in occupants {
        let e = state.entities.get_mut(id).expect("occupant");
        leave(e.unit.as_mut().expect("unit"));
        settle(spatial, e);
    }
}

fn alive(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0)
}

/// Whether `e` may stand in `host` (old `Garrisons.eligible`).
pub fn eligible(
    state: &State,
    catalog: &Catalog,
    spatial: &Spatial,
    e: &Entity,
    host: &Entity,
) -> bool {
    let Some(u) = &e.unit else {
        return false;
    };
    let policy = catalog.get(host.definition).garrison.as_ref();
    // M2.6: split forms and containing abilities.
    u.contained.is_none()
        && u.release.is_none()
        && u.work.cargo.is_none()
        && alive(host)
        && host.ready_tick <= state.tick
        && host.construction.is_none()
        && !spatial.body_of(e).air
        && host.owner == e.owner
        && policy.is_some_and(|p| p.accepts.contains(catalog.id(e.definition)))
}

/// Eligible and the lookout is neither taken nor promised to another unit.
pub fn available(
    state: &State,
    catalog: &Catalog,
    spatial: &Spatial,
    e: &Entity,
    host: &Entity,
) -> bool {
    eligible(state, catalog, spatial, e, host)
        && !state.entities.iter().any(|other| {
            other.id != e.id
                && other.unit.as_ref().is_some_and(|u| {
                    u.garrison.as_ref().is_some_and(|g| g.building == host.id)
                        || matches!(u.order, Some(Order::Garrison { target }) if target == host.id)
                })
        })
}

/// Step 5, before combat planning (old `Garrisons.tick`): occupants stay put or leave a
/// lookout they may no longer use; units ordered in walk to the entrance and enter.
pub fn tick(state: &mut State, catalog: &Catalog, spatial: &mut Spatial) {
    let _span = tracing::trace_span!("garrisons").entered();
    let tick = state.tick;
    let units: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| alive(e) && e.unit.is_some())
        .map(|e| e.id)
        .collect();
    for id in units {
        let e = state.entities.get(id).expect("unit");
        let u = e.unit.as_ref().expect("unit");
        if let Some(lookout) = &u.garrison {
            let host = state.entities.get(lookout.building);
            let stay = host.is_some_and(|h| eligible(state, catalog, spatial, e, h));
            let e = state.entities.get_mut(id).expect("unit");
            let u = e.unit.as_mut().expect("unit");
            if stay {
                u.route.clear();
                u.goal = None;
            } else {
                leave(u);
                settle(spatial, e);
            }
            continue;
        }
        let Some(Order::Garrison { target }) = u.order else {
            continue;
        };
        // M2.6: taunted and stunned units do not move in.
        if e.ready_tick > tick {
            continue;
        }
        let host = state.entities.get(target);
        let Some(host) = host.filter(|h| available(state, catalog, spatial, e, h)) else {
            let e = state.entities.get_mut(id).expect("unit");
            let u = e.unit.as_mut().expect("unit");
            u.order = None;
            u.route.clear();
            u.goal = None;
            spatial.update_body(e);
            continue;
        };
        let door = entrance(catalog, host);
        let reach = spatial.body_of(e).radius.ceil() as i32 + 2;
        let access = u.goal.map(|g| spatial.spot(g));
        let arrived = access.is_some_and(|a| {
            (a.x - door.x).abs() + (a.y - door.y).abs() <= reach
                && e.position.x == a.x * 1000
                && e.position.y == a.y * 1000
                && spatial.deck_of(e) == a.deck
        });
        if arrived {
            let height = catalog
                .get(host.definition)
                .garrison
                .as_ref()
                .map_or(0.0, |g| g.height);
            let (position, surface, building) = (host.position, host.surface.clone(), host.id);
            let e = state.entities.get_mut(id).expect("unit");
            let u = e.unit.as_mut().expect("unit");
            u.garrison = Some(Lookout {
                building,
                height_cm: round_half_up(height * 100.0) as i32,
                exit: door,
            });
            u.order = Some(Order::Hold);
            crate::orders::discard_navigation(u);
            u.target = None;
            u.idle = None;
            u.combat.attack = None;
            u.combat.pursuit = None;
            e.position = position;
            e.surface = surface;
            spatial.update_body(e);
        } else if u.route.is_empty() && u.retry_at <= tick {
            let deck = door.surface.as_deref().and_then(|s| spatial.deck_id(s));
            let body = spatial.body_of(e);
            let e = state.entities.get_mut(id).expect("unit");
            route_near(spatial, e, Spot::new(door.x, door.y, deck), reach, &body);
            e.unit.as_mut().expect("unit").retry_at = tick + ENTRY_RETRY_TICKS;
        }
    }
}

/// Routes to the first free cell in growing diamonds around `origin` that a route reaches
/// (old `nearest` with a routing acceptance test).
fn route_near(
    spatial: &mut Spatial,
    e: &mut Entity,
    origin: Spot,
    max: i32,
    body: &crate::spatial::Body,
) {
    for r in 0..=max {
        for dy in -r..=r {
            let rest = r - dy.abs();
            let xs: &[i32] = if rest == 0 { &[0] } else { &[-rest, rest] };
            for &dx in xs {
                let p = Spot::new(origin.x + dx, origin.y + dy, origin.deck);
                if spatial.free(p, Some(e.id), Some(body), None) && spatial.route_default(e, p) {
                    return;
                }
            }
        }
    }
}

/// The `garrison` and `unload` commands. `visible` tells whether the owner knows an entity.
pub fn command(
    state: &mut State,
    catalog: &Catalog,
    spatial: &mut Spatial,
    eligible_actors: &[EntityId],
    action: &Action,
    visible: &dyn Fn(&State, &Entity) -> bool,
) -> Result<Vec<EntityId>, String> {
    match action {
        Action::Garrison { target, append, .. } => {
            let append = append.unwrap_or(false);
            let host = state.entities.get(EntityId(*target));
            let Some(host) = host.filter(|h| visible(state, h)) else {
                return Err("No visible friendly watchtower".into());
            };
            let chosen = eligible_actors.iter().copied().find(|&id| {
                let e = state.entities.get(id).expect("actor");
                e.unit.as_ref().is_some_and(|u| u.garrison.is_none())
                    && can_issue(e, append)
                    && available(state, catalog, spatial, e, host)
            });
            let Some(id) = chosen else {
                return Err(
                    "The watchtower needs one available Archer and an empty lookout".into(),
                );
            };
            let host = host.id;
            issue(state, id, Order::Garrison { target: host }, append);
            let e = state.entities.get_mut(id).expect("actor");
            settle(spatial, e);
            Ok(vec![id])
        }
        Action::Unload { .. } => {
            let host = eligible_actors[0];
            let occupant = state
                .entities
                .iter()
                .find(|e| {
                    alive(e)
                        && e.unit
                            .as_ref()
                            .and_then(|u| u.garrison.as_ref())
                            .is_some_and(|g| g.building == host)
                })
                .map(|e| e.id);
            let Some(occupant) = occupant else {
                return Err("The watchtower is empty".into());
            };
            let e = state.entities.get_mut(occupant).expect("occupant");
            leave(e.unit.as_mut().expect("unit"));
            settle(spatial, e);
            Ok(vec![host])
        }
        _ => Err("Invalid request".into()),
    }
}

/// Eye height a lookout adds, centimetres (vision's `lift_cm`).
pub fn lift_cm(e: &Entity) -> i32 {
    e.unit
        .as_ref()
        .and_then(|u| u.garrison.as_ref())
        .map_or(0, |g| g.height_cm)
}
