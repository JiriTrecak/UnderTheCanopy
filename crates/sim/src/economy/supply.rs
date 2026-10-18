//! Colony limits: supply, the unit limit, worker population, the hero roster and
//! technology prerequisites (old `supply.ts`, `population.ts`, `content/heroRoster.ts`,
//! `content/prerequisites.ts`). Everything is derived from entities and queues, so no
//! saved counter can drift.

use super::{Ctx, alive, revival};
use crate::{
    catalog::{Catalog, Def},
    entity::Entity,
    setup::MatchSetup,
    state::State,
};
use canopy_content::{
    game::{Creation, Kind},
    placement::Owner,
};
use std::collections::{BTreeMap, BTreeSet};

/// An owner's supply (old `colonySupply`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Supply {
    /// Supply of living units.
    pub used: u32,
    /// Supply of queued units and heroes being revived.
    pub reserved: u32,
    /// Supply of units in training.
    pub training: u32,
    pub committed: u32,
    /// Provided supply, capped by the rules.
    pub capacity: u32,
    pub provided: u32,
    pub limit: u32,
    pub available: u32,
    pub over_capacity: u32,
    pub units: u32,
    pub queued_units: u32,
    pub unit_limit: u32,
}

fn cost(catalog: &Catalog, def: Def) -> u32 {
    catalog.get(def).supply_cost.unwrap_or(0)
}

/// `owner`'s supply, from its units and buildings.
pub fn colony(state: &State, catalog: &Catalog, index: &super::Index, owner: Owner) -> Supply {
    let rules = &catalog.content.rules;
    let mut s = Supply {
        limit: rules.max_supply,
        unit_limit: rules.max_units,
        ..Supply::default()
    };
    let mut reviving = BTreeSet::new();
    let members = || {
        index
            .units
            .iter()
            .chain(&index.buildings)
            .filter_map(|&id| state.entities.get(id))
            .filter(|e| e.owner == owner)
    };
    for e in members() {
        if !alive(e) || revival::fallen(e) {
            continue;
        }
        let d = catalog.get(e.definition);
        if e.unit.is_some() {
            s.used += d.supply_cost.unwrap_or(0);
            s.units += 1;
        }
        if e.construction.is_none() {
            s.provided += d.supply_provided.unwrap_or(0);
        }
        if let Some(p) = &e.production {
            for q in &p.queue {
                s.reserved += cost(catalog, q.definition);
                s.queued_units += 1;
            }
            if let Some(a) = &p.active {
                s.training += cost(catalog, a.definition);
            }
        }
        for q in e.revival.iter().flat_map(|r| &r.queue) {
            reviving.insert(q.hero);
            if q.progress > 0
                && let Some(hero) = state.entities.get(q.hero)
                && hero.owner == owner
                && revival::fallen(hero)
            {
                s.training += cost(catalog, hero.definition);
            }
        }
    }
    for e in members() {
        if revival::fallen(e)
            && (reviving.contains(&e.id) || crate::abilities::world_ops::returning(e))
        {
            s.reserved += cost(catalog, e.definition);
            s.queued_units += 1;
        }
    }
    s.capacity = s.provided.min(rules.max_supply);
    s.committed = s.used + s.reserved;
    s.available = s.capacity.saturating_sub(s.committed);
    s.over_capacity = s.committed.saturating_sub(s.capacity);
    s
}

/// Why a unit costing `cost` cannot be queued now (old `supplyAdmission`).
pub fn admission(supply: &Supply, cost: u32, hint: Option<&str>) -> Option<String> {
    if supply.units + supply.queued_units >= supply.unit_limit {
        return Some("Unit limit reached".into());
    }
    if cost > 0 && supply.committed + cost > supply.capacity {
        let (c, cap) = (supply.committed, supply.capacity);
        return Some(match hint {
            Some(name) => format!("Not enough supply ({c}/{cap}). Build a {name}."),
            None => format!("Not enough supply ({c}/{cap})."),
        });
    }
    None
}

/// Why queued work costing `cost` cannot start training now (old `supplyStart`): waiting
/// entries compete only with living units and training already under way.
pub fn start(supply: &Supply, cost: u32) -> Option<super::Status> {
    if supply.units >= supply.unit_limit {
        return Some(super::Status::UnitLimitReached);
    }
    (cost > 0 && supply.used + supply.training + cost > supply.capacity)
        .then_some(super::Status::SupplyBlocked)
}

/// Supply per owner computed once per pass and updated as training starts.
#[derive(Debug, Default)]
pub struct Cache {
    owners: BTreeMap<Owner, Supply>,
}

impl Cache {
    pub fn get(&mut self, ctx: &Ctx, owner: Owner) -> Supply {
        *self
            .owners
            .entry(owner)
            .or_insert_with(|| colony(ctx.state, ctx.catalog, ctx.index, owner))
    }

    pub fn started(&mut self, owner: Owner, cost: u32) {
        if let Some(s) = self.owners.get_mut(&owner) {
            s.training += cost;
        }
    }
}

/// Workers and idle workers of an owner (old `workerPopulation`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Population {
    pub workers: u32,
    pub available: u32,
}

pub fn population(
    state: &State,
    catalog: &Catalog,
    index: &super::Index,
    owner: Owner,
) -> Population {
    let mut p = Population::default();
    for e in index.units.iter().filter_map(|&id| state.entities.get(id)) {
        if e.owner != owner || !alive(e) || catalog.get(e.definition).behaviors.work.is_none() {
            continue;
        }
        let u = e.unit.as_ref().expect("unit");
        p.workers += 1;
        if u.order.is_none()
            && u.order_queue.is_empty()
            && u.work.is_idle()
            && u.contained.is_none()
            && u.release.is_none()
        {
            p.available += 1;
        }
    }
    p
}

/// Per owner, the name of the cheapest constructable building that provides supply in
/// its race's roster (fewest supply, then id). The old messages always named the ant
/// house ("Build a Mound"), also to beetle players.
pub fn hints(catalog: &Catalog, setup: &MatchSetup) -> BTreeMap<Owner, String> {
    let rules = &catalog.content.rules;
    let mut out = BTreeMap::new();
    for slot in &setup.slots {
        let race = slot.race.as_deref().unwrap_or(&rules.default_race);
        let roster: Option<&Vec<canopy_content::Id>> = rules.races.get(race).map(|r| &r.roster);
        let pick = catalog
            .content
            .definitions
            .iter()
            .filter(|d| {
                d.kind == Kind::Building
                    && d.supply_provided.is_some_and(|s| s > 0)
                    && matches!(d.creation, Some(Creation::Construct { .. }))
                    && roster.is_none_or(|r| r.contains(&d.id))
            })
            .min_by(|a, b| {
                a.supply_provided
                    .cmp(&b.supply_provided)
                    .then_with(|| a.id.cmp(&b.id))
            });
        if let Some(d) = pick {
            out.insert(Owner::Player(slot.player), d.name.clone());
        }
    }
    out
}

/// Per building definition, the requirements it fulfils (old `fulfilledPrerequisites`): an
/// upgraded building still counts as every building it was upgraded from.
pub fn fulfils(catalog: &Catalog) -> Vec<Vec<Def>> {
    let count = catalog.content.definitions.len();
    let mut out = vec![Vec::new(); count];
    for (i, origin) in catalog.content.definitions.iter().enumerate() {
        if origin.kind != Kind::Building {
            continue;
        }
        let mut seen = BTreeSet::new();
        let mut current = Some(Def(i as u16));
        while let Some(def) = current {
            if !seen.insert(def) {
                break;
            }
            out[usize::from(def.0)].push(Def(i as u16));
            current = catalog
                .get(def)
                .upgrade
                .as_ref()
                .and_then(|u| catalog.find(u.target.as_str()));
        }
    }
    for list in &mut out {
        list.sort_unstable();
    }
    out
}

/// Why `owner` may not buy something that requires `requires` (old
/// `prerequisiteReason`): each must be a completed living building, or one upgraded from it,
/// or a research the owner completed.
pub fn prerequisite(
    ctx: &Ctx,
    owner: Owner,
    requires: Option<&[canopy_content::Id]>,
) -> Option<String> {
    let requires = requires?;
    let mut complete = BTreeSet::new();
    for e in ctx
        .index
        .buildings
        .iter()
        .filter_map(|&id| ctx.state.entities.get(id))
    {
        if e.owner == owner && e.construction.is_none() && alive(e) {
            complete.extend(
                ctx.tables.fulfils[usize::from(e.definition.0)]
                    .iter()
                    .copied(),
            );
        }
    }
    let researched = ctx.state.economy.researched(owner);
    let rules = &ctx.catalog.content.rules;
    let missing: Vec<&str> = requires
        .iter()
        .filter(|id| match rules.research.get(id.as_str()) {
            Some(_) => !researched.contains(id),
            None => ctx
                .catalog
                .find(id.as_str())
                .is_none_or(|d| !complete.contains(&d)),
        })
        .map(|id| match rules.research.get(id.as_str()) {
            Some(r) => r.name.as_str(),
            None => ctx
                .catalog
                .content
                .definition(id.as_str())
                .map_or(id.as_str(), |d| d.name.as_str()),
        })
        .collect();
    (!missing.is_empty()).then(|| format!("Requires {}", missing.join(", ")))
}

/// Why `owner` may not order another `def`: a definition with `maxOwned` allows that many
/// living or queued at once (one Juggernaut at a time).
pub fn owned_limit(ctx: &Ctx, owner: Owner, def: Def) -> Option<String> {
    let d = ctx.catalog.get(def);
    let limit = d.max_owned?;
    let living = ctx
        .index
        .units
        .iter()
        .filter_map(|&id| ctx.state.entities.get(id))
        .filter(|e| e.owner == owner && e.definition == def && alive(e))
        .count();
    let queued = ctx
        .index
        .buildings
        .iter()
        .filter_map(|&id| ctx.state.entities.get(id))
        .filter(|e| e.owner == owner)
        .filter_map(|e| e.production.as_ref())
        .flat_map(|p| &p.queue)
        .filter(|q| q.definition == def)
        .count();
    (living + queued >= limit as usize).then(|| format!("Only {limit} {} at a time", d.name))
}

/// Why `def` cannot join the roster (old `heroAdmission`).
pub fn hero_admission(
    catalog: &Catalog,
    def: Def,
    roster: &crate::heroes::Roster,
) -> Option<String> {
    let d = catalog.get(def);
    if !d.is_hero() {
        return None;
    }
    if roster.definitions.contains(&d.id) {
        return Some("This hero already belongs to your roster. Revive fallen heroes.".into());
    }
    let used = roster.used();
    (used >= roster.capacity as usize).then(|| {
        format!(
            "Hero roster full ({used}/{}). Upgrade a Hall.",
            roster.capacity
        )
    })
}

/// Living units of `owner` (old `liveUnits().filter(owner).length`).
pub fn living_units(ctx: &Ctx, owner: Owner) -> usize {
    ctx.index
        .units
        .iter()
        .filter_map(|&id| ctx.state.entities.get(id))
        .filter(|e: &&Entity| e.owner == owner && alive(e))
        .count()
}
