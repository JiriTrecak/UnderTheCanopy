//! Economy cost: eight colonies of 40 workers and 15 buildings each running a full economy
//! (mining, felling, delivering, building, training, repairing) on a shipped map. A
//! measurement, not a behaviour test; run it in release:
//!
//! `cargo test --release -p canopy-sim --test economy_perf -- --ignored --nocapture`
#![allow(clippy::disallowed_methods)] // wall-clock timing of the benchmark itself

use canopy_content::{
    Content,
    game::{Creation, Kind, ProductionMode},
    map,
    placement::{Owner, Placement, Point},
};
use canopy_sim::{
    Action, MatchSetup, Queued, Slot, SlotKind, World,
    action::{FoundationPoint, HalfCells},
    catalog::{Catalog, Def},
    entity::EntityId,
    spatial::Spot,
    spawn,
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{Arc, Mutex},
    time::Instant,
};

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// Times spans by name.
#[derive(Default)]
struct Timer {
    names: Mutex<Vec<&'static str>>,
    stack: Mutex<Vec<(u64, Instant)>>,
    totals: Mutex<BTreeMap<&'static str, f64>>,
}

impl tracing::Subscriber for Timer {
    fn enabled(&self, _: &tracing::Metadata<'_>) -> bool {
        true
    }
    fn new_span(&self, span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
        let mut names = self.names.lock().unwrap();
        names.push(span.metadata().name());
        tracing::span::Id::from_u64(names.len() as u64)
    }
    fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
    fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
    fn event(&self, _: &tracing::Event<'_>) {}
    fn enter(&self, span: &tracing::span::Id) {
        self.stack.lock().unwrap().push((span.into_u64(), Instant::now()));
    }
    fn exit(&self, _: &tracing::span::Id) {
        let (id, start) = self.stack.lock().unwrap().pop().expect("entered");
        let name = self.names.lock().unwrap()[id as usize - 1];
        *self.totals.lock().unwrap().entry(name).or_default() += start.elapsed().as_secs_f64() * 1000.0;
    }
}

fn place(world: &mut World, catalog: &Catalog, owner: Owner, def: &str, x: f64, y: f64, name: &str) -> u32 {
    let placement = Placement {
        activation: None,
        id: name.into(),
        map_knowledge: None,
        definition: canopy_content::Id::new(def).unwrap(),
        position: Point { x, y, surface: None },
        rotation: 0.0,
        owner,
        initial_state: None,
        appearance: None,
    };
    spawn::create(world.state_mut(), catalog, &placement).unwrap().0
}

/// The nearest free cells to `centre` for a body of `def`.
fn free_cells(world: &World, def: Def, centre: (i32, i32), count: usize, taken: &mut Vec<(i32, i32)>) -> Vec<(i32, i32)> {
    let body = world.spatial().profiles.get(def).body;
    let mut out = Vec::new();
    for r in 0i32..80 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r || out.len() == count {
                    continue;
                }
                let (x, y) = (centre.0 + 2 * dx, centre.1 + 2 * dy);
                let spot = Spot::new(x, y, None);
                if !taken.contains(&(x, y)) && world.spatial().valid_spot(spot) && world.spatial().unit_walkable(spot, &body) {
                    out.push((x, y));
                    taken.push((x, y));
                }
            }
        }
    }
    out
}

/// A free site for `def` near `centre` on its building lattice, found with the placement
/// rules themselves.
fn site(world: &mut World, owner: Owner, def: &str, centre: (f64, f64), builder: u32) -> Option<(f64, f64)> {
    let footprint = world.catalog().content.definition(def)?.footprint.clone()?;
    let snap = |c: f64, w: u32| {
        let phase = f64::from(w) / 2.0 - 0.5;
        c - (c - phase).rem_euclid(4.0)
    };
    let (sx, sy) = (snap(centre.0, footprint.width), snap(centre.1, footprint.depth));
    for r in 1i32..12 {
        for dy in -r..=r {
            for dx in -r..=r {
                if dx.abs().max(dy.abs()) != r {
                    continue;
                }
                let x = sx + f64::from(4 * dx);
                let y = sy + f64::from(4 * dy);
                if x < 0.0 || y < 0.0 {
                    continue;
                }
                let point = FoundationPoint { x: HalfCells((x * 2.0) as u32), y: HalfCells((y * 2.0) as u32), surface: None };
                if world.can_build(owner, def, &point, Some(EntityId(builder)), 0).is_none() {
                    return Some((x, y));
                }
            }
        }
    }
    None
}

fn enqueue(world: &mut World, player: u8, seq: &mut u32, action: Action) {
    let tick = world.tick();
    world.enqueue(Queued { tick, player, seq: *seq, action }).unwrap();
    *seq += 1;
}

#[test]
#[ignore = "benchmark; run in release with --ignored --nocapture"]
fn eight_colonies_run_a_full_economy() {
    let content = Arc::new(Content::load(data()).unwrap_or_else(|errors| panic!("{errors}")));
    let catalog = Catalog::new(content.clone());
    let maps = map::library(data()).unwrap_or_else(|errors| panic!("{errors}"));
    let map = maps
        .iter()
        .map(|file| file.load().unwrap_or_else(|errors| panic!("{errors}")))
        .max_by_key(|m| (m.player_starts.len(), m.size))
        .expect("a shipped map");
    let rules = &content.rules;
    let fort = rules.starting_setup.fort.clone();
    let worker = rules
        .starting_setup
        .units
        .iter()
        .find(|u| content.definition(u.definition.as_str()).is_some_and(|d| d.behaviors.work.is_some()))
        .unwrap()
        .definition
        .clone();
    let worker_def = content.definition(worker.as_str()).unwrap();
    let work = worker_def.behaviors.work.as_ref().unwrap();
    let source = |item: &canopy_content::Id| match content.definition(item.as_str()).and_then(|d| d.creation.as_ref()) {
        Some(Creation::Harvest { source, .. }) => Some(source.clone()),
        _ => None,
    };
    let base_item = content.definition(fort.as_str()).unwrap().behaviors.storage.as_ref().unwrap().gathering.as_ref().unwrap().item.clone();
    let mine = source(&base_item).unwrap();
    let wood_source = work
        .harvests
        .iter()
        .flatten()
        .filter_map(source)
        .find(|s| content.definition(s.as_str()).is_some_and(|d| d.felling.is_some()))
        .unwrap();
    let builds: Vec<&canopy_content::game::Definition> = work.builds.iter().filter_map(|b| content.definition(b.as_str())).collect();
    let house = builds
        .iter()
        .filter(|d| d.supply_provided.is_some() && d.id != fort && d.requires.is_none())
        .min_by_key(|d| d.footprint.as_ref().map(|f| f.width))
        .unwrap()
        .id
        .clone();
    let barracks = builds
        .iter()
        .find(|d| {
            d.requires.is_none()
                && d.behaviors.production.as_ref().is_some_and(|p| {
                    p.mode == ProductionMode::Queued
                        && p.outputs.iter().any(|o| content.definition(o.as_str()).is_some_and(|u| u.behaviors.combat.is_some()))
                })
        })
        .unwrap()
        .id
        .clone();
    let soldier = content
        .definition(barracks.as_str())
        .unwrap()
        .behaviors
        .production
        .as_ref()
        .unwrap()
        .outputs
        .iter()
        .find(|o| content.definition(o.as_str()).is_some_and(|u| u.kind == Kind::Unit && u.behaviors.combat.is_some() && u.requires.is_none()))
        .unwrap()
        .clone();
    let players = 8u8;
    let setup = MatchSetup {
        seed: 9,
        slots: (0..players)
            .map(|player| Slot { player, kind: SlotKind::Ai, name: None, team: None, hero: None, race: None })
            .collect(),
    };
    let mut world = World::new(content.clone(), &map, setup).unwrap_or_else(|e| panic!("{e}"));
    // Anchors: the map's starts, then mines far from every anchor so far.
    let mut anchors: Vec<(f64, f64)> = map.player_starts.iter().map(|s| (s.x, s.z)).collect();
    let mines: Vec<(f64, f64)> = world
        .state()
        .entities
        .iter()
        .filter(|e| catalog.id(e.definition) == &mine)
        .map(|e| (f64::from(e.position.x) / 1000.0, f64::from(e.position.y) / 1000.0))
        .collect();
    let worker_id = catalog.find(worker.as_str()).unwrap();
    let mut taken = Vec::new();
    let mut seq = 0;
    let built = Instant::now();
    for p in 0..usize::from(players) {
        let owner = Owner::Player(p as u8);
        let wallet = world.state_mut().wallets.entry(owner).or_default();
        for item in [base_item.clone()].into_iter().chain(work.harvests.iter().flatten().cloned()) {
            wallet.insert(item, 100_000);
        }
        let mut workers: Vec<u32> = world
            .state()
            .entities
            .iter()
            .filter(|e| e.owner == owner && e.definition == worker_id)
            .map(|e| e.id.0)
            .collect();
        let hall = match anchors.get(p) {
            Some(&start) => start,
            None => {
                let first = free_cells(&world, worker_id, (mines[0].0 as i32, mines[0].1 as i32), 1, &mut taken)[0];
                workers.push(place(&mut world, &catalog, owner, worker.as_str(), f64::from(first.0), f64::from(first.1), &format!("bench.{p}.first")));
                let mut candidates = mines.clone();
                let distance = |c: &(f64, f64), anchors: &[(f64, f64)]| {
                    anchors.iter().map(|q| (c.0 - q.0).powi(2) + (c.1 - q.1).powi(2)).fold(f64::MAX, f64::min)
                };
                candidates.sort_by(|a, b| distance(b, &anchors).total_cmp(&distance(a, &anchors)));
                let found = candidates
                    .into_iter()
                    .find_map(|c| site(&mut world, owner, fort.as_str(), c, workers[0]))
                    .expect("fort site");
                place(&mut world, &catalog, owner, fort.as_str(), found.0, found.1, &format!("bench.{p}.fort"));
                anchors.push(found);
                found
            }
        };
        let centre = (hall.0 as i32, hall.1 as i32 + 16);
        let cells = free_cells(&world, worker_id, centre, 40 - workers.len(), &mut taken);
        for (i, (x, y)) in cells.into_iter().enumerate() {
            workers.push(place(&mut world, &catalog, owner, worker.as_str(), f64::from(x), f64::from(y), &format!("bench.{p}.w{i}")));
        }
        for b in 0..13 {
            let def = if b < 9 { &house } else { &barracks };
            if let Some((x, y)) = site(&mut world, owner, def.as_str(), hall, workers[0]) {
                place(&mut world, &catalog, owner, def.as_str(), x, y, &format!("bench.{p}.b{b}"));
            }
        }
        // Mining, felling, a construction site, training and repairs.
        let near = |world: &World, def: &canopy_content::Id, from: (f64, f64)| {
            world
                .state()
                .entities
                .iter()
                .filter(|e| catalog.id(e.definition) == def && e.resource.as_ref().is_some_and(|r| r.amount > 0))
                .min_by_key(|e| {
                    let (dx, dy) = (f64::from(e.position.x) / 1000.0 - from.0, f64::from(e.position.y) / 1000.0 - from.1);
                    ((dx * dx + dy * dy) as i64, e.id)
                })
                .map(|e| e.id.0)
                .unwrap()
        };
        let mine_id = near(&world, &mine, hall);
        let tree_id = near(&world, &wood_source, hall);
        world.step();
        enqueue(&mut world, p as u8, &mut seq, Action::Gather { actors: workers[..26].to_vec(), target: mine_id, append: None });
        enqueue(&mut world, p as u8, &mut seq, Action::Gather { actors: workers[26..36].to_vec(), target: tree_id, append: None });
        if let Some((x, y)) = site(&mut world, owner, house.as_str(), hall, workers[36]) {
            enqueue(&mut world, p as u8, &mut seq, Action::Build {
                actors: vec![workers[36]],
                definition: house.clone(),
                position: FoundationPoint { x: HalfCells((x * 2.0) as u32), y: HalfCells((y * 2.0) as u32), surface: None },
                rotation: None,
                append: None,
            });
        }
        let barracks_ids: Vec<u32> = world.state().entities.iter().filter(|e| e.owner == owner && catalog.id(e.definition) == &barracks).map(|e| e.id.0).collect();
        for &b in &barracks_ids {
            for _ in 0..2 {
                enqueue(&mut world, p as u8, &mut seq, Action::Produce { actor: b, definition: soldier.clone() });
            }
        }
        let houses: Vec<EntityId> = world.state().entities.iter().filter(|e| e.owner == owner && catalog.id(e.definition) == &house).map(|e| e.id).collect();
        for id in houses.into_iter().take(2) {
            if let Some(e) = world.state_mut().entities.get_mut(id) {
                e.hp = e.hp.map(|hp| hp / 2);
            }
        }
    }
    let counts: Vec<(usize, usize)> = (0..players)
        .map(|p| {
            let owner = Owner::Player(p);
            let units = world.state().entities.iter().filter(|e| e.owner == owner && e.definition == worker_id).count();
            let buildings = world.state().entities.iter().filter(|e| e.owner == owner && catalog.kind(e.definition) == Kind::Building).count();
            (units, buildings)
        })
        .collect();
    println!("{} map, setup {:.0} ms; workers and buildings per colony: {counts:?}", map.name, built.elapsed().as_secs_f64() * 1000.0);
    let timer = Arc::new(Timer::default());
    let ticks: u32 = std::env::var("TICKS").ok().and_then(|t| t.parse().ok()).unwrap_or(2400);
    let mut steps = Vec::new();
    tracing::subscriber::with_default(timer.clone(), || {
        for _ in 0..ticks {
            let start = Instant::now();
            world.step();
            steps.push(start.elapsed().as_secs_f64() * 1000.0);
            if std::env::var("SLOW").is_ok() && *steps.last().unwrap() > 20.0 {
                eprintln!("slow tick {}: {:.1} ms", world.state().tick, steps.last().unwrap());
            }
        }
    });
    let totals = timer.totals.lock().unwrap().clone();
    let per_tick = |name: &str| totals.get(name).copied().unwrap_or(0.0) / f64::from(ticks);
    println!(
        "economy per tick: assign {:.3} ms + advance {:.3} ms = {:.3} ms (step {:.3} ms on average)",
        per_tick("economy assign"),
        per_tick("economy advance"),
        per_tick("economy assign") + per_tick("economy advance"),
        steps.iter().sum::<f64>() / steps.len() as f64,
    );
    let state = world.state();
    let jobs = state.economy.jobs.len();
    let produced = state.accounting.produced.values().sum::<u32>();
    let soldiers = state.entities.iter().filter(|e| catalog.id(e.definition) == &soldier).count();
    println!("after {ticks} ticks: {jobs} jobs, {produced} goods harvested, {soldiers} soldiers trained");
    let view = Instant::now();
    for p in 0..players {
        let v = world.view(Owner::Player(p));
        assert!(v.colony.is_some());
    }
    println!("eight views with colony data: {:.3} ms", view.elapsed().as_secs_f64() * 1000.0);
    world.verify_economy().unwrap();
}
