//! The combat lab (docs/editor.md, "The Combat Lab"): a scenario
//! ([`canopy_content::lab::Lab`]) as a world on a flat grid of open and closed cells, its
//! orders given on their ticks, and what the movement and fighting did, measured per
//! group. The lab runs the real simulation; nothing here changes how it plays.

use crate::{
    Action, MatchSetup, Queued, Slot, SlotKind, World,
    action::{CellPoint, MAX_ACTORS},
    entity::EntityId,
    orders::Order,
    world::SetupError,
};
use canopy_content::{
    Content,
    lab::{Lab, OrderKind},
    map::{DRY_WATER_HEIGHT, MAP_HALO, SavedScene, TerrainData, UtcMap, Version, grid::Grid},
    placement::{Owner, Placement, Point},
};
use serde::Serialize;
use std::{collections::BTreeMap, sync::Arc};

/// The placement id of the scenario's `index`th unit.
pub fn placement_id(index: usize) -> String {
    format!("lab.{index}")
}

/// The index of a placement id made by [`placement_id`].
pub fn unit_index(placement: &str) -> Option<usize> {
    placement.strip_prefix("lab.")?.parse().ok()
}

fn floats(value: f32, count: usize) -> Grid {
    Grid(
        std::iter::repeat_n(value.to_le_bytes(), count)
            .flatten()
            .collect(),
    )
}

/// The scenario as a flat, dry sandbox map: its closed cells closed, its units placed.
pub fn map(lab: &Lab) -> UtcMap {
    let size = lab.size;
    let span = (size + 2 * MAP_HALO) as usize;
    let vertices = (span + 1) * (span + 1);
    let mut blocked = vec![0u8; span * span];
    for (i, closed) in lab.closed_cells().into_iter().enumerate() {
        if closed {
            let (x, y) = (i % size as usize, i / size as usize);
            blocked[(y + MAP_HALO as usize) * span + x + MAP_HALO as usize] = 1;
        }
    }
    let entities = lab
        .units
        .iter()
        .enumerate()
        .map(|(i, u)| Placement {
            activation: None,
            id: placement_id(i),
            map_knowledge: None,
            definition: u.definition.clone(),
            position: Point {
                x: u.x,
                y: u.y,
                surface: None,
            },
            rotation: u.facing,
            owner: u.owner,
            initial_state: None,
            appearance: None,
        })
        .collect();
    UtcMap {
        v: Version,
        size,
        name: lab.name.clone(),
        biome: None,
        description: None,
        music: None,
        mission: None,
        sandbox: true,
        initial_exploration: None,
        stamps: Vec::new(),
        player_starts: Vec::new(),
        entities,
        camps: Vec::new(),
        landscape: None,
        playable: None,
        authoring: SavedScene {
            terrain: TerrainData {
                version: Version,
                size,
                texture_grid: None,
                heights: floats(2.0, vertices),
                water_heights: floats(DRY_WATER_HEIGHT as f32, vertices),
                water_flow: Grid(
                    std::iter::repeat_n([128u8, 128, 0, 0], span * span)
                        .flatten()
                        .collect(),
                ),
                water_profiles: Vec::new(),
                grass: floats(1.0, vertices),
                rock: floats(0.0, vertices),
                paint: Vec::new(),
                shade: None,
                blocked: Some(Grid(blocked)),
                overlay: None,
                blocks: None,
                block_paint: None,
            },
            version: Version,
            objects: Vec::new(),
        },
    }
}

/// Every player the scenario's units belong to, as people at the keyboard (no computer
/// players: the lab gives the orders), each on a team of its own.
pub fn setup(lab: &Lab) -> MatchSetup {
    let last = lab
        .units
        .iter()
        .filter_map(|u| match u.owner {
            Owner::Player(p) => Some(p),
            Owner::Nobody => None,
        })
        .max()
        .unwrap_or(0);
    MatchSetup {
        seed: 1,
        slots: (0..=last)
            .map(|player| Slot {
                difficulty: Default::default(),
                player,
                kind: SlotKind::Human,
                name: None,
                team: None,
                hero: None,
                race: None,
            })
            .collect(),
    }
}

/// What a group did, so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupMeasure {
    pub name: String,
    pub units: u32,
    pub alive: u32,
    /// Members told to move that have stopped since.
    pub arrived: u32,
    /// Ticks from the group's last move order until its first and its last member stopped.
    pub first_arrival: Option<u32>,
    pub last_arrival: Option<u32>,
    /// Unit-ticks with a route but less than a quarter of a step made.
    pub stuck_ticks: u64,
    /// The longest a single member was held up in one go, in ticks.
    pub longest_stuck: u32,
    /// Times a moving member turned its course by more than 60° from one tick to the next.
    pub flips: u64,
    /// Health the group lost.
    pub damage_taken: u64,
    /// Ticks from the first order until a member first swung at something.
    pub first_swing: Option<u32>,
    /// The most members in reach of their target at once: how many got to fight.
    pub most_in_reach: u32,
}

/// What a run did, so far.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Measures {
    pub tick: u32,
    pub groups: Vec<GroupMeasure>,
    /// The deepest two ground bodies overlapped, in cells.
    pub worst_overlap: f64,
    /// Overlapping pairs this tick.
    pub overlapping: u32,
}

#[derive(Debug, Clone, Copy, Default)]
struct Track {
    last: Option<(i32, i32)>,
    motion: Option<(i64, i64)>,
    /// The group's move order this member has not finished yet.
    moving: bool,
    stuck: u32,
    hp: u32,
}

/// A scenario running in the real simulation.
pub struct Run {
    pub world: World,
    lab: Lab,
    /// Each unit's entity, by its index in the scenario.
    pub entities: Vec<Option<EntityId>>,
    groups: Vec<(String, Vec<usize>)>,
    tracks: Vec<Track>,
    /// The tick of each group's latest move order.
    ordered: Vec<Option<u32>>,
    /// The tick of each group's first order of any kind.
    first_order: Vec<Option<u32>>,
    measures: Measures,
    seq: u32,
}

impl Run {
    pub fn new(content: Arc<Content>, lab: &Lab) -> Result<Self, SetupError> {
        Self::tuned(content, lab, crate::movement::tuning::Tuning::default())
    }

    /// A run that plays movement by `tuning` (the lab's switches).
    pub fn tuned(
        content: Arc<Content>,
        lab: &Lab,
        tuning: crate::movement::tuning::Tuning,
    ) -> Result<Self, SetupError> {
        let mut world = World::new(content, &map(lab), setup(lab))?;
        world.set_tuning(tuning);
        let mut entities = vec![None; lab.units.len()];
        for e in world.state().entities.iter() {
            if let Some(i) = e.placement.as_deref().and_then(unit_index)
                && let Some(slot) = entities.get_mut(i)
            {
                *slot = Some(e.id);
            }
        }
        let groups: Vec<(String, Vec<usize>)> = lab
            .groups()
            .into_iter()
            .map(|g| {
                let members = (0..lab.units.len())
                    .filter(|&i| lab.units[i].group.as_deref() == Some(g))
                    .collect();
                (g.to_string(), members)
            })
            .collect();
        let tracks = entities
            .iter()
            .map(|id| Track {
                hp: id
                    .and_then(|id| world.state().entities.get(id))
                    .and_then(|e| e.hp)
                    .unwrap_or(0),
                ..Track::default()
            })
            .collect();
        let measures = Measures {
            tick: 0,
            groups: groups
                .iter()
                .map(|(name, members)| GroupMeasure {
                    name: name.clone(),
                    units: members.len() as u32,
                    alive: members.len() as u32,
                    ..GroupMeasure::default()
                })
                .collect(),
            ..Measures::default()
        };
        Ok(Self {
            world,
            ordered: vec![None; groups.len()],
            first_order: vec![None; groups.len()],
            lab: lab.clone(),
            entities,
            groups,
            tracks,
            measures,
            seq: 0,
        })
    }

    pub fn lab(&self) -> &Lab {
        &self.lab
    }

    pub fn measures(&self) -> &Measures {
        &self.measures
    }

    /// Ticks run.
    pub fn tick(&self) -> u32 {
        self.world.state().tick
    }

    pub fn finished(&self) -> bool {
        self.tick() >= self.lab.duration
    }

    /// The scenario's group `name` and its living members' entities.
    pub fn group(&self, name: &str) -> Vec<EntityId> {
        self.groups
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, members)| self.living(members))
            .unwrap_or_default()
    }

    fn living(&self, members: &[usize]) -> Vec<EntityId> {
        members
            .iter()
            .filter_map(|&i| self.entities[i])
            .filter(|&id| {
                self.world
                    .state()
                    .entities
                    .get(id)
                    .is_some_and(|e| e.hp.is_none_or(|hp| hp > 0))
            })
            .collect()
    }

    /// Gives `action` for `player` now, as a person at the keyboard would (the lab's hand
    /// orders): it applies on the next step. Whether the world took it in.
    pub fn command(&mut self, player: u8, action: Action) -> bool {
        self.seq += 1;
        self.world
            .enqueue(Queued {
                tick: self.world.tick(),
                player,
                seq: self.seq,
                action,
            })
            .is_ok()
    }

    /// Gives the orders of this tick, then steps the world and measures.
    pub fn step(&mut self) {
        let _span = tracing::info_span!("lab step").entered();
        let tick = self.world.tick();
        let orders: Vec<_> = self
            .lab
            .orders
            .iter()
            .filter(|o| o.tick == tick)
            .cloned()
            .collect();
        for o in orders {
            let Some(g) = self.groups.iter().position(|(n, _)| *n == o.group) else {
                continue;
            };
            self.first_order[g].get_or_insert(tick);
            let actors = self.living(&self.groups[g].1);
            let Some(owner) = actors
                .first()
                .and_then(|&id| self.world.state().entities.get(id))
                .map(|e| e.owner)
            else {
                continue;
            };
            let Owner::Player(player) = owner else {
                continue;
            };
            let point = |x: Option<f64>, y: Option<f64>| CellPoint {
                x: x.unwrap_or(0.0).max(0.0) as u32,
                y: y.unwrap_or(0.0).max(0.0) as u32,
                surface: None,
            };
            let target = o
                .target
                .as_deref()
                .and_then(|t| self.group(t).first().copied());
            if o.order.has_point() {
                self.ordered[g] = Some(tick);
                for &i in &self.groups[g].1 {
                    self.tracks[i].moving = true;
                }
                let group = &mut self.measures.groups[g];
                (group.arrived, group.first_arrival, group.last_arrival) = (0, None, None);
            }
            for chunk in actors.chunks(MAX_ACTORS) {
                let actors: Vec<u32> = chunk.iter().map(|id| id.0).collect();
                let action = match o.order {
                    OrderKind::Move | OrderKind::AttackMove => Action::Move {
                        actors,
                        destination: point(o.x, o.y),
                        attack_move: (o.order == OrderKind::AttackMove).then_some(true),
                        append: None,
                    },
                    OrderKind::Attack => match target {
                        Some(t) => Action::Attack {
                            actors,
                            target: t.0,
                            force: None,
                            append: None,
                        },
                        None => continue,
                    },
                    OrderKind::Hold => Action::Hold {
                        actors,
                        append: None,
                    },
                    OrderKind::Stop => Action::Stop { actors },
                };
                self.seq += 1;
                let _ = self.world.enqueue(Queued {
                    tick,
                    player,
                    seq: self.seq,
                    action,
                });
            }
        }
        self.world.step();
        self.measure();
    }

    fn measure(&mut self) {
        let tick = self.world.state().tick;
        self.measures.tick = tick;
        let state = self.world.state();
        let spatial = self.world.spatial();
        for (g, (_, members)) in self.groups.iter().enumerate() {
            let m = &mut self.measures.groups[g];
            m.alive = 0;
            let catalog = self.world.catalog();
            let in_reach = members
                .iter()
                .filter_map(|&i| self.entities[i].and_then(|id| state.entities.get(id)))
                .filter(|e| {
                    let range = catalog
                        .get(e.definition)
                        .behaviors
                        .combat
                        .as_ref()
                        .map_or(0.0, |c| c.range);
                    e.hp.is_none_or(|hp| hp > 0)
                        && e.unit
                            .as_ref()
                            .and_then(|u| u.target)
                            .and_then(|t| state.entities.get(t))
                            .is_some_and(|t| {
                                t.hp.is_none_or(|hp| hp > 0)
                                    && spatial.body_range(e, t) <= range * range + 1e-6
                            })
                })
                .count() as u32;
            m.most_in_reach = m.most_in_reach.max(in_reach);
            let swinging = members
                .iter()
                .filter_map(|&i| self.entities[i].and_then(|id| state.entities.get(id)))
                .any(|e| {
                    e.hp.is_none_or(|hp| hp > 0)
                        && e.unit.as_ref().is_some_and(|u| u.combat.attack.is_some())
                });
            if swinging
                && m.first_swing.is_none()
                && let Some(at) = self.first_order[g]
            {
                m.first_swing = Some(tick.saturating_sub(at));
            }
            for &i in members {
                let hp = self.entities[i]
                    .and_then(|id| state.entities.get(id))
                    .and_then(|e| e.hp)
                    .unwrap_or(0);
                let track = &mut self.tracks[i];
                m.damage_taken += u64::from(track.hp.saturating_sub(hp));
                track.hp = hp;
                let Some(e) = self.entities[i].and_then(|id| state.entities.get(id)) else {
                    continue;
                };
                if e.hp.is_some_and(|hp| hp == 0) {
                    continue;
                }
                m.alive += 1;
                let Some(u) = e.unit.as_ref() else {
                    continue;
                };
                let track = &mut self.tracks[i];
                let here = (e.position.x, e.position.y);
                let step = track
                    .last
                    .map(|(x, y)| (i64::from(here.0 - x), i64::from(here.1 - y)));
                track.last = Some(here);
                let speed = spatial
                    .profiles
                    .get(e.definition)
                    .motion
                    .map_or(0.0, |m| m.speed);
                let budget = speed * 1000.0 / f64::from(canopy_content::lab::TICKS);
                let moved = step.map_or(0.0, |(dx, dy)| ((dx * dx + dy * dy) as f64).sqrt());
                let attacking = u.combat.attack.is_some();
                if !u.route.is_empty() && !attacking && moved < budget / 4.0 {
                    m.stuck_ticks += 1;
                    track.stuck += 1;
                    m.longest_stuck = m.longest_stuck.max(track.stuck);
                } else {
                    track.stuck = 0;
                }
                if let Some((dx, dy)) = step.filter(|_| moved > budget / 4.0) {
                    if let Some((px, py)) = track.motion {
                        let dot = (dx * px + dy * py) as f64;
                        let lengths = (((dx * dx + dy * dy) * (px * px + py * py)) as f64).sqrt();
                        if lengths > 0.0 && dot < 0.5 * lengths {
                            m.flips += 1;
                        }
                    }
                    track.motion = Some((dx, dy));
                } else {
                    track.motion = None;
                }
                let busy = !u.route.is_empty()
                    || matches!(
                        u.order,
                        Some(Order::Move { .. } | Order::Patrol { .. } | Order::Follow { .. })
                    );
                if track.moving
                    && !busy
                    && u.target.is_none()
                    && let Some(at) = self.ordered[g]
                    && tick > at
                {
                    track.moving = false;
                    m.arrived += 1;
                    let took = tick - at;
                    m.first_arrival.get_or_insert(took);
                    m.last_arrival = Some(took);
                }
            }
        }
        self.overlap();
    }

    /// The deepest overlap of two ground bodies, through buckets of 8 cells.
    fn overlap(&mut self) {
        let state = self.world.state();
        let spatial = self.world.spatial();
        let mut bodies: Vec<(i32, i32, i32)> = Vec::new();
        for e in state.entities.iter() {
            if e.unit.is_none() || e.hp.is_some_and(|hp| hp == 0) {
                continue;
            }
            let u = e.unit.as_ref().expect("unit");
            if u.contained.is_some() || u.garrison.is_some() {
                continue;
            }
            let body = spatial.body_of(e);
            if body.air {
                continue;
            }
            bodies.push((e.position.x, e.position.y, body.radius_milli()));
        }
        const BUCKET: i32 = 8000;
        let mut buckets: BTreeMap<(i32, i32), Vec<usize>> = BTreeMap::new();
        for (i, b) in bodies.iter().enumerate() {
            buckets
                .entry((b.0.div_euclid(BUCKET), b.1.div_euclid(BUCKET)))
                .or_default()
                .push(i);
        }
        let mut worst = 0i64;
        let mut pairs = 0u32;
        for (i, a) in bodies.iter().enumerate() {
            let (bx, by) = (a.0.div_euclid(BUCKET), a.1.div_euclid(BUCKET));
            for nx in bx - 1..=bx + 1 {
                for ny in by - 1..=by + 1 {
                    for &j in buckets.get(&(nx, ny)).into_iter().flatten() {
                        if j <= i {
                            continue;
                        }
                        let b = bodies[j];
                        let (dx, dy) = (i64::from(a.0 - b.0), i64::from(a.1 - b.1));
                        let reach = i64::from(a.2 + b.2);
                        let d2 = dx * dx + dy * dy;
                        if d2 < reach * reach {
                            pairs += 1;
                            let depth = reach - (d2 as f64).sqrt() as i64;
                            worst = worst.max(depth);
                        }
                    }
                }
            }
        }
        self.measures.overlapping = pairs;
        self.measures.worst_overlap = self.measures.worst_overlap.max(worst as f64 / 1000.0);
    }
}
