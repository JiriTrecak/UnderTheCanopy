//! The constants movement and the attack approach are tuned by (docs/decisions.md D100,
//! D101). Matches always play [`Tuning::default`]; the Combat Lab may change them for its
//! own runs (`World::set_tuning`) to try values side by side. All integers: milli-cells,
//! ticks and hundredths, so every seat computes the same.

/// How melee units close on a target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Approach {
    /// The old game's: each picks a free cell within reach, nearest first, and routes there
    /// (the game's, until the others measure better in the lab).
    #[default]
    Spots,
    /// StarCraft II's: walk at the target, stop in reach; the ring forms as units slide
    /// round the planted ones in front.
    Direct,
    /// Attackers of one target share the ring round it, each taking the spot on its own
    /// side in the group's order; those bound for the far side walk round the flank, those
    /// beyond what the ring holds wait in a second row (D101; a lab mode for now).
    Ring,
}

impl Approach {
    pub const ALL: [Self; 3] = [Self::Spots, Self::Direct, Self::Ring];

    pub fn label(self) -> &'static str {
        match self {
            Self::Spots => "Spots (old)",
            Self::Direct => "Direct (StarCraft II)",
            Self::Ring => "Ring fill",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tuning {
    // ----- steering -----
    /// Room kept between bodies beyond touching, milli-cells.
    pub gap: i64,
    /// A unit stops beside allies standing at its goal within this of it (plus three of its
    /// radii), milli-cells.
    pub arrive: i64,
    /// How far ahead, in ticks of travel, a unit starts easing round bodies in its way.
    pub look_ticks: i64,
    /// Ticks blocked beside its goal before a unit settles there.
    pub settle_ticks: u32,
    /// Ticks held up by a solid body before negotiating past it (a yield or a detour).
    pub negotiate_ticks: u32,
    /// Ticks held up before planning the route again.
    pub reroute_ticks: u32,
    // ----- pushing (soft bodies) -----
    /// Rounds of pushing a tick: how far through a crowd a shove travels at once.
    pub push_rounds: u32,
    /// The most a body is pushed in one tick, milli-cells.
    pub push_most: i64,
    /// Room kept beyond touching once pushed apart, milli-cells.
    pub push_room: i64,
    /// How much of their overlap two allies on the move resolve a tick, hundredths (each
    /// takes half): lower squeezes more.
    pub squeeze: i64,
    /// An idle ally shoved by a mover steps aside out of its path (else straight away).
    pub aside: bool,
    // ----- attack approach -----
    pub approach: Approach,
    /// Ring spots are this many hundredths of a body apart.
    pub ring_spacing: i64,
    /// Fast melee units aim ahead of a target running away, overtake it and close the ring
    /// in front (level 3 of D101; off in matches, a lab switch).
    pub lead_fleeing: bool,
}

impl Default for Tuning {
    fn default() -> Self {
        Self {
            gap: 100,
            arrive: 3000,
            look_ticks: 10,
            settle_ticks: 8,
            negotiate_ticks: 12,
            reroute_ticks: 40,
            push_rounds: 4,
            push_most: 350,
            push_room: 20,
            squeeze: 50,
            aside: true,
            approach: Approach::Spots,
            ring_spacing: 105,
            lead_fleeing: false,
        }
    }
}
