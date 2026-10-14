//! Missiles and shells (old `missiles.ts`, `missileState.ts`, `shellCombat.ts`,
//! `shellState.ts`).
//!
//! A **missile** homes on its target: its flight time is fixed at launch from the
//! distance and speed; it follows the target until impact and hits if the shot is clear
//! then. Once released it no longer depends on its shooter.
//!
//! A **shell** flies to the point where its target stood and lands after a fixed time,
//! hitting every eligible body of the owners it was aimed at within its radius and slowing
//! units. Its victims' owners are captured at launch.

use super::{
    Slow,
    resolve::Hit,
    rules::{DamageType, Delivery},
    senses::Senses,
};
use crate::{
    catalog::Def,
    entity::{Entity, EntityId, Milli},
    orders::Order,
    state::State,
    terrain::SurfacePoint,
};
use canopy_content::placement::Owner;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Ticks a landed missile stays listed for views.
pub const MISSILE_LINGER: u32 = 4;
/// Ticks a landed shell stays listed for views.
pub const SHELL_LINGER: u32 = 20;

/// A point in flight: exact position, floor, and height above the floor in centimetres.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Aim {
    pub position: Milli,
    pub surface: Option<String>,
    #[serde(default)]
    pub elevation_cm: i32,
}

impl Aim {
    fn of(point: SurfacePoint, surface: Option<String>) -> Self {
        Self {
            position: Milli {
                x: crate::terrain::round_half_up(point.x * 1000.0) as i32,
                y: crate::terrain::round_half_up(point.y * 1000.0) as i32,
            },
            surface,
            elevation_cm: crate::terrain::round_half_up(point.elevation * 100.0) as i32,
        }
    }

    fn point(&self, senses: &Senses) -> SurfacePoint {
        SurfacePoint {
            x: f64::from(self.position.x) / 1000.0,
            y: f64::from(self.position.y) / 1000.0,
            deck: self
                .surface
                .as_deref()
                .and_then(|s| senses.terrain.deck_id(s)),
            elevation: f64::from(self.elevation_cm) / 100.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Missile {
    pub id: u32,
    pub source: EntityId,
    pub target: EntityId,
    /// The shooter's definition (its weapon and look).
    pub definition: Def,
    pub owner: Owner,
    pub origin: Aim,
    /// Where the target is, updated while it lives.
    pub destination: Aim,
    pub launched: u32,
    pub impact: u32,
    /// Raw damage in thousandths, after charge and bonuses.
    pub damage_milli: u64,
    pub damage_type: u8,
    /// Owners that saw both ends at launch.
    pub viewers: Vec<Owner>,
    pub resolved: bool,
    /// The critical strike it carries (abilities).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub critical: Option<crate::abilities::AbilityId>,
    /// A weapon cast or attack bonus it carries (abilities).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub enhancement: Option<Box<crate::abilities::weapons::Enhancement>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Shell {
    pub id: u32,
    pub source: EntityId,
    pub definition: Def,
    pub owner: Owner,
    pub origin: Aim,
    /// The ground point it lands on.
    pub target: Aim,
    pub launched: u32,
    pub impact: u32,
    pub damage_milli: u64,
    pub damage_type: u8,
    pub radius_milli: i64,
    pub slow_permille: u32,
    pub slow_ticks: u32,
    /// Owners whose bodies it hits.
    pub victims: Vec<Owner>,
    pub viewers: Vec<Owner>,
    pub resolved: bool,
}

/// Owners who see both `a` and `b` now.
fn viewers(senses: &Senses, state: &State, a: &Entity, b: &Entity) -> Vec<Owner> {
    let sight = senses.sight(state);
    senses
        .teams
        .owners()
        .iter()
        .copied()
        .filter(|&o| sight.visible(o, a) && sight.visible(o, b))
        .collect()
}

/// Fires a missile from `a` at `b` (old `Missiles.launch`); returns its flight time.
pub fn launch_missile(
    senses: &Senses,
    state: &mut State,
    a: EntityId,
    b: EntityId,
    damage_milli: u64,
    critical: Option<crate::abilities::AbilityId>,
    enhancement: Option<Box<crate::abilities::weapons::Enhancement>>,
) -> u32 {
    let (ea, eb) = (
        state.entities.get(a).expect("shooter"),
        state.entities.get(b).expect("target"),
    );
    let weapon = senses.weapon(ea).expect("weapon");
    let Delivery::Missile { speed } = weapon.delivery else {
        return 0;
    };
    let origin = senses.eye(state, ea);
    let destination = senses.eye(state, eb);
    let (dx, dy) = (destination.x - origin.x, destination.y - origin.y);
    let distance = (dx * dx + dy * dy).sqrt();
    let flight =
        ((distance / f64::from(speed) * f64::from(crate::mission::TICKS_PER_SECOND)).ceil() as u32)
            .max(1);
    let missile = Missile {
        id: state.counters.missile,
        source: a,
        target: b,
        definition: senses.weapon_def(ea),
        owner: ea.owner,
        origin: Aim::of(origin, ea.surface.clone()),
        destination: Aim::of(destination, eb.surface.clone()),
        launched: state.tick,
        impact: state.tick + flight,
        damage_milli,
        damage_type: weapon.damage_type.0,
        viewers: viewers(senses, state, ea, eb),
        resolved: false,
        critical,
        enhancement,
    };
    state.counters.missile += 1;
    state.combat.missiles.push(missile);
    flight
}

/// Lobs a shell from `a` at where `b` stands (old `ShellCombat.launch`).
pub fn launch_shell(
    senses: &Senses,
    state: &mut State,
    a: EntityId,
    b: EntityId,
    damage_milli: u64,
) {
    let (ea, eb) = (
        state.entities.get(a).expect("shooter"),
        state.entities.get(b).expect("target"),
    );
    let weapon = senses.weapon(ea).expect("weapon");
    let Delivery::Shell {
        flight,
        mut radius_milli,
        mut slow_permille,
        slow_ticks,
    } = weapon.delivery
    else {
        return;
    };
    let id = senses.catalog.id(ea.definition);
    for r in crate::stats::research(state, ea.owner) {
        let Some(research) = senses.catalog.content.rules.research.get(r) else {
            continue;
        };
        for effect in research.effects.iter().filter(|e| e.units.contains(id)) {
            if let Some(radius) = effect.splash_radius {
                radius_milli =
                    radius_milli.max(crate::terrain::round_half_up(radius * 1000.0) as i64);
            }
            slow_permille = slow_permille.max(effect.splash_slow_permille.unwrap_or(0));
        }
    }
    let mut victims: Vec<Owner> = senses
        .teams
        .owners()
        .iter()
        .copied()
        .filter(|&o| ea.owner == Owner::Nobody || !senses.teams.allied(o, ea.owner))
        .collect();
    // Neutral camps are hit when explicitly targeted or when the shooter is at war with one;
    // ordinary scenery never is.
    if eb.owner == Owner::Nobody
        || state
            .entities
            .iter()
            .any(|e| e.owner == Owner::Nobody && senses.hostile(ea, e))
    {
        victims.push(Owner::Nobody);
    }
    let forced = ea
        .unit
        .as_ref()
        .is_some_and(|u| matches!(u.order, Some(Order::Attack { force: true, .. })));
    if forced && !victims.contains(&eb.owner) {
        victims.push(eb.owner);
    }
    let origin = senses.eye(state, ea);
    let target = Aim {
        position: eb.position,
        surface: eb.surface.clone(),
        elevation_cm: 0,
    };
    let shell = Shell {
        id: state.counters.shell,
        source: a,
        definition: senses.weapon_def(ea),
        owner: ea.owner,
        origin: Aim::of(origin, ea.surface.clone()),
        target,
        launched: state.tick,
        impact: state.tick + flight,
        damage_milli,
        damage_type: weapon.damage_type.0,
        radius_milli,
        slow_permille,
        slow_ticks,
        victims,
        viewers: viewers(senses, state, ea, eb),
        resolved: false,
    };
    state.counters.shell += 1;
    state.combat.shells.push(shell);
}

/// Forgets landed shells and ended slows (old `ShellCombat.expire`, at planning).
pub fn expire(state: &mut State) {
    let tick = state.tick;
    for e in state.entities.iter_mut() {
        if let Some(u) = e.unit.as_mut()
            && !u.combat.slows.is_empty()
        {
            u.combat.slows.retain(|s| s.expires > tick);
        }
    }
    state
        .combat
        .shells
        .retain(|s| !s.resolved || tick < s.impact + SHELL_LINGER);
}

/// Lands due shells (old `ShellCombat.resolve`): splash hits and slows.
pub fn land_shells(senses: &Senses, state: &mut State, hits: &mut Vec<Hit>) {
    let tick = state.tick;
    let layered = senses.terrain.surfaces().is_some();
    for index in 0..state.combat.shells.len() {
        let shell = &state.combat.shells[index];
        if shell.resolved || shell.impact > tick {
            continue;
        }
        let shell = shell.clone();
        state.combat.shells[index].resolved = true;
        let r2 = shell.radius_milli * shell.radius_milli;
        let center = shell.target.point(senses);
        let mut slowed = Vec::new();
        for e in state.entities.iter() {
            let exposed = e.unit.as_ref().is_none_or(|u| {
                u.contained.is_none() && u.release.is_none() && u.garrison.is_none()
            });
            if !e.hp.is_some_and(|hp| hp > 0)
                || !senses
                    .combat
                    .weapon(shell.definition)
                    .is_some_and(|w| w.reaches(senses.air(e)))
                || (shell.damage_type != senses.combat.spell.0
                    && crate::abilities::statuses::ethereal(senses.catalog, e))
                || !exposed
                || !shell.victims.contains(&e.owner)
            {
                continue;
            }
            // Structures count from their footprint, units from their exact position.
            let (hx, hy) = super::senses::half_extents(senses.catalog, e);
            let gap = |d: i64, half: f64| (d.abs() - (half * 1000.0) as i64).max(0);
            let dx = gap(i64::from(e.position.x - shell.target.position.x), hx);
            let dy = gap(i64::from(e.position.y - shell.target.position.y), hy);
            if dx * dx + dy * dy > r2 {
                continue;
            }
            if layered {
                let to = SurfacePoint {
                    x: f64::from(e.position.x) / 1000.0,
                    y: f64::from(e.position.y) / 1000.0,
                    deck: e.surface.as_deref().and_then(|s| senses.terrain.deck_id(s)),
                    elevation: 0.0,
                };
                if !senses.terrain.shot_clear(center, to) {
                    continue;
                }
            }
            hits.push(Hit {
                source: shell.source,
                owner: shell.owner,
                target: e.id,
                damage_milli: shell.damage_milli,
                damage_type: DamageType(shell.damage_type),
                ..Hit::default()
            });
            let catalog = senses.catalog;
            if e.unit.is_some()
                && shell.slow_permille > 0
                && !crate::abilities::statuses::control_immune(
                    catalog,
                    e,
                    crate::abilities::library::control::MOVE_SLOW,
                )
                && !crate::abilities::statuses::invulnerable(catalog, e)
            {
                slowed.push(e.id);
            }
        }
        for id in slowed {
            let u = state
                .entities
                .get_mut(id)
                .expect("victim")
                .unit
                .as_mut()
                .expect("unit");
            let expires = tick + shell.slow_ticks;
            match u
                .combat
                .slows
                .iter_mut()
                .find(|s| s.permille == shell.slow_permille)
            {
                Some(s) => s.expires = s.expires.max(expires),
                None => u.combat.slows.push(Slow {
                    permille: shell.slow_permille,
                    expires,
                }),
            }
        }
    }
}

/// Follows targets and lands due missiles (old `Missiles.resolve`).
pub fn land_missiles(senses: &Senses, state: &mut State, hits: &mut Vec<Hit>) {
    let tick = state.tick;
    state
        .combat
        .missiles
        .retain(|m| !m.resolved || tick < m.impact + MISSILE_LINGER);
    for index in 0..state.combat.missiles.len() {
        if state.combat.missiles[index].resolved {
            continue;
        }
        let target = state.combat.missiles[index].target;
        let living = state
            .entities
            .get(target)
            .filter(|t| t.hp.is_none_or(|hp| hp > 0));
        if let Some(t) = living {
            let destination = Aim::of(senses.eye(state, t), t.surface.clone());
            state.combat.missiles[index].destination = destination;
        }
        let m = &state.combat.missiles[index];
        if tick < m.impact {
            continue;
        }
        let hittable = living.is_some_and(|t| {
            t.hp.is_some()
                && t.unit.as_ref().is_none_or(|u| {
                    u.contained.is_none() && u.release.is_none() && u.garrison.is_none()
                })
        });
        let clear = hittable
            && senses
                .terrain
                .shot_clear(m.origin.point(senses), m.destination.point(senses));
        let hit = clear.then(|| Hit {
            source: m.source,
            owner: m.owner,
            target,
            damage_milli: m.damage_milli,
            damage_type: DamageType(m.damage_type),
            weapon: true,
            critical: m.critical,
            cleave: None,
            enhancement: m.enhancement.clone(),
        });
        state.combat.missiles[index].resolved = true;
        hits.extend(hit);
    }
}
