//! Vision, fog of war and knowledge (step 11 of the tick; old `observation.ts`,
//! `visionMask.ts`).
//!
//! **Saved** ([`Knowledge`] in `State::knowledge`): per team, the fog cells (0 unexplored,
//! 1 explored, 2 in sight) and the stationary entities it remembers; per owner, the deaths
//! it witnessed; temporary sight sources from abilities; recent death cues.
//!
//! **Derived** ([`Vision`], rebuilt on load and checked by [`Vision::verify`]): every
//! sensor with its footprint, per-team coverage counts, the stationary entities on each
//! fog node and how many visible nodes each team has of them.
//!
//! # Following movement
//!
//! Vision reads entity positions; nothing has to tell it that something moved. Every
//! [`Vision::sync`] compares each sensor's position, height, radius and team with the
//! last pass and updates only the coverage that changed, so units moved by movement,
//! charges, teleports, scripts or tests are followed alike. `World::step` syncs in step 11;
//! a system that asks exact sight questions ([`Sight::visible`]) after moving units in the
//! same tick calls [`Vision::sync`] first.
//!
//! The old game kept a second, rounded cell position per entity next to the exact one and
//! fed sight only from it, so every movement path had to update both (a charge floored it
//! instead of rounding, so sight sat a cell off). Here there is one position.
//!
//! # Hooks for later steps
//!
//! - Garrisons, containment, forms, concealment and detection: [`SightRules`].
//! - Spell vision: [`add_spell_vision`], [`end_spell_visions`].
//! - Deaths: [`Vision::record_death`] before a dead unit is removed.

pub mod index;
pub mod knowledge;
pub mod mask;
pub mod rules;
pub mod scenery;
pub mod sensor;
pub mod sight;
pub mod statics;

pub use knowledge::{
    DeathCue, EXPLORED, FogCells, Knowledge, ObservedDeath, OwnerKnowledge, Sighting, SpellVision,
    TeamKnowledge, UNEXPLORED, VISIBLE,
};
pub use mask::FogStep;
pub use rules::{Current, SightRules};
pub use scenery::Scenery;
pub use sight::Sight;

use crate::{
    catalog::Catalog,
    entity::{EntityId, Milli},
    setup::MatchSetup,
    state::State,
    terrain::{DeckId, SurfacePoint, Terrain, round_half_up},
};
use canopy_content::{map::Exploration, placement::Owner};
use index::SectorIndex;
use mask::Mask;
use scenery::Chunks;
use sensor::{Sensor, SensorKey, Spans, Spec};
use statics::{StaticRecord, Statics};
use std::{collections::BTreeSet, iter::Peekable, sync::Arc, vec::IntoIter};

/// Who shares sight: the teams of the match (a slot's `team`, or its player index without
/// one, as the old hostility rule compared them).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Teams {
    /// `(player, team)` by player.
    slots: Vec<(u8, u8)>,
    /// Distinct teams of the slots, ascending: knowledge index order.
    teams: Vec<u8>,
    /// Slot owners, ascending.
    owners: Vec<Owner>,
}

impl Teams {
    pub fn new(setup: &MatchSetup) -> Self {
        let mut slots: Vec<(u8, u8)> = setup
            .slots
            .iter()
            .map(|s| (s.player, s.team.unwrap_or(s.player)))
            .collect();
        slots.sort_unstable();
        let mut teams: Vec<u8> = slots.iter().map(|s| s.1).collect();
        teams.sort_unstable();
        teams.dedup();
        let owners = slots.iter().map(|s| Owner::Player(s.0)).collect();
        Self {
            slots,
            teams,
            owners,
        }
    }

    pub fn team_of(&self, owner: Owner) -> Option<u8> {
        let Owner::Player(player) = owner else {
            return None;
        };
        Some(
            self.slots
                .binary_search_by_key(&player, |s| s.0)
                .map_or(player, |i| self.slots[i].1),
        )
    }

    /// Index of the owner's team among [`Teams::teams`].
    pub fn index(&self, owner: Owner) -> Option<usize> {
        let team = self.team_of(owner)?;
        self.teams.binary_search(&team).ok()
    }

    pub fn teams(&self) -> &[u8] {
        &self.teams
    }

    /// The players of the match.
    pub fn owners(&self) -> &[Owner] {
        &self.owners
    }

    /// Same owner, or players on one team: they share all ordinary sight.
    pub fn allied(&self, a: Owner, b: Owner) -> bool {
        a == b || (a != Owner::Nobody && b != Owner::Nobody && self.team_of(a) == self.team_of(b))
    }

    fn bit(&self, owner: Owner) -> u16 {
        self.index(owner).map_or(0, |i| 1 << i)
    }
}

/// The derived side of vision; see the module documentation.
#[derive(Debug, Clone)]
pub struct Vision {
    size: u32,
    nodes: usize,
    teams: Teams,
    ground: sensor::Ground,
    decks: Vec<[f64; 4]>,
    deck_cells: Arc<[u32]>,
    /// By key.
    sensors: Vec<Sensor>,
    index: SectorIndex,
    masks: Vec<Mask>,
    statics: Statics,
    /// Per team: visible footprint nodes of each stationary entity, by entity id.
    coverage: Vec<Vec<u16>>,
    /// Per team: remembered entities whose place changed since; forgotten once their
    /// remembered footprint is back in sight.
    stale: Vec<BTreeSet<EntityId>>,
    /// Live entities other than scenery, by id: what views go through.
    actors: Vec<EntityId>,
    /// Per team, then one for observers.
    scenery: Vec<Chunks>,
    /// Statics added, removed, moved or changed into another definition this pass, by id.
    reshaped: Vec<EntityId>,
    /// Statics that only look different this pass, by id.
    restyled: Vec<EntityId>,
}

impl Vision {
    /// Starts knowledge for a new match and observes the starting position.
    pub fn new(
        state: &mut State,
        catalog: &Catalog,
        terrain: &Terrain,
        setup: &MatchSetup,
        exploration: Exploration,
        rules: &dyn SightRules,
    ) -> Self {
        let teams = Teams::new(setup);
        let nodes = node_count(terrain);
        let explored = exploration == Exploration::Explored;
        let mut team = TeamKnowledge {
            team: 0,
            cells: FogCells::filled(nodes, if explored { EXPLORED } else { UNEXPLORED }),
            scenery: Default::default(),
            structures: Default::default(),
        };
        if explored {
            // Surveyed once: the neutral resources. Units and others' buildings are not.
            for e in state.entities.iter() {
                if e.owner == Owner::Nobody
                    && e.resource.is_some()
                    && e.unit.is_none()
                    && statics::alive(e)
                {
                    let memory = if statics::scenery(catalog, e) {
                        &mut team.scenery
                    } else {
                        &mut team.structures
                    };
                    memory.insert(e.id, Sighting::of(e));
                }
            }
        }
        state.knowledge = Knowledge {
            teams: teams
                .teams()
                .iter()
                .map(|&t| TeamKnowledge {
                    team: t,
                    ..team.clone()
                })
                .collect(),
            owners: teams
                .owners()
                .iter()
                .map(|&owner| OwnerKnowledge {
                    owner,
                    deaths: Vec::new(),
                })
                .collect(),
            spells: Vec::new(),
            cues: Vec::new(),
        };
        let mut vision = Self::derive(state, catalog, terrain, teams, rules);
        // The starting sight: everything covered is seen for the first time.
        for (t, team) in state.knowledge.teams.iter_mut().enumerate() {
            for record in &vision.statics.records {
                let id = record.sighting.id;
                if vision.coverage[t][id.0 as usize] == 0 {
                    continue;
                }
                let (memory, other) = if record.scenery {
                    (&mut team.scenery, &mut team.structures)
                } else {
                    (&mut team.structures, &mut team.scenery)
                };
                memory.insert(id, record.sighting.clone());
                other.remove(&id);
                vision.scenery[t].touch(id);
            }
        }
        vision.rebuild_scenery(state);
        vision
    }

    /// Rebuilds the derived side for saved knowledge, after checking it fits the match.
    pub fn restore(
        state: &mut State,
        catalog: &Catalog,
        terrain: &Terrain,
        setup: &MatchSetup,
        rules: &dyn SightRules,
    ) -> Result<Self, String> {
        let teams = Teams::new(setup);
        check(
            &state.knowledge,
            &teams,
            node_count(terrain),
            catalog,
            state,
        )?;
        Ok(Self::derive(state, catalog, terrain, teams, rules))
    }

    /// Everything derived, from scratch. Fog cells are made to agree with the coverage.
    fn derive(
        state: &mut State,
        catalog: &Catalog,
        terrain: &Terrain,
        teams: Teams,
        rules: &dyn SightRules,
    ) -> Self {
        let nodes = node_count(terrain);
        let deck_cells: Arc<[u32]> = terrain.surfaces().map_or_else(
            || Arc::from([]),
            |s| {
                (s.size() * s.size()..s.node_count())
                    .map(|id| s.node(id).expect("deck node").cell)
                    .collect()
            },
        );
        let count = teams.teams().len();
        let mut vision = Self {
            size: terrain.size(),
            nodes,
            ground: sensor::Ground::new(terrain.size(), terrain.heights()),
            decks: sensor::deck_bounds(terrain),
            deck_cells,
            sensors: Vec::new(),
            index: SectorIndex::new(terrain.size()),
            masks: (0..count).map(|_| Mask::new(nodes)).collect(),
            statics: Statics::new(nodes),
            coverage: vec![Vec::new(); count],
            stale: vec![BTreeSet::new(); count],
            actors: Vec::new(),
            scenery: vec![Chunks::default(); count + 1],
            reshaped: Vec::new(),
            restyled: Vec::new(),
            teams,
        };
        vision.sync(state, catalog, terrain, rules);
        for (mask, knowledge) in vision.masks.iter_mut().zip(&mut state.knowledge.teams) {
            mask.normalize(&mut knowledge.cells);
        }
        vision.scan(state, catalog, terrain);
        vision.reshaped.clear();
        vision.restyled.clear();
        let ids = state.counters.entity as usize;
        for (t, knowledge) in state.knowledge.teams.iter().enumerate() {
            let coverage = &mut vision.coverage[t];
            coverage.resize(ids, 0);
            for record in &vision.statics.records {
                coverage[record.sighting.id.0 as usize] =
                    visible_nodes(&record.nodes, &knowledge.cells);
            }
            for (id, remembered) in knowledge.scenery.iter().chain(&knowledge.structures) {
                if vision
                    .statics
                    .get(*id)
                    .is_none_or(|r| !r.sighting.same_place(remembered))
                {
                    vision.stale[t].insert(*id);
                }
            }
        }
        for chunks in &mut vision.scenery {
            chunks.touch_all(state.counters.entity);
        }
        vision.rebuild_scenery(state);
        vision
    }

    pub fn teams(&self) -> &Teams {
        &self.teams
    }

    /// Ground cells per side.
    pub fn size(&self) -> u32 {
        self.size
    }

    /// Fog nodes per team: ground cells, then bridge-deck nodes.
    pub fn nodes(&self) -> usize {
        self.nodes
    }

    /// The ground cell under each bridge-deck fog node, in node order after the ground.
    pub fn deck_cells(&self) -> &Arc<[u32]> {
        &self.deck_cells
    }

    /// Live entities other than neutral scenery, by id.
    pub fn actors(&self) -> &[EntityId] {
        &self.actors
    }

    /// The fog revision of the owner's team and the steps that led to it.
    pub fn fog_history(&self, owner: Owner) -> Option<(u32, &Arc<[FogStep]>)> {
        let mask = &self.masks[self.teams.index(owner)?];
        Some((mask.revision, &mask.history))
    }

    /// Scenery known to the owner's team; `None` (observer) for all of it.
    pub fn scenery(&self, owner: Option<Owner>) -> Scenery {
        let index = match owner {
            None => self.teams.teams().len(),
            Some(owner) => match self.teams.index(owner) {
                Some(index) => index,
                None => return Scenery::default(),
            },
        };
        self.scenery[index].list.clone()
    }

    pub fn sensors(&self) -> &[Sensor] {
        &self.sensors
    }

    fn sensor(&self, key: SensorKey) -> Option<&Sensor> {
        self.sensors
            .binary_search_by_key(&key, |s| s.key)
            .ok()
            .map(|i| &self.sensors[i])
    }

    /// Step 11: follow every sensor, turn coverage changes into fog, and refresh what each
    /// team remembers.
    pub fn update(
        &mut self,
        state: &mut State,
        catalog: &Catalog,
        terrain: &Terrain,
        rules: &dyn SightRules,
    ) {
        let _span = tracing::trace_span!("vision").entered();
        let tick = state.tick;
        let knowledge = &mut state.knowledge;
        knowledge.spells.retain(|s| s.expires > tick);
        knowledge
            .cues
            .retain(|c| tick.saturating_sub(c.tick) <= knowledge::DEATH_CUE_TICKS);
        for owner in &mut knowledge.owners {
            owner
                .deaths
                .retain(|d| tick.saturating_sub(d.tick) <= knowledge::DEATH_MEMORY_TICKS);
        }
        self.sync(state, catalog, terrain, rules);
        self.scan(state, catalog, terrain);
        let ids = state.counters.entity as usize;
        for t in 0..self.masks.len() {
            let team = &mut state.knowledge.teams[t];
            let changed = self.masks[t].settle(&mut team.cells);
            if self.coverage[t].len() < ids {
                self.coverage[t].resize(ids, 0);
            }
            let scenery_changed = Observe {
                statics: &self.statics,
                coverage: &mut self.coverage[t],
                stale: &mut self.stale[t],
                reshaped: &self.reshaped,
                restyled: &self.restyled,
                catalog,
                terrain,
            }
            .run(team, &changed);
            for id in scenery_changed {
                self.scenery[t].touch(id);
            }
        }
        self.reshaped.clear();
        self.restyled.clear();
        self.rebuild_scenery(state);
    }

    /// Brings every sensor up to date with the entities and spell visions: new, moved,
    /// raised, re-teamed and removed sensors change only the coverage that differs. Fog
    /// cells follow at the next [`Vision::update`].
    pub fn sync(
        &mut self,
        state: &State,
        catalog: &Catalog,
        terrain: &Terrain,
        rules: &dyn SightRules,
    ) {
        let _span = tracing::trace_span!("vision sensors").entered();
        let mut old = std::mem::take(&mut self.sensors).into_iter().peekable();
        let mut next = Vec::with_capacity(old.len());
        for e in state.entities.iter() {
            if !statics::alive(e) || rules.blind(e) {
                continue;
            }
            let radius = rules.vision(catalog, e);
            if radius == 0 {
                continue;
            }
            let deck = deck_of(terrain, e.surface.as_deref());
            let origin = statics::anchor(e);
            let mut elevation = rules.lift_cm(e);
            if let Some(flight) = rules.flight_cm(catalog, e) {
                elevation += flight + air_gap_cm(terrain, origin, deck);
            }
            let mut teams = self.teams.bit(e.owner);
            rules.lends_sight(e, &mut |owner| teams |= self.teams.bit(owner));
            let spec = Spec {
                owner: e.owner,
                origin,
                deck,
                elevation,
                radius: radius.saturating_mul(1000),
                ignore_terrain: false,
                teams,
            };
            self.follow(SensorKey::Entity(e.id), spec, &mut old, &mut next, terrain);
        }
        for v in state
            .knowledge
            .spells
            .iter()
            .filter(|v| v.expires > state.tick)
        {
            let spec = Spec {
                owner: v.owner,
                origin: v.point,
                deck: deck_of(terrain, v.surface.as_deref()),
                elevation: v.elevation,
                radius: v.radius,
                ignore_terrain: v.ignore_terrain,
                teams: self.teams.bit(v.owner),
            };
            self.follow(SensorKey::Spell(v.id), spec, &mut old, &mut next, terrain);
        }
        for gone in old {
            self.retire(gone);
        }
        self.sensors = next;
    }

    fn follow(
        &mut self,
        key: SensorKey,
        spec: Spec,
        old: &mut Peekable<IntoIter<Sensor>>,
        next: &mut Vec<Sensor>,
        terrain: &Terrain,
    ) {
        while let Some(gone) = old.next_if(|s| s.key < key) {
            self.retire(gone);
        }
        let sensor = match old.next_if(|s| s.key == key) {
            Some(sensor) if sensor.spec == spec => sensor,
            Some(sensor) => {
                let footprint = if same_shape(&sensor.spec, &spec) {
                    sensor.footprint.clone()
                } else {
                    sensor::footprint(terrain, &self.ground, &self.decks, &spec)
                };
                self.cover(
                    Some((sensor.spec.teams, &sensor.footprint)),
                    Some((spec.teams, &footprint)),
                );
                let (before, after) = (
                    self.index.circle(sensor.spec.origin, sensor.spec.radius),
                    self.index.circle(spec.origin, spec.radius),
                );
                if before != after {
                    self.index.remove(key, before);
                    self.index.insert(key, after);
                }
                Sensor {
                    key,
                    spec,
                    footprint,
                }
            }
            None => {
                let footprint = sensor::footprint(terrain, &self.ground, &self.decks, &spec);
                self.cover(None, Some((spec.teams, &footprint)));
                self.index
                    .insert(key, self.index.circle(spec.origin, spec.radius));
                Sensor {
                    key,
                    spec,
                    footprint,
                }
            }
        };
        next.push(sensor);
    }

    fn retire(&mut self, sensor: Sensor) {
        self.cover(Some((sensor.spec.teams, &sensor.footprint)), None);
        self.index.remove(
            sensor.key,
            self.index.circle(sensor.spec.origin, sensor.spec.radius),
        );
    }

    /// Moves a footprint's coverage from `old` to `new` in every team that shares it.
    fn cover(&mut self, old: Option<(u16, &Spans)>, new: Option<(u16, &Spans)>) {
        let (old_teams, old_spans): (u16, &[(u32, u32)]) = old.map_or((0, &[]), |(t, s)| (t, s));
        let (new_teams, new_spans): (u16, &[(u32, u32)]) = new.map_or((0, &[]), |(t, s)| (t, s));
        for (t, mask) in self.masks.iter_mut().enumerate() {
            let bit = 1 << t;
            let from: &[(u32, u32)] = if old_teams & bit != 0 { old_spans } else { &[] };
            let to: &[(u32, u32)] = if new_teams & bit != 0 { new_spans } else { &[] };
            if !from.is_empty() || !to.is_empty() {
                sensor::diff(from, to, |node, delta| mask.change(node, delta));
            }
        }
    }

    /// Finds stationary entities that appeared, left, moved or changed, and lists actors.
    fn scan(&mut self, state: &State, catalog: &Catalog, terrain: &Terrain) {
        let mut old = std::mem::take(&mut self.statics.records)
            .into_iter()
            .peekable();
        let mut next: Vec<StaticRecord> = Vec::with_capacity(old.len());
        let mut nodes = Vec::new();
        let observer = self.teams.teams().len();
        self.actors.clear();
        for e in state.entities.iter() {
            if !statics::alive(e) {
                continue;
            }
            let scenery = statics::scenery(catalog, e);
            if !scenery {
                self.actors.push(e.id);
            }
            if !statics::stationary(catalog, e) {
                continue;
            }
            while let Some(gone) = old.next_if(|r| r.sighting.id < e.id) {
                self.unplace(gone);
            }
            let record = match old.next_if(|r| r.sighting.id == e.id) {
                Some(record) if record.scenery == scenery && record.sighting.matches(e) => record,
                Some(mut record) => {
                    let sighting = Sighting::of(e);
                    if record.sighting.same_place(&sighting) {
                        self.restyled.push(e.id);
                    } else {
                        self.statics.lift(e.id, &record.nodes);
                        statics::fog_nodes(terrain, catalog, &sighting, e.position, &mut nodes);
                        self.statics.place(e.id, &nodes);
                        record.nodes.clone_from(&nodes);
                        self.reshaped.push(e.id);
                    }
                    if scenery || record.scenery {
                        self.scenery[observer].touch(e.id);
                    }
                    record.sighting = sighting;
                    record.scenery = scenery;
                    record
                }
                None => {
                    let sighting = Sighting::of(e);
                    statics::fog_nodes(terrain, catalog, &sighting, e.position, &mut nodes);
                    self.statics.place(e.id, &nodes);
                    self.reshaped.push(e.id);
                    if scenery {
                        self.scenery[observer].touch(e.id);
                    }
                    StaticRecord {
                        sighting,
                        nodes: nodes.clone(),
                        scenery,
                    }
                }
            };
            next.push(record);
        }
        for gone in old {
            self.unplace(gone);
        }
        self.statics.records = next;
    }

    fn unplace(&mut self, record: StaticRecord) {
        let id = record.sighting.id;
        self.statics.lift(id, &record.nodes);
        self.reshaped.push(id);
        if record.scenery {
            let observer = self.teams.teams().len();
            self.scenery[observer].touch(id);
        }
    }

    fn rebuild_scenery(&mut self, state: &State) {
        let observer = self.teams.teams().len();
        for (t, chunks) in self.scenery.iter_mut().enumerate() {
            if t == observer {
                let records = &self.statics.records;
                chunks.rebuild(|from, to| {
                    let start = records.partition_point(|r| r.sighting.id < from);
                    records[start..]
                        .iter()
                        .take_while(|r| r.sighting.id < to)
                        .filter(|r| r.scenery)
                        .map(|r| r.sighting.clone())
                        .collect()
                });
            } else {
                let memory = &state.knowledge.teams[t].scenery;
                chunks.rebuild(|from, to| memory.range(from..to).map(|(_, s)| s.clone()).collect());
            }
        }
    }

    /// Records a unit's death before it is removed: a cue for everyone who saw it and an
    /// eyewitness report for each of them (old `recordDeath`). Units inside garrisons or
    /// containers die unseen.
    pub fn record_death(
        &self,
        state: &mut State,
        catalog: &Catalog,
        terrain: &Terrain,
        rules: &dyn SightRules,
        id: EntityId,
    ) {
        let Some(entity) = state.entities.get(id) else {
            return;
        };
        if entity.unit.is_none() || rules.blind(entity) {
            return;
        }
        let sight = Sight {
            vision: self,
            state: &*state,
            catalog,
            terrain,
            rules,
        };
        let viewers: Vec<Owner> = self
            .teams
            .owners()
            .iter()
            .copied()
            .filter(|&owner| sight.seen(owner, entity))
            .collect();
        let sighting = Sighting::of(entity);
        let tick = state.tick;
        for owner in &mut state.knowledge.owners {
            if viewers.contains(&owner.owner) {
                owner.deaths.push(ObservedDeath {
                    id,
                    definition: sighting.definition,
                    position: sighting.position,
                    tick,
                });
                if owner.deaths.len() > knowledge::MAX_OBSERVED_DEATHS {
                    owner.deaths.remove(0);
                }
            }
        }
        state.knowledge.cues.push(DeathCue {
            tick,
            sighting,
            viewers,
        });
    }

    /// Rebuilds everything derived from `state` and reports the first difference from
    /// the incrementally maintained data, and any fog cell or remembered entity that
    /// disagrees with the coverage. For tests and debugging.
    pub fn verify(
        &self,
        state: &State,
        catalog: &Catalog,
        terrain: &Terrain,
        rules: &dyn SightRules,
    ) -> Result<(), String> {
        let mut copy = state.clone();
        let fresh = Self::derive(&mut copy, catalog, terrain, self.teams.clone(), rules);
        if copy.knowledge != state.knowledge {
            return Err("fog cells disagree with sensor coverage".into());
        }
        if fresh.sensors != self.sensors {
            return Err("sensors differ".into());
        }
        if fresh.index.contents() != self.index.contents() {
            return Err("sensor index differs".into());
        }
        for (t, mask) in self.masks.iter().enumerate() {
            // Counted straight from the footprints, not through footprint differences.
            let mut counts = vec![0u16; self.nodes];
            for sensor in fresh
                .sensors
                .iter()
                .filter(|s| s.spec.teams & (1 << t) != 0)
            {
                for &(start, end) in &sensor.footprint {
                    (start..end).for_each(|n| counts[n as usize] += 1);
                }
            }
            if counts != mask.counts() || counts != fresh.masks[t].counts() {
                return Err(format!("coverage counts of team {t} differ"));
            }
        }
        if fresh.statics != self.statics {
            return Err("stationary index differs".into());
        }
        if fresh.statics.coverage() != self.statics.coverage() {
            return Err("stationary cells differ".into());
        }
        if fresh.actors != self.actors {
            return Err("actors differ".into());
        }
        for t in 0..self.masks.len() {
            let trim = |c: &[u16]| c.iter().rposition(|&n| n > 0).map_or(0, |i| i + 1);
            let (a, b) = (&fresh.coverage[t], &self.coverage[t]);
            if a[..trim(a)] != b[..trim(b)] {
                return Err(format!("stationary coverage of team {t} differs"));
            }
            if fresh.stale[t] != self.stale[t] {
                return Err(format!("stale memories of team {t} differ"));
            }
            if fresh.scenery[t].list.chunks != self.scenery[t].list.chunks {
                return Err(format!("scenery of team {t} differs"));
            }
            let team = &state.knowledge.teams[t];
            for record in &self.statics.records {
                let id = record.sighting.id;
                if b.get(id.0 as usize).is_some_and(|&c| c > 0)
                    && team.remembered(id) != Some(&record.sighting)
                {
                    return Err(format!(
                        "team {t} sees entity {} but remembers it differently",
                        id.0
                    ));
                }
            }
        }
        let observer = self.teams.teams().len();
        if fresh.scenery[observer].list.chunks != self.scenery[observer].list.chunks {
            return Err("observer scenery differs".into());
        }
        Ok(())
    }
}

/// The per-team knowledge pass of one update (old "Scenery knowledge").
struct Observe<'a> {
    statics: &'a Statics,
    coverage: &'a mut Vec<u16>,
    stale: &'a mut BTreeSet<EntityId>,
    reshaped: &'a [EntityId],
    restyled: &'a [EntityId],
    catalog: &'a Catalog,
    terrain: &'a Terrain,
}

impl Observe<'_> {
    /// Returns the scenery ids whose memory changed.
    fn run(self, team: &mut TeamKnowledge, changed: &[u32]) -> Vec<EntityId> {
        let Self {
            statics,
            coverage,
            stale,
            reshaped,
            restyled,
            catalog,
            terrain,
        } = self;
        let mut refresh = Vec::new();
        let mut scenery_changed = Vec::new();
        // Nodes that came into or left sight.
        for &node in changed {
            let delta: i32 = if team.cells.get(node) == VISIBLE {
                1
            } else {
                -1
            };
            statics.at(node, |id| {
                // Moved footprints are recounted below against the new fog.
                if reshaped.binary_search(&id).is_ok() {
                    return;
                }
                let count = &mut coverage[id.0 as usize];
                *count = (i32::from(*count) + delta) as u16;
                if delta > 0 && *count == 1 {
                    refresh.push(id);
                }
            });
        }
        for &id in reshaped {
            let record = statics.get(id);
            let count = record.map_or(0, |r| visible_nodes(&r.nodes, &team.cells));
            coverage[id.0 as usize] = count;
            if count > 0 {
                refresh.push(id);
            }
            if let Some(remembered) = team.remembered(id) {
                if record.is_none_or(|r| !r.sighting.same_place(remembered)) {
                    stale.insert(id);
                } else {
                    stale.remove(&id);
                }
            }
        }
        refresh.extend(
            restyled
                .iter()
                .copied()
                .filter(|id| coverage[id.0 as usize] > 0),
        );
        // A remembered entity that moved or vanished is forgotten once its old place is
        // seen again.
        if !changed.is_empty() || !reshaped.is_empty() {
            let mut nodes = Vec::new();
            let forget: Vec<EntityId> = stale
                .iter()
                .copied()
                .filter(|id| coverage.get(id.0 as usize).is_none_or(|&c| c == 0))
                .filter(|&id| {
                    let remembered = team.remembered(id).expect("stale entities are remembered");
                    statics::fog_nodes(
                        terrain,
                        catalog,
                        remembered,
                        remembered.position,
                        &mut nodes,
                    );
                    nodes.iter().any(|&n| team.cells.get(n) == VISIBLE)
                })
                .collect();
            for id in forget {
                stale.remove(&id);
                if team.scenery.remove(&id).is_some() {
                    scenery_changed.push(id);
                }
                team.structures.remove(&id);
            }
        }
        refresh.sort_unstable();
        refresh.dedup();
        for id in refresh {
            if coverage[id.0 as usize] == 0 {
                continue;
            }
            let record = statics.get(id).expect("covered entities are stationary");
            let (memory, other) = if record.scenery {
                (&mut team.scenery, &mut team.structures)
            } else {
                (&mut team.structures, &mut team.scenery)
            };
            if memory.get(&id) != Some(&record.sighting) {
                memory.insert(id, record.sighting.clone());
                if record.scenery {
                    scenery_changed.push(id);
                }
            }
            if other.remove(&id).is_some() && !record.scenery {
                scenery_changed.push(id);
            }
            stale.remove(&id);
        }
        scenery_changed
    }
}

fn visible_nodes(nodes: &[u32], cells: &FogCells) -> u16 {
    nodes.iter().filter(|&&n| cells.get(n) == VISIBLE).count() as u16
}

fn same_shape(a: &Spec, b: &Spec) -> bool {
    a.origin == b.origin
        && a.deck == b.deck
        && a.elevation == b.elevation
        && a.radius == b.radius
        && a.ignore_terrain == b.ignore_terrain
}

/// Fog nodes of a map: ground cells, then bridge-deck nodes.
pub fn node_count(terrain: &Terrain) -> usize {
    terrain.surfaces().map_or_else(
        || (terrain.size() * terrain.size()) as usize,
        |s| s.node_count() as usize,
    )
}

fn deck_of(terrain: &Terrain, surface: Option<&str>) -> Option<DeckId> {
    surface.and_then(|name| terrain.deck_id(name))
}

/// How far the highest floor at a point (ground, water or deck) is above the floor a body
/// stands on there, in centimetres: aircraft fly above all of them.
pub(crate) fn air_gap_cm(terrain: &Terrain, at: Milli, deck: Option<DeckId>) -> i32 {
    let (x, y) = (f64::from(at.x) / 1000.0, f64::from(at.y) / 1000.0);
    let air = terrain.air_floor(x, y);
    let floor = terrain
        .height(SurfacePoint {
            x,
            y,
            deck,
            elevation: 0.0,
        })
        .unwrap_or(air);
    round_half_up((air - floor) * 100.0) as i32
}

/// Checks saved knowledge against the match: its teams, players, map size and content.
fn check(
    knowledge: &Knowledge,
    teams: &Teams,
    nodes: usize,
    catalog: &Catalog,
    state: &State,
) -> Result<(), String> {
    let saved: Vec<u8> = knowledge.teams.iter().map(|t| t.team).collect();
    if saved != teams.teams() {
        return Err("the saved knowledge does not match the teams".into());
    }
    let owners: Vec<Owner> = knowledge.owners.iter().map(|o| o.owner).collect();
    if owners != teams.owners() {
        return Err("the saved knowledge does not match the players".into());
    }
    let definitions = catalog.content.definitions.len();
    for team in &knowledge.teams {
        if team.cells.len() != nodes {
            return Err("the saved fog does not match the map".into());
        }
        for (id, s) in team.scenery.iter().chain(&team.structures) {
            if id.0 >= state.counters.entity || usize::from(s.definition.0) >= definitions {
                return Err("invalid remembered entity".into());
            }
        }
    }
    let spells = &knowledge.spells;
    if spells.len() > knowledge::MAX_SPELL_VISIONS
        || spells.windows(2).any(|w| w[0].id >= w[1].id)
        || spells.iter().any(|v| v.id >= state.counters.vision)
    {
        return Err("invalid spell visions".into());
    }
    Ok(())
}

/// Places a temporary sight source (the `vision` ability operation) and returns its id,
/// or `None` when [`knowledge::MAX_SPELL_VISIONS`] are already active. `spell.id` is
/// assigned here.
pub fn add_spell_vision(state: &mut State, mut spell: SpellVision) -> Option<u32> {
    if state.knowledge.spells.len() >= knowledge::MAX_SPELL_VISIONS {
        return None;
    }
    spell.id = state.counters.vision;
    state.counters.vision += 1;
    let id = spell.id;
    state.knowledge.spells.push(spell);
    Some(id)
}

/// Ends the sight sources that end with `caster` (it died or was removed).
pub fn end_spell_visions(state: &mut State, caster: EntityId) {
    state.knowledge.spells.retain(|v| v.caster != Some(caster));
}
