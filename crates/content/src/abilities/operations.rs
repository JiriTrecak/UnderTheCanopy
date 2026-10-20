//! Operations an ability performs on release or from a trigger, and their recipients.

use super::amount::{Amount, RankedReference};
use super::literal::Exactly;
use crate::{
    conditions::{AbilityCondition, CasterSubject, Locomotion, Relation, Subject, TargetFilter},
    id::Id,
    items::ItemModifiers,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// The literal amount `1` of single-recipient operations.
pub type One = Exactly<1>;

fn yes() -> bool {
    true
}

/// A fixed point or actor an operation is anchored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Anchor {
    Point,
    Caster,
    Target,
}

impl From<Subject> for Anchor {
    fn from(subject: Subject) -> Self {
        match subject {
            Subject::Target => Self::Target,
            Subject::Caster => Self::Caster,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum WeaponFilter {
    Melee,
    Projectile,
    Any,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct AttackBonus {
    /// Status template applied on a damaging weapon hit, before death resolution.
    pub status: Option<String>,
    pub amount: Amount,
    pub mana_cost: Amount,
    pub weapon: WeaponFilter,
    #[serde(default = "yes")]
    pub blocked_by_silence: bool,
    pub filter: Option<TargetFilter>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Critical {
    pub chance_permille: Amount,
    pub multiplier_permille: Amount,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cleave {
    pub damage_permille: Amount,
    pub radius: Amount,
    pub arc_degrees: f64,
    pub max_targets: u32,
    #[serde(default)]
    pub include_buildings: bool,
    pub filter: Option<TargetFilter>,
}

/// Ranked weapon policies, shared by learned passives and timed status grants.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CombatModifiers {
    pub attack_bonus: Option<AttackBonus>,
    pub critical: Option<Critical>,
    pub evasion_permille: Option<Amount>,
    pub cleave: Option<Cleave>,
}

impl CombatModifiers {
    pub(crate) fn filters(&self) -> impl Iterator<Item = &TargetFilter> {
        let bonus = self.attack_bonus.as_ref().and_then(|b| b.filter.as_ref());
        let cleave = self.cleave.as_ref().and_then(|c| c.filter.as_ref());
        bonus.into_iter().chain(cleave)
    }
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum RecipientOrder {
    #[default]
    Nearest,
    LowestHealth,
}

fn max_recipients() -> u32 {
    128
}

/// Units around a centre that an operation applies to instead of the aim itself.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RecipientQuery {
    pub center: Anchor,
    pub radius: Amount,
    pub relations: Vec<Relation>,
    #[serde(default)]
    pub allow_self: bool,
    #[serde(default)]
    pub include_buildings: bool,
    #[serde(default)]
    pub exclude_primary: bool,
    #[serde(default = "max_recipients")]
    pub max_targets: u32,
    #[serde(default)]
    pub order: RecipientOrder,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum UnitStat {
    Health,
    MaxHealth,
    MissingHealth,
    Mana,
    MaxMana,
    MissingMana,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ResultStat {
    Applied,
    Count,
    Health,
    MaxHealth,
    Mana,
    MaxMana,
}

/// Adds `floor(stat × permille / 1000)` to an amount, capped at one million. `Target` is
/// the primary aim, `Recipient` the current recipient; results come from an earlier
/// `record` in the same program.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "source", rename_all = "camelCase", deny_unknown_fields)]
pub enum AmountScale {
    Caster {
        stat: UnitStat,
        permille: Amount,
    },
    Target {
        stat: UnitStat,
        permille: Amount,
    },
    Recipient {
        stat: UnitStat,
        permille: Amount,
    },
    Result {
        id: String,
        stat: ResultStat,
        permille: Amount,
    },
}

impl AmountScale {
    pub fn permille(&self) -> &Amount {
        match self {
            Self::Caster { permille, .. }
            | Self::Target { permille, .. }
            | Self::Recipient { permille, .. }
            | Self::Result { permille, .. } => permille,
        }
    }
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Heal {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: Amount,
    pub scale: Option<AmountScale>,
    /// Records applied amount, recipient count and pre-operation resources for later
    /// operations of the same release.
    pub record: Option<String>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Damage {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: Amount,
    pub scale: Option<AmountScale>,
    pub record: Option<String>,
    pub damage_type: String,
}

/// Restores mana.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManaRestore {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: Amount,
    pub scale: Option<AmountScale>,
    pub record: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ControlKind {
    Stun,
    Root,
    Silence,
    Disarm,
    ItemBlocked,
    MoveSlow,
    AttackSlow,
    Taunt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Polarity {
    Positive,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum StackScope {
    Source,
    Ability,
}

/// Reapplication refreshes all stacks; additive numeric modifiers scale per stack.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stacking {
    pub max: u32,
    pub scope: StackScope,
}

/// Status modifiers that can take a per-rank amount.
#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum RankedModifier {
    MaxHp,
    HealthRegenPerSecond,
    ManaRegenPerSecond,
    Damage,
    Armor,
    DamagePermille,
    AttackSpeedPermille,
    MoveSpeedPermille,
    LifestealPermille,
    MeleeReflectionPermille,
}

impl RankedModifier {
    pub fn name(self) -> &'static str {
        match self {
            Self::MaxHp => "maxHp",
            Self::HealthRegenPerSecond => "healthRegenPerSecond",
            Self::ManaRegenPerSecond => "manaRegenPerSecond",
            Self::Damage => "damage",
            Self::Armor => "armor",
            Self::DamagePermille => "damagePermille",
            Self::AttackSpeedPermille => "attackSpeedPermille",
            Self::MoveSpeedPermille => "moveSpeedPermille",
            Self::LifestealPermille => "lifestealPermille",
            Self::MeleeReflectionPermille => "meleeReflectionPermille",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ShieldDepletion {
    /// Keep the status until its ordinary lifetime ends.
    Retain,
    /// End the whole status after absorbing the final point.
    Remove,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Concealment {
    pub fade_ticks: u32,
    pub break_on_attack: bool,
    pub break_on_cast: bool,
    #[serde(default)]
    pub attack_bonus: Amount,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct FormMovement {
    pub locomotion: Locomotion,
    pub flight_height: Option<f64>,
}

fn one() -> f64 {
    1.0
}

/// Temporary model, scale and movement class; body clearance stays unchanged.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Form {
    pub movement: Option<FormMovement>,
    #[serde(default = "one")]
    pub opacity: f64,
    pub asset: Option<Id>,
    /// Unit definition supplying weapon damage, range, timing and projectile policy.
    pub combat_profile: Option<Id>,
    #[serde(default = "one")]
    pub scale: f64,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum StatusLifetime {
    #[default]
    Duration,
    /// Ends with the persistent instance that applied it.
    Instance,
    UntilDeath,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManaShield {
    pub damage_per_mana: f64,
    pub absorb_permille: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Immunity {
    Spell,
    Physical,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SpellImmunity {
    Hostile,
    All,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PeriodicDamage {
    pub interval_ticks: u32,
    pub damage: Amount,
    pub damage_type: String,
}

/// A timed (or instance- or life-bound) state on a unit: modifiers, controls, shields,
/// forms and periodic damage. `amount` is the duration in ticks.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StatusEffect {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub id: String,
    pub amount: Amount,
    pub hero_duration: Option<Amount>,
    pub polarity: Polarity,
    pub dispel: bool,
    pub control_immunity: Option<Vec<ControlKind>>,
    /// Cannot attack or take non-spell damage; stays visible, collidable and targetable.
    pub ethereal: Option<bool>,
    /// 1000 is unchanged, per damage type.
    pub damage_taken_permille: Option<BTreeMap<String, Amount>>,
    pub combat_modifiers: Option<CombatModifiers>,
    pub stacking: Option<Stacking>,
    /// Flat bonus per stack to later primary basic attacks from this status's source.
    pub source_attack_bonus: Option<Amount>,
    #[serde(default)]
    pub modifiers: ItemModifiers,
    pub ranked_modifiers: Option<BTreeMap<RankedModifier, Amount>>,
    pub taunt: Option<bool>,
    pub stun: Option<bool>,
    pub disarm: Option<bool>,
    pub silence: Option<bool>,
    pub item_blocked: Option<bool>,
    pub break_on_damage: Option<bool>,
    pub shield: Option<Amount>,
    pub on_shield_depleted: Option<ShieldDepletion>,
    pub concealment: Option<Concealment>,
    pub detection_radius: Option<Amount>,
    pub form: Option<Form>,
    #[serde(default)]
    pub lifetime: StatusLifetime,
    pub mana_shield: Option<ManaShield>,
    pub immunity: Option<Immunity>,
    pub spell_immunity: Option<SpellImmunity>,
    pub periodic: Option<PeriodicDamage>,
    /// A field barrier in front of the holder: hostile weapon missiles that would cross it
    /// from the front are stopped where they meet it.
    pub barrier: Option<Barrier>,
}

/// A curved field pane `radius` world units in front of its holder, `arcDegrees` wide,
/// centred on the holder's facing.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Barrier {
    pub radius: f64,
    pub arc_degrees: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DispelPolarity {
    All,
    Positive,
    Negative,
}

/// Removes dispellable statuses; `amount` damages units that lose one.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Dispel {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: Amount,
    pub damage_type: String,
    pub polarity: DispelPolarity,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CorpseRelation {
    Ally,
    Enemy,
    Neutral,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum CorpseOrder {
    #[default]
    Nearest,
    Strongest,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CorpseQuery {
    pub radius: Amount,
    pub relations: Vec<CorpseRelation>,
    pub filter: Option<TargetFilter>,
    #[serde(default)]
    pub order: CorpseOrder,
}

/// A corpse query that also limits how many corpses a summon consumes.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SummonCorpseQuery {
    pub radius: Amount,
    pub relations: Vec<CorpseRelation>,
    pub filter: Option<TargetFilter>,
    #[serde(default)]
    pub order: CorpseOrder,
    /// `amount` summons are created per consumed corpse.
    pub max_targets: Amount,
}

/// Creates `amount` units of a definition.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Summon {
    pub target: Anchor,
    pub query: Option<RecipientQuery>,
    pub amount: Amount,
    pub definition: RankedReference,
    /// Absent for a permanent summon.
    pub duration_ticks: Option<Amount>,
    /// Remove the summon when its source dies, disappears or changes controller.
    pub ends_with_caster: Option<bool>,
    /// Living summons kept per owner, caster and ability across ranks.
    pub max_active: Option<Amount>,
    pub replace: bool,
    pub radius: f64,
    pub grants: Option<Vec<String>>,
    pub corpses: Option<SummonCorpseQuery>,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum AllLost {
    #[default]
    Kill,
    Return,
}

/// Splits the caster into linked member units for `amount` ticks.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Split {
    pub id: String,
    pub target: CasterSubject,
    pub amount: Amount,
    /// Distinct member slots in return-anchor priority order.
    pub members: Vec<RankedReference>,
    pub radius: u32,
    /// Absent preserves current health.
    pub return_health_permille: Option<Amount>,
    pub return_mana_permille: Option<Amount>,
    #[serde(default)]
    pub on_all_lost: AllLost,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Ownership {
    Original,
    Caster,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum SupplyPolicy {
    #[default]
    Require,
    AllowOverCap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum ResurrectTarget {
    Caster,
    Point,
}

/// Raises corpses as units.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Resurrect {
    pub ownership: Option<Ownership>,
    pub duration_ticks: Option<Amount>,
    pub grants: Option<Vec<String>>,
    pub target: ResurrectTarget,
    pub query: Option<RecipientQuery>,
    pub amount: Amount,
    pub corpses: CorpseQuery,
    pub health_permille: Amount,
    pub mana_permille: Amount,
    pub placement_radius: u32,
    #[serde(default)]
    pub supply: SupplyPolicy,
}

fn default_placement_wait() -> u32 {
    400
}

/// Returns the fallen holder after `amount` ticks; runs from a death trigger.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Revive {
    pub id: String,
    pub target: CasterSubject,
    pub query: Option<RecipientQuery>,
    pub amount: Amount,
    pub health_permille: Amount,
    pub mana_permille: Amount,
    pub placement_radius: u32,
    #[serde(default = "default_placement_wait")]
    pub placement_wait_ticks: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum DrainResource {
    Health,
    Mana,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Drain {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: Amount,
    pub scale: Option<AmountScale>,
    pub record: Option<String>,
    pub resource: DrainResource,
    pub restore_caster: bool,
    pub damage_type: String,
    #[serde(default)]
    pub damage_per_drained_permille: u32,
}

/// Moves recipients; `amount` is the arrival clearance search radius.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Teleport {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: Amount,
    pub destination: Anchor,
    pub preserve_offset: bool,
}

/// Drags recipients toward an anchor, as a vortex does: `amount` is the distance per
/// application in hundredths of a world unit. Recipients never come closer than
/// `minDistance` (world units) and never cross ground they could not stand on.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pull {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: Amount,
    pub toward: Anchor,
    #[serde(default)]
    pub min_distance: f64,
}

/// A temporary vision source; `amount` is its lifetime in ticks.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Vision {
    pub id: String,
    pub target: Anchor,
    pub query: Option<RecipientQuery>,
    pub amount: Amount,
    pub radius: Amount,
    pub ignore_terrain: bool,
    #[serde(default)]
    pub detect_invisible: bool,
    pub ends_with_caster: bool,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Convert {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: One,
    #[serde(default)]
    pub supply: SupplyPolicy,
}

#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum ContainLifetime {
    #[default]
    Duration,
    UntilDeath,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Digestion {
    pub interval_ticks: u32,
    pub damage: Amount,
    pub damage_type: String,
}

/// Carries recipients inside the caster for up to `amount` ticks.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Contain {
    pub id: String,
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    /// Share the carrier's sight with the contained unit's owner and allies.
    pub share_vision: Option<bool>,
    pub amount: Amount,
    #[serde(default)]
    pub lifetime: ContainLifetime,
    pub capacity: u32,
    pub digestion: Option<Digestion>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ReleaseContained {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: One,
}

/// Kills an owned non-hero unit other than the caster, bypassing mitigation.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sacrifice {
    pub target: Subject,
    pub query: Option<RecipientQuery>,
    pub filter: Option<TargetFilter>,
    pub amount: One,
    pub record: Option<String>,
}

/// Raises a finished building of the caster's owner: where the caster stands, or at the
/// aim point (the building lattice point nearest it whose ground allows the building).
/// `consume` uses the caster up, as an Alate settles into the mound it founds.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Found {
    pub target: Anchor,
    pub building: Id,
    #[serde(default)]
    pub consume: bool,
}

/// One operation, by `op`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "op", rename_all = "camelCase")]
pub enum Effect {
    Heal(Heal),
    Damage(Damage),
    Status(Box<StatusEffect>),
    Dispel(Dispel),
    Summon(Box<Summon>),
    Split(Split),
    Resurrect(Box<Resurrect>),
    Revive(Revive),
    Drain(Drain),
    Mana(ManaRestore),
    Teleport(Teleport),
    Pull(Pull),
    Vision(Vision),
    Convert(Convert),
    Contain(Contain),
    ReleaseContained(ReleaseContained),
    Sacrifice(Sacrifice),
    Found(Found),
}

impl Effect {
    /// Where the operation applies: an actor, or a point for point-anchored operations.
    pub fn target(&self) -> Anchor {
        match self {
            Self::Heal(e) => e.target.into(),
            Self::Damage(e) => e.target.into(),
            Self::Status(e) => e.target.into(),
            Self::Dispel(e) => e.target.into(),
            Self::Summon(e) => e.target,
            Self::Split(_) | Self::Revive(_) => Anchor::Caster,
            Self::Resurrect(e) => match e.target {
                ResurrectTarget::Caster => Anchor::Caster,
                ResurrectTarget::Point => Anchor::Point,
            },
            Self::Drain(e) => e.target.into(),
            Self::Mana(e) => e.target.into(),
            Self::Teleport(e) => e.target.into(),
            Self::Pull(e) => e.target.into(),
            Self::Vision(e) => e.target,
            Self::Convert(e) => e.target.into(),
            Self::Contain(e) => e.target.into(),
            Self::ReleaseContained(e) => e.target.into(),
            Self::Sacrifice(e) => e.target.into(),
            Self::Found(e) => e.target,
        }
    }

    pub fn query(&self) -> Option<&RecipientQuery> {
        match self {
            Self::Heal(e) => e.query.as_ref(),
            Self::Damage(e) => e.query.as_ref(),
            Self::Status(e) => e.query.as_ref(),
            Self::Dispel(e) => e.query.as_ref(),
            Self::Summon(e) => e.query.as_ref(),
            Self::Split(_) => None,
            Self::Resurrect(e) => e.query.as_ref(),
            Self::Revive(e) => e.query.as_ref(),
            Self::Drain(e) => e.query.as_ref(),
            Self::Mana(e) => e.query.as_ref(),
            Self::Teleport(e) => e.query.as_ref(),
            Self::Pull(e) => e.query.as_ref(),
            Self::Vision(e) => e.query.as_ref(),
            Self::Convert(e) => e.query.as_ref(),
            Self::Contain(e) => e.query.as_ref(),
            Self::ReleaseContained(e) => e.query.as_ref(),
            Self::Sacrifice(e) => e.query.as_ref(),
            Self::Found(_) => None,
        }
    }

    pub fn filter(&self) -> Option<&TargetFilter> {
        match self {
            Self::Heal(e) => e.filter.as_ref(),
            Self::Damage(e) => e.filter.as_ref(),
            Self::Status(e) => e.filter.as_ref(),
            Self::Dispel(e) => e.filter.as_ref(),
            Self::Drain(e) => e.filter.as_ref(),
            Self::Mana(e) => e.filter.as_ref(),
            Self::Teleport(e) => e.filter.as_ref(),
            Self::Pull(e) => e.filter.as_ref(),
            Self::Convert(e) => e.filter.as_ref(),
            Self::Contain(e) => e.filter.as_ref(),
            Self::ReleaseContained(e) => e.filter.as_ref(),
            Self::Sacrifice(e) => e.filter.as_ref(),
            Self::Summon(_)
            | Self::Split(_)
            | Self::Resurrect(_)
            | Self::Revive(_)
            | Self::Vision(_)
            | Self::Found(_) => None,
        }
    }

    /// The operation's main magnitude; its meaning depends on the operation.
    pub fn amount(&self) -> Amount {
        match self {
            Self::Heal(e) => e.amount.clone(),
            Self::Damage(e) => e.amount.clone(),
            Self::Status(e) => e.amount.clone(),
            Self::Dispel(e) => e.amount.clone(),
            Self::Summon(e) => e.amount.clone(),
            Self::Split(e) => e.amount.clone(),
            Self::Resurrect(e) => e.amount.clone(),
            Self::Revive(e) => e.amount.clone(),
            Self::Drain(e) => e.amount.clone(),
            Self::Mana(e) => e.amount.clone(),
            Self::Teleport(e) => e.amount.clone(),
            Self::Pull(e) => e.amount.clone(),
            Self::Vision(e) => e.amount.clone(),
            Self::Contain(e) => e.amount.clone(),
            Self::Convert(_) | Self::ReleaseContained(_) | Self::Sacrifice(_) | Self::Found(_) => {
                Amount::Fixed(1)
            }
        }
    }

    pub fn scale(&self) -> Option<&AmountScale> {
        match self {
            Self::Heal(e) => e.scale.as_ref(),
            Self::Damage(e) => e.scale.as_ref(),
            Self::Drain(e) => e.scale.as_ref(),
            Self::Mana(e) => e.scale.as_ref(),
            _ => None,
        }
    }

    pub fn record(&self) -> Option<&str> {
        match self {
            Self::Heal(e) => e.record.as_deref(),
            Self::Damage(e) => e.record.as_deref(),
            Self::Drain(e) => e.record.as_deref(),
            Self::Mana(e) => e.record.as_deref(),
            Self::Sacrifice(e) => e.record.as_deref(),
            _ => None,
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            Self::Heal(_) => "heal",
            Self::Damage(_) => "damage",
            Self::Status(_) => "status",
            Self::Dispel(_) => "dispel",
            Self::Summon(_) => "summon",
            Self::Split(_) => "split",
            Self::Resurrect(_) => "resurrect",
            Self::Revive(_) => "revive",
            Self::Drain(_) => "drain",
            Self::Mana(_) => "mana",
            Self::Teleport(_) => "teleport",
            Self::Pull(_) => "pull",
            Self::Vision(_) => "vision",
            Self::Convert(_) => "convert",
            Self::Contain(_) => "contain",
            Self::ReleaseContained(_) => "releaseContained",
            Self::Sacrifice(_) => "sacrifice",
            Self::Found(_) => "found",
        }
    }
}

/// The literal `"branch"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum BranchOp {
    Branch,
}

/// Runs `then` when the condition holds for the caster and aim, otherwise `else`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Branch {
    pub op: BranchOp,
    pub condition: AbilityCondition,
    pub then: Vec<Effect>,
    #[serde(rename = "else")]
    pub otherwise: Vec<Effect>,
}

/// An entry of a release or trigger program: an operation or a one-level branch.
#[derive(Debug, Clone, PartialEq)]
pub enum Operation {
    Effect(Effect),
    Branch(Branch),
}

impl Operation {
    /// The operations this entry may run: itself, or both arms of a branch.
    pub fn effects(&self) -> impl Iterator<Item = &Effect> {
        let (single, arms) = match self {
            Self::Effect(effect) => (Some(effect), None),
            Self::Branch(branch) => (None, Some(branch.then.iter().chain(&branch.otherwise))),
        };
        single.into_iter().chain(arms.into_iter().flatten())
    }
}

impl Serialize for Operation {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Effect(effect) => effect.serialize(serializer),
            Self::Branch(branch) => branch.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for Operation {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = serde_json::Value::deserialize(deserializer)?;
        if value.get("op").and_then(serde_json::Value::as_str) == Some("branch") {
            serde_json::from_value(value).map(Self::Branch)
        } else {
            serde_json::from_value(value).map(Self::Effect)
        }
        .map_err(D::Error::custom)
    }
}

/// Serializes `statuses`, whose templates carry `"op": "status"` like operations do.
pub(crate) mod status_templates {
    use super::StatusEffect;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize)]
    #[serde(tag = "op", rename_all = "camelCase")]
    enum Borrowed<'a> {
        Status(&'a StatusEffect),
    }

    #[derive(Deserialize, schemars::JsonSchema)]
    #[serde(tag = "op", rename_all = "camelCase")]
    pub(crate) enum Owned {
        Status(StatusEffect),
    }

    pub fn serialize<S: Serializer>(
        statuses: &Option<Vec<StatusEffect>>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        statuses
            .as_ref()
            .map(|list| list.iter().map(Borrowed::Status).collect::<Vec<_>>())
            .serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Vec<StatusEffect>>, D::Error> {
        let list = Option::<Vec<Owned>>::deserialize(deserializer)?;
        Ok(list.map(|list| {
            list.into_iter()
                .map(|Owned::Status(status)| status)
                .collect()
        }))
    }
}

impl schemars::JsonSchema for Operation {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "Operation".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let _ = &generator;
        let effect = generator.subschema_for::<Effect>();
        let branch = generator.subschema_for::<Branch>();
        schemars::json_schema!({"anyOf": [effect, branch]})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn round_trip(source: Value) -> Operation {
        let operation: Operation = serde_json::from_value(source.clone())
            .unwrap_or_else(|error| panic!("{error}: {source}"));
        assert_eq!(serde_json::to_value(&operation).unwrap(), source);
        operation
    }

    #[test]
    fn every_operation_round_trips() {
        let query = json!({"center": "caster", "radius": 4, "relations": ["enemy"], "allowSelf": false,
            "includeBuildings": false, "excludePrimary": true, "maxTargets": 3, "order": "lowest-health"});
        let corpses = json!({"radius": 6, "relations": ["enemy", "neutral"], "order": "strongest"});
        for source in [
            json!({"op": "heal", "target": "target", "amount": 5, "scale": {"source": "recipient", "stat": "missingHealth", "permille": 100}, "record": "healed"}),
            json!({"op": "damage", "target": "caster", "query": query, "filter": {"heroes": false}, "amount": {"rankParameter": "damage"}, "damageType": "spell"}),
            json!({"op": "status", "target": "target", "id": "slow", "amount": 40, "polarity": "negative", "dispel": true,
                "modifiers": {"moveSpeedPermille": -300}, "rankedModifiers": {"armor": {"rankParameter": "armor"}},
                "damageTakenPermille": {"spell": 1200}, "lifetime": "duration",
                "form": {"opacity": 0.5, "scale": 1.5, "movement": {"locomotion": "air", "flightHeight": 4.0}},
                "periodic": {"intervalTicks": 10, "damage": 3, "damageType": "spell"}}),
            json!({"op": "dispel", "target": "target", "amount": 0, "damageType": "spell", "polarity": "positive"}),
            json!({"op": "summon", "target": "point", "amount": 2, "definition": {"byRank": ["unit.test.a"]}, "replace": false, "radius": 2.5,
                "corpses": {"radius": 3, "relations": ["ally"], "order": "nearest", "maxTargets": 2}}),
            json!({"op": "split", "id": "parts", "target": "caster", "amount": 100, "members": ["unit.test.a", "unit.test.b"], "radius": 2, "onAllLost": "return"}),
            json!({"op": "resurrect", "target": "point", "amount": 1, "corpses": corpses, "healthPermille": 500, "manaPermille": 0, "placementRadius": 2, "supply": "allow-over-cap"}),
            json!({"op": "revive", "id": "again", "target": "caster", "amount": 80, "healthPermille": 1000, "manaPermille": 0, "placementRadius": 3, "placementWaitTicks": 400}),
            json!({"op": "drain", "target": "target", "amount": 10, "resource": "mana", "restoreCaster": true, "damageType": "spell", "damagePerDrainedPermille": 0}),
            json!({"op": "mana", "target": "caster", "amount": 10}),
            json!({"op": "teleport", "target": "caster", "amount": 2, "destination": "point", "preserveOffset": false}),
            json!({"op": "vision", "id": "eye", "target": "point", "amount": 100, "radius": 8, "ignoreTerrain": true, "detectInvisible": false, "endsWithCaster": false}),
            json!({"op": "convert", "target": "target", "amount": 1, "supply": "require"}),
            json!({"op": "contain", "id": "belly", "target": "target", "amount": 200, "lifetime": "untilDeath", "capacity": 1,
                "digestion": {"intervalTicks": 40, "damage": 5, "damageType": "spell"}}),
            json!({"op": "releaseContained", "target": "caster", "amount": 1}),
            json!({"op": "sacrifice", "target": "target", "amount": 1, "record": "fed"}),
        ] {
            round_trip(source);
        }
    }

    #[test]
    fn defaults_match_the_authoring_format() {
        let source = json!({"op": "status", "target": "caster", "id": "ward", "amount": 10, "polarity": "positive", "dispel": false,
            "form": {}, "concealment": {"fadeTicks": 4, "breakOnAttack": true, "breakOnCast": false}});
        let Operation::Effect(Effect::Status(status)) = serde_json::from_value(source).unwrap()
        else {
            panic!("status");
        };
        assert_eq!(status.lifetime, StatusLifetime::Duration);
        assert_eq!(status.modifiers, ItemModifiers::default());
        let form = status.form.unwrap();
        assert_eq!((form.opacity, form.scale), (1.0, 1.0));
        assert_eq!(status.concealment.unwrap().attack_bonus, Amount::Fixed(0));

        let query: RecipientQuery =
            serde_json::from_value(json!({"center": "target", "radius": 2, "relations": ["ally"]}))
                .unwrap();
        assert_eq!(query.max_targets, 128);
        assert_eq!(query.order, RecipientOrder::Nearest);
        assert!(!query.allow_self && !query.include_buildings && !query.exclude_primary);

        let Operation::Effect(Effect::Revive(revive)) = serde_json::from_value(json!({"op": "revive", "id": "r",
            "target": "caster", "amount": 1, "healthPermille": 1, "manaPermille": 0, "placementRadius": 0}))
        .unwrap() else {
            panic!("revive");
        };
        assert_eq!(revive.placement_wait_ticks, 400);
    }

    #[test]
    fn branches_hold_one_level_of_operations() {
        let source = json!({"op": "branch", "condition": {"kind": "relation", "of": "target", "to": "caster", "is": "ally"},
            "then": [{"op": "heal", "target": "target", "amount": 5}],
            "else": [{"op": "damage", "target": "target", "amount": 5, "damageType": "spell"}]});
        let operation = round_trip(source);
        assert_eq!(operation.effects().count(), 2);
        let nested = json!({"op": "branch", "condition": {"kind": "relation", "of": "target", "to": "caster", "is": "ally"},
            "then": [{"op": "branch", "condition": {"kind": "relation", "of": "target", "to": "caster", "is": "ally"}, "then": [], "else": []}],
            "else": []});
        assert!(serde_json::from_value::<Operation>(nested).is_err());
    }

    #[test]
    fn unknown_fields_and_operations_are_rejected() {
        for source in [
            json!({"op": "heal", "target": "target", "amount": 5, "damageType": "spell"}),
            json!({"op": "split", "id": "s", "target": "caster", "query": {"center": "caster", "radius": 1, "relations": ["ally"]},
                "amount": 1, "members": ["unit.test.a", "unit.test.b"], "radius": 1}),
            json!({"op": "convert", "target": "target", "amount": 2}),
            json!({"op": "explode", "target": "target", "amount": 1}),
            json!({"op": "summon", "target": "point", "filter": {}, "amount": 1, "definition": "unit.test.a", "replace": false, "radius": 1}),
        ] {
            assert!(
                serde_json::from_value::<Operation>(source.clone()).is_err(),
                "{source}"
            );
        }
    }
}
