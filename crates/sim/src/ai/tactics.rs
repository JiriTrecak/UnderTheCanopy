//! Heroes, items and local reactions (old `heroActions` and `reactions` in
//! `ai/tactics.ts`; ability decisions are in [`super::powers`]).

use super::{
    Beat,
    frame::{Frame, Known},
    geography::{Pt, dist},
    powers,
    state::WorkerReturn,
};
use crate::{action::Action, catalog::Def, orders::Order};
use canopy_content::items::ItemEffect;

/// How much an item is worth carrying (old `itemValue`).
pub fn item_value(f: &Frame, def: Option<Def>) -> f64 {
    let Some(d) = def.map(|d| f.definition(d)) else {
        return 0.0;
    };
    if let Some(tier) = d.item_tier.filter(|&t| t > 0) {
        return f64::from(tier) * 250.0;
    }
    match &d.item_effect {
        None => 0.0,
        Some(ItemEffect::Equipment {
            damage,
            armor,
            max_hp,
            ..
        }) => f64::from(*damage) * 8.0 + f64::from(*armor) * 20.0 + f64::from(*max_hp),
        Some(ItemEffect::Consumable { heal, .. }) => f64::from(*heal) * 0.5,
    }
}

/// Item use, pickups and swaps for each hero (old `heroActions`).
pub fn hero_actions(b: &mut Beat) {
    let f = b.f;
    let tick = f.tick;
    let heroes: Vec<&Known> = f
        .army
        .iter()
        .map(|&i| f.at(i))
        .filter(|e| f.def(e).is_hero())
        .collect();
    for hero in heroes {
        let stats = f.stats(hero);
        let max_hp = f.max_hp(hero);
        let missing = f64::from(max_hp) - f64::from(hero.hp.unwrap_or(0));
        let gear = powers::gear(f, hero);
        let fighting = |radius: f64| {
            f.hostiles
                .iter()
                .any(|&i| f.at(i).unit && dist(hero.p, f.at(i).p) < radius)
        };
        let healing = gear.as_ref().and_then(|gear| {
            gear.slots.iter().enumerate().position(|(slot, item)| {
                let Some(effect) = item.and_then(|d| f.definition(d).item_effect.as_ref()) else {
                    return false;
                };
                if gear.ready.get(slot).copied().unwrap_or(0) > tick {
                    return false;
                }
                let active = effect.active();
                let heal = active.and_then(|a| a.heal).unwrap_or(match effect {
                    ItemEffect::Consumable { heal, .. } => *heal,
                    ItemEffect::Equipment { .. } => 0,
                });
                let max_mana = stats.map_or(0, |s| s.max_mana);
                let missing_mana = f64::from(max_mana) - f64::from(gear.mana);
                let near = fighting(active.map_or(8.0, |a| {
                    if a.radius > 0 {
                        f64::from(a.radius)
                    } else {
                        8.0
                    }
                }));
                if heal > 0 && missing >= (f64::from(heal) * 0.65).min(f64::from(max_hp) * 0.35) {
                    return true;
                }
                if let Some(mana) = active.and_then(|a| a.mana).filter(|&m| m > 0)
                    && missing_mana >= f64::from(mana) * 0.65
                {
                    return true;
                }
                if active
                    .and_then(|a| a.heal_max_permille)
                    .is_some_and(|p| p > 0)
                    && missing >= f64::from(max_hp) * 0.3
                {
                    return true;
                }
                if active
                    .and_then(|a| a.reduce_ability_cooldown_ticks)
                    .is_some_and(|t| t > 0)
                {
                    return near && gear.cooldowns.iter().any(|&t| t.saturating_sub(tick) > 200);
                }
                active.is_some_and(|a| {
                    near && (a.damage.is_some_and(|d| d > 0) || a.status.is_some())
                })
            })
        });
        if let Some(slot) = healing
            && b.s.damage.get(&hero.id).is_none_or(|d| d.react_at <= tick)
            && b.emit(
                Action::UseItem {
                    actor: hero.id,
                    slot: slot as u32,
                },
                "Use healing before risking the veteran",
            )
        {
            continue;
        }
        if f.casting(hero) || f.stunned(hero) {
            continue;
        }
        if fighting(18.0) || powers::pickup_target(f, hero).is_some() {
            continue;
        }
        let mut items: Vec<&Known> = f
            .all
            .iter()
            .filter(|e| {
                e.item
                    && !e.remembered
                    && dist(e.p, hero.p) <= 12.0
                    && b.s
                        .inspected
                        .get(&format!("item:{}", e.id))
                        .is_none_or(|&t| t <= tick)
            })
            .collect();
        items.sort_by(|x, y| {
            item_value(f, Some(y.def))
                .total_cmp(&item_value(f, Some(x.def)))
                .then(dist(x.p, hero.p).total_cmp(&dist(y.p, hero.p)))
                .then(x.id.cmp(&y.id))
        });
        let (Some(target), Some(gear)) = (items.first(), gear) else {
            continue;
        };
        if gear.slots.contains(&None) {
            b.emit(
                Action::Pickup {
                    actor: hero.id,
                    target: target.id,
                    append: None,
                },
                "Collect the nearby camp reward",
            );
            continue;
        }
        let weakest = gear
            .slots
            .iter()
            .enumerate()
            .map(|(slot, &item)| (slot, item_value(f, item)))
            .min_by(|a, b| a.1.total_cmp(&b.1).then(a.0.cmp(&b.0)));
        let Some((slot, value)) = weakest else {
            continue;
        };
        if item_value(f, Some(target.def)) > value + 10.0
            && b.emit(
                Action::DropItem {
                    actor: hero.id,
                    slot: slot as u32,
                },
                "Make room for a stronger item",
            )
        {
            // The dropped item appears next decision; suppress weaker pickups briefly.
            b.s.inspected
                .insert(format!("replacement:{}", hero.id), tick + 400);
        } else {
            b.s.inspected
                .insert(format!("item:{}", target.id), tick + 1200);
        }
    }
}

/// Workers pull back from attackers and return to work; soldiers retreat when badly
/// wounded, keep ranged spacing and engage the best target near their operation (old
/// `reactions`).
pub fn reactions(b: &mut Beat) {
    let f = b.f;
    let tick = f.tick;
    let rules = f.rules;
    let combat_of = |e: &Known| f.def(e).behaviors.combat.as_ref();
    for &wi in &f.workers {
        let worker = f.at(wi);
        let danger: Vec<&Known> = f
            .hostiles
            .iter()
            .map(|&i| f.at(i))
            .filter(|e| combat_of(e).is_some_and(|c| dist(e.p, worker.p) < c.range + 5.0))
            .collect();
        let memory = b.s.damage.get(&worker.id).copied();
        if let Some(threat) = danger.first()
            && let Some(memory) = memory
            && tick >= memory.react_at
            && memory.hp < f.max_hp(worker)
        {
            if let Some(Order::Gather { target }) = f.order(worker) {
                b.s.worker_returns.insert(
                    worker.id,
                    WorkerReturn {
                        target: target.0,
                        until: tick + 240,
                    },
                );
            }
            let v = Pt::new(
                f.home.x + (f.home.x - threat.p.x) * 0.4,
                f.home.y + (f.home.y - threat.p.y) * 0.4,
            );
            let destination = f.nearest_safe(v, &[wi]);
            b.emit(
                Action::Move {
                    actors: vec![worker.id],
                    destination: f.geo.cell_point(destination),
                    attack_move: None,
                    append: None,
                },
                "Pull the threatened worker back through the base",
            );
        } else if let Some(back) = b.s.worker_returns.get(&worker.id).copied()
            && tick >= back.until
            && danger.is_empty()
            && f.get(back.target)
                .is_some_and(|r| !r.remembered && r.resource.is_some_and(|r| r.amount > 0))
            && b.emit(
                Action::Gather {
                    actors: vec![worker.id],
                    target: back.target,
                    append: None,
                },
                "Return the evacuated worker to harvesting",
            )
        {
            b.s.worker_returns.remove(&worker.id);
        }
    }
    for &si in &f.army {
        let soldier = f.at(si);
        if Some(soldier.id) == b.s.scout || f.stunned(soldier) || f.casting(soldier) {
            continue;
        }
        let Some(combat) = combat_of(soldier) else {
            continue;
        };
        let local: Vec<&Known> = f
            .hostiles
            .iter()
            .map(|&i| f.at(i))
            .filter(|e| combat_of(e).is_some() && dist(e.p, soldier.p) < 14.0)
            .collect();
        let health = f64::from(soldier.hp.unwrap_or(0)) / f64::from(f.max_hp(soldier));
        let memory = b.s.damage.get(&soldier.id).copied();
        if !local.is_empty()
            && health * 1000.0 < f64::from(rules.army.retreat_health_permille)
            && memory.is_some_and(|m| tick >= m.react_at)
        {
            let destination = f.nearest_safe(f.home, &[si]);
            b.emit(
                Action::Move {
                    actors: vec![soldier.id],
                    destination: f.geo.cell_point(destination),
                    attack_move: None,
                    append: None,
                },
                "Preserve a badly wounded fighter",
            );
            continue;
        }
        if local.is_empty() {
            continue;
        }
        // Fight near the assigned operation; never chase bait across the map.
        let center =
            b.s.mission
                .as_ref()
                .map_or(f.home, |m| f.geo.from_place(&m.point));
        let pursuit = f64::from(rules.army.pursuit_radius);
        let score = |e: &Known| {
            dist(soldier.p, e.p) * 2.0
                + f64::from(e.hp.unwrap_or(0)) / f.damage(e, soldier).max(1.0)
                - if f.def(e).is_hero() { 10.0 } else { 0.0 }
                - combat_of(e).map_or(0.0, |c| c.range)
        };
        let target = local
            .iter()
            .filter(|e| dist(e.p, center) <= pursuit || dist(e.p, f.home) < 20.0)
            .min_by(|a, b| score(a).total_cmp(&score(b)).then(a.id.cmp(&b.id)));
        let Some(target) = target else {
            continue;
        };
        let seen = b.s.contacts.get(&target.id).copied().unwrap_or(tick);
        if tick < seen + rules.reaction_ticks {
            continue;
        }
        let profiles = f.input.profiles();
        let reach =
            profiles.get(soldier.def).body.radius + profiles.get(target.def).body.radius + 3.0;
        let target_combat = combat_of(target).expect("combat");
        // Limited archer spacing: only after a shot and only when a melee threat is close.
        // The old check compared the remaining cooldown with the match tick, so it never
        // held after the first seconds (docs/decisions.md D26).
        let reloading = f.own_unit(soldier).is_some_and(|u| u.combat.cooldown > 0);
        if combat.projectile.is_some()
            && reloading
            && dist(target.p, soldier.p) < reach
            && target_combat.projectile.is_none()
            && target_combat.shell.is_none()
        {
            let d = dist(soldier.p, target.p).max(1.0);
            let p = Pt::new(
                soldier.p.x + (soldier.p.x - target.p.x) / d * 3.0,
                soldier.p.y + (soldier.p.y - target.p.y) / d * 3.0,
            );
            if dist(p, center) <= pursuit {
                let destination = f.nearest_safe(p, &[si]);
                b.emit(
                    Action::Move {
                        actors: vec![soldier.id],
                        destination: f.geo.cell_point(destination),
                        attack_move: None,
                        append: None,
                    },
                    "Create a little space for the ranged line",
                );
            }
        } else if !matches!(f.order(soldier), Some(Order::Attack { target: t, .. }) if t.0 == target.id)
        {
            b.emit(
                Action::Attack {
                    actors: vec![soldier.id],
                    target: target.id,
                    force: None,
                    append: None,
                },
                "Engage a visible target inside the operation area",
            );
        }
    }
}
