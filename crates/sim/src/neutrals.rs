//! Neutral map features (Warcraft III's destructibles, fountains and shops), all neutral
//! buildings a map places:
//!
//! - **Breakables** (crates, barrels, rubble closing a path) are attacked only when ordered
//!   to, and drop what they hold when they break. What they hold is fixed when they are
//!   made: the placement's items, else a roll of a loot pool on the state's generator.
//! - **Springs** restore health and mana to every living unit near them, of any owner, on a
//!   fixed pulse.
//! - **Shops** sell their wares to a player for a hero of theirs standing near: goods from
//!   the player's stock, the item straight into the hero's inventory. Each ware has a
//!   limited stock that grows back one at a time. A purchase is the protocol's `produce`
//!   with the shop as the actor and the item as the definition (the protocol is frozen,
//!   D10).

use crate::{
    abilities::Ctx,
    catalog::{Catalog, Def},
    entity::{Entity, EntityId},
    spatial::Spatial,
    state::{FactKind, State},
    stats::Stats,
};
use canopy_content::{
    Id,
    game::Ware,
    items::ItemEffect,
    placement::{InitialState, Owner, Placement, Point},
};
use serde::{Deserialize, Serialize};

/// A shop's wares in play.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shop {
    pub stalls: Vec<Stall>,
}

/// One ware and how many are left.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Stall {
    pub ware: Ware,
    pub stock: u32,
    /// The tick the next one grows back, while the stock is short.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub restock_at: Option<u32>,
}

impl Shop {
    /// A fully stocked shop selling `wares`.
    pub fn new(wares: &[Ware]) -> Self {
        Self {
            stalls: wares
                .iter()
                .map(|w| Stall {
                    ware: w.clone(),
                    stock: w.stock,
                    restock_at: None,
                })
                .collect(),
        }
    }
}

/// The parts a new entity of `def` takes from this module: a shop's stock, and what a
/// breakable holds (rolled now when it holds nothing named).
pub fn furnish(state: &mut State, catalog: &Catalog, entity: &mut Entity, initial: &InitialState) {
    let d = catalog.get(entity.definition);
    if let Some(shop) = &d.shop {
        let wares = initial.wares.as_deref().unwrap_or(&shop.wares);
        entity.shop = Some(Box::new(Shop::new(wares)));
    }
    let Some(breakable) = &d.breakable else {
        return;
    };
    if initial.inventory.is_some() {
        return;
    }
    let pool = initial
        .loot_pool
        .as_ref()
        .or(breakable.loot_pool.as_ref())
        .and_then(|id| catalog.content.rules.loot_pools.get(id));
    let items = match pool {
        Some(pool) => crate::camps::roll(state, pool),
        None => breakable.drops.clone().unwrap_or_default(),
    };
    for item in items {
        crate::economy::stock::add(&mut entity.inventory, &item, 1);
    }
}

/// A breakable that broke drops what it holds around where it stood.
pub fn on_death(state: &mut State, catalog: &Catalog, spatial: &Spatial, dead: EntityId) {
    let Some(e) = state.entities.get(dead) else {
        return;
    };
    if catalog.get(e.definition).breakable.is_none() {
        return;
    }
    let held: Vec<(Id, u32)> = e.inventory.iter().map(|(i, &n)| (i.clone(), n)).collect();
    let (x, y) = e.position.cell();
    let origin = (x, y, e.surface.clone());
    state
        .entities
        .get_mut(dead)
        .expect("dead")
        .inventory
        .clear();
    let tick = state.tick;
    for (item, count) in held {
        for _ in 0..count {
            let (px, py) = crate::camps::drop_position(state, spatial, origin.clone(), dead);
            let placement = Placement {
                activation: None,
                id: String::new(),
                map_knowledge: None,
                definition: item.clone(),
                position: Point {
                    x: f64::from(px),
                    y: f64::from(py),
                    surface: origin.2.clone(),
                },
                rotation: 0.0,
                owner: Owner::Nobody,
                initial_state: Some(InitialState {
                    quantity: Some(1),
                    ..Default::default()
                }),
                appearance: None,
            };
            if let Ok(id) = crate::spawn::create(state, catalog, &placement) {
                let e = state.entities.get_mut(id).expect("created");
                e.placement = None;
                e.ready_tick = tick + 1;
            }
        }
    }
}

/// Squared distance in cells from `p` to the footprint of `e` (0 inside it).
fn footprint_distance2(catalog: &Catalog, e: &Entity, p: crate::entity::Milli) -> f64 {
    let (hx, hy) = crate::combat::senses::half_extents(catalog, e);
    let dx = (f64::from(p.x - e.position.x) / 1000.0).abs() - hx;
    let dy = (f64::from(p.y - e.position.y) / 1000.0).abs() - hy;
    let (dx, dy) = (dx.max(0.0), dy.max(0.0));
    dx * dx + dy * dy
}

/// Step 3, after regeneration: springs pulse and shops grow their wares back.
pub fn tick(catalog: &Catalog, state: &mut State) {
    let _span = tracing::trace_span!("neutral features").entered();
    let tick = state.tick;
    let mut springs = Vec::new();
    for e in state.entities.iter_mut() {
        if let Some(shop) = &mut e.shop {
            for stall in &mut shop.stalls {
                if stall.restock_at.is_some_and(|t| t <= tick) {
                    stall.stock = (stall.stock + 1).min(stall.ware.stock);
                    stall.restock_at =
                        (stall.stock < stall.ware.stock).then_some(tick + stall.ware.restock_ticks);
                }
            }
        }
        if let Some(r) = &catalog.get(e.definition).restoration
            && e.ready_tick <= tick
            && e.hp.is_none_or(|hp| hp > 0)
            && e.construction.is_none()
            && tick.is_multiple_of(r.interval_ticks)
        {
            springs.push(e.id);
        }
    }
    for spring in springs {
        restore(catalog, state, spring);
    }
}

/// One pulse of a spring.
fn restore(catalog: &Catalog, state: &mut State, spring: EntityId) {
    let s = state.entities.get(spring).expect("spring");
    let r = catalog
        .get(s.definition)
        .restoration
        .as_ref()
        .expect("spring");
    let reach = r.radius * r.radius;
    let tick = state.tick;
    let near: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| {
            e.ready_tick <= tick
                && !e.fallen
                && crate::spatial::present(e)
                && e.unit.as_ref().is_some_and(|u| u.garrison.is_none())
                && footprint_distance2(catalog, s, e.position) <= reach
        })
        .map(|e| e.id)
        .collect();
    let gain = |flat: Option<u32>, permille: Option<u32>, max: u32| {
        flat.unwrap_or(0) + (u64::from(max) * u64::from(permille.unwrap_or(0)) / 1000) as u32
    };
    for id in near {
        let e = state.entities.get(id).expect("unit");
        let stats = Stats::of(catalog, e, crate::stats::research(state, e.owner));
        let health = gain(r.health, r.health_permille, stats.max_hp);
        let mana = gain(r.mana, r.mana_permille, stats.max_mana);
        let e = state.entities.get_mut(id).expect("unit");
        if let Some(hp) = &mut e.hp {
            *hp = hp.saturating_add(health).min(stats.max_hp).max(*hp);
        }
        if let Some(c) = &mut e.caster {
            c.mana = c.mana.saturating_add(mana).min(stats.max_mana).max(c.mana);
        }
    }
}

/// Whether `action` buys from a shop: `produce` addressed to an entity with wares.
pub fn buys(state: &State, action: &crate::action::Action) -> bool {
    match action {
        crate::action::Action::Produce { actor, .. } => state
            .entities
            .get(EntityId(*actor))
            .is_some_and(|e| e.shop.is_some()),
        _ => false,
    }
}

/// Why `hero` cannot take `item` now (a free slot, or a hero for a power-up).
fn take_problem(catalog: &Catalog, hero: &Entity, item: Def) -> Option<&'static str> {
    let on_pickup = matches!(
        catalog.items.get(item).map(|d| &d.effect),
        Some(ItemEffect::Consumable {
            on_pickup: Some(true),
            ..
        })
    );
    if on_pickup {
        return hero
            .progression
            .is_none()
            .then_some("Only a hero can use this item");
    }
    let free = hero
        .equipment
        .as_ref()
        .is_some_and(|q| q.slots.contains(&None));
    (!free).then_some("Inventory is full")
}

/// The units of `owner` that could carry what a shop sells, within its range, nearest
/// first (then by id).
pub fn buyers(catalog: &Catalog, state: &State, owner: Owner, shop: &Entity) -> Vec<EntityId> {
    let Some(range) = catalog.get(shop.definition).shop.as_ref().map(|s| s.range) else {
        return Vec::new();
    };
    let tick = state.tick;
    let mut near: Vec<(u64, EntityId)> = state
        .entities
        .iter()
        .filter(|e| {
            e.owner == owner
                && e.ready_tick <= tick
                && !e.fallen
                && crate::spatial::present(e)
                && catalog.get(e.definition).behaviors.inventory.is_some()
        })
        .filter_map(|e| {
            let radius = catalog
                .get(e.definition)
                .dimensions
                .as_ref()
                .map_or(0.0, |d| d.radius);
            let reach = range + radius;
            let d2 = footprint_distance2(catalog, shop, e.position);
            (d2 <= reach * reach).then_some(((d2 * 1000.0) as u64, e.id))
        })
        .collect();
    near.sort_unstable();
    near.into_iter().map(|(_, id)| id).collect()
}

/// Buys `item` at `shop` for `owner` (step 1).
pub fn buy(
    ctx: &mut Ctx,
    state: &mut State,
    owner: Owner,
    shop: EntityId,
    item: &Id,
) -> Result<(), String> {
    let catalog = ctx.catalog();
    let s = state.entities.get(shop).ok_or("Shop is not visible")?;
    if !ctx.senses.sight(state).seen(owner, s) || s.hp.is_some_and(|hp| hp == 0) {
        return Err("Shop is not visible".into());
    }
    let stalls = &s.shop.as_ref().expect("a shop").stalls;
    let Some(slot) = stalls.iter().position(|st| &st.ware.item == item) else {
        return Err("Not sold here".into());
    };
    let stall = &stalls[slot];
    if stall.stock == 0 {
        return Err("Sold out".into());
    }
    let def = catalog.find(item.as_str()).ok_or("Not sold here")?;
    let price = stall.ware.price.clone();
    let candidates = buyers(catalog, state, owner, s);
    if candidates.is_empty() {
        return Err("A hero must stand near the shop".into());
    }
    let mut problem = None;
    let hero = candidates.into_iter().find(|&h| {
        let e = state.entities.get(h).expect("buyer");
        match take_problem(catalog, e, def) {
            None => true,
            Some(p) => {
                problem.get_or_insert(p);
                false
            }
        }
    });
    let Some(hero) = hero else {
        return Err(problem.unwrap_or("Inventory is full").into());
    };
    {
        let ectx = ctx.economy.ctx(state, catalog, ctx.spatial);
        let reservation =
            crate::economy::stock::reserve(ectx.state, catalog, ectx.index, owner, &price)
                .ok_or("Insufficient resources")?;
        crate::economy::stock::pay(ectx.state, &reservation);
        for cost in &price {
            crate::economy::stock::record(
                ectx.state,
                owner,
                FactKind::Consumed,
                format!("Bought {}", catalog.get(def).name),
                Some((&cost.item, cost.amount)),
            );
        }
    }
    let tick = state.tick;
    let stall = &mut state
        .entities
        .get_mut(shop)
        .and_then(|e| e.shop.as_mut())
        .expect("a shop")
        .stalls[slot];
    stall.stock -= 1;
    if stall.restock_at.is_none() {
        stall.restock_at = Some(tick + stall.ware.restock_ticks);
    }
    let on_pickup = matches!(
        catalog.items.get(def).map(|d| &d.effect),
        Some(ItemEffect::Consumable {
            on_pickup: Some(true),
            ..
        })
    );
    if on_pickup {
        crate::items::effects::power_up(ctx, state, hero, def);
        return Ok(());
    }
    let e = state.entities.get_mut(hero).expect("hero");
    let q = crate::items::equipment(e);
    let free = q.slots.iter().position(Option::is_none).expect("free slot");
    q.slots[free] = Some(def);
    if q.runtime.len() != q.slots.len() {
        q.runtime.resize(q.slots.len(), None);
    }
    q.runtime[free] = None;
    Ok(())
}

/// A shop's wares as a viewer sees them.
#[derive(Debug, Clone, PartialEq)]
pub struct WareView {
    pub item: Def,
    pub price: Vec<canopy_content::game::Cost>,
    pub stock: u32,
    pub most: u32,
    /// Ticks until the next one grows back, and the whole wait.
    pub restock: Option<(u32, u32)>,
}

/// What anyone who sees a shop knows of its wares.
pub fn view(catalog: &Catalog, tick: u32, e: &Entity) -> Option<Vec<WareView>> {
    let shop = e.shop.as_ref()?;
    Some(
        shop.stalls
            .iter()
            .filter_map(|st| {
                Some(WareView {
                    item: catalog.find(st.ware.item.as_str())?,
                    price: st.ware.price.clone(),
                    stock: st.stock,
                    most: st.ware.stock,
                    restock: st
                        .restock_at
                        .map(|t| (t.saturating_sub(tick), st.ware.restock_ticks)),
                })
            })
            .collect(),
    )
}
