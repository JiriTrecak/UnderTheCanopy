//! AI players (old `src/sim/ai/*`, the brains of `World`): one per AI slot, run inside
//! the simulation on every peer (step 12 of the tick), deciding from their own player's
//! knowledge and issuing ordinary [`Action`]s for the next tick.
//!
//! # Layers
//!
//! - [`geography`]: the briefing (frozen public map facts) and terrain connectivity,
//!   shared by every AI of a match.
//! - [`frame`]: one decision's view: what the player sees and remembers, its own private
//!   state, and questions about it (power, prerequisites, safe points).
//! - [`navigation`]: known blockers and the engine's sweeps, sectors, grid and mesh over
//!   them; [`placement`]: building sites from the same knowledge.
//! - Decisions, in the old order: ability learning and casting ([`powers`]), heroes and
//!   items, strategy (every `strategyTicks`), economy (every `economyTicks`), reactions
//!   (every decision), operations (every `operationTicks`).
//! - [`continuity`]: orders already in progress are kept rather than reissued.
//!
//! # Commands
//!
//! A decision issues at most `limits.commandsPerBeat` commands, at most one spending
//! command (build, train, upgrade, research, revive), at most one command per actor, and
//! none while 32 are pending. Sequence numbers start above 1,000,000. Commands are queued
//! for the next tick on every peer (never sent over the network); each one's receipt
//! (applied or refused) comes back through [`Brains::receipt`].
//!
//! # State
//!
//! Each AI's [`Memory`] is saved in `State::ai`; everything else here is derived and
//! rebuilt from the player's knowledge on load. The only randomness is the placement
//! cursor, seeded from the match seed and the player.

pub mod continuity;
pub mod economy;
pub mod frame;
pub mod geography;
pub mod navigation;
pub mod operations;
pub mod placement;
pub mod powers;
pub mod rules;
pub mod state;
pub mod tactics;

use crate::{action::Action, setup::MatchSetup};
use canopy_content::placement::Owner;
use frame::{Frame, Input, Known};
use geography::Geography;
use navigation::{ObservedBlockers, ObservedNavigation};
pub use rules::{AiRules, AiStyle};
use serde::Serialize;
pub use state::Memory;
use state::{MAX_PENDING, MAX_TRACE, Pending, SEQUENCE_BASE, Trace};
use std::{cell::RefCell, sync::Arc};

/// A command an AI issues: its sequence number and action.
#[derive(Debug, Clone, PartialEq)]
pub struct Command {
    pub seq: u32,
    pub action: Action,
}

/// What the debug overlay shows of one AI.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub player: u8,
    /// `kind: key` of the current operation, or `planning`.
    pub mission: String,
    pub plan: state::Plan,
    pub metrics: state::Metrics,
    /// The latest decisions.
    pub trace: Vec<Trace>,
}

/// The actors a command addresses: its units, or the building or unit it acts through.
pub fn actors(action: &Action) -> Vec<u32> {
    match action {
        Action::Garrison { actors, .. }
        | Action::Gather { actors, .. }
        | Action::Construct { actors, .. }
        | Action::Move { actors, .. }
        | Action::Attack { actors, .. }
        | Action::Stop { actors }
        | Action::Hold { actors, .. }
        | Action::Patrol { actors, .. }
        | Action::Follow { actors, .. }
        | Action::Build { actors, .. } => actors.clone(),
        Action::AbilityAutocast { actor, .. }
        | Action::CastAbility { actor, .. }
        | Action::Unload { actor }
        | Action::Research { actor, .. }
        | Action::CancelResearch { actor, .. }
        | Action::Upgrade { actor }
        | Action::CancelUpgrade { actor }
        | Action::Revive { actor, .. }
        | Action::CancelRevival { actor, .. }
        | Action::LearnAbility { actor, .. }
        | Action::Pickup { actor, .. }
        | Action::DropItem { actor, .. }
        | Action::UseItem { actor, .. }
        | Action::Produce { actor, .. }
        | Action::Cancel { actor, .. }
        | Action::Rally { actor, .. }
        | Action::Pause { actor, .. } => vec![*actor],
        Action::Noop | Action::Ping { .. } => Vec::new(),
    }
}

/// Replaces the units a group command addresses.
fn set_actors(action: &mut Action, ids: Vec<u32>) {
    match action {
        Action::Garrison { actors, .. }
        | Action::Gather { actors, .. }
        | Action::Construct { actors, .. }
        | Action::Move { actors, .. }
        | Action::Attack { actors, .. }
        | Action::Stop { actors }
        | Action::Hold { actors, .. }
        | Action::Patrol { actors, .. }
        | Action::Follow { actors, .. }
        | Action::Build { actors, .. } => *actors = ids,
        _ => {}
    }
}

fn grouped(action: &Action) -> bool {
    matches!(
        action,
        Action::Garrison { .. }
            | Action::Gather { .. }
            | Action::Construct { .. }
            | Action::Move { .. }
            | Action::Attack { .. }
            | Action::Stop { .. }
            | Action::Hold { .. }
            | Action::Patrol { .. }
            | Action::Follow { .. }
            | Action::Build { .. }
    )
}

/// Commands that pay: at most one per decision.
fn spends(action: &Action) -> bool {
    matches!(
        action,
        Action::Build { .. }
            | Action::Produce { .. }
            | Action::Upgrade { .. }
            | Action::Research { .. }
            | Action::Revive { .. }
    )
}

fn type_name(action: &Action) -> String {
    serde_json::to_value(action)
        .ok()
        .and_then(|v| v.get("type").and_then(|t| t.as_str()).map(String::from))
        .unwrap_or_default()
}

/// One decision in progress: the frame, the AI's memory and the commands issued so far.
pub struct Beat<'f, 'a> {
    pub f: &'f Frame<'a>,
    pub s: &'f mut Memory,
    out: Vec<Command>,
    /// Actors given a command (or kept on their order) this beat, sorted.
    claimed: Vec<u32>,
    spent: bool,
}

impl<'f, 'a> Beat<'f, 'a> {
    pub fn new(f: &'f Frame<'a>, s: &'f mut Memory) -> Self {
        Self {
            f,
            s,
            out: Vec::new(),
            claimed: Vec::new(),
            spent: false,
        }
    }

    pub fn rules(&self) -> &'a AiRules {
        self.f.rules
    }

    fn claim(&mut self, id: u32) {
        if let Err(i) = self.claimed.binary_search(&id) {
            self.claimed.insert(i, id);
        }
    }

    pub fn claimed(&self, id: u32) -> bool {
        self.claimed.binary_search(&id).is_ok()
    }

    /// Issues `action` unless a limit or an earlier command this beat stops it (old
    /// `emit`). Units already carrying out the same intention keep it and are only
    /// claimed. Returns whether a command was issued.
    pub fn emit(&mut self, mut action: Action, reason: impl Into<String>) -> bool {
        let f = self.f;
        let tick = f.tick;
        if grouped(&action) {
            let available: Vec<u32> = actors(&action)
                .into_iter()
                .filter(|&id| !self.claimed(id))
                .collect();
            if available.is_empty() {
                return false;
            }
            set_actors(&mut action, available);
        }
        let mut ids = actors(&action);
        let signature = continuity::canonical(&action);
        if self.out.len() >= self.rules().limits.commands_per_beat as usize
            || ids.iter().any(|&id| self.claimed(id))
            || (spends(&action) && self.spent)
            || self.s.pending.len() >= MAX_PENDING
            || self
                .s
                .inspected
                .get(&format!("reject:{signature}"))
                .is_some_and(|&t| t > tick)
        {
            return false;
        }
        if ids.iter().any(|id| {
            self.s
                .pending
                .iter()
                .any(|p| actors(&p.action).contains(id))
        }) {
            return false;
        }
        // Keep active paths intact; reissue only when progress has stalled.
        if matches!(
            action,
            Action::Move { .. }
                | Action::Attack { .. }
                | Action::Gather { .. }
                | Action::Pickup { .. }
        ) {
            let intent = continuity::intent(&action);
            let interval = self.rules().order_interval_ticks;
            let mut remaining = Vec::new();
            for id in ids {
                let continuing = f.get(id).is_some_and(|e| {
                    continuity::continues(
                        f,
                        e,
                        self.s.orders.get_mut(&id),
                        &action,
                        &intent,
                        tick,
                        interval,
                    )
                });
                if continuing {
                    // Continuing a higher-priority action also claims the actor.
                    self.claim(id);
                } else {
                    remaining.push(id);
                }
            }
            if remaining.is_empty() {
                return false;
            }
            if grouped(&action) {
                set_actors(&mut action, remaining.clone());
            }
            ids = remaining;
        }
        let mut units: Vec<&Known> = Vec::with_capacity(ids.len());
        for &id in &ids {
            match f.get(id) {
                Some(e) if e.owner == f.me => units.push(e),
                _ => return false,
            }
        }
        self.s.sequence += 1;
        let seq = SEQUENCE_BASE + self.s.sequence;
        let intent = continuity::intent(&action);
        for e in units {
            self.claim(e.id);
            self.s.orders.insert(
                e.id,
                state::OrderMemo {
                    signature: intent.clone(),
                    tick,
                    point: f.geo.place(e.p),
                    hp: e.hp.unwrap_or(0),
                },
            );
        }
        self.spent |= spends(&action);
        let reason = reason.into();
        self.s.pending.push(Pending {
            seq,
            tick,
            action: action.clone(),
            reason: reason.clone(),
        });
        self.s.metrics.issued += 1;
        self.s.trace.push(Trace {
            tick,
            reason,
            action: type_name(&action),
            accepted: None,
        });
        if self.s.trace.len() > MAX_TRACE {
            self.s.trace.remove(0);
        }
        self.out.push(Command { seq, action });
        true
    }

    pub fn finish(self) -> Vec<Command> {
        self.out
    }
}

/// One AI player's derived side.
struct Brain {
    player: u8,
    owner: Owner,
    /// Decides on ticks where `tick % decisionTicks == phase`.
    phase: u32,
    race: Option<String>,
    rules: AiRules,
    blockers: RefCell<ObservedBlockers>,
    navigation: RefCell<Option<ObservedNavigation>>,
}

/// Every AI of a match: derived, rebuilt on load.
pub struct Brains {
    geography: Option<Arc<Geography>>,
    brains: Vec<Brain>,
}

impl Brains {
    /// The AIs of the match's AI slots. `scales` gives placements' appearance scales.
    pub fn new(
        catalog: &crate::catalog::Catalog,
        map: &canopy_content::map::UtcMap,
        terrain: Arc<crate::terrain::Terrain>,
        setup: &MatchSetup,
        scales: std::collections::BTreeMap<String, f64>,
    ) -> Self {
        let mut slots: Vec<&crate::setup::Slot> = setup
            .slots
            .iter()
            .filter(|s| s.kind == crate::setup::SlotKind::Ai)
            .collect();
        slots.sort_by_key(|s| s.player);
        if slots.is_empty() {
            return Self {
                geography: None,
                brains: Vec::new(),
            };
        }
        let geography = Arc::new(Geography::new(map, catalog, terrain, scales));
        let cells = (geography.size * geography.size) as usize;
        let count = slots.len() as u32;
        let brains = slots
            .iter()
            .enumerate()
            .map(|(index, slot)| {
                let style = AiStyle::of_match(setup.seed, slot.player);
                let rules = AiRules::for_race(catalog, slot.race.as_deref(), style);
                Brain {
                    player: slot.player,
                    owner: Owner::Player(slot.player),
                    phase: index as u32 * rules.decision_ticks / count,
                    race: slot.race.clone(),
                    rules,
                    blockers: RefCell::new(ObservedBlockers::new(cells)),
                    navigation: RefCell::new(None),
                }
            })
            .collect();
        Self {
            geography: Some(geography),
            brains,
        }
    }

    /// Plays `player` in `style` instead of the one its match seed chose (tools and
    /// experiments; the next decision uses it).
    pub fn restyle(&mut self, catalog: &crate::catalog::Catalog, player: u8, style: AiStyle) {
        for b in self.brains.iter_mut().filter(|b| b.player == player) {
            let rules = AiRules::for_race(catalog, b.race.as_deref(), style);
            b.phase = b.phase.min(rules.decision_ticks.saturating_sub(1));
            b.rules = rules;
        }
    }

    /// The style each AI player plays, by player.
    pub fn styles(&self) -> Vec<(u8, AiStyle)> {
        self.brains
            .iter()
            .map(|b| (b.player, b.rules.style))
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.brains.is_empty()
    }

    pub fn players(&self) -> impl Iterator<Item = u8> + '_ {
        self.brains.iter().map(|b| b.player)
    }

    /// Fresh memories for a new match, by player.
    pub fn memories(&self, seed: u32) -> Vec<Memory> {
        let briefing = self.geography.as_ref().map_or(0, |g| g.fingerprint);
        self.brains
            .iter()
            .map(|b| {
                let mixed = seed ^ (u32::from(b.player) + 1).wrapping_mul(0x9e37_79b1);
                Memory::new(b.player, briefing, mixed)
            })
            .collect()
    }

    /// Checks restored memories against this match.
    pub fn check(
        &self,
        memories: &[Memory],
        pending: &[crate::world::Queued],
    ) -> Result<(), String> {
        if memories.len() != self.brains.len()
            || memories
                .iter()
                .zip(&self.brains)
                .any(|(m, b)| m.player != b.player)
        {
            return Err("the saved AI players do not match the match's AI slots".into());
        }
        let briefing = self.geography.as_ref().map_or(0, |g| g.fingerprint);
        for m in memories {
            m.check()?;
            if m.briefing != briefing {
                return Err("the saved AI was made for another map".into());
            }
            for p in &m.pending {
                let queued = pending.iter().any(|q| {
                    q.player == m.player
                        && q.seq == p.seq
                        && q.tick == p.tick + 1
                        && q.action == p.action
                });
                if !queued {
                    return Err("an AI command is missing from the saved queue".into());
                }
            }
        }
        Ok(())
    }

    /// Builds each AI's known blockers and navigation from its knowledge (match start and
    /// after a restore), as the old `prepareNavigation` did.
    pub fn prepare(&self, input: &Input, tick: u32) {
        let Some(geo) = self.geography.clone() else {
            return;
        };
        for brain in &self.brains {
            let f = Frame::new(
                input,
                &geo,
                brain.owner,
                tick,
                &brain.rules,
                &brain.blockers,
                &brain.navigation,
            );
            f.prepare_navigation();
        }
    }

    /// Takes a command's receipt (old `receipt`).
    pub fn receipt(memories: &mut [Memory], player: u8, seq: u32, tick: u32, accepted: bool) {
        let Some(s) = memories.iter_mut().find(|m| m.player == player) else {
            return;
        };
        let Some(i) = s.pending.iter().position(|p| p.seq == seq) else {
            return;
        };
        let p = s.pending.remove(i);
        if accepted {
            s.metrics.accepted += 1;
        } else {
            s.metrics.rejected += 1;
        }
        let name = type_name(&p.action);
        if let Some(t) = s
            .trace
            .iter_mut()
            .rev()
            .find(|t| t.tick == p.tick && t.action == name && t.accepted.is_none())
        {
            t.accepted = Some(accepted);
        }
        if accepted {
            match &p.action {
                Action::Build { .. } => s.metrics.builds += 1,
                Action::Produce { .. } => s.metrics.recruits += 1,
                Action::CastAbility { .. } => s.metrics.casts += 1,
                Action::Pickup { .. } => s.metrics.pickups += 1,
                Action::Revive { .. } => s.metrics.revivals += 1,
                _ => {}
            }
            return;
        }
        if let Action::Build {
            definition,
            position,
            rotation,
            ..
        } = &p.action
        {
            let key = state::site_key(
                definition.as_str(),
                f64::from(position.x.0) / 2.0,
                f64::from(position.y.0) / 2.0,
                rotation.unwrap_or(0),
            );
            s.failed_sites.insert(key, tick + 1200);
        }
        for id in actors(&p.action) {
            s.orders.remove(&id);
        }
        // A bounded cooldown after a refusal: do not probe an unseen blocker every beat.
        s.inspected.insert(
            format!("reject:{}", continuity::canonical(&p.action)),
            tick + 160,
        );
    }

    /// Step 12: the AIs due this tick decide (old `World.tick` brains loop). Returns each
    /// command with its player, to be queued for the next tick.
    pub fn decide(&self, input: &Input, memories: &mut [Memory], tick: u32) -> Vec<(u8, Command)> {
        let Some(geo) = self.geography.clone() else {
            return Vec::new();
        };
        let mut out = Vec::new();
        if input.state.outcome.is_some() {
            return out;
        }
        for (brain, memory) in self.brains.iter().zip(memories.iter_mut()) {
            if tick % brain.rules.decision_ticks != brain.phase {
                continue;
            }
            let _span = tracing::trace_span!("ai decision").entered();
            let f = Frame::new(
                input,
                &geo,
                brain.owner,
                tick,
                &brain.rules,
                &brain.blockers,
                &brain.navigation,
            );
            let commands = think(&f, memory);
            out.extend(commands.into_iter().map(|c| (brain.player, c)));
        }
        out
    }

    /// The debug overlay's view of every AI.
    pub fn summaries(memories: &[Memory]) -> Vec<Summary> {
        memories
            .iter()
            .map(|s| Summary {
                player: s.player,
                mission: s.mission.as_ref().map_or_else(
                    || "planning".into(),
                    |m| format!("{}: {}", m.kind.name(), m.key),
                ),
                plan: s.plan.clone(),
                metrics: s.metrics,
                trace: s.trace[s.trace.len().saturating_sub(8)..].to_vec(),
            })
            .collect()
    }

    /// Reachability searches run by every AI's navigation (profiling).
    pub fn searches(&self) -> u64 {
        self.brains
            .iter()
            .filter_map(|b| b.navigation.borrow().as_ref().map(|n| n.searches))
            .sum()
    }
}

/// One decision (old `decide`).
fn think(f: &Frame, s: &mut Memory) -> Vec<Command> {
    let tick = f.tick;
    let rules = f.rules;
    operations::observe(f, s);
    let mut beat = Beat::new(f, s);
    // Skill and item management and immediate survival claim their actors first.
    powers::ability_actions(&mut beat);
    tactics::hero_actions(&mut beat);
    if tick >= beat.s.next_strategy {
        operations::strategy(&mut beat);
        beat.s.next_strategy = tick + rules.strategy_ticks;
    }
    if tick >= beat.s.next_economy {
        economy::economy(&mut beat);
        beat.s.next_economy = tick + rules.economy_ticks;
    }
    tactics::reactions(&mut beat);
    if tick >= beat.s.next_operation {
        operations::operations(&mut beat);
        beat.s.next_operation = tick + rules.operation_ticks;
    }
    beat.finish()
}
