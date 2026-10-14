//! Forms (old `forms.ts`, `flight.ts`, `context.ts` `formDefinition`/`reconcileWeapon`):
//! the newest form status chooses the unit's weapon (a combat profile definition) and its
//! locomotion; identity, body and ownership stay authored.
//!
//! Nothing is cached: the weapon definition is looked up from the statuses when asked (a
//! unit without statuses answers at once), and flight is explicit saved state because a
//! landing can be blocked, keeping the unit airborne until space opens.

use super::{Ctx, statuses};
use crate::{
    catalog::{Catalog, Def},
    entity::{EntityId, Milli},
    orders::{Order, discard_navigation},
    spatial::Spot,
    state::State,
};
use canopy_content::conditions::Locomotion;
use serde::{Deserialize, Serialize};

/// Locomotion that differs from the definition's: taken off by a form, or landed by one.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flight {
    pub air: bool,
    /// Flight height in centimetres while airborne.
    #[serde(default)]
    pub height_cm: i32,
}

/// Default flight height in world units (old `flightHeight ?? 6`).
pub const DEFAULT_FLIGHT_HEIGHT: f64 = 6.0;

fn native(catalog: &Catalog, def: Def) -> (bool, i32) {
    let m = catalog.get(def).behaviors.movement.as_ref();
    let air = m.and_then(|m| m.locomotion) == Some(Locomotion::Air);
    let height = m
        .and_then(|m| m.flight_height)
        .unwrap_or(DEFAULT_FLIGHT_HEIGHT);
    (air, crate::terrain::round_half_up(height * 100.0) as i32)
}

/// Whether the entity flies now.
pub fn airborne(catalog: &Catalog, e: &crate::entity::Entity) -> bool {
    match e.unit.as_ref().and_then(|u| u.flight) {
        Some(f) => f.air,
        None => native(catalog, e.definition).0,
    }
}

/// Whether a flight override says the entity flies, given its definition's own answer
/// (for systems that read definitions without the catalog).
pub fn air(native_air: bool, e: &crate::entity::Entity) -> bool {
    e.unit
        .as_ref()
        .and_then(|u| u.flight)
        .map_or(native_air, |f| f.air)
}

/// Flight height in centimetres when airborne.
pub fn flight_cm(catalog: &Catalog, e: &crate::entity::Entity) -> Option<i32> {
    match e.unit.as_ref().and_then(|u| u.flight) {
        Some(f) => f.air.then_some(f.height_cm),
        None => {
            let (air, height) = native(catalog, e.definition);
            air.then_some(height)
        }
    }
}

/// The definition whose weapon the unit uses: its newest form's combat profile, or its own
/// (old `weaponDefinition`).
pub fn weapon_def(catalog: &Catalog, e: &crate::entity::Entity) -> Def {
    if e.statuses.is_empty() {
        return e.definition;
    }
    statuses::active_form(catalog, e)
        .and_then(|(_, form)| form.combat_profile.as_ref())
        .and_then(|id| catalog.find(id.as_str()))
        .unwrap_or(e.definition)
}

/// The locomotion the unit wants: its newest form's, or its definition's (old
/// `desiredMovement`); `None` without movement.
fn desired(catalog: &Catalog, e: &crate::entity::Entity) -> Option<(bool, i32)> {
    let d = catalog.get(e.definition);
    let movement = d.behaviors.movement.as_ref()?;
    let (native_air, native_height) = native(catalog, e.definition);
    if let Some(m) = statuses::active_form(catalog, e).and_then(|(_, f)| f.movement.as_ref()) {
        let air = m.locomotion == Locomotion::Air;
        let height = m
            .flight_height
            .or(movement.flight_height)
            .unwrap_or(DEFAULT_FLIGHT_HEIGHT);
        return Some((air, crate::terrain::round_half_up(height * 100.0) as i32));
    }
    Some((native_air, native_height))
}

/// Takes off or lands as the newest form asks (old `reconcileFlight`). A landing waits for
/// clear ground within three cells.
pub fn reconcile_flight(ctx: &mut Ctx, state: &mut State, id: EntityId) {
    let catalog = ctx.catalog();
    let tick = state.tick;
    let Some(e) = state.entities.get(id) else {
        return;
    };
    let Some(u) = e.unit.as_ref() else {
        return;
    };
    if !super::actor::alive(e)
        || u.contained.is_some()
        || u.release.is_some()
        || u.garrison.is_some()
    {
        return;
    }
    let Some((want_air, height)) = desired(catalog, e) else {
        return;
    };
    let (native_air, native_height) = native(catalog, e.definition);
    let flying = airborne(catalog, e);
    if want_air {
        let flight = if native_air && height == native_height {
            None
        } else {
            Some(Flight {
                air: true,
                height_cm: height,
            })
        };
        let entered = !flying;
        let e = state.entities.get_mut(id).expect("unit");
        e.unit.as_mut().expect("unit").flight = flight;
        if entered {
            e.surface = None;
            clear_route(ctx, state, id, tick);
        }
        return;
    }
    if !flying {
        return;
    }
    // Land on the exact spot when it is free, else on the nearest free cell.
    let mut body = ctx.spatial.body_of(e);
    body.air = false;
    let origin = Spot::new(
        super::cell_round(e.position.x),
        super::cell_round(e.position.y),
        None,
    );
    let landing = if ctx.spatial.free(origin, Some(id), Some(&body), None) {
        Some((e.position, None))
    } else {
        ctx.spatial
            .nearest(origin, 3, Some(id), Some(&body), None, None)
            .map(|s| {
                (
                    Milli {
                        x: s.x * 1000,
                        y: s.y * 1000,
                    },
                    s.deck,
                )
            })
    };
    let Some((at, deck)) = landing else {
        return;
    };
    let surface = deck.map(|d| ctx.spatial.deck_name(d));
    let e = state.entities.get_mut(id).expect("unit");
    e.position = at;
    e.surface = surface;
    e.unit.as_mut().expect("unit").flight = if native_air {
        Some(Flight {
            air: false,
            height_cm: 0,
        })
    } else {
        None
    };
    clear_route(ctx, state, id, tick);
}

/// Drops the route of a unit whose locomotion changed and plans it again (old
/// `clearRoute`).
fn clear_route(ctx: &mut Ctx, state: &mut State, id: EntityId, tick: u32) {
    let e = state.entities.get_mut(id).expect("unit");
    let u = e.unit.as_mut().expect("unit");
    let destination = match &u.order {
        Some(Order::Move { destination, .. }) => Some(Spot::new(
            destination.x,
            destination.y,
            destination
                .surface
                .as_deref()
                .and_then(|s| ctx.spatial.deck_id(s)),
        )),
        _ => u.goal.map(|g| ctx.spatial.spot(g)),
    };
    discard_navigation(u);
    u.retry_at = tick;
    u.combat.attack = None;
    u.combat.charge = None;
    ctx.spatial.update_body(e);
    if let Some(mut goal) = destination {
        if air_of(ctx.catalog(), e) {
            goal.deck = None;
        }
        let e = state.entities.get_mut(id).expect("unit");
        ctx.spatial.route(e, goal, false);
    }
}

fn air_of(catalog: &Catalog, e: &crate::entity::Entity) -> bool {
    airborne(catalog, e)
}

/// Drops an unreleased swing or a charge begun with another weapon profile (old
/// `reconcileWeapon`).
pub fn reconcile_weapon(catalog: &Catalog, state: &mut State, id: EntityId) {
    let Some(e) = state.entities.get(id) else {
        return;
    };
    if e.unit.is_none() {
        return;
    }
    let profile = weapon_def(catalog, e);
    let own = e.definition;
    let u = state
        .entities
        .get_mut(id)
        .expect("unit")
        .unit
        .as_mut()
        .expect("unit");
    if u.combat
        .attack
        .as_ref()
        .is_some_and(|a| !a.released && a.profile.unwrap_or(own) != profile)
    {
        u.combat.attack = None;
    }
    if u.combat
        .charge
        .as_ref()
        .is_some_and(|c| c.profile.unwrap_or(own) != profile)
    {
        u.combat.charge = None;
    }
}

/// The model scale and opacity a renderer applies (old `spellAppearance`,
/// `spellFormOpacity`): (asset, scale, opacity).
pub fn appearance(
    catalog: &Catalog,
    e: &crate::entity::Entity,
) -> Option<(Option<String>, f64, f64)> {
    let (_, form) = statuses::active_form(catalog, e)?;
    Some((
        form.asset.as_ref().map(|a| a.as_str().to_string()),
        form.scale,
        form.opacity,
    ))
}
