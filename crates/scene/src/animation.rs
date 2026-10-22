//! Animation state selection and clip clocks, as the old game animated units (old
//! `BakedAnimator`, `attackPhase`, `castAnimation` and the settlement layer's state
//! choice), advanced once per simulation tick so a pose is a function of the tick: shots
//! of the same tick look the same, and the renderer interpolates between ticks.
//!
//! Semantics kept from the old animator: clips play at 1.5× (`SPEED`); semantic aliases
//! that share a clip (run and charge) keep their gait phase; one-shot states clamp at
//! their end and return to idle (except death); a new state fades in linearly while the
//! old one fades out from its current weight; at most the current clip and the two
//! heaviest fading ones are blended; `sample` locks the current clip to authoritative
//! time (attack wind-up and impact, work strokes, cast phases) while fades keep running.
//!
//! New: a locomotion clip whose state declares `nominalSpeed` plays at the unit's actual
//! ground speed over it instead (feet do not slide), and steps report the clip events
//! they crossed, for state effects.

use crate::models::ModelInfo;
use canopy_content::abilities::Animations;
use canopy_render::actors::{MAX_POSES, Pose};

/// Clip time per second of game time (old `CharacterPlayer.speed`).
pub const SPEED: f32 = 1.5;
/// Seconds per simulation tick.
pub const TICK_SECONDS: f32 = 0.025;
/// Default cross-fade, in clip seconds.
pub const FADE: f32 = 0.18;
/// Fallback contact point of attacks and casts without an authored event.
pub const CONTACT: f32 = 0.55;
/// Bounds of a stride-matched clip's rate (clip seconds per second): a crawl or a sprint
/// far from the authored stride stays a plausible gait.
pub const STRIDE_RATE: (f32, f32) = (0.4, 3.0);

/// Engine animation states (the old `AntState` names models declare).
pub mod states {
    pub const IDLE: &str = "idle";
    pub const WALK: &str = "walk";
    pub const RUN: &str = "run";
    pub const CHARGE: &str = "charge";
    pub const ATTACK: &str = "attack";
    pub const HIT: &str = "hit";
    pub const CAST: &str = "cast";
    pub const DEATH: &str = "death";
    pub const CARRY: &str = "carry";
    pub const CARRY_WALK: &str = "carry_walk";
    pub const CARRY_RUN: &str = "carry_run";

    /// Gaits: the states whose clip can follow the ground speed (`nominalSpeed`).
    pub const LOCOMOTION: [&str; 5] = [WALK, RUN, CHARGE, CARRY_WALK, CARRY_RUN];
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Track {
    clip: u16,
    time: f32,
    once: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Fading {
    track: Track,
    from: f32,
    elapsed: f32,
    duration: f32,
}

/// One model's clip clocks and fades.
#[derive(Debug, Clone, PartialEq)]
pub struct Animator {
    state: String,
    current: Track,
    fade_in: Option<(f32, f32)>,
    fading: Vec<Fading>,
    /// The current clip's rate (clip seconds per second of game time).
    rate: f32,
}

impl Animator {
    /// Starts in idle, or the first declared state; `None` without states.
    pub fn new(model: &ModelInfo) -> Option<Self> {
        let state = if model.has(states::IDLE) {
            states::IDLE.to_owned()
        } else {
            model.states.keys().find(|s| model.has(s))?.clone()
        };
        let info = model.state(&state)?;
        Some(Self {
            current: Track {
                clip: info.clip,
                time: 0.0,
                once: !info.looping,
            },
            state,
            fade_in: None,
            fading: Vec::new(),
            rate: SPEED,
        })
    }

    /// Sets the current clip's rate ([`SPEED`] unless a gait follows the ground speed);
    /// fades keep [`SPEED`].
    pub fn set_rate(&mut self, rate: f32) {
        self.rate = rate;
    }

    pub fn state(&self) -> &str {
        &self.state
    }

    /// Current clip position, 0–1.
    pub fn phase(&self, model: &ModelInfo) -> f32 {
        let duration = model.duration(self.current.clip);
        if duration > 0.0 {
            self.current.time / duration
        } else {
            0.0
        }
    }

    /// The current clip and its time.
    pub fn current(&self) -> (u16, f32) {
        (self.current.clip, self.current.time)
    }

    /// Changes state; unknown states are ignored.
    pub fn set_state(&mut self, model: &ModelInfo, state: &str, restart: bool, fade: f32) {
        if self.state == state && !restart {
            return;
        }
        let Some(info) = model.state(state).filter(|_| model.has(state)) else {
            return;
        };
        let previous = self.current;
        self.state = state.to_owned();
        // Semantic aliases (run/charge) share one clip: keep its gait phase.
        if previous.clip == info.clip && !restart {
            return;
        }
        // Re-entering a clip that is still fading out restarts that same action.
        self.fading.retain(|f| f.track.clip != info.clip);
        if previous.clip != info.clip && fade > 0.0 {
            self.fading.push(Fading {
                track: previous,
                from: self.current_weight(),
                elapsed: 0.0,
                duration: fade,
            });
            self.fade_in = Some((0.0, fade));
        } else {
            self.fade_in = None;
        }
        self.current = Track {
            clip: info.clip,
            time: 0.0,
            once: !info.looping,
        };
    }

    /// Advances every clock by `dt` seconds of game time.
    pub fn update(&mut self, model: &ModelInfo, dt: f32) {
        self.advance_fades(model, dt * SPEED);
        let duration = model.duration(self.current.clip);
        let finished = advance(&mut self.current, duration, dt * self.rate);
        if finished && self.state != states::DEATH && model.has(states::IDLE) {
            self.set_state(model, states::IDLE, false, FADE);
        }
    }

    /// Locks the current clip to `normalized` (0–1) while fades advance by `dt`.
    pub fn sample(&mut self, model: &ModelInfo, normalized: f32, dt: f32) {
        self.current.time = normalized.clamp(0.0, 0.999_999) * model.duration(self.current.clip);
        self.advance_fades(model, dt * SPEED);
    }

    fn current_weight(&self) -> f32 {
        self.fade_in
            .map_or(1.0, |(elapsed, duration)| (elapsed / duration).min(1.0))
    }

    fn advance_fades(&mut self, model: &ModelInfo, step: f32) {
        if let Some((elapsed, duration)) = &mut self.fade_in {
            *elapsed += step;
            if *elapsed >= *duration {
                self.fade_in = None;
            }
        }
        for f in &mut self.fading {
            let duration = model.duration(f.track.clip);
            advance(&mut f.track, duration, step);
            f.elapsed += step;
        }
        self.fading.retain(|f| f.elapsed < f.duration);
    }

    /// The blended poses: current first, then the two heaviest fades, normalised; with
    /// each looping pose's clip length (0 for one-shots), for interpolation.
    pub fn poses(&self, model: &ModelInfo) -> Poses {
        let mut out = Poses::default();
        let length = |t: &Track| if t.once { 0.0 } else { model.duration(t.clip) };
        out.poses[0] = Pose {
            clip: self.current.clip,
            time: self.current.time,
            weight: self.current_weight(),
        };
        out.loops[0] = length(&self.current);
        let mut fades: Vec<(Pose, f32)> = self
            .fading
            .iter()
            .map(|f| {
                (
                    Pose {
                        clip: f.track.clip,
                        time: f.track.time,
                        weight: f.from * (1.0 - f.elapsed / f.duration),
                    },
                    length(&f.track),
                )
            })
            .collect();
        fades.sort_by(|a, b| b.0.weight.total_cmp(&a.0.weight));
        let mut count = 1;
        for (pose, loops) in fades.into_iter().take(MAX_POSES - 1) {
            out.poses[count] = pose;
            out.loops[count] = loops;
            count += 1;
        }
        let total: f32 = out.poses[..count].iter().map(|p| p.weight).sum();
        let total = if total > 0.0 { total } else { 1.0 };
        for pose in &mut out.poses[..count] {
            pose.weight /= total;
        }
        out.count = count as u8;
        out
    }
}

/// Up to [`MAX_POSES`] blended poses.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Poses {
    pub poses: [Pose; MAX_POSES],
    /// Clip length of looping poses (they wrap when interpolated), 0 for one-shots.
    pub loops: [f32; MAX_POSES],
    pub count: u8,
}

impl Poses {
    /// One clip at `time`, not looping.
    pub fn single(clip: u16, time: f32) -> Self {
        let mut p = Self::default();
        p.poses[0] = Pose {
            clip,
            time,
            weight: 1.0,
        };
        p.count = 1;
        p
    }

    pub fn used(&self) -> &[Pose] {
        &self.poses[..usize::from(self.count)]
    }
}

/// Advances a track; true when a one-shot reached its end in this step.
fn advance(track: &mut Track, duration: f32, step: f32) -> bool {
    if track.once {
        if track.time >= duration {
            return false;
        }
        track.time += step;
        if track.time < duration {
            return false;
        }
        track.time = duration;
        return true;
    }
    track.time = if duration > 0.0 {
        (track.time + step) % duration
    } else {
        0.0
    };
    false
}

/// The authored weapon clip around the authoritative release tick (old `attackPhase`):
/// wind-up maps onto `[0, contact]`, recovery onto `[contact, 1]`.
pub fn attack_phase(tick: f32, started: u32, impact: u32, ends: u32, contact: f32) -> f32 {
    let (started, impact, ends) = (started as f32, impact as f32, ends as f32);
    let phase = if tick <= impact {
        contact * (tick - started).max(0.0) / (impact - started).max(1.0)
    } else {
        contact + (1.0 - contact) * (tick - impact) / (ends - impact).max(1.0)
    };
    phase.clamp(0.0, 1.0)
}

/// The events of `state` its clip crossed between `before` and `after` (clip, seconds):
/// within one clip from just after `before` up to `after`, around the end of a loop, and
/// from the start when the clip changed or a one-shot restarted.
pub fn crossed(
    model: &ModelInfo,
    state: &str,
    before: (u16, f32),
    after: (u16, f32),
) -> Vec<String> {
    let Some(info) = model.state(state) else {
        return Vec::new();
    };
    let duration = model.duration(after.0);
    if info.events.is_empty() || duration <= 0.0 || info.clip != after.0 {
        return Vec::new();
    }
    let to = after.1 / duration;
    let from = (before.0 == after.0).then_some(before.1 / duration);
    info.events
        .iter()
        .filter(|(_, t)| match from {
            Some(from) if to > from => *t > from && *t <= to,
            Some(from) if to == from => false,
            Some(from) if info.looping => *t > from || *t <= to,
            _ => *t <= to,
        })
        .map(|(name, _)| name.clone())
        .collect()
}

/// A cast's timeline (`CastingView`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CastTimeline {
    pub start: u32,
    pub release: u32,
    pub finish: u32,
    pub channel_end: Option<u32>,
}

/// The pose of a cast at `tick` (old `castAnimation`): the state to show and its phase.
pub fn cast_animation(
    animations: &Animations,
    timeline: CastTimeline,
    tick: f32,
    has: impl Fn(&str) -> bool,
    contact: impl Fn(&str) -> f32,
) -> (String, f32) {
    let pick = |desired: &str| -> String {
        if has(desired) {
            desired.to_owned()
        } else if has(&animations.fallback) {
            animations.fallback.clone()
        } else {
            states::IDLE.to_owned()
        }
    };
    let release = timeline.release as f32;
    let channeling = timeline
        .channel_end
        .is_some_and(|end| tick >= release && tick < end as f32);
    if channeling {
        let desired = animations.channel.as_deref().unwrap_or(&animations.prepare);
        return (pick(desired), ((tick - release) % 80.0) / 80.0);
    }
    let recovery_start = timeline.channel_end.map_or(release, |e| e as f32);
    let recovery = tick >= recovery_start;
    let desired = if tick < release {
        &animations.prepare
    } else if tick < release + 1.0 {
        &animations.release
    } else {
        &animations.recover
    };
    let clip = pick(desired);
    let continuous =
        animations.prepare == animations.release && animations.release == animations.recover;
    let progress = if recovery {
        (tick - recovery_start) / (timeline.finish as f32 - recovery_start).max(1.0)
    } else {
        (tick - timeline.start as f32) / (release - timeline.start as f32).max(1.0)
    };
    let contact = contact(&clip);
    let phase = if continuous {
        if recovery {
            contact + (1.0 - contact) * progress
        } else {
            contact * progress
        }
    } else {
        progress
    };
    (clip, phase.clamp(0.0, 0.999_999))
}

/// What one tick of the simulation says about a unit, for its animation.
#[derive(Debug, Clone, Default)]
pub struct UnitInputs<'a> {
    pub tick: u32,
    pub moving: bool,
    pub strolling: bool,
    pub charging: bool,
    /// The swing in progress: started, impact, ends.
    pub attack: Option<(u32, u32, u32)>,
    pub hp: Option<u32>,
    /// A cast in progress: its ability (identity of the cast with its release tick), the
    /// ability's caster animations and the timeline.
    pub cast: Option<(u16, &'a Animations, CastTimeline)>,
    pub carrying: bool,
    /// The work pose: state name and the strike cycle `(ticks, progress)`.
    pub work: Option<(&'a str, Option<(u32, u32)>)>,
    /// The model's variant takes work poses (the old `variant === "base"`).
    pub works: bool,
    /// Fallback contact of casts (`castContact` of the render asset).
    pub cast_contact: Option<f32>,
    /// Ground speed in the model's own metres per second (world speed over the drawn
    /// scale), measured while moving: gaits with a `nominalSpeed` follow it.
    pub speed: Option<f32>,
    /// A mission scene's animation (`mission.animate`): the state, the tick it began and
    /// whether it is held. It plays while the unit stands and fights nothing.
    pub scripted: Option<(&'a str, u32, bool)>,
}

/// The state a scripted animation plays: its own, else a near one the model has
/// (`spell` and `victory` as `cast`).
pub fn scripted_state<'a>(model: &ModelInfo, name: &'a str) -> &'a str {
    match name {
        _ if model.has(name) => name,
        "spell" | "victory" if model.has(states::CAST) => states::CAST,
        _ => name,
    }
}

/// A unit's animation: the clocks and what the previous tick showed.
#[derive(Debug, Clone, PartialEq)]
pub struct UnitAnimation {
    pub animator: Animator,
    attack_started: Option<u32>,
    last_hp: Option<u32>,
    /// Tick of the latest hit reaction.
    pub reaction: Option<u32>,
    cast_key: Option<(u16, u32)>,
    was_casting: bool,
    first: bool,
    /// The start tick of the scripted animation last begun.
    scripted: Option<u32>,
}

/// A building's states (D65). A building at rest holds its `idle` pose and nothing moves;
/// it plays `work` for as long as it trains, researches, upgrades or revives, and `spawn`
/// once whenever a training cycle ends. Motion is the signal that something happens.
pub mod building_states {
    pub const IDLE: &str = "idle";
    pub const WORK: &str = "work";
    pub const SPAWN: &str = "spawn";
    /// A tower's shot (its crew fired).
    pub const ATTACK: &str = "attack";
}

/// A tower's shot cuts in almost at once (clip seconds).
pub const ATTACK_FADE: f32 = 0.04;

/// Fade between a building's rest and its work, in clip seconds: machinery starting up
/// and winding down rather than snapping.
pub const BUILDING_FADE: f32 = 0.45;

/// A building's animation: rest, work and spawn from its public activity.
#[derive(Debug, Clone, PartialEq)]
pub struct BuildingAnimation {
    pub animator: Animator,
    cycles: Option<u32>,
}

impl BuildingAnimation {
    /// `None` unless the model has a rest pose and something to play.
    pub fn new(model: &ModelInfo) -> Option<Self> {
        use building_states::*;
        if !model.has(IDLE) || !(model.has(WORK) || model.has(SPAWN) || model.has(ATTACK)) {
            return None;
        }
        Some(Self {
            animator: Animator::new(model)?,
            cycles: None,
        })
    }

    /// One tick: picks the state from `busy`, the finished training `cycles` and whether
    /// its crew `fired` (a tower's `attack`, once per shot), advances the clock, and
    /// returns the clip events crossed this tick (for effects).
    pub fn step(&mut self, model: &ModelInfo, busy: bool, cycles: u32, fired: bool) -> Vec<String> {
        use building_states::*;
        let spawned = self.cycles.is_some_and(|c| cycles > c);
        let first = self.cycles.is_none();
        self.cycles = Some(cycles);
        let a = &mut self.animator;
        if fired && model.has(ATTACK) {
            a.set_state(model, ATTACK, true, ATTACK_FADE);
        } else if spawned && model.has(SPAWN) {
            a.set_state(model, SPAWN, true, BUILDING_FADE);
        } else if a.state() != SPAWN && a.state() != ATTACK {
            let wanted = if busy && model.has(WORK) { WORK } else { IDLE };
            a.set_state(
                model,
                wanted,
                false,
                if first { 0.0 } else { BUILDING_FADE },
            );
        }
        let state = a.state().to_owned();
        let before = a.current();
        a.update(model, if first { 0.0 } else { TICK_SECONDS });
        let after = a.current();
        if after.0 != before.0 || state == IDLE || state != a.state() {
            return Vec::new();
        }
        crossed(model, &state, before, after)
    }
}

/// The rate of `state`'s clip: a gait with a `nominalSpeed` plays at the measured ground
/// `speed` over it (within [`STRIDE_RATE`]); everything else at [`SPEED`].
pub fn stride_rate(model: &ModelInfo, state: &str, speed: Option<f32>) -> f32 {
    let nominal = model
        .state(state)
        .and_then(|s| s.nominal_speed)
        .filter(|_| states::LOCOMOTION.contains(&state));
    match (nominal, speed) {
        (Some(nominal), Some(speed)) => (speed / nominal).clamp(STRIDE_RATE.0, STRIDE_RATE.1),
        _ => SPEED,
    }
}

/// What the animation decided beyond the pose.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Decision {
    /// The unit faces its work target instead of its own rotation.
    pub face_work: bool,
    /// Clip events of the current state crossed this tick (for event effects).
    pub events: Vec<String>,
}

impl UnitAnimation {
    pub fn new(model: &ModelInfo) -> Option<Self> {
        Some(Self {
            animator: Animator::new(model)?,
            attack_started: None,
            last_hp: None,
            reaction: None,
            cast_key: None,
            was_casting: false,
            first: true,
            scripted: None,
        })
    }

    fn contact(model: &ModelInfo, state: &str, fallback: f32) -> f32 {
        model
            .state(state)
            .and_then(|s| s.contact)
            .unwrap_or(fallback)
    }

    /// One tick of the old settlement layer's state choice and clock.
    pub fn step(&mut self, model: &ModelInfo, i: &UnitInputs<'_>) -> Decision {
        let first = self.first;
        let dt = if first { 0.0 } else { TICK_SECONDS };
        self.first = false;
        let tick = i.tick as f32;
        let a = &mut self.animator;
        let previous = a.state.clone();
        let before = a.current();
        let attacked = i
            .attack
            .is_some_and(|(started, ..)| self.attack_started != Some(started));
        if let Some((started, ..)) = i.attack {
            self.attack_started = Some(started);
        }
        let hurt = matches!((self.last_hp, i.hp), (Some(before), Some(now)) if now < before);
        if hurt {
            self.reaction = Some(i.tick);
        }
        let cast_contact = i.cast_contact.unwrap_or(CONTACT);
        let cast_pose = i.cast.map(|(ability, animations, timeline)| {
            let key = (ability, timeline.release);
            let started = self.cast_key != Some(key);
            let (state, phase) = cast_animation(
                animations,
                timeline,
                tick,
                |s| model.has(s),
                |s| Self::contact(model, s, cast_contact),
            );
            (key, started, state, phase)
        });
        let mut decision = Decision::default();
        let moving = i.moving;
        if i.charging {
            // A physical lunge owns the pose even while cast recovery is active.
            a.set_state(model, states::CHARGE, false, 0.025);
        } else if let Some((key, started, state, _)) = &cast_pose {
            a.set_state(model, state, *started, 0.08);
            self.cast_key = Some(*key);
        } else if self.was_casting && !moving && i.attack.is_none() {
            a.set_state(model, states::IDLE, false, 0.12);
        } else if i.attack.is_some() && !moving {
            a.set_state(model, states::ATTACK, attacked, FADE);
        } else if attacked {
            a.set_state(model, states::ATTACK, true, FADE);
        } else if hurt && !moving && i.work.and_then(|w| w.1).is_none() {
            a.set_state(model, states::HIT, true, FADE);
        } else if let Some((name, start, looping)) =
            i.scripted.filter(|_| !moving && i.attack.is_none())
        {
            let state = scripted_state(model, name);
            if self.scripted != Some(start) {
                a.set_state(model, state, true, FADE);
                self.scripted = Some(start);
            } else if looping && a.state != state {
                a.set_state(model, state, false, FADE);
            }
            // Played once, it falls back to idle by itself and stays there.
        } else if moving {
            let carrying = if i.strolling {
                states::CARRY_WALK
            } else {
                states::CARRY_RUN
            };
            let state = if i.carrying && model.has(carrying) {
                carrying
            } else if i.strolling {
                states::WALK
            } else {
                states::RUN
            };
            a.set_state(model, state, false, FADE);
        } else if i.attack.is_none() && a.state == states::ATTACK {
            a.set_state(model, states::IDLE, false, FADE);
        } else if ![states::ATTACK, states::HIT, states::CAST].contains(&a.state.as_str()) {
            let work = if i.works { i.work } else { None };
            let state = match work {
                Some((animation, _)) => animation,
                None if i.carrying && model.has(states::CARRY) => states::CARRY,
                None => states::IDLE,
            };
            a.set_state(model, state, false, FADE);
            decision.face_work = work.is_some();
        }
        self.was_casting = cast_pose.is_some();
        let cycle = i.work.and_then(|w| w.1);
        a.set_rate(stride_rate(model, &a.state, i.speed));
        if i.charging {
            a.update(model, if previous == states::CHARGE { dt } else { 0.0 });
        } else if let Some((_, _, _, phase)) = &cast_pose {
            a.sample(model, *phase, dt);
        } else if let (Some((started, impact, ends)), true) = (i.attack, a.state == states::ATTACK)
        {
            let contact = Self::contact(model, states::ATTACK, CONTACT);
            let phase = attack_phase(tick, started, impact, ends, contact);
            a.sample(model, phase.min(0.999_999), dt);
        } else if let (Some((ticks, progress)), Some((animation, _))) = (cycle, i.work)
            && a.state == animation
        {
            // The authoritative work phase locks the stroke to the damage tick.
            a.sample(
                model,
                (progress as f32 / ticks.max(1) as f32).min(0.999_999),
                dt,
            );
        } else {
            let restarted =
                cast_pose.as_ref().is_some_and(|c| c.1) || (hurt && a.state == states::HIT);
            a.update(
                model,
                if previous == a.state && !restarted {
                    dt
                } else {
                    0.0
                },
            );
        }
        self.last_hp = i.hp;
        if !first {
            decision.events = crossed(model, &a.state, before, a.current());
        }
        decision
    }

    /// Starts the death clip (the unit died this tick).
    pub fn die(&mut self, model: &ModelInfo) {
        self.animator.set_state(model, states::DEATH, true, FADE);
    }

    /// The hit recoil at `tick` (old local recoil): 0–1, for 0.22 s after a hit.
    pub fn recoil(&self, tick: u32) -> f32 {
        let Some(at) = self.reaction else {
            return 0.0;
        };
        let age = tick.saturating_sub(at) as f32 * TICK_SECONDS;
        if age < 0.22 {
            (std::f32::consts::PI * (age / 0.16).min(1.0)).sin() * (-age * 8.0).exp()
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::{ClipInfo, StateInfo};

    fn clip(name: &str, duration: f32) -> ClipInfo {
        ClipInfo {
            name: name.into(),
            duration,
            frames: (duration * 30.0).ceil() as u32 + 1,
            sockets: Vec::new(),
        }
    }

    /// A worker-like model: idle, walk, run (charge aliases run), attack with a contact at
    /// 0.4, hit, death, carry poses and chop.
    fn model() -> ModelInfo {
        let clips = vec![
            clip("idle", 2.0),
            clip("walk", 1.0),
            clip("run", 0.8),
            clip("attack", 1.0),
            clip("hit", 0.5),
            clip("death", 1.5),
            clip("carry_run", 0.8),
            clip("chop", 1.2),
            clip("cast", 1.0),
        ];
        let state = |clip: u16, looping: bool, contact: Option<f32>| StateInfo {
            clip,
            looping,
            contact,
            events: Vec::new(),
            nominal_speed: None,
        };
        let states = [
            ("idle", state(0, true, None)),
            ("walk", state(1, true, None)),
            ("run", state(2, true, None)),
            ("charge", state(2, true, None)),
            ("attack", state(3, false, Some(0.4))),
            ("hit", state(4, false, None)),
            ("death", state(5, false, None)),
            ("carry_run", state(6, true, None)),
            ("chop", state(7, true, None)),
            ("cast", state(8, false, Some(0.5))),
        ]
        .into_iter()
        .map(|(n, s)| (n.to_owned(), s))
        .collect();
        ModelInfo {
            name: "test".into(),
            clips,
            states,
            ..ModelInfo::default()
        }
    }

    fn step(u: &mut UnitAnimation, m: &ModelInfo, i: UnitInputs) -> Decision {
        u.step(m, &i)
    }

    #[test]
    fn clocks_run_at_one_and_a_half_and_loop() {
        let m = model();
        let mut a = Animator::new(&m).unwrap();
        assert_eq!(a.state(), "idle");
        for _ in 0..40 {
            a.update(&m, TICK_SECONDS);
        }
        // One second of game time is 1.5 s of clip time, inside the 2 s idle.
        assert!((a.current().1 - 1.5).abs() < 1e-4);
        for _ in 0..40 {
            a.update(&m, TICK_SECONDS);
        }
        assert!((a.current().1 - 1.0).abs() < 1e-3, "{:?}", a.current());
    }

    #[test]
    fn fades_blend_old_and_new_and_end() {
        let m = model();
        let mut a = Animator::new(&m).unwrap();
        a.set_state(&m, "walk", false, FADE);
        let p = a.poses(&m);
        assert_eq!(p.count, 2);
        assert_eq!((p.poses[0].clip, p.poses[0].weight), (1, 0.0));
        assert_eq!((p.poses[1].clip, p.poses[1].weight), (0, 1.0));
        assert_eq!(p.loops[..2], [1.0, 2.0]);
        // Half way through the fade (0.09 clip seconds = 0.06 s of game time).
        a.update(&m, 0.06);
        let p = a.poses(&m);
        assert!((p.poses[0].weight - 0.5).abs() < 1e-3, "{p:?}");
        a.update(&m, 0.1);
        assert_eq!(a.poses(&m).count, 1);
        // Run and charge share a clip: switching keeps the phase.
        a.set_state(&m, "run", false, FADE);
        a.update(&m, 0.2);
        let phase = a.current().1;
        a.set_state(&m, "charge", false, FADE);
        assert_eq!(a.current(), (2, phase));
        assert_eq!(a.state(), "charge");
    }

    #[test]
    fn one_shots_return_to_idle_but_death_stays() {
        let m = model();
        let mut a = Animator::new(&m).unwrap();
        a.set_state(&m, "hit", true, FADE);
        for _ in 0..20 {
            a.update(&m, TICK_SECONDS);
        }
        assert_eq!(a.state(), "idle");
        a.set_state(&m, "death", true, FADE);
        for _ in 0..200 {
            a.update(&m, TICK_SECONDS);
        }
        assert_eq!(a.state(), "death");
        assert_eq!(a.current(), (5, 1.5));
    }

    #[test]
    fn scripted_animations_play_once_or_hold_and_yield_to_walking() {
        let m = model();
        let mut u = UnitAnimation::new(&m).unwrap();
        let at = |tick, scripted| UnitInputs {
            tick,
            scripted,
            ..Default::default()
        };
        step(&mut u, &m, at(0, None));
        step(&mut u, &m, at(1, Some(("spell", 1, false))));
        assert_eq!(u.animator.state(), "cast", "spell plays as cast");
        for tick in 2..80 {
            step(&mut u, &m, at(tick, Some(("spell", 1, false))));
        }
        assert_eq!(u.animator.state(), "idle", "once, then back to idle");
        step(&mut u, &m, at(80, Some(("attack", 80, true))));
        for tick in 81..200 {
            step(&mut u, &m, at(tick, Some(("attack", 80, true))));
        }
        assert_eq!(u.animator.state(), "attack", "held");
        let mut walking = at(200, Some(("attack", 80, true)));
        walking.moving = true;
        step(&mut u, &m, walking);
        assert_eq!(u.animator.state(), "run");
    }

    #[test]
    fn attacks_lock_contact_to_the_impact_tick() {
        assert_eq!(attack_phase(10.0, 10, 20, 40, 0.4), 0.0);
        assert!((attack_phase(15.0, 10, 20, 40, 0.4) - 0.2).abs() < 1e-6);
        assert!((attack_phase(20.0, 10, 20, 40, 0.4) - 0.4).abs() < 1e-6);
        assert!((attack_phase(30.0, 10, 20, 40, 0.4) - 0.7).abs() < 1e-6);
        assert_eq!(attack_phase(99.0, 10, 20, 40, 0.4), 1.0);
        let m = model();
        let mut u = UnitAnimation::new(&m).unwrap();
        step(
            &mut u,
            &m,
            UnitInputs {
                tick: 9,
                ..Default::default()
            },
        );
        for tick in 10..=20 {
            step(
                &mut u,
                &m,
                UnitInputs {
                    tick,
                    attack: Some((10, 20, 40)),
                    ..Default::default()
                },
            );
        }
        assert_eq!(u.animator.state(), "attack");
        // At the impact tick the clip stands exactly on its authored contact.
        assert!((u.animator.phase(&m) - 0.4).abs() < 1e-4);
        // The swing ends: back to idle.
        step(
            &mut u,
            &m,
            UnitInputs {
                tick: 41,
                ..Default::default()
            },
        );
        assert_eq!(u.animator.state(), "idle");
    }

    #[test]
    fn movement_carry_hits_and_work_choose_their_states() {
        let m = model();
        let mut u = UnitAnimation::new(&m).unwrap();
        let moving = UnitInputs {
            tick: 1,
            moving: true,
            ..Default::default()
        };
        step(&mut u, &m, moving.clone());
        assert_eq!(u.animator.state(), "run");
        step(
            &mut u,
            &m,
            UnitInputs {
                strolling: true,
                ..moving.clone()
            },
        );
        assert_eq!(u.animator.state(), "walk");
        step(
            &mut u,
            &m,
            UnitInputs {
                carrying: true,
                ..moving.clone()
            },
        );
        assert_eq!(u.animator.state(), "carry_run");
        // Carrying while strolling has no carry_walk here: plain walk.
        step(
            &mut u,
            &m,
            UnitInputs {
                carrying: true,
                strolling: true,
                ..moving
            },
        );
        assert_eq!(u.animator.state(), "walk");
        // Standing hurt plays the hit reaction and records the recoil.
        step(
            &mut u,
            &m,
            UnitInputs {
                tick: 5,
                hp: Some(10),
                ..Default::default()
            },
        );
        step(
            &mut u,
            &m,
            UnitInputs {
                tick: 6,
                hp: Some(8),
                ..Default::default()
            },
        );
        assert_eq!(u.animator.state(), "hit");
        assert!(u.recoil(8) > 0.5);
        assert_eq!(u.recoil(20), 0.0);
        // A work stroke follows the authoritative cycle, and faces the work.
        let mut w = UnitAnimation::new(&m).unwrap();
        let d = step(
            &mut w,
            &m,
            UnitInputs {
                tick: 1,
                work: Some(("chop", Some((40, 10)))),
                works: true,
                ..Default::default()
            },
        );
        assert!(d.face_work);
        step(
            &mut w,
            &m,
            UnitInputs {
                tick: 2,
                work: Some(("chop", Some((40, 20)))),
                works: true,
                ..Default::default()
            },
        );
        assert_eq!(w.animator.state(), "chop");
        assert!((w.animator.phase(&m) - 0.5).abs() < 1e-4);
        // Variants without work poses stay idle.
        let mut s = UnitAnimation::new(&m).unwrap();
        step(
            &mut s,
            &m,
            UnitInputs {
                tick: 1,
                work: Some(("chop", None)),
                works: false,
                ..Default::default()
            },
        );
        assert_eq!(s.animator.state(), "idle");
    }

    #[test]
    fn casts_follow_their_timeline() {
        let animations = Animations {
            prepare: "cast".into(),
            release: "cast".into(),
            recover: "cast".into(),
            fallback: "idle".into(),
            channel: None,
        };
        let timeline = CastTimeline {
            start: 0,
            release: 10,
            finish: 30,
            channel_end: None,
        };
        let has = |s: &str| s == "cast" || s == "idle";
        let contact = |_: &str| 0.5;
        // Continuous clip: prepare maps onto [0, contact], recovery onto [contact, 1].
        assert_eq!(
            cast_animation(&animations, timeline, 5.0, has, contact),
            ("cast".into(), 0.25)
        );
        assert_eq!(
            cast_animation(&animations, timeline, 20.0, has, contact),
            ("cast".into(), 0.75)
        );
        // A missing clip falls back.
        let missing = Animations {
            prepare: "summon".into(),
            ..animations.clone()
        };
        assert_eq!(
            cast_animation(&missing, timeline, 2.0, has, contact).0,
            "idle"
        );
        // Channels loop every 80 ticks between release and the channel's end.
        let channel = CastTimeline {
            channel_end: Some(100),
            ..timeline
        };
        assert_eq!(
            cast_animation(&animations, channel, 50.0, has, contact).1,
            0.5
        );
        let m = model();
        let mut u = UnitAnimation::new(&m).unwrap();
        for tick in 0..=10 {
            step(
                &mut u,
                &m,
                UnitInputs {
                    tick,
                    cast: Some((3, &animations, timeline)),
                    ..Default::default()
                },
            );
        }
        assert_eq!(u.animator.state(), "cast");
        assert!((u.animator.phase(&m) - 0.5).abs() < 1e-4);
        // After the cast the unit returns to idle with a short fade.
        step(
            &mut u,
            &m,
            UnitInputs {
                tick: 31,
                ..Default::default()
            },
        );
        assert_eq!(u.animator.state(), "idle");
    }

    /// A forge-like building: a rest pose, a work loop striking twice a cycle, a spawn.
    fn building() -> ModelInfo {
        let state = |clip: u16, looping: bool, events: &[(&str, f32)]| StateInfo {
            clip,
            looping,
            contact: None,
            events: events.iter().map(|(n, t)| ((*n).to_owned(), *t)).collect(),
            nominal_speed: None,
        };
        ModelInfo {
            name: "building".into(),
            clips: vec![clip("idle", 0.1), clip("work", 1.5), clip("spawn", 1.0)],
            states: [
                ("idle", state(0, true, &[])),
                ("work", state(1, true, &[("strike", 0.3), ("strike", 0.8)])),
                ("spawn", state(2, false, &[])),
            ]
            .into_iter()
            .map(|(n, s)| (n.to_owned(), s))
            .collect(),
            ..ModelInfo::default()
        }
    }

    #[test]
    fn buildings_rest_until_busy_and_strike_on_their_events() {
        let m = building();
        let mut b = BuildingAnimation::new(&m).unwrap();
        for _ in 0..40 {
            assert!(b.step(&m, false, 0, false).is_empty());
        }
        assert_eq!(b.animator.state(), "idle");
        // Two strikes per 1.5 s clip at 1.5× speed: about two strikes a second.
        let strikes: usize = (0..80).map(|_| b.step(&m, true, 0, false).len()).sum();
        assert_eq!(b.animator.state(), "work");
        assert!(
            (3..=5).contains(&strikes),
            "{strikes} strikes in two seconds"
        );
        // A finished cycle plays the spawn once, then work resumes.
        b.step(&m, true, 1, false);
        assert_eq!(b.animator.state(), "spawn");
        for _ in 0..40 {
            b.step(&m, true, 1, false);
        }
        assert_eq!(b.animator.state(), "work");
        b.step(&m, false, 1, false);
        assert_eq!(b.animator.state(), "idle");
    }

    #[test]
    fn towers_fire_once_per_shot_of_their_crew() {
        let mut m = building();
        m.clips.push(clip("attack", 0.8));
        m.states.insert(
            "attack".into(),
            StateInfo {
                clip: 3,
                looping: false,
                contact: None,
                events: vec![("fire".into(), 0.1)],
                nominal_speed: None,
            },
        );
        let mut b = BuildingAnimation::new(&m).unwrap();
        b.step(&m, false, 0, false);
        // Each shot restarts the clip and crosses `fire` once; between shots it rests.
        let mut fired = 0;
        for tick in 0..60 {
            let events = b.step(&m, false, 0, tick % 20 == 0);
            fired += events.iter().filter(|e| *e == "fire").count();
        }
        assert_eq!(fired, 3);
        for _ in 0..40 {
            b.step(&m, false, 0, false);
        }
        assert_eq!(b.animator.state(), "idle");
    }

    /// A walker: a 1 s walk loop stepping at a quarter and three quarters, its stride
    /// authored at 2 m/s, and an attack releasing at 0.4.
    fn walker(nominal: Option<f32>) -> ModelInfo {
        let mut m = model();
        let walk = m.states.get_mut("walk").unwrap();
        walk.events = vec![("step".into(), 0.25), ("step".into(), 0.75)];
        walk.nominal_speed = nominal;
        m.states.get_mut("attack").unwrap().events = vec![("release".into(), 0.4)];
        m
    }

    #[test]
    fn gaits_follow_the_ground_speed_when_their_stride_is_authored() {
        let walked = |m: &ModelInfo, speed: Option<f32>| {
            let mut u = UnitAnimation::new(m).unwrap();
            let mut steps = 0;
            for tick in 0..=40 {
                steps += u
                    .step(
                        m,
                        &UnitInputs {
                            tick,
                            moving: true,
                            strolling: true,
                            speed,
                            ..Default::default()
                        },
                    )
                    .events
                    .len();
            }
            steps
        };
        // One second at 1.5× crosses three steps whatever the speed, without a stride.
        assert_eq!(walked(&walker(None), Some(6.0)), 3);
        assert_eq!(walked(&walker(Some(2.0)), None), 3);
        // At 4 m/s over a 2 m/s stride: two loops, four steps; at 1 m/s: half a loop.
        assert_eq!(walked(&walker(Some(2.0)), Some(4.0)), 4);
        assert_eq!(walked(&walker(Some(2.0)), Some(1.0)), 1);
        // Far off the stride the rate is clamped, not frozen or frantic.
        assert_eq!(
            stride_rate(&walker(Some(2.0)), "walk", Some(0.0)),
            STRIDE_RATE.0
        );
        assert_eq!(
            stride_rate(&walker(Some(2.0)), "walk", Some(100.0)),
            STRIDE_RATE.1
        );
        // Only gaits follow the ground.
        assert_eq!(stride_rate(&walker(Some(2.0)), "idle", Some(4.0)), SPEED);
    }

    #[test]
    fn a_swing_reports_its_release_once_at_the_impact_tick() {
        let m = walker(None);
        let mut u = UnitAnimation::new(&m).unwrap();
        let mut released = Vec::new();
        for tick in 0..=45 {
            let attack = (10..40).contains(&tick).then_some((10, 20, 40));
            let d = u.step(
                &m,
                &UnitInputs {
                    tick,
                    attack,
                    ..Default::default()
                },
            );
            if d.events.iter().any(|e| e == "release") {
                released.push(tick);
            }
        }
        assert_eq!(released, [20]);
        // A second swing releases again.
        for tick in 46..=60 {
            let d = u.step(
                &m,
                &UnitInputs {
                    tick,
                    attack: Some((46, 50, 60)),
                    ..Default::default()
                },
            );
            if d.events.iter().any(|e| e == "release") {
                released.push(tick);
            }
        }
        assert_eq!(released, [20, 50]);
    }
}
