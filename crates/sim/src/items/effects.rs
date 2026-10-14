//! Item effects (old `itemEffects.ts`): auras, actives and consumables, permanent
//! power-ups, on-hit procs, lifesteal and melee reflection, item shields and rescue.

use super::{Bonuses, ItemHit, ItemRuntime, ItemStatus, ItemStatusKind, equipment};
use crate::{
    abilities::{Ctx, actor},
    catalog::Def,
    combat::rules::DamageType,
    entity::{Entity, EntityId},
    state::State,
    stats::Stats,
};
use canopy_content::{
    items::{ActiveTarget, AuraTarget, ItemEffect},
    placement::Owner,
};

fn allies(ctx: &Ctx, a: &Entity, b: &Entity) -> bool {
    ctx.senses.allied(a.owner, b.owner)
}

fn active_unit(e: &Entity, tick: u32) -> bool {
    actor::alive(e) && e.ready_tick <= tick && crate::spatial::present(e)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Relation {
    Caster,
    Allies,
    Enemies,
}

/// Units an item affects around `source` (old `targets`), in id order.
fn targets(
    ctx: &Ctx,
    state: &State,
    source: EntityId,
    relation: Relation,
    radius: u32,
) -> Vec<EntityId> {
    let tick = state.tick;
    let Some(s) = state.entities.get(source) else {
        return Vec::new();
    };
    let r = i64::from(radius) * 1000;
    state
        .entities
        .iter()
        .filter(|t| active_unit(t, tick))
        .filter(|t| match relation {
            Relation::Caster => t.id == source,
            Relation::Allies => actor::within(s.position, t.position, r) && allies(ctx, s, t),
            Relation::Enemies => {
                actor::within(s.position, t.position, r)
                    && t.id != source
                    && !allies(ctx, s, t)
                    && (s.owner != Owner::Nobody || t.owner != Owner::Nobody)
            }
        })
        .map(|t| t.id)
        .collect()
}

/// The runtime of a slot's item, created on first use (old `runtime`).
fn runtime<'a>(ctx: &Ctx, e: &'a mut Entity, slot: usize) -> &'a mut ItemRuntime {
    let catalog = ctx.catalog();
    let q = equipment(e);
    if q.runtime.len() != q.slots.len() {
        q.runtime.resize(q.slots.len(), None);
    }
    let effect = q.slots[slot]
        .and_then(|d| catalog.items.get(d))
        .map(|d| &d.effect);
    let charges = effect.and_then(|e| {
        e.active()
            .and_then(|a| a.charges)
            .or_else(|| rescue_of(e).map(|r| r.charges))
    });
    q.runtime[slot].get_or_insert(ItemRuntime {
        ready_tick: 0,
        hits: 0,
        charges,
    })
}

fn rescue_of(effect: &ItemEffect) -> Option<&canopy_content::items::Rescue> {
    match effect {
        ItemEffect::Equipment { rescue, .. } | ItemEffect::Consumable { rescue, .. } => {
            rescue.as_ref()
        }
    }
}

fn aura_of(effect: &ItemEffect) -> Option<&canopy_content::items::ItemAura> {
    match effect {
        ItemEffect::Equipment { aura, .. } | ItemEffect::Consumable { aura, .. } => aura.as_ref(),
    }
}

/// Item statuses expire and auras are rebuilt; pools follow the new maximums (old
/// `ItemEffects.tick`).
pub fn tick(ctx: &mut Ctx, state: &mut State) {
    let _span = tracing::trace_span!("item effects").entered();
    let tick = state.tick;
    let catalog = ctx.catalog();
    for e in state.entities.iter_mut() {
        if e.unit.is_none() {
            continue;
        }
        let living = actor::alive(e);
        if let Some(q) = &mut e.equipment
            && !q.statuses.is_empty()
        {
            q.statuses
                .retain(|s| s.kind != ItemStatusKind::Aura && s.expires > tick && living);
        }
        super::tidy(e);
    }
    let sources: Vec<EntityId> = state
        .entities
        .iter()
        .filter(|e| active_unit(e, tick))
        .map(|e| e.id)
        .collect();
    for &source in &sources {
        let items: Vec<Def> = {
            let Some(q) = state
                .entities
                .get(source)
                .and_then(|e| e.equipment.as_ref())
            else {
                continue;
            };
            let mut unique: Vec<Def> = Vec::new();
            for d in q.slots.iter().flatten() {
                if !unique.contains(d) {
                    unique.push(*d);
                }
            }
            unique
        };
        for item in items {
            let Some(aura) = catalog.items.get(item).and_then(|d| aura_of(&d.effect)) else {
                continue;
            };
            let relation = match aura.target {
                AuraTarget::Allies => Relation::Allies,
                AuraTarget::Enemies => Relation::Enemies,
            };
            for target in targets(ctx, state, source, relation, aura.radius) {
                let e = state.entities.get_mut(target).expect("target");
                let q = equipment(e);
                if !q
                    .statuses
                    .iter()
                    .any(|s| s.item == item && s.kind == ItemStatusKind::Aura)
                {
                    q.statuses.push(ItemStatus {
                        item,
                        source,
                        kind: ItemStatusKind::Aura,
                        expires: tick + 1,
                        shield: None,
                    });
                }
            }
        }
    }
    for id in sources {
        let e = state.entities.get(id).expect("unit");
        let stats = Stats::of(catalog, e, crate::stats::research(state, e.owner));
        let e = state.entities.get_mut(id).expect("unit");
        if let Some(hp) = &mut e.hp {
            *hp = (*hp).min(stats.max_hp);
        }
        if let Some(c) = &mut e.caster {
            c.mana = c.mana.min(stats.max_mana);
        }
    }
}

/// A permanent power-up (old `applyPowerup`).
pub fn power_up(ctx: &Ctx, state: &mut State, hero: EntityId, item: Def) -> bool {
    let catalog = ctx.catalog();
    let Some(e) = state.entities.get(hero) else {
        return false;
    };
    let permanent = match catalog.items.get(item).map(|d| &d.effect) {
        Some(ItemEffect::Consumable {
            permanent: Some(p), ..
        }) => p.clone(),
        _ => return false,
    };
    if !actor::alive(e) || e.progression.is_none() {
        return false;
    }
    let e = state.entities.get_mut(hero).expect("hero");
    let b = equipment(e).bonuses.get_or_insert_with(Bonuses::default);
    b.max_hp += permanent.max_hp.unwrap_or(0);
    b.damage += permanent.damage.unwrap_or(0);
    b.armor += permanent.armor.unwrap_or(0);
    b.max_mana += permanent.max_mana.unwrap_or(0);
    let e = state.entities.get(hero).expect("hero");
    let stats = Stats::of(catalog, e, crate::stats::research(state, e.owner));
    let e = state.entities.get_mut(hero).expect("hero");
    if let Some(hp) = &mut e.hp {
        *hp = stats.max_hp.min(*hp + permanent.max_hp.unwrap_or(0));
    }
    if let Some(c) = &mut e.caster {
        c.mana = stats.max_mana.min(c.mana + permanent.max_mana.unwrap_or(0));
    }
    true
}

/// Uses the item in `slot` (old `ItemEffects.use`).
pub fn use_item(
    ctx: &mut Ctx,
    state: &mut State,
    hero: EntityId,
    slot: usize,
) -> Result<(), &'static str> {
    let catalog = ctx.catalog();
    let tick = state.tick;
    let Some(e) = state.entities.get(hero) else {
        return Err("This item cannot be used");
    };
    let item = e
        .equipment
        .as_ref()
        .and_then(|q| q.slots.get(slot).copied().flatten());
    let Some(item) = item else {
        return Err("This item cannot be used");
    };
    let Some(decl) = catalog.items.get(item) else {
        return Err("This item cannot be used");
    };
    let effect = decl.effect.clone();
    let consumable = matches!(effect, ItemEffect::Consumable { .. });
    if effect.active().is_none() && !consumable {
        return Err("This item cannot be used");
    }
    if !actor::alive(e)
        || crate::abilities::statuses::stunned(catalog, e)
        || crate::abilities::statuses::item_blocked(catalog, e)
        || e.caster.as_ref().is_some_and(|c| c.pending.is_some())
    {
        return Err("Cannot use items while busy or stunned");
    }
    if let ItemEffect::Consumable {
        permanent: Some(_), ..
    } = &effect
    {
        if !power_up(ctx, state, hero, item) {
            return Err("Only a hero can use this item");
        }
        consume(state, hero, slot);
        return Ok(());
    }
    let active = effect.active().cloned();
    let (ready, charges) = {
        let e = state.entities.get_mut(hero).expect("hero");
        let r = runtime(ctx, e, slot);
        (r.ready_tick, r.charges)
    };
    if ready > tick {
        return Err("Item is cooling down");
    }
    if charges == Some(0) {
        return Err("No charges remaining");
    }
    let relation = match active.as_ref().map_or(ActiveTarget::Caster, |a| a.target) {
        ActiveTarget::Caster => Relation::Caster,
        ActiveTarget::Allies => Relation::Allies,
        ActiveTarget::Enemies => Relation::Enemies,
    };
    let recipients = targets(
        ctx,
        state,
        hero,
        relation,
        active.as_ref().map_or(0, |a| a.radius),
    );
    let heal = active
        .as_ref()
        .and_then(|a| a.heal)
        .unwrap_or(match &effect {
            ItemEffect::Consumable { heal, .. } => *heal,
            _ => 0,
        });
    let mana = active.as_ref().and_then(|a| a.mana).unwrap_or(0);
    let heal_max = active
        .as_ref()
        .and_then(|a| a.heal_max_permille)
        .unwrap_or(0);
    if recipients.is_empty() {
        return Err("No targets in range");
    }
    let only_restores = active.as_ref().is_none_or(|a| {
        a.status.is_none() && a.damage.is_none() && a.reduce_ability_cooldown_ticks.is_none()
    });
    if only_restores {
        let useful = recipients.iter().any(|&t| {
            let e = state.entities.get(t).expect("target");
            let stats = Stats::of(catalog, e, crate::stats::research(state, e.owner));
            ((heal > 0 || heal_max > 0) && e.hp.unwrap_or(0) < stats.max_hp)
                || (mana > 0 && e.caster.as_ref().is_some_and(|c| c.mana < stats.max_mana))
        });
        if !useful {
            return Err("Health and mana are already full");
        }
    }
    for &t in &recipients {
        let e = state.entities.get(t).expect("target");
        let stats = Stats::of(catalog, e, crate::stats::research(state, e.owner));
        let e = state.entities.get_mut(t).expect("target");
        if let Some(hp) = &mut e.hp {
            let bonus =
                (f64::from(stats.max_hp) * f64::from(heal_max) / 1000.0 + 0.5).floor() as u32;
            *hp = stats.max_hp.min(*hp + heal + bonus);
        }
        if let Some(c) = &mut e.caster {
            let limit = stats.max_mana.saturating_sub(c.escrow());
            c.mana = limit.min(c.mana + mana);
            if let Some(reduce) = active
                .as_ref()
                .and_then(|a| a.reduce_ability_cooldown_ticks)
            {
                for ready in c.cooldowns.values_mut() {
                    *ready = tick.max(ready.saturating_sub(reduce));
                }
            }
        }
        if let Some(status) = active.as_ref().and_then(|a| a.status.as_ref()) {
            let q = equipment(e);
            q.statuses
                .retain(|s| s.item != item || s.kind != ItemStatusKind::Active);
            q.statuses.push(ItemStatus {
                item,
                source: hero,
                kind: ItemStatusKind::Active,
                expires: tick + status.duration_ticks,
                shield: status.shield.filter(|&s| s > 0),
            });
        }
        // Damage waits in saved state for the next combat resolution.
        if let Some(damage) = active.as_ref().and_then(|a| a.damage) {
            let kind = active
                .as_ref()
                .and_then(|a| a.damage_type.as_deref())
                .map_or(ctx.senses.combat.spell.0, |t| catalog.spells.damage_type(t));
            let h = state.entities.get_mut(hero).expect("hero");
            equipment(h).hits.push(ItemHit {
                item,
                target: t,
                damage,
                damage_type: kind,
            });
        }
    }
    let e = state.entities.get_mut(hero).expect("hero");
    let r = runtime(ctx, e, slot);
    r.ready_tick = tick + active.as_ref().map_or(0, |a| a.cooldown_ticks);
    if let Some(c) = &mut r.charges {
        *c = c.saturating_sub(1);
    }
    let charges = r.charges;
    if charges == Some(0) || (consumable && charges.is_none()) {
        consume(state, hero, slot);
    }
    Ok(())
}

/// Empties a slot (old `consume`).
pub fn consume(state: &mut State, hero: EntityId, slot: usize) {
    if let Some(q) = state
        .entities
        .get_mut(hero)
        .and_then(|e| e.equipment.as_mut())
    {
        if let Some(s) = q.slots.get_mut(slot) {
            *s = None;
        }
        if let Some(r) = q.runtime.get_mut(slot) {
            *r = None;
        }
    }
}

/// Queued item damage of living units (old `drainHits`), as combat hits.
pub fn drain_hits(state: &mut State) -> Vec<crate::combat::resolve::Hit> {
    let mut hits = Vec::new();
    for e in state.entities.iter_mut() {
        let Some(q) = &mut e.equipment else {
            continue;
        };
        if q.hits.is_empty() {
            continue;
        }
        let list = std::mem::take(&mut q.hits);
        if actor::alive(e) {
            for h in list {
                hits.push(crate::combat::resolve::Hit {
                    source: e.id,
                    owner: e.owner,
                    target: h.target,
                    damage_milli: u64::from(h.damage) * 1000,
                    damage_type: DamageType(h.damage_type),
                    weapon: false,
                    ..crate::combat::resolve::Hit::default()
                });
            }
        }
        super::tidy(e);
    }
    hits
}

/// What a damaging weapon hit sets off (old `onHit`): melee reflection, lifesteal and item
/// procs. Returns the extra hits.
pub fn on_hit(
    ctx: &mut Ctx,
    state: &mut State,
    source: EntityId,
    target: EntityId,
    damage: u32,
    kind: DamageType,
) -> Vec<crate::combat::resolve::Hit> {
    let mut hits = Vec::new();
    let catalog = ctx.catalog();
    let (Some(s), Some(t)) = (state.entities.get(source), state.entities.get(target)) else {
        return hits;
    };
    if damage == 0 || allies(ctx, s, t) {
        return hits;
    }
    let stats = Stats::of(catalog, s, crate::stats::research(state, s.owner));
    let profile = crate::abilities::forms::weapon_def(catalog, s);
    let melee = catalog
        .get(profile)
        .behaviors
        .combat
        .as_ref()
        .is_some_and(|c| c.projectile.is_none() && c.shell.is_none());
    if melee {
        let reflection =
            Stats::of(catalog, t, crate::stats::research(state, t.owner)).melee_reflection_permille;
        let reflected = u64::from(damage) * u64::from(reflection) / 1000;
        if reflected > 0 {
            hits.push(crate::combat::resolve::Hit {
                source: target,
                owner: t.owner,
                target: source,
                damage_milli: reflected * 1000,
                damage_type: ctx.senses.combat.spell,
                weapon: false,
                ..crate::combat::resolve::Hit::default()
            });
        }
    }
    let healed =
        u64::from(t.hp.unwrap_or(0).min(damage)) * u64::from(stats.lifesteal_permille) / 1000;
    let target_at = t.position;
    let s = state.entities.get_mut(source).expect("source");
    if let Some(hp) = &mut s.hp {
        *hp = stats.max_hp.min(*hp + healed as u32);
    }
    let slots: Vec<Option<Def>> = s
        .equipment
        .as_ref()
        .map_or_else(Vec::new, |q| q.slots.clone());
    let mut seen: Vec<Def> = Vec::new();
    for (slot, item) in slots.into_iter().enumerate() {
        let Some(item) = item else {
            continue;
        };
        if seen.contains(&item) {
            continue;
        }
        seen.push(item);
        let Some(proc) = catalog
            .items
            .get(item)
            .and_then(|d| d.effect.on_hit().cloned())
        else {
            continue;
        };
        let e = state.entities.get_mut(source).expect("source");
        let r = runtime(ctx, e, slot);
        r.hits = (r.hits + 1) % proc.every.max(1);
        if r.hits != 0 {
            continue;
        }
        let r2 = i64::from(proc.radius) * 1000;
        let mut candidates: Vec<(i64, EntityId)> =
            targets(ctx, state, source, Relation::Enemies, 32)
                .into_iter()
                .filter_map(|id| {
                    let p = state.entities.get(id)?.position;
                    let d = actor::distance2(p, target_at);
                    (d <= r2 * r2).then_some((d, id))
                })
                .collect();
        candidates.sort();
        let kind = if proc.damage_type.is_empty() {
            kind
        } else {
            DamageType(catalog.spells.damage_type(&proc.damage_type))
        };
        let owner = state
            .entities
            .get(source)
            .map_or(Owner::Nobody, |e| e.owner);
        let damage_milli = u64::from(proc.damage.unwrap_or(0)) * 1000
            + stats.damage_milli * u64::from(proc.attack_damage_permille.unwrap_or(0)) / 1000;
        for (_, id) in candidates.into_iter().take(proc.targets as usize) {
            hits.push(crate::combat::resolve::Hit {
                source,
                owner,
                target: id,
                damage_milli,
                damage_type: kind,
                weapon: false,
                ..crate::combat::resolve::Hit::default()
            });
        }
    }
    hits
}

/// Invulnerability and item shields after armour (old `absorb`).
pub fn absorb(ctx: &crate::abilities::Ctx, e: &mut Entity, damage: u32) -> u32 {
    absorb_with(ctx.catalog(), e, damage)
}

pub fn absorb_with(catalog: &crate::catalog::Catalog, e: &mut Entity, mut damage: u32) -> u32 {
    if e.equipment.is_none() && e.statuses.is_empty() {
        return damage;
    }
    if crate::abilities::statuses::invulnerable(catalog, e) {
        return 0;
    }
    if let Some(q) = &mut e.equipment {
        for s in &mut q.statuses {
            if let Some(shield) = &mut s.shield
                && *shield > 0
            {
                let n = damage.min(*shield);
                *shield -= n;
                damage -= n;
            }
        }
    }
    damage
}

/// Saves a unit at zero health with a rescue item (old `rescue`).
pub fn rescue(ctx: &mut Ctx, state: &mut State, id: EntityId) -> bool {
    let catalog = ctx.catalog();
    let tick = state.tick;
    let Some(slots) = state
        .entities
        .get(id)
        .and_then(|e| e.equipment.as_ref())
        .map(|q| q.slots.clone())
    else {
        return false;
    };
    for (slot, item) in slots.into_iter().enumerate() {
        let Some(item) = item else {
            continue;
        };
        let Some(rescue) = catalog
            .items
            .get(item)
            .and_then(|d| rescue_of(&d.effect).cloned())
        else {
            continue;
        };
        let e = state.entities.get_mut(id).expect("unit");
        let r = runtime(ctx, e, slot);
        if r.charges.unwrap_or(0) == 0 {
            continue;
        }
        let left = r.charges.map(|c| c - 1);
        r.charges = left;
        equipment(e).statuses.push(ItemStatus {
            item,
            source: id,
            kind: ItemStatusKind::Rescue,
            expires: tick + rescue.invulnerable_ticks,
            shield: None,
        });
        if left == Some(0) {
            consume(state, id, slot);
        }
        let e = state.entities.get(id).expect("unit");
        let max = Stats::of(catalog, e, crate::stats::research(state, e.owner)).max_hp;
        let hp =
            (f64::from(max) * f64::from(rescue.heal_max_permille) / 1000.0 + 0.5).floor() as u32;
        state.entities.get_mut(id).expect("unit").hp = Some(hp.max(1));
        return true;
    }
    false
}
