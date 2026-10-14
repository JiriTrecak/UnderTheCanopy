//! What vision asks of systems that later build steps add: garrisons and containment
//! (M2.4), forms, concealment and detection statuses (M2.6). [`Current`] answers for the
//! state that exists today; the step that adds a system fills in its method, and vision
//! picks the answer up on its next pass without further calls.

use crate::{catalog::Catalog, entity::Entity};
use canopy_content::{conditions::Locomotion, placement::Owner};

/// Flight height in world units when a definition does not say (old `flightHeight ?? 6`).
pub const DEFAULT_FLIGHT_HEIGHT: f64 = 6.0;

pub trait SightRules {
    /// Sight radius in cells; 0 for entities that see nothing. Forms and upgrades may
    /// change it (M2.6).
    fn vision(&self, catalog: &Catalog, entity: &Entity) -> u32;
    /// Height of the body above its floor in centimetres when it flies, `None` on the
    /// ground. Air forms change locomotion (M2.6).
    fn flight_cm(&self, catalog: &Catalog, entity: &Entity) -> Option<i32>;
    /// Extra eye height in centimetres: a garrisoned lookout's floor (M2.4 garrisons).
    fn lift_cm(&self, entity: &Entity) -> i32;
    /// The entity gives no sight and takes part in no sight test: inside a garrison or a
    /// containing ability, or waiting to be released from one (M2.4, M2.6).
    fn blind(&self, entity: &Entity) -> bool;
    /// Concealed by a status (M2.6): observers outside its team also need detection.
    fn hidden(&self, entity: &Entity, tick: u32) -> bool;
    /// Radius in cells within which the entity reveals concealed units (M2.6).
    fn detection(&self, entity: &Entity, tick: u32) -> u32;
    /// Owners that borrow this sensor's ordinary sight, such as the owner of a unit it
    /// contains with `shareVision` (M2.6). Ownership, control and detection stay separate.
    fn lends_sight(&self, entity: &Entity, lend: &mut dyn FnMut(Owner));
}

/// The systems as built so far: sight radius and flight straight from the definition,
/// lookout heights and held units; no forms, concealment or detection yet.
pub struct Current;

impl SightRules for Current {
    fn vision(&self, catalog: &Catalog, entity: &Entity) -> u32 {
        catalog.get(entity.definition).vision.unwrap_or(0)
    }

    fn flight_cm(&self, catalog: &Catalog, entity: &Entity) -> Option<i32> {
        let movement = catalog.get(entity.definition).behaviors.movement.as_ref()?;
        (movement.locomotion == Some(Locomotion::Air)).then(|| {
            let height = movement.flight_height.unwrap_or(DEFAULT_FLIGHT_HEIGHT);
            crate::terrain::round_half_up(height * 100.0) as i32
        })
    }

    fn lift_cm(&self, entity: &Entity) -> i32 {
        crate::garrisons::lift_cm(entity)
    }

    fn blind(&self, entity: &Entity) -> bool {
        // A lookout's archer still sees (from its height); a held unit does not.
        entity
            .unit
            .as_ref()
            .is_some_and(|u| u.contained.is_some() || u.release.is_some())
    }

    fn hidden(&self, _: &Entity, _: u32) -> bool {
        false
    }

    fn detection(&self, _: &Entity, _: u32) -> u32 {
        0
    }

    fn lends_sight(&self, _: &Entity, _: &mut dyn FnMut(Owner)) {}
}
