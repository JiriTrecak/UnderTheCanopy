//! One decision's view of the match (old `Frame` and `playerObservation` in
//! `ai/frame.ts`).
//!
//! The AI's input boundary: what its player sees and remembers, exactly as the player's
//! view lists it (own entities, others seen now through the team's fog, remembered
//! structures and known scenery), with private state readable only for its own entities.
//! It is built straight from the state through [`Sight`] instead of from a cloned
//! [`crate::view::View`]: the same entities, without copying them.

use super::AiRules;
use super::{
    geography::{Geography, Pt, dist},
    navigation::{Blocker, Mover, ObservedBlockers, ObservedNavigation},
    powers,
};
use crate::{
    catalog::{Catalog, Def},
    combat::{Senses, rules::armor_multiplier},
    economy::{Economy, Production, view::Colony},
    entity::{Entity, Milli, Resource, Unit},
    spatial::{Fixed, Profiles},
    state::State,
    stats::Stats,
    terrain::{
        DeckId,
        footprint::{Extent, footprint_cells},
    },
    vision::{FogCells, ObservedDeath, Sight, VISIBLE},
};
use canopy_content::{
    Id,
    game::{Definition, Kind},
    placement::Owner,
};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeSet,
};

/// Read-only world parts an AI decision may consult. Only [`Frame`] reads them, and only
/// within the player's knowledge.
pub struct Input<'a> {
    pub state: &'a State,
    pub catalog: &'a Catalog,
    /// The ability runtime's context, read only: perception, the economy's indexes,
    /// spatial profiles and the ability scorer.
    pub spells: &'a crate::abilities::Ctx<'a>,
}

impl<'a> Input<'a> {
    pub fn economy(&self) -> &'a Economy {
        self.spells.economy
    }

    pub fn profiles(&self) -> &'a Profiles {
        &self.spells.spatial.profiles
    }

    pub fn senses(&self) -> Senses<'a> {
        self.spells.senses
    }
}

/// An entity as the AI player knows it.
#[derive(Debug, Clone, Copy)]
pub struct Known<'a> {
    pub id: u32,
    pub def: Def,
    pub owner: Owner,
    pub p: Pt,
    /// The exact position.
    pub pos: Milli,
    /// Millidegrees.
    pub rotation: i32,
    pub hp: Option<u32>,
    /// Out of sight: as last seen.
    pub remembered: bool,
    /// Whether the player's side fights it; unknown for remembered entities.
    pub hostile: Option<bool>,
    pub construction: bool,
    pub resource: Option<&'a Resource>,
    pub item: bool,
    pub unit: bool,
    /// It moved during the latest tick.
    pub moving: bool,
    /// Inside a building or waiting to be released.
    pub contained: bool,
    /// The live entity: public data for others, everything for the player's own.
    live: Option<&'a Entity>,
}

impl<'a> Known<'a> {
    pub fn alive(&self) -> bool {
        self.hp != Some(0)
    }
}

/// A known entity's index in [`Frame::all`].
pub type Ix = usize;

pub struct Frame<'a> {
    pub input: &'a Input<'a>,
    pub geo: &'a Geography,
    pub tick: u32,
    pub me: Owner,
    pub rules: &'a AiRules,
    /// Everything known, by id.
    pub all: Vec<Known<'a>>,
    pub own: Vec<Ix>,
    pub workers: Vec<Ix>,
    pub army: Vec<Ix>,
    pub buildings: Vec<Ix>,
    pub stores: Vec<Ix>,
    pub hostiles: Vec<Ix>,
    pub resources: Vec<Ix>,
    /// Own fallen heroes, by id.
    pub fallen: Vec<&'a Entity>,
    pub home: Pt,
    /// Spendable goods.
    pub bank: Vec<(Id, u32)>,
    pub colony: Colony,
    pub researched: &'a [Id],
    pub deaths: &'a [ObservedDeath],
    cells: Option<&'a FogCells>,
    blockers: &'a RefCell<ObservedBlockers>,
    navigation: &'a RefCell<Option<ObservedNavigation>>,
    roster: RefCell<Option<Vec<Def>>>,
    blockers_ready: Cell<bool>,
}

fn camp(e: &Entity) -> Option<&str> {
    e.unit.as_ref()?.camp.as_ref().map(|c| c.id.as_str())
}

impl<'a> Frame<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        input: &'a Input<'a>,
        geo: &'a Geography,
        me: Owner,
        tick: u32,
        rules: &'a AiRules,
        blockers: &'a RefCell<ObservedBlockers>,
        navigation: &'a RefCell<Option<ObservedNavigation>>,
    ) -> Self {
        let _span = tracing::trace_span!("ai frame").entered();
        let state = input.state;
        let sight: Sight = input.spells.senses.sight(state);
        let knowledge = sight.knowledge(me);
        let cells = knowledge.map(|k| &k.cells);
        let deck = |name: Option<&String>| name.and_then(|n| geo.terrain.deck_id(n));
        let mut live: Vec<Known<'a>> = Vec::new();
        let span = tracing::trace_span!("ai frame actors").entered();
        for &id in input.senses().vision.actors() {
            let Some(e) = state.entities.get(id) else {
                continue;
            };
            if !sight.seen(me, e) {
                continue;
            }
            let u = e.unit.as_deref();
            let hostile = e.owner != me && input.senses().opponents(me, None, e.owner, camp(e));
            live.push(Known {
                id: id.0,
                def: e.definition,
                owner: e.owner,
                p: Pt::of(e.position, deck(e.surface.as_ref())),
                pos: e.position,
                rotation: e.rotation,
                hp: e.hp,
                remembered: false,
                hostile: Some(hostile),
                construction: e.construction.is_some(),
                resource: e.resource.as_ref(),
                item: e.item.is_some(),
                unit: u.is_some(),
                moving: u.is_some_and(|u| u.last_moved_tick.is_some_and(|t| t + 1 == state.tick)),
                contained: u.is_some_and(|u| u.contained.is_some() || u.release.is_some()),
                live: Some(e),
            });
        }
        drop(span);
        let span = tracing::trace_span!("ai frame memory").entered();
        let mut all: Vec<Known<'a>> = Vec::with_capacity(live.len() + 64);
        let mut live = live.into_iter().peekable();
        let remembered_known = |s: &'a crate::vision::Sighting, visible: bool| Known {
            id: s.id.0,
            def: s.definition,
            owner: s.owner,
            p: Pt::of(s.position, deck(s.surface.as_ref())),
            pos: s.position,
            rotation: s.rotation,
            hp: s.hp,
            remembered: !visible,
            hostile: visible.then_some(false),
            construction: s.construction.is_some(),
            resource: s.resource.as_ref(),
            item: s.item.is_some(),
            unit: false,
            moving: false,
            contained: false,
            live: None,
        };
        // Remembered structures stand in where nothing with their id is seen; known
        // scenery is live where in sight.
        let mut memory: Vec<Known<'a>> = Vec::new();
        if let Some(k) = knowledge {
            for s in k.structures.values() {
                memory.push(remembered_known(s, false));
            }
            // Scenery is never concealed: in sight when any of its fog nodes is.
            let terrain = &*geo.terrain;
            for s in k.scenery.values() {
                let visible = state.entities.get(s.id).is_some_and(|e| {
                    let mut seen = false;
                    crate::vision::statics::Place::of(e).nodes(terrain, input.catalog, |n| {
                        seen |= k.cells.get(n) == VISIBLE;
                    });
                    seen
                });
                memory.push(remembered_known(s, visible));
            }
            memory.sort_unstable_by_key(|k| k.id);
        }
        let mut memory = memory.into_iter().peekable();
        loop {
            match (live.peek().map(|k| k.id), memory.peek().map(|k| k.id)) {
                (None, None) => break,
                (Some(a), Some(b)) if a == b => {
                    memory.next();
                    all.extend(live.next());
                }
                (Some(a), Some(b)) if a < b => all.extend(live.next()),
                (Some(_), None) => all.extend(live.next()),
                _ => all.extend(memory.next()),
            }
        }
        drop(span);
        let catalog = input.catalog;
        let def = |k: &Known| catalog.get(k.def);
        let mut own = Vec::new();
        let mut workers = Vec::new();
        let mut army = Vec::new();
        let mut buildings = Vec::new();
        let mut stores = Vec::new();
        let mut hostiles = Vec::new();
        let mut resources = Vec::new();
        for (i, k) in all.iter().enumerate() {
            let d = def(k);
            if k.owner == me && k.alive() {
                own.push(i);
                let control = k.unit && d.behaviors.player_control.is_some() && !k.contained;
                if control && d.behaviors.work.is_some() {
                    workers.push(i);
                }
                if control && d.behaviors.combat.is_some() && d.behaviors.work.is_none() {
                    army.push(i);
                }
                if d.kind == Kind::Building {
                    buildings.push(i);
                    if !k.construction
                        && d.behaviors
                            .storage
                            .as_ref()
                            .is_some_and(|s| s.dropoff == Some(true))
                    {
                        stores.push(i);
                    }
                }
            }
            if !k.remembered && k.hostile == Some(true) && k.alive() {
                hostiles.push(i);
            }
            if k.resource
                .is_some_and(|r| r.amount > 0 && r.growing_until.is_none_or(|t| t == 0))
            {
                resources.push(i);
            }
        }
        let home_store = stores
            .iter()
            .copied()
            .find(|&i| def(&all[i]).supply_provided.is_some_and(|s| s > 0))
            .or_else(|| stores.first().copied())
            .or_else(|| buildings.first().copied());
        let home = match home_store {
            Some(i) => {
                let k = &all[i];
                let door = crate::economy::sites::entrance(catalog, k.def, k.pos, k.rotation);
                Pt::of(door, k.p.deck)
            }
            None => own
                .first()
                .map(|&i| all[i].p)
                .or_else(|| geo.starts.first().copied())
                .unwrap_or_default()
                .rounded(),
        };
        let span = tracing::trace_span!("ai frame colony").entered();
        let colony = crate::economy::view::colony(state, catalog, input.economy(), me);
        let bank = colony
            .goods
            .iter()
            .map(|g| (g.item.clone(), g.available))
            .collect();
        drop(span);
        let fallen = state
            .entities
            .iter()
            .filter(|e| e.fallen && e.owner == me)
            .collect();
        Self {
            input,
            geo,
            tick,
            me,
            rules,
            all,
            own,
            workers,
            army,
            buildings,
            stores,
            hostiles,
            resources,
            fallen,
            home,
            bank,
            colony,
            researched: state.economy.researched(me),
            deaths: sight.observed_deaths(me),
            cells,
            blockers,
            navigation,
            roster: RefCell::new(None),
            blockers_ready: Cell::new(false),
        }
    }

    pub fn catalog(&self) -> &'a Catalog {
        self.input.catalog
    }

    pub fn def(&self, k: &Known) -> &'a Definition {
        self.input.catalog.get(k.def)
    }

    pub fn definition(&self, def: Def) -> &'a Definition {
        self.input.catalog.get(def)
    }

    pub fn find(&self, id: &str) -> Option<Def> {
        self.input.catalog.find(id)
    }

    /// The known entity with this id.
    pub fn get(&self, id: u32) -> Option<&Known<'a>> {
        self.all
            .binary_search_by_key(&id, |k| k.id)
            .ok()
            .map(|i| &self.all[i])
    }

    /// The index of a known entity.
    pub fn index_of(&self, id: u32) -> Option<Ix> {
        self.all.binary_search_by_key(&id, |k| k.id).ok()
    }

    pub fn at(&self, i: Ix) -> &Known<'a> {
        &self.all[i]
    }

    /// Own entity state; `None` for others' entities.
    pub fn own_entity(&self, k: &Known<'a>) -> Option<&'a Entity> {
        if k.owner == self.me { k.live } else { None }
    }

    pub fn own_unit(&self, k: &Known<'a>) -> Option<&'a Unit> {
        self.own_entity(k)?.unit.as_deref()
    }

    pub fn order(&self, k: &Known<'a>) -> Option<&'a crate::orders::Order> {
        self.own_unit(k)?.order.as_ref()
    }

    pub fn queued(&self, k: &Known<'a>) -> &'a [crate::orders::Order] {
        self.own_unit(k).map_or(&[], |u| u.order_queue.as_slice())
    }

    pub fn job(&self, k: &Known<'a>) -> Option<&'a crate::economy::Job> {
        let job = self.own_unit(k)?.work.job?;
        self.input.state.economy.job(job)
    }

    pub fn cargo(&self, k: &Known<'a>) -> Option<&'a crate::economy::Cargo> {
        self.own_unit(k)?.work.cargo.as_ref()
    }

    pub fn production(&self, k: &Known<'a>) -> Option<&'a Production> {
        self.own_entity(k)?.production.as_deref()
    }

    /// Queued training at an own building.
    pub fn queue(&self, k: &Known<'a>) -> &'a [crate::economy::QueueEntry] {
        self.production(k).map_or(&[], |p| p.queue.as_slice())
    }

    pub fn casting(&self, k: &Known<'a>) -> bool {
        self.own_entity(k).is_some_and(|e| powers::casting(self, e))
    }

    pub fn stunned(&self, k: &Known<'a>) -> bool {
        self.own_entity(k).is_some_and(|e| powers::stunned(self, e))
    }

    /// Free for new work (old `free`).
    pub fn free(&self, k: &Known<'a>) -> bool {
        let Some(u) = self.own_unit(k) else {
            return false;
        };
        u.order.is_none()
            && u.order_queue.is_empty()
            && u.work.job.is_none()
            && u.work.employment.is_none()
            && u.work.pending_move.is_none()
            && u.release.is_none()
            && u.work.cargo.is_none()
            && !self.casting(k)
            && !self.stunned(k)
    }

    /// Statistics of a seen body (public to anyone who sees it).
    pub fn stats(&self, k: &Known<'a>) -> Option<Stats> {
        let e = k.live?;
        self.def(k).body.as_ref()?;
        Some(Stats::of(
            self.input.catalog,
            e,
            crate::stats::research(self.input.state, e.owner),
        ))
    }

    pub fn max_hp(&self, k: &Known<'a>) -> u32 {
        self.stats(k)
            .map(|s| s.max_hp)
            .or_else(|| self.def(k).body.as_ref().map(|b| b.max_hp))
            .unwrap_or(1)
    }

    /// Whether the cell under `p` is in sight (any floor of it).
    pub fn visible(&self, p: Pt) -> bool {
        if !self.geo.inside(p) {
            return false;
        }
        let (Some(cells), Some(i)) = (self.cells, self.geo.index(p)) else {
            return false;
        };
        if cells.get(i) == VISIBLE {
            return true;
        }
        self.geo
            .terrain
            .surfaces()
            .is_some_and(|s| s.column(i).iter().any(|&n| cells.get(n) == VISIBLE))
    }

    /// Fog value of the cell under `p` (any floor), 0 when never seen.
    pub fn explored(&self, cell: u32) -> u8 {
        let Some(cells) = self.cells else {
            return 0;
        };
        let ground = cells.get(cell);
        match self.geo.terrain.surfaces() {
            Some(s) => s
                .column(cell)
                .iter()
                .map(|&n| cells.get(n))
                .fold(ground, u8::max),
            None => ground,
        }
    }

    /// Whether the prerequisites are met (old `available`).
    pub fn available(&self, requires: Option<&[Id]>) -> bool {
        let Some(requires) = requires else {
            return true;
        };
        let fulfils = &self.input.economy().tables().fulfils;
        let mut complete = BTreeSet::new();
        for &i in &self.own {
            let k = &self.all[i];
            if k.construction || self.def(k).kind != Kind::Building {
                continue;
            }
            complete.extend(fulfils[usize::from(k.def.0)].iter().copied());
        }
        requires.iter().all(|id| {
            self.find(id.as_str())
                .is_some_and(|d| complete.contains(&d))
        })
    }

    pub fn balance(&self, item: &str) -> u32 {
        self.bank
            .iter()
            .find(|(i, _)| i.as_str() == item)
            .map_or(0, |(_, n)| *n)
    }

    /// Whether a completed store takes this item.
    pub fn accepts(&self, item: &str) -> bool {
        self.stores.iter().any(|&i| {
            self.def(&self.all[i])
                .behaviors
                .storage
                .as_ref()
                .is_some_and(|s| s.accepts.iter().any(|a| a.as_str() == item))
        })
    }

    /// Affordable with `reserve` set aside, and available.
    pub fn can_afford(&self, d: &Definition, reserve: &[(Id, u32)]) -> bool {
        self.available(d.requires.as_deref())
            && d.creation
                .as_ref()
                .map_or(&[][..], |c| c.items())
                .iter()
                .all(|c| {
                    let held = reserve
                        .iter()
                        .find(|(i, _)| *i == c.item)
                        .map_or(0, |(_, n)| *n);
                    self.balance(c.item.as_str()) >= held + c.amount
                })
    }

    /// Fighting strength of a seen body (old `power`).
    pub fn power(&self, k: &Known<'a>) -> f64 {
        let d = self.def(k);
        let s = self.stats(k);
        self.nominal_power(
            d,
            Some(f64::from(k.hp.unwrap_or(0))),
            s.map(|s| s.damage_milli as f64 / 1000.0),
            s.map(|s| s.armor),
            s.map(|s| f64::from(s.cooldown_ticks)),
        )
    }

    /// Strength of a definition at full health, or at `hp` (old `nominalPower`).
    pub fn nominal_power(
        &self,
        d: &Definition,
        hp: Option<f64>,
        damage: Option<f64>,
        armor: Option<i32>,
        cooldown: Option<f64>,
    ) -> f64 {
        let Some(c) = &d.behaviors.combat else {
            return 0.0;
        };
        let hp = hp.unwrap_or_else(|| d.body.as_ref().map_or(0.0, |b| f64::from(b.max_hp)));
        let damage = damage.unwrap_or(f64::from(c.damage));
        let armor = armor.unwrap_or_else(|| d.body.as_ref().map_or(0, |b| b.armor as i32));
        let cooldown = cooldown.unwrap_or(f64::from(c.cooldown_ticks));
        let k = self.input.catalog.content.rules.armor_coefficient;
        (hp.max(0.0) / armor_multiplier(k, armor) * damage * 40.0 / cooldown).sqrt()
            * if c.range > 3.0 { 1.2 } else { 1.0 }
    }

    /// Damage `attacker`'s weapon does to `target` (old `damage`).
    pub fn damage(&self, target: &Known<'a>, attacker: &Known<'a>) -> f64 {
        let combat = self.input.senses().combat;
        let (Some(weapon), Some(armor)) = (
            combat.weapon(attacker.def),
            combat.profile(target.def).armor,
        ) else {
            return 0.0;
        };
        let raw = self
            .stats(attacker)
            .map_or(u64::from(weapon.damage) * 1000, |s| s.damage_milli);
        let value = self.stats(target).map_or_else(
            || self.def(target).body.as_ref().map_or(0, |b| b.armor as i32),
            |s| s.armor,
        );
        f64::from(combat.mitigate(raw, weapon.damage_type, armor, value))
    }

    /// The known resource nearest to `origin` that `accept` takes; ties go to the lower id.
    pub fn nearest_resource(&self, origin: Pt, accept: impl Fn(&Known<'a>) -> bool) -> Option<Ix> {
        let mut best: Option<(f64, Ix)> = None;
        for &i in &self.resources {
            let r = &self.all[i];
            if !accept(r) {
                continue;
            }
            let d = dist(r.p, origin);
            if best.is_none_or(|(b, _)| d < b) {
                best = Some((d, i));
            }
        }
        best.map(|(_, i)| i)
    }

    /// Unit definitions the player has or can make: what its own entities build, train
    /// and upgrade into, transitively.
    pub fn unit_roster(&self) -> Vec<Def> {
        if let Some(r) = &*self.roster.borrow() {
            return r.clone();
        }
        let catalog = self.input.catalog;
        let mut ids: Vec<Def> = Vec::new();
        for &i in &self.own {
            let d = self.all[i].def;
            if !ids.contains(&d) {
                ids.push(d);
            }
        }
        let mut cursor = 0;
        while cursor < ids.len() {
            let d = catalog.get(ids[cursor]);
            let next = d
                .behaviors
                .work
                .iter()
                .flat_map(|w| &w.builds)
                .chain(d.behaviors.production.iter().flat_map(|p| &p.outputs))
                .chain(d.upgrade.iter().map(|u| &u.target));
            for id in next {
                if let Some(def) = catalog.find(id.as_str())
                    && !ids.contains(&def)
                {
                    ids.push(def);
                }
            }
            cursor += 1;
        }
        let roster: Vec<Def> = ids
            .into_iter()
            .filter(|&d| {
                let d = catalog.get(d);
                d.kind == Kind::Unit && d.dimensions.is_some()
            })
            .collect();
        *self.roster.borrow_mut() = Some(roster.clone());
        roster
    }

    /// The widest and tallest ground body the player has or can make.
    pub fn base_body(&self) -> (f64, f64) {
        let nav = self.input.profiles().navigation_body;
        let (mut radius, mut height) = (nav.radius, nav.height);
        for d in self.unit_roster() {
            let body = self.input.profiles().get(d).body;
            if !body.air {
                radius = radius.max(body.radius);
                height = height.max(body.height);
            }
        }
        (radius, height)
    }

    /// The body of a known unit.
    pub fn mover(&self, k: &Known<'a>) -> Mover {
        let body = self.input.profiles().get(k.def).body;
        Mover {
            at: Fixed::on(k.pos.x, k.pos.y, k.p.deck),
            radius: body.radius,
            height: body.height,
            air: body.air,
        }
    }

    // ----- known blockers and navigation ------------------------------------------------

    /// Brings the known blockers up to date with this observation (once per frame).
    fn sync_blockers(&self) {
        if self.blockers_ready.replace(true) {
            return;
        }
        let _span = tracing::trace_span!("ai blockers").entered();
        let catalog = self.input.catalog;
        let size = i64::from(self.geo.size);
        let mut blockers = self.blockers.borrow_mut();
        let mut present = Vec::new();
        let mut changed = Vec::new();
        for k in &self.all {
            let d = catalog.get(k.def);
            let building = d.kind == Kind::Building;
            if !building && !k.resource.is_some_and(|r| r.amount > 0) {
                continue;
            }
            present.push(k.id);
            let signature = (k.def.0, k.pos.x, k.pos.y, k.rotation, building);
            if blockers.current(k.id, &signature) {
                continue;
            }
            let extent = Extent::of(d.footprint.as_ref());
            let rotation = f64::from(k.rotation) / 1000.0;
            let bounds = footprint_cells(k.p.x, k.p.y, extent, rotation);
            let in_map = |x: i64, y: i64| {
                (x >= 0 && y >= 0 && x < size && y < size).then(|| (y * size + x) as u32)
            };
            let cells: Vec<u32> = if building {
                bounds.cells().filter_map(|(x, y)| in_map(x, y)).collect()
            } else {
                // Appearance is public: a resource's collision disc scales with it. The
                // old observation boundary dropped it, so the AI saw every tree at full
                // size (docs/decisions.md D27). A placement's scale never changes.
                let scale = self
                    .input
                    .state
                    .entities
                    .get(crate::entity::EntityId(k.id))
                    .and_then(|e| e.placement.as_ref())
                    .and_then(|p| self.geo.scales.get(p))
                    .copied()
                    .unwrap_or(1.0);
                let r = d.collision_radius.unwrap_or(0.0) * scale;
                let (px, py) = (k.p.x, k.p.y);
                let mut out = Vec::new();
                let y0 = bounds.min_y.min((py - r).ceil() as i64);
                let y1 = bounds.max_y.max((py + r).floor() as i64);
                let x0 = bounds.min_x.min((px - r).ceil() as i64);
                let x1 = bounds.max_x.max((px + r).floor() as i64);
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let (fx, fy) = (x as f64 - px, y as f64 - py);
                        if (bounds.contains(x, y) || fx * fx + fy * fy <= r * r)
                            && let Some(c) = in_map(x, y)
                        {
                            out.push(c);
                        }
                    }
                }
                out
            };
            changed.push(Blocker {
                id: k.id,
                signature,
                cells,
            });
        }
        blockers.update(&present, changed);
    }

    /// Whether a known blocker closes the cell.
    pub fn blocked(&self, cell: u32) -> bool {
        self.sync_blockers();
        self.blockers.borrow().blocked(cell)
    }

    /// Runs `query` on navigation brought up to date with this observation.
    pub fn routes<T>(&self, query: impl FnOnce(&mut ObservedNavigation) -> T) -> T {
        self.sync_blockers();
        let mut blockers = self.blockers.borrow_mut();
        let mut navigation = self.navigation.borrow_mut();
        let nav = match &mut *navigation {
            Some(nav) => {
                nav.sync(self.geo, &mut blockers);
                nav
            }
            none => none.insert(ObservedNavigation::new(self.geo, &mut blockers)),
        };
        drop(blockers);
        query(nav)
    }

    /// Prepares the sector graph and meshes of every body the player can field.
    pub fn prepare_navigation(&self) {
        let movers: Vec<Mover> = self
            .unit_roster()
            .into_iter()
            .map(|d| {
                let body = self.input.profiles().get(d).body;
                Mover {
                    at: Fixed::new(0, 0),
                    radius: body.radius,
                    height: body.height,
                    air: body.air,
                }
            })
            .collect();
        self.routes(|nav| nav.prepare(self.geo, &movers));
    }

    /// The nearest point to `p` the actors can stand on and reach, or (without actors) an
    /// open point connected to home (old `nearestSafe`).
    pub fn nearest_safe(&self, p: Pt, actors: &[Ix]) -> Pt {
        let _span = tracing::trace_span!("ai safe point").entered();
        let geo = self.geo;
        let movers: Vec<Mover> = actors.iter().map(|&i| self.mover(&self.all[i])).collect();
        if movers.is_empty() {
            let base = p.rounded();
            for r in 0..=8i32 {
                if let Some(q) = ring(base, r).find(|&q| {
                    geo.inside(q)
                        && geo.connected(self.home, q)
                        && geo.index(q).is_some_and(|i| !self.blocked(i))
                }) {
                    return q;
                }
            }
            return self.home;
        }
        let first = self.all[actors[0]].p.rounded();
        self.routes(|nav| {
            if !geo.layered() && movers.iter().any(|m| !nav.fits(geo, m.point(), m)) {
                return first;
            }
            let base = p.rounded();
            for r in 0..=24i32 {
                for q in ring(base, r) {
                    if geo.inside(q)
                        && movers.iter().all(|m| m.air || geo.connected(m.point(), q))
                        && movers.iter().all(|m| nav.fits(geo, q, m))
                        && movers.iter().all(|m| nav.reachable(geo, m, q))
                    {
                        return q;
                    }
                }
            }
            first
        })
    }

    /// Whether the cell is land on the briefing.
    pub fn land(&self, cell: u32) -> bool {
        self.geo.land(cell)
    }
}

/// The points of the square ring `r` around `base`, row by row (old ring scan).
fn ring(base: Pt, r: i32) -> impl Iterator<Item = Pt> {
    let (bx, by) = (base.x as i32, base.y as i32);
    (by - r..=by + r).flat_map(move |y| {
        (bx - r..=bx + r).filter_map(move |x| {
            (r == 0 || (x - bx).abs() == r || (y - by).abs() == r).then_some(Pt {
                x: f64::from(x),
                y: f64::from(y),
                deck: base.deck,
            })
        })
    })
}

/// The deck of a point, by name.
pub fn deck_of(geo: &Geography, surface: Option<&str>) -> Option<DeckId> {
    geo.deck_id(surface)
}
