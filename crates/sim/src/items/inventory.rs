//! Hero inventories (old `inventory.ts`, the item branches of `Game.command`): pickup
//! orders walk to an item and take it into a free slot, drop puts a slot's item on the
//! ground with its charges, use runs its effect, and units other than heroes drop what they
//! carry when they die.

use super::{effects, equipment};
use crate::{
    abilities::{Ctx, actor},
    action::Action,
    catalog::Def,
    entity::{Entity, EntityId, Milli},
    orders::{Order, discard_navigation},
    spatial::Spot,
    state::{FactKind, State},
    stats::Stats,
};
use canopy_content::{
    items::ItemEffect,
    placement::{InitialState, Owner, Placement, Point},
};

/// Whether `action` is an item command.
pub fn handles(action: &Action) -> bool {
    matches!(
        action,
        Action::Pickup { .. } | Action::DropItem { .. } | Action::UseItem { .. }
    )
}

/// Why `hero` cannot pick `target` up (old `pickupError`).
pub fn pickup_error(
    ctx: &Ctx,
    state: &State,
    hero: EntityId,
    target: EntityId,
) -> Option<&'static str> {
    let catalog = ctx.catalog();
    let h = state.entities.get(hero)?;
    let slots = h
        .equipment
        .as_ref()
        .map(|q| &q.slots)
        .filter(|s| !s.is_empty());
    let Some(slots) = slots.filter(|_| catalog.get(h.definition).behaviors.inventory.is_some())
    else {
        return Some("This unit has no inventory");
    };
    let t = state.entities.get(target);
    let effect = t.and_then(|t| catalog.items.get(t.definition));
    let valid = t.is_some_and(|t| {
        t.item.as_ref().is_some_and(|i| i.quantity == 1)
            && (t.owner == Owner::Nobody || t.owner == h.owner)
    }) && effect.is_some();
    if !valid {
        return Some("Cannot pick up this item");
    }
    if let ItemEffect::Consumable {
        on_pickup: Some(true),
        ..
    } = &effect.expect("effect").effect
    {
        return if h.progression.is_some() {
            None
        } else {
            Some("Only a hero can use this item")
        };
    }
    if !slots.contains(&None) {
        return Some("Inventory is full");
    }
    None
}

fn within_reach(ctx: &Ctx, hero: &Entity, target: &Entity) -> bool {
    let range = ctx
        .catalog()
        .get(hero.definition)
        .behaviors
        .inventory
        .as_ref()
        .map_or(0.0, |i| i.pickup_range);
    let (dx, dy) = (
        f64::from(hero.position.x - target.position.x) / 1000.0,
        f64::from(hero.position.y - target.position.y) / 1000.0,
    );
    if dx * dx + dy * dy > range * range {
        return false;
    }
    let terrain = ctx.senses.terrain;
    if terrain.surfaces().is_none() {
        return true;
    }
    let point = |e: &Entity| crate::terrain::SurfacePoint {
        x: f64::from(e.position.x) / 1000.0,
        y: f64::from(e.position.y) / 1000.0,
        deck: e.surface.as_deref().and_then(|s| terrain.deck_id(s)),
        elevation: 0.0,
    };
    terrain.melee_clear(point(hero), point(target))
}

fn stop(state: &mut State, hero: EntityId, message: Option<&str>) {
    let Some(e) = state.entities.get_mut(hero) else {
        return;
    };
    let owner = e.owner;
    if let Some(u) = &mut e.unit {
        u.order = None;
        discard_navigation(u);
    }
    if let Some(m) = message {
        state.fact(owner, FactKind::Error, m);
    }
}

fn busy(ctx: &Ctx, e: &Entity) -> bool {
    e.unit.as_ref().is_some_and(|u| u.taunt.is_some())
        || crate::abilities::statuses::stunned(ctx.catalog(), e)
        || e.caster.as_ref().is_some_and(|c| c.pending.is_some())
}

fn pickers(state: &State) -> Vec<(EntityId, EntityId)> {
    let tick = state.tick;
    state
        .entities
        .iter()
        .filter(|e| actor::alive(e) && e.ready_tick <= tick && crate::spatial::present(e))
        .filter_map(|e| match e.unit.as_ref()?.order.as_ref()? {
            Order::Pickup { target } => Some((e.id, *target)),
            _ => None,
        })
        .collect()
}

/// Walks pickup orders toward their items (old `Inventory.plan`, step 5).
pub fn plan(ctx: &mut Ctx, state: &mut State) {
    let _span = tracing::trace_span!("item pickups").entered();
    let tick = state.tick;
    for (hero, target) in pickers(state) {
        if let Some(error) = pickup_error(ctx, state, hero, target) {
            stop(state, hero, Some(error));
            continue;
        }
        let e = state.entities.get(hero).expect("hero");
        if busy(ctx, e) {
            continue;
        }
        let t = state.entities.get(target).expect("item");
        if within_reach(ctx, e, t) {
            let u = state
                .entities
                .get_mut(hero)
                .and_then(|e| e.unit.as_mut())
                .expect("unit");
            u.route.clear();
            u.goal = None;
            u.detour = None;
            continue;
        }
        let u = e.unit.as_ref().expect("unit");
        if u.route.is_empty() && u.retry_at <= tick {
            let deck = t.surface.as_deref().and_then(|s| ctx.spatial.deck_id(s));
            let at = Spot::new(
                crate::abilities::cell_round(t.position.x),
                crate::abilities::cell_round(t.position.y),
                deck,
            );
            let goal = ctx.spatial.nearest(at, 4, Some(hero), None, None, None);
            let routed = match goal {
                Some(goal) => {
                    let e = state.entities.get_mut(hero).expect("hero");
                    ctx.spatial.route(e, goal, false)
                }
                None => false,
            };
            if !routed {
                stop(state, hero, Some("Item is unreachable"));
                continue;
            }
            state
                .entities
                .get_mut(hero)
                .and_then(|e| e.unit.as_mut())
                .expect("unit")
                .retry_at = tick + 20;
        }
    }
}

/// Takes items in reach (old `Inventory.advance`, step 9).
pub fn advance(ctx: &mut Ctx, state: &mut State) {
    for (hero, target) in pickers(state) {
        if pickup_error(ctx, state, hero, target).is_some() {
            continue;
        }
        let e = state.entities.get(hero).expect("hero");
        if busy(ctx, e) {
            continue;
        }
        let t = state.entities.get(target).expect("item");
        if !within_reach(ctx, e, t) {
            continue;
        }
        let item = t.definition;
        let runtime = t.item.as_ref().and_then(|i| i.runtime.clone());
        let on_pickup = matches!(
            ctx.catalog().items.get(item).map(|d| &d.effect),
            Some(ItemEffect::Consumable {
                on_pickup: Some(true),
                ..
            })
        );
        if on_pickup {
            if effects::power_up(ctx, state, hero, item) {
                remove(ctx, state, target);
                stop(state, hero, None);
            }
            continue;
        }
        let e = state.entities.get_mut(hero).expect("hero");
        let q = equipment(e);
        let slot = q.slots.iter().position(Option::is_none).expect("free slot");
        q.slots[slot] = Some(item);
        if q.runtime.len() != q.slots.len() {
            q.runtime.resize(q.slots.len(), None);
        }
        q.runtime[slot] = runtime;
        remove(ctx, state, target);
        stop(state, hero, None);
    }
}

fn remove(ctx: &mut Ctx, state: &mut State, id: EntityId) {
    let mut ectx = ctx.economy.ctx(state, ctx.senses.catalog, ctx.spatial);
    crate::economy::work::remove(&mut ectx, id, false);
}

/// Puts a slot's item on the ground (old `Inventory.drop`).
pub fn drop(
    ctx: &mut Ctx,
    state: &mut State,
    hero: EntityId,
    slot: usize,
) -> Result<(), &'static str> {
    let Some(e) = state.entities.get(hero) else {
        return Err("Inventory slot is empty");
    };
    let Some(item) = e
        .equipment
        .as_ref()
        .and_then(|q| q.slots.get(slot).copied().flatten())
    else {
        return Err("Inventory slot is empty");
    };
    let (x, y) = e.position.cell();
    let origin = (x, y, e.surface.clone());
    let (px, py) = crate::camps::drop_position(state, ctx.spatial, origin.clone(), hero);
    let placement = Placement {
        activation: None,
        id: String::new(),
        map_knowledge: None,
        definition: ctx.catalog().id(item).clone(),
        position: Point {
            x: f64::from(px),
            y: f64::from(py),
            surface: origin.2,
        },
        rotation: 0.0,
        owner: Owner::Nobody,
        initial_state: Some(InitialState {
            quantity: Some(1),
            ..Default::default()
        }),
        appearance: None,
    };
    let dropped = crate::spawn::create(state, ctx.catalog(), &placement).expect("item definition");
    let tick = state.tick;
    let runtime = {
        let q = state
            .entities
            .get_mut(hero)
            .and_then(|e| e.equipment.as_mut())
            .expect("equipment");
        let r = q.runtime.get_mut(slot).and_then(Option::take);
        q.slots[slot] = None;
        r
    };
    let d = state.entities.get_mut(dropped).expect("dropped");
    d.placement = None;
    d.ready_tick = tick + 1;
    if let Some(i) = &mut d.item {
        i.runtime = runtime;
    }
    ctx.spatial.sync_entity(state, dropped);
    let e = state.entities.get(hero).expect("hero");
    let stats = Stats::of(ctx.catalog(), e, crate::stats::research(state, e.owner));
    let e = state.entities.get_mut(hero).expect("hero");
    if let Some(hp) = &mut e.hp {
        *hp = (*hp).min(stats.max_hp);
    }
    if let Some(c) = &mut e.caster {
        c.mana = c.mana.min(stats.max_mana);
    }
    Ok(())
}

/// A unit other than a hero drops every item when it dies (old `onDeath`).
pub fn on_death(ctx: &mut Ctx, state: &mut State, id: EntityId) {
    let Some(e) = state.entities.get(id) else {
        return;
    };
    if ctx.catalog().get(e.definition).is_hero() {
        return;
    }
    let count = e.equipment.as_ref().map_or(0, |q| q.slots.len());
    for slot in 0..count {
        let filled = state
            .entities
            .get(id)
            .and_then(|e| e.equipment.as_ref())
            .is_some_and(|q| q.slots[slot].is_some());
        if filled {
            let _ = drop(ctx, state, id, slot);
        }
    }
}

/// Applies an item command (old `pickup`, `dropItem`, `useItem` branches).
pub fn apply(
    ctx: &mut Ctx,
    state: &mut State,
    owner: Owner,
    action: &Action,
) -> Result<(), String> {
    match action {
        Action::Pickup {
            actor,
            target,
            append,
        } => {
            let hero = crate::abilities::orders::actor_of(ctx, state, owner, *actor)?;
            let target = EntityId(*target);
            let seen = state
                .entities
                .get(target)
                .is_some_and(|t| ctx.senses.sight(state).seen(owner, t));
            if !seen {
                return Err("Item is not visible".into());
            }
            if let Some(error) = pickup_error(ctx, state, hero, target) {
                return Err(error.into());
            }
            if !crate::orders::issue(
                state,
                hero,
                Order::Pickup { target },
                append.unwrap_or(false),
            ) {
                return Err("Order queue is full".into());
            }
            Ok(())
        }
        Action::DropItem { actor, slot } => {
            let hero = crate::abilities::orders::actor_of(ctx, state, owner, *actor)?;
            drop(ctx, state, hero, *slot as usize).map_err(str::to_string)
        }
        Action::UseItem { actor, slot } => {
            let hero = crate::abilities::orders::actor_of(ctx, state, owner, *actor)?;
            effects::use_item(ctx, state, hero, *slot as usize).map_err(str::to_string)
        }
        _ => Err("Invalid request".into()),
    }
}

/// Sets a slot from a mission (old `missionInventory` operations): giving clears the
/// slot's runtime; taking clamps pools.
pub fn set_slot(
    catalog: &crate::catalog::Catalog,
    state: &mut State,
    hero: EntityId,
    slot: usize,
    item: Option<Def>,
) {
    let Some(e) = state.entities.get_mut(hero) else {
        return;
    };
    let q = equipment(e);
    if slot >= q.slots.len() {
        return;
    }
    q.slots[slot] = item;
    if let Some(r) = q.runtime.get_mut(slot) {
        *r = None;
    }
    if item.is_none() {
        let e = state.entities.get(hero).expect("hero");
        let stats = Stats::of(catalog, e, crate::stats::research(state, e.owner));
        let e = state.entities.get_mut(hero).expect("hero");
        if let Some(hp) = &mut e.hp {
            *hp = (*hp).min(stats.max_hp);
        }
        if let Some(c) = &mut e.caster {
            c.mana = c.mana.min(stats.max_mana);
        }
    }
}

/// The milli position of a cell (for drops).
pub fn cell_point(x: i32, y: i32) -> Milli {
    Milli {
        x: x * 1000,
        y: y * 1000,
    }
}
