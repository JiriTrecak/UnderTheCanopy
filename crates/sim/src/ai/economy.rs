//! The colony planner (old `ai/economy.ts`): rebuilding, supply, hero revival and
//! recruitment, army production, upgrades, research, outposts, timber, worker training
//! and harvesting assignments. Every quantity comes from the player's own stocks and the
//! content's recipes; nothing names a content id.

use super::{
    Beat,
    frame::Known,
    geography::{Pt, dist},
    placement::{placeable, snap},
    state::site_key,
};
use crate::{
    action::{Action, FoundationPoint, HalfCells},
    catalog::Def,
    economy::{production::lane_size, revival, supply},
    heroes::Roster,
    orders::Order,
    terrain::footprint::{Extent, resource_center_separation},
};
use canopy_content::{
    Id,
    game::{Cost, Creation, Definition, Kind, ProductionMode},
};
use std::collections::BTreeMap;

type Bill = Vec<(Id, u32)>;

fn add_max(bill: &mut Bill, item: &Id, amount: u32) {
    match bill.iter_mut().find(|(i, _)| i == item) {
        Some((_, n)) => *n = (*n).max(amount),
        None => bill.push((item.clone(), amount)),
    }
}

fn items(d: &Definition) -> &[Cost] {
    d.creation.as_ref().map_or(&[], |c| c.items())
}

/// The economy pass with its revival savings guard.
struct Planner<'b, 'f, 'a> {
    b: &'b mut Beat<'f, 'a>,
    /// Goods kept for reviving a hero; spending below them is refused unless the
    /// workforce must recover.
    savings: Bill,
}

impl<'b, 'f, 'a> Planner<'b, 'f, 'a> {
    fn emit(&mut self, action: Action, reason: impl Into<String>) -> bool {
        let f = self.b.f;
        if !self.savings.is_empty() {
            let catalog = f.catalog();
            let recruit = match &action {
                Action::Produce { definition, .. } => f.find(definition.as_str()),
                _ => None,
            };
            let workforce = recruit.is_some_and(|d| {
                catalog.get(d).behaviors.work.is_some()
                    && (f.workers.len() as u32) < f.rules.workers.minimum
            });
            let bill: Option<Bill> = match &action {
                Action::Build { definition, .. } => f
                    .find(definition.as_str())
                    .map(|d| bill(items(catalog.get(d)))),
                Action::Produce { .. } => recruit.map(|d| bill(items(catalog.get(d)))),
                Action::Upgrade { actor } => f
                    .get(*actor)
                    .filter(|k| f.buildings.iter().any(|&i| f.at(i).id == k.id))
                    .and_then(|k| f.def(k).upgrade.as_ref())
                    .map(|u| bill(&u.items)),
                Action::Research { research, .. } => catalog
                    .content
                    .rules
                    .research
                    .get(research.as_str())
                    .map(|r| bill(&r.items)),
                _ => None,
            };
            if !workforce
                && bill.is_some_and(|bill| {
                    bill.iter().any(|(item, amount)| {
                        let kept = self
                            .savings
                            .iter()
                            .find(|(i, _)| i == item)
                            .map_or(0, |(_, n)| *n);
                        i64::from(f.balance(item.as_str())) - i64::from(*amount) < i64::from(kept)
                    })
                })
            {
                return false;
            }
        }
        self.b.emit(action, reason)
    }

    /// Places `d` near `origin` with an available builder (old `build`).
    fn build(&mut self, def: Def, reason: &str, origin: Pt) -> bool {
        let f = self.b.f;
        let d = f.definition(def);
        let builds = |w: &Known| {
            f.def(w)
                .behaviors
                .work
                .as_ref()
                .is_some_and(|work| work.builds.contains(&d.id))
        };
        let builders: Vec<&Known> = f
            .workers
            .iter()
            .map(|&i| f.at(i))
            .filter(|w| builds(w))
            .collect();
        let worker = builders
            .iter()
            .find(|w| f.free(w))
            .or_else(|| {
                builders.iter().find(|w| {
                    matches!(f.order(w), Some(Order::Gather { .. }))
                        && f.cargo(w).is_none()
                        && !w.contained
                        && f.queued(w).is_empty()
                })
            })
            .or_else(|| {
                builders.iter().find(|w| {
                    f.cargo(w).is_some_and(|c| !f.accepts(c.item.as_str()))
                        && f.queued(w).is_empty()
                        && f.order(w).is_none_or(|o| matches!(o, Order::Gather { .. }))
                })
            })
            .map(|w| w.id);
        let Some(worker) = worker else {
            return false;
        };
        if !f.can_afford(d, &[])
            || f.buildings.iter().any(|&i| f.at(i).construction)
            || f.buildings.len() as u32 >= f.catalog().content.rules.max_buildings
        {
            return false;
        }
        let near = d.placement_near.as_ref();
        let near_radius = near
            .and_then(|n| f.find(n.source.as_str()))
            .map(|s| {
                let source = f.definition(s);
                let (x, y) = resource_center_separation(
                    Extent::of(d.footprint.as_ref()),
                    Extent::of(source.footprint.as_ref()),
                    f64::from(source.construction_clearance.unwrap_or(0)),
                    0.0,
                    0.0,
                );
                x.max(y)
            })
            .unwrap_or(0.0);
        let home = f.home;
        for _ in 0..f.rules.limits.placement_candidates {
            let k = self.b.s.placement_cursor % 192;
            self.b.s.placement_cursor = self.b.s.placement_cursor.wrapping_add(1);
            let ring = k / 32;
            let angle = f64::from(k % 32) * std::f64::consts::PI / 16.0;
            let radius = match near {
                Some(n) => f64::from(n.radius).min(near_radius + f64::from(ring % 3)),
                None => 14.0 + f64::from(ring) * 6.0,
            };
            let cursor = Pt::new(
                origin.x + crate::math::cos(angle) * radius,
                origin.y + crate::math::sin(angle) * radius,
            );
            let p = snap(d, cursor, 0);
            let (dx, dy) = (home.x - p.x, home.y - p.y);
            let r = if dx.abs() > dy.abs() {
                if dx > 0.0 { 90 } else { 270 }
            } else if dy > 0.0 {
                0
            } else {
                180
            };
            let p = snap(d, cursor, r);
            let key = site_key(d.id.as_str(), p.x, p.y, r);
            if self.b.s.failed_sites.get(&key).is_some_and(|&t| t > f.tick)
                || !placeable(f, def, p, r)
            {
                continue;
            }
            return self.emit(
                Action::Build {
                    actors: vec![worker],
                    definition: d.id.clone(),
                    position: FoundationPoint {
                        x: HalfCells((p.x * 2.0) as u32),
                        y: HalfCells((p.y * 2.0) as u32),
                        surface: None,
                    },
                    rotation: Some(r),
                    append: None,
                },
                reason,
            );
        }
        false
    }
}

fn bill(costs: &[Cost]) -> Bill {
    costs.iter().map(|c| (c.item.clone(), c.amount)).collect()
}

fn harvest_source(d: &Definition) -> Option<&Id> {
    match &d.creation {
        Some(Creation::Harvest { source, .. }) => Some(source),
        _ => None,
    }
}

/// The economy pass (old `economy`).
pub fn economy(b: &mut Beat) {
    let _span = tracing::trace_span!("ai economy").entered();
    let mut p = Planner {
        b,
        savings: Vec::new(),
    };
    run(&mut p);
}

fn run(p: &mut Planner) {
    let f = p.b.f;
    let catalog = f.catalog();
    let rules = f.rules;
    let tick = f.tick;
    let content = &catalog.content;
    let definitions = &content.definitions;
    let def_of = |id: &Id| f.find(id.as_str());
    let get = |id: &Id| def_of(id).map(|d| catalog.get(d));
    let workers: Vec<&Known> = f.workers.iter().map(|&i| f.at(i)).collect();
    let army: Vec<&Known> = f.army.iter().map(|&i| f.at(i)).collect();
    let buildings: Vec<&Known> = f.buildings.iter().map(|&i| f.at(i)).collect();
    let mut buildable: Vec<&Id> = workers
        .iter()
        .flat_map(|w| f.def(w).behaviors.work.iter().flat_map(|work| &work.builds))
        .collect();
    buildable.sort();
    buildable.dedup();
    let defs: Vec<Def> = (0..definitions.len())
        .map(|i| Def(i as u16))
        .filter(|&d| buildable.binary_search(&catalog.id(d)).is_ok())
        .collect();
    let trains_worker = |d: &Definition| {
        d.behaviors.production.as_ref().is_some_and(|prod| {
            prod.outputs
                .iter()
                .any(|id| get(id).is_some_and(|o| o.behaviors.work.is_some()))
        })
    };
    let houses: Vec<Def> = defs
        .iter()
        .copied()
        .filter(|&d| {
            let d = catalog.get(d);
            d.supply_provided.is_some_and(|s| s > 0) && d.behaviors.production.is_none()
        })
        .collect();
    let producers: Vec<Def> = defs
        .iter()
        .copied()
        .filter(|&d| {
            catalog
                .get(d)
                .behaviors
                .production
                .as_ref()
                .is_some_and(|prod| {
                    prod.mode == ProductionMode::Queued
                        && rules
                            .composition
                            .iter()
                            .any(|c| prod.outputs.contains(&c.definition))
                })
        })
        .collect();
    let sanctuary = defs
        .iter()
        .copied()
        .find(|&d| catalog.get(d).behaviors.revival.is_some());
    let forest = defs.iter().copied().find(|&d| {
        catalog
            .get(d)
            .behaviors
            .production
            .as_ref()
            .is_some_and(|prod| {
                prod.outputs.iter().any(|id| {
                    get(id).is_some_and(|o| matches!(o.creation, Some(Creation::Plant { .. })))
                })
            })
    });
    let builds = |w: &Known, d: Def| {
        f.def(w)
            .behaviors
            .work
            .as_ref()
            .is_some_and(|work| work.builds.contains(catalog.id(d)))
    };
    // Resume explicitly after a builder dies or is redirected: the simulation never
    // takes an idle worker on the player's behalf.
    for project in buildings.iter().filter(|b| b.construction) {
        let target = crate::entity::EntityId(project.id);
        let staffed = workers.iter().any(|w| {
            matches!(f.order(w), Some(Order::Construct { target: t }) if *t == target)
                || f.queued(w)
                    .iter()
                    .any(|o| matches!(o, Order::Construct { target: t } if *t == target))
        });
        if staffed {
            continue;
        }
        let eligible: Vec<&&Known> = workers.iter().filter(|w| builds(w, project.def)).collect();
        let builder = eligible.iter().find(|w| f.free(w)).or_else(|| {
            eligible.iter().find(|w| {
                matches!(f.order(w), Some(Order::Gather { .. }))
                    && f.cargo(w).is_none()
                    && f.queued(w).is_empty()
            })
        });
        if let Some(builder) = builder
            && p.emit(
                Action::Construct {
                    actors: vec![builder.id],
                    target: project.id,
                    append: None,
                },
                "Resume an unfinished foundation",
            )
        {
            return;
        }
    }
    let hall = defs
        .iter()
        .copied()
        .find(|&d| trains_worker(catalog.get(d)));
    if let Some(hall) = hall
        && !buildings.iter().any(|b| trains_worker(f.def(b)))
    {
        // Keep the wallet for recovery while the bounded site search continues.
        p.build(
            hall,
            "Rebuild worker production and the main drop-off",
            f.home,
        );
        return;
    }
    let mut queues: Vec<Def> = Vec::new();
    let mut worker_queues: Vec<Def> = Vec::new();
    for b in &buildings {
        for q in f.queue(b) {
            if catalog.get(q.definition).behaviors.work.is_some() {
                worker_queues.push(q.definition);
            } else {
                queues.push(q.definition);
            }
        }
    }
    let supply = f.colony.supply;
    let mut promised: Bill = Vec::new();
    for b in &buildings {
        let mut bill: BTreeMap<&Id, u32> = BTreeMap::new();
        let mut order: Vec<&Id> = Vec::new();
        for q in f.queue(b) {
            for c in items(catalog.get(q.definition)) {
                if !bill.contains_key(&c.item) {
                    order.push(&c.item);
                }
                *bill.entry(&c.item).or_default() += c.amount;
            }
        }
        let inventory = f.own_entity(b).map(|e| &e.inventory);
        for id in order {
            let n = bill[id];
            let held = inventory.and_then(|inv| inv.get(id)).copied().unwrap_or(0);
            let more = n.saturating_sub(held);
            match promised.iter_mut().find(|(i, _)| i == id) {
                Some((_, v)) => *v += more,
                None => promised.push((id.clone(), more)),
            }
        }
    }
    let ready: Vec<&Known> = buildings
        .iter()
        .copied()
        .filter(|b| !b.construction)
        .collect();
    let barracks: Vec<&Known> = ready
        .iter()
        .copied()
        .filter(|b| producers.contains(&b.def))
        .collect();
    // Planned capacity prevents duplicate houses; started training keeps its reservation.
    let planned: u32 = buildings
        .iter()
        .filter(|b| b.construction)
        .map(|b| f.def(b).supply_provided.unwrap_or(0))
        .sum();
    if supply.capacity < supply.limit
        && supply.committed + 4 > supply.limit.min(supply.capacity + planned)
        && let Some(house) = houses
            .iter()
            .copied()
            .find(|&d| f.can_afford(catalog.get(d), &promised))
        && p.build(house, "Expand colony supply", f.home)
    {
        return;
    }
    let admitted =
        |d: &Definition| supply::admission(&supply, d.supply_cost.unwrap_or(0), None).is_none();
    for hero in f.fallen.iter() {
        // A hero returning through an ability is not revived at a building.
        let returning = hero
            .links
            .as_ref()
            .is_some_and(|l| l.spell_return.is_some());
        if returning
            || buildings.iter().any(|b| {
                f.own_entity(b)
                    .and_then(|e| e.revival.as_ref())
                    .is_some_and(|r| r.queue.iter().any(|q| q.hero == hero.id))
            })
        {
            continue;
        }
        let altar = ready.iter().find(|b| {
            let Some(e) = f.own_entity(b) else {
                return false;
            };
            e.revival.is_some()
                && f.def(b)
                    .behaviors
                    .revival
                    .as_ref()
                    .is_some_and(|r| (lane_size(e) as u32) < r.queue_capacity)
        });
        let hero_def = catalog.get(hero.definition);
        if let Some(altar) = altar.filter(|_| admitted(hero_def)) {
            let level =
                crate::stats::Stats::of(catalog, hero, crate::stats::research(f.input.state, f.me))
                    .level;
            let policy = f.def(altar).behaviors.revival.as_ref().expect("revival");
            let (terms, _) = revival::terms(policy, level);
            if terms.iter().all(|(item, n)| f.balance(item.as_str()) >= *n)
                && p.emit(
                    Action::Revive {
                        actor: altar.id,
                        hero: hero.id.0,
                    },
                    "Restore our veteran hero",
                )
            {
                return;
            }
            p.savings = terms;
            break;
        } else if altar.is_none()
            && let Some(sanctuary) = sanctuary
            && !buildings
                .iter()
                .any(|b| f.def(b).behaviors.revival.is_some())
        {
            if f.can_afford(catalog.get(sanctuary), &promised)
                && p.build(sanctuary, "Build a sanctuary for the fallen hero", f.home)
            {
                return;
            }
            p.savings = bill(items(catalog.get(sanctuary)));
            break;
        }
    }
    let has_producer = buildings.iter().any(|b| producers.contains(&b.def));
    let w = &rules.workers;
    let income_need = w.maximum.min(w.target + army.len() as u32 / 4);
    p.b.s.plan.workers = income_need;
    let pressure: f64 =
        p.b.s
            .sightings
            .iter()
            .filter(|e| {
                dist(f.geo.from_place(&e.point), f.home) < 40.0
                    && catalog.get(e.definition).behaviors.camp_defense.is_none()
                    && tick - e.seen < 400
            })
            .map(|e| {
                f.nominal_power(
                    catalog.get(e.definition),
                    Some(f64::from(e.hp)),
                    None,
                    None,
                    None,
                )
            })
            .sum();
    let typical = if rules.composition.is_empty() {
        0.0
    } else {
        rules
            .composition
            .iter()
            .map(|c| get(&c.definition).map_or(0.0, |d| f.nominal_power(d, None, None, None, None)))
            .sum::<f64>()
            / rules.composition.len() as f64
    };
    let a = &rules.army;
    let wanted = (workers.len() as u32).saturating_sub(w.minimum) * 2;
    let pressured = (pressure / typical.max(1.0) * 1.3).ceil() as u32;
    p.b.s.plan.army = a.maximum.min(a.minimum.max(wanted).max(pressured));
    let train_worker = |p: &mut Planner, reserve: &Bill| -> bool {
        if (workers.len() + worker_queues.len()) as u32 >= income_need {
            return false;
        }
        for hall in &ready {
            let Some(output) = f.def(hall).behaviors.production.as_ref().and_then(|prod| {
                prod.outputs
                    .iter()
                    .find(|id| get(id).is_some_and(|o| o.behaviors.work.is_some()))
            }) else {
                continue;
            };
            let e = f.own_entity(hall);
            if e.is_some_and(|e| e.upgrade.is_some()) || f.queue(hall).len() >= 2 {
                continue;
            }
            let Some(d) = get(output) else {
                continue;
            };
            if admitted(d)
                && f.can_afford(d, reserve)
                && p.emit(
                    Action::Produce {
                        actor: hall.id,
                        definition: output.clone(),
                    },
                    "Train a worker for the economy",
                )
            {
                return true;
            }
        }
        false
    };
    if ((workers.len() + worker_queues.len()) as u32) < w.minimum && train_worker(p, &promised) {
        return;
    }
    if !has_producer
        && let Some(d) = producers
            .iter()
            .copied()
            .find(|&d| f.can_afford(catalog.get(d), &promised))
        && p.build(d, "Establish army production", f.home)
    {
        return;
    }
    let roster = Roster::of(f.input.state, catalog, f.me);
    let hero_buildings: Vec<Def> = (0..definitions.len())
        .map(|i| Def(i as u16))
        .filter(|&d| {
            let dd = catalog.get(d);
            dd.behaviors.production.as_ref().is_some_and(|prod| {
                prod.mode == ProductionMode::Queued
                    && prod
                        .outputs
                        .iter()
                        .any(|id| get(id).is_some_and(|o| o.is_hero()))
            }) && (defs.contains(&d) || ready.iter().any(|b| b.def == d))
        })
        .collect();
    let mut hero_choices: Vec<Def> = Vec::new();
    for &d in &hero_buildings {
        for id in &catalog
            .get(d)
            .behaviors
            .production
            .as_ref()
            .expect("production")
            .outputs
        {
            if let Some(o) = def_of(id)
                && !hero_choices.contains(&o)
            {
                hero_choices.push(o);
            }
        }
    }
    hero_choices.retain(|&d| {
        let d = catalog.get(d);
        d.is_hero() && !roster.definitions.contains(&d.id)
    });
    hero_choices.sort_by(|&a, &b| {
        let (da, db) = (catalog.get(a), catalog.get(b));
        da.display_order
            .unwrap_or(0)
            .cmp(&db.display_order.unwrap_or(0))
            .then(a.cmp(&b))
    });
    let recruit = hero_choices.iter().copied().find(|&d| {
        supply::hero_admission(catalog, d, &roster).is_none()
            && f.available(catalog.get(d).requires.as_deref())
    });
    let trainer = recruit.and_then(|r| {
        let rid = catalog.id(r);
        ready.iter().copied().find(|b| {
            let Some(e) = f.own_entity(b) else {
                return false;
            };
            let prod = f.def(b).behaviors.production.as_ref();
            e.upgrade.is_none()
                && !e.production.as_ref().is_some_and(|p| p.paused)
                && prod.is_some_and(|prod| {
                    prod.outputs.contains(rid)
                        && prod
                            .queue_capacity
                            .is_some_and(|c| (lane_size(e) as u32) < c)
                })
        })
    });
    let academy = recruit.and_then(|r| {
        let rid = catalog.id(r);
        hero_buildings.iter().copied().find(|&d| {
            defs.contains(&d)
                && catalog
                    .get(d)
                    .behaviors
                    .production
                    .as_ref()
                    .is_some_and(|prod| prod.outputs.contains(rid))
                && !buildings.iter().any(|b| b.def == d)
        })
    });
    let fulfils = &f.input.economy().tables().fulfils;
    let useful_upgrade = |d: &Definition| -> bool {
        let Some(upgrade) = &d.upgrade else {
            return false;
        };
        let Some(target_def) = def_of(&upgrade.target) else {
            return false;
        };
        let target = catalog.get(target_def);
        if target.hero_capacity.unwrap_or(0) <= d.hero_capacity.unwrap_or(0) {
            return true;
        }
        if !hero_choices.is_empty()
            && roster.used() >= roster.capacity as usize
            && target.hero_capacity.unwrap_or(0) > roster.capacity
        {
            return true;
        }
        let own = def_of(&d.id).map_or(&[][..], |d| fulfils[usize::from(d.0)].as_slice());
        let added: Vec<&Id> = fulfils[usize::from(target_def.0)]
            .iter()
            .filter(|x| !own.contains(x))
            .map(|&x| catalog.id(x))
            .collect();
        let needs = |requires: &Option<Vec<Id>>| {
            requires
                .as_ref()
                .is_some_and(|r| r.iter().any(|id| added.contains(&id)))
        };
        definitions.iter().any(|x| needs(&x.requires))
            || content.rules.research.values().any(|r| needs(&r.requires))
    };
    let payable = |costs: &[Cost]| costs.iter().all(|c| f.balance(c.item.as_str()) >= c.amount);
    let threatened = f.hostiles.iter().any(|&i| dist(f.at(i).p, f.home) < 32.0);
    // Invest only after a fighting force exists and no observed enemy threatens home.
    if army.len() >= 4 && workers.len() as u32 >= w.minimum && !threatened {
        if let (Some(r), Some(t)) = (recruit, trainer) {
            let rd = catalog.get(r);
            if admitted(rd)
                && f.can_afford(rd, &promised)
                && p.emit(
                    Action::Produce {
                        actor: t.id,
                        definition: rd.id.clone(),
                    },
                    format!("Recruit {} for the hero roster", rd.name),
                )
            {
                return;
            }
        }
        if let Some(academy) = academy
            && p.build(
                academy,
                &format!(
                    "Establish {} for hero recruitment",
                    catalog.get(academy).name
                ),
                f.home,
            )
        {
            return;
        }
        // A specialised outpost is found from its placement and storage declarations.
        for &d in &defs {
            let dd = catalog.get(d);
            let (Some(near), true) = (
                &dd.placement_near,
                dd.behaviors
                    .storage
                    .as_ref()
                    .is_some_and(|s| s.dropoff == Some(true)),
            ) else {
                continue;
            };
            if buildings.iter().any(|b| b.def == d) {
                continue;
            }
            let source_def = def_of(&near.source);
            let source = f.nearest_resource(f.home, |e| {
                !e.remembered
                    && Some(e.def) == source_def
                    && !f.hostiles.iter().any(|&h| dist(f.at(h).p, e.p) < 16.0)
                    && f.geo.connected(f.home, e.p)
            });
            if let Some(source) = source {
                let s = f.at(source);
                let reason = format!("Claim observed {} with {}", f.def(s).name, dd.name);
                if p.build(d, &reason, s.p) {
                    return;
                }
            }
        }
        for b in &ready {
            let bd = f.def(b);
            let Some(upgrade) = &bd.upgrade else {
                continue;
            };
            if useful_upgrade(bd)
                && f.own_entity(b).is_some_and(|e| e.upgrade.is_none())
                && payable(&upgrade.items)
            {
                let name = get(&upgrade.target).map_or("", |t| t.name.as_str());
                if p.emit(
                    Action::Upgrade { actor: b.id },
                    format!("Advance to {name}"),
                ) {
                    return;
                }
            }
        }
        for &d in &producers {
            let dd = catalog.get(d);
            if dd.requires.as_ref().is_some_and(|r| !r.is_empty())
                && !buildings.iter().any(|b| b.def == d)
                && p.build(
                    d,
                    &format!("Unlock advanced production at {}", dd.name),
                    f.home,
                )
            {
                return;
            }
        }
        let forge = defs
            .iter()
            .copied()
            .find(|&d| catalog.get(d).behaviors.research.is_some());
        if let Some(forge) = forge
            && !buildings.iter().any(|b| b.def == forge)
            && p.build(
                forge,
                &format!(
                    "Establish {} for permanent improvements",
                    catalog.get(forge).name
                ),
                f.home,
            )
        {
            return;
        }
        let pending: Vec<&Id> = buildings
            .iter()
            .filter_map(|b| f.own_entity(b)?.research.as_ref())
            .flat_map(|r| r.queue.iter().map(|q| &q.id))
            .collect();
        for b in &ready {
            let Some(queue) = f.own_entity(b).and_then(|e| e.research.as_ref()) else {
                continue;
            };
            if !queue.queue.is_empty() {
                continue;
            }
            let outputs = f
                .def(b)
                .behaviors
                .research
                .as_ref()
                .map_or(&[][..], |r| &r.outputs);
            let mut choices: Vec<(&Id, &canopy_content::game::Research)> = outputs
                .iter()
                .filter(|id| !f.researched.contains(id) && !pending.contains(id))
                .filter_map(|id| Some((id, content.rules.research.get(id.as_str())?)))
                .filter(|(_, r)| {
                    f.available(r.requires.as_deref())
                        && payable(&r.items)
                        && r.effects.iter().any(|e| {
                            f.own
                                .iter()
                                .any(|&u| e.units.contains(catalog.id(f.at(u).def)))
                        })
                })
                .collect();
            choices.sort_by(|a, b| b.1.priority.cmp(&a.1.priority).then(a.0.cmp(b.0)));
            if let Some((id, r)) = choices.first()
                && p.emit(
                    Action::Research {
                        actor: b.id,
                        research: (*id).clone(),
                    },
                    format!("Research {} for the current force", r.name),
                )
            {
                return;
            }
        }
    }
    // A funded army must leave room for one next investment; otherwise cheap recruits
    // consume every delivery and the colony never advances.
    let mut reserve = promised.clone();
    if army.len() >= 8 && !threatened {
        let outpost = defs.iter().copied().find(|&d| {
            let dd = catalog.get(d);
            dd.placement_near.as_ref().is_some_and(|near| {
                dd.behaviors
                    .storage
                    .as_ref()
                    .is_some_and(|s| s.dropoff == Some(true))
                    && f.geo
                        .resources
                        .iter()
                        .any(|r| Some(r.definition) == def_of(&near.source))
            }) && !buildings.iter().any(|b| b.def == d)
        });
        let upgrade = ready.iter().find_map(|b| {
            let bd = f.def(b);
            let u = bd.upgrade.as_ref()?;
            (f.own_entity(b).is_some_and(|e| e.upgrade.is_none()) && useful_upgrade(bd))
                .then_some(u)
        });
        let next = outpost
            .or_else(|| {
                producers.iter().copied().find(|&d| {
                    let dd = catalog.get(d);
                    dd.requires.as_ref().is_some_and(|r| !r.is_empty())
                        && f.available(dd.requires.as_deref())
                        && !buildings.iter().any(|b| b.def == d)
                })
            })
            .or_else(|| {
                defs.iter().copied().find(|&d| {
                    catalog.get(d).behaviors.research.is_some()
                        && !buildings.iter().any(|b| b.def == d)
                })
            });
        let first = if trainer.is_some() {
            recruit.and_then(|r| catalog.get(r).creation.as_ref().map(|c| c.items()))
        } else {
            academy.and_then(|a| catalog.get(a).creation.as_ref().map(|c| c.items()))
        };
        let investment = first
            .or_else(|| outpost.and_then(|o| catalog.get(o).creation.as_ref().map(|c| c.items())))
            .or_else(|| upgrade.map(|u| u.items.as_slice()))
            .or_else(|| next.and_then(|n| catalog.get(n).creation.as_ref().map(|c| c.items())))
            .unwrap_or(&[]);
        for c in investment {
            add_max(&mut reserve, &c.item, c.amount);
        }
    }
    // Army production is independent of the worker pool.
    if (army.len() + queues.len()) < p.b.s.plan.army as usize {
        let enemies: Vec<&Known> = f
            .hostiles
            .iter()
            .map(|&i| f.at(i))
            .filter(|e| e.unit && f.def(e).behaviors.combat.is_some())
            .collect();
        let matchup = |d: &Definition| -> f64 {
            let c = d.behaviors.combat.as_ref().expect("combat");
            if enemies.is_empty() {
                return 1.0;
            }
            let n = enemies.len() as f64;
            let ranged = enemies
                .iter()
                .filter(|e| {
                    f.def(e)
                        .behaviors
                        .combat
                        .as_ref()
                        .is_some_and(|c| c.range > 3.0)
                })
                .count() as f64
                / n;
            let multipliers = content.rules.damage_multipliers.get(&c.damage_type);
            let armor = enemies
                .iter()
                .map(|e| {
                    let armor = f.def(e).body.as_ref().map(|b| &b.armor_type);
                    f64::from(
                        armor
                            .and_then(|a| multipliers.and_then(|m| m.get(a)))
                            .copied()
                            .unwrap_or(1000),
                    )
                })
                .sum::<f64>()
                / n
                / 1000.0;
            let shaped = armor
                * if c.range > 3.0 {
                    1.2 - ranged * 0.3
                } else {
                    1.0 + ranged * 0.3
                };
            shaped.clamp(0.65, 1.4)
        };
        let count = |d: Def| {
            army.iter().filter(|e| e.def == d).count() + queues.iter().filter(|&&q| q == d).count()
        };
        let mut choices: Vec<(Def, f64)> = rules
            .composition
            .iter()
            .filter_map(|c| {
                let d = def_of(&c.definition)?;
                let value =
                    (count(d) as f64 + 1.0) / (f64::from(c.weight) * matchup(catalog.get(d)));
                Some((d, value))
            })
            .collect();
        choices.sort_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        for (d, _) in choices {
            let dd = catalog.get(d);
            let producer = barracks.iter().find(|b| {
                f.def(b)
                    .behaviors
                    .production
                    .as_ref()
                    .is_some_and(|prod| prod.outputs.contains(&dd.id))
                    && f.queue(b).len() < 2
            });
            if producer.is_some()
                && f.available(dd.requires.as_deref())
                && army.len() as u32 >= a.minimum
                && !f.can_afford(dd, &reserve)
                && items(dd)
                    .iter()
                    .all(|c| f.balance(c.item.as_str()) >= c.amount || f.accepts(c.item.as_str()))
            {
                // Save for the composition's next recruit instead of spending every
                // delivery on an affordable fallback; below the army floor, replacements
                // come first.
                for c in items(dd) {
                    add_max(&mut reserve, &c.item, c.amount);
                }
                break;
            }
            if let Some(producer) = producer
                && admitted(dd)
                && f.can_afford(dd, &reserve)
                && p.emit(
                    Action::Produce {
                        actor: producer.id,
                        definition: dd.id.clone(),
                    },
                    format!("Train {} within colony supply", dd.name),
                )
            {
                return;
            }
        }
    }
    if let Some(sanctuary) = sanctuary
        && army.len() >= 5
        && !buildings.iter().any(|b| b.def == sanctuary)
        && f.can_afford(catalog.get(sanctuary), &reserve)
        && p.build(
            sanctuary,
            "Prepare hero recovery before the next fight",
            f.home,
        )
    {
        return;
    }
    let nearby_trees = f
        .resources
        .iter()
        .map(|&i| f.at(i))
        .filter(|e| {
            matches!(f.def(e).creation, Some(Creation::Plant { .. })) && dist(e.p, f.home) < 32.0
        })
        .count();
    if let Some(forest) = forest
        && workers.len() as u32 >= w.target
        && nearby_trees < 18
        && !buildings.iter().any(|b| b.def == forest)
        && f.can_afford(catalog.get(forest), &reserve)
        && p.build(forest, "Replenish accessible timber", f.home)
    {
        return;
    }
    // Add throughput only once the existing barracks queues are saturated.
    if barracks.len() < 3.min(1 + army.len() / 12)
        && barracks.iter().all(|b| f.queue(b).len() >= 2)
        && let Some(d) = producers
            .iter()
            .copied()
            .find(|&d| f.can_afford(catalog.get(d), &promised))
        && p.build(d, "Add production throughput to match income", f.home)
    {
        return;
    }
    if train_worker(p, &reserve) {
        return;
    }
    p.b.s.plan.economy = format!(
        "{} workers, {} army; {} training orders",
        workers.len(),
        army.len(),
        queues.len()
    );
    harvesting(p, &workers);
}

/// Worker assignments: release builders, replace exhausted sources, staff the most
/// needed goods, and scout for resources when none are known near home.
fn harvesting(p: &mut Planner, workers: &[&Known]) {
    let f = p.b.f;
    let catalog = f.catalog();
    let rules = f.rules;
    let tick = f.tick;
    let def_of = |id: &Id| f.find(id.as_str());
    let get = |id: &Id| def_of(id).map(|d| catalog.get(d));
    let free: Vec<&Known> = workers.iter().copied().filter(|w| f.free(w)).collect();
    let reserve = (rules.workers.reserve as usize).min(workers.len().saturating_sub(2));
    let gathering_target = |w: &Known| match f.order(w) {
        Some(Order::Gather { target }) => Some(target.0),
        _ => None,
    };
    let manual: Vec<&Known> = workers
        .iter()
        .copied()
        .filter(|w| {
            gathering_target(w).is_some()
                && f.own_unit(w).is_some_and(|u| u.work.employment.is_none())
        })
        .collect();
    let pending_move = |w: &Known| f.own_unit(w).is_some_and(|u| u.work.pending_move.is_some());
    if free.len() < reserve
        && workers.len() as u32 > rules.workers.minimum
        && let Some(release) = manual
            .iter()
            .filter(|w| f.cargo(w).is_none() && !pending_move(w))
            .max_by_key(|w| w.id)
        && p.emit(
            Action::Stop {
                actors: vec![release.id],
            },
            "Release one worker for construction",
        )
    {
        return;
    }
    let spare: Vec<&Known> = free.iter().skip(reserve).copied().collect();
    let harvests_of = |w: &Known| -> &[Id] {
        f.def(w)
            .behaviors
            .work
            .as_ref()
            .and_then(|work| work.harvests.as_deref())
            .unwrap_or(&[])
    };
    let workforce = gathering(f);
    let has_room =
        |r: &Known| workforce(r).is_none_or(|(workers, recommended)| workers < recommended);
    let hostile_near = |r: &Known, d: f64| f.hostiles.iter().any(|&h| dist(f.at(h).p, r.p) < d);
    // Gathering needs the resource in sight; the old AI also chose remembered ones, which
    // the simulation refuses, wasting the decision (docs/decisions.md D27).
    let orderable = |r: &Known| !r.remembered;
    for w in workers {
        let Some(target) = gathering_target(w) else {
            continue;
        };
        if f.job(w).is_some() || f.cargo(w).is_some() || pending_move(w) {
            continue;
        }
        if f.get(target)
            .and_then(|r| r.resource)
            .is_some_and(|r| r.amount > 0)
        {
            continue;
        }
        let harvests = harvests_of(w);
        let replacement = f.nearest_resource(w.p, |r| {
            harvests.iter().any(|id| {
                get(id)
                    .and_then(harvest_source)
                    .is_some_and(|s| Some(r.def) == def_of(s))
                    && f.accepts(id.as_str())
            }) && orderable(r)
                && has_room(r)
                && f.geo.connected(w.p, r.p)
                && !hostile_near(r, 12.0)
        });
        if let Some(r) = replacement
            && p.emit(
                Action::Gather {
                    actors: vec![w.id],
                    target: f.at(r).id,
                    append: None,
                },
                "Move an exhausted harvesting assignment to an observed resource",
            )
        {
            return;
        }
        if p.emit(
            Action::Stop { actors: vec![w.id] },
            "Release a harvesting assignment with no known source",
        ) {
            return;
        }
    }
    let mut harvests: Vec<&Id> = Vec::new();
    for w in workers {
        for id in harvests_of(w) {
            if !harvests.contains(&id) {
                harvests.push(id);
            }
        }
    }
    let source_of_target = |w: &Known| gathering_target(w).and_then(|t| f.get(t)).map(|r| r.def);
    let demand = |id: &Id| -> f64 {
        let source = get(id).and_then(harvest_source).and_then(&def_of);
        let count = workers
            .iter()
            .filter(|w| {
                f.job(w).and_then(|j| j.item.as_ref()) == Some(id)
                    || (source.is_some() && source_of_target(w) == source)
            })
            .count();
        (count as f64 + 1.0) * (1.0 + f64::from(f.balance(id.as_str())) / 100.0)
    };
    harvests.sort_by(|a, b| demand(a).total_cmp(&demand(b)).then(a.cmp(b)));
    for &item in &harvests {
        let Some(source) = get(item).and_then(harvest_source).and_then(&def_of) else {
            continue;
        };
        if !f.accepts(item.as_str()) {
            continue;
        }
        let mut worker = spare
            .iter()
            .copied()
            .find(|w| harvests_of(w).contains(item));
        // New outposts must be staffed even when every worker has a job: move one
        // empty-handed gatherer from a much better staffed source, keeping at least two
        // there. Hysteresis and a review interval prevent oscillation.
        let mut rebalanced = false;
        if worker.is_none()
            && p.b
                .s
                .inspected
                .get("economy:rebalance")
                .is_none_or(|&t| t <= tick)
        {
            let donor = harvests.iter().rev().copied().find(|&other| {
                if other == item || demand(other) <= demand(item) * 2.0 {
                    return false;
                }
                let Some(s) = get(other).and_then(harvest_source).and_then(&def_of) else {
                    return false;
                };
                manual
                    .iter()
                    .filter(|w| source_of_target(w) == Some(s))
                    .count()
                    > 2
            });
            if let Some(donor_source) = donor
                .and_then(&get)
                .and_then(harvest_source)
                .and_then(&def_of)
            {
                worker = manual.iter().copied().find(|w| {
                    f.cargo(w).is_none()
                        && !pending_move(w)
                        && harvests_of(w).contains(item)
                        && source_of_target(w) == Some(donor_source)
                });
                rebalanced = worker.is_some();
            }
        }
        let Some(worker) = worker else {
            continue;
        };
        let target = f.nearest_resource(f.home, |r| {
            r.def == source
                && orderable(r)
                && has_room(r)
                && f.geo.connected(f.home, r.p)
                && !hostile_near(r, 12.0)
        });
        if let Some(target) = target {
            let reason = if rebalanced {
                format!("Rebalance an empty-handed gatherer to {item}")
            } else {
                format!("Assign an available worker to {item}")
            };
            if p.emit(
                Action::Gather {
                    actors: vec![worker.id],
                    target: f.at(target).id,
                    append: None,
                },
                reason,
            ) {
                if rebalanced {
                    p.b.s
                        .inspected
                        .insert("economy:rebalance".into(), tick + 200);
                }
                return;
            }
        }
    }
    // On depletion, scout an authored resource site rather than knowing its amount in fog.
    if let Some(&first) = spare.first()
        && !f.resources.iter().any(|&i| dist(f.at(i).p, f.home) < 40.0)
    {
        let mut sites: Vec<&super::geography::ResourceSite> = f
            .geo
            .resources
            .iter()
            .filter(|r| {
                dist(r.point, f.home) < 80.0
                    && !f.visible(r.point)
                    && f.geo.connected(f.home, r.point)
            })
            .collect();
        sites.sort_by(|a, b| dist(a.point, f.home).total_cmp(&dist(b.point, f.home)));
        if let Some(site) = sites.first() {
            let index = f.index_of(first.id).expect("known");
            let destination = f.nearest_safe(site.point, &[index]);
            p.emit(
                Action::Move {
                    actors: vec![first.id],
                    destination: f.geo.cell_point(destination),
                    attack_move: None,
                    append: None,
                },
                "Inspect the next known resource site",
            );
        }
    }
}

/// Workers assigned to each seen harvest building against its recommendation (the
/// public `gathering` of a view). The old observation boundary dropped it for neutral
/// deposits, so the AI never respected the recommendation (docs/decisions.md D27).
fn gathering<'a>(f: &'a super::frame::Frame) -> impl Fn(&Known) -> Option<(u32, u32)> + 'a {
    let state = f.input.state;
    let catalog = f.catalog();
    let mut sites: BTreeMap<u32, u32> = BTreeMap::new();
    for &id in &f.input.economy().index.units {
        let Some(e) = state.entities.get(id) else {
            continue;
        };
        if e.hp == Some(0) {
            continue;
        }
        let Some(u) = e.unit.as_ref() else {
            continue;
        };
        let job = u
            .work
            .job
            .and_then(|j| state.economy.job(j))
            .filter(|j| j.kind == crate::economy::JobKind::Harvest)
            .and_then(|j| j.source);
        let order = match &u.order {
            Some(Order::Gather { target }) => Some(*target),
            _ => None,
        };
        if let Some(source) = job.or(order) {
            *sites.entry(source.0).or_default() += 1;
        }
    }
    move |r: &Known| {
        let d = catalog.get(r.def);
        if r.remembered || d.kind != Kind::Building {
            return None;
        }
        let h = d.harvesting.as_ref()?;
        Some((
            sites.get(&r.id).copied().unwrap_or(0),
            h.recommended_workers,
        ))
    }
}
