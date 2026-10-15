//! Unit wakes on water, carried over one-to-one from the old game (renderer §4.6, old
//! `waterDisturbances.ts` and `waterInteraction.ts`).
//!
//! The emitter samples each moving body's travel by distance: one ring every
//! [`SPACING`] world units while it wades (at most 3 per body and [`EMISSIONS_PER_FRAME`]
//! per frame), plus one ring where it enters the water. Rings live in a bounded ring
//! buffer of [`CAPACITY`] for [`LIFETIME`] seconds; strength follows the water depth and
//! speed, stretch follows speed. Teleports, re-entry into the window around the camera
//! focus, and time jumps reset tracks. The GPU pass (`water::gpu`) draws the live rings
//! in one instanced additive draw into a [`RESOLUTION`]² RGBA16F field covering
//! [`SPAN`]×[`SPAN`] world units around the focus.
//!
//! Only cosmetic history is kept: no simulation, network or save data, and no fog
//! queries. The caller passes only bodies that may wake water: ground units that are
//! visible, alive, and not contained, garrisoned or flying.

use std::collections::HashMap;

/// Rings kept at once.
pub const CAPACITY: usize = 1536;
/// New rings per frame, across every body.
pub const EMISSIONS_PER_FRAME: usize = 256;
/// Seconds a ring lives.
pub const LIFETIME: f32 = 2.8;
/// World units travelled between two rings.
pub const SPACING: f32 = 1.7;
/// Texels per side of the ripple field.
pub const RESOLUTION: u32 = 768;
/// World units per side of the ripple field, centred on the camera focus.
pub const SPAN: f32 = 160.0;
/// Base strength of a ring.
pub const STRENGTH: f32 = 0.3;
/// Radius growth per second of age.
pub const EXPANSION: f32 = 1.1;
/// Width of a ring's crest.
pub const WIDTH: f32 = 0.28;
/// Water above the feet that counts as wading.
const WADING: f32 = 0.035;

/// A body that may wake water this frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mover {
    /// Stable identity across frames.
    pub id: u32,
    /// World position (x east, z south).
    pub x: f32,
    pub z: f32,
    /// The surface it walks on (0: the ground; other values are the caller's, such as
    /// bridge decks, and are passed back to [`WaterField::walk`]).
    pub surface: u32,
    /// Height above that surface.
    pub elevation: f32,
}

/// Water and walking heights.
pub trait WaterField {
    /// Water surface at a world position (below the ground where dry).
    fn water(&self, x: f32, z: f32) -> f32;
    /// Height of `surface` at a world position.
    fn walk(&self, x: f32, z: f32, surface: u32) -> f32;
}

/// One ring: where and when it started, the water level it belongs to, the direction
/// of travel, strength and stretch along that direction.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Ring {
    pub x: f32,
    pub z: f32,
    pub time: f32,
    pub level: f32,
    pub dx: f32,
    pub dz: f32,
    pub strength: f32,
    pub stretch: f32,
}

/// A live ring as the GPU draws it: (x, z, age, level), (dx, dz, strength, stretch).
pub type RingInstance = [[f32; 4]; 2];

#[derive(Debug, Clone, Copy)]
struct Track {
    x: f32,
    z: f32,
    distance: f32,
    seen: u64,
    wet: bool,
}

/// The ring buffer and the per-body tracks.
#[derive(Debug, Clone)]
pub struct Ripples {
    rings: Vec<Ring>,
    head: usize,
    count: usize,
    tracks: HashMap<u32, Track>,
    frame: u64,
    cursor: usize,
    previous_time: Option<f32>,
    /// Rings emitted by the last update.
    pub emitted: usize,
}

impl Default for Ripples {
    fn default() -> Self {
        Self::new()
    }
}

impl Ripples {
    pub fn new() -> Self {
        Self {
            rings: vec![Ring::default(); CAPACITY],
            head: 0,
            count: 0,
            tracks: HashMap::new(),
            frame: 0,
            cursor: 0,
            previous_time: None,
            emitted: 0,
        }
    }

    /// Rings currently held (some may be expired until the next update).
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn is_empty(&self) -> bool {
        self.count == 0
    }

    /// The held rings, oldest first.
    pub fn rings(&self) -> impl Iterator<Item = &Ring> {
        (0..self.count).map(move |n| &self.rings[(self.head + n) % CAPACITY])
    }

    /// Forgets every ring and track.
    pub fn clear(&mut self) {
        self.tracks.clear();
        self.head = 0;
        self.count = 0;
        self.previous_time = None;
    }

    /// Advances to `time` (seconds) with this frame's `movers`, emitting rings for the
    /// ones wading near `focus` (x, z).
    pub fn update(
        &mut self,
        movers: &[Mover],
        field: &impl WaterField,
        time: f32,
        focus: [f32; 2],
    ) {
        let _span = tracing::trace_span!("ripples").entered();
        let dt = self.previous_time.map_or(0.0, |p| time - p);
        if !(0.0..=1.0).contains(&dt) {
            self.clear();
        }
        let dt = if self.previous_time.is_none() {
            0.0
        } else {
            dt
        };
        self.previous_time = Some(time);
        while self.count > 0 && time - self.rings[self.head].time >= LIFETIME {
            self.head = (self.head + 1) % CAPACITY;
            self.count -= 1;
        }
        self.frame += 1;
        let frame = self.frame;
        self.emitted = 0;
        let n = movers.len();
        for j in 0..n {
            let m = movers[(j + self.cursor) % n];
            // No work or history far from the view; re-entering seeds a new track.
            if (m.x - focus[0]).abs() > SPAN * 0.5 + 5.0
                || (m.z - focus[1]).abs() > SPAN * 0.5 + 5.0
            {
                continue;
            }
            let level = field.water(m.x, m.z);
            let feet = field.walk(m.x, m.z, m.surface) + m.elevation;
            let wet = level.is_finite() && level - feet > WADING;
            let Some(track) = self.tracks.get_mut(&m.id) else {
                let phase = (m.id as f32 * 0.618_034).fract() * SPACING;
                self.tracks.insert(
                    m.id,
                    Track {
                        x: m.x,
                        z: m.z,
                        distance: phase,
                        seen: frame,
                        wet,
                    },
                );
                continue;
            };
            track.seen = frame;
            let (dx, dz) = (m.x - track.x, m.z - track.z);
            let distance = dx.hypot(dz);
            let before = track.distance;
            let moved = distance > 0.00001 && dt > 0.0 && distance < (dt * 30.0).max(2.0);
            if moved {
                if wet {
                    track.distance += distance;
                    // Entry makes one ring; wading makes equally spaced ones.
                    let mut along = if track.wet {
                        SPACING - before
                    } else {
                        distance
                    };
                    let mut k = 0;
                    while along <= distance && k < 3 {
                        if self.emitted >= EMISSIONS_PER_FRAME {
                            break;
                        }
                        let f = (along / distance).min(1.0);
                        let (x, z) = (track.x + dx * f, track.z + dz * f);
                        let water = field.water(x, z);
                        let bed = field.walk(x, z, m.surface) + m.elevation;
                        if water - bed > WADING {
                            let speed = distance / dt;
                            let strength = STRENGTH
                                * ((water - bed) / 0.18).min(1.0)
                                * (0.45 + speed * 0.16).min(1.0);
                            let ring = Ring {
                                x,
                                z,
                                time,
                                level: water,
                                dx: dx / distance,
                                dz: dz / distance,
                                strength,
                                stretch: 1.0 + (speed * 0.07).min(0.45),
                            };
                            // Push (the buffer drops its oldest ring when full).
                            if self.count == CAPACITY {
                                self.head = (self.head + 1) % CAPACITY;
                                self.count -= 1;
                            }
                            self.rings[(self.head + self.count) % CAPACITY] = ring;
                            self.count += 1;
                            self.emitted += 1;
                        }
                        k += 1;
                        along += SPACING;
                    }
                    track.distance %= SPACING;
                } else {
                    track.distance = 0.0;
                }
            } else if distance > 0.0 {
                track.distance = 0.0;
            }
            track.x = m.x;
            track.z = m.z;
            track.wet = wet;
        }
        self.tracks.retain(|_, t| t.seen == frame);
        // Rotate the processing order so crowds share the per-frame budget fairly.
        self.cursor = if n > 0 {
            (self.cursor + EMISSIONS_PER_FRAME) % n
        } else {
            0
        };
    }

    /// The rings alive at `time` as GPU instances, into `out` (cleared first).
    pub fn live(&self, time: f32, out: &mut Vec<RingInstance>) {
        out.clear();
        for r in self.rings() {
            let age = time - r.time;
            if !(0.0..LIFETIME).contains(&age) {
                continue;
            }
            out.push([
                [r.x, r.z, age, r.level],
                [r.dx, r.dz, r.strength, r.stretch],
            ]);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Water at `level` everywhere; flat ground at `ground`, a deck at 5 on surface 1.
    struct Pool {
        level: f32,
        ground: f32,
    }

    impl WaterField for Pool {
        fn water(&self, _x: f32, _z: f32) -> f32 {
            self.level
        }
        fn walk(&self, x: f32, _z: f32, surface: u32) -> f32 {
            if surface == 1 {
                5.0
            } else if x < 0.0 {
                // Dry bank west of x = 0.
                self.level + 1.0
            } else {
                self.ground
            }
        }
    }

    const POOL: Pool = Pool {
        level: 0.0,
        ground: -0.5,
    };

    fn mover(id: u32, x: f32) -> Mover {
        Mover {
            id,
            x,
            z: 0.0,
            surface: 0,
            elevation: 0.0,
        }
    }

    /// Walks one body east at `speed` units/s for `seconds` at `fps`.
    fn walk(r: &mut Ripples, speed: f32, seconds: f32, fps: f32, start_x: f32) -> usize {
        let mut total = 0;
        let frames = (seconds * fps) as usize;
        for f in 0..=frames {
            let t = f as f32 / fps;
            r.update(&[mover(7, start_x + speed * t)], &POOL, t, [0.0, 0.0]);
            total += r.emitted;
        }
        total
    }

    #[test]
    fn spacing_does_not_depend_on_frame_rate() {
        for fps in [20.0, 60.0, 144.0] {
            let mut r = Ripples::new();
            // 17 units of wading: 10 rings (± the seeded phase).
            let n = walk(&mut r, 4.25, 4.0, fps, 1.0);
            assert!((9..=10).contains(&n), "{fps} fps: {n} rings");
        }
    }

    #[test]
    fn rings_expire_and_the_buffer_is_bounded() {
        let mut r = Ripples::new();
        walk(&mut r, 4.0, 2.0, 60.0, 1.0);
        let held = r.len();
        assert!(held > 0);
        let mut live = Vec::new();
        r.live(2.0, &mut live);
        assert_eq!(live.len(), held);
        assert!(live.iter().all(|i| i[0][2] >= 0.0 && i[0][2] < LIFETIME));
        // Standing still: everything expires after the lifetime.
        r.update(&[mover(7, 9.0)], &POOL, 2.0 + LIFETIME, [0.0, 0.0]);
        assert_eq!(r.len(), 0);

        // A crowd at a ford: never more than CAPACITY rings, at most 256 per frame.
        let mut r = Ripples::new();
        let mut movers: Vec<Mover> = (0..1000).map(|i| mover(i, 1.0 + i as f32 * 0.01)).collect();
        for f in 0..120 {
            for m in &mut movers {
                m.x += 0.2;
            }
            r.update(&movers, &POOL, f as f32 / 60.0, [0.0, 0.0]);
            assert!(r.emitted <= EMISSIONS_PER_FRAME);
            assert!(r.len() <= CAPACITY);
        }
        assert_eq!(r.len(), CAPACITY);
    }

    #[test]
    fn entering_water_makes_one_ring_and_dry_ground_none() {
        let mut r = Ripples::new();
        // On the dry bank: nothing.
        assert_eq!(walk(&mut r, 2.0, 0.5, 60.0, -2.0), 0);
        // Step into the water: exactly one entry ring, where the step ends.
        r.update(&[mover(7, 0.2)], &POOL, 0.5 + 1.0 / 60.0, [0.0, 0.0]);
        assert_eq!(r.emitted, 1);
        let ring = *r.rings().last().unwrap();
        assert!((ring.x - 0.2).abs() < 1e-5);
        assert_eq!(ring.level, 0.0);
        assert!((ring.dx - 1.0).abs() < 1e-6);
    }

    #[test]
    fn decks_teleports_far_bodies_and_time_jumps_emit_nothing() {
        // A body on a bridge deck above the water.
        let mut r = Ripples::new();
        let mut deck = mover(1, 1.0);
        deck.surface = 1;
        for f in 0..60 {
            deck.x += 0.1;
            r.update(&[deck], &POOL, f as f32 / 60.0, [0.0, 0.0]);
        }
        assert_eq!(r.len(), 0);
        // A teleport (50 units in one frame) resets the track.
        let mut r = Ripples::new();
        r.update(&[mover(2, 1.0)], &POOL, 0.0, [0.0, 0.0]);
        r.update(&[mover(2, 51.0)], &POOL, 1.0 / 60.0, [0.0, 0.0]);
        assert_eq!(r.emitted, 0);
        // Outside the window around the focus: no track at all.
        let mut r = Ripples::new();
        for f in 0..60 {
            let x = 200.0 + f as f32 * 0.1;
            r.update(&[mover(3, x)], &POOL, f as f32 / 60.0, [0.0, 0.0]);
        }
        assert_eq!(r.len(), 0);
        // Time running backwards clears everything.
        let mut r = Ripples::new();
        walk(&mut r, 4.0, 1.0, 60.0, 1.0);
        assert!(!r.is_empty());
        r.update(&[], &POOL, 0.0, [0.0, 0.0]);
        assert!(r.is_empty());
    }

    #[test]
    fn strength_and_stretch_follow_depth_and_speed() {
        let shallow = Pool {
            level: 0.0,
            ground: -0.09,
        };
        let mut a = Ripples::new();
        let mut b = Ripples::new();
        for f in 0..120 {
            let t = f as f32 / 60.0;
            a.update(&[mover(1, 1.0 + t * 1.0)], &shallow, t, [0.0, 0.0]);
            b.update(&[mover(1, 1.0 + t * 6.0)], &POOL, t, [0.0, 0.0]);
        }
        let slow = a.rings().next().unwrap();
        let fast = b.rings().next().unwrap();
        // Half the full-strength depth, slow: 0.3 · 0.5 · 0.61.
        assert!((slow.strength - STRENGTH * 0.5 * 0.61).abs() < 1e-3);
        assert!((fast.strength - STRENGTH).abs() < 1e-5);
        assert!((slow.stretch - 1.07).abs() < 1e-3);
        assert!((fast.stretch - 1.42).abs() < 1e-3);
    }
}
