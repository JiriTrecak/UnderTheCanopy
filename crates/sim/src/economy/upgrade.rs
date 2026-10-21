//! Paid in-place building upgrades (old `BuildingUpgrades`, `src/sim/game/upgrades.ts`).
//! The building stays what it is until the upgrade completes.

use super::{Ctx, UpgradeTask, ready, stock, supply};
use crate::{entity::EntityId, state::FactKind};

/// The `upgrade` command (old `BuildingUpgrades.enqueue`).
pub fn enqueue(ctx: &mut Ctx, b: EntityId) -> Result<(), String> {
    let e = ctx.state.entities.get(b).expect("building");
    let owner = e.owner;
    let recipe = ctx
        .catalog
        .get(e.definition)
        .upgrade
        .as_ref()
        .filter(|_| e.construction.is_none())
        .ok_or("Select a completed upgradeable building")?;
    if e.upgrade.is_some() {
        return Err("Upgrade already in progress".into());
    }
    let target = ctx
        .catalog
        .find(recipe.target.as_str())
        .ok_or("Select a completed upgradeable building")?;
    // A campaign's tech (`campaign`).
    if let Some(reason) = crate::campaign::refused(ctx.state, owner, recipe.target.as_str()) {
        return Err(reason);
    }
    let requires = ctx.catalog.get(target).requires.as_deref();
    if let Some(reason) = supply::prerequisite(ctx, owner, requires) {
        return Err(reason);
    }
    let Some(payment) = stock::reserve(ctx.state, ctx.catalog, ctx.index, owner, &recipe.items)
    else {
        return Err("Insufficient resources".into());
    };
    stock::pay(ctx.state, &payment);
    ctx.state.entities.get_mut(b).expect("building").upgrade = Some(UpgradeTask {
        target,
        progress: 0,
    });
    Ok(())
}

/// The `cancelUpgrade` command (old `BuildingUpgrades.cancel`).
pub fn cancel(ctx: &mut Ctx, b: EntityId) -> Result<(), String> {
    let e = ctx.state.entities.get(b).expect("building");
    if e.upgrade.is_none() {
        return Err("No upgrade in progress".into());
    }
    let items = ctx
        .catalog
        .get(e.definition)
        .upgrade
        .as_ref()
        .map(|u| u.items.clone())
        .unwrap_or_default();
    stock::refund_cost(ctx, b, &items);
    ctx.state.entities.get_mut(b).expect("building").upgrade = None;
    Ok(())
}

/// Step 9 (old `BuildingUpgrades.tick`): upgrades progress; a completed one turns the
/// building into its target, keeping its wounds.
pub fn advance(ctx: &mut Ctx) {
    let tick = ctx.state.tick;
    for b in ctx.index.buildings.clone() {
        let Some(e) = ctx.state.entities.get(b) else {
            continue;
        };
        if e.upgrade.is_none() || !ready(e, tick) || ctx.network.unpowered(b) {
            continue;
        }
        let owner = e.owner;
        let old = ctx.catalog.get(e.definition);
        let Some(recipe) = old.upgrade.as_ref() else {
            continue;
        };
        let task = e.upgrade.as_ref().expect("upgrade");
        let (target, progress) = (task.target, task.progress + 1);
        let e = ctx.state.entities.get_mut(b).expect("building");
        e.upgrade.as_mut().expect("upgrade").progress = progress;
        if progress < recipe.work_ticks {
            continue;
        }
        for cost in &recipe.items {
            stock::record(
                ctx.state,
                owner,
                FactKind::Consumed,
                "Building upgrade completed",
                Some((&cost.item, cost.amount)),
            );
        }
        let new = ctx.catalog.get(target);
        let (before, after) = (
            old.body.as_ref().map_or(0, |b| b.max_hp),
            new.body.as_ref().map_or(0, |b| b.max_hp),
        );
        let e = ctx.state.entities.get_mut(b).expect("building");
        if let Some(hp) = e.hp.as_mut() {
            *hp = (i64::from(*hp) + i64::from(after) - i64::from(before)).max(0) as u32;
        }
        e.definition = target;
        e.upgrade = None;
        ctx.spatial.sync_entity(ctx.state, b);
        stock::record(
            ctx.state,
            owner,
            FactKind::Command,
            format!("{} completed", new.name),
            None,
        );
    }
}
