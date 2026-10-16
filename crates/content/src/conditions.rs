//! Unit classifications used for targeting, target filters and ability conditions.

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// Ground units path around obstacles; air units ignore them and collide only with air.
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
pub enum Locomotion {
    Ground,
    Air,
}

/// Spell-targeting classification; never changes armour or movement.
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
pub enum UnitNature {
    Organic,
    Undead,
    Mechanical,
}

/// How a unit stands to the caster's owner.
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
pub enum Relation {
    Ally,
    Enemy,
}

/// The two actors of a cast.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Subject {
    Target,
    Caster,
}

/// The literal `"target"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum TargetSubject {
    Target,
}

/// The literal `"caster"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum CasterSubject {
    Caster,
}

/// Restricts which units qualify; every present criterion must hold. Units without a
/// level count as level 1.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TargetFilter {
    pub locomotion: Option<Vec<Locomotion>>,
    pub heroes: Option<bool>,
    pub summoned: Option<bool>,
    pub natures: Option<Vec<UnitNature>>,
    pub min_level: Option<u32>,
    pub max_level: Option<u32>,
}

impl TargetFilter {
    /// Rules the authoring format enforces beyond field types.
    pub fn problems(&self) -> Vec<&'static str> {
        let mut problems = Vec::new();
        if self.locomotion.as_deref().is_some_and(has_duplicates) {
            problems.push("Duplicate locomotion class");
        }
        if self.natures.as_deref().is_some_and(has_duplicates) {
            problems.push("Duplicate unit nature");
        }
        if let (Some(min), Some(max)) = (self.min_level, self.max_level)
            && min > max
        {
            problems.push("Minimum level exceeds maximum");
        }
        problems
    }
}

/// True when a slice lists some value twice.
pub(crate) fn has_duplicates<T: PartialEq>(items: &[T]) -> bool {
    items
        .iter()
        .enumerate()
        .any(|(i, item)| items[..i].contains(item))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum PoolResource {
    Health,
    Mana,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Measure {
    Current,
    Missing,
    FractionPermille,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Comparison {
    Lt,
    Lte,
    Eq,
    Gte,
    Gt,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Controller {
    Player,
    Neutral,
}

/// Compares a health or mana pool of the caster or target with a value.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ResourceCondition {
    pub of: Subject,
    pub resource: PoolResource,
    pub measure: Measure,
    pub comparison: Comparison,
    pub value: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ControllerCondition {
    pub of: Subject,
    pub is: Controller,
}

/// The target's relation to the caster.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RelationCondition {
    pub of: TargetSubject,
    pub to: CasterSubject,
    pub is: Relation,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MatchesCondition {
    pub of: Subject,
    pub relations: Option<Vec<Relation>>,
    pub filter: TargetFilter,
}

/// A single test; groups combine these, never other groups.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum ConditionLeaf {
    Resource(ResourceCondition),
    Controller(ControllerCondition),
    Relation(RelationCondition),
    Matches(MatchesCondition),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ConditionGroup {
    pub conditions: Vec<ConditionLeaf>,
}

/// A condition on the caster and aim, used by targeting and release branches.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AbilityCondition {
    Resource(ResourceCondition),
    Controller(ControllerCondition),
    Relation(RelationCondition),
    Matches(MatchesCondition),
    /// Every condition holds.
    All(ConditionGroup),
    /// At least one condition holds.
    Any(ConditionGroup),
}

impl ConditionLeaf {
    pub fn filter(&self) -> Option<&TargetFilter> {
        match self {
            Self::Matches(matches) => Some(&matches.filter),
            _ => None,
        }
    }
}

impl AbilityCondition {
    /// Every target filter the condition uses.
    pub fn filters(&self) -> Vec<&TargetFilter> {
        match self {
            Self::Matches(matches) => vec![&matches.filter],
            Self::All(group) | Self::Any(group) => group
                .conditions
                .iter()
                .filter_map(ConditionLeaf::filter)
                .collect(),
            _ => Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn conditions_parse_every_kind_and_round_trip() {
        let source = json!({"kind": "any", "conditions": [
            {"kind": "resource", "of": "target", "resource": "health", "measure": "fractionPermille", "comparison": "lte", "value": 500},
            {"kind": "controller", "of": "caster", "is": "neutral"},
            {"kind": "relation", "of": "target", "to": "caster", "is": "enemy"},
            {"kind": "matches", "of": "target", "relations": ["ally"], "filter": {"heroes": true, "natures": ["undead"]}}
        ]});
        let condition: AbilityCondition = serde_json::from_value(source.clone()).unwrap();
        assert_eq!(condition.filters().len(), 1);
        assert_eq!(serde_json::to_value(&condition).unwrap(), source);
    }

    #[test]
    fn groups_cannot_nest_and_literals_are_exact() {
        let nested = json!({"kind": "all", "conditions": [{"kind": "any", "conditions": []}]});
        assert!(serde_json::from_value::<AbilityCondition>(nested).is_err());
        let wrong_literal =
            json!({"kind": "relation", "of": "caster", "to": "caster", "is": "ally"});
        assert!(serde_json::from_value::<AbilityCondition>(wrong_literal).is_err());
        let unknown = json!({"kind": "controller", "of": "caster", "is": "player", "extra": 1});
        assert!(serde_json::from_value::<AbilityCondition>(unknown).is_err());
    }

    #[test]
    fn filter_rules() {
        let filter: TargetFilter = serde_json::from_value(
            json!({"locomotion": ["air", "air"], "minLevel": 3, "maxLevel": 2}),
        )
        .unwrap();
        assert_eq!(
            filter.problems(),
            [
                "Duplicate locomotion class",
                "Minimum level exceeds maximum"
            ]
        );
        assert!(TargetFilter::default().problems().is_empty());
    }
}
