//! A mission played in a campaign (docs/campaign.md, D85): the tech player 1 may use,
//! the heroes it carries in from earlier missions, the hero level cap and the campaign
//! variables scripts read and write.
//!
//! The campaign hands a mission a [`CampaignStart`] when the world is created; from then
//! on everything lives in the saved [`State`] (`state.campaign`, `state.tech`), so saves,
//! replays (which start from a save of the first tick) and restores carry it. Nothing
//! here reads a clock or iterates a hash map (D4).
//!
//! Tech limits are per owner ([`lock`], [`refused`]): the campaign sets player 1's, and a
//! computer player's behaviour can limit its own the same way.

use crate::{
    catalog::Catalog,
    entity::Entity,
    heroes::Progression,
    items::{Bonuses, ItemRuntime},
    mission::state::{MAX_VARIABLE_TEXT, MAX_VARIABLES, Variable},
    state::State,
};
use canopy_content::{Id, game::Kind, placement::Owner};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::{BTreeMap, BTreeSet};

/// What a campaign hands a mission when it starts.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignStart {
    /// The campaign's player (zero-based slot; player 1 is 0).
    #[serde(default)]
    pub player: u8,
    /// What the player may not build, train or research in this mission.
    #[serde(default)]
    pub locked: BTreeSet<Id>,
    /// Heroes carried from earlier missions, at most one per definition.
    #[serde(default)]
    pub heroes: Vec<CarriedHero>,
    /// Heroes reach at most this level in this mission.
    pub hero_level_cap: Option<u32>,
    /// The campaign variables earlier missions set.
    #[serde(default)]
    pub variables: BTreeMap<String, Variable>,
}

/// A hero as a won mission leaves it (old `companyMember`).
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarriedHero {
    pub definition: Id,
    #[serde(default)]
    pub experience: u32,
    /// Learned rank by ability binding id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub learned: BTreeMap<String, u8>,
    /// Item slots in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<Option<CarriedItem>>,
    /// Permanent bonuses from consumed power-ups.
    pub bonuses: Option<Bonuses>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CarriedItem {
    pub item: Id,
    /// Charges left; spent charges stay spent.
    pub charges: Option<u32>,
}

/// The campaign side of a running mission (saved).
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CampaignState {
    pub player: u8,
    pub hero_level_cap: Option<u32>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub variables: BTreeMap<String, Variable>,
}

/// What each owner may not build, train or research, by owner.
pub type TechLimits = BTreeMap<Owner, BTreeSet<Id>>;

/// A change a mission script makes to the campaign (applied once its run succeeds).
#[derive(Debug, Clone, PartialEq)]
pub enum Change {
    /// Sets (or with `None` removes) a campaign variable.
    Set {
        name: String,
        value: Option<Variable>,
    },
    /// Unlocks tech for the campaign player for the rest of the mission.
    Allow(Id),
    /// Locks tech for the campaign player for the rest of the mission.
    Forbid(Id),
}

/// Why `owner` may not build, train or research `id` right now, if it may not.
pub fn refused(state: &State, owner: Owner, id: &str) -> Option<String> {
    (!allowed(state, owner, id)).then(|| "message.not-available-in-mission".to_string())
}

/// Whether `owner`'s tech allows `id` (everything is allowed without a limit).
pub fn allowed(state: &State, owner: Owner, id: &str) -> bool {
    state
        .tech
        .get(&owner)
        .is_none_or(|locked| !locked.contains(id))
}

/// Locks `ids` for `owner` (a computer player's held-back tech, a mission's start).
pub fn lock(state: &mut State, owner: Owner, ids: impl IntoIterator<Item = Id>) {
    state.tech.entry(owner).or_default().extend(ids);
}

/// Unlocks `id` for `owner`.
pub fn unlock(state: &mut State, owner: Owner, id: &str) {
    if let Some(locked) = state.tech.get_mut(&owner) {
        locked.remove(id);
        if locked.is_empty() {
            state.tech.remove(&owner);
        }
    }
}

/// What `owner` may not build, train or research (for the HUD), sorted.
pub fn locked(state: &State, owner: Owner) -> Vec<Id> {
    state
        .tech
        .get(&owner)
        .map(|l| l.iter().cloned().collect())
        .unwrap_or_default()
}

/// Applies a script's change.
/// Outside a campaign (a mission map played on its own) variables are not kept and tech
/// changes apply to player 1.
pub fn apply(state: &mut State, change: &Change) {
    let owner = Owner::Player(state.campaign.as_ref().map_or(0, |c| c.player));
    match change {
        Change::Set { name, value } => {
            let Some(campaign) = state.campaign.as_mut() else {
                return;
            };
            match value {
                Some(value) => {
                    campaign.variables.insert(name.clone(), value.clone());
                }
                None => {
                    campaign.variables.remove(name);
                }
            }
        }
        Change::Allow(id) => unlock(state, owner, id.as_str()),
        Change::Forbid(id) => lock(state, owner, [id.clone()]),
    }
}

/// Checks a campaign variable a script sets: its name and value.
pub fn check_variable(
    variables: &BTreeMap<String, Variable>,
    name: &str,
    value: &Variable,
) -> Result<(), String> {
    if !variables.contains_key(name) && variables.len() >= MAX_VARIABLES {
        return Err(format!("Campaign holds at most {MAX_VARIABLES} variables"));
    }
    let valid = match value {
        Variable::String(text) => text.encode_utf16().count() <= MAX_VARIABLE_TEXT,
        Variable::Number(number) => number.is_finite(),
        Variable::Integer(_) | Variable::Boolean(_) => true,
    };
    if valid {
        Ok(())
    } else {
        Err(format!(
            "Campaign variable {name} must be a finite number, a boolean or text of at most {MAX_VARIABLE_TEXT} characters"
        ))
    }
}

/// Sets the campaign up in a new world, after its placements were created: the state,
/// player 1's tech, and the carried heroes on the heroes the map placed for the player.
pub fn begin(state: &mut State, catalog: &Catalog, start: &CampaignStart) {
    let owner = Owner::Player(start.player);
    state.campaign = Some(CampaignState {
        player: start.player,
        hero_level_cap: start.hero_level_cap,
        variables: start.variables.clone(),
    });
    if !start.locked.is_empty() {
        lock(state, owner, start.locked.iter().cloned());
    }
    let heroes: Vec<_> = state
        .entities
        .iter()
        .filter(|e| {
            e.owner == owner
                && e.unit.is_some()
                && e.placement.is_some()
                && catalog.get(e.definition).is_hero()
        })
        .map(|e| e.id)
        .collect();
    for id in heroes {
        let Some(e) = state.entities.get(id) else {
            continue;
        };
        let definition = catalog.id(e.definition);
        let Some(carried) = start.heroes.iter().find(|h| &h.definition == definition) else {
            continue;
        };
        let carried = carried.clone();
        let research = crate::stats::research(state, owner).to_vec();
        if let Some(e) = state.entities.get_mut(id) {
            take_on(catalog, e, &carried, start.hero_level_cap, &research);
        }
    }
}

/// The most experience a hero of `e`'s definition may hold under `cap`.
fn ceiling(catalog: &Catalog, e: &Entity, cap: Option<u32>) -> Option<u32> {
    let levels = &catalog
        .get(e.definition)
        .behaviors
        .progression
        .as_ref()?
        .levels;
    let top = cap
        .map_or(levels.len(), |c| levels.len().min(c as usize))
        .max(1);
    levels.get(top - 1).map(|l| l.experience)
}

/// Gives a placed hero a carried hero's progress (old `restoreCompanyMember`): experience
/// within the cap, items, permanent bonuses and learned ranks that its level allows (or
/// none of them, refunding the points); then full health and mana.
fn take_on(
    catalog: &Catalog,
    e: &mut Entity,
    carried: &CarriedHero,
    cap: Option<u32>,
    research: &[Id],
) {
    if let Some(ceiling) = ceiling(catalog, e, cap) {
        e.progression = Some(Progression {
            experience: carried.experience.min(ceiling),
        });
    }
    let d = catalog.get(e.definition);
    if let Some(inventory) = &d.behaviors.inventory {
        let slots = inventory.slots as usize;
        let q = crate::items::equipment(e);
        q.slots = vec![None; slots];
        q.runtime = vec![None; slots];
        for (slot, item) in carried.items.iter().take(slots).enumerate() {
            let Some(item) = item else {
                continue;
            };
            let Some(def) = catalog
                .find(item.item.as_str())
                .filter(|&def| catalog.items.get(def).is_some())
            else {
                continue;
            };
            q.slots[slot] = Some(def);
            q.runtime[slot] = item.charges.map(|charges| ItemRuntime {
                charges: Some(charges),
                ..ItemRuntime::default()
            });
        }
    }
    if let Some(bonuses) = carried.bonuses {
        crate::items::equipment(e).bonuses = Some(bonuses);
    }
    crate::items::tidy(e);
    let level = crate::stats::Stats::of(catalog, e, research).level;
    if let (Some(policy), Some(caster)) = (catalog.spells.caster(e.definition), e.caster.as_mut()) {
        let mut ranks: Vec<u8> = policy.bindings.iter().map(|b| b.initial_rank).collect();
        let mut spent = 0u32;
        let mut valid = true;
        for (index, binding) in policy.bindings.iter().enumerate() {
            let Some(&rank) = carried.learned.get(&binding.id) else {
                continue;
            };
            let learnable = binding.learning.as_ref().is_some_and(|required| {
                rank >= binding.initial_rank
                    && (rank == 0
                        || required
                            .get(usize::from(rank) - 1)
                            .is_some_and(|&l| l <= level))
            });
            if rank != binding.initial_rank && !learnable {
                valid = false;
                break;
            }
            spent += u32::from(rank.saturating_sub(binding.initial_rank));
            ranks[index] = rank;
        }
        if valid && spent <= level {
            for (current, rank) in caster.ranks.iter_mut().zip(ranks) {
                *current = (*current).max(rank);
            }
        }
    }
    crate::abilities::research_ranks(catalog, research, e);
    let stats = crate::stats::Stats::of(catalog, e, research);
    if e.hp.is_some() {
        e.hp = Some(stats.max_hp);
    }
    if let Some(caster) = e.caster.as_mut() {
        caster.mana = stats.max_mana;
    }
}

/// Every hero of `player` as it stands (fallen ones too, never summoned ones): what a won
/// mission carries on. One per definition, the first in id order.
pub fn carried(state: &State, catalog: &Catalog, player: u8) -> Vec<CarriedHero> {
    let owner = Owner::Player(player);
    let mut out: Vec<CarriedHero> = Vec::new();
    for e in state.entities.iter() {
        let d = catalog.get(e.definition);
        if e.owner != owner
            || e.unit.is_none()
            || d.kind != Kind::Unit
            || !d.is_hero()
            || crate::abilities::actor::summoned(e)
        {
            continue;
        }
        if out.iter().any(|h| h.definition == d.id) {
            continue;
        }
        let learned = match (catalog.spells.caster(e.definition), &e.caster) {
            (Some(policy), Some(caster)) => policy
                .bindings
                .iter()
                .zip(&caster.ranks)
                .filter(|(b, _)| b.learning.is_some())
                .map(|(b, &rank)| (b.id.clone(), rank))
                .collect(),
            _ => BTreeMap::new(),
        };
        let (items, bonuses) = match &e.equipment {
            Some(q) => (
                q.slots
                    .iter()
                    .enumerate()
                    .map(|(slot, item)| {
                        item.map(|def| CarriedItem {
                            item: catalog.id(def).clone(),
                            charges: q
                                .runtime
                                .get(slot)
                                .and_then(|r| r.as_ref())
                                .and_then(|r| r.charges),
                        })
                    })
                    .collect(),
                q.bonuses,
            ),
            None => (Vec::new(), None),
        };
        out.push(CarriedHero {
            definition: d.id.clone(),
            experience: e.progression.as_ref().map_or(0, |p| p.experience),
            learned,
            items,
            bonuses,
        });
    }
    out
}
