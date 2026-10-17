//! What observers count from tick to tick (the replay's summary panels, D60): goods
//! delivered (income over the last minute and gathered in all), and units and buildings
//! lost and killed with their value. The world only shows each tick's deliveries and its
//! recent deaths, so the tally follows every step; replays keep a copy with each keyframe
//! so a seek resumes the counts.

use canopy_content::{Id, placement::Owner};
use canopy_sim::World;
use std::collections::{BTreeMap, VecDeque};

/// Ticks of deliveries the income covers (one minute of match time).
pub const INCOME_TICKS: u32 = 2400;

/// One player's counts.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PlayerTally {
    /// Goods delivered to the player's drop-offs since the start, by item.
    pub gathered: BTreeMap<Id, u32>,
    pub units_lost: u32,
    pub buildings_lost: u32,
    /// The creation cost of what was lost, all goods added up.
    pub value_lost: u32,
    pub units_killed: u32,
    pub buildings_killed: u32,
    pub value_killed: u32,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tally {
    pub players: BTreeMap<u8, PlayerTally>,
    /// Deliveries of the last minute: tick, player, item, amount.
    recent: VecDeque<(u32, u8, Id, u32)>,
    /// The first tick observed (income before a minute has passed).
    first: Option<u32>,
    /// The latest tick observed.
    tick: u32,
    /// The newest death counted (the world's death sequence).
    death: u32,
}

/// What a definition cost to make: every good of its bill added up.
pub fn value(world: &World, definition: canopy_sim::catalog::Def) -> u32 {
    canopy_sim::economy::stock::price(world.catalog(), definition)
        .iter()
        .map(|c| c.amount)
        .sum()
}

impl Tally {
    /// Counts the step the world just took.
    pub fn observe(&mut self, world: &World) {
        let tick = world.tick();
        self.first.get_or_insert(tick);
        self.tick = tick;
        for d in &world.economy().deliveries {
            let Owner::Player(p) = d.owner else {
                continue;
            };
            if d.amount == 0 {
                continue;
            }
            *self
                .players
                .entry(p)
                .or_default()
                .gathered
                .entry(d.item.clone())
                .or_default() += d.amount;
            self.recent.push_back((tick, p, d.item.clone(), d.amount));
        }
        while self
            .recent
            .front()
            .is_some_and(|r| r.0 + INCOME_TICKS <= tick)
        {
            self.recent.pop_front();
        }
        let catalog = world.catalog();
        let mut newest = self.death;
        for f in world.events().deaths().filter(|f| f.sequence > self.death) {
            newest = newest.max(f.sequence);
            let d = catalog.get(f.definition);
            let building = d.kind == canopy_content::game::Kind::Building;
            if d.kind != canopy_content::game::Kind::Unit && !building {
                continue;
            }
            let cost = value(world, f.definition);
            if let Owner::Player(p) = f.owner {
                let t = self.players.entry(p).or_default();
                if building {
                    t.buildings_lost += 1;
                } else {
                    t.units_lost += 1;
                }
                t.value_lost += cost;
            }
            if let Some(Owner::Player(k)) = f.killer
                && f.killer != Some(f.owner)
            {
                let t = self.players.entry(k).or_default();
                if building {
                    t.buildings_killed += 1;
                } else {
                    t.units_killed += 1;
                }
                t.value_killed += cost;
            }
        }
        self.death = newest;
    }

    /// The world was rebuilt from a save: its death log starts again.
    pub fn restored(&mut self) {
        self.death = 0;
    }

    /// `player`'s deliveries of `item` per minute of match time over the last minute.
    pub fn per_minute(&self, player: u8, item: &str) -> u32 {
        let sampled = self
            .tick
            .saturating_sub(self.first.unwrap_or(self.tick))
            .min(INCOME_TICKS);
        if sampled < 40 {
            return 0;
        }
        let sum: u32 = self
            .recent
            .iter()
            .filter(|r| r.1 == player && r.2.as_str() == item)
            .map(|r| r.3)
            .sum();
        (u64::from(sum) * u64::from(INCOME_TICKS) / u64::from(sampled)) as u32
    }

    /// Seconds of match time the income has sampled (at most 60).
    pub fn sampled_seconds(&self) -> u32 {
        self.tick
            .saturating_sub(self.first.unwrap_or(self.tick))
            .min(INCOME_TICKS)
            / 40
    }
}
