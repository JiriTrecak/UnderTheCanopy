//! Desktop updates (D43), behaving as the old game's `UpdateController`.
//!
//! The main menu owns one [`Updater`]: it checks the release feed when the menu comes up
//! and reports what it found on the version indicator. Checking never downloads,
//! downloading never installs: the player downloads from the update details, and
//! installs with "Install and restart", which only the main menu offers (never a match).
//! A package is installed only after its signature verified and bound the advertised
//! version; every failure leaves the installed game untouched and allows a retry.
//!
//! The controller is a polled state machine: [`Backend`] calls return at once and their
//! outcomes arrive as [`Event`]s, drained by [`Updater::poll`] each frame, so the window
//! thread never waits on the network or the disk. The real backend is [`remote::Remote`]
//! (the network thread, verification and the package install); tests use a fake.

pub mod history;
pub mod http;
pub mod install;
pub mod remote;

use canopy_release::{PlatformPackage, ReleaseEntry, StableFeed, Version};
use std::path::{Path, PathBuf};

/// Where the update stands (old `UpdatePhase`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Phase {
    #[default]
    Idle,
    Checking,
    /// The installed version is the newest.
    Current,
    /// A newer version is offered.
    Available,
    Downloading,
    /// Downloaded and verified; waiting for "Install and restart".
    Ready,
    Installing,
    /// Installed; the game restarts into the new version.
    Restart,
    /// The check failed.
    Error,
}

/// A newer release the feed offers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub version: Version,
    /// The feed's plain notes (the fallback when the structured changelog is missing).
    pub notes: String,
    /// The package for this platform, if the feed has one.
    pub package: Option<PlatformPackage>,
}

/// The update on offer, if any: the feed's version must be newer than the installed one
/// (the old Tauri updater's rule). The package is the one for `platform`.
pub fn offer(feed: StableFeed, installed: Version, platform: &str) -> Option<Offer> {
    if feed.version <= installed {
        return None;
    }
    let mut platforms = feed.platforms;
    Some(Offer {
        version: feed.version,
        notes: feed.notes,
        package: platforms.remove(platform),
    })
}

/// What the menu shows (old `UpdateState`).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct State {
    pub phase: Phase,
    pub version: Option<Version>,
    pub notes: Option<String>,
    /// Bytes downloaded and the package size, when known.
    pub received: u64,
    pub total: Option<u64>,
    /// The last failure, in the player's words.
    pub error: Option<String>,
    /// Why this copy cannot install the offered update (no package for the platform, not
    /// running from an installed app); it still shows that the update exists.
    pub blocked: Option<String>,
}

impl State {
    /// An update exists (from offered to restarting).
    pub fn available(&self) -> bool {
        matches!(
            self.phase,
            Phase::Available
                | Phase::Downloading
                | Phase::Ready
                | Phase::Installing
                | Phase::Restart
        )
    }
}

/// An outcome from the backend.
#[derive(Debug)]
pub enum Event {
    /// The feed was read: an offer, nothing newer, or a failure (logged detail).
    Checked(Result<Option<Offer>, String>),
    /// The structured notes of a version (`releases/<v>/changelog.json`), if readable.
    Changelog(Version, Option<ReleaseEntry>),
    /// The download started; the package size if the server said.
    Started(Option<u64>),
    /// More bytes arrived.
    Received(u64),
    /// The package was downloaded and verified (its path), or not.
    Downloaded(Result<PathBuf, String>),
    /// The package was installed over the running copy, or not (nothing changed).
    Installed(Result<(), String>),
}

/// The outside world of the updater.
pub trait Backend {
    /// Reads the feed; answers [`Event::Checked`].
    fn check(&mut self);
    /// Reads a version's structured notes; answers [`Event::Changelog`].
    fn changelog(&mut self, version: Version);
    /// Downloads and verifies the offer's package; answers [`Event::Started`],
    /// [`Event::Received`] and [`Event::Downloaded`].
    fn download(&mut self, offer: &Offer);
    /// Installs a verified package; answers [`Event::Installed`].
    fn install(&mut self, package: &Path);
    /// Starts the installed copy (the game then exits).
    fn relaunch(&mut self) -> Result<(), String>;
    /// Why this copy cannot install `offer`, if it cannot.
    fn blocker(&self, offer: &Offer) -> Option<String>;
    fn poll(&mut self) -> Option<Event>;
}

pub const CHECK_FAILED: &str = "Could not check for updates. Check your connection and try again.";
pub const DOWNLOAD_FAILED: &str =
    "Download or signature verification failed. Nothing was installed. Try downloading again.";
pub const INSTALL_FAILED: &str =
    "Installation failed. Close other copies of the game and try again.";
pub const RESTART_FAILED: &str = "The update is installed. Restart the app to finish.";

/// One update session (old `UpdateController`); never downloads or installs on its own.
pub struct Updater {
    state: State,
    offer: Option<Offer>,
    /// The verified package, once downloaded.
    package: Option<PathBuf>,
    /// The offered version's structured notes.
    release: Option<ReleaseEntry>,
    backend: Box<dyn Backend>,
    /// A restart is wanted: the game flushes its files and calls [`Updater::restart`].
    restart_due: bool,
    /// The new copy was started; the game exits.
    relaunched: bool,
}

impl Updater {
    pub fn new(backend: Box<dyn Backend>) -> Self {
        Self {
            state: State::default(),
            offer: None,
            package: None,
            release: None,
            backend,
            restart_due: false,
            relaunched: false,
        }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// The offered version's structured notes, once they arrived.
    pub fn release(&self) -> Option<&ReleaseEntry> {
        self.release.as_ref()
    }

    /// Checking, downloading or installing (old `busy`).
    pub fn busy(&self) -> bool {
        matches!(
            self.state.phase,
            Phase::Checking | Phase::Downloading | Phase::Installing
        )
    }

    /// Checks the feed (old `check`): nothing while busy or restarting.
    pub fn check(&mut self) {
        if self.busy() || self.state.phase == Phase::Restart {
            return;
        }
        self.state = State {
            phase: Phase::Checking,
            ..State::default()
        };
        self.offer = None;
        self.package = None;
        self.release = None;
        self.backend.check();
    }

    /// The main menu came up: check again unless an update is already on offer (a
    /// download or a verified package is kept).
    pub fn menu_entered(&mut self) {
        if matches!(
            self.state.phase,
            Phase::Idle | Phase::Current | Phase::Error
        ) {
            self.check();
        }
    }

    /// Downloads and verifies the offered package (old `download`).
    pub fn download(&mut self) {
        if self.state.phase != Phase::Available || self.state.blocked.is_some() {
            return;
        }
        let Some(offer) = &self.offer else {
            return;
        };
        self.state.phase = Phase::Downloading;
        self.state.received = 0;
        self.state.total = None;
        self.state.error = None;
        self.backend.download(offer);
    }

    /// Installs the verified package (old `install`); the game restarts afterwards.
    pub fn install(&mut self) {
        if self.state.phase != Phase::Ready {
            return;
        }
        let Some(package) = &self.package else {
            return;
        };
        self.state.phase = Phase::Installing;
        self.state.error = None;
        self.backend.install(package);
    }

    /// Asks for the restart again ("Restart now" after a failed relaunch).
    pub fn request_restart(&mut self) {
        if self.state.phase == Phase::Restart {
            self.restart_due = true;
        }
    }

    /// Whether the game should flush its files and call [`Updater::restart`].
    pub fn restart_due(&self) -> bool {
        self.restart_due
    }

    /// Starts the installed copy (old `restart`); on success the game exits.
    pub fn restart(&mut self) {
        if !std::mem::take(&mut self.restart_due) || self.state.phase != Phase::Restart {
            return;
        }
        match self.backend.relaunch() {
            Ok(()) => self.relaunched = true,
            Err(error) => {
                tracing::warn!(%error, "cannot start the updated game");
                self.state.error = Some(RESTART_FAILED.into());
            }
        }
    }

    /// The new copy is running: the game must exit now.
    pub fn relaunched(&self) -> bool {
        self.relaunched
    }

    /// Applies the backend's outcomes; once per frame.
    pub fn poll(&mut self) {
        while let Some(event) = self.backend.poll() {
            self.apply(event);
        }
    }

    fn apply(&mut self, event: Event) {
        match event {
            Event::Checked(result) if self.state.phase == Phase::Checking => match result {
                Ok(Some(offer)) => {
                    tracing::info!(version = %offer.version, "update available");
                    self.state.phase = Phase::Available;
                    self.state.version = Some(offer.version);
                    self.state.notes = Some(offer.notes.clone());
                    self.state.blocked = self.backend.blocker(&offer);
                    self.backend.changelog(offer.version);
                    self.offer = Some(offer);
                }
                Ok(None) => self.state.phase = Phase::Current,
                Err(error) => {
                    tracing::warn!(%error, "update check failed");
                    self.state.phase = Phase::Error;
                    self.state.error = Some(CHECK_FAILED.into());
                }
            },
            Event::Changelog(version, entry) => {
                if self.state.version == Some(version)
                    && let Some(entry) = entry.filter(|e| e.version == version)
                {
                    self.release = Some(entry);
                }
            }
            Event::Started(total) if self.state.phase == Phase::Downloading => {
                self.state.total = total;
            }
            Event::Received(n) if self.state.phase == Phase::Downloading => {
                self.state.received += n;
            }
            Event::Downloaded(result) if self.state.phase == Phase::Downloading => match result {
                Ok(path) => {
                    tracing::info!(package = %path.display(), "update downloaded and verified");
                    self.package = Some(path);
                    self.state.phase = Phase::Ready;
                }
                Err(error) => {
                    tracing::warn!(%error, "update download failed");
                    self.state.phase = Phase::Available;
                    self.state.error = Some(DOWNLOAD_FAILED.into());
                }
            },
            Event::Installed(result) if self.state.phase == Phase::Installing => match result {
                Ok(()) => {
                    tracing::info!("update installed");
                    self.state.phase = Phase::Restart;
                    self.restart_due = true;
                }
                Err(error) => {
                    tracing::warn!(%error, "update install failed");
                    self.state.phase = Phase::Ready;
                    self.state.error = Some(INSTALL_FAILED.into());
                }
            },
            // Late answers to something no longer asked.
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::{BTreeMap, VecDeque};
    use std::rc::Rc;

    #[derive(Default)]
    struct Script {
        calls: Vec<String>,
        events: VecDeque<Event>,
        blocker: Option<String>,
        relaunch: Option<Result<(), String>>,
    }

    struct Fake(Rc<RefCell<Script>>);

    impl Backend for Fake {
        fn check(&mut self) {
            self.0.borrow_mut().calls.push("check".into());
        }
        fn changelog(&mut self, version: Version) {
            self.0
                .borrow_mut()
                .calls
                .push(format!("changelog {version}"));
        }
        fn download(&mut self, offer: &Offer) {
            self.0
                .borrow_mut()
                .calls
                .push(format!("download {}", offer.version));
        }
        fn install(&mut self, package: &Path) {
            self.0
                .borrow_mut()
                .calls
                .push(format!("install {}", package.display()));
        }
        fn relaunch(&mut self) -> Result<(), String> {
            let mut s = self.0.borrow_mut();
            s.calls.push("relaunch".into());
            s.relaunch.clone().unwrap_or(Ok(()))
        }
        fn blocker(&self, _: &Offer) -> Option<String> {
            self.0.borrow().blocker.clone()
        }
        fn poll(&mut self) -> Option<Event> {
            self.0.borrow_mut().events.pop_front()
        }
    }

    fn v(s: &str) -> Version {
        s.parse().unwrap()
    }

    fn rig() -> (Updater, Rc<RefCell<Script>>) {
        let script = Rc::new(RefCell::new(Script::default()));
        (Updater::new(Box::new(Fake(script.clone()))), script)
    }

    fn feed(version: &str, platforms: &[&str]) -> StableFeed {
        StableFeed {
            version: v(version),
            notes: "Title\n\nFixed\n- a".into(),
            pub_date: "2026-10-15T00:00:00Z".into(),
            platforms: platforms
                .iter()
                .map(|p| {
                    (
                        p.to_string(),
                        PlatformPackage {
                            url: format!("https://example.org/{p}.tar.gz"),
                            signature: "c2ln".into(),
                        },
                    )
                })
                .collect::<BTreeMap<_, _>>(),
        }
    }

    fn offered(version: &str) -> Offer {
        offer(
            feed(version, &["darwin-aarch64"]),
            v("0.3.0"),
            "darwin-aarch64",
        )
        .unwrap()
    }

    fn send(u: &mut Updater, s: &Rc<RefCell<Script>>, event: Event) {
        s.borrow_mut().events.push_back(event);
        u.poll();
    }

    #[test]
    fn only_a_newer_feed_version_is_an_update() {
        let installed = v("0.3.0");
        assert!(
            offer(
                feed("0.3.0", &["darwin-aarch64"]),
                installed,
                "darwin-aarch64"
            )
            .is_none()
        );
        assert!(
            offer(
                feed("0.2.9", &["darwin-aarch64"]),
                installed,
                "darwin-aarch64"
            )
            .is_none()
        );
        let o = offer(
            feed("0.3.1", &["darwin-aarch64"]),
            installed,
            "darwin-aarch64",
        )
        .unwrap();
        assert_eq!(o.version, v("0.3.1"));
        assert_eq!(
            o.package.unwrap().url,
            "https://example.org/darwin-aarch64.tar.gz"
        );
        // A newer version without a package for this platform is still an update.
        let o = offer(
            feed("0.4.0", &["darwin-aarch64"]),
            installed,
            "windows-x86_64",
        )
        .unwrap();
        assert!(o.package.is_none());
        // The live feed's shape reads into the decision.
        let live: StableFeed = serde_json::from_str(
            r#"{"version":"0.2.3","notes":"n","pub_date":"2026-10-13T15:44:03.334Z",
            "platforms":{"darwin-aarch64":{"url":"https://x/p.app.tar.gz","signature":"c2ln"}}}"#,
        )
        .unwrap();
        assert!(offer(live.clone(), v("0.2.2"), "darwin-aarch64").is_some());
        assert!(offer(live, v("0.3.0"), "darwin-aarch64").is_none());
    }

    #[test]
    fn check_download_install_restart_are_separate_steps() {
        let (mut u, s) = rig();
        u.menu_entered();
        assert_eq!(u.state().phase, Phase::Checking);
        assert!(u.busy());
        send(&mut u, &s, Event::Checked(Ok(Some(offered("0.3.1")))));
        assert_eq!(u.state().phase, Phase::Available);
        assert!(u.state().available());
        assert_eq!(u.state().version, Some(v("0.3.1")));
        // Checking only asked for the notes: nothing downloads on its own.
        assert_eq!(s.borrow().calls, ["check", "changelog 0.3.1"]);
        // Install is refused before a verified download.
        u.install();
        assert_eq!(u.state().phase, Phase::Available);

        u.download();
        assert_eq!(u.state().phase, Phase::Downloading);
        u.download(); // a second click does nothing
        send(&mut u, &s, Event::Started(Some(100)));
        send(&mut u, &s, Event::Received(40));
        send(&mut u, &s, Event::Received(60));
        assert_eq!((u.state().received, u.state().total), (100, Some(100)));
        send(&mut u, &s, Event::Downloaded(Ok("/tmp/p.tar.gz".into())));
        assert_eq!(u.state().phase, Phase::Ready);
        // Downloading did not install.
        assert!(!s.borrow().calls.iter().any(|c| c.starts_with("install")));
        // Re-entering the menu keeps the verified package.
        u.menu_entered();
        assert_eq!(u.state().phase, Phase::Ready);

        u.install();
        assert_eq!(u.state().phase, Phase::Installing);
        send(&mut u, &s, Event::Installed(Ok(())));
        assert_eq!(u.state().phase, Phase::Restart);
        assert!(u.restart_due());
        u.restart();
        assert!(u.relaunched());
        assert_eq!(
            s.borrow().calls,
            [
                "check",
                "changelog 0.3.1",
                "download 0.3.1",
                "install /tmp/p.tar.gz",
                "relaunch"
            ]
        );
    }

    #[test]
    fn failures_allow_retry_and_never_reach_installation() {
        let (mut u, s) = rig();
        u.check();
        send(&mut u, &s, Event::Checked(Err("offline".into())));
        // A failed check never claims the game is current.
        assert_eq!(u.state().phase, Phase::Error);
        assert_eq!(u.state().error.as_deref(), Some(CHECK_FAILED));
        u.menu_entered(); // retries
        assert_eq!(u.state().phase, Phase::Checking);
        assert_eq!(u.state().error, None);
        send(&mut u, &s, Event::Checked(Ok(Some(offered("0.3.1")))));

        // A bad signature: back to available with the old message; nothing installs.
        u.download();
        send(
            &mut u,
            &s,
            Event::Downloaded(Err("signature mismatch".into())),
        );
        assert_eq!(u.state().phase, Phase::Available);
        assert_eq!(u.state().error.as_deref(), Some(DOWNLOAD_FAILED));
        u.install();
        assert_eq!(u.state().phase, Phase::Available);
        assert!(!s.borrow().calls.iter().any(|c| c.starts_with("install")));

        // A failed install keeps the package for another try.
        u.download();
        assert_eq!(u.state().error, None);
        send(&mut u, &s, Event::Downloaded(Ok("/tmp/p".into())));
        u.install();
        send(&mut u, &s, Event::Installed(Err("busy".into())));
        assert_eq!(u.state().phase, Phase::Ready);
        assert_eq!(u.state().error.as_deref(), Some(INSTALL_FAILED));
        u.install();
        send(&mut u, &s, Event::Installed(Ok(())));

        // A failed relaunch says so and can be retried.
        s.borrow_mut().relaunch = Some(Err("no".into()));
        u.restart();
        assert!(!u.relaunched());
        assert_eq!(u.state().error.as_deref(), Some(RESTART_FAILED));
        assert!(!u.restart_due());
        s.borrow_mut().relaunch = None;
        u.request_restart();
        u.restart();
        assert!(u.relaunched());
        // Restarting: no new checks.
        u.check();
        assert_eq!(u.state().phase, Phase::Restart);
    }

    #[test]
    fn current_and_blocked_offers_and_late_answers() {
        let (mut u, s) = rig();
        u.check();
        u.check(); // busy: one request
        send(&mut u, &s, Event::Checked(Ok(None)));
        assert_eq!(u.state().phase, Phase::Current);
        assert!(!u.state().available());
        assert_eq!(s.borrow().calls, ["check"]);
        // An answer nobody waits for changes nothing.
        send(&mut u, &s, Event::Downloaded(Ok("/x".into())));
        send(&mut u, &s, Event::Installed(Ok(())));
        assert_eq!(u.state().phase, Phase::Current);

        // An update this copy cannot install is shown, but not downloaded.
        s.borrow_mut().blocker = Some("not from an app".into());
        u.menu_entered();
        send(&mut u, &s, Event::Checked(Ok(Some(offered("0.4.0")))));
        assert_eq!(u.state().phase, Phase::Available);
        assert_eq!(u.state().blocked.as_deref(), Some("not from an app"));
        u.download();
        assert_eq!(u.state().phase, Phase::Available);

        // Structured notes only for the offered version.
        let entry = |version: &str| ReleaseEntry {
            changes: Default::default(),
            version: v(version),
            published_at: "2026-10-15T00:00:00Z".into(),
            title: "T".into(),
        };
        send(
            &mut u,
            &s,
            Event::Changelog(v("0.3.9"), Some(entry("0.3.9"))),
        );
        assert!(u.release().is_none());
        send(
            &mut u,
            &s,
            Event::Changelog(v("0.4.0"), Some(entry("0.3.9"))),
        );
        assert!(u.release().is_none());
        send(
            &mut u,
            &s,
            Event::Changelog(v("0.4.0"), Some(entry("0.4.0"))),
        );
        assert_eq!(u.release().map(|r| r.version), Some(v("0.4.0")));
    }
}
