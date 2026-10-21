//! Recording a match as it plays (D60): the simulation thread hands the recorder every
//! turn the session gave the world, right where the world executes it, so the record is
//! exactly what was simulated. Computer players are not recorded: the simulation decides
//! for them again on playback. Chat is not part of the simulation and is not recorded.
//!
//! The recording lives in memory behind a mutex the simulation thread holds for a push
//! per tick; the window thread copies it out to save a replay (the result screen, the
//! last match, a feedback report).

use super::format::FINGERPRINT_EVERY;
use canopy_sim::{MatchSetup, Queued, World, state::Outcome};
use std::sync::{Arc, Mutex};

/// A match recorded so far.
#[derive(Debug, Clone, PartialEq)]
pub struct Recording {
    pub setup: MatchSetup,
    /// The world the recording starts from, when it is not a fresh start (a loaded save).
    pub start: Option<canopy_sim::Save>,
    pub start_tick: u32,
    pub commands: Vec<Queued>,
    pub fingerprints: Vec<(u32, u32)>,
    /// The world's clock after the latest recorded step.
    pub tick: u32,
    pub outcome: Option<Outcome>,
}

/// The shared recording: cheap to clone, one per match.
#[derive(Debug, Clone)]
pub struct Recorder(Arc<Mutex<Recording>>);

impl Recorder {
    /// Starts recording `world` as it stands (before its first step here).
    pub fn new(world: &World) -> Self {
        let tick = world.tick();
        Self(Arc::new(Mutex::new(Recording {
            setup: world.setup().clone(),
            // A campaign mission starts from what its campaign handed it (tech, carried
            // heroes), which only a save of the world holds.
            start: (tick > 0 || !world.pending().is_empty() || world.state().campaign.is_some())
                .then(|| world.save()),
            start_tick: tick,
            commands: Vec::new(),
            fingerprints: Vec::new(),
            tick,
            outcome: None,
        })))
    }

    /// One step: the commands the session handed the world for it, then the world after
    /// the step.
    pub fn stepped(&self, commands: &[Queued], world: &World) {
        let tick = world.tick();
        let fingerprint = tick
            .is_multiple_of(FINGERPRINT_EVERY)
            .then(|| world.fingerprint());
        let mut r = self.0.lock().unwrap_or_else(|e| e.into_inner());
        r.commands.extend_from_slice(commands);
        r.tick = tick;
        if let Some(f) = fingerprint {
            r.fingerprints.push((tick, f));
        }
        if r.outcome.is_none() {
            r.outcome.clone_from(&world.state().outcome);
        }
    }

    /// A copy of the recording so far.
    pub fn snapshot(&self) -> Recording {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}
