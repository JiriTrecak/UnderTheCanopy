//! The published ability library compiled once for the simulation (old `ContentRegistry`
//! ability lookups and `context.ts` `formDefinition` caches): abilities and statuses are
//! addressed by small indexes, rank parameters of the hot status data are resolved per rank,
//! and every caster definition's bindings are resolved to abilities.
//!
//! Everything here is a pure function of the content, so it lives in the shared catalog.

use crate::catalog::Def;
use canopy_content::{
    Content,
    abilities::{
        Ability, AbilityAi, Activation, Amount, Control, ControlKind, Effect, Polarity,
        RankParameters, RankedModifier, RankedReference, ShieldDepletion, StackScope, StatusEffect,
        StatusLifetime, TriggerEvent,
    },
    items::ItemModifiers,
};
use serde::{Deserialize, Serialize};

/// Index of an ability in the published library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct AbilityId(pub u16);

/// A status of an ability: its index among the ability's distinct status ids, in
/// declaration order (templates, release program, triggers).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct StatusKey {
    pub ability: AbilityId,
    pub status: u8,
}

/// Control kinds as bits ([`ControlKind`] order).
pub mod control {
    pub const STUN: u8 = 1;
    pub const ROOT: u8 = 2;
    pub const SILENCE: u8 = 4;
    pub const DISARM: u8 = 8;
    pub const ITEM_BLOCKED: u8 = 16;
    pub const MOVE_SLOW: u8 = 32;
    pub const ATTACK_SLOW: u8 = 64;
    pub const TAUNT: u8 = 128;
    pub const ALL: u8 = 255;
}

pub fn control_bit(kind: ControlKind) -> u8 {
    match kind {
        ControlKind::Stun => control::STUN,
        ControlKind::Root => control::ROOT,
        ControlKind::Silence => control::SILENCE,
        ControlKind::Disarm => control::DISARM,
        ControlKind::ItemBlocked => control::ITEM_BLOCKED,
        ControlKind::MoveSlow => control::MOVE_SLOW,
        ControlKind::AttackSlow => control::ATTACK_SLOW,
        ControlKind::Taunt => control::TAUNT,
    }
}

/// Stat modifiers of an item, aura or status (old `ItemModifiers`), with which keys are
/// present: stacking statuses keep the strongest value per present key.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Modifiers {
    pub max_hp: i64,
    pub max_mana: i64,
    pub damage: i64,
    pub armor: i64,
    pub health_regen: f64,
    pub mana_regen: f64,
    pub damage_permille: i64,
    pub attack_speed_permille: i64,
    pub move_speed_permille: i64,
    pub cooldown_reduction_permille: i64,
    pub lifesteal_permille: i64,
    pub melee_reflection_permille: i64,
    pub rooted: bool,
    pub control_immune: bool,
    pub invulnerable: bool,
    /// Bit per key ([`Key`] order) that the declaration names.
    pub present: u16,
}

/// Modifier keys, as bits of [`Modifiers::present`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Key {
    MaxHp,
    MaxMana,
    Damage,
    Armor,
    HealthRegen,
    ManaRegen,
    DamagePermille,
    AttackSpeedPermille,
    MoveSpeedPermille,
    CooldownReductionPermille,
    LifestealPermille,
    MeleeReflectionPermille,
    Rooted,
    ControlImmune,
    Invulnerable,
}

const NUMERIC: [Key; 12] = [
    Key::MaxHp,
    Key::MaxMana,
    Key::Damage,
    Key::Armor,
    Key::HealthRegen,
    Key::ManaRegen,
    Key::DamagePermille,
    Key::AttackSpeedPermille,
    Key::MoveSpeedPermille,
    Key::CooldownReductionPermille,
    Key::LifestealPermille,
    Key::MeleeReflectionPermille,
];

impl Modifiers {
    pub fn has(&self, key: Key) -> bool {
        self.present & (1 << key as u8) != 0
    }

    fn mark(&mut self, key: Key) {
        self.present |= 1 << key as u8;
    }

    pub fn clear(&mut self, key: Key) {
        self.present &= !(1 << key as u8);
        match key {
            Key::Rooted => self.rooted = false,
            Key::MoveSpeedPermille => self.move_speed_permille = 0,
            Key::AttackSpeedPermille => self.attack_speed_permille = 0,
            _ => self.set(key, 0.0),
        }
    }

    pub fn of(m: &ItemModifiers) -> Self {
        let mut out = Self::default();
        let mut set = |key: Key, value: Option<f64>| {
            if let Some(v) = value {
                out.set(key, v);
            }
        };
        set(Key::MaxHp, m.max_hp.map(f64::from));
        set(Key::MaxMana, m.max_mana.map(f64::from));
        set(Key::Damage, m.damage.map(f64::from));
        set(Key::Armor, m.armor.map(f64::from));
        set(Key::HealthRegen, m.health_regen_per_second);
        set(Key::ManaRegen, m.mana_regen_per_second);
        set(Key::DamagePermille, m.damage_permille.map(f64::from));
        set(
            Key::AttackSpeedPermille,
            m.attack_speed_permille.map(f64::from),
        );
        set(Key::MoveSpeedPermille, m.move_speed_permille.map(f64::from));
        set(
            Key::CooldownReductionPermille,
            m.cooldown_reduction_permille.map(f64::from),
        );
        set(Key::LifestealPermille, m.lifesteal_permille.map(f64::from));
        set(
            Key::MeleeReflectionPermille,
            m.melee_reflection_permille.map(f64::from),
        );
        if let Some(v) = m.rooted {
            out.rooted = v;
            out.mark(Key::Rooted);
        }
        if let Some(v) = m.control_immune {
            out.control_immune = v;
            out.mark(Key::ControlImmune);
        }
        if let Some(v) = m.invulnerable {
            out.invulnerable = v;
            out.mark(Key::Invulnerable);
        }
        out
    }

    /// Sets a numeric key (booleans: non-zero is true).
    pub fn set(&mut self, key: Key, value: f64) {
        self.mark(key);
        let int = value as i64;
        match key {
            Key::MaxHp => self.max_hp = int,
            Key::MaxMana => self.max_mana = int,
            Key::Damage => self.damage = int,
            Key::Armor => self.armor = int,
            Key::HealthRegen => self.health_regen = value,
            Key::ManaRegen => self.mana_regen = value,
            Key::DamagePermille => self.damage_permille = int,
            Key::AttackSpeedPermille => self.attack_speed_permille = int,
            Key::MoveSpeedPermille => self.move_speed_permille = int,
            Key::CooldownReductionPermille => self.cooldown_reduction_permille = int,
            Key::LifestealPermille => self.lifesteal_permille = int,
            Key::MeleeReflectionPermille => self.melee_reflection_permille = int,
            Key::Rooted => self.rooted = value != 0.0,
            Key::ControlImmune => self.control_immune = value != 0.0,
            Key::Invulnerable => self.invulnerable = value != 0.0,
        }
    }

    pub fn get(&self, key: Key) -> f64 {
        match key {
            Key::MaxHp => self.max_hp as f64,
            Key::MaxMana => self.max_mana as f64,
            Key::Damage => self.damage as f64,
            Key::Armor => self.armor as f64,
            Key::HealthRegen => self.health_regen,
            Key::ManaRegen => self.mana_regen,
            Key::DamagePermille => self.damage_permille as f64,
            Key::AttackSpeedPermille => self.attack_speed_permille as f64,
            Key::MoveSpeedPermille => self.move_speed_permille as f64,
            Key::CooldownReductionPermille => self.cooldown_reduction_permille as f64,
            Key::LifestealPermille => self.lifesteal_permille as f64,
            Key::MeleeReflectionPermille => self.melee_reflection_permille as f64,
            Key::Rooted => f64::from(u8::from(self.rooted)),
            Key::ControlImmune => f64::from(u8::from(self.control_immune)),
            Key::Invulnerable => f64::from(u8::from(self.invulnerable)),
        }
    }

    /// Numeric values multiplied by `stacks` (old stacking statuses).
    pub fn times(mut self, stacks: u32) -> Self {
        if stacks == 1 {
            return self;
        }
        for key in NUMERIC {
            if self.has(key) {
                self.set(key, self.get(key) * f64::from(stacks));
            }
        }
        self
    }

    /// Merges `other` into a per-source group: a key not yet present is taken; a numeric
    /// key replaces the kept one when its magnitude is larger (old `spellModifiers`).
    pub fn strongest(&mut self, other: &Self) {
        for key in NUMERIC {
            if !other.has(key) {
                continue;
            }
            let v = other.get(key);
            if !self.has(key) || v.abs() > self.get(key).abs() {
                self.set(key, v);
            }
        }
        for key in [Key::Rooted, Key::ControlImmune, Key::Invulnerable] {
            if other.has(key) && !self.has(key) {
                self.set(key, other.get(key));
            }
        }
    }

    /// Whether any present value is truthy (old `Object.values(m).some(Boolean)`).
    pub fn any(&self) -> bool {
        NUMERIC
            .iter()
            .chain(&[Key::Rooted, Key::ControlImmune, Key::Invulnerable])
            .any(|&k| self.has(k) && self.get(k) != 0.0)
    }
}

/// A status declaration at one rank.
#[derive(Debug, Clone, Default)]
pub struct StatusRank {
    /// Fixed modifiers with the ranked ones merged over them.
    pub modifiers: Modifiers,
    pub duration: u32,
    pub hero_duration: Option<u32>,
    pub shield: Option<u32>,
    pub periodic_damage: u32,
    pub detection: u32,
    pub concealment_bonus: u32,
    pub source_attack_bonus: Option<u32>,
    /// Per damage type index (combat rules order): permille taken.
    pub damage_taken: Vec<(u8, u32)>,
}

/// A status declaration resolved for the runtime.
#[derive(Debug, Clone)]
pub struct StatusDecl {
    pub name: String,
    pub polarity: Polarity,
    pub dispel: bool,
    pub lifetime: StatusLifetime,
    pub stun: bool,
    pub silence: bool,
    pub disarm: bool,
    pub item_blocked: bool,
    pub taunt: bool,
    pub break_on_damage: bool,
    pub ethereal: bool,
    /// Control kinds it protects against (bits); `ALL` with the `controlImmune` modifier.
    pub control_immunity: u8,
    pub control_immunity_declared: bool,
    pub stacking: Option<(u32, StackScope)>,
    pub remove_when_depleted: bool,
    /// Periodic damage: interval and damage type index.
    pub periodic: Option<(u32, u8)>,
    /// Death triggers of this status run as the applier, which is captured on application.
    pub needs_source: bool,
    /// Some trigger of the ability names this status.
    pub carries_triggers: bool,
    /// The full declaration (forms, concealment, combat modifiers, immunities).
    pub source: StatusEffect,
    pub ranks: Vec<StatusRank>,
}

impl StatusDecl {
    pub fn rank(&self, rank: u8) -> &StatusRank {
        &self.ranks[usize::from(rank.max(1)) - 1]
    }

    /// Control components of the status at `rank` (old `statusControls`).
    pub fn controls(&self, rank: u8) -> u8 {
        let m = &self.rank(rank).modifiers;
        let mut bits = 0;
        if self.stun {
            bits |= control::STUN;
        }
        if self.disarm {
            bits |= control::DISARM;
        }
        if self.silence {
            bits |= control::SILENCE;
        }
        if self.item_blocked {
            bits |= control::ITEM_BLOCKED;
        }
        if self.taunt {
            bits |= control::TAUNT;
        }
        if m.rooted {
            bits |= control::ROOT;
        }
        if m.move_speed_permille < 0 {
            bits |= control::MOVE_SLOW;
        }
        if m.attack_speed_permille < 0 {
            bits |= control::ATTACK_SLOW;
        }
        bits
    }

    /// Whether an application blocked in `blocked` still carries something worth keeping
    /// (old `statusHasPayload`).
    pub fn has_payload(&self, rank: u8, blocked: u8) -> bool {
        if self.controls(rank) & !blocked != 0 {
            return true;
        }
        let r = self.rank(rank);
        let mut m = r.modifiers;
        m.clear(Key::Rooted);
        if m.move_speed_permille < 0 {
            m.clear(Key::MoveSpeedPermille);
        }
        if m.attack_speed_permille < 0 {
            m.clear(Key::AttackSpeedPermille);
        }
        let s = &self.source;
        m.any()
            || self.ethereal
            || s.damage_taken_permille.is_some()
            || s.periodic.is_some()
            || s.shield.is_some()
            || s.mana_shield.is_some()
            || s.concealment.is_some()
            || s.detection_radius.is_some()
            || s.barrier.is_some()
            || s.form.is_some()
            || s.immunity.is_some()
            || s.spell_immunity.is_some()
            || s.control_immunity.as_ref().is_some_and(|c| !c.is_empty())
            || s.combat_modifiers.is_some()
            || s.source_attack_bonus.is_some()
            || self.carries_triggers
    }
}

/// An ability binding of a caster definition.
#[derive(Debug, Clone)]
pub struct BindingDecl {
    pub id: String,
    pub ability: AbilityId,
    pub initial_rank: u8,
    pub controls: Vec<Control>,
    pub learning: Option<Vec<u32>>,
    pub ai: Option<AbilityAi>,
}

impl BindingDecl {
    pub fn allows(&self, control: Control) -> bool {
        self.controls.contains(&control)
    }
}

/// `behaviors.abilities` of a definition.
#[derive(Debug, Clone)]
pub struct CasterPolicy {
    pub max_mana: u32,
    pub regen_per_second: f64,
    pub bindings: Vec<BindingDecl>,
}

impl CasterPolicy {
    pub fn binding(&self, id: &str) -> Option<(usize, &BindingDecl)> {
        self.bindings.iter().enumerate().find(|(_, b)| b.id == id)
    }
}

/// One ability with its resolved statuses.
#[derive(Debug, Clone)]
pub struct Compiled {
    pub statuses: Vec<StatusDecl>,
}

/// The compiled ability library.
#[derive(Debug, Clone)]
pub struct Spells {
    abilities: Vec<Compiled>,
    /// Ability indexes sorted by id.
    by_id: Vec<u16>,
    /// Ability ids by index.
    ids: Vec<String>,
    casters: Vec<Option<CasterPolicy>>,
    /// Damage type ids in combat rules order (sorted).
    damage_types: Vec<String>,
}

impl Spells {
    pub fn new(content: &Content) -> Self {
        let damage_types: Vec<String> = content
            .rules
            .damage_types
            .keys()
            .map(|k| k.as_str().to_string())
            .collect();
        let library = &content.abilities.abilities;
        assert!(library.len() < usize::from(u16::MAX), "too many abilities");
        let mut by_id: Vec<u16> = (0..library.len() as u16).collect();
        by_id.sort_by(|&a, &b| library[usize::from(a)].id.cmp(&library[usize::from(b)].id));
        let mut spells = Self {
            abilities: Vec::new(),
            by_id,
            ids: library.iter().map(|a| a.id.as_str().to_string()).collect(),
            casters: Vec::new(),
            damage_types,
        };
        spells.abilities = library.iter().map(|a| spells.compile(a)).collect();
        spells.casters = content
            .definitions
            .iter()
            .map(|d| {
                let policy = d.behaviors.abilities.as_ref()?;
                Some(CasterPolicy {
                    max_mana: policy.max_mana,
                    regen_per_second: policy.mana_regen_per_second,
                    bindings: policy
                        .bindings
                        .iter()
                        .filter_map(|b| {
                            Some(BindingDecl {
                                id: b.id.clone(),
                                ability: spells.find(b.ability.as_str())?,
                                initial_rank: b.initial_rank as u8,
                                controls: b.controls.clone(),
                                learning: b.learning.as_ref().map(|l| l.required_levels.clone()),
                                ai: b.ai.clone(),
                            })
                        })
                        .collect(),
                })
            })
            .collect();
        spells
    }

    pub fn damage_type(&self, id: &str) -> u8 {
        self.damage_types
            .binary_search_by(|t| t.as_str().cmp(id))
            .map_or(0, |i| i as u8)
    }

    pub fn damage_type_name(&self, index: u8) -> &str {
        &self.damage_types[usize::from(index)]
    }

    pub fn find(&self, id: &str) -> Option<AbilityId> {
        self.by_id
            .binary_search_by(|&i| self.abilities_id(i).cmp(id))
            .ok()
            .map(|i| AbilityId(self.by_id[i]))
    }

    fn abilities_id(&self, index: u16) -> &str {
        &self.ids[usize::from(index)]
    }

    pub fn compiled(&self, id: AbilityId) -> &Compiled {
        &self.abilities[usize::from(id.0)]
    }

    pub fn status(&self, key: StatusKey) -> &StatusDecl {
        &self.abilities[usize::from(key.ability.0)].statuses[usize::from(key.status)]
    }

    pub fn status_named(&self, ability: AbilityId, name: &str) -> Option<StatusKey> {
        self.compiled(ability)
            .statuses
            .iter()
            .position(|s| s.name == name)
            .map(|i| StatusKey {
                ability,
                status: i as u8,
            })
    }

    pub fn caster(&self, def: Def) -> Option<&CasterPolicy> {
        self.casters.get(usize::from(def.0))?.as_ref()
    }

    fn compile(&self, a: &Ability) -> Compiled {
        let mut statuses: Vec<StatusDecl> = Vec::new();
        for status in a.all_effects().into_iter().filter_map(|e| e.status()) {
            if statuses.iter().any(|s| s.name == status.id) {
                continue;
            }
            statuses.push(self.status_decl(a, status));
        }
        Compiled { statuses }
    }

    fn status_decl(&self, a: &Ability, s: &StatusEffect) -> StatusDecl {
        let fixed = Modifiers::of(&s.modifiers);
        let ranks = a
            .ranks
            .iter()
            .map(|rank| {
                let value = |amount: &Amount| amount.at(rank).unwrap_or(0).max(0) as u32;
                let mut modifiers = fixed;
                for (key, amount) in s.ranked_modifiers.iter().flatten() {
                    let v = amount.at(rank).unwrap_or(0) as f64;
                    modifiers.set(ranked_key(*key), v);
                }
                StatusRank {
                    modifiers,
                    duration: value(&s.amount),
                    hero_duration: s.hero_duration.as_ref().map(value),
                    shield: s.shield.as_ref().map(value),
                    periodic_damage: s.periodic.as_ref().map_or(0, |p| value(&p.damage)),
                    detection: s.detection_radius.as_ref().map_or(0, value),
                    concealment_bonus: s.concealment.as_ref().map_or(0, |c| value(&c.attack_bonus)),
                    source_attack_bonus: s.source_attack_bonus.as_ref().map(value),
                    damage_taken: s
                        .damage_taken_permille
                        .iter()
                        .flatten()
                        .map(|(t, v)| (self.damage_type(t), value(v)))
                        .collect(),
                }
            })
            .collect();
        let immunity: u8 = s
            .control_immunity
            .iter()
            .flatten()
            .fold(0, |bits, &k| bits | control_bit(k));
        let carries_triggers = a
            .triggers
            .iter()
            .flatten()
            .any(|t| t.while_status.as_deref() == Some(s.id.as_str()));
        let needs_source = a.triggers.iter().flatten().any(|t| {
            t.event == TriggerEvent::Death
                && t.executor.is_some()
                && t.while_status.as_deref() == Some(s.id.as_str())
        });
        StatusDecl {
            name: s.id.clone(),
            polarity: s.polarity,
            dispel: s.dispel,
            lifetime: s.lifetime,
            stun: s.stun == Some(true),
            silence: s.silence == Some(true),
            disarm: s.disarm == Some(true),
            item_blocked: s.item_blocked == Some(true),
            taunt: s.taunt == Some(true),
            break_on_damage: s.break_on_damage == Some(true),
            ethereal: s.ethereal == Some(true),
            control_immunity: if fixed.control_immune {
                control::ALL
            } else {
                immunity
            },
            control_immunity_declared: s.control_immunity.is_some(),
            stacking: s.stacking.as_ref().map(|st| (st.max, st.scope)),
            remove_when_depleted: s.on_shield_depleted == Some(ShieldDepletion::Remove),
            periodic: s
                .periodic
                .as_ref()
                .map(|p| (p.interval_ticks, self.damage_type(&p.damage_type))),
            needs_source,
            carries_triggers,
            source: s.clone(),
            ranks,
        }
    }
}

fn ranked_key(key: RankedModifier) -> Key {
    match key {
        RankedModifier::MaxHp => Key::MaxHp,
        RankedModifier::HealthRegenPerSecond => Key::HealthRegen,
        RankedModifier::ManaRegenPerSecond => Key::ManaRegen,
        RankedModifier::Damage => Key::Damage,
        RankedModifier::Armor => Key::Armor,
        RankedModifier::DamagePermille => Key::DamagePermille,
        RankedModifier::AttackSpeedPermille => Key::AttackSpeedPermille,
        RankedModifier::MoveSpeedPermille => Key::MoveSpeedPermille,
        RankedModifier::LifestealPermille => Key::LifestealPermille,
        RankedModifier::MeleeReflectionPermille => Key::MeleeReflectionPermille,
    }
}

/// An amount at a one-based rank of `ability` (old `value`); 0 when the rank lacks it.
pub fn value(ability: &Ability, rank: u8, amount: &Amount) -> i64 {
    parameters(ability, rank)
        .and_then(|r| amount.at(r))
        .unwrap_or(0)
}

/// The rank parameters of a one-based rank.
pub fn parameters(ability: &Ability, rank: u8) -> Option<&RankParameters> {
    usize::from(rank)
        .checked_sub(1)
        .and_then(|i| ability.ranks.get(i))
}

/// A ranked definition reference at a one-based rank.
pub fn reference(reference: &RankedReference, rank: u8) -> &str {
    reference
        .at_rank(usize::from(rank))
        .or_else(|| reference.ids().last())
        .map_or("", |id| id.as_str())
}

pub fn passive(a: &Ability) -> bool {
    a.activation == Activation::Passive
}

/// Every operation of an ability, flattened (old `allEffects`).
pub fn all_effects(a: &Ability) -> impl Iterator<Item = &Effect> {
    a.on_release
        .iter()
        .chain(a.triggers.iter().flatten().flat_map(|t| &t.operations))
        .flat_map(|op| op.effects())
}
