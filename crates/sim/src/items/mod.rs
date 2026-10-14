//! Hero items (old `itemEffects.ts`, `inventory.ts`, `itemModifiers.ts`, `itemValidation.ts`,
//! `regeneration.ts` and the item half of `scenario/inventory.ts`): inventory slots, pickup
//! and drop, equipment and permanent bonuses, item auras and statuses, actives, rescue,
//! on-hit procs, item shields, and health regeneration.

pub mod effects;
pub mod inventory;
pub mod regeneration;

use crate::{
    abilities::library::Modifiers,
    catalog::{Catalog, Def},
    entity::{Entity, EntityId},
};
use canopy_content::{Content, items::ItemEffect};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// An item definition's effect with its modifiers resolved.
#[derive(Debug, Clone)]
pub struct ItemDecl {
    pub effect: ItemEffect,
    pub modifiers: Option<Modifiers>,
    pub aura: Option<Modifiers>,
    pub active_status: Option<Modifiers>,
    /// Flat equipment stats (equipment items).
    pub flat: Option<(u32, u32, u32)>,
}

/// Item effects of every definition, by [`Def`].
#[derive(Debug, Clone, Default)]
pub struct Items {
    by_def: Vec<Option<ItemDecl>>,
}

impl Items {
    pub fn new(content: &Content) -> Self {
        Self {
            by_def: content
                .definitions
                .iter()
                .map(|d| {
                    let effect = d.item_effect.clone()?;
                    let (modifiers, aura, active, flat) = match &effect {
                        ItemEffect::Equipment {
                            damage,
                            armor,
                            max_hp,
                            modifiers,
                            aura,
                            active,
                            ..
                        } => (modifiers, aura, active, Some((*damage, *armor, *max_hp))),
                        ItemEffect::Consumable {
                            modifiers,
                            aura,
                            active,
                            ..
                        } => (modifiers, aura, active, None),
                    };
                    Some(ItemDecl {
                        modifiers: modifiers.as_ref().map(Modifiers::of),
                        aura: aura.as_ref().map(|a| Modifiers::of(&a.modifiers)),
                        active_status: active
                            .as_ref()
                            .and_then(|a| a.status.as_ref())
                            .map(|s| Modifiers::of(&s.modifiers)),
                        flat,
                        effect,
                    })
                })
                .collect(),
        }
    }

    pub fn get(&self, def: Def) -> Option<&ItemDecl> {
        self.by_def.get(usize::from(def.0))?.as_ref()
    }
}

/// Charges, cooldown and proc counter of one carried item (old `ItemRuntime`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemRuntime {
    #[serde(default)]
    pub ready_tick: u32,
    #[serde(default)]
    pub hits: u32,
    pub charges: Option<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ItemStatusKind {
    Aura,
    Active,
    Rescue,
}

/// An item status on a unit (old `itemStatuses`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemStatus {
    pub item: Def,
    pub source: EntityId,
    pub kind: ItemStatusKind,
    pub expires: u32,
    pub shield: Option<u32>,
}

/// Item damage queued for the next combat resolution (old `itemHits`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ItemHit {
    pub item: Def,
    pub target: EntityId,
    pub damage: u32,
    pub damage_type: u8,
}

/// Permanent bonuses from consumed power-ups (old `progression.bonuses`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Bonuses {
    pub max_hp: u32,
    pub damage: u32,
    pub armor: u32,
    pub max_mana: u32,
}

/// Item slots and the item effects on a unit.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Equipment {
    /// Hero item slots; empty for units without an inventory.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub slots: Vec<Option<Def>>,
    /// Runtime of each slot's item.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime: Vec<Option<ItemRuntime>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub statuses: Vec<ItemStatus>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hits: Vec<ItemHit>,
    pub bonuses: Option<Bonuses>,
}

impl Equipment {
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Drops an empty equipment block.
pub fn tidy(e: &mut Entity) {
    if e.equipment.as_ref().is_some_and(|q| q.is_empty()) {
        e.equipment = None;
    }
}

/// The equipment block, created on demand.
pub fn equipment(e: &mut Entity) -> &mut Equipment {
    e.equipment.get_or_insert_with(Default::default)
}

/// Equipment and item status modifiers (old `nonSpellModifiers`).
pub fn non_spell_modifiers(catalog: &Catalog, e: &Entity, f: &mut dyn FnMut(&Modifiers)) {
    let Some(q) = &e.equipment else {
        return;
    };
    for item in q.slots.iter().flatten() {
        if let Some(m) = catalog.items.get(*item).and_then(|d| d.modifiers.as_ref()) {
            f(m);
        }
    }
    for s in &q.statuses {
        f(&status_modifiers(catalog, s));
    }
}

/// What an item status changes (old `statusModifiers`).
pub fn status_modifiers(catalog: &Catalog, s: &ItemStatus) -> Modifiers {
    let decl = catalog.items.get(s.item);
    match s.kind {
        ItemStatusKind::Rescue => {
            let mut m = Modifiers::default();
            m.set(crate::abilities::library::Key::Invulnerable, 1.0);
            m
        }
        ItemStatusKind::Aura => decl.and_then(|d| d.aura).unwrap_or_default(),
        ItemStatusKind::Active => decl.and_then(|d| d.active_status).unwrap_or_default(),
    }
}

/// Whether any non-spell modifier satisfies `test`.
pub fn any_modifier(catalog: &Catalog, e: &Entity, test: impl Fn(&Modifiers) -> bool) -> bool {
    let mut any = false;
    non_spell_modifiers(catalog, e, &mut |m| any |= test(m));
    any
}

/// Hero item slots, for the mission API (`None` without an inventory).
pub fn slots(catalog: &Catalog, e: &Entity) -> Option<Vec<Option<Def>>> {
    let inventory = catalog.get(e.definition).behaviors.inventory.as_ref()?;
    Some(
        e.equipment
            .as_ref()
            .filter(|q| !q.slots.is_empty())
            .map_or_else(|| vec![None; inventory.slots as usize], |q| q.slots.clone()),
    )
}

/// Flat equipment stats and permanent bonuses: (max hp, damage, armour, max mana).
pub fn flat_stats(catalog: &Catalog, e: &Entity) -> (i64, i64, i64, i64) {
    let Some(q) = &e.equipment else {
        return (0, 0, 0, 0);
    };
    let mut out = q.bonuses.map_or((0, 0, 0, 0), |b| {
        (
            i64::from(b.max_hp),
            i64::from(b.damage),
            i64::from(b.armor),
            i64::from(b.max_mana),
        )
    });
    for item in q.slots.iter().flatten() {
        if let Some((damage, armor, max_hp)) = catalog.items.get(*item).and_then(|d| d.flat) {
            out.0 += i64::from(max_hp);
            out.1 += i64::from(damage);
            out.2 += i64::from(armor);
        }
    }
    out
}
