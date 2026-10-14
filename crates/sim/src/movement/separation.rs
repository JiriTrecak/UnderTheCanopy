//! Nudging idle bodies apart (old `separateOverlaps`, `src/sim/game/separation.ts`).
//! Overlaps arise from spawns, releases, revivals and ghosting workers turning solid;
//! movers already escape through the separating-motion rule, so only parked units are
//! handled, each every 20 ticks staggered by id.

use super::Motion;
use crate::{spatial::Spot, state::State};

pub fn separate(motion: &mut Motion, state: &mut State) {
    let tick = state.tick;
    let units = motion.active(state);
    let mut synced = false;
    for id in units {
        let Some(e) = state.entities.get(id) else {
            continue;
        };
        let u = e.unit.as_ref().expect("unit");
        let parked = (tick + id.0).is_multiple_of(20)
            && u.route.is_empty()
            && u.order.is_none()
            && u.target.is_none()
            && motion.spatial.profiles.get(e.definition).motion.is_some();
        if !parked {
            continue;
        }
        if !synced {
            motion.spatial.sync_bodies(state);
            synced = true;
        }
        if motion.spatial.bodies.get(id).is_none_or(|b| b.ignores) {
            continue;
        }
        let at = super::Mover::of(&motion.spatial, e);
        if motion
            .spatial
            .unit_segment_clear(at.at, at.at, Some(id), None, None, None)
        {
            continue;
        }
        let body = motion.spatial.body_of(e);
        let spot = motion.spatial.nearest(
            Spot::new(at.cell.x, at.cell.y, at.cell.deck),
            3,
            Some(id),
            Some(&body),
            None,
            None,
        );
        if let Some(spot) = spot {
            let e = state.entities.get_mut(id).expect("unit");
            motion.spatial.route(e, spot, false);
        }
    }
}
