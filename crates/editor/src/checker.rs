//! The loader's check of the open package after every step (docs/editor.md): the full
//! load runs on a worker thread over the package with the unsaved documents laid on top
//! ([`Overlay`]); its errors become the problems list.

use canopy_content::{
    Content,
    assets::Files,
    package::{Overlay, Package},
};
use std::sync::{Arc, mpsc};
use std::time::Instant;

/// One problem the loader found.
#[derive(Debug, Clone, PartialEq)]
pub struct Problem {
    /// The document it is in, from the package root, when it names one.
    pub document: Option<String>,
    /// Where in it (a JSON path or an id).
    pub location: String,
    pub message: String,
    /// The file as the loader names it.
    pub file: String,
}

/// The outcome of one check.
#[derive(Debug, Clone)]
pub struct Report {
    /// The store's generation it checked.
    pub generation: u64,
    pub problems: Vec<Problem>,
    pub ms: f32,
}

pub struct Checker {
    package: Package,
    tx: mpsc::Sender<Report>,
    rx: mpsc::Receiver<Report>,
    running: bool,
    /// The newest request while a check runs; older ones are dropped.
    waiting: Option<(u64, Overlay)>,
    report: Option<Report>,
}

impl Checker {
    pub fn new(package: Package) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            package,
            tx,
            rx,
            running: false,
            waiting: None,
            report: None,
        }
    }

    /// Checks the package with `overlay` as generation `generation`.
    pub fn request(&mut self, generation: u64, overlay: Overlay) {
        self.waiting = Some((generation, overlay));
        self.start();
    }

    fn start(&mut self) {
        if self.running {
            return;
        }
        let Some((generation, overlay)) = self.waiting.take() else {
            return;
        };
        self.running = true;
        let package = self.package.clone().with_overlay(Arc::new(overlay));
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new()
            .name("editor check".into())
            .spawn(move || {
                let started = Instant::now();
                let prefix = format!("{}/", package.id());
                let problems =
                    match Content::from_packages(std::slice::from_ref(&package), Files::Check) {
                        Ok(_) => Vec::new(),
                        Err(errors) => errors
                            .0
                            .into_iter()
                            .map(|e| Problem {
                                document: e
                                    .file
                                    .strip_prefix(&prefix)
                                    .filter(|path| path.ends_with(".json"))
                                    .map(str::to_string),
                                location: e.location,
                                message: e.message,
                                file: e.file,
                            })
                            .collect(),
                    };
                let _ = tx.send(Report {
                    generation,
                    problems,
                    ms: started.elapsed().as_secs_f32() * 1000.0,
                });
            });
        if let Err(error) = spawned {
            tracing::warn!(%error, "editor: cannot start a check");
            self.running = false;
        }
    }

    /// Takes finished checks; starts the next waiting one.
    pub fn poll(&mut self) {
        while let Ok(report) = self.rx.try_recv() {
            self.running = false;
            self.report = Some(report);
        }
        self.start();
    }

    /// The latest finished check.
    pub fn report(&self) -> Option<&Report> {
        self.report.as_ref()
    }

    /// Whether a check is running or waiting.
    pub fn busy(&self) -> bool {
        self.running || self.waiting.is_some()
    }
}
