//! Campaign missions: the map's Lua script and the `mission` API (docs/simulation.md "Lua").
//!
//! `on_start` runs once, then `on_tick` every [`RUN_EVERY`] ticks, each run in a fresh
//! sandbox. A run is a transaction: API calls change a draft of the mission state and queue
//! [`MissionEffect`]s; only when the whole run succeeds is the draft committed and the
//! effects applied. The first failure stops the script for good. A cinematic line of
//! dialogue pauses the game while the lockstep clock keeps running.

mod api;
mod effect;
mod hook;
pub mod sandbox;
mod script;
pub mod state;
pub mod systems;

pub use api::{CALL_LIMIT, MAX_DAMAGE, NAMES};
pub use effect::{CAMPAIGN_PLAYER, MissionEffect};
pub use script::Script;
pub use state::MissionState;
pub use systems::{Current, Systems};

use crate::{
    catalog::Catalog,
    entity::Entity,
    state::{FactKind, State},
};
use std::sync::Arc;

/// Simulation ticks per second (docs/decisions.md D3).
pub const TICKS_PER_SECOND: u32 = 40;
/// `on_tick` runs on game ticks divisible by this.
pub const RUN_EVERY: u32 = 4;

/// A match's mission: the compiled script and the effects of its latest run (for tests and
/// debugging; not saved).
#[derive(Debug)]
pub struct Runtime {
    pub script: Arc<Script>,
    effects: Vec<MissionEffect>,
}

impl Runtime {
    pub fn new(script: Arc<Script>) -> Self {
        Self {
            script,
            effects: Vec::new(),
        }
    }

    /// Effects applied by the most recent successful run.
    pub fn effects(&self) -> &[MissionEffect] {
        &self.effects
    }

    /// Step 10 of the tick: turn scripted units, then run the script when due.
    pub fn tick(&mut self, state: &mut State, catalog: &Catalog) {
        if let Some(effects) = tick(state, catalog, &self.script, &Current) {
            self.effects = effects;
        }
    }
}

/// The lockstep clock: game ticks plus ticks paused for cinematic dialogue.
pub fn clock(state: &State) -> u32 {
    state.tick + state.mission.as_ref().map_or(0, |m| m.paused_ticks)
}

/// Step 2 of the tick: while a cinematic line plays, the game stands still. Returns true
/// when this tick is such a pause; the line's remaining time runs down.
pub fn pause(state: &mut State) -> bool {
    if state.outcome.is_some() {
        return false;
    }
    let tick = state.tick;
    let Some(mission) = state.mission.as_mut() else {
        return false;
    };
    let Some(dialogue) = mission
        .dialogue
        .as_mut()
        .filter(|d| d.cinematic && d.remaining > 0)
    else {
        return false;
    };
    dialogue.remaining -= 1;
    if dialogue.remaining == 0 {
        dialogue.until = tick;
    }
    mission.paused_ticks += 1;
    true
}

/// Why player commands are refused right now: a scripted scene or cinematic line.
pub fn blocks_commands(state: &State) -> Option<&'static str> {
    let mission = state.mission.as_ref()?;
    let cinematic = mission.dialogue.as_ref().is_some_and(|d| d.remaining > 0);
    (mission.scene.is_some() || cinematic).then_some("Cinematic dialogue is playing")
}

/// Advances the mission by one game tick. Returns the applied effects when the script ran
/// successfully.
pub fn tick(
    state: &mut State,
    catalog: &Catalog,
    script: &Script,
    systems: &dyn Systems,
) -> Option<Vec<MissionEffect>> {
    let mission = state.mission.as_ref()?;
    if mission.error.is_some() || state.outcome.is_some() {
        return None;
    }
    advance_facing(state, catalog, systems);
    let mission = state.mission.as_ref()?;
    if mission.started && !state.tick.is_multiple_of(RUN_EVERY) {
        return None;
    }
    let callback = if mission.started {
        "on_tick"
    } else {
        "on_start"
    };
    let mut api = api::Api::new(state, catalog, script, systems, mission.clone());
    let result = sandbox::run(
        &script.mission.script,
        callback,
        &NAMES,
        &mut |name, args| api.call(name, args),
    )
    .and_then(|_| api.finish());
    match result {
        Ok((draft, effects)) => {
            for effect in &effects {
                effect::apply(effect, state, catalog, script, systems);
            }
            // M2.2: rebuild spatial occupancy when effects moved, created or changed units.
            state.mission = Some(draft);
            Some(effects)
        }
        Err(message) => {
            let mission = state.mission.as_mut()?;
            mission.scene = None;
            mission.facing.clear();
            mission.error = Some(message.clone());
            state.fact(
                CAMPAIGN_PLAYER,
                FactKind::Error,
                format!("Mission script stopped: {message}"),
            );
            None
        }
    }
}

/// Turns units toward their scripted facing; a unit leaves the list once it faces its
/// target, dies, or receives an order.
fn advance_facing(state: &mut State, catalog: &Catalog, systems: &dyn Systems) {
    let Some(mission) = state.mission.as_mut() else {
        return;
    };
    let mut facing = std::mem::take(&mut mission.facing);
    facing.retain(|target| {
        let Some(entity) = entity_by_placement_mut(state, &target.placement) else {
            return false;
        };
        if entity.unit.is_none() || !alive(entity) || systems.has_order(entity) {
            return false;
        }
        let rate = catalog
            .get(entity.definition)
            .behaviors
            .movement
            .as_ref()
            .map_or(systems::DEFAULT_TURN_RATE, |m| m.turn_rate);
        !systems::turn_toward(entity, target.target, rate)
    });
    if let Some(mission) = state.mission.as_mut() {
        mission.facing = facing;
    }
}

fn alive(entity: &Entity) -> bool {
    entity.hp.is_none_or(|hp| hp > 0)
}

/// The entity created from a placement (the first, in id order).
fn entity_by_placement<'a>(state: &'a State, placement: &str) -> Option<&'a Entity> {
    state
        .entities
        .iter()
        .find(|e| e.placement.as_deref() == Some(placement))
}

fn entity_by_placement_mut<'a>(state: &'a mut State, placement: &str) -> Option<&'a mut Entity> {
    state
        .entities
        .iter_mut()
        .find(|e| e.placement.as_deref() == Some(placement))
}
