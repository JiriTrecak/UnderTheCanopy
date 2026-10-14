//! What vision asks of abilities (old observation's concealment, detection, form and
//! containment-sight rules): [`SpellSight`] answers vision's [`SightRules`].

use crate::{catalog::Catalog, entity::Entity, vision::SightRules};
use canopy_content::placement::Owner;
use std::sync::Arc;

/// Sight rules with forms, concealment, detection and containment sight.
pub struct SpellSight {
    pub catalog: Arc<Catalog>,
}

impl SightRules for SpellSight {
    fn vision(&self, catalog: &Catalog, entity: &Entity) -> u32 {
        catalog.get(entity.definition).vision.unwrap_or(0)
    }

    fn flight_cm(&self, catalog: &Catalog, entity: &Entity) -> Option<i32> {
        super::forms::flight_cm(catalog, entity)
    }

    fn lift_cm(&self, entity: &Entity) -> i32 {
        crate::garrisons::lift_cm(entity)
    }

    fn blind(&self, entity: &Entity) -> bool {
        entity
            .unit
            .as_ref()
            .is_some_and(|u| u.contained.is_some() || u.release.is_some())
    }

    fn hidden(&self, entity: &Entity, tick: u32) -> bool {
        !entity.statuses.is_empty() && super::statuses::hidden(&self.catalog, entity, tick)
    }

    fn detection(&self, entity: &Entity, tick: u32) -> u32 {
        if entity.statuses.is_empty() {
            return 0;
        }
        super::statuses::detection(&self.catalog, entity, tick)
    }

    fn lends_sight(&self, entity: &Entity, lend: &mut dyn FnMut(Owner)) {
        if let Some(l) = &entity.links {
            for &owner in &l.lent_sight {
                lend(owner);
            }
        }
    }
}
