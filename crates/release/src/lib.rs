//! Releases of the game (D43): the release log every player-facing change is noted in,
//! the update feed the game checks and the release tool publishes, and the signatures
//! that bind each package to its version. The formats are the old desktop app's, so
//! installed copies of it update to this game and this game updates the same way.

pub mod feed;
pub mod log;
pub mod signature;
pub mod version;

pub use feed::{Artifact, ChangelogIndex, ChangelogLink, PlatformPackage, StableFeed};
pub use log::{ChangeKind, ChangeSet, ReleaseEntry, ReleaseImage, ReleaseLog};
pub use signature::{SignatureError, verify_package};
pub use version::Version;

/// Where releases are served (CloudFront over the release bucket).
pub const BASE_URL: &str = "https://d3dte7yaa1v3g8.cloudfront.net";
/// The update feed: the newest release and its packages.
pub const FEED_URL: &str = "https://d3dte7yaa1v3g8.cloudfront.net/stable.json";
/// The updater's public key (minisign, base64 of the key file as Tauri stores it). Its
/// secret half signs every package; never replace it, or installed copies stop updating.
pub const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDFGNzA1MTQyNjdBQkUzOEEKUldTSzQ2dG5RbEZ3SDF4L3BiYk55ZFVRcnhXVmdlaWordGE1WUVYdlg2R2U3cVJFd3l3Zk92UisK";

/// The feed's name for the platform this build runs on (`darwin-aarch64`, …).
pub fn host_platform() -> String {
    let os = match std::env::consts::OS {
        "macos" => "darwin",
        other => other,
    };
    let arch = std::env::consts::ARCH;
    format!("{os}-{arch}")
}
