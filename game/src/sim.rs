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

use canopy_net::Session;
use canopy_scene::live::{Presentation, Presenter};
use canopy_sim::World;
use std::{
    collections::VecDeque,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
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

/// One presented tick.
#[derive(Debug)]
pub struct Snapshot {
    pub presentation: Presentation,
    /// Simulation time of tick `origin` under the schedule this tick was computed on.
    pub epoch: Instant,
    pub origin: u32,
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
    handle: Option<JoinHandle<()>>,
}

/// Whether a unit is held inside another entity (the view does not carry it yet).
pub fn held(world: &World, id: canopy_sim::entity::EntityId) -> bool {
    world
        .state()
        .entities
        .get(id)
        .and_then(|e| e.unit.as_ref())
        .is_some_and(|u| u.contained.is_some() || u.release.is_some())
}

/// Steps `world` once through `session` and presents the tick; `None` while the session
/// waits for the next beat.
pub fn advance(
    world: &mut World,
    session: &mut Session,
    presenter: &mut Presenter,
) -> Option<(Presentation, TickCost, bool)> {
    let started = Instant::now();
    session.advance(world)?;
    let stepped = Instant::now();
    let view = {
        let _span = tracing::trace_span!("observer_view").entered();
        world.observer_view()
    };
    let viewed = Instant::now();
    let presentation = presenter.update(world, &view, &|id| held(world, id));
    let presented = Instant::now();
    let ms = |a: Instant, b: Instant| (b - a).as_secs_f32() * 1000.0;
    Some((
        presentation,
        TickCost {
            step: ms(started, stepped),
            view: ms(stepped, viewed),
            present: ms(viewed, presented),
        },
        view.outcome.is_some(),
    ))
}

/// The presentation of the world as it stands (before its first step, or a shot).
pub fn present(world: &World, presenter: &mut Presenter) -> Presentation {
    let view = world.observer_view();
    presenter.update(world, &view, &|id| held(world, id))
}

impl SimThread {
    /// Starts the match: first fast-forwards to `start_tick` (presenting every tick), then
    /// runs at `tick_length` per tick (zero: as fast as possible).
    pub fn spawn(
        mut world: World,
        mut session: Session,
        mut presenter: Presenter,
        tick_length: Duration,
        start_tick: u32,
    ) -> Self {
        let mailbox = Arc::new(Mailbox::default());
        let stop = Arc::new(AtomicBool::new(false));
        let handle = {
            let mailbox = mailbox.clone();
            let stop = stop.clone();
            std::thread::Builder::new()
                .name("simulation".into())
                .spawn(move || {
                    let _span = tracing::info_span!("simulation_thread").entered();
                    let started = Instant::now();
                    while world.tick() < start_tick && !stop.load(Ordering::Relaxed) {
                        if advance(&mut world, &mut session, &mut presenter).is_none() {
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
                    let first = present(&world, &mut presenter);
                    let tick0 = world.tick();
                    // Schedule: simulation time of tick n is epoch + (n − tick0) · length.
                    let mut epoch = Instant::now();
                    let mut slowdowns = 0;
                    mailbox.publish(Arc::new(Snapshot {
                        presentation: first,
                        epoch,
                        origin: tick0,
                        tick_length,
                        cost: TickCost::default(),
                        slowdowns,
                        decided: false,
                    }));
                    while !stop.load(Ordering::Relaxed) {
                        let next = world.tick() + 1;
                        let ahead = (next - tick0).saturating_sub(LEAD);
                        let due = epoch + tick_length * ahead;
                        let now = Instant::now();
                        if now < due {
                            std::thread::sleep((due - now).min(Duration::from_millis(5)));
                            continue;
                        }
                        if !tick_length.is_zero() {
                            let behind =
                                ((now - due).as_secs_f64() / tick_length.as_secs_f64()) as u32;
                            if behind > MAX_BACKLOG {
                                epoch += tick_length * (behind - MAX_BACKLOG);
                                slowdowns += 1;
                            }
                        }
                        let Some((presentation, cost, decided)) =
                            advance(&mut world, &mut session, &mut presenter)
                        else {
                            std::thread::sleep(Duration::from_millis(1));
                            continue;
                        };
                        mailbox.publish(Arc::new(Snapshot {
                            presentation,
                            epoch,
                            origin: tick0,
                            tick_length,
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
            handle: Some(handle),
        }
    }

    pub fn mailbox(&self) -> &Mailbox {
        &self.mailbox
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
mod ai_fixture;

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot(tick: u32) -> Arc<Snapshot> {
        Arc::new(Snapshot {
            presentation: Presentation {
                tick,
                ..Default::default()
            },
            epoch: Instant::now(),
            origin: 0,
            tick_length: TICK,
            cost: TickCost::default(),
            slowdowns: 0,
            decided: false,
        })
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
        let thread = SimThread::spawn(world, session, presenter, Duration::ZERO, 100);
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
        // Fast-forwarded to tick 100, then every tick in order (the reader may miss some
        // between polls, but never sees one out of order).
        assert!((100..110).contains(&seen[0]), "{seen:?}");
        assert!(seen.windows(2).all(|w| w[0] < w[1]));
        let newest = thread.mailbox().latest().unwrap();
        assert!(newest >= 400);
        assert!(units > 0);
        drop(thread);
    }
}
