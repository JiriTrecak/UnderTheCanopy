//! Ability events for presentation (old `AbilityEvent`): what the renderer and the effects
//! system need to play visuals and sounds. They are cosmetic and never saved; each lists the
//! owners who may see it, decided when it happened.

use super::library::AbilityId;
use crate::entity::EntityId;
use canopy_content::placement::Owner;

/// The kinds of ability events (the old ~40 event names).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub enum EventKind {
    Accepted,
    Released,
    Cancelled,
    Finished,
    WaveStarted,
    Wave,
    Projectile,
    Impact,
    Healed,
    Damaged,
    Drained,
    ManaRestored,
    StatusApplied,
    Dispelled,
    Summoned,
    Resurrected,
    RevivalStarted,
    Revived,
    RevivalCancelled,
    SplitStarted,
    SplitEnded,
    Teleported,
    VisionCreated,
    Converted,
    Contained,
    ReleasedContained,
    Digested,
    Sacrificed,
    Returned,
    Death,
    Kill,
    WeaponRelease,
    Interval,
    CriticalStrike,
    Evaded,
    Cleaved,
    WeaponEnhanced,
    EnhancedHit,
    WeaponOrdered,
    WeaponOrderCancelled,
}

/// A point of an event: milli-cells and absolute height in world units.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize)]
pub struct EventPoint {
    pub x: i32,
    pub y: i32,
    pub height: f64,
}

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AbilityEvent {
    pub id: u32,
    pub cast: u32,
    pub tick: u32,
    pub ability: AbilityId,
    pub kind: EventKind,
    pub caster: EntityId,
    /// `None` for point targets.
    pub target: Option<EntityId>,
    /// Where it comes from (the caster when it happened, or the stated origin).
    pub origin: EventPoint,
    /// Where it lands.
    pub point: EventPoint,
    pub viewers: Vec<Owner>,
    pub amount: Option<i64>,
    pub status: Option<String>,
    pub reason: Option<String>,
    pub duration_ticks: Option<u32>,
    pub radius: Option<f64>,
    pub until_death: bool,
    pub spawned: Vec<EntityId>,
}

/// Recent events (old `observedEvents`: the last 400 ticks, at most 1,024).
#[derive(Debug, Clone, Default)]
pub struct Events {
    list: Vec<AbilityEvent>,
    sequence: u32,
    /// Recent deaths, for observers' statistics (replays). Cosmetic like the rest.
    deaths: std::collections::VecDeque<Fallen>,
    death_sequence: u32,
}

/// One death: who died, whose it was and who is credited with the kill (`None` when no
/// lethal blow is known: scripts, sacrifices, expired summons). Cosmetic, never saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fallen {
    /// Counts deaths from 1 since the world was built or restored.
    pub sequence: u32,
    pub tick: u32,
    pub entity: EntityId,
    pub definition: crate::catalog::Def,
    pub owner: Owner,
    pub killer: Option<Owner>,
    /// The entity that struck the lethal blow, when known.
    pub source: Option<EntityId>,
}

/// Deaths kept for observers: the last [`EVENT_TICKS`], at most this many.
const MAX_DEATHS: usize = 4096;

/// Ticks an event stays listed in views.
pub const EVENT_TICKS: u32 = 400;
const MAX_EVENTS: usize = 1024;

impl Events {
    pub fn push(&mut self, mut event: AbilityEvent) {
        self.sequence += 1;
        event.id = self.sequence;
        self.list.push(event);
        if self.list.len() > MAX_EVENTS {
            let excess = self.list.len() - MAX_EVENTS;
            self.list.drain(..excess);
        }
    }

    /// Events `owner` may see (all for observers) from the last [`EVENT_TICKS`].
    pub fn observed(&self, owner: Option<Owner>, tick: u32) -> Vec<AbilityEvent> {
        self.list
            .iter()
            .filter(|e| owner.is_none_or(|o| e.viewers.contains(&o)))
            .filter(|e| tick.saturating_sub(e.tick) <= EVENT_TICKS)
            .cloned()
            .collect()
    }

    /// Takes every event (tests and tools).
    pub fn drain(&mut self) -> Vec<AbilityEvent> {
        std::mem::take(&mut self.list)
    }

    pub fn all(&self) -> &[AbilityEvent] {
        &self.list
    }

    /// Notes a death (observers' statistics).
    pub fn fallen(&mut self, mut death: Fallen) {
        self.death_sequence += 1;
        death.sequence = self.death_sequence;
        while self
            .deaths
            .front()
            .is_some_and(|d| death.tick.saturating_sub(d.tick) > EVENT_TICKS)
            || self.deaths.len() >= MAX_DEATHS
        {
            self.deaths.pop_front();
        }
        self.deaths.push_back(death);
    }

    /// The deaths of the last [`EVENT_TICKS`], oldest first.
    pub fn deaths(&self) -> impl Iterator<Item = &Fallen> {
        self.deaths.iter()
    }
}
