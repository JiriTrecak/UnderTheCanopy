//! The world: state, content, map and the command queue, advanced one tick at a time.

use crate::abilities::{self, sight::SpellSight};
use crate::{
    action::Action,
    ai,
    catalog::Catalog,
    combat,
    economy::{self, Economy},
    entity::EntityId,
    fingerprint, mission,
    movement::Motion,
    orders,
    setup::MatchSetup,
    spawn,
    state::{FactKind, State},
    terrain::Terrain,
    victory,
    view::View,
    vision::{Sight, Vision},
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
    /// Derived combat rules, camps and planning scratch; rebuilt on restore.
    combat: combat::Runtime,
    /// Derived economy indexes and tables; rebuilt on restore, never saved.
    economy: Economy,
    /// How the match ends.
    rules: victory::Rules,
    /// What vision asks of abilities: forms, concealment, detection, lent sight.
    sight: SpellSight,
    /// Recent ability events for views (cosmetic, never saved).
    events: abilities::Events,
    /// The AI players' derived side; their memories are in the state.
    ai: ai::Brains,
    /// The mission's computer players (campaign AI), from the map; memories in the state.
    campaign: ai::campaign::Campaign,
}

impl World {
    /// Starts a match on `map`.
    pub fn new(content: Arc<Content>, map: &UtcMap, setup: MatchSetup) -> Result<Self, SetupError> {
        Self::start(content, map, setup, None)
    }

    /// Starts a mission of a campaign on `map`: the campaign's tech for its player, its
    /// carried heroes on the heroes the map places, the hero level cap and the campaign
    /// variables (docs/campaign.md).
    pub fn new_campaign(
        content: Arc<Content>,
        map: &UtcMap,
        setup: MatchSetup,
        campaign: &crate::campaign::CampaignStart,
    ) -> Result<Self, SetupError> {
        Self::start(content, map, setup, Some(campaign))
    }

    fn start(
        content: Arc<Content>,
        map: &UtcMap,
        setup: MatchSetup,
        start: Option<&crate::campaign::CampaignStart>,
    ) -> Result<Self, SetupError> {
        // A mission's computer players are the ones its map lists.
        let setup = ai::campaign::mission_setup(map, setup);
        let campaign = ai::campaign::Campaign::new(map);
        let catalog = Arc::new(Catalog::new(content));
        let terrain = Arc::new(compile(map, &catalog)?);
        let mut combat = combat::Runtime::new(&catalog, map, &setup).map_err(SetupError)?;
        let mut state = State::new(setup.seed);
        let mission = mission_runtime(map, &setup)?;
        if mission.is_some() {
            state.mission = Some(mission::MissionState::default());
        }
        let placements = spawn::placements(map, &catalog, &terrain, &setup).map_err(SetupError)?;
        spawn::validate(&catalog, &terrain, &setup, &placements.list).map_err(SetupError)?;
        let mut starting = std::collections::BTreeSet::new();
        for placement in placements.list.iter().filter(|p| p.activation.is_none()) {
            let id = spawn::create(&mut state, &catalog, placement).map_err(SetupError)?;
            crate::camps::enlist(&mut state, &combat.camps, id, None);
            if placements.starting_units.contains(&placement.id) {
                starting.insert(id);
            }
        }
        if let Some(start) = start {
            crate::campaign::begin(&mut state, &catalog, start);
            // A computer player of the campaign's race playing the held-back skirmish AI
            // keeps to the mission's tech too (docs/campaign.md "Campaign AI").
            let race = |player: u8| {
                setup.slots.iter().find(|s| s.player == player).map(|s| {
                    s.race
                        .clone()
                        .unwrap_or_else(|| catalog.content.rules.default_race.clone())
                })
            };
            let skirmish = map.mission.iter().flat_map(|m| &m.players).filter(|p| {
                p.ai.behaviour == canopy_content::map::Behaviour::Skirmish
                    && p.player > 1
                    && race(p.player - 1) == race(start.player)
            });
            for p in skirmish {
                crate::campaign::lock(
                    &mut state,
                    Owner::Player(p.player - 1),
                    start.locked.iter().cloned(),
                );
            }
        }
        combat.limits.level_cap = level_cap(&state);
        let rules = match_rules(map);
        if rules == victory::Rules::Skirmish {
            for start in &map.player_starts {
                let seated = setup
                    .slots
                    .iter()
                    .any(|s| Owner::Player(s.player) == start.owner());
                if !seated {
                    continue;
                }
                let fort = state
                    .entities
                    .iter()
                    .find(|e| e.placement.as_deref() == Some(start.main_fort.as_str()))
                    .ok_or_else(|| SetupError("Missing main fort".into()))?;
                state.objectives.insert(start.owner(), fort.id.0);
            }
        }
        for slot in &setup.slots {
            state.wallets.entry(Owner::Player(slot.player)).or_default();
        }
        let sight = SpellSight {
            catalog: catalog.clone(),
        };
        state.campaign_ai = campaign.memories();
        let vision = Vision::new(
            &mut state,
            &catalog,
            &terrain,
            &setup,
            &campaign.private_sight(),
            map.exploration(),
            &sight,
        );
        let mut motion = Motion::new(
            terrain.clone(),
            &catalog,
            &state,
            crate::teams::Teams::new(&setup),
            appearance_scales(map, &terrain),
        );
        let mut economy = Economy::new(&state, &catalog, &setup);
        // Starting workers go to work wherever starts give bases (skirmish and missions).
        if rules != victory::Rules::Sandbox && !starting.is_empty() {
            let mut ctx = economy.ctx(&mut state, &catalog, &mut motion.spatial);
            economy::harvest::start_gathering(&mut ctx, &setup, &starting);
        }
        let mut brains = ai::Brains::new(
            &catalog,
            map,
            terrain.clone(),
            &setup,
            appearance_scales(map, &terrain),
        );
        state.ai = brains.memories(setup.seed);
        hold_back(&campaign, &catalog, &mut brains, &mut economy);
        let mut world = Self {
            catalog,
            terrain,
            setup,
            state,
            pending: Vec::new(),
            mission,
            vision,
            motion,
            combat,
            economy,
            rules,
            sight,
            events: abilities::Events::default(),
            ai: brains,
            campaign,
        };
        world.sync_ai_tech();
        world.prepare_ai();
        Ok(world)
    }

    /// Builds the AI players' known blockers and navigation from their knowledge.
    fn prepare_ai(&mut self) {
        if self.ai.is_empty() {
            return;
        }
        self.economy.index(&self.state, &self.catalog);
        let tick = mission::clock(&self.state);
        let spells = abilities::Ctx {
            senses: combat::Senses {
                catalog: &self.catalog,
                terrain: &self.terrain,
                vision: &self.vision,
                rules: &self.sight,
                combat: &self.combat.rules,
                camps: &self.combat.camps,
                teams: &self.combat.teams,
                cache: None,
            },
            spatial: &mut self.motion.spatial,
            events: &mut self.events,
            economy: &mut self.economy,
            limits: self.combat.limits,
            suppress: 0,
        };
        let input = ai::frame::Input {
            state: &self.state,
            catalog: &self.catalog,
            spells: &spells,
        };
        self.ai.prepare(&input, tick);
    }

    /// Step 12: the AI players due this tick decide; their commands are queued for the
    /// next tick on every peer.
    fn think(&mut self, clock: u32) {
        if self.ai.is_empty() {
            return;
        }
        let _span = tracing::trace_span!("ai").entered();
        self.economy.index(&self.state, &self.catalog);
        let idle = self.campaign.idle_brains(&self.state.campaign_ai);
        let mut memories = std::mem::take(&mut self.state.ai);
        let commands = {
            let spells = abilities::Ctx {
                senses: combat::Senses {
                    catalog: &self.catalog,
                    terrain: &self.terrain,
                    vision: &self.vision,
                    rules: &self.sight,
                    combat: &self.combat.rules,
                    camps: &self.combat.camps,
                    teams: &self.combat.teams,
                    cache: None,
                },
                spatial: &mut self.motion.spatial,
                events: &mut self.events,
                economy: &mut self.economy,
                limits: self.combat.limits,
                suppress: 0,
            };
            let input = ai::frame::Input {
                state: &self.state,
                catalog: &self.catalog,
                spells: &spells,
            };
            self.ai.decide(&input, &mut memories, clock, &idle)
        };
        self.state.ai = memories;
        for (player, command) in commands {
            let queued = Queued {
                tick: clock + 1,
                player,
                seq: command.seq,
                action: command.action,
            };
            // The AI only issues well-formed commands; a malformed one is dropped as the
            // old queue did, and its pending entry expires with the memory bound.
            if queued.action.problem().is_none() {
                self.pending.push(queued);
            }
        }
    }

    /// Plays AI `player` in `style` (tools and experiments, D75).
    pub fn set_ai_style(&mut self, player: u8, style: ai::AiStyle) {
        self.ai.restyle(&self.catalog, player, style);
    }

    /// The mission's tech limits for AI `player` (definitions and research it may not
    /// make): the campaign's tech gating sets it; derived, so set again after a restore.
    pub fn set_ai_tech(&mut self, player: u8, forbidden: Vec<canopy_content::Id>) {
        self.ai.forbid(player, forbidden);
    }

    /// The style each AI player plays, by player.
    pub fn ai_styles(&self) -> Vec<(u8, ai::AiStyle)> {
        self.ai.styles()
    }

    /// What each AI player is doing, for the debug overlay.
    pub fn ai_summaries(&self) -> Vec<ai::Summary> {
        ai::Brains::summaries(&self.state.ai)
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

    /// Limits what `player` may build, train and research to its race's tech without
    /// `locked` (a campaign's computer player held back to the mission's tech).
    pub fn limit_tech(&mut self, player: u8, locked: impl IntoIterator<Item = canopy_content::Id>) {
        crate::campaign::lock(&mut self.state, Owner::Player(player), locked);
        self.sync_ai_tech();
    }

    /// Tells the computer players what their tech limits keep from them (derived from the
    /// saved limits, so a restore tells them again).
    fn sync_ai_tech(&mut self) {
        for owner in self.ai_owners() {
            if let Owner::Player(player) = owner
                && let Some(locked) = self.state.tech.get(&owner)
            {
                self.ai.forbid(player, locked.iter().cloned().collect());
            }
        }
    }

    /// The heroes of `player` as a won mission carries them on (`campaign::carried`).
    pub fn carried_heroes(&self, player: u8) -> Vec<crate::campaign::CarriedHero> {
        crate::campaign::carried(&self.state, &self.catalog, player)
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

    /// Commands scheduled and not yet applied, in the order they were queued.
    pub fn pending(&self) -> &[Queued] {
        &self.pending
    }

    /// Simulates one tick.
    pub fn step(&mut self) {
        let clock = self.tick();
        self.advance();
        let _s = tracing::trace_span!("tick: ai").entered();
        self.think(clock);
    }

    /// Steps 1 to 11 of a tick.
    fn advance(&mut self) {
        // Spans named `tick: …` mark the steps for profiling and `just bench-sim`.
        macro_rules! step {
            ($name:literal) => {
                tracing::trace_span!(concat!("tick: ", $name)).entered()
            };
        }
        // Goods that reached a stock this tick (deliveries, income, bounties) start afresh.
        self.economy.deliveries.clear();
        {
            let _s = step!("commands");
            self.apply_due();
        }
        // Step 2: cinematic dialogue pauses the game; only the clock advances.
        if mission::pause(&mut self.state) {
            self.observe();
            return;
        }
        macro_rules! senses {
            () => {
                combat::Senses {
                    catalog: &self.catalog,
                    terrain: &self.terrain,
                    vision: &self.vision,
                    rules: &self.sight,
                    combat: &self.combat.rules,
                    camps: &self.combat.camps,
                    teams: &self.combat.teams,
                    cache: None,
                }
            };
        }
        macro_rules! ctx {
            () => {
                abilities::Ctx {
                    senses: senses!(),
                    spatial: &mut self.motion.spatial,
                    events: &mut self.events,
                    economy: &mut self.economy,
                    limits: self.combat.limits,
                    suppress: 0,
                }
            };
        }
        // A decided match stands still.
        if self.state.outcome.is_some() {
            self.observe();
            self.state.tick += 1;
            return;
        }
        // Step 2: the ability lifecycle, cast orders, autocast and taunts.
        let s = step!("abilities");
        self.motion.spatial.sync_static(&self.state);
        self.motion.spatial.sync_bodies(&self.state);
        abilities::step::begin(&mut ctx!(), &mut self.state);
        drop(s);
        // Step 3: item effects and regeneration.
        let s = step!("items");
        crate::items::effects::tick(&mut ctx!(), &mut self.state);
        crate::items::regeneration::tick(&mut ctx!(), &mut self.state);
        crate::neutrals::tick(&self.catalog, &mut self.state);
        drop(s);
        // Step 4: order advance, then work assignment.
        let s = step!("orders");
        self.motion.spatial.sync_static(&self.state);
        let ai = self.ai_owners();
        orders::plan::advance(
            &mut self.motion,
            &mut self.state,
            &senses!(),
            orders::plan::Work {
                economy: &mut self.economy,
                catalog: &self.catalog,
                senses: economy::command::Senses {
                    vision: &self.vision,
                    terrain: &self.terrain,
                    rules: &self.sight,
                },
                events: &mut self.events,
                limits: self.combat.limits,
                ai: &ai,
            },
        );
        drop(s);
        {
            let _s = step!("economy assign");
            let mut ctx =
                self.economy
                    .ctx(&mut self.state, &self.catalog, &mut self.motion.spatial);
            economy::assign(&mut ctx);
        }
        // Step 5: garrisons, pickups, combat and order planning, separation, idle motion,
        // movement.
        let s = step!("garrisons");
        crate::garrisons::tick(&mut self.state, &self.catalog, &mut self.motion.spatial);
        crate::tunnels::tick(&mut self.state, &self.catalog, &mut self.motion.spatial);
        crate::garrisons::staff(&mut ctx!(), &mut self.state);
        crate::items::inventory::plan(&mut ctx!(), &mut self.state);
        drop(s);
        let s = step!("combat plan");
        combat::plan::plan(
            &senses!(),
            &mut self.motion.spatial,
            &mut self.state,
            &mut self.combat.scratch,
            self.economy.network().unpowered_buildings(),
        );
        drop(s);
        let s = step!("separation");
        self.motion.separate(&mut self.state);
        drop(s);
        let s = step!("idle");
        self.motion.idle(&mut self.state);
        drop(s);
        let s = step!("movement");
        self.motion.move_units(&mut self.state);
        drop(s);
        // Step 6: casters that arrived in range; item effects after movement.
        let s = step!("cast approach");
        abilities::step::after_movement(&mut ctx!(), &mut self.state);
        crate::items::effects::tick(&mut ctx!(), &mut self.state);
        drop(s);
        // Step 7: combat resolution on current sight, then deaths.
        let s = step!("combat resolve");
        self.vision
            .sync(&self.state, &self.catalog, &self.terrain, &self.sight);
        let outcome = combat::resolve::resolve(&mut ctx!(), &mut self.state);
        if !outcome.dead.is_empty() {
            // The dead no longer see.
            self.vision
                .sync(&self.state, &self.catalog, &self.terrain, &self.sight);
            combat::deaths::process(&mut ctx!(), &mut self.state, &outcome.dead);
        }
        drop(s);
        // Step 8: reactions, persistent effects, deliveries and releases, then taunts.
        let s = step!("ability resolve");
        self.vision
            .sync(&self.state, &self.catalog, &self.terrain, &self.sight);
        abilities::step::resolve(&mut ctx!(), &mut self.state);
        drop(s);
        // Step 9: hero returns, revival, jobs, training, regrowth, upgrades, research,
        // and items taken in reach.
        let s = step!("economy advance");
        abilities::world_ops::tick_returns(&mut ctx!(), &mut self.state);
        {
            let mut ctx =
                self.economy
                    .ctx(&mut self.state, &self.catalog, &mut self.motion.spatial);
            economy::advance(&mut ctx);
        }
        crate::items::inventory::advance(&mut ctx!(), &mut self.state);
        drop(s);
        // Step 10: the mission script.
        if let Some(mission) = &mut self.mission {
            let _s = step!("mission");
            let placed = mission.tick(&mut self.state, &self.catalog);
            self.motion.spatial.sync_static(&self.state);
            if placed {
                self.motion.spatial.sync_bodies(&self.state);
            }
        }
        // Step 10b: the mission's computer players (campaign AI).
        if !self.campaign.is_empty() {
            let _s = step!("campaign ai");
            let mut ctx =
                self.economy
                    .ctx(&mut self.state, &self.catalog, &mut self.motion.spatial);
            self.campaign.step(&mut ctx, &self.combat.teams);
            self.motion.spatial.sync_static(&self.state);
        }
        // Step 11: fog and knowledge, then defeat and victory.
        let s = step!("vision");
        self.observe();
        drop(s);
        let _s = step!("victory");
        if self.rules == victory::Rules::Skirmish {
            let teams = self.vision.teams().clone();
            let mut ctx =
                self.economy
                    .ctx(&mut self.state, &self.catalog, &mut self.motion.spatial);
            if victory::advance(&mut ctx, &teams) {
                self.motion.spatial.sync_static(&self.state);
                self.observe();
            }
        }
        self.economy.index(&self.state, &self.catalog);
        self.state.tick += 1;
    }

    /// Owners whose slot an AI plays.
    fn ai_owners(&self) -> Vec<Owner> {
        self.setup
            .slots
            .iter()
            .filter(|s| s.kind == crate::setup::SlotKind::Ai)
            .map(|s| Owner::Player(s.player))
            .collect()
    }

    fn observe(&mut self) {
        self.vision
            .update(&mut self.state, &self.catalog, &self.terrain, &self.sight);
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
            let result = self.apply(owner, &command.action);
            let accepted = result.is_ok();
            if let Err(reason) = result {
                self.state.fact(owner, FactKind::Error, reason);
            }
            ai::Brains::receipt(
                &mut self.state.ai,
                command.player,
                command.seq,
                tick,
                accepted,
            );
        }
    }

    fn apply(&mut self, owner: Owner, action: &Action) -> Result<(), String> {
        // Escape during a scene: allowed while every other command is refused.
        match action {
            Action::SkipScene => {
                mission::skip(&mut self.state, owner);
                return Ok(());
            }
            Action::Clicked { actor } => {
                mission::clicked(&mut self.state, owner, *actor);
                return Ok(());
            }
            _ => {}
        }
        if let Some(reason) = mission::blocks_commands(&self.state) {
            return Err(reason.into());
        }
        if self.state.outcome.is_some() {
            return Err("Match has ended".into());
        }
        if self.defeated(owner) {
            return Err("Your colony has been defeated".into());
        }
        match action {
            Action::Noop => Ok(()),
            Action::Ping { position } => {
                if let Some(p) = position {
                    let tick = self.state.tick;
                    let pings = &mut self.state.pings;
                    pings.retain(|q| tick.saturating_sub(q.tick) < crate::state::PING_TICKS);
                    if pings.len() >= 32 {
                        pings.remove(0);
                    }
                    pings.push(crate::state::Ping {
                        owner,
                        x: p.x,
                        y: p.y,
                        tick,
                    });
                }
                Ok(())
            }
            _ if orders::command::handles(action) => {
                self.motion.spatial.sync_static(&self.state);
                let senses = combat::Senses {
                    catalog: &self.catalog,
                    terrain: &self.terrain,
                    vision: &self.vision,
                    rules: &self.sight,
                    combat: &self.combat.rules,
                    camps: &self.combat.camps,
                    teams: &self.combat.teams,
                    cache: None,
                };
                let applied = orders::command::apply(
                    &mut self.state,
                    &mut self.motion.spatial,
                    &senses,
                    owner,
                    action,
                )?;
                // A new order (or stop) cancels the casts of the units it reached.
                let append = match action {
                    Action::Move { append, .. }
                    | Action::Hold { append, .. }
                    | Action::Patrol { append, .. }
                    | Action::Follow { append, .. }
                    | Action::Attack { append, .. }
                    | Action::Garrison { append, .. } => append.unwrap_or(false),
                    _ => false,
                };
                if !append && !matches!(action, Action::Unload { .. }) {
                    let mut ctx = abilities::Ctx {
                        senses,
                        spatial: &mut self.motion.spatial,
                        events: &mut self.events,
                        economy: &mut self.economy,
                        limits: self.combat.limits,
                        suppress: 0,
                    };
                    for id in applied {
                        abilities::cast::cancel(&mut ctx, &mut self.state, id, "Interrupted");
                    }
                }
                Ok(())
            }
            _ if abilities::orders::handles(action) || crate::items::inventory::handles(action) => {
                self.motion.spatial.sync_static(&self.state);
                self.motion.spatial.sync_bodies(&self.state);
                let control = if self.ai_owners().contains(&owner) {
                    canopy_content::abilities::Control::Ai
                } else {
                    canopy_content::abilities::Control::Player
                };
                let senses = combat::Senses {
                    catalog: &self.catalog,
                    terrain: &self.terrain,
                    vision: &self.vision,
                    rules: &self.sight,
                    combat: &self.combat.rules,
                    camps: &self.combat.camps,
                    teams: &self.combat.teams,
                    cache: None,
                };
                let mut ctx = abilities::Ctx {
                    senses,
                    spatial: &mut self.motion.spatial,
                    events: &mut self.events,
                    economy: &mut self.economy,
                    limits: self.combat.limits,
                    suppress: 0,
                };
                // Commands act between ticks: like the old game, they see the last completed
                // tick (casts accepted now release on the same tick as there).
                let tick = self.state.tick;
                self.state.tick = tick.saturating_sub(1);
                let result = if crate::items::inventory::handles(action) {
                    crate::items::inventory::apply(&mut ctx, &mut self.state, owner, action)
                } else {
                    abilities::orders::apply(&mut ctx, &mut self.state, owner, action, control)
                };
                self.state.tick = tick;
                result
            }
            _ if crate::neutrals::buys(&self.state, action) => {
                self.motion.spatial.sync_static(&self.state);
                self.motion.spatial.sync_bodies(&self.state);
                let mut ctx = abilities::Ctx {
                    senses: combat::Senses {
                        catalog: &self.catalog,
                        terrain: &self.terrain,
                        vision: &self.vision,
                        rules: &self.sight,
                        combat: &self.combat.rules,
                        camps: &self.combat.camps,
                        teams: &self.combat.teams,
                        cache: None,
                    },
                    spatial: &mut self.motion.spatial,
                    events: &mut self.events,
                    economy: &mut self.economy,
                    limits: self.combat.limits,
                    suppress: 0,
                };
                let Action::Produce { actor, definition } = action else {
                    unreachable!("a purchase is a produce");
                };
                crate::neutrals::buy(
                    &mut ctx,
                    &mut self.state,
                    owner,
                    EntityId(*actor),
                    definition,
                )
            }
            _ if economy::command::handles(action) => {
                self.motion.spatial.sync_static(&self.state);
                self.motion.spatial.sync_bodies(&self.state);
                let senses = economy::command::Senses {
                    vision: &self.vision,
                    terrain: &self.terrain,
                    rules: &self.sight,
                };
                let mut ctx =
                    self.economy
                        .ctx(&mut self.state, &self.catalog, &mut self.motion.spatial);
                economy::command::apply(&mut ctx, senses, owner, action).map(|_| ())
            }
            _ => Err("not available yet".into()),
        }
    }

    /// Whether `owner` is out of the match (old `isDefeated`).
    pub fn defeated(&mut self, owner: Owner) -> bool {
        let index = self.economy.index(&self.state, &self.catalog);
        victory::defeated(self.rules, &self.state, &self.catalog, index, owner)
    }

    /// Why `owner` may not place `definition` at `point` with `builder`, if it may not
    /// (old `Game.canBuild`; the AI's placement search asks this).
    pub fn can_build(
        &mut self,
        owner: Owner,
        definition: &str,
        point: &crate::action::FoundationPoint,
        builder: Option<EntityId>,
        rotation: i32,
    ) -> Option<String> {
        self.motion.spatial.sync_static(&self.state);
        let senses = economy::command::Senses {
            vision: &self.vision,
            terrain: &self.terrain,
            rules: &self.sight,
        };
        let ctx = self
            .economy
            .ctx(&mut self.state, &self.catalog, &mut self.motion.spatial);
        let sight = senses.sight(&ctx);
        economy::construction::can_build(&ctx, &sight, owner, definition, point, builder, rotation)
    }

    /// The economy's derived data (indexes, supply hints, the tick's deliveries).
    pub fn economy(&self) -> &Economy {
        &self.economy
    }

    /// Runs `f` with the economy context, for tests and tools that set up economic state
    /// directly (as a mission would).
    pub fn with_economy<T>(&mut self, f: impl FnOnce(&mut economy::Ctx) -> T) -> T {
        let mut ctx = self
            .economy
            .ctx(&mut self.state, &self.catalog, &mut self.motion.spatial);
        f(&mut ctx)
    }

    /// Checks every incrementally maintained economy index against a rebuild.
    pub fn verify_economy(&mut self) -> Result<(), String> {
        self.state.economy.jobs.verify()?;
        self.economy.verify(&self.state, &self.catalog)
    }

    /// Removes an entity with its economic consequences but without a death (old
    /// `Economy.remove`): what a cancelled project or a defeated colony does.
    pub fn remove_entity(&mut self, id: EntityId) {
        self.with_economy(|ctx| economy::work::remove(ctx, id, false));
    }

    /// The desync fingerprint of the current tick (docs/decisions.md D13).
    pub fn fingerprint(&self) -> u32 {
        fingerprint::fingerprint(&self.state)
    }

    /// What `owner` sees.
    pub fn view(&self, owner: Owner) -> View {
        let mut view = View::build(
            &self.state,
            &self.catalog,
            &self.terrain,
            &self.vision,
            &self.sight,
            &self.economy,
            owner,
        );
        self.add_combat(&mut view, Some(owner));
        view.defeated = self.defeated_players();
        view
    }

    /// The match's players who are out so far (the index is current after every step).
    fn defeated_players(&self) -> Vec<Owner> {
        self.setup
            .slots
            .iter()
            .map(|s| Owner::Player(s.player))
            .filter(|&o| {
                victory::defeated(
                    self.rules,
                    &self.state,
                    &self.catalog,
                    &self.economy.index,
                    o,
                )
            })
            .collect()
    }

    /// Everything, for spectators.
    pub fn observer_view(&self) -> View {
        let mut view = View::observe(
            &self.state,
            &self.catalog,
            &self.terrain,
            &self.vision,
            &self.economy,
        );
        self.add_combat(&mut view, None);
        view.defeated = self.defeated_players();
        view.ai = self.ai_summaries();
        view
    }

    /// Combat state of the view's live entities and the viewer's battle.
    fn add_combat(&self, view: &mut View, viewer: Option<Owner>) {
        let senses = self.senses();
        for entity in view.entities.iter_mut().filter(|e| !e.remembered) {
            if let Some(e) = self.state.entities.get(entity.id()) {
                entity.combat = combat::CombatView::of(
                    &senses,
                    &self.state,
                    e,
                    viewer,
                    entity.private.is_some(),
                );
                entity.magic = abilities::view::MagicView::of(
                    &senses,
                    &self.state,
                    e,
                    entity.private.is_some(),
                );
            }
        }
        view.battle = combat::Battle::of(&senses, &self.state, viewer);
        view.spells = abilities::view::Spellcraft::of(&senses, &self.state, &self.events, viewer);
    }

    /// Recent ability events (all of them; views filter by viewer).
    pub fn events(&self) -> &abilities::Events {
        &self.events
    }

    /// Takes the recorded ability events (tests and tools).
    pub fn drain_events(&mut self) -> Vec<abilities::AbilityEvent> {
        self.events.drain()
    }

    /// Combat's perception and allegiance answers.
    pub fn senses(&self) -> combat::Senses<'_> {
        combat::Senses {
            catalog: &self.catalog,
            terrain: &self.terrain,
            vision: &self.vision,
            rules: &self.sight,
            combat: &self.combat.rules,
            camps: &self.combat.camps,
            teams: &self.combat.teams,
            cache: None,
        }
    }

    /// Sight questions (what an owner sees, knows or detects) without building a view:
    /// the AI's and the command checks' way in.
    pub fn sight(&self) -> Sight<'_> {
        Sight {
            vision: &self.vision,
            state: &self.state,
            catalog: &self.catalog,
            terrain: &self.terrain,
            rules: &self.sight,
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
        let mut combat = combat::Runtime::new(&catalog, map, &save.setup).map_err(SetupError)?;
        combat.limits.level_cap = level_cap(&save.state);
        let mission = mission_runtime(map, &save.setup)?;
        if mission.is_some() != save.state.mission.is_some() {
            return Err(SetupError(
                "the save does not match the map's mission".into(),
            ));
        }
        let mut state = save.state;
        crate::economy::lane::order_legacy(&mut state);
        let campaign = ai::campaign::Campaign::new(map);
        campaign.check(&state.campaign_ai).map_err(SetupError)?;
        let sight = SpellSight {
            catalog: catalog.clone(),
        };
        let vision = Vision::restore(
            &mut state,
            &catalog,
            &terrain,
            &save.setup,
            &campaign.private_sight(),
            &sight,
        )
        .map_err(SetupError)?;
        let motion = Motion::new(
            terrain.clone(),
            &catalog,
            &state,
            crate::teams::Teams::new(&save.setup),
            appearance_scales(map, &terrain),
        );
        let mut economy = Economy::new(&state, &catalog, &save.setup);
        let mut brains = ai::Brains::new(
            &catalog,
            map,
            terrain.clone(),
            &save.setup,
            appearance_scales(map, &terrain),
        );
        brains.check(&state.ai, &save.pending).map_err(SetupError)?;
        hold_back(&campaign, &catalog, &mut brains, &mut economy);
        let mut world = Self {
            catalog,
            terrain,
            setup: save.setup,
            state,
            pending: save.pending,
            mission,
            vision,
            motion,
            combat,
            economy,
            rules: match_rules(map),
            sight,
            events: abilities::Events::default(),
            ai: brains,
            campaign,
        };
        world.sync_ai_tech();
        world.prepare_ai();
        Ok(world)
    }
}

impl World {
    /// Moves this world to `save`, a save of another moment of the same match (a replay's
    /// keyframe, D60), as [`World::restore`] would rebuild it but much faster: the parts
    /// derived from the map, content and setup stay, and the spatial model and the AI
    /// players' navigation are synced to the saved state as after any tick (only what
    /// differs is redone) rather than built from nothing. The world then plays on exactly
    /// as a restored one.
    pub fn rewind(&mut self, save: Save) -> Result<(), SetupError> {
        if save.version != SAVE_VERSION {
            return Err(SetupError(format!(
                "unsupported save version {}",
                save.version
            )));
        }
        if save.setup != self.setup {
            return Err(SetupError("the save is of another match".into()));
        }
        if self.mission.is_some() != save.state.mission.is_some() {
            return Err(SetupError(
                "the save does not match the map's mission".into(),
            ));
        }
        self.ai
            .check(&save.state.ai, &save.pending)
            .map_err(SetupError)?;
        self.campaign
            .check(&save.state.campaign_ai)
            .map_err(SetupError)?;
        let mut state = save.state;
        crate::economy::lane::order_legacy(&mut state);
        let vision = Vision::restore(
            &mut state,
            &self.catalog,
            &self.terrain,
            &self.setup,
            &self.campaign.private_sight(),
            &self.sight,
        )
        .map_err(SetupError)?;
        self.motion.rewind(&state);
        self.economy = Economy::new(&state, &self.catalog, &self.setup);
        income(&self.campaign, &mut self.economy);
        self.combat.scratch = Default::default();
        self.combat.limits.level_cap = level_cap(&state);
        self.state = state;
        self.pending = save.pending;
        self.vision = vision;
        self.events = abilities::Events::default();
        if let Some(m) = &self.mission {
            self.mission = Some(mission::Runtime::new(m.script.clone()));
        }
        self.sync_ai_tech();
        self.prepare_ai();
        Ok(())
    }
}

/// Holds the mission's Skirmish players back (style, army cap, pace, income).
fn hold_back(
    campaign: &ai::campaign::Campaign,
    catalog: &Catalog,
    brains: &mut ai::Brains,
    economy: &mut Economy,
) {
    for (player, limits) in campaign.skirmish_limits() {
        brains.hold_back(catalog, player, &limits);
    }
    income(campaign, economy);
}

/// The mission's income handicaps (derived economy tables).
fn income(campaign: &ai::campaign::Campaign, economy: &mut Economy) {
    for (player, limits) in campaign.skirmish_limits() {
        if let Some(income) = limits.income {
            let permille = (income * 1000.0).round().max(0.0) as u32;
            economy.set_income(Owner::Player(player), permille);
        }
    }
}

/// How the map decides its outcome.
fn match_rules(map: &UtcMap) -> victory::Rules {
    if map.mission.is_some() {
        victory::Rules::Mission
    } else if map.sandbox {
        victory::Rules::Sandbox
    } else {
        victory::Rules::Skirmish
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

/// A campaign's hero level cap, from the saved state.
fn level_cap(state: &State) -> Option<u32> {
    state.campaign.as_ref().and_then(|c| c.hero_level_cap)
}

fn mission_runtime(
    map: &UtcMap,
    setup: &MatchSetup,
) -> Result<Option<mission::Runtime>, SetupError> {
    let script = mission::Script::new(map, setup).map_err(SetupError)?;
    Ok(script.map(|s| mission::Runtime::new(Arc::new(s))))
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
