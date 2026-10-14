//! The cheap desync signal sent every 8 ticks (docs/decisions.md D13): counters, stocks,
//! outcome and a rotating sample of at most 32 actors. Not a proof of equal state.

use crate::{entity::Stock, state::State};
use canopy_content::placement::Owner;

/// Ticks between fingerprints in multiplayer.
pub const EVERY: u32 = 8;
/// Actors sampled per fingerprint.
pub const ACTORS: usize = 32;

/// FNV-1a over 32-bit words.
pub struct Fnv(u32);

impl Default for Fnv {
    fn default() -> Self {
        Self(0x811c_9dc5)
    }
}

impl Fnv {
    pub fn word(&mut self, value: u32) {
        self.0 = (self.0 ^ value).wrapping_mul(0x0100_0193);
    }

    pub fn int(&mut self, value: i64) {
        self.word(value as u32);
        self.word((value >> 32) as u32);
    }

    pub fn option(&mut self, value: Option<i64>) {
        match value {
            Some(value) => self.int(value),
            None => self.word(u32::MAX),
        }
    }

    pub fn text(&mut self, text: &str) {
        let mut hash = Fnv::default();
        text.bytes().for_each(|b| hash.word(u32::from(b)));
        self.word(hash.0);
    }

    pub fn owner(&mut self, owner: Owner) {
        self.word(owner_code(owner));
    }

    pub fn stock(&mut self, stock: &Stock) {
        self.int(stock.len() as i64);
        for (item, amount) in stock {
            self.text(item.as_str());
            self.word(*amount);
        }
    }

    pub fn finish(&self) -> u32 {
        self.0
    }
}

/// The fingerprint of `state` at its current tick.
pub fn fingerprint(state: &State) -> u32 {
    let mut h = Fnv::default();
    h.word(state.tick);
    h.word(state.random);
    h.int(state.entities.len() as i64);
    let c = &state.counters;
    for counter in [
        c.entity, c.job, c.queue, c.fact, c.cast, c.missile, c.shell, c.vision,
    ] {
        h.word(counter);
    }
    h.stock(&state.accounting.produced);
    h.stock(&state.accounting.consumed);
    h.stock(&state.accounting.lost);
    for (owner, stock) in &state.wallets {
        h.owner(*owner);
        h.stock(stock);
    }
    for (owner, count) in &state.objectives {
        h.owner(*owner);
        h.word(*count);
    }
    // Jobs are a small active-work set: harvests in full, the rest by count.
    for job in state.economy.jobs.iter() {
        if job.kind == crate::economy::JobKind::Harvest {
            h.word(job.id);
            h.word(job.worker.0);
            h.option(job.source.map(|s| i64::from(s.0)));
            h.word(job.target.0);
            h.word(job.phase as u32);
            h.option(job.arrived_tick.map(i64::from));
            h.word(job.progress);
            h.word(job.amount);
        }
    }
    h.int(state.economy.jobs.len() as i64);
    h.option(state.outcome.as_ref().map(|o| o.defeated.len() as i64));
    if let Some(outcome) = &state.outcome {
        h.option(outcome.winner.map(|w| i64::from(owner_code(w))));
        outcome.defeated.iter().for_each(|o| h.owner(*o));
    }

    let actors: Vec<_> = state
        .entities
        .iter()
        .filter(|e| e.unit.is_some() || e.construction.is_some())
        .collect();
    let stride = actors.len().div_ceil(ACTORS).max(1);
    let phase = (state.tick / EVERY) as usize % stride;
    for e in actors.iter().skip(phase).step_by(stride) {
        h.word(e.id.0);
        h.word(u32::from(e.definition.0));
        h.owner(e.owner);
        h.int(i64::from(e.position.x));
        h.int(i64::from(e.position.y));
        h.option(e.hp.map(i64::from));
        h.stock(&e.inventory);
        h.option(e.construction.as_ref().map(|c| i64::from(c.progress)));
        h.option(e.resource.as_ref().map(|r| i64::from(r.amount)));
        let cargo = e.unit.as_ref().and_then(|u| u.work.cargo.as_ref());
        h.option(cargo.map(|c| i64::from(c.amount)));
        if let Some(c) = cargo {
            h.text(c.item.as_str());
        }
        let p = e.production.as_ref();
        h.option(p.map(|p| p.queue.len() as i64));
        h.option(p.and_then(|p| p.queue.first()).map(|q| i64::from(q.id)));
        h.option(
            p.and_then(|p| p.active.as_ref())
                .map(|a| i64::from(a.progress)),
        );
        h.option(p.map(|p| i64::from(p.produced)));
        for q in e.revival.iter().flat_map(|r| &r.queue) {
            h.word(q.id);
            h.word(q.hero.0);
            h.word(q.level);
            h.word(q.progress);
        }
        h.option(e.upgrade.as_ref().map(|u| i64::from(u.progress)));
    }
    h.finish()
}

fn owner_code(owner: Owner) -> u32 {
    match owner {
        Owner::Nobody => 0,
        Owner::Player(slot) => u32::from(slot) + 1,
    }
}
