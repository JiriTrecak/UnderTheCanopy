//! Sight questions other systems ask (old `Observation.visible`, `previouslyVisible`,
//! `detects`, `explored`, `currentlyVisible`).

use super::{
    Vision,
    knowledge::{FogCells, ObservedDeath, Sighting, TeamKnowledge, UNEXPLORED, VISIBLE},
    rules::SightRules,
    sensor::{Sensor, SensorKey},
    statics,
};
use crate::{
    catalog::Catalog,
    entity::{Entity, Milli},
    state::State,
    terrain::{SurfacePoint, Terrain, footprint::Extent},
};
use canopy_content::placement::Owner;

/// Everything a sight question needs, borrowed for one query or a batch of them.
#[derive(Clone, Copy)]
pub struct Sight<'a> {
    pub vision: &'a Vision,
    pub state: &'a State,
    pub catalog: &'a Catalog,
    pub terrain: &'a Terrain,
    pub rules: &'a dyn SightRules,
}

impl<'a> Sight<'a> {
    /// The fog and memory of `owner`'s team; `None` for owners outside the match.
    pub fn knowledge(&self, owner: Owner) -> Option<&'a TeamKnowledge> {
        let team = self.vision.teams().index(owner)?;
        self.state.knowledge.teams.get(team)
    }

    pub fn cells(&self, owner: Owner) -> Option<&'a FogCells> {
        self.knowledge(owner).map(|k| &k.cells)
    }

    /// Whether every node has been seen at some time.
    pub fn explored(&self, owner: Owner, nodes: &[u32]) -> bool {
        self.cells(owner)
            .is_some_and(|c| nodes.iter().all(|&n| c.get(n) != UNEXPLORED))
    }

    /// Whether every node is in sight now.
    pub fn in_sight(&self, owner: Owner, nodes: &[u32]) -> bool {
        self.cells(owner)
            .is_some_and(|c| nodes.iter().all(|&n| c.get(n) == VISIBLE))
    }

    /// Deaths `owner` witnessed recently, oldest first.
    pub fn observed_deaths(&self, owner: Owner) -> &'a [ObservedDeath] {
        self.state
            .knowledge
            .owner(owner)
            .map_or(&[], |o| o.deaths.as_slice())
    }

    /// The stationary entity as `owner` remembers it.
    pub fn remembered(&self, owner: Owner, id: crate::entity::EntityId) -> Option<&'a Sighting> {
        self.knowledge(owner)?.remembered(id)
    }

    /// What the owner knows now, from its fog (old `previouslyVisible`): its own entities,
    /// and others with part of their footprint in sight that it also detects. Views and
    /// command checks use this.
    pub fn seen(&self, owner: Owner, entity: &Entity) -> bool {
        if entity.owner == owner {
            return true;
        }
        let Some(cells) = self.cells(owner) else {
            return false;
        };
        let mut in_sight = false;
        statics::Place::of(entity).nodes(self.terrain, self.catalog, |n| {
            in_sight |= cells.get(n) == VISIBLE;
        });
        in_sight && self.detects(owner, entity, None)
    }

    /// Exact sight for gameplay (old `visible`): targeting, projectile witnesses. Sensors
    /// must be current ([`Vision::sync`] after movement).
    pub fn visible(&self, owner: Owner, entity: &Entity) -> bool {
        if !self.detects(owner, entity, None) {
            return false;
        }
        if entity.owner == owner || self.spell_visible(owner, entity) {
            return true;
        }
        let airborne = self.rules.flight_cm(self.catalog, entity).is_some();
        let target = self.sight_point(entity);
        let layered = self.terrain.surfaces().is_some();
        let anchor = statics::anchor(entity);
        let mut cells = Vec::new();
        if !(airborne || layered) {
            statics::Place::of(entity).nodes(self.terrain, self.catalog, |n| cells.push(n));
        }
        let (hx, hy) = half_extents(self.catalog, entity);
        let query = self.vision.index.sectors(
            Milli {
                x: anchor.x - hx - 1000,
                y: anchor.y - hy - 1000,
            },
            Milli {
                x: anchor.x + hx + 1000,
                y: anchor.y + hy + 1000,
            },
        );
        let size = i64::from(self.terrain.size());
        // Sensors that do not share their sight with `owner` cannot answer; with a team,
        // that is exactly those without its bit (a sensor's teams include its owner's).
        let shared = self.vision.teams().index(owner).map(|t| 1u16 << t);
        self.vision.index.any(query, shared, |key| {
            let (Some(sensor), SensorKey::Entity(id)) = (self.vision.sensor(key), key) else {
                return false;
            };
            let Some(eye_entity) = self.state.entities.get(id) else {
                return false;
            };
            if !self.shares(owner, sensor) {
                return false;
            }
            let eye = self.eye(eye_entity);
            let radius = i64::from(sensor.spec.radius);
            if airborne || layered {
                point_range(eye_entity.position, entity.position, hx, hy) <= radius * radius
                    && self.terrain_visible(eye, target, airborne)
            } else {
                let (sx, sy) = (
                    i64::from(sensor.spec.origin.x),
                    i64::from(sensor.spec.origin.y),
                );
                cells.iter().any(|&cell| {
                    let (cx, cy) = (i64::from(cell) % size * 1000, i64::from(cell) / size * 1000);
                    (cx - sx).pow(2) + (cy - sy).pow(2) <= radius * radius
                        && self.terrain.tactical().visible(
                            ground(eye),
                            crate::terrain::TerrainPoint::new(
                                (cx / 1000) as f64,
                                (cy / 1000) as f64,
                            ),
                        )
                })
            }
        })
    }

    /// Whether `owner` may perceive `entity` at all: concealed units need a detector in
    /// range (and in sight) as well as ordinary sight. `observer` is the asking unit, for
    /// owners without a team (neutral camps).
    pub fn detects(&self, owner: Owner, entity: &Entity, observer: Option<&Entity>) -> bool {
        let tick = self.state.tick;
        if !self.rules.hidden(entity, tick)
            || (owner != Owner::Nobody && self.vision.teams().allied(owner, entity.owner))
            || observer.is_some_and(|o| o.id == entity.id)
        {
            return true;
        }
        let target = self.sight_point(entity);
        let airborne = self.rules.flight_cm(self.catalog, entity).is_some();
        let near = |eye: Milli, radius: i64| {
            let (dx, dy) = (
                i64::from(entity.position.x - eye.x),
                i64::from(entity.position.y - eye.y),
            );
            dx * dx + dy * dy <= radius * radius
        };
        let detector = |s: &Entity| {
            let r = i64::from(self.rules.detection(s, tick)) * 1000;
            r > 0
                && !self.rules.blind(s)
                && statics::alive(s)
                && near(s.position, r)
                && self.terrain_visible(self.eye(s), target, airborne)
        };
        if owner == Owner::Nobody {
            return observer.is_some_and(detector);
        }
        let teams = self.vision.teams();
        for sensor in &self.vision.sensors {
            if let SensorKey::Entity(id) = sensor.key
                && teams.allied(owner, sensor.spec.owner)
                && let Some(s) = self.state.entities.get(id)
                && detector(s)
            {
                return true;
            }
        }
        self.state.knowledge.spells.iter().any(|v| {
            v.detect_invisible
                && v.expires > tick
                && teams.allied(owner, v.owner)
                && near(v.point, i64::from(v.radius))
                && (v.ignore_terrain || self.terrain_visible(self.spell_eye(v), target, airborne))
        })
    }

    /// Inside a temporary sight source shared with `owner`.
    fn spell_visible(&self, owner: Owner, entity: &Entity) -> bool {
        let tick = self.state.tick;
        let teams = self.vision.teams();
        let airborne = self.rules.flight_cm(self.catalog, entity).is_some();
        let target = self.sight_point(entity);
        self.state.knowledge.spells.iter().any(|v| {
            let (dx, dy) = (
                i64::from(entity.position.x - v.point.x),
                i64::from(entity.position.y - v.point.y),
            );
            v.expires > tick
                && teams.allied(owner, v.owner)
                && dx * dx + dy * dy <= i64::from(v.radius).pow(2)
                && (v.ignore_terrain || self.terrain_visible(self.spell_eye(v), target, airborne))
        })
    }

    /// Ordinary sight shared through allies or lent through containment.
    fn shares(&self, owner: Owner, sensor: &Sensor) -> bool {
        let teams = self.vision.teams();
        teams.allied(owner, sensor.spec.owner)
            || teams
                .index(owner)
                .is_some_and(|t| sensor.spec.teams & (1 << t) != 0)
    }

    fn terrain_visible(&self, eye: SurfacePoint, target: SurfacePoint, airborne: bool) -> bool {
        // Aircraft above an open field are seen unless solid terrain or a deck is in the way.
        if airborne {
            self.terrain.shot_clear(eye, target)
        } else {
            self.terrain.visible(eye, target)
        }
    }

    /// Where an entity is seen at: its exact position, raised by flight and lookouts.
    pub fn sight_point(&self, entity: &Entity) -> SurfacePoint {
        let deck = entity
            .surface
            .as_deref()
            .and_then(|n| self.terrain.deck_id(n));
        let mut elevation = self.rules.lift_cm(entity);
        if let Some(flight) = self.rules.flight_cm(self.catalog, entity) {
            elevation += flight + super::air_gap_cm(self.terrain, entity.position, deck);
        }
        SurfacePoint {
            x: f64::from(entity.position.x) / 1000.0,
            y: f64::from(entity.position.y) / 1000.0,
            deck,
            elevation: f64::from(elevation) / 100.0,
        }
    }

    /// A sensor's eye: the same as where it is seen.
    fn eye(&self, entity: &Entity) -> SurfacePoint {
        self.sight_point(entity)
    }

    fn spell_eye(&self, v: &super::knowledge::SpellVision) -> SurfacePoint {
        SurfacePoint {
            x: f64::from(v.point.x) / 1000.0,
            y: f64::from(v.point.y) / 1000.0,
            deck: v.surface.as_deref().and_then(|n| self.terrain.deck_id(n)),
            elevation: f64::from(v.elevation) / 100.0,
        }
    }
}

fn ground(p: SurfacePoint) -> crate::terrain::TerrainPoint {
    crate::terrain::TerrainPoint {
        x: p.x,
        y: p.y,
        elevation: p.elevation,
    }
}

/// Half the footprint in milli-cells (old `pointRange` box).
fn half_extents(catalog: &Catalog, entity: &Entity) -> (i32, i32) {
    match catalog.get(entity.definition).footprint.as_ref() {
        None => (0, 0),
        Some(footprint) => {
            let e = Extent::of(Some(footprint)).rotated(f64::from(entity.rotation) / 1000.0);
            ((e.width * 500.0) as i32, (e.depth * 500.0) as i32)
        }
    }
}

/// Squared distance in milli-cells from `eye` to the box of half extents `(hx, hy)` around
/// `target` (old `Spatial.pointRange`).
fn point_range(eye: Milli, target: Milli, hx: i32, hy: i32) -> i64 {
    let dx = (i64::from(eye.x) - i64::from(target.x)).abs() - i64::from(hx);
    let dy = (i64::from(eye.y) - i64::from(target.y)).abs() - i64::from(hy);
    dx.max(0).pow(2) + dy.max(0).pow(2)
}
