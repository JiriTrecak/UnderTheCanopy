//! Follows the package's files on disk (docs/editor.md): documents changed by an agent,
//! git or another editor reload, or become conflicts when they have unsaved edits.

use notify::{EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;

pub struct Watch {
    root: PathBuf,
    rx: mpsc::Receiver<notify::Result<notify::Event>>,
    /// Kept alive: dropping it stops the watch.
    _watcher: RecommendedWatcher,
}

impl Watch {
    /// Watches the package folder `root`; `None` (logged) when the system refuses.
    pub fn new(root: &Path) -> Option<Self> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let (tx, rx) = mpsc::channel();
        let started = notify::recommended_watcher(move |event| {
            let _ = tx.send(event);
        })
        .and_then(|mut watcher| {
            watcher.watch(&root, RecursiveMode::Recursive)?;
            Ok(watcher)
        });
        match started {
            Ok(watcher) => Some(Self {
                root,
                rx,
                _watcher: watcher,
            }),
            Err(error) => {
                tracing::warn!(%error, "editor: cannot watch the package's files");
                None
            }
        }
    }

    /// Documents (JSON files, `/`-separated from the package root) changed since the last
    /// call.
    pub fn changed(&self) -> BTreeSet<String> {
        let mut out = BTreeSet::new();
        while let Ok(event) = self.rx.try_recv() {
            let Ok(event) = event else {
                continue;
            };
            if matches!(event.kind, EventKind::Access(_)) {
                continue;
            }
            for path in event.paths {
                let Ok(relative) = path.strip_prefix(&self.root) else {
                    continue;
                };
                let hidden = relative
                    .components()
                    .any(|c| c.as_os_str().to_string_lossy().starts_with('.'));
                if hidden || relative.extension().is_none_or(|e| e != "json") {
                    continue;
                }
                let parts: Vec<_> = relative
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy().into_owned())
                    .collect();
                out.insert(parts.join("/"));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, Instant};

    #[test]
    fn reports_changed_documents_by_package_path() {
        let root = std::env::temp_dir().join(format!("canopy-watch-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("units/ants")).unwrap();
        let watch = Watch::new(&root).expect("watching works here");
        std::fs::write(root.join("units/ants/worker.json"), "{}").unwrap();
        std::fs::write(root.join("units/ants/notes.txt"), "not a document").unwrap();
        let start = Instant::now();
        let mut seen = BTreeSet::new();
        while !seen.contains("units/ants/worker.json") && start.elapsed() < Duration::from_secs(5) {
            seen.extend(watch.changed());
            std::thread::sleep(Duration::from_millis(20));
        }
        assert!(seen.contains("units/ants/worker.json"), "{seen:?}");
        assert!(!seen.iter().any(|p| p.ends_with(".txt")));
        let _ = std::fs::remove_dir_all(&root);
    }
}
