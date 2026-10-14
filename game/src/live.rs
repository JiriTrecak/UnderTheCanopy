//! The render thread's side of the live scene: picks the two presentations around the
//! render tick, interpolates them and hands the result to the renderer, loading newly
//! listed models and hiding felled trees as presentations ask.

use crate::sim::{Mailbox, Snapshot, render_tick};
use canopy_render::{Gpu, Renderer, actors::ActorInstance};
use canopy_scene::{interpolate, live::Presentation};
use std::{sync::Arc, time::Instant};

#[derive(Default)]
pub struct LiveScene {
    snapshots: Vec<Arc<Snapshot>>,
    instances: Vec<ActorInstance>,
    hidden_revision: Option<u32>,
    applied: Vec<u64>,
    /// Frames shown at the newest snapshot because the next one was late.
    pub held: u32,
    /// The tick shown by the latest frame.
    pub shown: f64,
}

impl LiveScene {
    /// Copies the newest snapshots when the simulation published any.
    pub fn poll(&mut self, mailbox: &Mailbox) {
        let newest = self.snapshots.last().map(|s| s.tick());
        if mailbox.latest() != newest {
            mailbox.take(&mut self.snapshots);
        }
    }

    pub fn newest(&self) -> Option<&Arc<Snapshot>> {
        self.snapshots.last()
    }

    pub fn snapshots(&self) -> &[Arc<Snapshot>] {
        &self.snapshots
    }

    /// Prepares the frame at `now`.
    pub fn frame(
        &mut self,
        renderer: &mut Renderer,
        gpu: &Gpu,
        pack: &canopy_assets::Pack,
        now: Instant,
    ) {
        let Some(newest) = self.snapshots.last().cloned() else {
            return;
        };
        let tick = render_tick(&newest, now);
        let ticks: Vec<u32> = self.snapshots.iter().map(|s| s.tick()).collect();
        let Some((a, b, t)) = interpolate::bracket(&ticks, tick) else {
            return;
        };
        if a == b && tick > f64::from(newest.tick()) {
            self.held += 1;
        }
        self.shown = f64::from(ticks[a]) + f64::from(t) * f64::from(ticks[b] - ticks[a]);
        let (older, newer) = (self.snapshots[a].clone(), self.snapshots[b].clone());
        self.show(
            renderer,
            gpu,
            pack,
            (a != b).then_some(&older.presentation),
            &newer.presentation,
            t,
        );
    }

    /// Shows `b` (blended from `a` by `t`).
    pub fn show(
        &mut self,
        renderer: &mut Renderer,
        gpu: &Gpu,
        pack: &canopy_assets::Pack,
        a: Option<&Presentation>,
        b: &Presentation,
        t: f32,
    ) {
        renderer.sync_actor_models(gpu, pack, &b.models);
        if self.hidden_revision != Some(b.hidden.revision)
            && let Some(statics) = renderer.statics_mut()
        {
            let bits = &b.hidden.bits;
            self.applied.resize(bits.len(), 0);
            for (word, (&now, before)) in bits.iter().zip(self.applied.iter_mut()).enumerate() {
                let changed = now ^ *before;
                if changed == 0 {
                    continue;
                }
                for bit in 0..64 {
                    if changed & (1 << bit) != 0 {
                        statics.set_hidden(&gpu.queue, word * 64 + bit, now & (1 << bit) != 0);
                    }
                }
                *before = now;
            }
            self.hidden_revision = Some(b.hidden.revision);
        }
        interpolate::interpolate(a, b, t, &mut self.instances);
        renderer.set_actors(&self.instances);
    }
}
