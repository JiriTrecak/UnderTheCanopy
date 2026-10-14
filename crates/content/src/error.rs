//! Content errors: which file, where in it, and what is wrong.

use serde::de::DeserializeOwned;
use std::fmt;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
#[error("{file} · {location}: {message}")]
pub struct Error {
    /// Path relative to the data folder.
    pub file: String,
    /// JSON path or content id.
    pub location: String,
    pub message: String,
}

impl Error {
    pub fn new(file: &str, location: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            file: file.into(),
            location: location.into(),
            message: message.into(),
        }
    }
}

/// Every problem found in one load; the data is unusable if there is at least one.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Errors(pub Vec<Error>);

impl fmt::Display for Errors {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        writeln!(f, "{} content error(s):", self.0.len())?;
        for error in &self.0 {
            writeln!(f, "  {error}")?;
        }
        Ok(())
    }
}

impl std::error::Error for Errors {}

/// Deserializes `value`, reporting the JSON path of the first mismatch.
pub fn parse<T: DeserializeOwned>(
    value: serde_json::Value,
    file: &str,
    location: &str,
) -> Result<T, Error> {
    serde_path_to_error::deserialize(value).map_err(|error| {
        let path = error.path().to_string();
        let location = match (location.is_empty(), path.as_str()) {
            (true, _) => path,
            (false, ".") => location.to_string(),
            (false, _) => format!("{location}.{path}"),
        };
        Error::new(file, location, error.into_inner().to_string())
    })
}

/// Reads and parses a JSON file below `root`.
pub fn read_json(root: &std::path::Path, file: &str) -> Result<serde_json::Value, Error> {
    let text = std::fs::read_to_string(root.join(file))
        .map_err(|error| Error::new(file, "", error.to_string()))?;
    serde_json::from_str(&text).map_err(|error| Error::new(file, "", error.to_string()))
}
