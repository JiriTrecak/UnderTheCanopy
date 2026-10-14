use std::path::PathBuf;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{path}: {source}")]
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("not an asset pack (bad magic)")]
    Magic,
    #[error("pack format {found}, this build reads {expected}: cook again")]
    Version { found: u32, expected: u32 },
    #[error("corrupt pack: {0}")]
    Corrupt(String),
    #[error("no entry {0:?} in the pack")]
    Missing(String),
    #[error("entry {name:?} is a {found:?}, not a {expected:?}")]
    Kind {
        name: String,
        found: crate::EntryKind,
        expected: crate::EntryKind,
    },
}

impl Error {
    pub(crate) fn corrupt(message: impl Into<String>) -> Self {
        Self::Corrupt(message.into())
    }
}
