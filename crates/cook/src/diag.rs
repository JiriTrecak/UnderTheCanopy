//! Problems found in the inputs. Errors stop the cook; warnings are printed.

use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Warning,
    Error,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub severity: Severity,
    /// The input file or asset the problem is in.
    pub source: String,
    pub message: String,
}

impl fmt::Display for Diagnostic {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let label = match self.severity {
            Severity::Warning => "warning",
            Severity::Error => "error",
        };
        write!(f, "{label}: {}: {}", self.source, self.message)
    }
}

/// Collects diagnostics about one source.
#[derive(Debug, Default)]
pub struct Diagnostics {
    pub source: String,
    pub list: Vec<Diagnostic>,
}

impl Diagnostics {
    pub fn new(source: impl Into<String>) -> Self {
        Self {
            source: source.into(),
            list: Vec::new(),
        }
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.push(Severity::Warning, message.into());
    }

    pub fn error(&mut self, message: impl Into<String>) {
        self.push(Severity::Error, message.into());
    }

    fn push(&mut self, severity: Severity, message: String) {
        let diagnostic = Diagnostic {
            severity,
            source: self.source.clone(),
            message,
        };
        if !self.list.contains(&diagnostic) {
            self.list.push(diagnostic);
        }
    }

    pub fn has_errors(&self) -> bool {
        self.list.iter().any(|d| d.severity == Severity::Error)
    }
}
