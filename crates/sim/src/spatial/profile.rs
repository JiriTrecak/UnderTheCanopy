//! Per-definition movement and collision data, resolved once from content (old
//! `Spatial.dimensions`, `locomotion`, `minimumGroundBody`).

use crate::{
    catalog::{Catalog, Def},
    terrain::{footprint::Extent, round_half_up},
};
use canopy_content::{conditions::Locomotion, game::Kind};

/// A collision body: square navigation footprint of `radius` cells, `height` world units
/// tall, kept `spacing` apart in formations.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Body {
    pub radius: f64,
    pub height: f64,
    pub spacing: f64,
    pub air: bool,
}

impl Body {
    /// Radius in milli-cells.
    pub fn radius_milli(&self) -> i32 {
        round_half_up(self.radius * 1000.0) as i32
    }
}

/// How a unit moves.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Motion {
    /// Cells per second.
    pub speed: f64,
    /// Cells per second while strolling idle.
    pub walk_speed: Option<f64>,
    /// Degrees per second.
    pub turn_rate: u32,
    pub idle_wander: bool,
}

#[derive(Debug, Clone)]
pub struct Profile {
    pub kind: Kind,
    pub body: Body,
    /// Whether the definition declares dimensions (units do).
    pub sized: bool,
    pub footprint: Extent,
    pub has_footprint: bool,
    pub collision_radius: Option<f64>,
    pub motion: Option<Motion>,
    pub player_control: bool,
    pub work: bool,
    pub combat: bool,
}

/// Profiles for every definition, by [`Def`].
#[derive(Debug, Clone)]
pub struct Profiles {
    profiles: Vec<Profile>,
    /// The smallest ground body: the envelope for queries without an actor.
    pub navigation_body: Body,
    /// The largest unit radius in milli-cells (broad-phase reach).
    pub max_radius: i32,
}

impl Profiles {
    pub fn new(catalog: &Catalog) -> Self {
        let definitions = &catalog.content.definitions;
        let ground = || {
            definitions.iter().filter(|d| {
                d.kind == Kind::Unit
                    && d.behaviors.movement.as_ref().and_then(|m| m.locomotion) != Some(Locomotion::Air)
            })
        };
        let navigation_body = if ground().next().is_some() {
            let min = |f: fn(&canopy_content::game::Dimensions) -> f64| {
                ground()
                    .filter_map(|d| d.dimensions.as_ref().map(f))
                    .fold(f64::INFINITY, f64::min)
            };
            Body {
                radius: min(|d| d.radius),
                height: min(|d| d.height),
                spacing: min(|d| d.formation_spacing),
                air: false,
            }
        } else {
            Body {
                radius: 0.0,
                height: 0.0,
                spacing: 0.0,
                air: false,
            }
        };
        let max_radius = definitions
            .iter()
            .filter(|d| d.kind == Kind::Unit)
            .filter_map(|d| d.dimensions.as_ref().map(|b| b.radius))
            .fold(navigation_body.radius, f64::max);
        let profiles = definitions
            .iter()
            .map(|d| {
                let movement = d.behaviors.movement.as_ref();
                let air = movement.and_then(|m| m.locomotion) == Some(Locomotion::Air);
                let body = match &d.dimensions {
                    Some(b) => Body {
                        radius: b.radius,
                        height: b.height,
                        spacing: b.formation_spacing,
                        air,
                    },
                    None => Body {
                        air,
                        ..navigation_body
                    },
                };
                Profile {
                    kind: d.kind,
                    body,
                    sized: d.dimensions.is_some(),
                    footprint: Extent::of(d.footprint.as_ref()),
                    has_footprint: d.footprint.is_some(),
                    collision_radius: d.collision_radius,
                    motion: movement.map(|m| Motion {
                        speed: m.speed,
                        walk_speed: m.walk_speed,
                        turn_rate: m.turn_rate,
                        idle_wander: m.idle_wander == Some(true),
                    }),
                    player_control: d.behaviors.player_control.is_some(),
                    work: d.behaviors.work.is_some(),
                    combat: d.behaviors.combat.is_some(),
                }
            })
            .collect();
        Self {
            profiles,
            navigation_body,
            max_radius: round_half_up(max_radius * 1000.0) as i32,
        }
    }

    pub fn get(&self, def: Def) -> &Profile {
        &self.profiles[usize::from(def.0)]
    }
}
