//! Goods buildings make by themselves (`income`, D62: a Substance Well pumps its deposit
//! with nobody working it).
//!
//! A building with `income` counts ticks while it stands complete and alive, held by a
//! player, and powered when it needs power (D66); every `intervalTicks` it adds `amount`
//! of the item to its owner's wallet. An unpowered building holds its count, as training
//! does. The count lives on the entity, so saves and replays resume it exactly.

use super::{Ctx, Delivery, alive, stock};
use crate::state::FactKind;
use canopy_content::placement::Owner;

/// Step 9: every building with income advances one tick, in id order.
pub fn advance(ctx: &mut Ctx) {
    let _span = tracing::trace_span!("income").entered();
    let tick = ctx.state.tick;
    for i in 0..ctx.index.buildings.len() {
        let id = ctx.index.buildings[i];
        let Some(e) = ctx.state.entities.get(id) else {
            continue;
        };
        let Some(income) = ctx.catalog.get(e.definition).income.as_ref() else {
            continue;
        };
        if !alive(e)
            || e.construction.is_some()
            || e.owner == Owner::Nobody
            || ctx.network.unpowered(id)
        {
            continue;
        }
        let owner = e.owner;
        let e = ctx.state.entities.get_mut(id).expect("building");
        e.income += 1;
        if e.income < income.interval_ticks {
            continue;
        }
        e.income = 0;
        stock::add(
            ctx.state.wallets.entry(owner).or_default(),
            &income.item,
            income.amount,
        );
        stock::record(
            ctx.state,
            owner,
            FactKind::Produced,
            "Income",
            Some((&income.item, income.amount)),
        );
        ctx.deliveries.push(Delivery {
            tick,
            owner,
            item: income.item.clone(),
            amount: income.amount,
            store: id,
            carrier: None,
        });
    }
}
