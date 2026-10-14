//! Content identifiers such as `unit.ants.warrior`.

use serde::{Deserialize, Deserializer, Serialize};
use std::{borrow::Borrow, fmt};

/// A lowercase dotted or dashed identifier: `[a-z][a-z0-9]*` segments joined by `.` or `-`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Id(String);

impl Id {
    pub fn new(text: impl Into<String>) -> Result<Self, String> {
        let text = text.into();
        if is_valid(&text) {
            Ok(Self(text))
        } else {
            Err(format!("invalid id {text:?}"))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The part before the first `.`, which names the kind for definitions (`unit`, `item`…).
    pub fn prefix(&self) -> &str {
        self.0.split('.').next().unwrap_or_default()
    }
}

fn is_valid(text: &str) -> bool {
    let bytes = text.as_bytes();
    if !bytes.first().is_some_and(u8::is_ascii_lowercase) {
        return false;
    }
    let mut after_separator = false;
    for &byte in &bytes[1..] {
        match byte {
            b'a'..=b'z' | b'0'..=b'9' => after_separator = false,
            b'.' | b'-' if !after_separator => after_separator = true,
            _ => return false,
        }
    }
    !after_separator
}

impl<'de> Deserialize<'de> for Id {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Self::new(String::deserialize(deserializer)?).map_err(serde::de::Error::custom)
    }
}

impl fmt::Display for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Borrow<str> for Id {
    fn borrow(&self) -> &str {
        &self.0
    }
}

impl PartialEq<str> for Id {
    fn eq(&self, other: &str) -> bool {
        self.0 == other
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_dotted_and_dashed_segments() {
        for text in [
            "unit",
            "unit.ants.warrior",
            "asset.icons.spell-holy-light",
            "a1.b2",
        ] {
            assert!(Id::new(text).is_ok(), "{text}");
        }
    }

    #[test]
    fn rejects_malformed_ids() {
        for text in [
            "", "Unit", "1unit", "unit.", "unit..a", "unit.-a", "unit_a", "unit.A", ".unit",
        ] {
            assert!(Id::new(text).is_err(), "{text}");
        }
    }
}
