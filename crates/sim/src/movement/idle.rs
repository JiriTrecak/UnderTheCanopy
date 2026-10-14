//! Idle strolls (old `idleMotion`, `src/sim/game/idleMotion.ts`): idle workers that may
//! wander take a short walk to a neighbouring cell of their home every 6–12 seconds. The
//! cadence comes from a hash of id and tick, not the random generator.

use super::Motion;
use crate::{
    entity::{Idle, Place},
    navigation::cells::CellSet,
    spatial::{Fixed, Spot},
    state::State,
};

pub fn idle(motion: &mut Motion, state: &mut State) {
    let _span = tracing::trace_span!("idle motion").entered();
    let tick = state.tick;
    let units = motion.active(state);
    let mut synced = false;
    let mut claims: [Option<CellSet>; 2] = [None, None];
    for &id in &units {
        let e = state.entities.get_mut(id).expect("active unit");
        let profile = motion.spatial.profiles.get(e.definition);
        let wanders = profile.work && profile.motion.is_some_and(|m| m.idle_wander);
        let u = e.unit.as_mut().expect("unit");
        let busy = u.order.is_some()
            || !u.order_queue.is_empty()
            || u.target.is_some()
            || u.combat.cooldown > 0
            || u.camp.is_some();
        if !wanders || busy {
            if u.idle.as_ref().is_some_and(|i| i.walking) {
                u.route.clear();
                u.segment = None;
                u.goal = None;
            }
            u.idle = None;
            continue;
        }
        if !u.route.is_empty() {
            continue;
        }
        let seed = id.0.wrapping_mul(73_856_093) ^ tick.wrapping_mul(19_349_663);
        let wait = 240 + seed % 241;
        let (cx, cy) = e.position.cell();
        let Some(idle) = u.idle.as_mut() else {
            u.idle = Some(Idle {
                home: Place::new(cx, cy),
                next_tick: tick + wait,
                walking: false,
            });
            continue;
        };
        if idle.walking {
            idle.walking = false;
            idle.next_tick = tick + wait;
        }
        if tick < idle.next_tick {
            continue;
        }
        idle.next_tick = tick + wait;
        let target = Spot::new(
            idle.home.x + (seed % 3) as i32 - 1,
            idle.home.y + (seed / 3 % 3) as i32 - 1,
            None,
        );
        let size = motion.spatial.size() as i32;
        if target.x < 0
            || target.y < 0
            || target.x >= size
            || target.y >= size
            || (target.x == cx && target.y == cy)
        {
            continue;
        }
        if !synced {
            motion.spatial.sync_bodies(state);
            synced = true;
        }
        let e = state.entities.get(id).expect("active unit");
        let air = motion.spatial.body_of(e).air;
        // Cells of every active unit of the same locomotion (no route changes move bodies).
        let set = claims[usize::from(air)].get_or_insert_with(|| {
            let mut set = motion.spatial.cell_set();
            for &other in &units {
                if let Some(b) = motion.spatial.bodies.get(other)
                    && b.air == air
                {
                    set.insert(b.node);
                }
            }
            set
        });
        let Some(target_node) = motion.spatial.node(target) else {
            continue;
        };
        if set.contains(target_node) {
            continue;
        }
        let own = motion.spatial.entity_node(e).unwrap_or(u32::MAX);
        let had = set.remove(own);
        let from = Fixed::on(e.position.x, e.position.y, motion.spatial.deck_of(e));
        let body = motion.spatial.body_of(e);
        let clear = motion
            .spatial
            .clear_segment(from, target.fixed(), Some(&*set), &body);
        if had {
            set.insert(own);
        }
        if !clear {
            continue;
        }
        let e = state.entities.get_mut(id).expect("active unit");
        if motion.spatial.route(e, target, true)
            && let Some(idle) = e.unit.as_mut().and_then(|u| u.idle.as_mut())
        {
            idle.walking = true;
        }
    }
}
