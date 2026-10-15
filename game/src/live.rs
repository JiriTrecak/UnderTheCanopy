//! The render thread's side of the live scene: picks the two presentations around the
//! render tick, interpolates them and hands the result to the renderer, loading newly
//! listed models and hiding felled trees as presentations ask.

use crate::sim::{Mailbox, Snapshot, render_tick};
use canopy_render::{
    Gpu, Renderer, actors::ActorInstance, effects::EffectFrame, water::ripples::Mover,
};
use canopy_scene::{
    effects::frame::EffectEvaluator,
    interpolate,
    live::{Body, Presentation},
    world_ui::{WorldUi, WorldUiInput},
};
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
    /// Ground units of the latest frame, for the water's wakes.
    movers: Vec<Mover>,
    effects: Option<EffectEvaluator>,
    effect_frame: EffectFrame,
    /// The camera's eye, to order translucent effects back to front.
    pub eye: Option<glam::Vec3>,
    fog: canopy_scene::fog::FogFeed,
    world_ui: Option<WorldUi>,
    world_ui_frame: canopy_render::world_ui::WorldUiFrame,
    /// What the player selected and how the world UI shows (the game fills it).
    pub world_ui_input: WorldUiInput,
    /// The bodies of the latest frame and where their models stand.
    bodies: Vec<Body>,
    keys: Vec<u64>,
}

impl LiveScene {
    /// Shows health bars, circles, rallies and labels (`data` holds the HUD font).
    pub fn attach_world_ui(
        &mut self,
        data: &std::path::Path,
        pack: &canopy_assets::Pack,
        content: &canopy_content::Content,
        colors: Vec<[f32; 3]>,
    ) {
        self.world_ui = Some(WorldUi::new(data, Some(pack), content, colors));
    }

    /// Builds the world UI of the latest frame for `camera` on an output of `size`.
    pub fn draw_world_ui(
        &mut self,
        renderer: &mut Renderer,
        gpu: &Gpu,
        camera: &canopy_render::CameraView,
        scene: &canopy_scene::MapScene,
        seconds: f32,
    ) {
        let Some(ui) = &mut self.world_ui else {
            return;
        };
        let scale = self.world_ui_input.scale;
        if ui.atlases_dirty {
            let (glyphs, icons) = ui.atlases(scale);
            renderer.set_world_ui_atlases(gpu, glyphs, icons);
        }
        let keys = &self.keys;
        let instances = &self.instances;
        let position = |key: u64| {
            keys.binary_search(&key)
                .ok()
                .map(|i| instances[i].transform.w_axis.truncate())
        };
        ui.build(
            &self.bodies,
            &position,
            &self.world_ui_input,
            camera,
            renderer.size(),
            &scene.water_field(),
            seconds,
            &mut self.world_ui_frame,
        );
        if ui.atlases_dirty {
            let (glyphs, icons) = ui.atlases(scale);
            renderer.set_world_ui_atlases(gpu, glyphs, icons);
        }
        renderer.set_world_ui(&mut self.world_ui_frame);
    }

    /// Shows the presenter's effects: loads the textures their draws refer to.
    pub fn attach_effects(
        &mut self,
        renderer: &mut Renderer,
        gpu: &Gpu,
        pack: &canopy_assets::Pack,
        table: Arc<canopy_assets::EffectTable>,
    ) {
        let evaluator = EffectEvaluator::new(table);
        renderer.load_effect_textures(gpu, pack, evaluator.textures());
        self.effects = Some(evaluator);
    }

    /// Feeds the frame's ground units to the water's ripples (every frame).
    pub fn feed_ripples(
        &self,
        renderer: &mut Renderer,
        scene: &canopy_scene::MapScene,
        seconds: f32,
        focus: [f32; 2],
    ) {
        renderer
            .ripples_mut()
            .update(&self.movers, &scene.water_field(), seconds, focus);
    }

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
        self.fog.feed(renderer, gpu, b.fog.as_ref());
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
        let actors = self.instances.len();
        self.keys.clear();
        self.keys.extend(b.actors.iter().map(|a| a.key));
        self.bodies.clone_from(&b.bodies);
        if let Some(evaluator) = &mut self.effects {
            let tick = match a {
                Some(a) => f64::from(a.tick) + f64::from(t) * f64::from(b.tick - a.tick),
                None => f64::from(b.tick),
            };
            evaluator.frame(
                a.map(|a| &a.effects),
                &b.effects,
                t,
                tick,
                self.eye,
                &mut self.effect_frame,
                &mut self.instances,
            );
            renderer.set_effects(&mut self.effect_frame);
        }
        // One instance per presented actor, in order.
        self.movers.clear();
        for (actor, instance) in b.actors.iter().zip(&self.instances[..actors]) {
            if let Some((surface, elevation)) = actor.walker {
                let p = instance.transform.w_axis;
                self.movers.push(Mover {
                    id: actor.key as u32,
                    x: p.x,
                    z: p.z,
                    surface,
                    elevation,
                });
            }
        }
        renderer.set_actors(&self.instances);
    }
}
