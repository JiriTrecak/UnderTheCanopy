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
    ui_bodies: Vec<Body>,
    keys: Vec<u64>,
    /// Entities with a selection body as the latest frame shows them: id, body (circle
    /// radius, pick height) and interpolated position.
    bodies: Vec<(u32, [f32; 2], glam::Vec3)>,
    /// Building foundations of the latest frame (D44).
    foundations: Vec<canopy_render::terrain::foundations::Foundation>,
    /// An entity whose model is not drawn (the first-person camera's own body).
    pub hidden_entity: Option<u32>,
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

    /// The minimap's markers and the camera's view quad for the latest frame.
    pub fn draw_minimap(
        &self,
        renderer: &mut Renderer,
        camera: &canopy_render::CameraView,
        scene: &canopy_scene::MapScene,
        pixels: u32,
    ) {
        let keys = &self.keys;
        let instances = &self.instances;
        let position = |key: u64| {
            keys.binary_search(&key)
                .ok()
                .map(|i| instances[i].transform.w_axis.truncate())
        };
        let mut frame = canopy_render::minimap::MinimapFrame::default();
        canopy_scene::world_ui::minimap_frame(
            &self.ui_bodies,
            &position,
            &scene.colors,
            scene.size,
            pixels,
            canopy_scene::world_ui::ground_footprint(camera),
            &mut frame,
        );
        renderer.set_minimap(&mut frame);
    }

    /// The portrait of body `id` as the latest frame shows it: its model at its scale,
    /// playing its idle clip (`seconds` of real time).
    pub fn portrait(&self, id: u32, seconds: f32) -> Option<canopy_render::PortraitSubject> {
        let i = self.ui_bodies.binary_search_by_key(&id, |b| b.id).ok()?;
        let b = &self.ui_bodies[i];
        let colour = self
            .keys
            .binary_search(&b.key)
            .ok()
            .and_then(|k| self.instances[k].owner_color);
        let mut instance =
            ActorInstance::rest(b.model, glam::Mat4::from_scale(glam::Vec3::splat(b.scale)));
        instance.owner_color = colour;
        if let Some((clip, duration)) = b.idle.filter(|&(_, d)| d > 0.0) {
            instance.poses[0] = canopy_render::actors::Pose {
                clip,
                time: (seconds * canopy_scene::animation::SPEED) % duration,
                weight: 1.0,
            };
            instance.pose_count = 1;
        }
        Some(canopy_render::PortraitSubject {
            instance,
            building: b.building,
        })
    }

    /// The units giant canopy models open around (old `cutawaySubjects`): the living
    /// units the frame draws, where they are drawn, except a hidden camera body.
    pub fn cutaway_subjects(
        &self,
    ) -> impl Iterator<Item = canopy_render::cutaway::CutawaySubject> + '_ {
        self.ui_bodies
            .iter()
            .filter(|b| !b.building && !b.item && !b.deposit && !b.remembered && b.hp != Some(0))
            .filter(|b| self.hidden_entity != Some(b.id))
            .map(|b| canopy_render::cutaway::CutawaySubject {
                position: self
                    .keys
                    .binary_search(&b.key)
                    .ok()
                    .map_or(b.position, |i| {
                        self.instances[i].transform.w_axis.truncate()
                    }),
                height: b.height,
            })
    }

    /// How far a camera ray from `origin` along unit `dir` runs before scenery or a
    /// building stops it, up to `far` (old `cameraObstruction`): felled trees and units
    /// let it through.
    pub fn camera_obstruction(
        &self,
        obstacles: &canopy_scene::obstruction::Obstacles,
        origin: glam::Vec3,
        dir: glam::Vec3,
        far: f32,
    ) -> f32 {
        let hidden = |i: usize| {
            self.applied
                .get(i / 64)
                .is_some_and(|w| w & (1 << (i % 64)) != 0)
        };
        let mut nearest = obstacles.scenery(origin, dir, far, &hidden);
        for b in self.ui_bodies.iter().filter(|b| b.building || b.deposit) {
            if let Ok(i) = self.keys.binary_search(&b.key) {
                let x = &self.instances[i];
                nearest = obstacles.live(x.model, &x.transform, origin, dir, nearest);
            }
        }
        nearest
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
        let water = scene.water_field();
        let ground = canopy_scene::live::FoundationGround {
            ground: &water,
            foundations: &self.foundations,
        };
        ui.build(
            &self.ui_bodies,
            &position,
            &self.world_ui_input,
            camera,
            renderer.size(),
            &ground,
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

    /// The latest frame's selectable bodies: id, (circle radius, pick height), position.
    pub fn bodies(&self) -> &[(u32, [f32; 2], glam::Vec3)] {
        &self.bodies
    }

    /// Where the latest frame draws an entity's model (model space to world).
    pub fn entity_transform(&self, id: u32) -> Option<glam::Mat4> {
        let key = ((canopy_scene::live::Part::Entity as u64) << 40) | u64::from(id);
        let i = self.keys.binary_search(&key).ok()?;
        self.instances.get(i).map(|x| x.transform)
    }

    /// The latest frame's live model instances.
    pub fn instances(&self) -> &[ActorInstance] {
        &self.instances
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
        self.ui_bodies.clone_from(&b.bodies);
        canopy_scene::live::foundations(&b.bodies, &mut self.foundations);
        renderer.set_foundations(gpu, &self.foundations);
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
        self.bodies.clear();
        for (actor, instance) in b.actors.iter().zip(&self.instances[..actors]) {
            if let Some(body) = actor.body
                && actor.key >> 40 == canopy_scene::live::Part::Entity as u64
            {
                self.bodies
                    .push((actor.key as u32, body, instance.transform.w_axis.truncate()));
            }
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
        if let Some(id) = self.hidden_entity {
            let key = ((canopy_scene::live::Part::Entity as u64) << 40) | u64::from(id);
            if let Ok(i) = self.keys.binary_search(&key)
                && let Some(x) = self.instances.get_mut(i)
            {
                x.shadow_only = true;
            }
        }
        renderer.set_actors(&self.instances);
    }
}
