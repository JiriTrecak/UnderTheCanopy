//! Posts: what the map placed for a computer player. Passive defends them where they stand;
//! Base and Waves also train lost defenders again, revive heroes and rebuild buildings, all
//! free.

use super::{
    ALERT, HOME_SLACK, LEASH, Memory, Play, Post, Purpose, REBUILD_DELAY_SECONDS, RETRY_TICKS,
    Training, distance2, living, within,
};
use crate::{
    catalog::Def,
    economy::{revival, sites},
    entity::{EntityId, Milli},
    orders::Order,
    terrain::footprint::{Extent, body_overlaps_bounds, footprint_bounds, footprint_cells},
};
use canopy_content::game::{Creation, Kind, ProductionMode};

impl Play<'_, '_> {
    /// Adopts placed units and buildings of ours not yet posted: at activation, and as the
    /// script spawns or hands over more.
    pub(super) fn scan(&mut self, m: &mut Memory) {
        let catalog = self.ctx.catalog;
        let found: Vec<Post> = self
            .ctx
            .state
            .entities
            .iter()
            .filter(|e| e.owner == self.me && living(e))
            .filter_map(|e| {
                let placement = e.placement.as_ref()?;
                if m.posts.iter().any(|p| &p.placement == placement) {
                    return None;
                }
                let d = catalog.get(e.definition);
                if !matches!(d.kind, Kind::Unit | Kind::Building) {
                    return None;
                }
                Some(Post {
                    placement: placement.clone(),
                    definition: d.id.clone(),
                    building: d.kind == Kind::Building,
                    home: e.position,
                    rotation: e.rotation,
                    surface: e.surface.clone(),
                    entity: Some(e.id),
                    lost: false,
                    training: false,
                    due: None,
                    altar: None,
                    sent: None,
                })
            })
            .collect();
        m.posts.extend(found);
    }

    /// Notices lost posts: given away for good, or fallen and waiting for a replacement,
    /// a revival or a rebuild.
    pub(super) fn check_posts(&mut self, m: &mut Memory, keeps: bool) {
        let tick = self.tick;
        let rebuild = keeps && self.plan.ai.rebuilds();
        for post in m.posts.iter_mut().filter(|p| !p.lost) {
            let Some(id) = post.entity else {
                if post.building && rebuild && post.due.is_none() {
                    post.due = Some(tick + super::seconds(f64::from(REBUILD_DELAY_SECONDS)));
                }
                continue;
            };
            let Some(e) = self.ctx.state.entities.get(id) else {
                post.entity = None;
                if post.building && rebuild {
                    post.due = Some(tick + super::seconds(f64::from(REBUILD_DELAY_SECONDS)));
                }
                continue;
            };
            if e.owner != self.me {
                post.lost = true;
                post.entity = None;
                continue;
            }
            // A fallen hero keeps its entity; it waits for its revival.
            if crate::heroes::fallen(e) {
                if keeps && post.altar.is_none() {
                    self.find_altar(post, id);
                }
                continue;
            }
            if e.hp == Some(0) {
                post.entity = None;
            }
        }
    }

    /// Picks the altar a fallen hero revives at and when.
    fn find_altar(&self, post: &mut Post, hero: EntityId) {
        let catalog = self.ctx.catalog;
        let state = &*self.ctx.state;
        let Some(h) = state.entities.get(hero) else {
            return;
        };
        let level =
            crate::stats::Stats::of(catalog, h, crate::stats::research(state, self.me)).level;
        let altar = self
            .ctx
            .index
            .buildings
            .iter()
            .filter_map(|&b| state.entities.get(b))
            .filter(|b| b.owner == self.me && living(b) && b.construction.is_none())
            .find_map(|b| {
                let policy = catalog.get(b.definition).behaviors.revival.as_ref()?;
                Some((b.id, revival::terms(policy, level).1))
            });
        if let Some((altar, ticks)) = altar {
            post.altar = Some(altar);
            post.due = Some(self.tick + ticks.max(1));
        }
    }

    /// Fallen heroes come back at their altar when their time is up.
    pub(super) fn revive(&mut self, m: &mut Memory) {
        let tick = self.tick;
        for post in m.posts.iter_mut().filter(|p| p.altar.is_some()) {
            if post.due.is_some_and(|d| d > tick) {
                continue;
            }
            let (Some(hero), Some(altar)) = (post.entity, post.altar) else {
                continue;
            };
            let fallen = self
                .ctx
                .state
                .entities
                .get(hero)
                .is_some_and(crate::heroes::fallen);
            let standing = self
                .ctx
                .state
                .entities
                .get(altar)
                .filter(|b| b.owner == self.me && living(b));
            let Some(door) = standing.map(|b| sites::entrance_of(self.ctx.catalog, b)) else {
                // The altar fell: look for another at the next decision.
                post.altar = None;
                post.due = None;
                continue;
            };
            if !fallen {
                post.altar = None;
                post.due = None;
                continue;
            }
            let def = self.ctx.state.entities.get(hero).expect("hero").definition;
            let Some(spot) = self.spot_near(def, door, &[]) else {
                post.due = Some(tick + RETRY_TICKS);
                continue;
            };
            revival::restore(self.ctx, hero, spot);
            post.altar = None;
            post.due = None;
            post.sent = self.send(hero, post.home, true);
        }
    }

    /// Lost defenders are trained again, free, at a building of ours that trains them.
    pub(super) fn replace(&mut self, m: &mut Memory) {
        for index in 0..m.posts.len() {
            let post = &m.posts[index];
            if post.building || post.lost || post.training || post.entity.is_some() {
                continue;
            }
            let Some(def) = self.find(post.definition.as_str()) else {
                continue;
            };
            if let Some(training) = self.train(m, def, Purpose::Post(index as u32)) {
                m.training.push(training);
                m.posts[index].training = true;
            }
        }
    }

    /// Free training of `def` at whichever of our buildings that train it is free first;
    /// `None` when none can.
    pub(super) fn train(&self, m: &Memory, def: Def, purpose: Purpose) -> Option<Training> {
        let catalog = self.ctx.catalog;
        let d = catalog.get(def);
        let Some(Creation::Train { work_ticks, .. }) = &d.creation else {
            return None;
        };
        let state = &*self.ctx.state;
        let (building, start) = self
            .ctx
            .index
            .buildings
            .iter()
            .filter_map(|&b| state.entities.get(b))
            .filter(|b| {
                b.owner == self.me
                    && living(b)
                    && b.construction.is_none()
                    && b.ready_tick <= self.tick
                    && catalog
                        .get(b.definition)
                        .behaviors
                        .production
                        .as_ref()
                        .is_some_and(|p| {
                            p.mode == ProductionMode::Queued && p.outputs.contains(&d.id)
                        })
            })
            .map(|b| {
                let busy = m
                    .training
                    .iter()
                    .filter(|t| t.building == b.id)
                    .map(|t| t.done)
                    .max()
                    .unwrap_or(0);
                (b.id, busy.max(self.tick))
            })
            .min_by_key(|&(id, start)| (start, id))?;
        Some(Training {
            building,
            unit: d.id.clone(),
            done: start + (*work_ticks).max(1),
            purpose,
        })
    }

    /// Finished training leaves its building; training at a building that fell is lost.
    pub(super) fn advance_training(&mut self, m: &mut Memory) {
        let tick = self.tick;
        let mut index = 0;
        while index < m.training.len() {
            let t = m.training[index].clone();
            if t.done > tick {
                index += 1;
                continue;
            }
            let standing = self
                .ctx
                .state
                .entities
                .get(t.building)
                .filter(|b| b.owner == self.me && living(b));
            let def = self.find(t.unit.as_str());
            let (Some(building), Some(def)) = (standing, def) else {
                m.training.remove(index);
                self.trained(m, t.purpose, None);
                continue;
            };
            let toward = match t.purpose {
                Purpose::Post(p) => m.posts[p as usize].home,
                Purpose::Wave(_) => sites::entrance_of(self.ctx.catalog, building),
            };
            let Some(spot) = sites::deployment(self.ctx, t.building, toward, def, 8) else {
                m.training[index].done = tick + RETRY_TICKS / 4;
                index += 1;
                continue;
            };
            m.training.remove(index);
            let unit = self.spawn_unit(def, spot);
            self.trained(m, t.purpose, Some(unit));
        }
    }

    fn trained(&mut self, m: &mut Memory, purpose: Purpose, unit: Option<EntityId>) {
        match purpose {
            Purpose::Post(p) => {
                let post = &mut m.posts[p as usize];
                post.training = false;
                post.entity = unit;
                if let Some(unit) = unit {
                    let home = post.home;
                    post.sent = self.send(unit, home, true);
                }
            }
            Purpose::Wave(id) => {
                if let Some(wave) = m.waves.iter_mut().find(|w| w.id == id) {
                    wave.pending = wave.pending.saturating_sub(1);
                    wave.units.extend(unit);
                }
            }
        }
    }

    /// Destroyed buildings rise again where they stood, free, once nothing stands in the
    /// way; their sites raise themselves over the building's construction time.
    pub(super) fn rebuild(&mut self, m: &mut Memory) {
        let tick = self.tick;
        for post in m.posts.iter_mut().filter(|p| p.building && !p.lost) {
            match post.entity {
                Some(id) => self.raise(id),
                None if post.due.is_some_and(|d| d <= tick) => {
                    let Some(def) = self.find(post.definition.as_str()) else {
                        post.due = None;
                        continue;
                    };
                    if !self.site_clear(def, post.home, post.rotation) {
                        post.due = Some(tick + RETRY_TICKS);
                        continue;
                    }
                    let id = crate::spawn::insert(
                        self.ctx.state,
                        self.ctx.catalog,
                        crate::spawn::Spawn {
                            definition: def,
                            owner: self.me,
                            position: post.home,
                            surface: post.surface.clone(),
                            rotation: post.rotation,
                            placement: None,
                            initial: Default::default(),
                            complete: false,
                            ready_tick: tick,
                        },
                    );
                    self.ctx.index.sync(self.ctx.state, self.ctx.catalog);
                    self.ctx.spatial.sync_entity(self.ctx.state, id);
                    post.entity = Some(id);
                    post.due = None;
                }
                None => {}
            }
        }
    }

    /// One tick of a rebuilt site raising itself (as builders would, without them).
    fn raise(&mut self, id: EntityId) {
        let tick = self.tick;
        let catalog = self.ctx.catalog;
        let Some(b) = self.ctx.state.entities.get_mut(id) else {
            return;
        };
        if b.construction.is_none() || !living(b) {
            return;
        }
        let def = catalog.get(b.definition);
        let work_ticks = match &def.creation {
            Some(Creation::Construct { work_ticks, .. }) => (*work_ticks).max(1),
            _ => 1,
        };
        let max_hp = def.body.as_ref().map_or(1, |b| b.max_hp);
        let permille = catalog.content.rules.construction_hp_permille;
        let start = (u64::from(max_hp) * u64::from(permille) / 1000).max(1) as u32;
        let c = b.construction.as_mut().expect("site");
        c.progress += 1;
        let supported = max_hp.min(
            start
                + (u64::from(max_hp - start.min(max_hp)) * u64::from(c.progress)
                    / u64::from(work_ticks)) as u32,
        );
        let gained = supported.saturating_sub(c.supported_hp);
        c.supported_hp = supported;
        let done = c.progress >= work_ticks;
        if let Some(hp) = b.hp.as_mut() {
            *hp += gained;
        }
        if done {
            b.construction = None;
            b.ready_tick = tick + 1;
            self.ctx.spatial.sync_entity(self.ctx.state, id);
        }
    }

    /// Whether a building of `def` may stand at `at` again: no other building and no
    /// ground unit on its footprint.
    fn site_clear(&self, def: Def, at: Milli, rotation: i32) -> bool {
        let d = self.ctx.catalog.get(def);
        let (x, y) = (
            f64::from(at.x) / f64::from(Milli::CELL),
            f64::from(at.y) / f64::from(Milli::CELL),
        );
        let degrees = f64::from(rotation) / 1000.0;
        let extent = Extent::of(d.footprint.as_ref());
        let size = i64::from(self.ctx.spatial.size());
        let cells_free = footprint_cells(x, y, extent, degrees)
            .cells()
            .all(|(cx, cy)| {
                cx >= 0
                    && cy >= 0
                    && cx < size
                    && cy < size
                    && !self.ctx.spatial.building_at((cy * size + cx) as u32)
            });
        let edges = footprint_bounds(x, y, extent, degrees);
        let state = &*self.ctx.state;
        let crowded = self.ctx.index.units.iter().any(|&id| {
            state.entities.get(id).is_some_and(|e| {
                let body = self.ctx.spatial.profiles.get(e.definition).body;
                living(e)
                    && e.unit.as_ref().is_some_and(|u| u.contained.is_none())
                    && !body.air
                    && body_overlaps_bounds(
                        f64::from(e.position.x) / 1000.0,
                        f64::from(e.position.y) / 1000.0,
                        body.radius,
                        edges,
                    )
            })
        });
        cells_free && !crowded
    }

    /// Defenders hold their ground: they walk back home when idle and away, break off when
    /// drawn too far, and join a neighbour in a fight. Units the script ordered elsewhere
    /// take that place as their home; units on other script orders are left alone.
    pub(super) fn defend(&mut self, m: &mut Memory) {
        let marching: Vec<EntityId> = m
            .waves
            .iter()
            .flat_map(|w| w.units.iter().chain(&w.heroes))
            .copied()
            .collect();
        // (post, unit, position, engaged target position).
        let mut guards: Vec<(usize, EntityId, Milli, Option<Milli>)> = Vec::new();
        for (index, post) in m.posts.iter_mut().enumerate() {
            let Some(id) = post.entity.filter(|_| !post.building) else {
                continue;
            };
            if marching.contains(&id) {
                continue;
            }
            let Some(e) = self.own_unit(id) else {
                continue;
            };
            let u = e.unit.as_ref().expect("unit");
            if u.contained.is_some() || e.ready_tick > self.tick {
                continue;
            }
            match &u.order {
                None => {}
                Some(Order::Move { destination, .. })
                    if post.sent == Some((destination.x, destination.y)) => {}
                Some(Order::Move { destination, .. }) => {
                    // The script's move: that is where it stands guard now.
                    post.home = super::milli_cell(destination.x, destination.y);
                    post.sent = Some((destination.x, destination.y));
                    continue;
                }
                Some(_) => continue,
            }
            let engaged = u
                .target
                .and_then(|t| self.ctx.state.entities.get(t))
                .filter(|t| living(t) && self.enemy(t.owner))
                .map(|t| t.position);
            guards.push((index, id, e.position, engaged));
        }
        for &(index, id, at, engaged) in &guards {
            let home = m.posts[index].home;
            let ordered = self
                .ctx
                .state
                .entities
                .get(id)
                .and_then(|e| e.unit.as_ref())
                .is_some_and(|u| u.order.is_some());
            if !within(at, home, LEASH) {
                if engaged.is_some() || !ordered {
                    m.posts[index].sent = self.send(id, home, false);
                }
                continue;
            }
            if engaged.is_some() || ordered {
                continue;
            }
            // Idle: help the nearest fighting neighbour, else walk back home.
            let call = guards
                .iter()
                .filter_map(|&(_, _, p, target)| Some((p, target?)))
                .filter(|&(p, _)| within(p, at, ALERT))
                .min_by_key(|&(p, t)| (distance2(p, at), t.x, t.y))
                .map(|(_, t)| t);
            if let Some(target) = call {
                m.posts[index].sent = self.send(id, target, true);
            } else if !within(at, home, HOME_SLACK) {
                m.posts[index].sent = self.send(id, home, true);
            }
        }
    }
}
