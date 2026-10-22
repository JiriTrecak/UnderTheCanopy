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
    /// Something to settle that does not stop the game loading (the campaign check's
    /// missing art, contradictions): it never blocks Test in game or saving.
    pub warning: bool,
}

/// The outcome of one check.
#[derive(Debug, Clone)]
pub struct Report {
    /// The store's generation it checked.
    pub generation: u64,
    pub problems: Vec<Problem>,
    pub ms: f32,
    /// The loaded game data when the check passed.
    pub content: Option<Arc<Content>>,
}

pub struct Checker {
    package: Package,
    tx: mpsc::Sender<Report>,
    rx: mpsc::Receiver<Report>,
    running: bool,
    /// The running check's thread and generation (a check that panics sends no report).
    worker: Option<(std::thread::JoinHandle<()>, u64)>,
    /// The newest request while a check runs; older ones are dropped.
    waiting: Option<(u64, Overlay, Vec<String>)>,
    report: Option<Report>,
    /// The game data of the last check that passed.
    content: Option<Arc<Content>>,
}

impl Checker {
    pub fn new(package: Package) -> Self {
        let (tx, rx) = mpsc::channel();
        Self {
            package,
            tx,
            rx,
            running: false,
            worker: None,
            waiting: None,
            report: None,
            content: None,
        }
    }

    /// Checks the package with `overlay` as generation `generation`, and the map packages
    /// at `maps` (folders in the package) with it.
    pub fn request(&mut self, generation: u64, overlay: Overlay, maps: Vec<String>) {
        self.waiting = Some((generation, overlay, maps));
        self.start();
    }

    fn start(&mut self) {
        if self.running {
            return;
        }
        let Some((generation, overlay, maps)) = self.waiting.take() else {
            return;
        };
        self.running = true;
        let overlay = Arc::new(overlay);
        let package = self.package.clone().with_overlay(overlay.clone());
        let tx = self.tx.clone();
        let spawned = std::thread::Builder::new()
            .name("editor check".into())
            .spawn(move || {
                let started = Instant::now();
                let prefix = format!("{}/", package.id());
                let mut content = None;
                let problems =
                    match Content::from_packages(std::slice::from_ref(&package), Files::Check) {
                        Ok(loaded) => {
                            content = Some(Arc::new(loaded));
                            Vec::new()
                        }
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
                                warning: false,
                            })
                            .collect(),
                    };
                // The maps with unsaved changes: they load, they follow the content's
                // rules, and their starts are playable.
                let mut problems = problems;
                if let Some(content) = &content {
                    for root in &maps {
                        let document = format!("{root}/map.json");
                        let mut fail = |message: String| {
                            problems.push(Problem {
                                document: Some(document.clone()),
                                location: String::new(),
                                message,
                                file: document.clone(),
                                warning: false,
                            })
                        };
                        let loaded = canopy_content::package::Package::open_overlaid(
                            &package.root.join(root),
                            Some(Arc::new(overlay.below(root))),
                        )
                        .map_err(|e| e.to_string())
                        .and_then(|p| {
                            canopy_content::map::package::load_package(&p)
                                .map_err(|e| e.to_string())
                        });
                        let map = match loaded {
                            Ok(map) => map,
                            Err(error) => {
                                fail(error);
                                continue;
                            }
                        };
                        for error in canopy_content::map::validate(&map, content) {
                            fail(format!("{}: {}", error.location, error.message));
                        }
                        let setup = canopy_scene::spectator_setup(&map, None);
                        match canopy_sim::World::new(content.clone(), &map, setup) {
                            Ok(world) => {
                                if let Err(error) = canopy_sim::spawn::playable(
                                    &map,
                                    world.catalog(),
                                    world.terrain(),
                                ) {
                                    fail(error);
                                }
                                if let Err(error) =
                                    canopy_sim::spawn::connected(&map, world.terrain())
                                {
                                    fail(error);
                                }
                            }
                            Err(error) => fail(error.0),
                        }
                    }
                }
                // What the editor sees wrong with the campaigns.
                problems.extend(crate::campaign::check::problems(
                    &package,
                    content.as_deref(),
                ));
                let _ = tx.send(Report {
                    generation,
                    problems,
                    ms: started.elapsed().as_secs_f32() * 1000.0,
                    content,
                });
            });
        match spawned {
            Ok(handle) => self.worker = Some((handle, generation)),
            Err(error) => {
                tracing::warn!(%error, "editor: cannot start a check");
                self.running = false;
            }
        }
    }

    /// Takes finished checks; starts the next waiting one.
    pub fn poll(&mut self) {
        let finished = self
            .worker
            .as_ref()
            .is_some_and(|(handle, _)| handle.is_finished());
        while let Ok(report) = self.rx.try_recv() {
            self.running = false;
            if let Some(content) = &report.content {
                self.content = Some(content.clone());
            }
            self.report = Some(report);
        }
        // A check that ended without a report panicked: say so, and check again.
        if finished && let Some((handle, generation)) = self.worker.take() {
            let panicked = handle.join().is_err();
            if self.running {
                self.running = false;
                tracing::error!(
                    generation,
                    panicked,
                    "editor: the check ended without a report"
                );
                self.report = Some(Report {
                    generation,
                    problems: vec![Problem {
                        document: None,
                        location: String::new(),
                        message:
                            "The check crashed; the log says where. Edit again to check again."
                                .into(),
                        file: String::new(),
                        warning: false,
                    }],
                    ms: 0.0,
                    content: None,
                });
            }
        }
        self.start();
    }

    /// The latest finished check.
    pub fn report(&self) -> Option<&Report> {
        self.report.as_ref()
    }

    /// The game data of the last check that passed (previews draw from it).
    pub fn content(&self) -> Option<&Arc<Content>> {
        self.content.as_ref()
    }

    /// Whether a check is running or waiting.
    pub fn busy(&self) -> bool {
        self.running || self.waiting.is_some()
    }
}
