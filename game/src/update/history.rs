//! The installed copy's version and its release history: `releases/log.json` is built
//! into the executable, so What's new works offline (old `installed.ts`).

use canopy_release::{ReleaseEntry, ReleaseLog, Version};
use std::sync::OnceLock;

/// The release log as it was when this copy was built.
const LOG: &str = include_str!("../../../releases/log.json");

/// The installed version: the workspace version this copy was built as.
pub fn installed() -> Version {
    env!("CARGO_PKG_VERSION")
        .parse()
        .expect("the workspace version is a release version")
}

/// The bundled release log (empty if it does not read, which a test rules out).
pub fn log() -> &'static ReleaseLog {
    static LOG_CELL: OnceLock<ReleaseLog> = OnceLock::new();
    LOG_CELL.get_or_init(|| {
        ReleaseLog::parse(LOG).unwrap_or_else(|error| {
            tracing::error!(%error, "the bundled release log does not read");
            ReleaseLog {
                schema_version: 1,
                unreleased: Default::default(),
                releases: Vec::new(),
            }
        })
    })
}

/// Releases up to the installed version, newest first (old `installedHistory`).
pub fn installed_history() -> Vec<&'static ReleaseEntry> {
    log().history(installed())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_bundled_log_reads() {
        ReleaseLog::parse(LOG).expect("releases/log.json reads and validates");
        let _ = installed();
    }
}
