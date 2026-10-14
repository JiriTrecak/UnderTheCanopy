//! Goods: prices, what stores hold and may take, reservations and refunds, and the
//! accounting facts (old `price`, `available`, `balance`, `room`, `reserveCost`, `pay`,
//! `admitProject`, `refund`, `GameContext.event`).
//!
//! Currencies live in player wallets; other goods physically sit in buildings. A project
//! (construction site, queued unit, research, upgrade, revival) holds its whole bill from
//! the moment it is accepted.

use super::{Ctx, Index, Jobs};
use crate::{
    catalog::{Catalog, Def},
    entity::{Entity, EntityId, Stock},
    state::{Fact, FactKind, MAX_FACTS, State},
};
use canopy_content::{
    Id,
    game::{Cost, Creation},
    placement::Owner,
};

/// A definition's creation bill (old `price`).
pub fn price(catalog: &Catalog, def: Def) -> &[Cost] {
    catalog
        .get(def)
        .creation
        .as_ref()
        .map_or(&[], Creation::items)
}

pub fn currency(catalog: &Catalog, item: &str) -> bool {
    catalog
        .content
        .definition(item)
        .is_some_and(|d| d.currency == Some(true))
}

pub fn quantity(stock: &Stock, item: &str) -> u32 {
    stock.get(item).copied().unwrap_or(0)
}

pub fn total(stock: &Stock) -> u32 {
    stock.values().sum()
}

pub fn add(stock: &mut Stock, item: &Id, amount: u32) {
    if amount > 0 {
        *stock.entry(item.clone()).or_default() += amount;
    }
}

/// Removes goods that must be there (old `add` with a negative amount).
pub fn take(stock: &mut Stock, item: &str, amount: u32) {
    if amount == 0 {
        return;
    }
    let held = stock.get_mut(item).expect("goods to take");
    debug_assert!(*held >= amount, "negative inventory {item}");
    *held = held.saturating_sub(amount);
    if *held == 0 {
        stock.remove(item);
    }
}

/// Whether the store's inventory lacks any part of `bill` (old `missing`).
pub fn missing(stock: &Stock, bill: &[Cost]) -> bool {
    bill.iter()
        .any(|c| quantity(stock, c.item.as_str()) < c.amount)
}

fn dropoff(catalog: &Catalog, e: &Entity) -> bool {
    catalog
        .get(e.definition)
        .behaviors
        .storage
        .as_ref()
        .is_some_and(|s| s.dropoff == Some(true))
}

/// Physical goods a completed drop-off can spend: its stock less what its queued units
/// already hold (old `available`).
pub fn available(catalog: &Catalog, e: &Entity, item: &str) -> u32 {
    if currency(catalog, item) || e.construction.is_some() || !dropoff(catalog, e) {
        return 0;
    }
    let reserved: u32 = e.production.as_ref().map_or(0, |p| {
        p.queue
            .iter()
            .flat_map(|q| price(catalog, q.definition))
            .filter(|c| c.item.as_str() == item)
            .map(|c| c.amount)
            .sum()
    });
    quantity(&e.inventory, item).saturating_sub(reserved)
}

/// What `owner` can spend of `item` (old `balance`).
pub fn balance(state: &State, catalog: &Catalog, index: &Index, owner: Owner, item: &str) -> u32 {
    if currency(catalog, item) {
        return state
            .wallets
            .get(&owner)
            .map_or(0, |w| quantity(w, item));
    }
    index
        .buildings
        .iter()
        .filter_map(|&id| state.entities.get(id))
        .filter(|e| e.owner == owner && super::alive(e))
        .map(|e| available(catalog, e, item))
        .sum()
}

/// A store's capacity: a site holds its bill, a building its storage (old `capacity`).
pub fn capacity(catalog: &Catalog, e: &Entity) -> u32 {
    if e.construction.is_some() {
        price(catalog, e.definition).iter().map(|c| c.amount).sum()
    } else {
        catalog
            .get(e.definition)
            .behaviors
            .storage
            .as_ref()
            .map_or(0, |s| s.capacity)
    }
}

/// Space left at a store once inbound goods arrive; unlimited for currencies at a
/// completed drop-off (old `room`).
pub fn room(catalog: &Catalog, jobs: &Jobs, e: &Entity, item: Option<&str>) -> u32 {
    if item.is_some_and(|i| currency(catalog, i)) && e.construction.is_none() && dropoff(catalog, e)
    {
        return u32::MAX;
    }
    capacity(catalog, e)
        .saturating_sub(total(&e.inventory))
        .saturating_sub(jobs.incoming(e.id))
}

/// Whether the store accepts `item` (old `legalStore`).
pub fn accepts(catalog: &Catalog, e: &Entity, item: &str) -> bool {
    catalog
        .get(e.definition)
        .behaviors
        .storage
        .as_ref()
        .is_some_and(|s| s.accepts.iter().any(|a| a.as_str() == item))
}

/// Where reserved goods come from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Holder {
    Wallet(Owner),
    Store(EntityId),
}

/// Goods set aside for a bill, not yet taken.
pub type Reservation = Vec<(Holder, Id, u32)>;

/// Finds `bill` among `owner`'s wallet and stores without changing anything; `None` when
/// any part is short (old `reserveCost`).
pub fn reserve(
    state: &State,
    catalog: &Catalog,
    index: &Index,
    owner: Owner,
    bill: &[Cost],
) -> Option<Reservation> {
    let mut entries = Vec::new();
    for cost in bill {
        let item = cost.item.as_str();
        if currency(catalog, item) {
            let held = state.wallets.get(&owner).map_or(0, |w| quantity(w, item));
            if held < cost.amount {
                return None;
            }
            if cost.amount > 0 {
                entries.push((Holder::Wallet(owner), cost.item.clone(), cost.amount));
            }
            continue;
        }
        let mut left = cost.amount;
        for &id in &index.buildings {
            if left == 0 {
                break;
            }
            let Some(source) = state.entities.get(id) else {
                continue;
            };
            if source.owner != owner || !super::alive(source) {
                continue;
            }
            let amount = left.min(available(catalog, source, item));
            if amount > 0 {
                entries.push((Holder::Store(id), cost.item.clone(), amount));
                left -= amount;
            }
        }
        if left > 0 {
            return None;
        }
    }
    Some(entries)
}

fn holder_stock(state: &mut State, holder: Holder) -> Option<&mut Stock> {
    match holder {
        Holder::Wallet(owner) => Some(state.wallets.entry(owner).or_default()),
        Holder::Store(id) => state.entities.get_mut(id).map(|e| &mut e.inventory),
    }
}

/// Spends a reservation (old `pay`).
pub fn pay(state: &mut State, reservation: &Reservation) {
    for (holder, item, amount) in reservation {
        if let Some(stock) = holder_stock(state, *holder) {
            take(stock, item.as_str(), *amount);
        }
    }
}

/// Moves a reservation into the project's inventory (old `admitProject`).
pub fn admit(state: &mut State, project: EntityId, reservation: &Reservation) {
    for (holder, item, amount) in reservation {
        if let Some(stock) = holder_stock(state, *holder) {
            take(stock, item.as_str(), *amount);
        }
        if let Some(e) = state.entities.get_mut(project) {
            add(&mut e.inventory, item, *amount);
        }
    }
}

/// Returns up to `bill` of the project's inventory to its owner: currencies to the wallet,
/// goods into completed drop-offs by id while they have room (old `refund`). Whatever does
/// not fit stays in the project.
pub fn refund(ctx: &mut Ctx, project: EntityId, bill: &[(Id, u32)]) {
    let Some(e) = ctx.state.entities.get(project) else {
        return;
    };
    let owner = e.owner;
    let halls: Vec<EntityId> = ctx
        .index
        .buildings
        .iter()
        .copied()
        .filter(|&id| {
            id != project
                && ctx.state.entities.get(id).is_some_and(|h| {
                    super::alive(h)
                        && h.owner == owner
                        && h.construction.is_none()
                        && dropoff(ctx.catalog, h)
                })
        })
        .collect();
    for (item, n) in bill {
        let held = ctx
            .state
            .entities
            .get(project)
            .map_or(0, |e| quantity(&e.inventory, item.as_str()));
        let mut left = (*n).min(held);
        if currency(ctx.catalog, item.as_str()) {
            add(ctx.state.wallets.entry(owner).or_default(), item, left);
            if let Some(e) = ctx.state.entities.get_mut(project) {
                take(&mut e.inventory, item.as_str(), left);
            }
            continue;
        }
        for &h in &halls {
            if left == 0 {
                break;
            }
            let hall = ctx.state.entities.get(h).expect("hall");
            if !accepts(ctx.catalog, hall, item.as_str()) {
                continue;
            }
            let amount = left.min(room(ctx.catalog, &ctx.state.economy.jobs, hall, None));
            if let Some(hall) = ctx.state.entities.get_mut(h) {
                add(&mut hall.inventory, item, amount);
            }
            if let Some(e) = ctx.state.entities.get_mut(project) {
                take(&mut e.inventory, item.as_str(), amount);
            }
            left -= amount;
        }
    }
}

/// Puts a paid task's bill back on the building and refunds it (old `refundCost`).
pub fn refund_cost(ctx: &mut Ctx, building: EntityId, bill: &[Cost]) {
    let bill: Vec<(Id, u32)> = bill.iter().map(|c| (c.item.clone(), c.amount)).collect();
    if let Some(e) = ctx.state.entities.get_mut(building) {
        for (item, amount) in &bill {
            add(&mut e.inventory, item, *amount);
        }
    }
    refund(ctx, building, &bill);
}

/// Records a player-facing fact; goods produced, consumed or lost also enter the
/// accounting (old `GameContext.event`).
pub fn record(
    state: &mut State,
    owner: Owner,
    kind: FactKind,
    message: impl Into<String>,
    goods: Option<(&Id, u32)>,
) {
    let id = state.counters.fact;
    state.counters.fact += 1;
    let goods = goods.filter(|&(_, amount)| amount > 0);
    state.facts.push(Fact {
        id,
        tick: state.tick,
        owner,
        kind,
        message: message.into(),
        item: goods.map(|(item, _)| item.clone()),
        amount: goods.map(|(_, amount)| amount),
    });
    if state.facts.len() > MAX_FACTS {
        state.facts.remove(0);
    }
    if let Some((item, amount)) = goods {
        let book = match kind {
            FactKind::Produced => Some(&mut state.accounting.produced),
            FactKind::Consumed => Some(&mut state.accounting.consumed),
            FactKind::Lost => Some(&mut state.accounting.lost),
            _ => None,
        };
        if let Some(book) = book {
            add(book, item, amount);
        }
    }
}

/// A bill as `(item, amount)` pairs.
pub fn bill(costs: &[Cost]) -> Vec<(Id, u32)> {
    costs.iter().map(|c| (c.item.clone(), c.amount)).collect()
}
