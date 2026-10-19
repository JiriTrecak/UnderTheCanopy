//! Loading a match on a worker thread while the loading screen shows its progress.
//!
//! The worker reads the map, builds (or restores) the world and its session, the static
//! scene and the presenter ([`crate::world::prepare`]); the window thread then uploads
//! the scene to the GPU and starts the simulation thread. Leaving the loading screen
//! drops the job: the worker finishes on its own and its result is discarded.

use crate::{
    data::GameData,
    paths::Paths,
    world::{self, MatchRequest, MatchSpec, Prepared, Stage},
};
use anyhow::Result;
use std::sync::{Arc, mpsc};

/// What to load.
pub enum Job {
    Match(Box<MatchRequest>),
    /// A development view (`--map`, `--spectate` with their options).
    Dev(DevSpec),
}

/// The owned form of [`MatchSpec`] for development views.
#[derive(Debug, Clone)]
pub struct DevSpec {
    pub map: String,
    pub hour: Option<f32>,
    pub players: Option<usize>,
    pub biome: Option<String>,
    pub crowd: Option<u32>,
    pub colors: Option<Vec<usize>>,
    pub races: Option<Vec<String>>,
    /// The local player's slot (`--map`); `None` spectates (`--spectate`).
    pub human: Option<u8>,
    /// Trying an ability with this unit (the editor's encounter).
    pub encounter: Option<String>,
}

enum Message {
    Progress(Stage, f32),
    Done(Box<Result<Prepared>>),
}

/// A load in progress.
pub struct LoadJob {
    rx: mpsc::Receiver<Message>,
    pub stage: Stage,
    pub progress: f32,
}

impl LoadJob {
    pub fn spawn(paths: Paths, data: Arc<GameData>, job: Job) -> Self {
        let (tx, rx) = mpsc::channel();
        std::thread::Builder::new()
            .name("match-loader".into())
            .spawn(move || {
                let progress_tx = tx.clone();
                let progress = move |stage: Stage, value: f32| {
                    let _ = progress_tx.send(Message::Progress(stage, value));
                };
                let result = match job {
                    Job::Match(request) => world::prepare(&paths, &data, *request, &progress),
                    Job::Dev(spec) => dev(&paths, &data, &spec, &progress),
                };
                let _ = tx.send(Message::Done(Box::new(result)));
            })
            .expect("the loader thread starts");
        Self {
            rx,
            stage: Stage::Map,
            progress: 0.0,
        }
    }

    /// The prepared match once the worker is done.
    pub fn poll(&mut self) -> Option<Result<Prepared>> {
        while let Ok(message) = self.rx.try_recv() {
            match message {
                Message::Progress(stage, value) => {
                    self.stage = stage;
                    self.progress = value;
                }
                Message::Done(result) => return Some(*result),
            }
        }
        None
    }
}

fn dev(
    paths: &Paths,
    data: &GameData,
    spec: &DevSpec,
    progress: &dyn Fn(Stage, f32),
) -> Result<Prepared> {
    progress(Stage::World, 0.3);
    let (loaded, game) = world::load(
        paths,
        &MatchSpec {
            map: &spec.map,
            hour: spec.hour,
            players: spec.players,
            biome: spec.biome.as_deref(),
            crowd: spec.crowd,
            gallery: false,
            only: None,
            colors: spec.colors.clone(),
            races: spec.races.clone(),
            human: spec.human,
            content: Some(data.content.clone()),
            encounter: spec.encounter.as_deref(),
        },
    )?;
    progress(Stage::Gpu, 0.85);
    let entry = data.map(&spec.map);
    let map_identity = entry.and_then(|m| m.identity().ok()).unwrap_or_default();
    Ok(Prepared {
        loaded,
        game,
        map_name: entry.map_or_else(|| spec.map.clone(), |m| m.name.clone()),
        map_identity,
        objectives: Vec::new(),
    })
}
