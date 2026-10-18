//! What one player may know at the current tick: the renderer's and HUD's only input.
//!
//! Built every frame, so it borrows nothing and copies little: fog cells and known scenery
//! are shared (`Arc`) with the simulation until they change, and carry revisions so a
//! renderer uploads only what changed. Entities other than scenery (units, buildings,
//! items) are listed afresh; there are at most a few thousand.

use crate::{
    catalog::{Catalog, Def},
    economy::{
        Delivery, Economy,
        view::{Colony, PrivateWork, PublicWork, WorkViews},
    },
    entity::{Entity, EntityId, Stock},
    mission::state::{Dialogue, ObjectiveState, Scene},
    state::{Fact, Outcome, State},
    teams::Relation,
    terrain::Terrain,
    vision::{DeathCue, FogStep, Scenery, Sight, SightRules, Sighting, VISIBLE, Vision},
};
use canopy_content::placement::Owner;
use std::{collections::BTreeMap, sync::Arc};

/// An entity as the viewer knows it.
#[derive(Debug, Clone, PartialEq)]
pub struct EntityView {
    /// Its public state: live while seen, as last seen when `remembered`.
    pub public: Sighting,
    pub relation: Relation,
    /// Out of sight: a building as it was when last seen.
    pub remembered: bool,
    /// Owner-only state; `Some` for the viewer's own entities (and for every entity in an
    /// observer's view).
    pub private: Option<Private>,
    /// How a seen unit moves; `None` for other entities and remembered ones.
    pub motion: Option<MotionView>,
    /// Carried goods, the work pose and a harvest site's workforce; `None` when there is
    /// none or the entity is remembered.
    pub work: Option<PublicWork>,
    /// Stats, swing, charge and lookout of a seen body (`combat`).
    pub combat: Option<crate::combat::CombatView>,
    /// Statuses, casting, mana, ranks and items of a seen unit (`abilities`).
    pub magic: Option<crate::abilities::view::MagicView>,
}

/// Movement a renderer animates.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MotionView {
    /// It moved during the latest tick.
    pub moving: bool,
    /// It is on an idle stroll (walking pace).
    pub strolling: bool,
}

impl MotionView {
    fn of(entity: &Entity, tick: u32) -> Option<Self> {
        let unit = entity.unit.as_ref()?;
        Some(Self {
            // The state's tick counts completed steps, so the latest step was `tick − 1`.
            moving: unit.last_moved_tick.is_some_and(|t| t + 1 == tick),
            strolling: unit.idle.as_ref().is_some_and(|i| i.walking),
        })
    }
}

impl EntityView {
    pub fn id(&self) -> EntityId {
        self.public.id
    }

    pub fn definition(&self) -> Def {
        self.public.definition
    }
}

/// What only an entity's owner knows about it. Orders, jobs, production and abilities
/// join as their systems arrive.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Private {
    pub inventory: Stock,
    /// The current order and those queued behind it.
    pub order: Option<crate::orders::Order>,
    pub queued: Vec<crate::orders::Order>,
    /// Job, workplace, production, research, upgrade and revival state.
    pub economy: Box<PrivateWork>,
    /// A unit held inside another entity (a training building, a lookout, a cage).
    pub contained: bool,
    /// A unit waiting to be placed back on the map after leaving its container.
    pub releasing: bool,
    /// A unit under a stun.
    pub stunned: bool,
}

/// The viewer's fog of war.
#[derive(Debug, Clone)]
pub struct FogView {
    /// Ground cells per side.
    pub size: u32,
    /// Advances whenever a cell changes; restarts with the world (a renderer resets when
    /// it gets a new world).
    pub revision: u32,
    /// 0 unexplored, 1 explored, 2 in sight per node: `size²` ground cells, then one per
    /// bridge-deck node (see `deck_cells`). Shared with the simulation until it changes.
    pub cells: Arc<Vec<u8>>,
    /// The ground cell under each deck node.
    pub deck_cells: Arc<[u32]>,
    history: Arc<[FogStep]>,
}

impl FogView {
    /// A ground cell, in sight when any floor above it is (the flat minimap's union).
    pub fn flat(&self, cell: u32) -> u8 {
        let ground = self.cells.get(cell as usize).copied().unwrap_or(0);
        let decks = (self.size * self.size) as usize;
        self.deck_cells
            .iter()
            .enumerate()
            .filter(|&(_, &c)| c == cell)
            .map(|(i, _)| self.cells[decks + i])
            .fold(ground, u8::max)
    }

    /// Whether the node is in sight.
    pub fn visible(&self, node: u32) -> bool {
        self.cells.get(node as usize) == Some(&VISIBLE)
    }

    /// The nodes that changed since `revision`, possibly repeated, in order; `None` when
    /// that revision is too old to patch from (upload everything).
    /// The same fog with other `cells` under another `revision` and without change
    /// history, so a consumer that last saw any other revision reloads it whole. For
    /// presentation only (a mission scene's revealed set); the simulation never sees it.
    pub fn restaged(&self, cells: Vec<u8>, revision: u32) -> Self {
        Self {
            size: self.size,
            revision,
            cells: Arc::new(cells),
            deck_cells: self.deck_cells.clone(),
            history: Arc::from(Vec::new()),
        }
    }

    pub fn changes_since(&self, revision: u32) -> Option<Vec<u32>> {
        if revision == self.revision {
            return Some(Vec::new());
        }
        let first = self.history.first()?;
        if revision > self.revision || first.revision > revision + 1 {
            return None;
        }
        Some(
            self.history
                .iter()
                .filter(|s| s.revision > revision)
                .flat_map(|s| s.nodes.iter().copied())
                .collect(),
        )
    }
}

/// Campaign presentation from the mission state.
#[derive(Debug, Clone, PartialEq)]
pub struct MissionView {
    pub objective: String,
    pub objectives: BTreeMap<String, ObjectiveState>,
    pub dialogue: Option<Dialogue>,
    pub scene: Option<Scene>,
    /// A cinematic line holds the game still.
    pub paused: bool,
    /// The script stopped with this error.
    pub error: Option<String>,
}

#[derive(Debug, Clone)]
pub struct View {
    pub tick: u32,
    /// `None` for an observer, who sees everything.
    pub owner: Option<Owner>,
    /// Units, buildings, items and owned resources the viewer sees, or remembers under
    /// fog, in id order.
    pub entities: Vec<EntityView>,
    /// Neutral harvestable scenery as the viewer knows it (live where in sight).
    pub scenery: Scenery,
    /// `None` for observers and owners outside the match.
    pub fog: Option<FogView>,
    /// The viewer's facts (all of them for an observer), oldest first.
    pub facts: Vec<Fact>,
    /// Deaths the viewer saw in the last few seconds.
    pub deaths: Vec<DeathCue>,
    /// Main forts by owner: the viewer's own, or everyone's for an observer.
    pub objectives: BTreeMap<Owner, u32>,
    pub mission: Option<MissionView>,
    pub outcome: Option<Outcome>,
    /// The viewer's goods, supply, workers and research; `None` for observers.
    pub colony: Option<Colony>,
    /// Goods delivered to the viewer's drop-offs this tick (all of them for observers).
    pub deliveries: Vec<Delivery>,
    /// Projectiles in flight, corpses and fallen heroes (`combat`).
    pub battle: crate::combat::Battle,
    /// What each AI player is doing (observers only, for the debug overlay).
    pub ai: Vec<crate::ai::Summary>,
    /// Ability events, deliveries in flight and hero returns (`abilities`).
    pub spells: crate::abilities::view::Spellcraft,
    /// Recent pings of the viewer's team (everyone's for an observer).
    pub pings: Vec<crate::state::Ping>,
    /// Players out of the match so far, for everyone (filled by the world).
    pub defeated: Vec<Owner>,
}

impl View {
    /// What `owner` knows.
    #[allow(clippy::too_many_arguments)]
    pub fn build(
        state: &State,
        catalog: &Catalog,
        terrain: &Terrain,
        vision: &Vision,
        rules: &dyn SightRules,
        economy: &Economy,
        owner: Owner,
    ) -> Self {
        let _span = tracing::trace_span!("view").entered();
        let works = WorkViews::new(state, catalog, economy);
        let sight = Sight {
            vision,
            state,
            catalog,
            terrain,
            rules,
        };
        let teams = vision.teams();
        let relation = |other: Owner| teams.relation(owner, other);
        let knowledge = sight.knowledge(owner);
        let mut live = vision
            .actors()
            .iter()
            .filter_map(|&id| state.entities.get(id))
            .filter(|e| sight.seen(owner, e))
            .map(|e| EntityView {
                public: Sighting::of(e),
                relation: relation(e.owner),
                remembered: false,
                private: (e.owner == owner).then(|| private(state, catalog, economy, &works, e)),
                motion: MotionView::of(e, state.tick),
                work: crate::economy::view::public(
                    state,
                    catalog,
                    &works,
                    e,
                    crate::economy::view::at_goal(terrain, e),
                ),
                combat: None,
                magic: None,
            })
            .peekable();
        // Remembered structures stand in where nothing with their id is seen.
        let mut remembered = knowledge
            .into_iter()
            .flat_map(|k| k.structures.values())
            .peekable();
        let mut entities = Vec::new();
        loop {
            let next_live = live.peek().map(EntityView::id);
            let next_memory = remembered.peek().map(|s| s.id);
            match (next_live, next_memory) {
                (None, None) => break,
                (Some(a), Some(b)) if a == b => {
                    remembered.next();
                    entities.extend(live.next());
                }
                (Some(a), Some(b)) if a < b => entities.extend(live.next()),
                (Some(_), None) => entities.extend(live.next()),
                _ => {
                    let s = remembered.next().expect("peeked");
                    entities.push(remembered_view(s, relation(s.owner)));
                }
            }
        }
        // Fallen heroes leave the world but stay the owner's: the card revives them, the
        // portraits and the roster count them (old roster). Vision no longer tracks them.
        let missing: Vec<EntityView> = state
            .entities
            .iter()
            .filter(|e| e.fallen && e.owner == owner)
            .filter(|e| {
                entities
                    .binary_search_by_key(&e.id, EntityView::id)
                    .is_err()
            })
            .map(|e| EntityView {
                public: Sighting::of(e),
                relation: relation(e.owner),
                remembered: false,
                private: Some(private(state, catalog, economy, &works, e)),
                motion: MotionView::of(e, state.tick),
                work: None,
                combat: None,
                magic: None,
            })
            .collect();
        if !missing.is_empty() {
            entities.extend(missing);
            entities.sort_by_key(EntityView::id);
        }
        let fog = knowledge.and_then(|k| {
            let (revision, history) = vision.fog_history(owner)?;
            Some(FogView {
                size: vision.size(),
                revision,
                cells: k.cells.0.clone(),
                deck_cells: vision.deck_cells().clone(),
                history: history.clone(),
            })
        });
        let pings = state
            .pings
            .iter()
            .filter(|p| state.tick.saturating_sub(p.tick) < crate::state::PING_TICKS)
            .filter(|p| {
                matches!(
                    relation(p.owner),
                    crate::teams::Relation::Own | crate::teams::Relation::Ally
                )
            })
            .copied()
            .collect();
        Self {
            pings,
            defeated: Vec::new(),
            tick: state.tick,
            owner: Some(owner),
            entities,
            scenery: vision.scenery(Some(owner)),
            fog,
            facts: state
                .facts
                .iter()
                .filter(|f| f.owner == owner)
                .cloned()
                .collect(),
            deaths: state
                .knowledge
                .cues
                .iter()
                .filter(|c| c.viewers.contains(&owner))
                .cloned()
                .collect(),
            objectives: state
                .objectives
                .get(&owner)
                .map(|&fort| BTreeMap::from([(owner, fort)]))
                .unwrap_or_default(),
            mission: state.mission.as_ref().map(mission),
            outcome: state.outcome.clone(),
            colony: Some(crate::economy::view::colony(state, catalog, economy, owner)),
            deliveries: economy
                .deliveries
                .iter()
                .filter(|d| d.owner == owner)
                .cloned()
                .collect(),
            battle: crate::combat::Battle::default(),
            ai: Vec::new(),
            spells: crate::abilities::view::Spellcraft::default(),
        }
    }

    /// Everything, for spectators and replays: every live entity with its private state,
    /// no fog.
    pub fn observe(
        state: &State,
        catalog: &Catalog,
        terrain: &Terrain,
        vision: &Vision,
        economy: &Economy,
    ) -> Self {
        let works = WorkViews::new(state, catalog, economy);
        let entities = vision
            .actors()
            .iter()
            .filter_map(|&id| state.entities.get(id))
            .map(|e| EntityView {
                public: Sighting::of(e),
                relation: if e.owner == Owner::Nobody {
                    Relation::Neutral
                } else {
                    Relation::Enemy
                },
                remembered: false,
                private: Some(private(state, catalog, economy, &works, e)),
                motion: MotionView::of(e, state.tick),
                work: crate::economy::view::public(
                    state,
                    catalog,
                    &works,
                    e,
                    crate::economy::view::at_goal(terrain, e),
                ),
                combat: None,
                magic: None,
            })
            .collect();
        Self {
            defeated: Vec::new(),
            pings: state
                .pings
                .iter()
                .filter(|p| state.tick.saturating_sub(p.tick) < crate::state::PING_TICKS)
                .copied()
                .collect(),
            tick: state.tick,
            owner: None,
            entities,
            scenery: vision.scenery(None),
            fog: None,
            facts: state.facts.clone(),
            deaths: state.knowledge.cues.clone(),
            objectives: state.objectives.clone(),
            mission: state.mission.as_ref().map(mission),
            outcome: state.outcome.clone(),
            colony: None,
            deliveries: economy.deliveries.clone(),
            battle: crate::combat::Battle::default(),
            ai: Vec::new(),
            spells: crate::abilities::view::Spellcraft::default(),
        }
    }

    pub fn entity(&self, id: EntityId) -> Option<&EntityView> {
        self.entities
            .binary_search_by_key(&id, EntityView::id)
            .ok()
            .map(|i| &self.entities[i])
    }

    /// Known resources a known building placed on them (`placementOn`) stands on: they are
    /// drawn and picked as that building (a Substance Well on its deposit, D62).
    pub fn covered(&self, catalog: &Catalog) -> std::collections::BTreeSet<EntityId> {
        let mut on: Vec<(i32, i32, &canopy_content::Id)> = self
            .entities
            .iter()
            .filter_map(|e| {
                let source = &catalog.get(e.definition()).placement_on.as_ref()?.source;
                Some((e.public.position.x, e.public.position.y, source))
            })
            .collect();
        if on.is_empty() {
            return Default::default();
        }
        on.sort_unstable();
        self.entities
            .iter()
            .filter(|e| {
                e.public.resource.is_some() && {
                    let id = catalog.id(e.definition());
                    on.binary_search(&(e.public.position.x, e.public.position.y, id))
                        .is_ok()
                }
            })
            .map(EntityView::id)
            .collect()
    }
}

fn private(
    state: &State,
    catalog: &Catalog,
    economy: &Economy,
    works: &WorkViews,
    entity: &Entity,
) -> Private {
    Private {
        inventory: entity.inventory.clone(),
        order: entity.unit.as_ref().and_then(|u| u.order.clone()),
        queued: entity
            .unit
            .as_ref()
            .map_or_else(Vec::new, |u| u.order_queue.clone()),
        economy: Box::new(crate::economy::view::private(
            state, catalog, economy, works, entity,
        )),
        contained: entity.unit.as_ref().is_some_and(|u| u.contained.is_some()),
        releasing: entity.unit.as_ref().is_some_and(|u| u.release.is_some()),
        stunned: entity.unit.is_some() && crate::abilities::statuses::stunned(catalog, entity),
    }
}

fn remembered_view(sighting: &Sighting, relation: Relation) -> EntityView {
    EntityView {
        public: sighting.clone(),
        relation,
        remembered: true,
        private: None,
        motion: None,
        work: None,
        combat: None,
        magic: None,
    }
}

fn mission(m: &crate::mission::MissionState) -> MissionView {
    MissionView {
        objective: m.objective.clone(),
        objectives: m.objective_states.clone(),
        dialogue: m.dialogue.clone(),
        scene: m.scene.clone(),
        paused: m
            .dialogue
            .as_ref()
            .is_some_and(|d| d.cinematic && d.remaining > 0),
        error: m.error.clone(),
    }
}
