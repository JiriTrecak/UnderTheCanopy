//! Rank-dependent numbers and definition references.

use crate::id::Id;
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, Visitor},
    ser::SerializeMap,
};
use std::{collections::BTreeMap, fmt};

/// Parameters of one ability rank: name to integer value.
pub type RankParameters = BTreeMap<String, i32>;

/// A fixed non-negative integer, or a parameter every rank declares
/// (`{"rankParameter": "damage"}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Amount {
    Fixed(u32),
    Rank(String),
}

impl Default for Amount {
    fn default() -> Self {
        Self::Fixed(0)
    }
}

impl Amount {
    /// The value at one rank; `None` when the rank lacks the parameter.
    pub fn at(&self, rank: &RankParameters) -> Option<i64> {
        match self {
            Self::Fixed(value) => Some(i64::from(*value)),
            Self::Rank(name) => rank.get(name).map(|&v| i64::from(v)),
        }
    }
}

impl Serialize for Amount {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Fixed(value) => serializer.serialize_u32(*value),
            Self::Rank(name) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("rankParameter", name)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for Amount {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct AmountVisitor;
        impl<'de> Visitor<'de> for AmountVisitor {
            type Value = Amount;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a non-negative integer or {\"rankParameter\": name}")
            }
            fn visit_u64<E: de::Error>(self, value: u64) -> Result<Amount, E> {
                u32::try_from(value)
                    .map(Amount::Fixed)
                    .map_err(|_| E::custom(format!("amount {value} is too large")))
            }
            fn visit_i64<E: de::Error>(self, value: i64) -> Result<Amount, E> {
                u64::try_from(value)
                    .map_err(|_| E::custom(format!("amount {value} is negative")))
                    .and_then(|v| self.visit_u64(v))
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Amount, A::Error> {
                let mut name = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key != "rankParameter" {
                        return Err(de::Error::unknown_field(&key, &["rankParameter"]));
                    }
                    if name.is_some() {
                        return Err(de::Error::duplicate_field("rankParameter"));
                    }
                    name = Some(map.next_value::<String>()?);
                }
                name.map(Amount::Rank)
                    .ok_or_else(|| de::Error::missing_field("rankParameter"))
            }
        }
        deserializer.deserialize_any(AmountVisitor)
    }
}

/// One definition for all ranks, or exactly one per rank (`{"byRank": [...]}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum RankedReference {
    All(Id),
    ByRank(Vec<Id>),
}

impl RankedReference {
    /// The definition at a one-based rank.
    pub fn at_rank(&self, rank: usize) -> Option<&Id> {
        match self {
            Self::All(id) => Some(id),
            Self::ByRank(ids) => rank.checked_sub(1).and_then(|i| ids.get(i)),
        }
    }

    /// Every definition referenced, at any rank.
    pub fn ids(&self) -> &[Id] {
        match self {
            Self::All(id) => std::slice::from_ref(id),
            Self::ByRank(ids) => ids,
        }
    }
}

impl Serialize for RankedReference {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::All(id) => id.serialize(serializer),
            Self::ByRank(ids) => {
                let mut map = serializer.serialize_map(Some(1))?;
                map.serialize_entry("byRank", ids)?;
                map.end()
            }
        }
    }
}

impl<'de> Deserialize<'de> for RankedReference {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct ReferenceVisitor;
        impl<'de> Visitor<'de> for ReferenceVisitor {
            type Value = RankedReference;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a definition id or {\"byRank\": [ids]}")
            }
            fn visit_str<E: de::Error>(self, text: &str) -> Result<RankedReference, E> {
                Id::new(text).map(RankedReference::All).map_err(E::custom)
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<RankedReference, A::Error> {
                let mut ids = None;
                while let Some(key) = map.next_key::<String>()? {
                    if key != "byRank" {
                        return Err(de::Error::unknown_field(&key, &["byRank"]));
                    }
                    if ids.is_some() {
                        return Err(de::Error::duplicate_field("byRank"));
                    }
                    ids = Some(map.next_value::<Vec<Id>>()?);
                }
                match ids {
                    Some(ids) if ids.is_empty() => Err(de::Error::custom("byRank is empty")),
                    Some(ids) => Ok(RankedReference::ByRank(ids)),
                    None => Err(de::Error::missing_field("byRank")),
                }
            }
        }
        deserializer.deserialize_any(ReferenceVisitor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn amounts_are_numbers_or_rank_parameters() {
        let fixed: Amount = serde_json::from_value(json!(7)).unwrap();
        let ranked: Amount = serde_json::from_value(json!({"rankParameter": "heal"})).unwrap();
        assert_eq!(fixed, Amount::Fixed(7));
        assert_eq!(ranked, Amount::Rank("heal".into()));
        assert_eq!(
            serde_json::to_value(&ranked).unwrap(),
            json!({"rankParameter": "heal"})
        );
        let rank = RankParameters::from([("heal".into(), 40)]);
        assert_eq!(ranked.at(&rank), Some(40));
        assert_eq!(Amount::Rank("other".into()).at(&rank), None);
        for bad in [
            json!(-1),
            json!(1.5),
            json!("5"),
            json!({"rankParameter": "a", "x": 1}),
            json!({}),
        ] {
            assert!(
                serde_json::from_value::<Amount>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }

    #[test]
    fn ranked_references_are_ids_or_per_rank_lists() {
        let all: RankedReference = serde_json::from_value(json!("unit.test.wisp")).unwrap();
        assert_eq!(all.at_rank(3).unwrap().as_str(), "unit.test.wisp");
        let source = json!({"byRank": ["unit.test.a", "unit.test.b"]});
        let ranked: RankedReference = serde_json::from_value(source.clone()).unwrap();
        assert_eq!(ranked.at_rank(2).unwrap().as_str(), "unit.test.b");
        assert_eq!(ranked.at_rank(3), None);
        assert_eq!(serde_json::to_value(&ranked).unwrap(), source);
        for bad in [
            json!("Bad"),
            json!({"byRank": []}),
            json!({"ids": []}),
            json!(3),
        ] {
            assert!(
                serde_json::from_value::<RankedReference>(bad.clone()).is_err(),
                "{bad}"
            );
        }
    }
}
