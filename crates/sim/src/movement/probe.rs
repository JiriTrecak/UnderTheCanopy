//! What steering decided for each unit in the last movement step, for the combat lab's
//! debug view (docs/editor.md, "The Combat Lab"). Recording is off unless asked for
//! ([`crate::World::set_probe`]); it only writes down what steering computed and never
//! feeds back, so a run is the same with it on or off.

use crate::entity::EntityId;
use std::collections::BTreeMap;

/// One step's traces, by unit.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Probe {
    /// The tick the traces were taken on.
    pub tick: u32,
    pub units: BTreeMap<EntityId, Trace>,
}

/// How one unit steered, in milli-cells per tick.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Trace {
    /// The waypoint it steered for.
    pub waypoint: Option<(i32, i32)>,
    /// Straight at the waypoint, as far as it may go this tick.
    pub desired: (i64, i64),
    /// The room it kept from neighbours (as added: half the push).
    pub separation: (i64, i64),
    /// The sidestep from bodies ahead.
    pub avoidance: (i64, i64),
    /// What it asked for: the sum, limited to its reach.
    pub velocity: (i64, i64),
    /// The headings it tried, in order, and what stopped each.
    pub tried: Vec<((i64, i64), Verdict)>,
    pub outcome: Outcome,
    /// Ticks since it last made progress.
    pub stalled: u32,
}

/// Why a heading was or was not taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Terrain or a building in the way.
    Terrain,
    /// A foe or a planted ally in the way.
    Bodies,
    /// Taken.
    Clear,
}

/// How the step ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Outcome {
    /// Moved along its route.
    #[default]
    Moved,
    /// Turned on the spot first (a reversal).
    Turned,
    /// Stopped with its group at the goal.
    Settled,
    /// Every heading blocked; waiting.
    Blocked,
    /// Held up long enough to negotiate: a yield, a detour or a side step.
    Recovering,
    /// Held up so long that it planned its route again.
    Replanned,
    /// Terrain across its way: planned again.
    Rerouted,
    /// Following a detour round the bodies in its way.
    Detour,
}

impl Outcome {
    pub fn label(self) -> &'static str {
        match self {
            Self::Moved => "moving",
            Self::Turned => "turning",
            Self::Settled => "settled",
            Self::Blocked => "blocked",
            Self::Recovering => "negotiating",
            Self::Replanned => "replanned",
            Self::Rerouted => "rerouted",
            Self::Detour => "detouring",
        }
    }
}

/// Records into `trace` when probing.
pub(super) fn note(trace: &mut Option<Trace>, f: impl FnOnce(&mut Trace)) {
    if let Some(t) = trace {
        f(t);
    }
}
