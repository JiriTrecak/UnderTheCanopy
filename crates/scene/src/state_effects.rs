//! A definition's `presentation.effects` (D65), for buildings and units alike: each
//! `{effect, state | event, socket}` loops from a model socket for as long as the model is
//! in `state`, or plays once whenever its clip crosses `event`. Looping effects follow
//! their socket as the model moves and animates, and their lights take only the ambient
//! share of the effect lights ([`crate::effects::AMBIENT_LIGHTS`]).
//!
//! Also the activity that lights a material's `activeGlow`: 1 while a model works, eased
//! so machinery spins up and winds down rather than snapping.

use crate::{
    effects::{EffectDirector, Play},
    live::affine_point,
    models::ModelInfo,
};
use canopy_content::game::{Socket, StateEffect};
use glam::{Quat, Vec3};
use std::collections::BTreeMap;

/// Activity change per tick: about 0.4 s from rest to full work and back.
pub const ACTIVITY_STEP: f32 = 0.06;

/// Activity one tick on, easing towards 1 while `working` and 0 at rest.
pub fn ease_activity(activity: f32, working: bool) -> f32 {
    let target = if working { 1.0 } else { 0.0 };
    activity + (target - activity).clamp(-ACTIVITY_STEP, ACTIVITY_STEP)
}

/// A model as drawn this tick, for its effect sockets.
pub struct Mount<'a> {
    pub info: &'a ModelInfo,
    /// The asset's declared sockets (name to node).
    pub sockets: Option<&'a [Socket]>,
    /// Current clip and seconds into it.
    pub clip: (u16, f32),
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    /// A turret's bearing: sockets on the turret turn with it.
    pub aim: Option<f32>,
}

impl Mount<'_> {
    /// Where `socket` stands in the world: a declared socket name resolves to its node,
    /// anything else names a node; the model's origin without a socket or such a node.
    pub fn point(&self, socket: Option<&str>) -> Vec3 {
        let Some(socket) = socket else {
            return self.position;
        };
        let node = self
            .sockets
            .and_then(|s| s.iter().find(|k| k.name == socket))
            .map_or(socket, |k| k.node.as_str());
        self.info
            .socket_or_rest(node, self.clip.0, self.clip.1)
            .map_or(self.position, |m| {
                let local = self
                    .info
                    .aimed(node, affine_point(&m, Vec3::ZERO), self.aim);
                self.position + self.rotation * (local * self.scale)
            })
    }
}

/// The looping state effects one model is playing, by the effect's index in its
/// presentation.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct StateCues {
    playing: BTreeMap<usize, Vec<u32>>,
}

/// What a model is doing this tick, for its effects.
pub struct Doing<'a> {
    /// Seen now (a remembered building or an unseen unit plays nothing).
    pub shown: bool,
    /// Whether it is in a state an effect names.
    pub in_state: &'a dyn Fn(&str) -> bool,
    /// Clip events crossed this tick.
    pub events: &'a [String],
}

impl StateCues {
    /// Ends every looping effect (the model left the view or changed).
    pub fn stop(&mut self, director: &mut EffectDirector) {
        for cues in std::mem::take(&mut self.playing).values() {
            director.stop(cues);
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing.values().any(|c| !c.is_empty())
    }

    /// One tick of `effects` for the model `entity` mounted at `mount`: looping effects
    /// start as it enters their state and stop as it leaves, event effects play once per
    /// crossed event, and the looping ones playing are moved onto their sockets
    /// (`moves`, for [`EffectDirector::relocate`]).
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        director: &mut EffectDirector,
        effects: &[StateEffect],
        entity: u32,
        tick: u32,
        doing: &Doing<'_>,
        mount: &Mount<'_>,
        moves: &mut BTreeMap<u32, Vec3>,
    ) {
        for (k, effect) in effects.iter().enumerate() {
            let Some(index) = director.find(effect.effect.as_str()) else {
                continue;
            };
            let seed = entity.wrapping_mul(0x9e37_79b1) ^ tick ^ (k as u32).wrapping_mul(7919);
            let play = |at: Vec3| Play {
                effect: index,
                tick: f64::from(tick),
                seed,
                at,
                from: None,
                radius: None,
            };
            let socket = effect.socket.as_deref();
            if let Some(wanted) = &effect.state {
                let active = doing.shown && (doing.in_state)(wanted);
                let playing = self.playing.get(&k).is_some_and(|c| !c.is_empty());
                if active && !playing {
                    let cues = director.play_ambient(play(mount.point(socket)));
                    self.playing.insert(k, cues);
                } else if active {
                    let at = mount.point(socket);
                    for &cue in self.playing.get(&k).into_iter().flatten() {
                        moves.insert(cue, at);
                    }
                } else if playing {
                    let cues = self.playing.remove(&k).unwrap_or_default();
                    director.stop(&cues);
                }
            } else if let Some(event) = &effect.event
                && doing.shown
            {
                for _ in doing.events.iter().filter(|n| *n == event) {
                    director.play(play(mount.point(socket)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        effects::{AMBIENT_LIGHTS, EffectScene, EffectSnapshot, Sightings},
        models::ClipInfo,
    };
    use canopy_assets::{
        CookedEffect, EffectBlend, EffectLayer, EffectTable, Matrix3x4, Motion, Primitive,
    };
    use canopy_sim::entity::Milli;
    use std::sync::Arc;

    fn layer(motion: Motion, primitive: Primitive) -> EffectLayer {
        EffectLayer {
            id: "layer".into(),
            primitive,
            motion,
            start: 0.0,
            duration: 0.5,
            sustain: true,
            at_source: false,
            follow: false,
            offset: [0.0; 3],
            color: [1.0, 0.5, 0.0],
            accent: [1.0; 3],
            count: 1,
            size: 1.0,
            height: 0.0,
            intensity: None,
            core: 0.0,
            spread: None,
            length: None,
            arc_height: 0.0,
            blend: EffectBlend::Additive,
            ground: false,
            disc: false,
            texture: None,
            flipbook: None,
            model: None,
            attachment: None,
            emitter: None,
            cue_motion: None,
            trail: None,
            rain: None,
            rides: None,
            ribbon: None,
            ramp: None,
            opacity_curve: None,
            scale_curve: None,
            soft: None,
        }
    }

    fn director(effects: Vec<(&str, bool, Primitive)>) -> EffectDirector {
        let mut effects: Vec<CookedEffect> = effects
            .into_iter()
            .map(|(id, looping, primitive)| CookedEffect {
                id: id.into(),
                duration: 1.0,
                looping,
                layers: vec![layer(
                    if primitive == Primitive::Light {
                        Motion::Light
                    } else {
                        Motion::Billboard
                    },
                    primitive,
                )],
            })
            .collect();
        effects.sort_by(|a, b| a.id.cmp(&b.id));
        EffectDirector::bind(
            Arc::new(EffectTable { effects }),
            &[],
            BTreeMap::new(),
            |_| None,
        )
    }

    struct Nowhere;

    impl EffectScene for Nowhere {
        fn anchor(&self, _: u32) -> Option<Vec3> {
            None
        }
        fn socket(&self, _: u32, _: &str) -> Option<(Vec3, Quat)> {
            None
        }
        fn floor(&self, _: Milli, _: Option<&str>) -> f32 {
            0.0
        }
        fn missile_pose(&self, _: u32) -> Option<(Vec3, Vec3)> {
            None
        }
    }

    fn step(d: &mut EffectDirector, tick: u32) -> EffectSnapshot {
        d.step(
            &Sightings {
                tick,
                ..Sightings::default()
            },
            &Nowhere,
        )
    }

    fn at(x: f32, y: f32, z: f32) -> Matrix3x4 {
        [1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, y, 0.0, 0.0, 1.0, z]
    }

    /// A walker with an exhaust pipe two metres up and a muzzle a metre ahead.
    fn walker() -> ModelInfo {
        ModelInfo {
            name: "walker".into(),
            clips: vec![ClipInfo {
                name: "walk".into(),
                duration: 1.0,
                frames: 1,
                sockets: vec![at(0.0, 2.0, 0.0), at(0.0, 1.0, 1.0)],
            }],
            sockets: vec!["socket_pipe".into(), "socket_muzzle".into()],
            socket_rest: vec![at(0.0, 2.0, 0.0), at(0.0, 1.0, 1.0)],
            ..ModelInfo::default()
        }
    }

    fn mount<'a>(
        info: &'a ModelInfo,
        sockets: Option<&'a [Socket]>,
        x: f32,
        scale: f32,
    ) -> Mount<'a> {
        Mount {
            info,
            sockets,
            clip: (0, 0.0),
            position: Vec3::new(x, 0.0, 0.0),
            rotation: Quat::IDENTITY,
            scale: Vec3::splat(scale),
            aim: None,
        }
    }

    #[test]
    fn effects_loop_in_their_state_follow_their_socket_and_fire_on_events() {
        let mut d = director(vec![
            ("effect.test.exhaust", true, Primitive::Particle),
            ("effect.test.blast", false, Primitive::Particle),
        ]);
        let info = walker();
        let sockets: Vec<Socket> = serde_json::from_value(serde_json::json!([
            {"name": "pipe", "node": "socket_pipe", "offset": [0, 0, 0]}
        ]))
        .unwrap();
        let effects: Vec<StateEffect> = serde_json::from_value(serde_json::json!([
            {"effect": "effect.test.exhaust", "state": "move", "socket": "pipe"},
            {"effect": "effect.test.blast", "event": "release", "socket": "socket_muzzle"},
        ]))
        .unwrap();
        let mut cues = StateCues::default();
        let mut tick_at = |d: &mut EffectDirector,
                           (tick, x): (u32, f32),
                           moving: bool,
                           events: &[String],
                           shown: bool| {
            let in_state = |s: &str| s == "move" && moving;
            let mut moves = BTreeMap::new();
            cues.update(
                d,
                &effects,
                7,
                tick,
                &Doing {
                    shown,
                    in_state: &in_state,
                    events,
                },
                &mount(&info, Some(&sockets), x, 2.0),
                &mut moves,
            );
            d.relocate(&moves);
            (step(d, tick), cues.is_playing())
        };
        assert!(
            tick_at(&mut d, (1, 0.0), false, &[], true)
                .0
                .cues
                .is_empty()
        );
        // Moving: the exhaust starts at the pipe (a declared socket name, drawn at 2×).
        let (snap, _) = tick_at(&mut d, (2, 0.0), true, &[], true);
        assert_eq!(snap.cues.len(), 1);
        assert!(
            (snap.cues[0].root.y - 4.0).abs() < 0.1,
            "{:?}",
            snap.cues[0].root
        );
        // It follows the walker.
        let (snap, _) = tick_at(&mut d, (3, 5.0), true, &[], true);
        assert_eq!(snap.cues.len(), 1);
        assert!((snap.cues[0].root.x - 5.0).abs() < 1e-4);
        // A release fires the blast once, at the muzzle.
        let exhaust = snap.cues[0].id;
        let (snap, _) = tick_at(&mut d, (4, 5.0), true, &["release".into()], true);
        assert_eq!(snap.cues.len(), 2);
        let blast = snap.cues.iter().find(|c| c.id != exhaust).unwrap().clone();
        assert!((blast.root.z - 2.0).abs() < 0.1, "{:?}", blast.root);
        // Standing still ends the exhaust; out of sight nothing plays.
        let (snap, playing) = tick_at(&mut d, (5, 5.0), false, &[], true);
        assert!(snap.cues.iter().all(|c| c.id == blast.id) && !playing);
        let (snap, playing) = tick_at(&mut d, (6, 5.0), true, &["release".into()], false);
        assert!(snap.cues.iter().all(|c| c.id == blast.id) && !playing);
    }

    #[test]
    fn working_machines_leave_effect_lights_for_spells() {
        let mut d = director(vec![("effect.test.glow", true, Primitive::Light)]);
        let info = walker();
        let effects: Vec<StateEffect> = serde_json::from_value(serde_json::json!([
            {"effect": "effect.test.glow", "state": "always"}
        ]))
        .unwrap();
        let mut machines = vec![StateCues::default(); 12];
        for (i, cues) in machines.iter_mut().enumerate() {
            cues.update(
                &mut d,
                &effects,
                i as u32,
                1,
                &Doing {
                    shown: true,
                    in_state: &|s| s == "always",
                    events: &[],
                },
                &mount(&info, None, i as f32 * 10.0, 1.0),
                &mut BTreeMap::new(),
            );
        }
        assert_eq!(step(&mut d, 1).cues.len(), AMBIENT_LIGHTS);
        // A spell's flash still gets its light.
        let index = d.find("effect.test.glow").unwrap();
        let flash = d.play(Play {
            effect: index,
            tick: 2.0,
            seed: 1,
            at: Vec3::ZERO,
            from: None,
            radius: None,
        });
        assert_eq!(flash.len(), 1);
    }

    #[test]
    fn a_model_without_clips_keeps_its_sockets() {
        let mut info = walker();
        info.clips.clear();
        let m = mount(&info, None, 1.0, 1.0);
        assert_eq!(m.point(Some("socket_pipe")), Vec3::new(1.0, 2.0, 0.0));
        assert_eq!(m.point(Some("socket_none")), m.position);
        assert_eq!(m.point(None), m.position);
    }
}
