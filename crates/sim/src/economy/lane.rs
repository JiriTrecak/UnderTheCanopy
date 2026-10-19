//! A building's one queue (D73): its training, research and hero revivals in the order the
//! player queued them. Only the first advances; the rest wait, as in Warcraft III.
//!
//! Each item records its place: training and revival entries by their id, research by its
//! `order`, all drawn from the match's queue counter. Each kind keeps its own list (and
//! capacity), each list in queued order, so the building's head is the smallest of the
//! three heads. Automatic production (planting, a building's own output) is not queued and
//! runs as before; an upgrade holds the whole building.

use super::{Production, QueueEntry, ResearchEntry, ResearchQueue, RevivalEntry, RevivalQueue};
use crate::{entity::Entity, state::State};

/// Which kind of work leads a building's queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Lane {
    Production,
    Research,
    Revival,
}

/// One queued item.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Item<'a> {
    Training(&'a QueueEntry),
    Research(&'a ResearchEntry),
    Revival(&'a RevivalEntry),
}

/// The order key of research: one from before D73 (0) goes after everything numbered.
fn research_order(r: &ResearchEntry) -> u32 {
    if r.order == 0 { u32::MAX } else { r.order }
}

impl Item<'_> {
    pub fn lane(&self) -> Lane {
        match self {
            Self::Training(_) => Lane::Production,
            Self::Research(_) => Lane::Research,
            Self::Revival(_) => Lane::Revival,
        }
    }

    /// Where it stands in the building's queue: the smaller goes first; on a tie (only
    /// unnumbered research from an old save) training goes before research.
    pub fn key(&self) -> (u32, Lane) {
        let order = match self {
            Self::Training(q) => q.id,
            Self::Research(r) => research_order(r),
            Self::Revival(r) => r.id,
        };
        (order, self.lane())
    }
}

/// The building's queued items in order, from its three queues (as an entity or a view
/// holds them).
pub fn items<'a>(
    production: Option<&'a Production>,
    research: Option<&'a ResearchQueue>,
    revival: Option<&'a RevivalQueue>,
) -> Vec<Item<'a>> {
    let mut out: Vec<Item> = production
        .iter()
        .flat_map(|p| p.queue.iter().map(Item::Training))
        .chain(
            research
                .iter()
                .flat_map(|r| r.queue.iter().map(Item::Research)),
        )
        .chain(
            revival
                .iter()
                .flat_map(|r| r.queue.iter().map(Item::Revival)),
        )
        .collect();
    // Stable: each kind's own list is already in queued order.
    out.sort_by_key(Item::key);
    out
}

/// [`items`] of an entity.
pub fn of(e: &Entity) -> Vec<Item<'_>> {
    items(
        e.production.as_deref(),
        e.research.as_ref(),
        e.revival.as_ref(),
    )
}

/// The kind of the item that leads the building's queue, if any is queued.
pub fn head(e: &Entity) -> Option<Lane> {
    let training = e
        .production
        .as_ref()
        .and_then(|p| p.queue.first())
        .map(Item::Training);
    let research = e
        .research
        .as_ref()
        .and_then(|r| r.queue.first())
        .map(Item::Research);
    let revival = e
        .revival
        .as_ref()
        .and_then(|r| r.queue.first())
        .map(Item::Revival);
    [training, research, revival]
        .into_iter()
        .flatten()
        .min_by_key(Item::key)
        .map(|i| i.lane())
}

/// The next place in a building's queue.
pub fn next(state: &mut State) -> u32 {
    let id = state.counters.queue;
    state.counters.queue += 1;
    id
}

/// Numbers research queued before D73 (saves without `order`) after everything queued at
/// its building, in entity order: research then waits for the training queued with it.
pub fn order_legacy(state: &mut State) {
    let mut counter = state.counters.queue;
    for e in state.entities.iter_mut() {
        for r in e.research.iter_mut().flat_map(|r| r.queue.iter_mut()) {
            if r.order == 0 {
                r.order = counter;
                counter += 1;
            }
        }
    }
    state.counters.queue = counter;
}
