//! The simulation thread (D3): `canopy_sim::World` steps every 25 ms on its own thread,
//! fed by a `canopy_net::Session` (local for single-player and spectating, so the same
//! path carries multiplayer later), and each tick is presented
//! (`canopy_scene::live::Presenter`) and published to the render thread.
//!
//! **Schedule.** Tick `n` belongs to simulation time `epoch + n · 25 ms`. The thread
//! computes it one tick early (from `epoch + (n − 1) · 25 ms`), so a tick's cost is hidden
//! from the picture, and never earlier: the match never runs faster than real time. When
//! it falls behind by more than [`MAX_BACKLOG`] ticks it catches up at full speed up to
//! that backlog and moves the epoch for the rest (the match slows down rather than
//! skipping ticks; results do not depend on timing).
//!
//! **Hand-off.** The newest [`RING`] snapshots sit in a ring behind a mutex the
//! simulation holds only to push a pointer; the render thread looks at an atomic tick
//! counter and `try_lock`s, keeping what it has when the lock is busy, so it never waits
//! for the simulation.
//!
//! **Render clock.** The picture shows simulation time minus one tick (D24, Q15):
//! [`render_tick`] from the snapshot's epoch, then two-snapshot interpolation.

use canopy_content::placement::Owner;
use canopy_net::Session;
use canopy_scene::live::{Presentation, Presenter};
use canopy_sim::{World, view::View};
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
        mpsc,
    },
    thread::JoinHandle,
    time::{Duration, Instant},
};

/// Length of a simulation tick (D3).
pub const TICK: Duration = Duration::from_millis(25);
/// Ticks the thread computes ahead of their simulation time.
pub const LEAD: u32 = 1;
/// Ticks of backlog the thread catches up at full speed before it moves the schedule.
pub const MAX_BACKLOG: u32 = 8;
/// Snapshots the render thread can reach.
pub const RING: usize = 4;

/// What one tick cost on the simulation thread, milliseconds.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct TickCost {
    pub step: f32,
    pub view: f32,
    pub present: f32,
}

impl TickCost {
    pub fn total(&self) -> f32 {
        self.step + self.view + self.present
    }
}

/// Whose knowledge the thread presents: a player's (fog, private state of its own
/// entities) or everything (spectators).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Seat {
    Observer,
    /// Zero-based player slot.
    Player(u8),
}

impl Seat {
    /// The seat's view of the world as it stands.
    pub fn view(self, world: &World) -> View {
        match self {
            Self::Observer => world.observer_view(),
            Self::Player(slot) => world.view(Owner::Player(slot)),
        }
    }
}

/// Ticks between AI summaries for the debug overlay.
pub const AI_SUMMARY_TICKS: u32 = 10;

/// One presented tick.
#[derive(Debug)]
pub struct Snapshot {
    pub presentation: Presentation,
    /// The seat's view the presentation was made from: the HUD's input.
    pub view: Arc<View>,
    /// What the computer players are doing (debug overlay), when asked for: refreshed
    /// every [`AI_SUMMARY_TICKS`].
    pub ai: Option<Arc<Vec<canopy_sim::ai::Summary>>>,
    /// The seat's own units held inside another entity or waiting to be placed back
    /// (sorted ids, from the view's private flags).
    pub held: Arc<Vec<u32>>,
    /// The entities a mission's camera shot names (old worker `targets`), while a scene
    /// with a shot plays.
    pub scene: Option<SceneTargets>,
    /// Every player's colony, for the observer panel (observers only; refreshed every
    /// [`AI_SUMMARY_TICKS`]).
    pub colonies: Option<Arc<Vec<ColonyStats>>>,
    /// Simulation time of tick `origin` under the schedule this tick was computed on.
    pub epoch: Instant,
    pub origin: u32,
    /// Real time per tick under this schedule: the tick length over the match speed.
    pub tick_length: Duration,
    pub cost: TickCost,
    /// Times the schedule has moved because the simulation fell behind.
    pub slowdowns: u32,
    /// The match outcome is decided.
    pub decided: bool,
}

impl Snapshot {
    pub fn tick(&self) -> u32 {
        self.presentation.tick
    }
}

/// The ring the two threads share.
#[derive(Default)]
pub struct Mailbox {
    ring: Mutex<VecDeque<Arc<Snapshot>>>,
    /// The newest published tick + 1 (0: none yet).
    latest: AtomicU32,
}

impl Mailbox {
    /// Publishes a snapshot (simulation side): holds the lock for a pointer push.
    pub fn publish(&self, snapshot: Arc<Snapshot>) {
        let tick = snapshot.tick();
        {
            let mut ring = self.ring.lock().unwrap_or_else(|e| e.into_inner());
            if ring.len() == RING {
                ring.pop_front();
            }
            ring.push_back(snapshot);
        }
        self.latest.store(tick + 1, Ordering::Release);
    }

    /// The newest published tick, without locking.
    pub fn latest(&self) -> Option<u32> {
        self.latest.load(Ordering::Acquire).checked_sub(1)
    }

    /// Copies the ring into `out` (oldest first) unless the lock is busy; never waits.
    pub fn take(&self, out: &mut Vec<Arc<Snapshot>>) -> bool {
        let Ok(ring) = self.ring.try_lock() else {
            return false;
        };
        out.clear();
        out.extend(ring.iter().cloned());
        true
    }
}

/// The render tick at `now`: simulation time in ticks, one tick behind.
pub fn render_tick(snapshot: &Snapshot, now: Instant) -> f64 {
    let length = snapshot.tick_length.as_secs_f64();
    if length <= 0.0 {
        return f64::from(snapshot.tick());
    }
    let since = if now >= snapshot.epoch {
        (now - snapshot.epoch).as_secs_f64()
    } else {
        -(snapshot.epoch - now).as_secs_f64()
    };
    f64::from(snapshot.origin) + since / length - 1.0
}

/// A running match.
pub struct SimThread {
    mailbox: Arc<Mailbox>,
    stop: Arc<AtomicBool>,
    /// The debug overlay wants AI summaries.
    want_ai: Arc<AtomicBool>,
    handle: Option<JoinHandle<()>>,
    controls: mpsc::Sender<Control>,
    notices: mpsc::Receiver<Notice>,
    live: Arc<Mutex<LiveStatus>>,
}

/// What the window thread asks of a running match.
pub enum Control {
    /// Holds the match still (single-player; multiplayer has no pause, D32).
    Pause(bool),
    /// Replies with the world's save between two ticks.
    Save(mpsc::Sender<canopy_sim::Save>),
    /// A chat line (networked matches).
    Chat(String),
    /// Match speed (debug overlay; single player only): simulation ticks per real tick
    /// length. Only the pacing changes, never the results.
    Speed(f32),
    /// Whose view the match presents (debug overlay; single player only): another seat
    /// (an observer reveals the map), or `None` for the seat's own. The HUD and commands
    /// keep the seat's view.
    Shown(Option<Seat>),
}

/// The match speeds the debug overlay offers (old 1–4×, and half speed).
pub const SPEEDS: [f32; 5] = [0.5, 1.0, 2.0, 3.0, 4.0];

/// What a running match tells the window thread.
#[derive(Debug, Clone)]
pub enum Notice {
    Net(canopy_net::Event),
    /// The outcome was decided at `tick`.
    Decided {
        outcome: canopy_sim::state::Outcome,
        tick: u32,
    },
}

/// The match's connection and pause state, updated every loop of the thread.
#[derive(Debug, Clone)]
pub struct LiveStatus {
    pub status: canopy_net::Status,
    pub paused: bool,
    pub tick: u32,
}

/// What a networked seat reports once its world is built (`ready`).
pub struct Ready {
    pub content: canopy_net::ContentIdentity,
    pub build: String,
}

/// The wire form of an outcome: zero-based player slots.
pub fn wire_outcome(outcome: &canopy_sim::state::Outcome) -> canopy_net::wire::WireOutcome {
    let player = |o: &Owner| match o {
        Owner::Player(p) => Some(*p),
        Owner::Nobody => None,
    };
    canopy_net::wire::WireOutcome {
        winner: outcome.winner.as_ref().and_then(player),
        defeated: outcome.defeated.iter().filter_map(player).collect(),
    }
}

/// Whether a unit is held inside another entity or waiting to be placed back (the
/// presenter asks for every entity; a view carries it for owned ones only).
pub fn held(world: &World, id: canopy_sim::entity::EntityId) -> bool {
    world
        .state()
        .entities
        .get(id)
        .and_then(|e| e.unit.as_ref())
        .is_some_and(|u| u.contained.is_some() || u.release.is_some())
}

/// A presented tick before it is published.
pub struct Advanced {
    pub presentation: Presentation,
    pub view: Arc<View>,
    pub held: Arc<Vec<u32>>,
    pub scene: Option<SceneTargets>,
    pub cost: TickCost,
    pub decided: bool,
}

/// An entity a mission's camera shot names, as it stands.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct SceneTarget {
    pub id: u32,
    pub definition: canopy_sim::catalog::Def,
    /// Cells (x, z).
    pub position: [f32; 2],
}

/// The shot's subject and the unit it looks at, resolved from their placement ids to
/// the entities they made (old worker `targets`: from the whole state, so a shot can
/// frame units the player has not seen; the scene reveals its set anyway).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct SceneTargets {
    pub subject: Option<SceneTarget>,
    pub look_at: Option<SceneTarget>,
}

/// The targets of the camera shot `view`'s mission scene names; `None` without a shot.
pub fn scene_targets(world: &World, view: &View) -> Option<SceneTargets> {
    let camera = view.mission.as_ref()?.scene.as_ref()?.camera.as_ref()?;
    let find = |placement: &str| {
        world
            .state()
            .entities
            .iter()
            .find(|e| e.placement.as_deref() == Some(placement))
            .map(|e| SceneTarget {
                id: e.id.0,
                definition: e.definition,
                position: [e.position.x as f32 / 1000.0, e.position.y as f32 / 1000.0],
            })
    };
    Some(SceneTargets {
        subject: find(&camera.entity),
        look_at: camera.look_at.as_deref().and_then(find),
    })
}

/// How far around a mission scene the picture reveals (old `PresentationView`: past a
/// hero's sight, so framing a speaker shows something new), cells.
pub const SCENE_REVEAL_RADIUS: f64 = 36.0;

/// The view a mission scene is shown with (old `PresentationView`): within
/// [`SCENE_REVEAL_RADIUS`] of the scene every cell is in sight and every entity shown,
/// without exploring anything or telling the player's knowledge (the HUD, commands and
/// the AI keep the plain view). `None` when no scene plays, the match is decided or the
/// viewer has no fog.
pub fn stage(world: &World, view: &View) -> Option<View> {
    let scene = view.mission.as_ref()?.scene.as_ref()?;
    if view.outcome.is_some() {
        return None;
    }
    let fog = view.fog.as_ref()?;
    let (sx, sy) = (
        f64::from(scene.position.x) / 1000.0,
        f64::from(scene.position.y) / 1000.0,
    );
    let r = SCENE_REVEAL_RADIUS;
    let size = i64::from(fog.size);
    let near = |x: i64, y: i64| ((x as f64 - sx).hypot(y as f64 - sy)) <= r;
    let mut cells = (*fog.cells).clone();
    let (y0, y1) = (
        ((sy - r).floor() as i64).max(0),
        ((sy + r) as i64).min(size - 1),
    );
    let (x0, x1) = (
        ((sx - r).floor() as i64).max(0),
        ((sx + r) as i64).min(size - 1),
    );
    for y in y0..=y1 {
        for x in x0..=x1 {
            if near(x, y) {
                cells[(y * size + x) as usize] = canopy_sim::vision::VISIBLE;
            }
        }
    }
    let ground = (size * size) as usize;
    for (i, &cell) in fog.deck_cells.iter().enumerate() {
        let cell = i64::from(cell);
        if near(cell % size, cell / size)
            && let Some(node) = cells.get_mut(ground + i)
        {
            *node = canopy_sim::vision::VISIBLE;
        }
    }
    // A revision of its own, above any the simulation reaches, for every staged state:
    // the renderer reloads the staged fog, and the plain one again afterwards.
    let key = u64::from(fog.revision)
        ^ (i64::from(scene.position.x) as u64).rotate_left(21)
        ^ (i64::from(scene.position.y) as u64).rotate_left(42);
    let revision = 0x8000_0000 | (key.wrapping_mul(0x9e37_79b9_7f4a_7c15) >> 33) as u32;
    let mut staged = view.clone();
    staged.fog = Some(fog.restaged(cells, revision));
    // Every entity on the set, live (old: the full view's entities replace known ones).
    let all = world.observer_view();
    let mut entities: std::collections::BTreeMap<_, _> =
        staged.entities.drain(..).map(|e| (e.id(), e)).collect();
    for e in all.entities {
        let (x, y) = (
            f64::from(e.public.position.x) / 1000.0,
            f64::from(e.public.position.y) / 1000.0,
        );
        if (x - sx).hypot(y - sy) <= r {
            entities.insert(e.id(), e);
        }
    }
    staged.entities = entities.into_values().collect();
    Some(staged)
}

/// The view's own units that are held inside another entity or waiting to be placed
/// back, sorted.
pub fn held_units(view: &View) -> Vec<u32> {
    view.entities
        .iter()
        .filter(|e| e.motion.is_some())
        .filter(|e| {
            e.private
                .as_ref()
                .is_some_and(|p| p.contained || p.releasing)
        })
        .map(|e| e.id().0)
        .collect()
}

/// A hero in the observer panel (old `ObserverPlayerStats.heroes`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HeroStats {
    pub id: u32,
    pub definition: canopy_sim::catalog::Def,
    pub level: u32,
    pub status: HeroStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HeroStatus {
    Alive,
    Fallen,
    /// Fallen and queued for revival at a building.
    Reviving,
}

impl HeroStatus {
    pub fn name(self) -> &'static str {
        match self {
            Self::Alive => "alive",
            Self::Fallen => "fallen",
            Self::Reviving => "reviving",
        }
    }
}

/// One player's row of the observer panel (old `observerStats`): the colony's goods,
/// supply and population, and the counts the old panel made from the whole state
/// (units inside buildings included, fallen heroes excluded).
#[derive(Debug, Clone, PartialEq)]
pub struct ColonyStats {
    pub player: u8,
    pub colony: canopy_sim::economy::view::Colony,
    pub units: u32,
    pub workers: u32,
    pub army: u32,
    /// By id.
    pub heroes: Vec<HeroStats>,
}

/// Every slot's row of the observer panel (observers only: it reads the whole state).
pub fn colonies(world: &World) -> Vec<ColonyStats> {
    use canopy_content::game::SelectionClass;
    let state = world.state();
    let catalog = world.catalog();
    let reviving: std::collections::BTreeSet<canopy_sim::entity::EntityId> = state
        .entities
        .iter()
        .filter(|e| canopy_sim::abilities::actor::alive(e))
        .flat_map(|e| {
            e.revival
                .iter()
                .flat_map(|r| r.queue.iter().map(|q| q.hero))
        })
        .collect();
    let mut rows: Vec<ColonyStats> = world
        .setup()
        .slots
        .iter()
        .map(|s| ColonyStats {
            player: s.player,
            colony: canopy_sim::economy::view::colony(
                state,
                catalog,
                world.economy(),
                Owner::Player(s.player),
            ),
            units: 0,
            workers: 0,
            army: 0,
            heroes: Vec::new(),
        })
        .collect();
    rows.sort_by_key(|r| r.player);
    for e in state.entities.iter() {
        let Owner::Player(player) = e.owner else {
            continue;
        };
        let Some(row) = rows.iter_mut().find(|r| r.player == player) else {
            continue;
        };
        if e.unit.is_none() {
            continue;
        }
        let d = catalog.get(e.definition);
        let living = canopy_sim::abilities::actor::alive(e);
        if living {
            row.units += 1;
            if d.behaviors.work.is_some() {
                row.workers += 1;
            }
            if d.selection_class == Some(SelectionClass::Army) {
                row.army += 1;
            }
        }
        if d.is_hero() && (living || e.fallen) {
            row.heroes.push(HeroStats {
                id: e.id.0,
                definition: e.definition,
                level: canopy_sim::stats::level(catalog, e),
                status: if living {
                    HeroStatus::Alive
                } else if reviving.contains(&e.id) {
                    HeroStatus::Reviving
                } else {
                    HeroStatus::Fallen
                },
            });
        }
    }
    for row in &mut rows {
        row.heroes.sort_by_key(|h| h.id);
    }
    rows
}

/// Steps `world` once through `session` and presents the tick for `seat`; `None` while
/// the session waits for the next beat. `shown` presents another seat's view instead
/// (the debug overlay's reveal and fog perspective, single player only); the HUD keeps
/// the seat's own view.
pub fn advance(
    world: &mut World,
    session: &mut Session,
    presenter: &mut Presenter,
    seat: Seat,
    shown: Option<Seat>,
) -> Option<Advanced> {
    let started = Instant::now();
    session.advance(world)?;
    let stepped = Instant::now();
    let view = {
        let _span = tracing::trace_span!("seat_view").entered();
        seat.view(world)
    };
    // Another seat's view (debug), else a mission scene's staging.
    let other = match shown.filter(|&s| s != seat) {
        Some(s) => Some(s.view(world)),
        None => stage(world, &view),
    };
    let viewed = Instant::now();
    let presentation = presenter.update(world, other.as_ref().unwrap_or(&view), &|id| {
        held(world, id)
    });
    let presented = Instant::now();
    let ms = |a: Instant, b: Instant| (b - a).as_secs_f32() * 1000.0;
    Some(Advanced {
        held: Arc::new(held_units(&view)),
        scene: scene_targets(world, &view),
        presentation,
        decided: view.outcome.is_some(),
        view: Arc::new(view),
        cost: TickCost {
            step: ms(started, stepped),
            view: ms(stepped, viewed),
            present: ms(viewed, presented),
        },
    })
}

/// The presentation of the world as it stands for `seat` (before its first step, or a
/// shot), with the view it was made from.
pub fn present(world: &World, presenter: &mut Presenter, seat: Seat) -> (Presentation, Arc<View>) {
    let view = seat.view(world);
    let staged = stage(world, &view);
    let presentation = presenter.update(world, staged.as_ref().unwrap_or(&view), &|id| {
        held(world, id)
    });
    (presentation, Arc::new(view))
}

impl SimThread {
    /// Starts a match: first fast-forwards to `start_tick` (presenting every tick), then
    /// runs at `tick_length` per tick (zero: as fast as possible), presenting for `seat`.
    /// A networked seat reports `ready` once its world is built, and its outcome to the
    /// server once decided (D32).
    pub fn start(
        mut world: World,
        mut session: Session,
        mut presenter: Presenter,
        tick_length: Duration,
        start_tick: u32,
        seat: Seat,
        ready: Option<Ready>,
    ) -> Self {
        let mailbox = Arc::new(Mailbox::default());
        let stop = Arc::new(AtomicBool::new(false));
        let want_ai = Arc::new(AtomicBool::new(false));
        let (control_tx, controls) = mpsc::channel::<Control>();
        let (notice_tx, notices) = mpsc::channel::<Notice>();
        let live = Arc::new(Mutex::new(LiveStatus {
            status: session.status(),
            paused: false,
            tick: world.tick(),
        }));
        let handle = {
            let mailbox = mailbox.clone();
            let stop = stop.clone();
            let want_ai = want_ai.clone();
            let live = live.clone();
            std::thread::Builder::new()
                .name("simulation".into())
                .spawn(move || {
                    let _span = tracing::info_span!("simulation_thread").entered();
                    if let Some(ready) = ready {
                        session.ready(ready.content, &ready.build, world.tick());
                    }
                    let started = Instant::now();
                    while world.tick() < start_tick && !stop.load(Ordering::Relaxed) {
                        if advance(&mut world, &mut session, &mut presenter, seat, None).is_none() {
                            std::thread::sleep(Duration::from_millis(1));
                        }
                    }
                    if start_tick > 0 {
                        tracing::info!(
                            tick = world.tick(),
                            ms = started.elapsed().as_millis() as u64,
                            "fast-forwarded"
                        );
                    }
                    let (first, first_view) = present(&world, &mut presenter, seat);
                    let first_held = Arc::new(held_units(&first_view));
                    let first_scene = scene_targets(&world, &first_view);
                    let tick0 = world.tick();
                    let mut ai: Option<Arc<Vec<canopy_sim::ai::Summary>>> = None;
                    let observing = seat == Seat::Observer;
                    let mut colony_list = observing.then(|| Arc::new(colonies(&world)));
                    // Schedule: simulation time of tick n is epoch + (n − origin) · pace.
                    // A speed change moves the origin to the newest tick, so the
                    // schedule stays continuous.
                    let mut epoch = Instant::now();
                    let mut origin = tick0;
                    let mut pace = tick_length;
                    let mut shown: Option<Seat> = None;
                    let mut slowdowns = 0;
                    let networked = session.room().is_some();
                    let mut paused_at: Option<Instant> = None;
                    let mut reported = false;
                    let first_decided = first_view.outcome.is_some();
                    mailbox.publish(Arc::new(Snapshot {
                        presentation: first,
                        view: first_view,
                        ai: None,
                        held: first_held,
                        scene: first_scene,
                        colonies: colony_list.clone(),
                        epoch,
                        origin: tick0,
                        tick_length,
                        cost: TickCost::default(),
                        slowdowns,
                        decided: first_decided,
                    }));
                    while !stop.load(Ordering::Relaxed) {
                        // Controls from the window thread.
                        while let Ok(control) = controls.try_recv() {
                            match control {
                                Control::Pause(pause) if !networked => {
                                    match (pause, paused_at) {
                                        (true, None) => paused_at = Some(Instant::now()),
                                        (false, Some(at)) => {
                                            // Resume where the match stopped: no catch-up.
                                            epoch += at.elapsed();
                                            paused_at = None;
                                        }
                                        _ => {}
                                    }
                                }
                                Control::Pause(_) => {}
                                Control::Save(reply) => {
                                    let _ = reply.send(world.save());
                                }
                                Control::Chat(text) => session.chat(&text),
                                Control::Speed(speed)
                                    if !networked && !tick_length.is_zero() && speed > 0.0 =>
                                {
                                    let now_tick = world.tick();
                                    epoch += pace * (now_tick - origin);
                                    origin = now_tick;
                                    pace = tick_length.div_f32(speed);
                                }
                                Control::Shown(seat) if !networked => shown = seat,
                                Control::Speed(_) | Control::Shown(_) => {}
                            }
                        }
                        for event in session.poll() {
                            let _ = notice_tx.send(Notice::Net(event));
                        }
                        if let Ok(mut l) = live.try_lock() {
                            l.status = session.status();
                            l.paused = paused_at.is_some();
                            l.tick = world.tick();
                        }
                        if paused_at.is_some() {
                            std::thread::sleep(Duration::from_millis(5));
                            continue;
                        }
                        let next = world.tick() + 1;
                        let ahead = (next - origin).saturating_sub(LEAD);
                        let due = epoch + pace * ahead;
                        let now = Instant::now();
                        if now < due {
                            std::thread::sleep((due - now).min(Duration::from_millis(5)));
                            continue;
                        }
                        if !pace.is_zero() {
                            let behind = ((now - due).as_secs_f64() / pace.as_secs_f64()) as u32;
                            if behind > MAX_BACKLOG {
                                epoch += pace * (behind - MAX_BACKLOG);
                                slowdowns += 1;
                            }
                        }
                        let Some(Advanced {
                            presentation,
                            view,
                            held,
                            scene,
                            cost,
                            decided,
                        }) = advance(&mut world, &mut session, &mut presenter, seat, shown)
                        else {
                            std::thread::sleep(Duration::from_millis(1));
                            continue;
                        };
                        if let (Some(outcome), false) = (&view.outcome, reported) {
                            reported = true;
                            let _ = notice_tx.send(Notice::Decided {
                                outcome: outcome.clone(),
                                tick: world.tick(),
                            });
                            if networked {
                                session.report_outcome(
                                    wire_outcome(outcome),
                                    world.tick(),
                                    world.fingerprint(),
                                );
                            }
                        }
                        if !want_ai.load(Ordering::Relaxed) {
                            ai = None;
                        } else if ai.is_none() || world.tick().is_multiple_of(AI_SUMMARY_TICKS) {
                            ai = Some(Arc::new(world.ai_summaries()));
                        }
                        if observing && world.tick().is_multiple_of(AI_SUMMARY_TICKS) {
                            colony_list = Some(Arc::new(colonies(&world)));
                        }
                        mailbox.publish(Arc::new(Snapshot {
                            presentation,
                            view,
                            ai: ai.clone(),
                            held,
                            scene,
                            colonies: colony_list.clone(),
                            epoch,
                            origin,
                            tick_length: pace,
                            cost,
                            slowdowns,
                            decided,
                        }));
                    }
                })
                .expect("the simulation thread starts")
        };
        Self {
            mailbox,
            stop,
            want_ai,
            handle: Some(handle),
            controls: control_tx,
            notices,
            live,
        }
    }

    /// Sends a control to the simulation thread.
    pub fn control(&self, control: Control) {
        let _ = self.controls.send(control);
    }

    /// What happened since the last call (network events, the decision).
    pub fn notices(&self) -> Vec<Notice> {
        self.notices.try_iter().collect()
    }

    /// The session's status and the pause, as of the thread's last loop.
    pub fn live(&self) -> LiveStatus {
        self.live
            .lock()
            .map(|l| l.clone())
            .unwrap_or_else(|e| e.into_inner().clone())
    }

    /// The world's save, taken between ticks (waits for the thread at most `timeout`).
    pub fn save(&self, timeout: Duration) -> Option<canopy_sim::Save> {
        let (tx, rx) = mpsc::channel();
        self.control(Control::Save(tx));
        rx.recv_timeout(timeout).ok()
    }

    pub fn mailbox(&self) -> &Mailbox {
        &self.mailbox
    }

    /// Asks for (or stops) AI summaries in the snapshots.
    pub fn want_ai_summaries(&self, on: bool) {
        self.want_ai.store(on, Ordering::Relaxed);
    }

    /// Whether the thread has stopped (it only stops when asked, or by panicking).
    pub fn finished(&self) -> bool {
        self.handle.as_ref().is_none_or(JoinHandle::is_finished)
    }
}

impl Drop for SimThread {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.handle.take()
            && handle.join().is_err()
        {
            tracing::error!("the simulation thread panicked");
        }
    }
}

#[cfg(test)]
#[path = "../../crates/sim/tests/ai_fixture/mod.rs"]
#[allow(clippy::all)]
pub(crate) mod ai_fixture;

#[cfg(test)]
#[path = "../../crates/sim/tests/economy_fixture/mod.rs"]
#[allow(clippy::all)]
pub(crate) mod economy_fixture;

#[cfg(test)]
mod tests {
    use super::*;

    fn empty_view(tick: u32) -> View {
        View {
            tick,
            owner: None,
            entities: Vec::new(),
            scenery: Default::default(),
            fog: None,
            facts: Vec::new(),
            deaths: Vec::new(),
            objectives: Default::default(),
            mission: None,
            outcome: None,
            colony: None,
            deliveries: Vec::new(),
            battle: Default::default(),
            ai: Vec::new(),
            spells: Default::default(),
            pings: Vec::new(),
            defeated: Vec::new(),
        }
    }

    fn snapshot(tick: u32) -> Arc<Snapshot> {
        Arc::new(Snapshot {
            presentation: Presentation {
                tick,
                ..Default::default()
            },
            view: Arc::new(empty_view(tick)),
            ai: None,
            held: Arc::default(),
            scene: None,
            colonies: None,
            epoch: Instant::now(),
            origin: 0,
            tick_length: TICK,
            cost: TickCost::default(),
            slowdowns: 0,
            decided: false,
        })
    }

    #[test]
    fn observer_rows_count_units_and_tell_fallen_and_reviving_heroes() {
        use canopy_sim::{economy::RevivalEntry, economy::RevivalQueue, entity::EntityId};
        let map = ai_fixture::skirmish(vec![], vec![]);
        let mut world = ai_fixture::world(&map, false);
        ai_fixture::run(&mut world, 2);
        let owner = Owner::Player(1);
        let workers = ai_fixture::all_of(&world, owner, ai_fixture::WORKER).len() as u32;
        let hero = ai_fixture::all_of(&world, owner, ai_fixture::HERO)[0];
        let rows = colonies(&world);
        let row = rows.iter().find(|r| r.player == 1).unwrap();
        assert_eq!(row.workers, workers);
        assert_eq!(row.units, workers + 1, "the workers and the hero");
        assert_eq!(row.heroes.len(), 1);
        assert_eq!(row.heroes[0].status, HeroStatus::Alive);

        // Fallen heroes stay listed and leave the unit count.
        let state = world.state_mut();
        canopy_sim::heroes::retain(state.entities.get_mut(EntityId(hero)).unwrap());
        let row = colonies(&world)
            .into_iter()
            .find(|r| r.player == 1)
            .unwrap();
        assert_eq!(row.units, workers);
        assert_eq!(row.heroes[0].status, HeroStatus::Fallen);

        // Queued for revival at a building: reviving.
        let hall = ai_fixture::all_of(&world, owner, ai_fixture::HALL)[0];
        world
            .state_mut()
            .entities
            .get_mut(EntityId(hall))
            .unwrap()
            .revival = Some(RevivalQueue {
            queue: vec![RevivalEntry {
                id: 1,
                hero: EntityId(hero),
                level: 1,
                progress: 0,
            }],
        });
        let row = colonies(&world)
            .into_iter()
            .find(|r| r.player == 1)
            .unwrap();
        assert_eq!(row.heroes[0].status, HeroStatus::Reviving);
    }

    #[test]
    fn the_mailbox_keeps_the_newest_snapshots_in_order() {
        let mailbox = Mailbox::default();
        let mut out = Vec::new();
        assert_eq!(mailbox.latest(), None);
        assert!(mailbox.take(&mut out) && out.is_empty());
        for tick in 0..10 {
            mailbox.publish(snapshot(tick));
        }
        assert_eq!(mailbox.latest(), Some(9));
        assert!(mailbox.take(&mut out));
        let ticks: Vec<u32> = out.iter().map(|s| s.tick()).collect();
        assert_eq!(ticks, [6, 7, 8, 9]);
    }

    #[test]
    fn a_busy_mailbox_does_not_block_the_reader() {
        let mailbox = Arc::new(Mailbox::default());
        mailbox.publish(snapshot(1));
        let guard = mailbox.ring.lock().unwrap();
        let mut out = vec![snapshot(0)];
        // The writer holds the lock: the reader returns at once and keeps what it had.
        assert!(!mailbox.take(&mut out));
        assert_eq!(out[0].tick(), 0);
        drop(guard);
        assert!(mailbox.take(&mut out));
        assert_eq!(out[0].tick(), 1);
    }

    #[test]
    fn the_handoff_carries_every_tick_across_threads() {
        let mailbox = Arc::new(Mailbox::default());
        let writer = {
            let mailbox = mailbox.clone();
            std::thread::spawn(move || {
                for tick in 0..2000 {
                    mailbox.publish(snapshot(tick));
                }
            })
        };
        let mut out = Vec::new();
        let mut last = None;
        while last != Some(1999) {
            if mailbox.take(&mut out) {
                // Whatever the reader sees is in order and consecutive.
                for pair in out.windows(2) {
                    assert_eq!(pair[0].tick() + 1, pair[1].tick());
                }
                last = out.last().map(|s| s.tick());
            }
        }
        writer.join().unwrap();
    }

    #[test]
    fn the_render_clock_trails_simulation_time_by_one_tick() {
        let epoch = Instant::now();
        let s = Snapshot {
            presentation: Presentation::default(),
            view: Arc::new(empty_view(0)),
            ai: None,
            held: Arc::default(),
            scene: None,
            colonies: None,
            epoch,
            origin: 100,
            tick_length: TICK,
            cost: TickCost::default(),
            slowdowns: 0,
            decided: false,
        };
        let at = |ms: u64| render_tick(&s, epoch + Duration::from_millis(ms));
        assert!((at(250) - 109.0).abs() < 1e-9);
        assert!((at(262) - 109.48).abs() < 1e-9);
        assert_eq!(at(0), 99.0);
    }

    /// A headless AI-versus-AI match on test-owned content runs on the simulation thread
    /// at full speed and every published tick is presented in order.
    #[test]
    fn a_spectated_ai_match_runs_and_presents_every_tick() {
        use canopy_scene::live::Setup;
        let map = super::ai_fixture::skirmish(vec![], vec![]);
        let setup = super::ai_fixture::setup(true);
        let session = Session::local(setup.clone(), None);
        let world = World::new(super::ai_fixture::content(), &map, setup).unwrap();
        let units = world.state().entities.len();
        let presenter = Presenter::new(
            &world,
            &map,
            None,
            Setup {
                colors: vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                tree_slots: Default::default(),
                statics: 0,
            },
        );
        let thread = SimThread::start(
            world,
            session,
            presenter,
            Duration::ZERO,
            100,
            Seat::Observer,
            None,
        );
        let started = Instant::now();
        let mut seen: Vec<u32> = Vec::new();
        let mut out = Vec::new();
        while seen.last().is_none_or(|&t| t < 400) {
            assert!(
                started.elapsed() < Duration::from_secs(120),
                "stalled at {seen:?}"
            );
            assert!(!thread.finished(), "the simulation thread stopped");
            if thread.mailbox().take(&mut out) {
                for s in &out {
                    if seen.last().is_none_or(|&t| s.tick() > t) {
                        seen.push(s.tick());
                        assert_eq!(s.presentation.tick, s.tick());
                        assert!(!s.presentation.actors.is_empty());
                        assert!(
                            s.presentation
                                .actors
                                .windows(2)
                                .all(|w| w[0].key < w[1].key)
                        );
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(1));
        }
        // Fast-forwarded to tick 100 before anything is published, then ticks in order
        // (the reader may miss some between polls, never sees one out of order).
        assert!(seen[0] >= 100, "{seen:?}");
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
        let newest = thread.mailbox().latest().unwrap();
        assert!(newest >= 400);
        assert!(units > 0);
        drop(thread);
    }

    /// The debug overlay's match speed paces the thread without changing a result, and
    /// the shown view (reveal) changes the presentation, not the HUD's view.
    #[test]
    fn match_speed_paces_ticks_and_reveal_changes_only_the_picture() {
        use canopy_scene::live::Setup;
        let map = super::ai_fixture::skirmish(vec![], vec![]);
        let setup = super::ai_fixture::setup(true);
        let session = Session::local(setup.clone(), None);
        let world = World::new(super::ai_fixture::content(), &map, setup.clone()).unwrap();
        let presenter = Presenter::new(
            &world,
            &map,
            None,
            Setup {
                colors: vec![[1.0, 0.0, 0.0], [0.0, 0.0, 1.0]],
                tree_slots: Default::default(),
                statics: 0,
            },
        );
        let thread = SimThread::start(world, session, presenter, TICK, 0, Seat::Player(0), None);
        thread.control(Control::Speed(4.0));
        thread.control(Control::Shown(Some(Seat::Observer)));
        let started = Instant::now();
        while thread.mailbox().latest().is_none_or(|t| t < 120) {
            assert!(started.elapsed() < Duration::from_secs(20), "stalled");
            std::thread::sleep(Duration::from_millis(2));
        }
        // 120 ticks take 3 s at 1×; at 4× about 0.75 s.
        assert!(
            started.elapsed() < Duration::from_millis(2000),
            "{:?}",
            started.elapsed()
        );
        let mut out = Vec::new();
        while !thread.mailbox().take(&mut out) {}
        let newest = out.last().unwrap();
        assert_eq!(newest.tick_length, TICK / 4);
        assert!(newest.presentation.fog.is_none(), "the observer's picture");
        assert!(
            newest.view.fog.is_some(),
            "the player's own view for the HUD"
        );
        // The same ticks as a world stepped on its own.
        let save = thread.save(Duration::from_secs(5)).unwrap();
        drop(thread);
        let mut alone = World::new(super::ai_fixture::content(), &map, setup).unwrap();
        while alone.tick() < save.state.tick {
            alone.step();
        }
        let resumed = World::restore(super::ai_fixture::content(), &map, save).unwrap();
        assert_eq!(resumed.fingerprint(), alone.fingerprint());
    }
}
