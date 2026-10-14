//! Colony-wide research bought at buildings (old `Research`, `src/sim/game/research.ts`)
//! and the effects the economy owns.

use super::{Ctx, ResearchEntry, alive, ready, stock, supply};
use crate::{catalog::Catalog, entity::EntityId, state::FactKind};
use canopy_content::{Id, placement::Owner};

/// The `research` command (old `Research.enqueue`).
pub fn enqueue(ctx: &mut Ctx, b: EntityId, id: &Id) -> Result<(), String> {
    let e = ctx.state.entities.get(b).expect("building");
    let owner = e.owner;
    let policy = ctx.catalog.get(e.definition).behaviors.research.as_ref();
    let recipe = ctx.catalog.content.rules.research.get(id.as_str());
    let (Some(policy), Some(recipe)) = (policy, recipe) else {
        return Err("Select a completed research building".into());
    };
    if !policy.outputs.contains(id) || e.research.is_none() || e.construction.is_some() {
        return Err("Select a completed research building".into());
    }
    if ctx.state.economy.researched(owner).contains(id) {
        return Err("Already researched".into());
    }
    let queued = ctx.index.buildings.iter().any(|&other| {
        ctx.state.entities.get(other).is_some_and(|o| {
            o.owner == owner
                && alive(o)
                && o.research.as_ref().is_some_and(|r| r.queue.iter().any(|q| &q.id == id))
        })
    });
    if queued {
        return Err("Research already queued".into());
    }
    let capacity = policy.queue_capacity as usize;
    if e.research.as_ref().map_or(0, |r| r.queue.len()) >= capacity {
        return Err("Research queue is full".into());
    }
    if let Some(reason) = supply::prerequisite(ctx, owner, recipe.requires.as_deref()) {
        return Err(reason);
    }
    let Some(payment) = stock::reserve(ctx.state, ctx.catalog, ctx.index, owner, &recipe.items)
    else {
        return Err("Insufficient resources".into());
    };
    stock::pay(ctx.state, &payment);
    ctx.state
        .entities
        .get_mut(b)
        .and_then(|e| e.research.as_mut())
        .expect("research")
        .queue
        .push(ResearchEntry {
            id: id.clone(),
            progress: 0,
        });
    Ok(())
}

/// The `cancelResearch` command (old `Research.cancel`): the bill comes back.
pub fn cancel(ctx: &mut Ctx, b: EntityId, id: &Id) -> Result<(), String> {
    let index = ctx
        .state
        .entities
        .get(b)
        .and_then(|e| e.research.as_ref())
        .and_then(|r| r.queue.iter().position(|q| &q.id == id))
        .ok_or("Research is not queued here")?;
    let items = ctx
        .catalog
        .content
        .rules
        .research
        .get(id.as_str())
        .map(|r| r.items.clone())
        .unwrap_or_default();
    stock::refund_cost(ctx, b, &items);
    if let Some(r) = ctx.state.entities.get_mut(b).and_then(|e| e.research.as_mut()) {
        r.queue.remove(index);
    }
    Ok(())
}

/// Maximum health a research adds to `unit` (its `maxHp` effects).
fn hp_bonus(catalog: &Catalog, research: &Id, unit: &Id) -> u32 {
    catalog
        .content
        .rules
        .research
        .get(research.as_str())
        .map_or(0, |r| {
            r.effects
                .iter()
                .filter(|e| e.units.contains(unit))
                .map(|e| e.max_hp.unwrap_or(0))
                .sum()
        })
}

/// Maximum health completed research adds to units of `def` owned by `owner`. Combat's
/// stats (levels, items) add theirs on top.
pub fn max_hp_bonus(catalog: &Catalog, researched: &[Id], unit: &Id) -> u32 {
    researched.iter().map(|r| hp_bonus(catalog, r, unit)).sum()
}

/// Full health of a fresh unit of `def` for `owner`: its first level's or its body's,
/// with research.
pub fn fresh_max_hp(
    catalog: &Catalog,
    state: &crate::state::State,
    owner: Owner,
    def: crate::catalog::Def,
) -> Option<u32> {
    let d = catalog.get(def);
    let base = d
        .behaviors
        .progression
        .as_ref()
        .and_then(|p| p.levels.first())
        .map(|l| l.max_hp)
        .or(d.body.as_ref().map(|b| b.max_hp))?;
    Some(base + max_hp_bonus(catalog, state.economy.researched(owner), &d.id))
}

/// Step 9 (old `Research.tick`): each building's first research advances; on completion
/// the colony learns it and living units gain its health.
pub fn advance(ctx: &mut Ctx) {
    let tick = ctx.state.tick;
    for b in ctx.index.buildings.clone() {
        let Some(e) = ctx.state.entities.get(b) else {
            continue;
        };
        if !alive(e) || !ready(e, tick) || e.construction.is_some() || e.upgrade.is_some() {
            continue;
        }
        let owner = e.owner;
        let Some(entry) = e.research.as_ref().and_then(|r| r.queue.first()).cloned() else {
            continue;
        };
        let Some(recipe) = ctx.catalog.content.rules.research.get(entry.id.as_str()) else {
            continue;
        };
        let progress = entry.progress + 1;
        if let Some(head) = ctx
            .state
            .entities
            .get_mut(b)
            .and_then(|e| e.research.as_mut())
            .and_then(|r| r.queue.first_mut())
        {
            head.progress = progress;
        }
        if progress < recipe.work_ticks {
            continue;
        }
        for cost in &recipe.items {
            stock::record(
                ctx.state,
                owner,
                FactKind::Consumed,
                "Research completed",
                Some((&cost.item, cost.amount)),
            );
        }
        let list = ctx.state.economy.research.entry(owner).or_default();
        list.push(entry.id.clone());
        list.sort();
        for &u in &ctx.index.units {
            let Some(e) = ctx.state.entities.get_mut(u) else {
                continue;
            };
            if e.owner != owner {
                continue;
            }
            let bonus = hp_bonus(ctx.catalog, &entry.id, ctx.catalog.id(e.definition));
            if let Some(hp) = e.hp.as_mut()
                && *hp > 0
            {
                *hp += bonus;
            }
        }
        if let Some(r) = ctx.state.entities.get_mut(b).and_then(|e| e.research.as_mut()) {
            r.queue.remove(0);
        }
        stock::record(
            ctx.state,
            owner,
            FactKind::Command,
            format!("{} researched", recipe.name),
            None,
        );
    }
}
