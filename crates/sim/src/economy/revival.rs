//! Hero revival at buildings (old `Revival` building queue, `src/sim/game/revival.ts`,
//! `content/revival.ts`). Revival shares the building's one queue with training and
//! research: what was queued first goes first (D72), and it shares training's capacity.
//!
//! Heroes fall in combat and are kept as fallen entities ([`crate::heroes::retain`]);
//! revival brings the same entity back; ability returns (`spellReturn`) live in
//! [`crate::abilities::world_ops`].

use super::{
    Ctx, RevivalEntry, alive,
    lane::{self, Lane},
    production, ready, stock,
    supply::{self, colony},
};
use crate::{
    entity::{Entity, EntityId, Milli, Unit},
    spatial::Spot,
    state::FactKind,
    stats::Stats,
};
use canopy_content::{Id, game::Revival};

/// A hero that fell and waits to be revived.
pub fn fallen(e: &Entity) -> bool {
    crate::heroes::fallen(e)
}

/// Brings a fallen hero back at `spot` with full health (old `returnAt`): a fresh unit,
/// part of the simulation from the next tick.
pub fn restore(ctx: &mut Ctx, hero: EntityId, spot: Spot) {
    let tick = ctx.state.tick;
    let surface = spot.deck.map(|d| ctx.spatial.deck_name(d));
    let Some(e) = ctx.state.entities.get_mut(hero) else {
        return;
    };
    e.position = Milli {
        x: spot.x * 1000,
        y: spot.y * 1000,
    };
    e.surface = surface;
    e.unit = Some(Box::<Unit>::default());
    e.fallen = false;
    e.ready_tick = tick + 1;
    crate::abilities::world_ops::restore_pools(ctx.catalog, ctx.state, hero, 1000, 1000);
}

/// The price and time of reviving a hero of `level` (old `revivalTerms`): the base, plus
/// the per-level increments above level one.
pub fn terms(policy: &Revival, level: u32) -> (Vec<(Id, u32)>, u32) {
    let mut amounts: Vec<(Id, u32)> = Vec::new();
    let mut add = |item: &Id, amount: u32| match amounts.iter_mut().find(|(i, _)| i == item) {
        Some((_, n)) => *n += amount,
        None => amounts.push((item.clone(), amount)),
    };
    for p in &policy.items {
        add(&p.item, p.amount);
    }
    for p in &policy.items_per_level {
        add(&p.item, p.amount * level.saturating_sub(1));
    }
    amounts.retain(|(_, n)| *n > 0);
    (
        amounts,
        policy.work_ticks + policy.work_ticks_per_level * level.saturating_sub(1),
    )
}

fn costs(bill: &[(Id, u32)]) -> Vec<canopy_content::game::Cost> {
    bill.iter()
        .map(|(item, amount)| canopy_content::game::Cost {
            item: item.clone(),
            amount: *amount,
        })
        .collect()
}

/// Whether any building has `hero` queued.
fn queued(ctx: &Ctx, hero: EntityId) -> bool {
    ctx.index.buildings.iter().any(|&b| {
        ctx.state
            .entities
            .get(b)
            .and_then(|e| e.revival.as_ref())
            .is_some_and(|r| r.queue.iter().any(|q| q.hero == hero))
    })
}

/// The `revive` command (old `Revival.enqueue`).
pub fn enqueue(ctx: &mut Ctx, b: EntityId, hero: EntityId) -> Result<(), String> {
    let e = ctx.state.entities.get(b).expect("building");
    let owner = e.owner;
    let policy = ctx
        .catalog
        .get(e.definition)
        .behaviors
        .revival
        .as_ref()
        .filter(|_| e.revival.is_some() && e.construction.is_none())
        .ok_or("Select a completed revival building")?;
    let h = ctx
        .state
        .entities
        .get(hero)
        .filter(|h| fallen(h) && h.owner == owner)
        .ok_or("Choose one of your fallen heroes")?;
    if crate::abilities::world_ops::returning(h) {
        return Err("Hero is already returning through an ability".into());
    }
    if queued(ctx, hero) {
        return Err("Hero is already being revived".into());
    }
    if production::lane_size(e) >= policy.queue_capacity as usize {
        return Err("Revival queue is full".into());
    }
    let supply = colony(ctx.state, ctx.catalog, ctx.index, owner);
    let hint = ctx.tables.hints.get(&owner).map(String::as_str);
    let cost = ctx.catalog.get(h.definition).supply_cost.unwrap_or(0);
    if let Some(reason) = supply::admission(&supply, cost, hint) {
        return Err(reason);
    }
    let level = Stats::of(ctx.catalog, h, crate::stats::research(ctx.state, owner)).level;
    let (bill, _) = terms(policy, level);
    let Some(payment) = stock::reserve(ctx.state, ctx.catalog, ctx.index, owner, &costs(&bill))
    else {
        return Err("Insufficient resources".into());
    };
    stock::pay(ctx.state, &payment);
    let id = lane::next(ctx.state);
    ctx.state
        .entities
        .get_mut(b)
        .and_then(|e| e.revival.as_mut())
        .expect("revival")
        .queue
        .push(RevivalEntry {
            id,
            hero,
            level,
            progress: 0,
        });
    Ok(())
}

/// The `cancelRevival` command (old `Revival.cancel`): the bill comes back.
pub fn cancel(ctx: &mut Ctx, b: EntityId, hero: EntityId) -> Result<(), String> {
    let e = ctx.state.entities.get(b).expect("building");
    let index = e
        .revival
        .as_ref()
        .and_then(|r| r.queue.iter().position(|q| q.hero == hero))
        .ok_or("Hero is not queued here")?;
    let level = e.revival.as_ref().expect("revival").queue[index].level;
    let bill = ctx
        .catalog
        .get(e.definition)
        .behaviors
        .revival
        .as_ref()
        .map(|p| terms(p, level).0)
        .unwrap_or_default();
    stock::refund_cost(ctx, b, &costs(&bill));
    if let Some(r) = ctx
        .state
        .entities
        .get_mut(b)
        .and_then(|e| e.revival.as_mut())
    {
        r.queue.remove(index);
    }
    Ok(())
}

/// Step 9 (old building half of `Revival.tick`): the head revival of each building whose
/// lane it leads progresses; a revived hero returns beside the doorway.
pub fn advance(ctx: &mut Ctx) {
    let tick = ctx.state.tick;
    for b in ctx.index.buildings.clone() {
        let Some(e) = ctx.state.entities.get(b) else {
            continue;
        };
        let Some(policy) = ctx.catalog.get(e.definition).behaviors.revival.as_ref() else {
            continue;
        };
        let Some(entry) = e.revival.as_ref().and_then(|r| r.queue.first()).cloned() else {
            continue;
        };
        if !alive(e)
            || e.construction.is_some()
            || e.upgrade.is_some()
            || e.production.as_ref().is_some_and(|p| p.paused)
            || lane::head(e) != Some(Lane::Revival)
            || !ready(e, tick)
            || ctx.network.unpowered(b)
        {
            continue;
        }
        let owner = e.owner;
        let hero = ctx
            .state
            .entities
            .get(entry.hero)
            .filter(|h| fallen(h) && h.owner == owner);
        let Some(hero) = hero else {
            let _ = cancel(ctx, b, entry.hero);
            continue;
        };
        let (bill, work_ticks) = terms(policy, entry.level);
        let hero_def = hero.definition;
        if entry.progress == 0 {
            let supply = colony(ctx.state, ctx.catalog, ctx.index, owner);
            let cost = ctx.catalog.get(hero_def).supply_cost.unwrap_or(0);
            if supply::start(&supply, cost).is_some() {
                continue;
            }
        }
        let progress = work_ticks.min(entry.progress + 1);
        if let Some(head) = ctx
            .state
            .entities
            .get_mut(b)
            .and_then(|e| e.revival.as_mut())
            .and_then(|r| r.queue.first_mut())
        {
            head.progress = progress;
        }
        if progress < work_ticks {
            continue;
        }
        if supply::living_units(ctx, owner) >= ctx.catalog.content.rules.max_units as usize {
            continue;
        }
        let building = ctx.state.entities.get(b).expect("building");
        let (x, y) = super::sites::entrance_of(ctx.catalog, building).cell();
        let body = ctx.spatial.profiles.get(hero_def).body;
        let Some(spot) = ctx.spatial.nearest(
            Spot::new(x, y, None),
            12,
            Some(entry.hero),
            Some(&body),
            None,
            None,
        ) else {
            continue;
        };
        restore(ctx, entry.hero, spot);
        for (item, amount) in &bill {
            stock::record(
                ctx.state,
                owner,
                FactKind::Consumed,
                "Hero revival completed",
                Some((item, *amount)),
            );
        }
        if let Some(r) = ctx
            .state
            .entities
            .get_mut(b)
            .and_then(|e| e.revival.as_mut())
        {
            r.queue.remove(0);
        }
        let name = ctx.catalog.get(hero_def).name.clone();
        stock::record(
            ctx.state,
            owner,
            FactKind::Command,
            format!("{name} has returned"),
            None,
        );
    }
}
