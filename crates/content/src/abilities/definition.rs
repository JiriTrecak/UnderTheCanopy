//! Ability definitions, presentations and the bindings that give units abilities.

use super::amount::{Amount, RankParameters};
use super::literal::{SchemaVersion, True, trimmed_name};
use super::operations::{CombatModifiers, Effect, Operation, StatusEffect, WeaponFilter};
use crate::{
    conditions::{AbilityCondition, Relation, TargetFilter},
    effects::EffectBinding,
    id::Id,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_with::skip_serializing_none;

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Activation {
    Targeted,
    Passive,
}

/// A single enhanced normal weapon attack, using weapon timing and the cast's mana and
/// cooldown.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WeaponCast {
    pub bonus: Amount,
    pub weapon: WeaponFilter,
    pub status: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum PersistentAnchor {
    Point,
    Caster,
    Target,
}

/// A lasting instance that runs the release program every `interval_ticks`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Persistent {
    pub duration_ticks: Amount,
    pub interval_ticks: Amount,
    pub anchor: PersistentAnchor,
    #[serde(default = "yes")]
    pub ends_with_caster: bool,
    pub tether_range: Option<f64>,
    #[serde(default)]
    pub upkeep_mana: Amount,
    #[serde(default)]
    pub toggle: bool,
}

/// How the release reaches its recipients.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
pub enum Delivery {
    /// The grounded caster runs at a unit; release operations apply only on contact.
    #[serde(rename_all = "camelCase")]
    Charge { speed: f64, max_ticks: u32 },
    /// Self-cast seekers that run the release program on visible enemies.
    #[serde(rename_all = "camelCase")]
    Swarm {
        count: Amount,
        duration_ticks: Amount,
        speed: f64,
        radius: f64,
        launch_interval_ticks: u32,
        attack_interval_ticks: u32,
        max_per_target: u32,
        return_threshold: Amount,
        return_permille: u32,
        max_hits_per_trip: u32,
        return_timeout_ticks: u32,
        filter: Option<TargetFilter>,
        #[serde(default)]
        include_buildings: bool,
    },
    #[serde(rename_all = "camelCase")]
    Projectile { speed: f64 },
    #[serde(rename_all = "camelCase")]
    Chain {
        #[serde(default)]
        skip_full_health: bool,
        bounces: Amount,
        radius: f64,
        interval_ticks: u32,
        retention_permille: u32,
    },
    #[serde(rename_all = "camelCase")]
    Line { speed: f64, length: f64, width: f64 },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Aura {
    pub radius: f64,
    pub melee_only: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Autocast {
    pub interval_ticks: u32,
    pub enabled_by_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TargetingKind {
    Unit,
    Point,
    /// `"self"`.
    #[serde(rename = "self")]
    Caster,
}

/// The literal `"reject"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum OutOfRange {
    Reject,
}

/// What the ability may be aimed at.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Targeting {
    pub condition: Option<AbilityCondition>,
    pub filter: Option<TargetFilter>,
    pub include_buildings: Option<bool>,
    pub kind: TargetingKind,
    /// Area radius of point aims.
    pub radius: Option<Amount>,
    pub max_targets: Option<u32>,
    pub alive: True,
    pub visible: bool,
    pub relations: Vec<Relation>,
    pub allow_self: bool,
    pub range: Amount,
    pub out_of_range: OutOfRange,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Channel {
    pub waves: Amount,
    pub interval_ticks: Amount,
    pub tether_range: Option<f64>,
}

/// The literal `"mana"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CostResource {
    Mana,
}

/// The literal `"release"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AtRelease {
    Release,
}

/// The literal `"ability"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CooldownScope {
    Ability,
}

/// The literal `"refund"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CancelPolicy {
    Refund,
}

/// Mana is spent at release.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cost {
    pub resource: CostResource,
    pub amount: Amount,
    pub commit: AtRelease,
}

/// The cooldown starts at release and belongs to the ability.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cooldown {
    pub ticks: Amount,
    pub start: AtRelease,
    pub scope: CooldownScope,
}

/// The fixed list `["caster", "target", "range", "visibility", "relationship"]`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct RevalidateAtRelease;

const REVALIDATED: [&str; 5] = ["caster", "target", "range", "visibility", "relationship"];

impl Serialize for RevalidateAtRelease {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        REVALIDATED.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for RevalidateAtRelease {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let list = Vec::<String>::deserialize(deserializer)?;
        if list == REVALIDATED {
            Ok(Self)
        } else {
            Err(D::Error::custom(format!("expected {REVALIDATED:?}")))
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cast {
    pub prepare_ticks: u32,
    pub recover_ticks: u32,
    pub channel: Option<Channel>,
    pub cost: Cost,
    pub cooldown: Cooldown,
    pub cancel_before_release: CancelPolicy,
    pub revalidate_at_release: RevalidateAtRelease,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TriggerEvent {
    Interval,
    WeaponRelease,
    WeaponHit,
    MeleeDamaged,
    Damaged,
    Death,
    Kill,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum TriggerWeapon {
    Melee,
    Projectile,
    Siege,
}

/// The literal `"statusSource"`: a death trigger on a status runs for its applier.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Executor {
    StatusSource,
}

fn certain() -> u32 {
    1000
}

fn every_time() -> u32 {
    1
}

/// Operations run when an event happens to the holder.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Trigger {
    pub id: String,
    pub event: TriggerEvent,
    /// Interval triggers only; the first pulse comes after this many ticks.
    pub interval_ticks: Option<Amount>,
    pub weapon: Option<TriggerWeapon>,
    pub blocked_by_silence: Option<bool>,
    pub include_buildings: Option<bool>,
    /// For kill or weaponRelease: filters the hostile victim.
    pub filter: Option<TargetFilter>,
    pub executor: Option<Executor>,
    pub cooldown_ticks: Option<Amount>,
    pub while_status: Option<String>,
    #[serde(default = "certain")]
    pub chance_permille: u32,
    #[serde(default = "every_time")]
    pub every: u32,
    #[serde(default)]
    pub mana_cost: Amount,
    pub operations: Vec<Operation>,
}

/// A spell or passive. Numbers that vary by rank are [`Amount`]s naming a rank parameter.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Ability {
    pub schema_version: SchemaVersion,
    pub id: Id,
    #[serde(deserialize_with = "trimmed_name")]
    pub name: String,
    #[serde(default)]
    pub description: String,
    pub activation: Activation,
    pub weapon_cast: Option<WeaponCast>,
    pub combat_modifiers: Option<CombatModifiers>,
    pub pierces_spell_immunity: Option<bool>,
    pub persistent: Option<Persistent>,
    pub delivery: Option<Delivery>,
    pub aura: Option<Aura>,
    pub autocast: Option<Autocast>,
    pub ranks: Vec<RankParameters>,
    pub targeting: Targeting,
    pub cast: Cast,
    /// Reusable status templates, applied only by explicit reference.
    #[serde(default, with = "super::operations::status_templates")]
    pub statuses: Option<Vec<StatusEffect>>,
    pub on_release: Vec<Operation>,
    pub triggers: Option<Vec<Trigger>>,
    pub presentation: Id,
}

impl Ability {
    /// Every operation the ability can run: status templates, release (both branch arms)
    /// and triggers, in that order.
    pub fn all_effects(&self) -> Vec<StatusOrEffect<'_>> {
        let templates = self.statuses.iter().flatten().map(StatusOrEffect::Template);
        let programs = self
            .on_release
            .iter()
            .chain(self.triggers.iter().flatten().flat_map(|t| &t.operations))
            .flat_map(Operation::effects)
            .map(StatusOrEffect::Effect);
        templates.chain(programs).collect()
    }

    /// The first status declared with this id, anywhere in the ability.
    pub fn status(&self, id: &str) -> Option<&StatusEffect> {
        self.all_effects()
            .into_iter()
            .filter_map(StatusOrEffect::status)
            .find(|s| s.id == id)
    }
}

/// An entry of [`Ability::all_effects`]: a status template or an operation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum StatusOrEffect<'a> {
    Template(&'a StatusEffect),
    Effect(&'a Effect),
}

impl<'a> StatusOrEffect<'a> {
    pub fn status(self) -> Option<&'a StatusEffect> {
        match self {
            Self::Template(status) => Some(status),
            Self::Effect(Effect::Status(status)) => Some(status),
            Self::Effect(_) => None,
        }
    }

    pub fn effect(self) -> Option<&'a Effect> {
        match self {
            Self::Effect(effect) => Some(effect),
            Self::Template(_) => None,
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Animations {
    pub prepare: String,
    pub release: String,
    pub recover: String,
    pub fallback: String,
    pub channel: Option<String>,
}

/// How an ability looks and sounds: caster animations, icon and visual effects by event.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Presentation {
    pub schema_version: SchemaVersion,
    pub id: Id,
    pub icon: Option<Id>,
    pub animations: Animations,
    pub effects: Vec<EffectBinding>,
}

/// The `library` of `content/abilities/published.json`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AbilityLibrary {
    pub schema_version: SchemaVersion,
    pub abilities: Vec<Ability>,
    pub presentations: Vec<Presentation>,
}

impl AbilityLibrary {
    pub fn ability(&self, id: &str) -> Option<&Ability> {
        self.abilities.iter().find(|a| a.id == *id)
    }

    pub fn presentation(&self, id: &str) -> Option<&Presentation> {
        self.presentations.iter().find(|p| p.id == *id)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Control {
    Player,
    Ai,
    Scenario,
}

/// A command card button for the ability.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AbilityCommand {
    pub hotkey: String,
    /// 1–4.
    pub column: u32,
    pub icon: Option<Id>,
}

/// Hero level required for each rank.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Learning {
    pub required_levels: Vec<u32>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AiIntent {
    Utility,
    Escape,
    Reinforce,
    Scout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiPreference {
    WoundedAlly,
    Enemy,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AbilityAi {
    pub intent: Option<AiIntent>,
    pub preference: AiPreference,
    pub interval_ticks: u32,
}

/// One ability a unit has, and who may use it.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AbilityBinding {
    pub id: String,
    pub ability: Id,
    /// 0 when the ability must be learned first.
    pub initial_rank: u32,
    pub controls: Vec<Control>,
    pub command: Option<AbilityCommand>,
    pub learning: Option<Learning>,
    pub ai: Option<AbilityAi>,
}

/// `behaviors.abilities`: the unit's mana pool and abilities.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AbilityCaster {
    pub max_mana: u32,
    pub mana_regen_per_second: f64,
    pub bindings: Vec<AbilityBinding>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn deliveries_parse_with_defaults() {
        let chain: Delivery = serde_json::from_value(json!({"kind": "chain", "bounces": 3,
            "radius": 5, "intervalTicks": 8, "retentionPermille": 800}))
        .unwrap();
        assert!(matches!(
            chain,
            Delivery::Chain {
                skip_full_health: false,
                ..
            }
        ));
        for source in [
            json!({"kind": "charge", "speed": 30.0, "maxTicks": 80}),
            json!({"kind": "projectile", "speed": 20.0}),
            json!({"kind": "line", "speed": 20.0, "length": 12.0, "width": 2.0}),
            json!({"kind": "swarm", "count": 3, "durationTicks": 200, "speed": 10.0, "radius": 8.0, "launchIntervalTicks": 4,
                "attackIntervalTicks": 10, "maxPerTarget": 1, "returnThreshold": 20, "returnPermille": 500,
                "maxHitsPerTrip": 2, "returnTimeoutTicks": 100, "includeBuildings": false}),
        ] {
            let delivery: Delivery = serde_json::from_value(source.clone()).unwrap();
            assert_eq!(serde_json::to_value(delivery).unwrap(), source);
        }
        assert!(
            serde_json::from_value::<Delivery>(json!({"kind": "projectile", "speed": 2, "x": 1}))
                .is_err()
        );
    }

    #[test]
    fn persistent_and_trigger_defaults() {
        let persistent: Persistent = serde_json::from_value(
            json!({"durationTicks": 100, "intervalTicks": 10, "anchor": "caster"}),
        )
        .unwrap();
        assert!(persistent.ends_with_caster && !persistent.toggle);
        assert_eq!(persistent.upkeep_mana, Amount::Fixed(0));
        let trigger: Trigger = serde_json::from_value(json!({"id": "t", "event": "kill",
            "operations": [{"op": "mana", "target": "caster", "amount": 5}]}))
        .unwrap();
        assert_eq!((trigger.chance_permille, trigger.every), (1000, 1));
        assert_eq!(trigger.mana_cost, Amount::Fixed(0));
    }

    #[test]
    fn cast_literals_are_exact() {
        let cast = json!({"prepareTicks": 1, "recoverTicks": 2,
            "cost": {"resource": "mana", "amount": 5, "commit": "release"},
            "cooldown": {"ticks": 20, "start": "release", "scope": "ability"},
            "cancelBeforeRelease": "refund",
            "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]});
        let parsed: Cast = serde_json::from_value(cast.clone()).unwrap();
        assert_eq!(serde_json::to_value(parsed).unwrap(), cast);
        let mut reordered = cast;
        reordered["revalidateAtRelease"] =
            json!(["target", "caster", "range", "visibility", "relationship"]);
        assert!(serde_json::from_value::<Cast>(reordered).is_err());
    }

    #[test]
    fn ability_names_are_trimmed() {
        let ability = json!({"schemaVersion": 1, "id": "ability.test.zap", "name": "  Zap ",
            "activation": "targeted", "ranks": [{}],
            "targeting": {"kind": "self", "alive": true, "visible": true, "relations": ["ally"], "allowSelf": true, "range": 0, "outOfRange": "reject"},
            "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 0, "commit": "release"},
                "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund",
                "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
            "statuses": [{"op": "status", "target": "target", "id": "mark", "amount": 5, "polarity": "negative", "dispel": true}],
            "onRelease": [{"op": "mana", "target": "caster", "amount": 1}],
            "presentation": "presentation.test.zap"});
        let parsed: Ability = serde_json::from_value(ability.clone()).unwrap();
        assert_eq!(parsed.name, "Zap");
        assert_eq!(parsed.description, "");
        assert!(parsed.status("mark").is_some());
        let mut expected = ability;
        expected["name"] = json!("Zap");
        expected["description"] = json!("");
        expected["statuses"][0]["modifiers"] = json!({});
        expected["statuses"][0]["lifetime"] = json!("duration");
        assert_eq!(serde_json::to_value(&parsed).unwrap(), expected);
    }
}
