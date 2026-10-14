//! Hero item effects (`definition.itemEffect`).

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PermanentBonuses {
    pub max_hp: Option<u32>,
    pub damage: Option<u32>,
    pub armor: Option<u32>,
    pub max_mana: Option<u32>,
}

/// Stat changes an item, aura or item status applies.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemModifiers {
    pub max_hp: Option<u32>,
    pub max_mana: Option<u32>,
    pub damage: Option<i32>,
    pub armor: Option<i32>,
    pub health_regen_per_second: Option<f64>,
    pub mana_regen_per_second: Option<f64>,
    pub damage_permille: Option<i32>,
    pub attack_speed_permille: Option<i32>,
    pub move_speed_permille: Option<i32>,
    pub cooldown_reduction_permille: Option<u32>,
    pub lifesteal_permille: Option<u32>,
    pub melee_reflection_permille: Option<u32>,
    pub rooted: Option<bool>,
    pub control_immune: Option<bool>,
    pub invulnerable: Option<bool>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemStatus {
    pub duration_ticks: u32,
    pub modifiers: ItemModifiers,
    pub shield: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActiveTarget {
    #[serde(rename = "self")]
    Caster,
    Allies,
    Enemies,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AuraTarget {
    Allies,
    Enemies,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ActiveUse {
    pub cooldown_ticks: u32,
    pub charges: Option<u32>,
    pub target: ActiveTarget,
    pub radius: u32,
    pub heal: Option<u32>,
    pub mana: Option<u32>,
    pub heal_max_permille: Option<u32>,
    pub damage: Option<u32>,
    pub damage_type: Option<String>,
    pub status: Option<ItemStatus>,
    pub reduce_ability_cooldown_ticks: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ItemAura {
    pub radius: u32,
    pub target: AuraTarget,
    pub modifiers: ItemModifiers,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct OnHit {
    pub every: u32,
    pub radius: u32,
    pub targets: u32,
    pub damage: Option<u32>,
    pub attack_damage_permille: Option<u32>,
    pub damage_type: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Rescue {
    pub heal_max_permille: u32,
    pub invulnerable_ticks: u32,
    pub charges: u32,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum ItemEffect {
    #[serde(rename_all = "camelCase")]
    Equipment {
        damage: u32,
        armor: u32,
        max_hp: u32,
        modifiers: Option<ItemModifiers>,
        aura: Option<ItemAura>,
        active: Option<ActiveUse>,
        on_hit: Option<OnHit>,
        rescue: Option<Rescue>,
    },
    #[serde(rename_all = "camelCase")]
    Consumable {
        heal: u32,
        modifiers: Option<ItemModifiers>,
        aura: Option<ItemAura>,
        active: Option<ActiveUse>,
        on_hit: Option<OnHit>,
        rescue: Option<Rescue>,
        permanent: Option<PermanentBonuses>,
        on_pickup: Option<bool>,
    },
}

impl ItemEffect {
    pub fn active(&self) -> Option<&ActiveUse> {
        match self {
            Self::Equipment { active, .. } | Self::Consumable { active, .. } => active.as_ref(),
        }
    }

    pub fn on_hit(&self) -> Option<&OnHit> {
        match self {
            Self::Equipment { on_hit, .. } | Self::Consumable { on_hit, .. } => on_hit.as_ref(),
        }
    }
}
