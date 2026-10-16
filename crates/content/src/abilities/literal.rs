//! Values the format allows exactly one of, and other small encodings.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

/// A number that must be exactly `N`, such as `schemaVersion: 1`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Exactly<const N: u32>;

/// `schemaVersion` of every published document.
pub type SchemaVersion = Exactly<1>;

impl<const N: u32> Serialize for Exactly<N> {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_u32(N)
    }
}

impl<'de, const N: u32> Deserialize<'de> for Exactly<N> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        match u64::deserialize(deserializer)? {
            value if value == u64::from(N) => Ok(Self),
            value => Err(D::Error::custom(format!("expected {N}, found {value}"))),
        }
    }
}

/// The literal `true`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct True;

impl Serialize for True {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_bool(true)
    }
}

impl<'de> Deserialize<'de> for True {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if bool::deserialize(deserializer)? {
            Ok(Self)
        } else {
            Err(D::Error::custom("expected true"))
        }
    }
}

/// Display names are trimmed and must not be empty (`z.string().trim().min(1)`).
pub(crate) fn trimmed_name<'de, D: Deserializer<'de>>(deserializer: D) -> Result<String, D::Error> {
    let text = String::deserialize(deserializer)?;
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(D::Error::custom("name must not be empty"));
    }
    Ok(trimmed.to_string())
}

impl schemars::JsonSchema for True {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        "True".into()
    }

    fn json_schema(generator: &mut schemars::SchemaGenerator) -> schemars::Schema {
        let _ = &generator;
        schemars::json_schema!({"const": true})
    }
}

impl<const N: u32> schemars::JsonSchema for Exactly<N> {
    fn schema_name() -> std::borrow::Cow<'static, str> {
        format!("Exactly{N}").into()
    }

    fn json_schema(_: &mut schemars::SchemaGenerator) -> schemars::Schema {
        schemars::json_schema!({"const": N})
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn literals_accept_only_their_value() {
        assert!(serde_json::from_value::<SchemaVersion>(json!(1)).is_ok());
        assert!(serde_json::from_value::<SchemaVersion>(json!(2)).is_err());
        assert_eq!(serde_json::to_value(Exactly::<1>).unwrap(), json!(1));
        assert!(serde_json::from_value::<True>(json!(true)).is_ok());
        assert!(serde_json::from_value::<True>(json!(false)).is_err());
    }
}
