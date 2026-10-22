//! The scene half of the `mission` table (docs/campaign.md "Scenes"): scenes and their
//! waits, cinematic mode, saved cameras, fades, animations and spoken lines.
//!
//! A scene is a function of the script's `scenes` table, started with `mission.play`.
//! Every run calls it from the top. Each call it makes to the API, other than a query, is
//! a numbered statement: the statements done in earlier runs are passed over (they answer
//! nothing), the next one is done, and a wait whose time has not come ends the run there
//! ([`SUSPEND`]). When the function returns, the scene is over. A skip does every remaining
//! statement at once: waits pass, walks end where they lead, turns are made, lines are
//! dropped, cameras cut and fades jump to their end, so the world stands as if the scene
//! had played.

use super::{Api, Args, Reply, milli, none, number, one, round, text};
use crate::mission::{
    TICKS_PER_SECOND, alive,
    cinema::{
        self, Animation, Fade, MAX_ANIMATIONS, MAX_SCENE_MOVES, OPAQUE, Pose, SceneMove, ScenePlay,
        Shot,
    },
    effect::MissionEffect,
    entity_by_placement,
    sandbox::{Host, SUSPEND, Scalar},
    state::Dialogue,
};
use canopy_content::game::Kind;

pub const NAMES: [&str; 15] = [
    "clicks",
    "play",
    "playing",
    "cinematic",
    "camera_to",
    "camera_pan",
    "camera_reset",
    "fade_out",
    "fade_in",
    "animate",
    "speak",
    "wait",
    "wait_arrived",
    "wait_facing",
    "wait_line",
];

/// Functions that only read: inside a scene they answer every run and are no statements.
/// A new read-only function of the `mission` table belongs here.
pub const QUERIES: [&str; 14] = [
    "clicks",
    "has_item",
    "dialogue_busy",
    "tick",
    "count",
    "stock",
    "alive",
    "position",
    "in_region",
    "arrived",
    "facing_done",
    "get",
    "playing",
    "campaign_get",
];

/// The longest blend, fade or wait, seconds.
const LONGEST: f64 = 600.0;
/// Reading time of a line without a length: a second plus this many characters a second,
/// within [`LINE_SECONDS`].
const READING_RATE: f64 = 15.0;
const LINE_SECONDS: (f64, f64) = (2.5, 20.0);
/// Non-looping animations are forgotten this long after they started, ticks.
const ANIMATION_MEMORY: u32 = 30 * TICKS_PER_SECOND;

impl Host for Api<'_> {
    fn call(&mut self, name: &'static str, args: Vec<Scalar>) -> Reply {
        Api::call(self, name, args)
    }

    fn scene(&mut self) -> Option<String> {
        let play = self.draft.cinema.scene.as_mut()?;
        let name = play.name.clone();
        if play.skipping {
            // Walks still under way end where they lead before anything else of the
            // scene is done at once.
            for SceneMove {
                placement,
                destination,
            } in std::mem::take(&mut play.moves)
            {
                self.effects.push(MissionEffect::Place {
                    placement,
                    destination,
                    walking: true,
                });
            }
        }
        self.cursor = Some(0);
        Some(name)
    }
}

impl Api<'_> {
    fn skipping(&self) -> bool {
        self.cursor.is_some() && self.draft.cinema.scene.as_ref().is_some_and(|s| s.skipping)
    }

    /// A call the scene makes that is not a query.
    pub(super) fn statement(&mut self, name: &str, args: Vec<Scalar>) -> Reply {
        match name {
            "speak" => {
                let reply = self.step(|api, skipping| {
                    if skipping {
                        none()
                    } else {
                        api.dispatch(name, args)
                    }
                })?;
                self.wait_step(|api| Ok(!api.dialogue_busy()))?;
                Ok(reply)
            }
            "wait" | "wait_arrived" | "wait_facing" | "wait_line" => {
                let a = Args(args);
                self.wait_step(|api| api.ready(name, &a))
            }
            _ => self.step(|api, skipping| {
                if skipping {
                    api.instant(name, args)
                } else {
                    api.walked(name, args)
                }
            }),
        }
    }

    /// The next statement index; done before when below the scene's count.
    fn next_statement(&mut self) -> (bool, bool) {
        let cursor = self.cursor.as_mut().expect("in the scene");
        let index = *cursor;
        *cursor += 1;
        let play = self.draft.cinema.scene.as_ref().expect("a scene in play");
        (index < play.done, play.skipping)
    }

    fn done(&mut self) {
        let tick = self.state.tick;
        if let Some(play) = self.draft.cinema.scene.as_mut() {
            play.done += 1;
            play.since = tick;
        }
    }

    fn step(&mut self, act: impl FnOnce(&mut Self, bool) -> Reply) -> Reply {
        let (before, skipping) = self.next_statement();
        if before {
            return none();
        }
        let reply = act(self, skipping)?;
        self.done();
        Ok(reply)
    }

    fn wait_step(&mut self, ready: impl FnOnce(&Self) -> Result<bool, String>) -> Reply {
        let (before, skipping) = self.next_statement();
        if before {
            return none();
        }
        // Arguments are checked even when the wait is skipped.
        if !ready(self)? && !skipping {
            return Err(SUSPEND.into());
        }
        self.done();
        none()
    }

    /// A statement done now; a walk is remembered for a skip.
    fn walked(&mut self, name: &str, args: Vec<Scalar>) -> Reply {
        let effects = self.effects.len();
        let reply = self.dispatch(name, args)?;
        if let Some(MissionEffect::Move {
            placement,
            destination,
            ..
        }) = self.effects[effects..].last()
        {
            let (placement, destination) = (placement.clone(), *destination);
            if let Some(play) = self.draft.cinema.scene.as_mut() {
                play.moves.retain(|m| m.placement != placement);
                play.moves.push(SceneMove {
                    placement,
                    destination,
                });
                if play.moves.len() > MAX_SCENE_MOVES {
                    play.moves.remove(0);
                }
            }
        }
        Ok(reply)
    }

    /// A statement done at once, as a skip does it.
    fn instant(&mut self, name: &str, args: Vec<Scalar>) -> Reply {
        match name {
            "say" | "animate" => none(),
            "move" | "attack_move" | "face" => {
                let effects = self.effects.len();
                let reply = self.dispatch(name, args)?;
                if let Some(effect) = self.effects.get_mut(effects) {
                    *effect = match effect.clone() {
                        MissionEffect::Move {
                            placement,
                            destination,
                            ..
                        } => MissionEffect::Place {
                            placement,
                            destination,
                            walking: false,
                        },
                        MissionEffect::Face { placement } => {
                            let index = self
                                .draft
                                .facing
                                .iter()
                                .position(|f| f.placement == placement);
                            match index.map(|i| self.draft.facing.remove(i)) {
                                Some(facing) => MissionEffect::Turn {
                                    placement,
                                    target: facing.target,
                                },
                                None => MissionEffect::Face { placement },
                            }
                        }
                        other => other,
                    };
                }
                Ok(reply)
            }
            _ => self.dispatch(name, args),
        }
    }

    /// Whether a wait is over.
    fn ready(&self, name: &str, a: &Args) -> Result<bool, String> {
        match name {
            "wait" => {
                let ticks = seconds(a.get(0), false)?;
                let since = self.draft.cinema.scene.as_ref().map_or(0, |s| s.since);
                Ok(self.state.tick.saturating_sub(since) >= ticks)
            }
            "wait_arrived" => {
                let id = &self.placement(a.get(0))?.id;
                // An order given this run reaches the unit only when the run ends.
                let ordered = self.effects.iter().any(|e| {
                    matches!(e, MissionEffect::Move { placement, .. }
                        | MissionEffect::Follow { placement, .. }
                        | MissionEffect::Attack { placement, .. } if placement == id)
                });
                Ok(!ordered
                    && self
                        .entity(a.get(0))?
                        .filter(|e| e.unit.is_some() && alive(e))
                        .is_none_or(|e| !self.systems.has_order(e)))
            }
            "wait_facing" => Ok(match a.get(0) {
                Scalar::Nil => self.draft.facing.is_empty(),
                raw => {
                    let id = &self.placement(raw)?.id;
                    !self.draft.facing.iter().any(|f| &f.placement == id)
                }
            }),
            _ => Ok(!self.dialogue_busy()),
        }
    }

    /// The scene's function returned: the scene is over. A skipped scene finishes its
    /// turns and drops its line (its walks ended when the skip began).
    pub(in crate::mission) fn scene_ended(&mut self) {
        let tick = self.state.tick;
        let Some(play) = self.draft.cinema.scene.take() else {
            return;
        };
        if !play.skipping {
            return;
        }
        for facing in std::mem::take(&mut self.draft.facing) {
            self.effects.push(MissionEffect::Turn {
                placement: facing.placement,
                target: facing.target,
            });
        }
        self.draft.dialogue = None;
        self.draft.cinema.animations.retain(|a| a.looping);
        self.draft.cinema.skipped = Some(tick);
    }

    /// Animations of the living, the newest [`MAX_ANIMATIONS`].
    pub(in crate::mission) fn tidy_cinema(&mut self) {
        let tick = self.state.tick;
        let state = self.state;
        let cinema = &mut self.draft.cinema;
        cinema.animations.retain(|a| {
            (a.looping || tick.saturating_sub(a.start) < ANIMATION_MEMORY)
                && entity_by_placement(state, &a.placement).is_none_or(alive)
        });
        let extra = cinema.animations.len().saturating_sub(MAX_ANIMATIONS);
        cinema.animations.drain(..extra);
    }

    pub(super) fn scene_call(&mut self, name: &str, a: &Args) -> Reply {
        let tick = self.state.tick;
        let skipping = self.skipping();
        match name {
            "play" => {
                let scene = identifier(a.get(0), 64, "Scene")?;
                let skippable = flag(a.get(1), true, "Skippable")?;
                if self.draft.cinema.scene.is_some() {
                    return Err("A scene is already playing".into());
                }
                self.draft.cinema.scene = Some(ScenePlay {
                    name: scene,
                    done: 0,
                    since: tick,
                    skippable,
                    skipping: false,
                    moves: Vec::new(),
                });
                none()
            }
            "clicks" => {
                let id = &self.placement(a.get(0))?.id;
                let count = self.draft.clicks.get(id).copied().unwrap_or(0);
                one(Scalar::Integer(i64::from(count)))
            }
            "playing" => one(self
                .draft
                .cinema
                .scene
                .as_ref()
                .map_or(Scalar::Nil, |s| Scalar::String(s.name.clone()))),
            "cinematic" => {
                self.draft.cinema.on = flag(a.get(0), true, "Cinematic")?;
                none()
            }
            "camera_to" => {
                let id = text(a.get(0), 64)?;
                if !self.cameras().any(|c| c.id == id) {
                    return Err(format!("Unknown camera: {id}"));
                }
                let pose = Pose {
                    camera: Some(id.to_string()),
                    target: None,
                };
                let ticks = seconds(a.get(1), skipping)?;
                self.shoot(pose, ticks)
            }
            "camera_pan" => {
                let target = milli(self.coordinate(a.get(0))?, self.coordinate(a.get(1))?);
                let ticks = seconds(a.get(2), skipping)?;
                let camera = self
                    .draft
                    .cinema
                    .shot
                    .as_ref()
                    .and_then(|s| s.to.camera.clone())
                    .ok_or("Pan a saved camera: cut to one with camera_to first")?;
                self.shoot(
                    Pose {
                        camera: Some(camera),
                        target: Some(target),
                    },
                    ticks,
                )
            }
            "camera_reset" => {
                let ticks = seconds(a.get(0), skipping)?;
                let target = match (a.get(1), a.get(2)) {
                    (Scalar::Nil, Scalar::Nil) => self.looked_at(),
                    (x, y) => Some(milli(self.coordinate(x)?, self.coordinate(y)?)),
                };
                if self.draft.cinema.shot.is_none() && target.is_none() {
                    return none();
                }
                self.shoot(
                    Pose {
                        camera: None,
                        target,
                    },
                    ticks,
                )
            }
            "fade_out" | "fade_in" => {
                let ticks = seconds(a.get(0), skipping)?;
                let current = self.draft.cinema.fade.as_ref();
                let color = match a.get(1) {
                    Scalar::Nil => current.map_or([0; 3], |f| f.color),
                    Scalar::String(name) => {
                        cinema::color(name).ok_or("Fade colour must be black, white or #rrggbb")?
                    }
                    _ => return Err("Fade colour must be black, white or #rrggbb".into()),
                };
                let from = current.map_or(0, |f| f.at(tick));
                let to = if name == "fade_out" { OPAQUE } else { 0 };
                self.draft.cinema.fade = Some(Fade {
                    from,
                    to,
                    color,
                    start: tick,
                    ticks,
                });
                none()
            }
            "animate" => {
                let placement = self.placement(a.get(0))?;
                if self.placed(placement)?.kind != Kind::Unit {
                    return Err("Only units play animations".into());
                }
                let animation = identifier(a.get(1), 32, "Animation")?;
                let looping = flag(a.get(2), false, "Looping")?;
                let placement = placement.id.clone();
                let list = &mut self.draft.cinema.animations;
                list.retain(|a| a.placement != placement);
                if animation != "idle" {
                    list.push(Animation {
                        placement,
                        name: animation,
                        start: tick,
                        looping,
                    });
                }
                none()
            }
            "speak" => self.speak(a),
            _ => Err(format!("mission.{name} works only inside a scene")),
        }
    }

    fn cameras(&self) -> impl Iterator<Item = &canopy_content::map::MapCamera> {
        self.script.mission.cameras.iter().flatten()
    }

    /// Where the view looks now: the current pose's pan, else its saved camera's target.
    fn looked_at(&self) -> Option<crate::entity::Milli> {
        let pose = &self.draft.cinema.shot.as_ref()?.to;
        pose.target.or_else(|| {
            let id = pose.camera.as_deref()?;
            let camera = self.cameras().find(|c| c.id == id)?;
            Some(milli(camera.x, camera.y))
        })
    }

    fn shoot(&mut self, to: Pose, ticks: u32) -> Reply {
        let from = self
            .draft
            .cinema
            .shot
            .as_ref()
            .map(|s| s.to.clone())
            .unwrap_or_default();
        self.draft.cinema.shot = Some(Shot {
            from,
            to,
            start: self.state.tick,
            ticks,
        });
        none()
    }

    fn speak(&mut self, a: &Args) -> Reply {
        let speaker = self
            .entity(a.get(0))?
            .filter(|e| e.unit.is_some() && alive(e))
            .ok_or("The speaker must be a living unit")?;
        let definition = self.catalog.get(speaker.definition);
        let line = text(a.get(1), 2000)?.to_string();
        let ticks = match a.get(2) {
            Scalar::Nil => {
                let reading = 1.0 + line.chars().count() as f64 / READING_RATE;
                reading.clamp(LINE_SECONDS.0, LINE_SECONDS.1) * f64::from(TICKS_PER_SECOND)
            }
            raw => number(raw, 0.5, 60.0)? * f64::from(TICKS_PER_SECOND),
        };
        let ticks = round(ticks) as u32;
        let name = match a.get(3) {
            Scalar::Nil => definition.name.clone(),
            raw => text(raw, 80)?.to_string(),
        };
        let id = self.draft.next_dialogue;
        self.draft.next_dialogue += 1;
        self.draft.dialogue = Some(Dialogue {
            id,
            actor: Some(speaker.id),
            speaker: name,
            portrait: definition.id.clone(),
            text: line,
            duration_ticks: ticks,
            cinematic: false,
            remaining: 0,
            until: self.state.tick + ticks,
        });
        none()
    }
}

/// Seconds as ticks: nil is 0, a skip makes everything instant.
fn seconds(raw: &Scalar, skipping: bool) -> Result<u32, String> {
    let value = match raw {
        Scalar::Nil => 0.0,
        raw => number(raw, 0.0, LONGEST)?,
    };
    Ok(if skipping {
        0
    } else {
        round(value * f64::from(TICKS_PER_SECOND)) as u32
    })
}

fn flag(raw: &Scalar, default: bool, what: &str) -> Result<bool, String> {
    match raw {
        Scalar::Nil => Ok(default),
        Scalar::Boolean(b) => Ok(*b),
        _ => Err(format!("{what} must be a boolean")),
    }
}

/// A name of letters, digits and underscores, not starting with a digit.
fn identifier(raw: &Scalar, max: usize, what: &str) -> Result<String, String> {
    let name = text(raw, max).map_err(|_| format!("{what} must be a name"))?;
    let valid = name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit());
    if valid {
        Ok(name.to_string())
    } else {
        Err(format!(
            "{what} names use letters, digits and underscores: {name}"
        ))
    }
}
