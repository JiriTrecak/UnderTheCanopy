//! What statuses mean for their holder (old `statuses.ts` queries, `controlPolicy.ts`,
//! `damagePolicy.ts`, `concealment.ts`, `eligibility.ts` `spellImmunity`): controls and
//! immunities, stat modifiers, damage taken and absorbed, concealment and detection.
//!
//! Everything is read from the entity's saved statuses and items through the compiled
//! library, so there is no cache to keep in step; units without statuses or items take a
//! fast path.

use super::{
    library::{Key, Modifiers, StatusDecl, control},
    state::Status,
};
use crate::{catalog::Catalog, entity::Entity};
use canopy_content::abilities::{Immunity, SpellImmunity, StackScope};

fn decl<'a>(catalog: &'a Catalog, s: &Status) -> &'a StatusDecl {
    catalog.spells.status(s.key)
}

/// Control kinds the unit is immune to (old `controlImmunities`, raw grants only).
pub fn control_immunities(catalog: &Catalog, e: &Entity) -> u8 {
    if e.statuses.is_empty() && e.equipment.is_none() {
        return 0;
    }
    if crate::items::any_modifier(catalog, e, |m| m.control_immune) {
        return control::ALL;
    }
    e.statuses
        .iter()
        .fold(0, |bits, s| bits | decl(catalog, s).control_immunity)
}

pub fn control_immune(catalog: &Catalog, e: &Entity, kind: u8) -> bool {
    control_immunities(catalog, e) & kind != 0
}

/// Whether a spell status imposes a control kind the unit is not immune to (old
/// `spellControl`).
pub fn spell_control(catalog: &Catalog, e: &Entity, kind: u8) -> bool {
    if e.statuses.is_empty() {
        return false;
    }
    spell_control_with(catalog, e, control_immunities(catalog, e), kind)
}

/// [`spell_control`] with the unit's [`control_immunities`] already known.
pub fn spell_control_with(catalog: &Catalog, e: &Entity, immune: u8, kind: u8) -> bool {
    if e.statuses.is_empty() || immune & kind != 0 {
        return false;
    }
    e.statuses
        .iter()
        .any(|s| s.blocked & kind == 0 && flag(decl(catalog, s), kind))
}

/// Whether a declaration imposes a flag control kind.
pub fn flag(d: &StatusDecl, kind: u8) -> bool {
    match kind {
        control::STUN => d.stun,
        control::SILENCE => d.silence,
        control::DISARM => d.disarm,
        control::ITEM_BLOCKED => d.item_blocked,
        control::TAUNT => d.taunt,
        _ => false,
    }
}

pub fn stunned(catalog: &Catalog, e: &Entity) -> bool {
    spell_control(catalog, e, control::STUN)
}

pub fn silenced(catalog: &Catalog, e: &Entity) -> bool {
    spell_control(catalog, e, control::SILENCE)
}

pub fn disarmed(catalog: &Catalog, e: &Entity) -> bool {
    spell_control(catalog, e, control::DISARM)
}

pub fn item_blocked(catalog: &Catalog, e: &Entity) -> bool {
    spell_control(catalog, e, control::ITEM_BLOCKED)
}

/// Rooted by an item or status and not immune (old `itemFlag(rooted) && !controlImmune`).
pub fn rooted(catalog: &Catalog, e: &Entity) -> bool {
    if e.statuses.is_empty() && e.equipment.is_none() {
        return false;
    }
    let mut rooted = false;
    for_each_modifier(catalog, e, &mut |m| rooted |= m.rooted);
    rooted && !control_immune(catalog, e, control::ROOT)
}

/// Invulnerable by an item or status modifier (old `itemFlag(invulnerable)`).
pub fn invulnerable(catalog: &Catalog, e: &Entity) -> bool {
    if e.statuses.is_empty() && e.equipment.is_none() {
        return false;
    }
    let mut on = false;
    for_each_modifier(catalog, e, &mut |m| on |= m.invulnerable);
    on
}

/// Ethereal: no weapon attacks, no non-spell damage (old `isEthereal`).
pub fn ethereal(catalog: &Catalog, e: &Entity) -> bool {
    e.statuses.iter().any(|s| decl(catalog, s).ethereal)
}

/// Every modifier of the unit in the old order: spell statuses (per-source stacks merged at
/// the end), then equipment and item statuses (old `itemModifiers`).
pub fn for_each_modifier(catalog: &Catalog, e: &Entity, f: &mut dyn FnMut(&Modifiers)) {
    spell_modifiers(catalog, e, f);
    crate::items::non_spell_modifiers(catalog, e, f);
}

/// Spell status modifiers (old `spellModifiers`).
pub fn spell_modifiers(catalog: &Catalog, e: &Entity, f: &mut dyn FnMut(&Modifiers)) {
    if e.statuses.is_empty() {
        return;
    }
    spell_modifiers_with(catalog, e, control_immunities(catalog, e), f);
}

/// [`spell_modifiers`] with the unit's [`control_immunities`] already known.
pub fn spell_modifiers_with(
    catalog: &Catalog,
    e: &Entity,
    immune: u8,
    f: &mut dyn FnMut(&Modifiers),
) {
    if e.statuses.is_empty() {
        return;
    }
    let mut groups: Vec<(super::library::StatusKey, Modifiers)> = Vec::new();
    for s in &e.statuses {
        let d = decl(catalog, s);
        let mut m = d.rank(s.rank).modifiers;
        if d.stacking.is_some() {
            m = m.times(s.stacks.unwrap_or(1));
        }
        let blocked = immune | s.blocked;
        if blocked & control::ROOT != 0 {
            m.clear(Key::Rooted);
        }
        if blocked & control::MOVE_SLOW != 0 && m.move_speed_permille < 0 {
            m.clear(Key::MoveSpeedPermille);
        }
        if blocked & control::ATTACK_SLOW != 0 && m.attack_speed_permille < 0 {
            m.clear(Key::AttackSpeedPermille);
        }
        if matches!(d.stacking, Some((_, StackScope::Source))) {
            match groups.iter_mut().find(|(k, _)| *k == s.key) {
                Some((_, group)) => group.strongest(&m),
                None => {
                    let mut group = Modifiers::default();
                    group.strongest(&m);
                    groups.push((s.key, group));
                }
            }
            continue;
        }
        f(&m);
    }
    for (_, m) in &groups {
        f(m);
    }
}

/// The strongest spell immunity (old `spellImmunity`).
pub fn spell_immunity(catalog: &Catalog, e: &Entity, tick: Option<u32>) -> Option<SpellImmunity> {
    let mut result = None;
    for s in &e.statuses {
        if tick.is_some_and(|t| s.expires <= t) {
            continue;
        }
        match decl(catalog, s).source.spell_immunity {
            Some(SpellImmunity::All) => return Some(SpellImmunity::All),
            Some(p) => result = Some(p),
            None => {}
        }
    }
    result
}

/// Whether a spell may affect a unit with `immunity` at `relation` (old `acceptsSpell`).
pub fn accepts(pierces: bool, immunity: Option<SpellImmunity>, enemy: bool) -> bool {
    pierces
        || match immunity {
            None => true,
            Some(SpellImmunity::Hostile) => !enemy,
            Some(SpellImmunity::All) => false,
        }
}

/// Damage taken of `kind` in permille: 0 when immune (old `spellDamageMultiplier`).
/// `spell` is the spell damage type index.
pub fn damage_multiplier(catalog: &Catalog, e: &Entity, kind: u8, spell: u8) -> f64 {
    if e.statuses.is_empty() {
        return 1000.0;
    }
    let mut low: u32 = 1000;
    let mut high: u32 = 1000;
    for s in &e.statuses {
        let d = decl(catalog, s);
        let immune = match d.source.immunity {
            Some(Immunity::Spell) => kind == spell,
            Some(Immunity::Physical) => kind != spell,
            None => false,
        };
        if immune || (kind != spell && d.ethereal) {
            return 0.0;
        }
        for &(t, v) in &d.rank(s.rank).damage_taken {
            if t == kind {
                low = low.min(v);
                high = high.max(v);
            }
        }
    }
    f64::from(low) * f64::from(high) / 1000.0
}

/// Spell immunity and shields after armour and item shields (old `spellAbsorb`).
pub fn absorb(catalog: &Catalog, e: &mut Entity, mut damage: u32, kind: u8, spell: u8) -> u32 {
    if e.statuses.is_empty() {
        return damage;
    }
    let immune = e
        .statuses
        .iter()
        .any(|s| match decl(catalog, s).source.immunity {
            Some(Immunity::Spell) => kind == spell,
            Some(Immunity::Physical) => kind != spell,
            None => false,
        });
    if immune {
        return 0;
    }
    for i in 0..e.statuses.len() {
        let Some(shield) = decl(catalog, &e.statuses[i]).source.mana_shield.clone() else {
            continue;
        };
        let Some(caster) = e.caster.as_mut() else {
            continue;
        };
        let absorbed = (f64::from(damage) * f64::from(shield.absorb_permille) / 1000.0)
            .floor()
            .min((f64::from(caster.mana) * shield.damage_per_mana).floor());
        caster.mana = caster
            .mana
            .saturating_sub((absorbed / shield.damage_per_mana).ceil() as u32);
        damage -= absorbed as u32;
    }
    for s in &mut e.statuses {
        if let Some(shield) = &mut s.shield
            && *shield > 0
        {
            let n = damage.min(*shield);
            *shield -= n;
            damage -= n;
        }
    }
    let depleted = |s: &Status| s.shield == Some(0) && decl(catalog, s).remove_when_depleted;
    if e.statuses.iter().any(depleted) {
        e.statuses.retain(|s| !depleted(s));
    }
    damage
}

/// Any damage ends `breakOnDamage` statuses (old `wakeOnDamage`).
pub fn wake(catalog: &Catalog, e: &mut Entity, damage: u32) {
    if damage == 0 || e.statuses.is_empty() {
        return;
    }
    e.statuses.retain(|s| !decl(catalog, s).break_on_damage);
}

/// Concealed past its fade (old `spellHidden`).
pub fn hidden(catalog: &Catalog, e: &Entity, tick: u32) -> bool {
    e.statuses.iter().any(|s| {
        s.expires > tick
            && decl(catalog, s)
                .source
                .concealment
                .as_ref()
                .is_some_and(|c| tick >= s.started + c.fade_ticks)
    })
}

/// Radius within which it reveals concealed units (old `spellDetection`).
pub fn detection(catalog: &Catalog, e: &Entity, tick: u32) -> u32 {
    e.statuses
        .iter()
        .filter(|s| s.expires > tick)
        .map(|s| decl(catalog, s).rank(s.rank).detection)
        .max()
        .unwrap_or(0)
}

/// Cosmetic concealment fade, permille of full opacity (old `concealmentOpacity`).
pub fn concealment_opacity(catalog: &Catalog, e: &Entity, tick: u32) -> f64 {
    let mut opacity: f64 = 1.0;
    for s in e.statuses.iter().filter(|s| s.expires > tick) {
        if let Some(c) = &decl(catalog, s).source.concealment {
            let progress = if c.fade_ticks == 0 {
                1.0
            } else {
                (f64::from(tick.saturating_sub(s.started)) / f64::from(c.fade_ticks))
                    .clamp(0.0, 1.0)
            };
            opacity = opacity.min(1.0 - 0.72 * progress);
        }
    }
    opacity
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    Attack,
    Cast,
}

/// Ends concealment that breaks on `action` and returns the revealing attack bonus (old
/// `revealForAction`).
pub fn reveal(catalog: &Catalog, e: &mut Entity, tick: u32, action: Action) -> u32 {
    if e.statuses.is_empty() {
        return 0;
    }
    let mut bonus = 0;
    let mut broken = false;
    for s in e.statuses.iter().filter(|s| s.expires > tick) {
        let d = decl(catalog, s);
        let Some(c) = &d.source.concealment else {
            continue;
        };
        let breaks = match action {
            Action::Attack => c.break_on_attack,
            Action::Cast => c.break_on_cast,
        };
        if !breaks {
            continue;
        }
        broken = true;
        if action == Action::Attack && tick >= s.started + c.fade_ticks {
            bonus = bonus.max(d.rank(s.rank).concealment_bonus);
        }
    }
    if broken {
        e.statuses.retain(|s| {
            let Some(c) = &decl(catalog, s).source.concealment else {
                return true;
            };
            let breaks = match action {
                Action::Attack => c.break_on_attack,
                Action::Cast => c.break_on_cast,
            };
            !(breaks && s.expires > tick)
        });
    }
    bonus
}

/// The newest form status (old `activeSpellFormEntry`): latest start, then cast, then
/// ability and status order.
pub fn active_form<'a>(
    catalog: &'a Catalog,
    e: &'a Entity,
) -> Option<(&'a Status, &'a canopy_content::abilities::Form)> {
    let mut latest: Option<(&Status, &canopy_content::abilities::Form)> = None;
    for s in &e.statuses {
        let Some(form) = decl(catalog, s).source.form.as_ref() else {
            continue;
        };
        let newer = match latest {
            None => true,
            Some((p, _)) => {
                s.started > p.started
                    || (s.started == p.started
                        && (s.cast > p.cast
                            || (s.cast == p.cast
                                && (ability_id(catalog, s) < ability_id(catalog, p)
                                    || (s.key.ability == p.key.ability
                                        && decl(catalog, s).name < decl(catalog, p).name)))))
            }
        };
        if newer {
            latest = Some((s, form));
        }
    }
    latest
}

fn ability_id<'a>(catalog: &'a Catalog, s: &Status) -> &'a str {
    catalog.content.abilities.abilities[usize::from(s.key.ability.0)]
        .id
        .as_str()
}

/// Bonus of the source's marks on `target` for its next primary weapon attack (old
/// `markedWeaponBonus`).
pub fn marked_bonus(catalog: &Catalog, source: &Entity, target: &Entity, tick: u32) -> u32 {
    let mut bonus = 0;
    for s in &target.statuses {
        if s.source != source.id || s.owner != source.owner || s.expires <= tick {
            continue;
        }
        if let Some(b) = decl(catalog, s).rank(s.rank).source_attack_bonus {
            bonus += b * s.stacks.unwrap_or(1);
        }
    }
    bonus
}
