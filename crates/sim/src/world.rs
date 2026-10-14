//! The world: state, content, map and the command queue, advanced one tick at a time.

use crate::{
    action::Action,
    catalog::Catalog,
    fingerprint, mission,
    movement::Motion,
    orders,
    setup::MatchSetup,
    spawn,
    state::{FactKind, State},
    terrain::Terrain,
    view::View,
    vision::{self, Sight, Vision},
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
    /// Compiled once from the map; immutable and shared with every system.
    terrain: Arc<Terrain>,
    setup: MatchSetup,
    state: State,
    pending: Vec<Queued>,
    mission: Option<mission::Runtime>,
    /// Derived sight coverage; rebuilt on restore.
    vision: Vision,
    /// Derived spatial model and movement scratch; rebuilt on restore, never saved.
    motion: Motion,
}

impl World {
    /// Starts a match on `map`.
    pub fn new(content: Arc<Content>, map: &UtcMap, setup: MatchSetup) -> Result<Self, SetupError> {
        let catalog = Arc::new(Catalog::new(content));
        let terrain = Arc::new(compile(map, &catalog)?);
        let mut state = State::new(setup.seed);
        let mission = mission_runtime(map)?;
        if mission.is_some() {
            state.mission = Some(mission::MissionState::default());
        }
        let placements = spawn::placements(map, &catalog, &terrain, &setup).map_err(SetupError)?;
        spawn::validate(&catalog, &terrain, &setup, &placements).map_err(SetupError)?;
        for placement in placements.iter().filter(|p| p.activation.is_none()) {
            spawn::create(&mut state, &catalog, placement).map_err(SetupError)?;
        }
        for slot in &setup.slots {
            state.wallets.entry(Owner::Player(slot.player)).or_default();
        }
        let vision = Vision::new(
            &mut state,
            &catalog,
            &terrain,
            &setup,
            map.exploration(),
            &SIGHT,
        );
        let motion = Motion::new(
            terrain.clone(),
            &catalog,
            &state,
            crate::teams::Teams::new(&setup),
            appearance_scales(map, &terrain),
        );
        Ok(Self {
            catalog,
            terrain,
            setup,
            state,
            pending: Vec::new(),
            mission,
            vision,
            motion,
        })
    }

    /// The lockstep clock commands are scheduled on. It runs on while cinematic dialogue
    /// pauses the game, so game time (`state().tick`) may lag behind it.
    pub fn tick(&self) -> u32 {
        mission::clock(&self.state)
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The match terrain, shared read-only.
    pub fn terrain(&self) -> &Arc<Terrain> {
        &self.terrain
    }

    pub fn setup(&self) -> &MatchSetup {
        &self.setup
    }

    /// The spatial model: occupancy, bodies, navigation and route queries.
    pub fn spatial(&self) -> &crate::spatial::Spatial {
        &self.motion.spatial
    }

    /// The spatial model for path queries, which only warm its caches.
    pub fn spatial_mut(&mut self) -> &mut crate::spatial::Spatial {
        &mut self.motion.spatial
    }

    /// The campaign mission, on mission maps.
    pub fn mission(&self) -> Option<&mission::Runtime> {
        self.mission.as_ref()
    }

    /// Schedules a command. Commands for past ticks and malformed ones are refused here;
    /// game rules are checked when the command is applied.
    pub fn enqueue(&mut self, command: Queued) -> Result<(), Rejected> {
        if command.tick < self.tick() {
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
        // Step 2: cinematic dialogue pauses the game; only the clock advances.
        if mission::pause(&mut self.state) {
            self.observe();
            return;
        }
        // Steps 4–5: order advance, then order planning, separation, idle motion and
        // movement.
        self.motion.spatial.sync_static(&self.state);
        orders::plan::advance(&mut self.motion, &mut self.state);
        let perception = orders::plan::Perception {
            vision: &self.vision,
            catalog: &self.catalog,
            terrain: &self.terrain,
            rules: &SIGHT,
        };
        orders::plan::plan(&mut self.motion, &mut self.state, perception);
        self.motion.separate(&mut self.state);
        self.motion.idle(&mut self.state);
        self.motion.move_units(&mut self.state);
        // Step 10: the mission script.
        if let Some(mission) = &mut self.mission {
            mission.tick(&mut self.state, &self.catalog);
            self.motion.spatial.sync_static(&self.state);
        }
        // Step 11: fog and knowledge.
        self.observe();
        self.state.tick += 1;
    }

    fn observe(&mut self) {
        self.vision
            .update(&mut self.state, &self.catalog, &self.terrain, &SIGHT);
    }

    fn apply_due(&mut self) {
        let tick = self.tick();
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

    fn apply(&mut self, owner: Owner, action: &Action) -> Result<(), String> {
        if let Some(reason) = mission::blocks_commands(&self.state) {
            return Err(reason.into());
        }
        match action {
            Action::Noop | Action::Ping => Ok(()),
            _ if orders::command::handles(action) => {
                self.motion.spatial.sync_static(&self.state);
                orders::command::apply(&mut self.state, &mut self.motion.spatial, owner, action)
                    .map(|_| ())
            }
            _ => Err("not available yet".into()),
        }
    }

    /// The desync fingerprint of the current tick (docs/decisions.md D13).
    pub fn fingerprint(&self) -> u32 {
        fingerprint::fingerprint(&self.state)
    }

    /// What `owner` sees.
    pub fn view(&self, owner: Owner) -> View {
        View::build(
            &self.state,
            &self.catalog,
            &self.terrain,
            &self.vision,
            &SIGHT,
            owner,
        )
    }

    /// Everything, for spectators.
    pub fn observer_view(&self) -> View {
        View::observe(&self.state, &self.vision)
    }

    /// Sight questions (what an owner sees, knows or detects) without building a view:
    /// the AI's and the command checks' way in.
    pub fn sight(&self) -> Sight<'_> {
        Sight {
            vision: &self.vision,
            state: &self.state,
            catalog: &self.catalog,
            terrain: &self.terrain,
            rules: &SIGHT,
        }
    }

    pub fn vision(&self) -> &Vision {
        &self.vision
    }

    /// Direct access for tests and tools. Derived data (vision) follows the changes at the
    /// next step.
    pub fn state_mut(&mut self) -> &mut State {
        &mut self.state
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

    /// Resumes a saved match on the same content and map. The terrain is rebuilt from the
    /// map, never saved.
    pub fn restore(content: Arc<Content>, map: &UtcMap, save: Save) -> Result<Self, SetupError> {
        if save.version != SAVE_VERSION {
            return Err(SetupError(format!(
                "unsupported save version {}",
                save.version
            )));
        }
        let catalog = Arc::new(Catalog::new(content));
        let terrain = Arc::new(compile(map, &catalog)?);
        let mission = mission_runtime(map)?;
        if mission.is_some() != save.state.mission.is_some() {
            return Err(SetupError(
                "the save does not match the map's mission".into(),
            ));
        }
        let mut state = save.state;
        let vision = Vision::restore(&mut state, &catalog, &terrain, &save.setup, &SIGHT)
            .map_err(SetupError)?;
        let motion = Motion::new(
            terrain.clone(),
            &catalog,
            &state,
            crate::teams::Teams::new(&save.setup),
            appearance_scales(map, &terrain),
        );
        Ok(Self {
            catalog,
            terrain,
            setup: save.setup,
            state,
            pending: save.pending,
            mission,
            vision,
            motion,
        })
    }
}

fn compile(map: &UtcMap, catalog: &Catalog) -> Result<Terrain, SetupError> {
    Terrain::compile(map, &catalog.content).map_err(|e| SetupError(e.to_string()))
}

/// Appearance scales of map and scenery placements: resource collision discs scale with
/// them.
fn appearance_scales(map: &UtcMap, terrain: &Terrain) -> std::collections::BTreeMap<String, f64> {
    map.entities
        .iter()
        .chain(&terrain.scene().resources)
        .filter_map(|p| Some((p.id.clone(), p.appearance.as_ref()?.scale?)))
        .collect()
}

fn mission_runtime(map: &UtcMap) -> Result<Option<mission::Runtime>, SetupError> {
    let script = mission::Script::new(map).map_err(SetupError)?;
    Ok(script.map(|s| mission::Runtime::new(Arc::new(s))))
}

/// The sight rules of the systems built so far.
const SIGHT: vision::Current = vision::Current;

pub const SAVE_VERSION: u32 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Save {
    pub version: u32,
    pub setup: MatchSetup,
    pub state: State,
    pub pending: Vec<Queued>,
}
