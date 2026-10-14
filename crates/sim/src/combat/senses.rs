//! What a unit perceives and whom it fights (old `Combat.perceives`, `hostile`,
//! `opponents`, `allied`, `weaponEligible`, `terrainClear` and `Spatial.attackClear`).

use super::rules::Rules;
use crate::{
    camps::Camps,
    catalog::Catalog,
    entity::{Entity, EntityId},
    state::State,
    teams::{Relation, Teams},
    terrain::{SurfacePoint, Terrain},
    vision::{Sight, SightRules, Vision},
};
use canopy_content::placement::Owner;
use std::cell::{Cell, RefCell};

/// Remembers `visible(owner, target)` answers for one planning pass, where positions and
/// sight do not change (old `planningSight`).
#[derive(Debug, Default)]
pub struct SightCache {
    pass: Cell<u32>,
    /// Per owner slot (players 0..8), per entity id: pass number × 2 + answer.
    stamps: RefCell<Vec<u32>>,
    width: Cell<usize>,
}

impl SightCache {
    /// Starts a pass over entity ids below `ids`.
    pub fn begin(&self, ids: usize) {
        let mut stamps = self.stamps.borrow_mut();
        if self.width.get() < ids || self.pass.get() >= u32::MAX / 2 - 1 {
            let width = ids.next_power_of_two().max(64);
            *stamps = vec![0; width * 8];
            self.width.set(width);
            self.pass.set(0);
        }
        self.pass.set(self.pass.get() + 1);
    }

    fn get(&self, owner: u8, id: EntityId) -> Option<bool> {
        let index = usize::from(owner) * self.width.get() + id.0 as usize;
        let value = *self.stamps.borrow().get(index)?;
        (value >> 1 == self.pass.get()).then_some(value & 1 == 1)
    }

    fn set(&self, owner: u8, id: EntityId, visible: bool) {
        let index = usize::from(owner) * self.width.get() + id.0 as usize;
        if let Some(slot) = self.stamps.borrow_mut().get_mut(index) {
            *slot = self.pass.get() << 1 | u32::from(visible);
        }
    }
}

/// Everything perception and allegiance questions need, borrowed for a pass.
#[derive(Clone, Copy)]
pub struct Senses<'a> {
    pub catalog: &'a Catalog,
    pub terrain: &'a Terrain,
    pub vision: &'a Vision,
    pub rules: &'a dyn SightRules,
    pub combat: &'a Rules,
    pub camps: &'a Camps,
    pub teams: &'a Teams,
    /// Answers cached for a planning pass; `None` outside one.
    pub cache: Option<&'a SightCache>,
}

fn alive(e: &Entity) -> bool {
    e.hp.is_none_or(|hp| hp > 0)
}

/// Neither held inside something nor waiting to be placed, nor in a lookout.
fn exposed(e: &Entity) -> bool {
    e.unit
        .as_ref()
        .is_none_or(|u| u.contained.is_none() && u.release.is_none() && u.garrison.is_none())
}

fn camp(e: &Entity) -> Option<&str> {
    e.unit.as_ref()?.camp.as_ref().map(|c| c.id.as_str())
}

impl<'a> Senses<'a> {
    pub fn sight<'s>(&'s self, state: &'s State) -> Sight<'s> {
        Sight {
            vision: self.vision,
            state,
            catalog: self.catalog,
            terrain: self.terrain,
            rules: self.rules,
        }
    }

    /// Allegiance only (old `opponents`): players on different teams; a player and a
    /// neutral camp whose aggression is `players`.
    pub fn opponents(
        &self,
        a: Owner,
        a_camp: Option<&str>,
        b: Owner,
        b_camp: Option<&str>,
    ) -> bool {
        match self.teams.relation(a, b) {
            Relation::Enemy => true,
            Relation::Own | Relation::Ally => false,
            Relation::Neutral => {
                if a == Owner::Nobody && b == Owner::Nobody {
                    return false;
                }
                // An owner outside the match counts like a neutral side.
                let neutral = if a == Owner::Nobody { a_camp } else { b_camp };
                if a != Owner::Nobody && b != Owner::Nobody {
                    return false;
                }
                self.camps.hostile(neutral)
            }
        }
    }

    /// The same team, or the same owner (old `allied`).
    pub fn allied(&self, a: Owner, b: Owner) -> bool {
        a != Owner::Nobody && b != Owner::Nobody && self.teams.allied(a, b)
    }

    /// Whether `a` would fight `b` unprompted (old `hostile`): a living, exposed opponent
    /// with a body.
    pub fn hostile(&self, a: &Entity, b: &Entity) -> bool {
        a.id != b.id
            && b.hp.is_some()
            && alive(b)
            && exposed(b)
            && self.opponents(a.owner, camp(a), b.owner, camp(b))
    }

    /// Whether `a`'s weapon can hit `b` (old `weaponEligible`; M2.6 adds ethereal forms
    /// and damage immunities).
    pub fn weapon_eligible(&self, a: &Entity, b: &Entity) -> bool {
        self.combat.reaches(a.definition, b.definition)
    }

    /// Whether `a` perceives `b` (old `perceives`): detection, then the owner's exact
    /// sight; a neutral unit sees by itself, within its camp's aggro range over the
    /// terrain.
    pub fn perceives(&self, state: &State, a: &Entity, b: &Entity) -> bool {
        let sight = self.sight(state);
        if !sight.detects(a.owner, b, Some(a)) {
            return false;
        }
        let Owner::Player(slot) = a.owner else {
            let radius = self
                .camps
                .of(a)
                .map(|c| c.aggro_range)
                .or_else(|| self.combat.weapon(a.definition).map(|w| w.aggro))
                .or(self.catalog.get(a.definition).vision)
                .unwrap_or(0);
            return crate::camps::neutral_sees(&sight, a, b, radius);
        };
        if let Some(cache) = self.cache
            && let Some(known) = cache.get(slot, b.id)
        {
            return known;
        }
        let visible = sight.visible(a.owner, b);
        if let Some(cache) = self.cache {
            cache.set(slot, b.id, visible);
        }
        visible
    }

    /// Where `e` stands as a firing or sight origin: its exact position raised by flight
    /// and a lookout (old `elevatedPoint`).
    pub fn eye(&self, state: &State, e: &Entity) -> SurfacePoint {
        self.sight(state).sight_point(e)
    }

    /// The firing origin `e` would have at another cell (flight height only).
    pub fn eye_at(
        &self,
        e: &Entity,
        x: f64,
        y: f64,
        deck: Option<crate::terrain::DeckId>,
    ) -> SurfacePoint {
        let mut elevation = 0;
        if let Some(flight) = self.rules.flight_cm(self.catalog, e) {
            let at = crate::entity::Milli {
                x: crate::terrain::round_half_up(x * 1000.0) as i32,
                y: crate::terrain::round_half_up(y * 1000.0) as i32,
            };
            elevation = flight + crate::vision::air_gap_cm(self.terrain, at, deck);
        }
        SurfacePoint {
            x,
            y,
            deck,
            elevation: f64::from(elevation) / 100.0,
        }
    }

    /// Whether a weapon at `origin` reaches `target` over the terrain (old `attackClear`):
    /// aimed at the nearest point of its footprint, at flight height for aircraft.
    pub fn attack_clear(
        &self,
        state: &State,
        origin: SurfacePoint,
        target: &Entity,
        ranged: bool,
    ) -> bool {
        let (hx, hy) = half_extents(self.catalog, target);
        let (cx, cy) = (
            f64::from(target.position.x) / 1000.0,
            f64::from(target.position.y) / 1000.0,
        );
        let mut end = SurfacePoint {
            x: origin.x.clamp(cx - hx, cx + hx),
            y: origin.y.clamp(cy - hy, cy + hy),
            deck: target
                .surface
                .as_deref()
                .and_then(|s| self.terrain.deck_id(s)),
            elevation: 0.0,
        };
        if self.combat.profile(target.definition).air {
            end.elevation = self.eye(state, target).elevation;
        }
        if ranged {
            self.terrain.shot_clear(origin, end)
        } else {
            self.terrain.melee_clear(origin, end)
        }
    }

    /// Whether `a` can hit `b` from where it stands (old `terrainClear`).
    pub fn terrain_clear(&self, state: &State, a: &Entity, b: &Entity) -> bool {
        let ranged = self.combat.weapon(a.definition).is_some_and(|w| w.ranged());
        self.attack_clear(state, self.eye(state, a), b, ranged)
    }
}

/// Half a footprint in cells, turned with the entity (old footprint box); zero for units.
pub fn half_extents(catalog: &Catalog, e: &Entity) -> (f64, f64) {
    match catalog.get(e.definition).footprint.as_ref() {
        None => (0.0, 0.0),
        Some(f) => {
            let quarter = crate::terrain::round_half_up(f64::from(e.rotation) / 90_000.0) as i64;
            let (w, d) = (f64::from(f.width), f64::from(f.depth));
            if quarter.rem_euclid(2) != 0 {
                (d / 2.0, w / 2.0)
            } else {
                (w / 2.0, d / 2.0)
            }
        }
    }
}
