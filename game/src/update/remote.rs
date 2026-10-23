//! The real updater backend: the release feed over HTTPS, the package downloaded into the
//! user's cache folder and verified against the updater key and the advertised version,
//! the install over the running bundle and the relaunch.

use super::http::{Http, Progress};
use super::{Backend, Event, Offer, install};
use canopy_release::{BASE_URL, FEED_URL, PUBLIC_KEY, ReleaseEntry, StableFeed, Version};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// The old updater's check timeout.
const CHECK_TIMEOUT: Duration = Duration::from_secs(30);
/// The old What's new timeout for a version's changelog.
const CHANGELOG_TIMEOUT: Duration = Duration::from_secs(10);

pub struct Remote {
    http: Result<Http, String>,
    tx: mpsc::Sender<Event>,
    rx: mpsc::Receiver<Event>,
    installed: Version,
    platform: String,
    /// The running executable (its bundle is what installs replace).
    exe: Option<PathBuf>,
    /// Where packages are downloaded.
    downloads: PathBuf,
}

impl Remote {
    /// A backend for the copy at `installed`, downloading into `downloads`.
    pub fn new(installed: Version, downloads: PathBuf) -> Self {
        let (tx, rx) = mpsc::channel();
        let http = Http::new();
        if let Err(error) = &http {
            tracing::warn!(%error, "the updater's network thread did not start");
        }
        Self {
            http,
            tx,
            rx,
            installed,
            platform: canopy_release::host_platform(),
            exe: std::env::current_exe().ok(),
            downloads,
        }
    }

    fn bundle(&self) -> Result<PathBuf, String> {
        self.exe
            .as_deref()
            .and_then(install::bundle_of)
            .ok_or_else(|| "not running from an app bundle".to_string())
    }
}

/// Reads the feed's answer into the update decision.
fn read_feed(body: &[u8], installed: Version, platform: &str) -> Result<Option<Offer>, String> {
    let feed: StableFeed =
        serde_json::from_slice(body).map_err(|e| format!("the update feed does not read: {e}"))?;
    Ok(super::offer(feed, installed, platform))
}

/// Verifies a downloaded package; removes it if it does not verify.
fn verify(file: &Path, signature: &str, version: Version) -> Result<(), String> {
    let result = std::fs::File::open(file)
        .map_err(|e| e.to_string())
        .and_then(|f| {
            canopy_release::verify_package(
                PUBLIC_KEY,
                signature,
                &version.to_string(),
                std::io::BufReader::new(f),
                |_| {},
            )
            .map_err(|e| e.to_string())
        });
    if result.is_err() {
        let _ = std::fs::remove_file(file);
    }
    result
}

impl Backend for Remote {
    fn check(&mut self) {
        let tx = self.tx.clone();
        let (installed, platform) = (self.installed, self.platform.clone());
        match &self.http {
            Ok(http) => http.get(FEED_URL, CHECK_TIMEOUT, move |body| {
                let result = body.and_then(|b| read_feed(&b, installed, &platform));
                let _ = tx.send(Event::Checked(result));
            }),
            Err(error) => {
                let _ = tx.send(Event::Checked(Err(error.clone())));
            }
        }
    }

    fn changelog(&mut self, version: Version) {
        let Ok(http) = &self.http else {
            return;
        };
        let tx = self.tx.clone();
        let url = format!("{BASE_URL}/releases/{version}/changelog.json");
        http.get(&url, CHANGELOG_TIMEOUT, move |body| {
            // The feed's plain notes remain when the changelog is missing or bad.
            let entry = body
                .ok()
                .and_then(|b| serde_json::from_slice::<ReleaseEntry>(&b).ok());
            let _ = tx.send(Event::Changelog(version, entry));
        });
    }

    fn download(&mut self, offer: &Offer) {
        let tx = self.tx.clone();
        let fail = |message: String| {
            let _ = tx.send(Event::Downloaded(Err(message)));
        };
        let Some(package) = offer.package.clone() else {
            return fail(format!("no package for {}", self.platform));
        };
        if !package.url.starts_with("https://") {
            return fail(format!(
                "refusing a package not served over HTTPS: {}",
                package.url
            ));
        }
        let Ok(http) = &self.http else {
            return fail("the network thread is not running".into());
        };
        let version = offer.version;
        let file = self
            .downloads
            .join(format!("canopy-{version}-{}.tar.gz", self.platform));
        let progress_tx = tx.clone();
        let progress = move |p: Progress| {
            let _ = progress_tx.send(match p {
                Progress::Started(total) => Event::Started(total),
                Progress::Received(n) => Event::Received(n),
            });
        };
        let done_file = file.clone();
        http.download(&package.url, file, progress, move |result| {
            if let Err(error) = result {
                let _ = tx.send(Event::Downloaded(Err(error)));
                return;
            }
            // Hashing a large package takes a moment: off the network thread.
            let spawned = std::thread::Builder::new()
                .name("canopy-verify".into())
                .spawn({
                    let tx = tx.clone();
                    let file = done_file.clone();
                    move || {
                        let result = verify(&file, &package.signature, version).map(|()| file);
                        let _ = tx.send(Event::Downloaded(result));
                    }
                });
            if let Err(error) = spawned {
                let _ = std::fs::remove_file(&done_file);
                let _ = tx.send(Event::Downloaded(Err(error.to_string())));
            }
        });
    }

    fn install(&mut self, package: &Path) {
        let tx = self.tx.clone();
        let bundle = match self.bundle() {
            Ok(bundle) => bundle,
            Err(error) => {
                let _ = tx.send(Event::Installed(Err(error)));
                return;
            }
        };
        let package = package.to_path_buf();
        let spawned = std::thread::Builder::new()
            .name("canopy-install".into())
            .spawn({
                let tx = tx.clone();
                move || {
                    let result = install::install_app_archive(&package, &bundle);
                    if result.is_ok() {
                        let _ = std::fs::remove_file(&package);
                    }
                    let _ = tx.send(Event::Installed(result));
                }
            });
        if let Err(error) = spawned {
            let _ = tx.send(Event::Installed(Err(error.to_string())));
        }
    }

    fn relaunch(&mut self) -> Result<(), String> {
        install::relaunch(&self.bundle()?)
    }

    fn blocker(&self, offer: &Offer) -> Option<String> {
        if offer.package.is_none() {
            return Some(tr!("update.no-package"));
        }
        install::blocker(self.exe.as_deref().unwrap_or(Path::new("")))
    }

    fn poll(&mut self) -> Option<Event> {
        self.rx.try_recv().ok()
    }
}

/// A backend that never reaches the network (offscreen captures, benchmarks): checks
/// stay pending.
pub struct Offline;

impl Backend for Offline {
    fn check(&mut self) {}
    fn changelog(&mut self, _: Version) {}
    fn download(&mut self, _: &Offer) {}
    fn install(&mut self, _: &Path) {}
    fn relaunch(&mut self) -> Result<(), String> {
        Err("offline".into())
    }
    fn blocker(&self, _: &Offer) -> Option<String> {
        None
    }
    fn poll(&mut self) -> Option<Event> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_feed_answer_decides() {
        let body = br#"{"version":"0.3.1","notes":"n","pub_date":"2026-10-15T00:00:00Z",
            "platforms":{"darwin-aarch64":{"url":"https://x/p","signature":"c2ln"}}}"#;
        let installed = "0.3.0".parse().unwrap();
        let offer = read_feed(body, installed, "darwin-aarch64")
            .unwrap()
            .unwrap();
        assert_eq!(offer.version.to_string(), "0.3.1");
        assert!(offer.package.is_some());
        assert!(
            read_feed(body, "0.3.1".parse().unwrap(), "darwin-aarch64")
                .unwrap()
                .is_none()
        );
        // A broken feed is a failed check, never "latest".
        assert!(read_feed(b"<html>", installed, "darwin-aarch64").is_err());
        assert!(read_feed(br#"{"version":"x"}"#, installed, "darwin-aarch64").is_err());
    }
}
