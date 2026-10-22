//! The campaign AI (docs/campaign.md "Campaign AI"): the computer players a mission lists in
//! `mission.players`, each played by a behaviour far simpler than the skirmish AI.
//!
//! - **Passive** holds its ground: the units the map placed defend where they stand (they
//!   answer attackers near them and their neighbours, and walk back home once it is over;
//!   a unit drawn too far breaks off).
//! - **Base** is Passive that keeps its defenders at full strength (trained again, free, at
//!   its own buildings that train them), revives its heroes free at an altar, and rebuilds
//!   destroyed buildings free when `rebuild`. It never attacks.
//! - **Waves** is Base plus wave lines ([`waves`]).
//! - **Skirmish** is the skirmish AI ([`super::Brains`]) held back by the mission's limits
//!   ([`super::Brains::hold_back`]); it mines and pays as usual. Nothing else costs resources.
//!
//! A **dormant** player does nothing until the script wakes it (`mission.ai_wake`); its
//! script-activated placements appear then and its timers start.
//!
//! Like the skirmish AI it runs inside the simulation on every peer: after the mission
//! script (step 10), acting on the state directly with ordinary unit orders. Its
//! [`Memory`] is saved in `State::campaign_ai`; [`Campaign`] is derived from the map.

mod posts;
pub mod setup;
mod waves;

pub use setup::mission_setup;

use crate::{
    catalog::Def,
    economy::Ctx,
    entity::{Entity, EntityId, Milli, Place},
    orders,
    spatial::Spot,
    state::State,
    teams::{Relation, Teams},
};
use canopy_content::{
    Id,
    map::{Behaviour, MissionPlayer, Region, SkirmishLimits, UtcMap, WaveTarget},
    placement::Owner,
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Ticks between decisions (orders, roster checks); timers run every tick.
pub const DECIDE_TICKS: u32 = 20;
/// Defenders walk back once further than this from home (cells).
const HOME_SLACK: i64 = 3;
/// A defender drawn further than this from home breaks off and walks back (cells).
const LEASH: i64 = 18;
/// Defenders this close to a fighting neighbour join in (cells).
const ALERT: i64 = 10;
/// Seconds before a destroyed building is rebuilt.
const REBUILD_DELAY_SECONDS: u32 = 30;
/// Ticks before a blocked rebuild, revival or hatch is tried again.
const RETRY_TICKS: u32 = 80;

/// What a mission script asks of a computer player (`mission.ai_*`).
#[derive(Debug, Clone, PartialEq)]
pub enum Order {
    /// Pause or resume its waves; resuming starts lines that wait for the script.
    Waves(bool),
    /// Every running line sends its next wave now.
    AttackNow,
    /// Where its waves go from now on, over each line's own target.
    Target(WaveTarget),
    Behaviour(Behaviour),
    /// A dormant player arrives and starts playing.
    Wake,
}

/// One computer player's saved side.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    /// Zero-based player slot.
    pub player: u8,
    pub behaviour: Behaviour,
    pub dormant: bool,
    /// Its roster and wave timers are set up (once it is active).
    pub activated: bool,
    /// The tick it became active: wave times count from here.
    pub since: u32,
    pub waves_on: bool,
    pub attack_now: bool,
    /// The script's target, over each line's own.
    pub target: Option<WaveTarget>,
    pub lines: Vec<LineState>,
    /// What the map placed for it, in the order first seen.
    pub posts: Vec<Post>,
    /// Free training in progress, in the order started.
    pub training: Vec<Training>,
    /// Waves gathering or under way, oldest first.
    pub waves: Vec<Wave>,
    pub next_wave: u32,
}

/// A wave line's progress.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LineState {
    /// When the next wave is due; `None` until the line starts.
    pub next: Option<u32>,
    pub sent: u32,
    pub done: bool,
}

/// A unit or building the map placed for the player, kept or rebuilt in place.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Post {
    pub placement: String,
    pub definition: Id,
    pub building: bool,
    /// Where it stands guard, or where the building stands.
    pub home: Milli,
    /// Millidegrees (buildings).
    pub rotation: i32,
    pub surface: Option<String>,
    /// Who holds the post now: the placed entity, its replacement or a rebuilt site.
    pub entity: Option<EntityId>,
    /// Given away (a script transferred it): never replaced.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub lost: bool,
    /// A replacement is in training.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub training: bool,
    /// When a rebuild, or a hero's revival, is due.
    pub due: Option<u32>,
    /// The altar a fallen hero revives at.
    pub altar: Option<EntityId>,
    /// The cell of the last move the AI gave it; any other move is the script's, and moves
    /// its home.
    pub sent: Option<(i32, i32)>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Purpose {
    /// Replaces the post at this index.
    Post(u32),
    /// Joins the wave of this id.
    Wave(u32),
}

/// A unit trained free at a building.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Training {
    pub building: EntityId,
    pub unit: Id,
    /// The tick it is ready.
    pub done: u32,
    pub purpose: Purpose,
}

/// A wave: gathering while its units train, then under way.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Wave {
    pub id: u32,
    pub line: u32,
    pub units: Vec<EntityId>,
    pub heroes: Vec<EntityId>,
    /// Units still in training.
    pub pending: u32,
    pub launched: bool,
    pub target: WaveTarget,
}

impl Memory {
    fn new(plan: &MissionPlayer) -> Self {
        Self {
            player: plan.player.saturating_sub(1),
            behaviour: plan.ai.behaviour,
            dormant: plan.ai.dormant,
            activated: false,
            since: 0,
            waves_on: true,
            attack_now: false,
            target: None,
            lines: vec![LineState::default(); plan.ai.waves.len()],
            posts: Vec::new(),
            training: Vec::new(),
            waves: Vec::new(),
            next_wave: 1,
        }
    }

    /// Whether the skirmish AI plays this player now.
    pub fn skirmish(&self) -> bool {
        self.behaviour == Behaviour::Skirmish && !self.dormant
    }
}

/// The campaign players of a match: derived from the map, never saved.
#[derive(Debug, Clone, Default)]
pub struct Campaign {
    /// By zero-based player.
    players: Vec<MissionPlayer>,
    regions: Vec<Region>,
    /// Each player start's owner and main fort placement.
    forts: Vec<(Owner, String)>,
}

impl Campaign {
    pub fn new(map: &UtcMap) -> Self {
        let Some(mission) = &map.mission else {
            return Self::default();
        };
        let mut players = mission.players.clone();
        players.sort_by_key(|p| p.player);
        Self {
            players,
            regions: mission.regions.clone(),
            forts: map
                .player_starts
                .iter()
                .map(|s| (s.owner(), s.main_fort.clone()))
                .collect(),
        }
    }

    /// Whether the map lists its computer players: the campaign AI plays them.
    pub fn is_empty(&self) -> bool {
        self.players.is_empty()
    }

    /// The listed players, zero-based.
    pub fn players(&self) -> impl Iterator<Item = u8> + '_ {
        self.players.iter().map(|p| p.player.saturating_sub(1))
    }

    fn plan(&self, player: u8) -> Option<&MissionPlayer> {
        self.players
            .iter()
            .find(|p| p.player.saturating_sub(1) == player)
    }

    /// Allies that keep their vision to themselves, zero-based.
    pub fn private_sight(&self) -> Vec<u8> {
        self.players
            .iter()
            .filter(|p| p.team == canopy_content::map::Team::Ally && !p.shares_vision())
            .map(|p| p.player.saturating_sub(1))
            .collect()
    }

    /// Each listed player's limits for the Skirmish behaviour, zero-based.
    pub fn skirmish_limits(&self) -> Vec<(u8, SkirmishLimits)> {
        self.players
            .iter()
            .map(|p| {
                (
                    p.player.saturating_sub(1),
                    p.ai.skirmish.clone().unwrap_or_default(),
                )
            })
            .collect()
    }

    /// Fresh memories for a new match.
    pub fn memories(&self) -> Vec<Memory> {
        self.players.iter().map(Memory::new).collect()
    }

    /// Checks restored memories against the map's players.
    pub fn check(&self, memories: &[Memory]) -> Result<(), String> {
        let fits = memories.len() == self.players.len()
            && memories.iter().zip(&self.players).all(|(m, p)| {
                m.player == p.player.saturating_sub(1) && m.lines.len() == p.ai.waves.len()
            });
        if fits {
            Ok(())
        } else {
            Err("the saved campaign players do not match the mission's players".into())
        }
    }

    /// Players the skirmish AI must leave alone this tick: dormant, or played by another
    /// behaviour.
    pub fn idle_brains(&self, memories: &[Memory]) -> Vec<u8> {
        memories
            .iter()
            .filter(|m| !m.skirmish())
            .map(|m| m.player)
            .collect()
    }

    /// After the mission script (step 10): every active player plays its behaviour.
    pub fn step(&self, ctx: &mut Ctx, teams: &Teams) {
        if self.players.is_empty() || ctx.state.outcome.is_some() {
            return;
        }
        let _span = tracing::trace_span!("campaign ai").entered();
        let mut memories = std::mem::take(&mut ctx.state.campaign_ai);
        for memory in memories.iter_mut() {
            let Some(plan) = self.plan(memory.player) else {
                continue;
            };
            let mut play = Play {
                campaign: self,
                plan,
                me: Owner::Player(memory.player),
                tick: ctx.state.tick,
                ctx,
                teams,
            };
            play.run(memory);
        }
        ctx.state.campaign_ai = memories;
    }

    fn region(&self, id: &str) -> Option<Milli> {
        self.regions
            .iter()
            .find(|r| r.id == id)
            .map(|r| milli_cell(r.x as i32, r.y as i32))
    }
}

/// Applies a script's order to `player`'s memory (mission effects, step 10).
pub fn command(state: &mut State, player: u8, order: &Order) {
    let tick = state.tick;
    let Some(m) = state.campaign_ai.iter_mut().find(|m| m.player == player) else {
        return;
    };
    match order {
        Order::Waves(on) => {
            m.waves_on = *on;
            if *on {
                for line in m.lines.iter_mut().filter(|l| l.next.is_none() && !l.done) {
                    line.next = Some(tick);
                }
            }
        }
        Order::AttackNow => m.attack_now = true,
        Order::Target(target) => m.target = Some(target.clone()),
        Order::Behaviour(behaviour) => m.behaviour = *behaviour,
        Order::Wake => {
            if m.dormant {
                m.dormant = false;
                m.since = tick;
            }
        }
    }
}

/// Holds every computer player's clock back by `ticks` (cinematic mode): waves, free
/// training, rebuilds and revivals fall due that much later, so nothing launches during
/// a cinematic or all at once after it.
pub fn delay(memories: &mut [Memory], ticks: u32) {
    for m in memories {
        // Before activation too: an opening cinematic delays the first waves.
        m.since = m.since.saturating_add(ticks);
        for line in &mut m.lines {
            if let Some(next) = &mut line.next {
                *next = next.saturating_add(ticks);
            }
        }
        for training in &mut m.training {
            training.done = training.done.saturating_add(ticks);
        }
        for post in &mut m.posts {
            if let Some(due) = &mut post.due {
                *due = due.saturating_add(ticks);
            }
        }
    }
}

/// One player's turn.
struct Play<'p, 'c> {
    campaign: &'p Campaign,
    plan: &'p MissionPlayer,
    me: Owner,
    tick: u32,
    ctx: &'p mut Ctx<'c>,
    teams: &'p Teams,
}

impl Play<'_, '_> {
    fn run(&mut self, m: &mut Memory) {
        if m.dormant {
            return;
        }
        let mut decide = self.tick.is_multiple_of(DECIDE_TICKS);
        if !m.activated {
            self.activate(m);
            decide = true;
        }
        if m.behaviour == Behaviour::Skirmish {
            return;
        }
        let keeps = matches!(m.behaviour, Behaviour::Base | Behaviour::Waves);
        if decide {
            self.scan(m);
            self.check_posts(m, keeps);
        }
        if keeps {
            self.rebuild(m);
            self.revive(m);
        }
        self.advance_training(m);
        self.lines(m);
        if decide {
            if keeps {
                self.replace(m);
            }
            self.steer_waves(m);
            self.defend(m);
        }
    }

    fn activate(&mut self, m: &mut Memory) {
        m.activated = true;
        for (state, line) in m.lines.iter_mut().zip(&self.plan.ai.waves) {
            if state.next.is_none() {
                state.next = line.after.map(|a| m.since + seconds(a));
            }
        }
    }

    // Shared helpers.

    fn find(&self, id: &str) -> Option<Def> {
        self.ctx.catalog.find(id)
    }

    /// A living unit of ours on the map.
    fn own_unit(&self, id: EntityId) -> Option<&Entity> {
        self.ctx
            .state
            .entities
            .get(id)
            .filter(|e| e.owner == self.me && e.unit.is_some() && living(e))
    }

    fn enemy(&self, owner: Owner) -> bool {
        matches!(owner, Owner::Player(_)) && self.teams.relation(self.me, owner) == Relation::Enemy
    }

    /// A free spot for a body of `def` near `at`, avoiding `taken`.
    fn spot_near(&self, def: Def, at: Milli, taken: &[Spot]) -> Option<Spot> {
        let (x, y) = at.cell();
        let body = self.ctx.spatial.profiles.get(def).body;
        let accept = |s: Spot| !taken.contains(&s);
        self.ctx.spatial.nearest(
            Spot::new(x, y, None),
            12,
            None,
            Some(&body),
            Some(&accept),
            None,
        )
    }

    /// A new unit of ours at `spot`, taking part from the next tick.
    fn spawn_unit(&mut self, def: Def, spot: Spot) -> EntityId {
        let surface = spot.deck.map(|d| self.ctx.spatial.deck_name(d));
        let id = crate::spawn::insert(
            self.ctx.state,
            self.ctx.catalog,
            crate::spawn::Spawn {
                definition: def,
                owner: self.me,
                position: Milli {
                    x: spot.x * Milli::CELL,
                    y: spot.y * Milli::CELL,
                },
                surface,
                rotation: 0,
                placement: None,
                initial: Default::default(),
                complete: true,
                ready_tick: self.tick + 1,
            },
        );
        self.ctx.index.sync(self.ctx.state, self.ctx.catalog);
        id
    }

    /// Orders a unit to `to`: an attack-move, or a plain walk that ignores what it meets.
    fn send(&mut self, id: EntityId, to: Milli, attack_move: bool) -> Option<(i32, i32)> {
        let (x, y) = to.cell();
        let order = orders::Order::Move {
            destination: Place::new(x, y),
            attack_move,
        };
        orders::issue(self.ctx.state, id, order, false).then_some((x, y))
    }

    /// Where a wave aimed at `target` goes from `from`: a region's centre, an enemy unit's
    /// position, or the side of an enemy building facing `from` (a building's centre lies
    /// inside its footprint, where no route ends).
    fn resolve(&self, target: &WaveTarget, from: Milli) -> Option<Milli> {
        match target {
            WaveTarget::Region(id) => self.campaign.region(id),
            WaveTarget::Nearest => self
                .nearest_enemy(from, |e| e.unit.is_some())
                .map(|e| e.position)
                .or_else(|| self.resolve(&WaveTarget::Base, from)),
            WaveTarget::Base => self.base(from).map(|e| self.approach(e, from)).or_else(|| {
                self.nearest_enemy(from, |e| e.unit.is_some())
                    .map(|e| e.position)
            }),
        }
    }

    /// The enemy base a wave marches on: the nearest living main fort (a player start's
    /// `mainFort`, else any building of a race's fort line: its starting fort and what it
    /// upgrades into), else the nearest enemy building. Never a unit.
    fn base(&self, from: Milli) -> Option<&Entity> {
        let started = self
            .campaign
            .forts
            .iter()
            .filter(|(o, _)| self.enemy(*o))
            .filter_map(|(_, placement)| {
                self.ctx
                    .state
                    .entities
                    .iter()
                    .find(|e| e.placement.as_deref() == Some(placement.as_str()) && living(e))
            })
            .filter(|e| self.enemy(e.owner))
            .min_by_key(|e| (distance2(e.position, from), e.id));
        let forts = self.fort_line();
        started
            .or_else(|| {
                self.nearest_enemy(from, |e| {
                    e.unit.is_none() && forts.contains(&self.ctx.catalog.id(e.definition))
                })
            })
            .or_else(|| self.nearest_enemy(from, |e| e.unit.is_none()))
    }

    /// Every race's main fort and the buildings it upgrades into.
    fn fort_line(&self) -> Vec<&Id> {
        let content = &self.ctx.catalog.content;
        let rules = &content.rules;
        let mut out: Vec<&Id> = Vec::new();
        let setups = std::iter::once(&rules.starting_setup).chain(
            rules
                .races
                .values()
                .filter_map(|r| r.starting_setup.as_ref()),
        );
        for setup in setups {
            let mut next = Some(&setup.fort);
            while let Some(id) = next.filter(|id| !out.contains(id)) {
                out.push(id);
                next = content
                    .definition(id.as_str())
                    .and_then(|d| d.upgrade.as_ref())
                    .map(|u| &u.target);
            }
        }
        out
    }

    /// The point just outside `building`'s footprint nearest `from`.
    fn approach(&self, building: &Entity, from: Milli) -> Milli {
        let d = self.ctx.catalog.get(building.definition);
        let Some(f) = &d.footprint else {
            return building.position;
        };
        // Half the longer side, so any rotation stays inside, and two cells of room.
        let reach = (f.width.max(f.depth) as i32 * Milli::CELL) / 2 + 2 * Milli::CELL;
        let p = building.position;
        Milli {
            x: p.x + (from.x - p.x).clamp(-reach, reach),
            y: p.y + (from.y - p.y).clamp(-reach, reach),
        }
    }

    /// The nearest living enemy entity that `filter` accepts (units or buildings).
    fn nearest_enemy(&self, from: Milli, filter: impl Fn(&Entity) -> bool) -> Option<&Entity> {
        let index = &self.ctx.index;
        index
            .units
            .iter()
            .chain(&index.buildings)
            .filter_map(|&id| self.ctx.state.entities.get(id))
            .filter(|e| {
                self.enemy(e.owner)
                    && living(e)
                    && e.hp.is_some_and(|hp| hp > 0)
                    && e.ready_tick <= self.tick
                    && e.unit.as_ref().is_none_or(|u| u.contained.is_none())
                    && filter(e)
            })
            .min_by_key(|e| (distance2(e.position, from), e.id))
    }
}

/// Alive and not a fallen hero.
fn living(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0) && !crate::heroes::fallen(e)
}

fn distance2(a: Milli, b: Milli) -> i64 {
    let (dx, dy) = (i64::from(a.x - b.x), i64::from(a.y - b.y));
    dx * dx + dy * dy
}

fn within(a: Milli, b: Milli, cells: i64) -> bool {
    distance2(a, b) <= (cells * i64::from(Milli::CELL)).pow(2)
}

fn milli_cell(x: i32, y: i32) -> Milli {
    Milli {
        x: x * Milli::CELL,
        y: y * Milli::CELL,
    }
}

/// Authored seconds in ticks.
fn seconds(value: f64) -> u32 {
    (value * f64::from(crate::mission::TICKS_PER_SECOND)).round() as u32
}
