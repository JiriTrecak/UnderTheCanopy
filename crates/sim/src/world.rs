//! The world: state, content, map and the command queue, advanced one tick at a time.

use crate::{
    action::Action,
    catalog::Catalog,
    fingerprint,
    setup::MatchSetup,
    spawn,
    state::{FactKind, State},
    view::View,
};
use canopy_content::{Content, map::UtcMap, placement::Owner};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

/// A command scheduled for a tick by a player. Sequence numbers order a player's commands
/// within one tick.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Queued {
    pub tick: u32,
    /// Zero-based player slot.
    pub player: u8,
    pub seq: u32,
    pub action: Action,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Rejected {
    #[error("tick {0} has already been simulated")]
    Late(u32),
    #[error("player {0} is not in this match")]
    UnknownPlayer(u8),
    #[error("malformed command: {0}")]
    Malformed(&'static str),
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("cannot start the match: {0}")]
pub struct SetupError(pub String);

pub struct World {
    catalog: Arc<Catalog>,
    setup: MatchSetup,
    state: State,
    pending: Vec<Queued>,
}

impl World {
    /// Starts a match on `map`.
    pub fn new(content: Arc<Content>, map: &UtcMap, setup: MatchSetup) -> Result<Self, SetupError> {
        let catalog = Arc::new(Catalog::new(content));
        let mut state = State::new(setup.seed);
        let placements = spawn::placements(map, &catalog, &setup).map_err(SetupError)?;
        for placement in placements.iter().filter(|p| p.activation.is_none()) {
            spawn::create(&mut state, &catalog, placement).map_err(SetupError)?;
        }
        for slot in &setup.slots {
            state.wallets.entry(Owner::Player(slot.player)).or_default();
        }
        Ok(Self {
            catalog,
            setup,
            state,
            pending: Vec::new(),
        })
    }

    pub fn tick(&self) -> u32 {
        self.state.tick
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    pub fn setup(&self) -> &MatchSetup {
        &self.setup
    }

    /// Schedules a command. Commands for past ticks and malformed ones are refused here;
    /// game rules are checked when the command is applied.
    pub fn enqueue(&mut self, command: Queued) -> Result<(), Rejected> {
        if command.tick < self.state.tick {
            return Err(Rejected::Late(command.tick));
        }
        if !self.setup.slots.iter().any(|s| s.player == command.player) {
            return Err(Rejected::UnknownPlayer(command.player));
        }
        if let Some(problem) = command.action.problem() {
            return Err(Rejected::Malformed(problem));
        }
        self.pending.push(command);
        Ok(())
    }

    /// Simulates one tick.
    pub fn step(&mut self) {
        self.apply_due();
        self.state.tick += 1;
    }

    fn apply_due(&mut self) {
        let tick = self.state.tick;
        let (mut due, keep): (Vec<_>, Vec<_>) = std::mem::take(&mut self.pending)
            .into_iter()
            .partition(|c| c.tick <= tick);
        self.pending = keep;
        due.sort_by_key(|c| (c.tick, c.player, c.seq));
        for command in due {
            let owner = Owner::Player(command.player);
            if let Err(reason) = self.apply(owner, &command.action) {
                self.state.fact(owner, FactKind::Error, reason);
            }
        }
    }

    fn apply(&mut self, _owner: Owner, action: &Action) -> Result<(), String> {
        match action {
            Action::Noop | Action::Ping => Ok(()),
            _ => Err("not available yet".into()),
        }
    }

    /// The desync fingerprint of the current tick (docs/decisions.md D13).
    pub fn fingerprint(&self) -> u32 {
        fingerprint::fingerprint(&self.state)
    }

    /// What `owner` sees.
    pub fn view(&self, owner: Owner) -> View {
        View::build(&self.state, &self.catalog, owner)
    }

    /// A save: setup, state and pending commands. The content and map are referenced by the
    /// caller, not embedded.
    pub fn save(&self) -> Save {
        Save {
            version: SAVE_VERSION,
            setup: self.setup.clone(),
            state: self.state.clone(),
            pending: self.pending.clone(),
        }
    }

    /// Resumes a saved match on the same content and map.
    pub fn restore(content: Arc<Content>, save: Save) -> Result<Self, SetupError> {
        if save.version != SAVE_VERSION {
            return Err(SetupError(format!(
                "unsupported save version {}",
                save.version
            )));
        }
        Ok(Self {
            catalog: Arc::new(Catalog::new(content)),
            setup: save.setup,
            state: save.state,
            pending: save.pending,
        })
    }
}

pub const SAVE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Save {
    pub version: u32,
    pub setup: MatchSetup,
    pub state: State,
    pub pending: Vec<Queued>,
}
