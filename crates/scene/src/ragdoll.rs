//! Ragdolls (D114): a unit that dies plays the first moment of its death clip, then its
//! body falls under physics onto the ground and its held weapon drops loose, as StarCraft
//! II hands its death keyframes over to physics.
//!
//! The parts and their joints are fitted to the skin by the cooker
//! ([`canopy_assets::Ragdoll`]). Each part's frame is the model's rest space carried by the
//! skinning matrix of the joint that starts it, so the part's pose at the handover is that
//! joint's matrix in the death clip and its shapes are authored in rest coordinates.
//! Afterwards every joint's palette entry is its part's motion since the handover applied
//! to its matrix then: vertices weighed between two parts blend smoothly at the seams.
//!
//! Physics is presentation only: it never touches the simulation (D4), and a replay shows
//! the same falls because the scene feeds it the same deaths in the same order. Parts
//! collide with the ground only, never with each other or other bodies.
//!
//! A caller whose next presentation is always a later tick (the match's simulation thread)
//! may take the next tick's step ahead, on a thread of its own, while it simulates that
//! tick ([`Ragdolls::step_ahead`]): the bodies fall exactly as they would have, off the
//! presentation's critical path.
//!
//! A body that has come to rest sleeps ([`REST`]): the physics skips it and its pose is
//! worked out once, until something wakes it.

use crate::animation::{SPEED, TICK_SECONDS};
use canopy_assets::{CLIP_FRAME_RATE, Matrix3x4, Ragdoll, RagdollShape};
use glam::{Mat4, Quat, Vec3};
use rapier3d::prelude::{
    ColliderBuilder, Group, InteractionGroups, InteractionTestMode, JointAxis, PhysicsWorld,
    RigidBodyBuilder, RigidBodyHandle, SphericalJointBuilder,
};
use std::{
    collections::BTreeMap,
    sync::{Arc, mpsc},
};

/// World units per second² bodies fall at: brisker than the earth's, as the old corpse
/// fall was (15 t²).
const GRAVITY: f32 = 22.0;
/// Most bodies falling at once; deaths beyond play their death clip as before.
pub const MAX_FALLING: usize = 96;
/// Ticks of the death clip before physics takes over (the first flinch).
pub const HANDOVER_TICKS: u32 = 5;
/// How far a joint may turn from where the death clip left it: swing and twist (radians).
const SWING: f32 = 0.9;
const TWIST: f32 = 0.45;
/// No part weighs less than this share of the heaviest (chains of very unequal masses
/// jitter).
const LIGHTEST: f32 = 0.18;
/// A loose weapon weighs between these shares of the heaviest part.
const WEAPON_MASS: (f32, f32) = (0.08, 0.3);
/// The push a killing blow gives by default, world units per second, at the top of the
/// body (the feet get [`PUSH_FOOT`] of it): a body is knocked back a step or two from what
/// killed it, hopping off its feet ([`HOP`]) and skidding as it falls (Jiri: "actually see
/// units move a bit back when hit"). Weapons and abilities may set their own
/// (`presentation.push`).
pub const PUSH: f32 = 5.0;
const PUSH_FOOT: f32 = 0.75;
/// Every blow lifts the body at this share of its push: a short hop back.
const HOP: f32 = 0.3;
/// Harder blows launch the whole body: from [`LAUNCH`]`.0` world units per second the feet
/// share the push more and more, and every part rises at up to [`LIFT`] of the push, fully
/// from `LAUNCH.1` (a fireball's 9 throws a body off the ground in an arc; the default
/// knocks it back a step).
const LAUNCH: (f32, f32) = (5.5, 11.0);
const LIFT: f32 = 0.5;
/// A loose weapon flies off with up to this much extra sideways speed and spin.
const WEAPON_TOSS: f32 = 1.4;
const WEAPON_SPIN: f32 = 7.0;
const FRICTION: f32 = 0.6;
const RESTITUTION: f32 = 0.05;
const LINEAR_DAMPING: f32 = 0.15;
const ANGULAR_DAMPING: f32 = 1.2;
/// A body sleeps once it has settled: for [`SETTLE_TICKS`] steps no point of any part
/// strayed more than [`REST`] of the body's height from where it lay. Asleep it costs the
/// physics nothing and its pose is worked out once. Where the parts are counts, not their
/// velocities, so a body trembling in place on its joints sleeps too.
const REST: f32 = 0.02;
const SETTLE_TICKS: u32 = 20;

const GROUND: Group = Group::GROUP_1;
const PARTS: Group = Group::GROUP_2;

/// What the scene needs of a model to make it fall: its fitted parts, its joints' rest
/// origins and its death clip's skinning matrices.
#[derive(Debug, Clone, PartialEq)]
pub struct RagdollModel {
    pub ragdoll: Ragdoll,
    /// Joint origins at rest (model space).
    pub origins: Vec<[f32; 3]>,
    /// Joints per frame.
    pub joints: usize,
    pub frames: u32,
    /// The death clip's skinning matrices, frame-major.
    pub matrices: Vec<Matrix3x4>,
    /// Model height at rest.
    pub height: f32,
}

impl RagdollModel {
    /// The death clip's matrices at `time` seconds into it (the nearest baked frame).
    fn frame(&self, time: f32) -> &[Matrix3x4] {
        let last = self.frames.saturating_sub(1) as usize;
        let f = ((time.max(0.0) * CLIP_FRAME_RATE).round() as usize).min(last);
        &self.matrices[f * self.joints..(f + 1) * self.joints]
    }
}

/// A row-major 3 × 4 affine matrix as a glam matrix.
pub fn mat4(m: &Matrix3x4) -> Mat4 {
    Mat4::from_cols_array(&[
        m[0], m[4], m[8], 0.0, m[1], m[5], m[9], 0.0, m[2], m[6], m[10], 0.0, m[3], m[7], m[11],
        1.0,
    ])
}

/// A glam matrix as a row-major 3 × 4 affine matrix.
pub fn rows(m: &Mat4) -> Matrix3x4 {
    let c = m.to_cols_array();
    [
        c[0], c[4], c[8], c[12], c[1], c[5], c[9], c[13], c[2], c[6], c[10], c[14],
    ]
}

fn vector(v: Vec3) -> rapier3d::math::Vector {
    rapier3d::math::Vector::new(v.x, v.y, v.z)
}

fn ours(v: rapier3d::math::Vector) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

fn rotation(q: Quat) -> rapier3d::math::Rotation {
    rapier3d::math::Rotation::from_xyzw(q.x, q.y, q.z, q.w)
}

fn quat(q: rapier3d::math::Rotation) -> Quat {
    Quat::from_xyzw(q.x, q.y, q.z, q.w).normalize()
}

fn pose(translation: Vec3, turn: Quat) -> rapier3d::math::Pose {
    rapier3d::math::Pose::from_parts(vector(translation), rotation(turn))
}

/// A body falling.
struct Fallen {
    model: Arc<RagdollModel>,
    parts: Vec<RigidBodyHandle>,
    /// Each part's world pose at the handover (rigid: the instance's scale is in the
    /// shapes).
    start: Vec<Mat4>,
    /// The instance transform at the handover.
    instance: Mat4,
    /// Each joint's skinning matrix at the handover.
    base: Arc<[Mat4]>,
    /// The root part: the instance follows it.
    root: usize,
    /// The body's pose while it sleeps (unsunk): what lies still is worked out once.
    asleep: Option<RagdollPose>,
}

/// Whether a body has settled ([`REST`]), watched after every step.
struct Rest {
    parts: Vec<RigidBodyHandle>,
    /// Two far points of each part's shape (in its frame): how far they moved is how far
    /// the part moved.
    ends: Vec<[Vec3; 2]>,
    /// How far a point may stray and still count as lying still (world units).
    tolerance: f32,
    /// Where each part lay (world translation and rotation) when the body last moved, and
    /// the steps since.
    lay: Vec<(Vec3, Quat)>,
    still: u32,
}

/// The physics and what watches its bodies settle: all a step takes, in place or on the
/// stepper's thread, so both fall alike.
struct Physics {
    world: PhysicsWorld,
    /// By body id, as [`Ragdolls`]' bodies.
    rest: BTreeMap<u32, Rest>,
}

impl Physics {
    /// One tick: the bodies fall, and those that have settled go to sleep.
    fn step(&mut self) {
        self.world.step();
        let _span = tracing::trace_span!("ragdoll_settle").entered();
        let bodies = &mut self.world.bodies;
        for rest in self.rest.values_mut() {
            if rest.parts.iter().all(|&h| bodies[h].is_sleeping()) {
                continue;
            }
            let at =
                |h: RigidBodyHandle| (ours(bodies[h].translation()), quat(*bodies[h].rotation()));
            let moved = rest
                .parts
                .iter()
                .zip(&rest.lay)
                .zip(&rest.ends)
                .any(|((&h, &lay), ends)| strayed(at(h), lay, ends) > rest.tolerance);
            if moved {
                // Awake again (something woke it) or still moving: watched from here.
                for (&h, lay) in rest.parts.iter().zip(&mut rest.lay) {
                    *lay = at(h);
                }
                rest.still = 0;
                continue;
            }
            rest.still += 1;
            if rest.still >= SETTLE_TICKS {
                for &h in &rest.parts {
                    if let Some(body) = bodies.get_mut(h) {
                        body.sleep();
                    }
                }
            }
        }
    }
}

/// How far the farther of a part's `ends` strayed between two of its poses.
fn strayed(now: (Vec3, Quat), then: (Vec3, Quat), ends: &[Vec3; 2]) -> f32 {
    ends.iter()
        .map(|&e| ((now.0 + now.1 * e) - (then.0 + then.1 * e)).length())
        .fold(0.0, f32::max)
}

/// A falling body's pose: where its instance stands and each part's motion since the
/// handover in model space, for [`palette`].
#[derive(Debug, Clone)]
pub struct RagdollPose {
    pub model: Arc<RagdollModel>,
    pub base: Arc<[Mat4]>,
    pub position: Vec3,
    pub rotation: Quat,
    /// Per part: translation and rotation (model space).
    pub parts: Vec<(Vec3, Quat)>,
}

impl RagdollPose {
    /// The pose between `a` and `self` at `t` (0 = `a`).
    pub fn blend(&self, a: &RagdollPose, t: f32) -> RagdollPose {
        if a.parts.len() != self.parts.len() {
            return self.clone();
        }
        RagdollPose {
            model: self.model.clone(),
            base: self.base.clone(),
            position: a.position.lerp(self.position, t),
            rotation: a.rotation.slerp(self.rotation, t),
            parts: a
                .parts
                .iter()
                .zip(&self.parts)
                .map(|(pa, pb)| (pa.0.lerp(pb.0, t), pa.1.slerp(pb.1, t)))
                .collect(),
        }
    }

    /// Appends the model-space joint palette to `out`.
    pub fn palette(&self, out: &mut Vec<Matrix3x4>) {
        let motions: Vec<Mat4> = self
            .parts
            .iter()
            .map(|&(t, r)| Mat4::from_rotation_translation(r, t))
            .collect();
        for (j, base) in self.base.iter().enumerate() {
            let part = self.model.ragdoll.joint_part.get(j).copied().unwrap_or(0) as usize;
            let motion = motions.get(part).copied().unwrap_or(Mat4::IDENTITY);
            out.push(rows(&(motion * *base)));
        }
    }
}

/// The scene's falling bodies and the ground they land on.
pub struct Ragdolls {
    /// None while the stepper holds it ([`Ragdolls::step_ahead`]).
    physics: Option<Box<Physics>>,
    ground: bool,
    fallen: BTreeMap<u32, Fallen>,
    /// The thread that takes steps ahead, once one was asked for.
    stepper: Option<Stepper>,
    /// Whether the next tick's step was taken ahead (or is being taken).
    ahead: bool,
}

fn new_physics() -> Box<Physics> {
    let mut world = PhysicsWorld {
        gravity: rapier3d::math::Vector::new(0.0, -GRAVITY, 0.0),
        ..Default::default()
    };
    world.integration_parameters.dt = TICK_SECONDS;
    Box::new(Physics {
        world,
        rest: BTreeMap::new(),
    })
}

impl Default for Ragdolls {
    fn default() -> Self {
        Self {
            physics: Some(new_physics()),
            ground: false,
            fallen: BTreeMap::new(),
            stepper: None,
            ahead: false,
        }
    }
}

/// A thread stepping the physics it is handed and handing it back.
struct Stepper {
    jobs: mpsc::Sender<Box<Physics>>,
    done: mpsc::Receiver<Box<Physics>>,
}

impl Stepper {
    fn start() -> Option<Self> {
        let (jobs, inbox) = mpsc::channel::<Box<Physics>>();
        let (outbox, done) = mpsc::channel();
        std::thread::Builder::new()
            .name("ragdolls".into())
            .spawn(move || {
                // The next update waits for this step.
                canopy_render::priority::raise_current_thread();
                for mut physics in inbox {
                    {
                        let _span = tracing::trace_span!("ragdoll_step_ahead").entered();
                        physics.step();
                    }
                    if outbox.send(physics).is_err() {
                        break;
                    }
                }
            })
            .ok()?;
        Some(Self { jobs, done })
    }
}

impl Ragdolls {
    /// The physics, back from the stepper first if it is away. A stepper that died (a
    /// panic in the physics) took it along: its bodies are gone, the ground goes in again.
    fn physics(&mut self) -> &mut Physics {
        if self.physics.is_none() {
            let back = self.stepper.as_ref().and_then(|s| s.done.recv().ok());
            if back.is_none() {
                self.fallen.clear();
                self.ground = false;
                self.stepper = None;
                self.ahead = false;
            }
            self.physics = Some(back.unwrap_or_else(new_physics));
        }
        self.physics.as_mut().expect("the physics is back")
    }

    /// Whether the ground is in (bodies fall forever without it).
    pub fn has_ground(&self) -> bool {
        self.ground
    }

    /// The ground: a height field of `vertices²` samples (row index z) whose first vertex
    /// stands at `origin` on both axes, one world unit apart.
    pub fn set_ground(&mut self, vertices: usize, heights: &[f32], origin: f32) {
        let _span = tracing::trace_span!("ragdoll_ground").entered();
        if vertices < 2 || heights.len() != vertices * vertices {
            return;
        }
        // Parry stores heights by column (x), each running along z.
        let mut by_column = vec![0.0; heights.len()];
        for z in 0..vertices {
            for x in 0..vertices {
                by_column[x * vertices + z] = heights[z * vertices + x];
            }
        }
        let cells = (vertices - 1) as f32;
        let field = rapier3d::parry::utils::Array2::new(vertices, vertices, by_column);
        let centre = origin + cells * 0.5;
        let collider =
            ColliderBuilder::heightfield(field, rapier3d::math::Vector::new(cells, 1.0, cells))
                .translation(rapier3d::math::Vector::new(centre, 0.0, centre))
                .friction(FRICTION)
                .collision_groups(InteractionGroups::new(
                    GROUND,
                    PARTS,
                    InteractionTestMode::And,
                ));
        self.physics().world.colliders.insert(collider);
        self.ground = true;
    }

    pub fn len(&self) -> usize {
        self.fallen.len()
    }

    pub fn is_empty(&self) -> bool {
        self.fallen.is_empty()
    }

    pub fn contains(&self, id: u32) -> bool {
        self.fallen.contains_key(&id)
    }

    /// Starts body `id` falling: the model at `instance` (scale, rotation, translation)
    /// `time` seconds into its death clip, thrown by `push` (world units per second at the
    /// top of the body, across the ground). `seed` varies the weapon's toss. False when too many bodies
    /// fall already.
    pub fn spawn(
        &mut self,
        id: u32,
        model: &Arc<RagdollModel>,
        instance: (Vec3, Quat, Vec3),
        time: f32,
        push: Vec3,
        seed: u32,
    ) -> bool {
        let _span = tracing::trace_span!("ragdoll_spawn").entered();
        if self.fallen.len() >= MAX_FALLING || self.fallen.contains_key(&id) {
            return false;
        }
        let (scale, turn, at) = instance;
        let physics = &mut self.physics().world;
        let s = scale.x.abs().max(1e-3);
        let transform = Mat4::from_scale_rotation_translation(scale, turn, at);
        let now = model.frame(time);
        // One baked frame earlier: how the clip moved the body into the handover.
        let earlier = model.frame(time - 1.0 / CLIP_FRAME_RATE);
        let frame_seconds = 1.0 / CLIP_FRAME_RATE / SPEED;
        let ragdoll = &model.ragdoll;
        let heaviest = ragdoll
            .parts
            .iter()
            .map(|p| p.mass)
            .fold(0.0f32, f32::max)
            .max(1e-3);
        let height = model.height.max(1e-3);
        let ground_y = at.y;
        let root = root_of(ragdoll);
        let mut parts = Vec::with_capacity(ragdoll.parts.len());
        let mut start = Vec::with_capacity(ragdoll.parts.len());
        let mut lay = Vec::with_capacity(ragdoll.parts.len());
        let mut ends = Vec::with_capacity(ragdoll.parts.len());
        for (i, part) in ragdoll.parts.iter().enumerate() {
            let j = part.joint as usize;
            let centre = Vec3::from(match part.shape {
                RagdollShape::Capsule { a, b, .. } => [
                    (a[0] + b[0]) * 0.5,
                    (a[1] + b[1]) * 0.5,
                    (a[2] + b[2]) * 0.5,
                ],
                RagdollShape::Box { centre, .. } => centre,
            });
            let world = |m: &[Matrix3x4]| {
                let skin = mat4(&m[j]);
                let (_, r, t) = skin.to_scale_rotation_translation();
                (
                    transform.transform_point3(t),
                    (turn * r).normalize(),
                    transform.transform_point3(skin.transform_point3(centre)),
                )
            };
            let (p, r, c) = world(now);
            let (_, r0, c0) = world(earlier);
            // The clip's own motion carries on.
            let mut velocity = (c - c0) / frame_seconds;
            let spin = (r * r0.inverse()).to_scaled_axis() / frame_seconds;
            let loose = part.parent.is_none() && i != root;
            let mut mass = (part.mass * s * s * s).max(LIGHTEST * heaviest * s * s * s);
            let rise = ((c.y - ground_y) / (height * s)).clamp(0.0, 1.0);
            let strength = push.length();
            let launch = ((strength - LAUNCH.0) / (LAUNCH.1 - LAUNCH.0)).clamp(0.0, 1.0);
            let foot = PUSH_FOOT + (1.0 - PUSH_FOOT) * launch;
            let lift = HOP + (LIFT - HOP) * launch;
            velocity += push * (foot + (1.0 - foot) * rise) + Vec3::Y * strength * lift;
            let mut angular = spin;
            if loose {
                let h = hash(seed.wrapping_add(i as u32));
                let side = Vec3::new(push.z, 0.0, -push.x).normalize_or_zero();
                let toss = (unit(h) * 2.0 - 1.0) * WEAPON_TOSS;
                velocity += side * toss + Vec3::Y * WEAPON_TOSS * 0.5 * unit(h >> 8);
                angular += Vec3::new(unit(h >> 4) - 0.5, unit(h >> 12) - 0.5, unit(h >> 16) - 0.5)
                    * 2.0
                    * WEAPON_SPIN;
                mass = mass.clamp(
                    WEAPON_MASS.0 * heaviest * s * s * s,
                    WEAPON_MASS.1 * heaviest * s * s * s,
                );
            }
            let body = RigidBodyBuilder::dynamic()
                .pose(pose(p, r))
                .linvel(vector(velocity))
                .angvel(vector(angular))
                .linear_damping(LINEAR_DAMPING)
                .angular_damping(ANGULAR_DAMPING)
                .build();
            let handle = physics.insert_body(body);
            let collider = match part.shape {
                RagdollShape::Capsule { a, b, radius } => ColliderBuilder::capsule_from_endpoints(
                    vector(Vec3::from(a) * s),
                    vector(Vec3::from(b) * s),
                    radius * s,
                ),
                RagdollShape::Box {
                    centre,
                    rotation: q,
                    half,
                } => ColliderBuilder::cuboid(half[0] * s, half[1] * s, half[2] * s).position(pose(
                    Vec3::from(centre) * s,
                    Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize(),
                )),
            }
            .mass(mass)
            .friction(FRICTION)
            .restitution(RESTITUTION)
            .collision_groups(InteractionGroups::new(
                PARTS,
                GROUND,
                InteractionTestMode::And,
            ));
            physics
                .colliders
                .insert_with_parent(collider, handle, &mut physics.bodies);
            parts.push(handle);
            start.push(Mat4::from_rotation_translation(r, p));
            lay.push((p, r));
            ends.push(match part.shape {
                RagdollShape::Capsule { a, b, .. } => [Vec3::from(a) * s, Vec3::from(b) * s],
                RagdollShape::Box {
                    centre,
                    rotation: q,
                    half,
                } => {
                    let corner =
                        Quat::from_xyzw(q[0], q[1], q[2], q[3]).normalize() * Vec3::from(half);
                    let centre = Vec3::from(centre);
                    [(centre - corner) * s, (centre + corner) * s]
                }
            });
        }
        // Ball joints at each part's pivot, measured from where the clip left them.
        for (i, part) in ragdoll.parts.iter().enumerate() {
            let Some(parent) = part.parent else { continue };
            let parent = parent as usize;
            let pivot = Vec3::from(model.origins[part.joint as usize]) * s;
            let world_pivot = start[i].transform_point3(pivot);
            let in_parent = start[parent].inverse().transform_point3(world_pivot);
            let (_, rp, _) = start[parent].to_scale_rotation_translation();
            let (_, rc, _) = start[i].to_scale_rotation_translation();
            let joint = SphericalJointBuilder::new()
                .local_frame1(pose(in_parent, (rp.inverse() * rc).normalize()))
                .local_frame2(pose(pivot, Quat::IDENTITY))
                .contacts_enabled(false)
                .limits(JointAxis::AngX, [-TWIST, TWIST])
                .limits(JointAxis::AngY, [-SWING, SWING])
                .limits(JointAxis::AngZ, [-SWING, SWING]);
            physics.insert_impulse_joint(parts[parent], parts[i], joint);
        }
        let base: Arc<[Mat4]> = now.iter().map(mat4).collect();
        self.physics().rest.insert(
            id,
            Rest {
                parts: parts.clone(),
                ends,
                tolerance: REST * height * s,
                lay,
                still: 0,
            },
        );
        self.fallen.insert(
            id,
            Fallen {
                model: model.clone(),
                root,
                parts,
                start,
                instance: transform,
                base,
                asleep: None,
            },
        );
        true
    }

    /// Lets bodies fall for `ticks` ticks (a step taken ahead is the first of them).
    pub fn step(&mut self, ticks: u32) {
        let ahead = std::mem::take(&mut self.ahead);
        if self.fallen.is_empty() {
            return;
        }
        let _span = tracing::trace_span!("ragdoll_step", bodies = self.fallen.len()).entered();
        let physics = self.physics();
        for _ in u32::from(ahead)..ticks.min(4) {
            physics.step();
        }
    }

    /// Takes the next tick's step now on the stepper's thread, while the caller goes on;
    /// the next [`Ragdolls::step`] counts it as its first. Only for a caller whose next
    /// step is of at least one tick: a step of none would leave the bodies a tick ahead.
    pub fn step_ahead(&mut self) {
        if self.fallen.is_empty() || self.ahead {
            return;
        }
        if self.stepper.is_none() {
            self.stepper = Stepper::start();
        }
        let (Some(stepper), Some(physics)) = (&self.stepper, self.physics.take()) else {
            return;
        };
        match stepper.jobs.send(physics) {
            Ok(()) => self.ahead = true,
            Err(unsent) => self.physics = Some(unsent.0),
        }
    }

    /// Body `id`'s pose now, sunk `sink` world units into the ground.
    pub fn pose(&mut self, id: u32, sink: f32) -> Option<RagdollPose> {
        self.fallen.get(&id)?;
        self.physics();
        let world = &self.physics.as_deref().expect("the physics is back").world;
        let f = self.fallen.get_mut(&id).expect("falling");
        let asleep = f.parts.iter().all(|&h| world.bodies[h].is_sleeping());
        if !asleep {
            f.asleep = None;
        } else if f.asleep.is_none() {
            f.asleep = Some(posed(world, f));
        }
        let mut pose = match &f.asleep {
            Some(pose) => pose.clone(),
            None => posed(world, f),
        };
        pose.position.y -= sink;
        Some(pose)
    }

    /// Takes body `id` out of the world.
    pub fn remove(&mut self, id: u32) {
        if let Some(f) = self.fallen.remove(&id) {
            let physics = self.physics();
            physics.rest.remove(&id);
            for h in f.parts {
                physics.world.remove_body(h);
            }
        }
    }

    /// Takes every body out (a replay seek); the ground stays.
    pub fn clear(&mut self) {
        self.physics();
        self.ahead = false;
        let ids: Vec<u32> = self.fallen.keys().copied().collect();
        for id in ids {
            self.remove(id);
        }
    }
}

/// Body `f`'s pose in `world`, unsunk.
fn posed(world: &PhysicsWorld, f: &Fallen) -> RagdollPose {
    let now: Vec<Mat4> = f
        .parts
        .iter()
        .map(|&h| {
            let b = &world.bodies[h];
            Mat4::from_rotation_translation(quat(*b.rotation()), ours(b.translation()))
        })
        .collect();
    // The instance follows the root part; its scale stays.
    let follow = now[f.root] * f.start[f.root].inverse() * f.instance;
    let (_, rotation, position) = follow.to_scale_rotation_translation();
    let parts = now
        .iter()
        .zip(&f.start)
        .map(|(n, s)| {
            let motion = follow.inverse() * *n * s.inverse() * f.instance;
            let (_, r, t) = motion.to_scale_rotation_translation();
            (t, r)
        })
        .collect();
    RagdollPose {
        model: f.model.clone(),
        base: f.base.clone(),
        position,
        rotation,
        parts,
    }
}

/// The root part: the heaviest capsule without a parent (loose equipment is a box).
fn root_of(ragdoll: &Ragdoll) -> usize {
    ragdoll
        .parts
        .iter()
        .enumerate()
        .filter(|(_, p)| p.parent.is_none() && matches!(p.shape, RagdollShape::Capsule { .. }))
        .max_by(|a, b| a.1.mass.total_cmp(&b.1.mass))
        .map_or(0, |(i, _)| i)
}

fn hash(mut x: u32) -> u32 {
    x ^= x >> 16;
    x = x.wrapping_mul(0x7feb_352d);
    x ^= x >> 15;
    x = x.wrapping_mul(0x846c_a68b);
    x ^ (x >> 16)
}

/// 0–1 from the low byte of `h`.
fn unit(h: u32) -> f32 {
    (h & 0xff) as f32 / 255.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_assets::RagdollPart;

    /// A stick figure 2 m tall: a torso part and a head part on it, and a loose stick.
    fn model() -> Arc<RagdollModel> {
        let identity: Matrix3x4 = [1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0];
        let capsule = |y0: f32, y1: f32, r: f32| RagdollShape::Capsule {
            a: [0.0, y0, 0.0],
            b: [0.0, y1, 0.0],
            radius: r,
        };
        Arc::new(RagdollModel {
            ragdoll: Ragdoll {
                parts: vec![
                    RagdollPart {
                        joint: 0,
                        parent: None,
                        shape: capsule(0.2, 1.3, 0.2),
                        mass: 40.0,
                    },
                    RagdollPart {
                        joint: 1,
                        parent: Some(0),
                        shape: capsule(1.6, 1.8, 0.2),
                        mass: 8.0,
                    },
                    RagdollPart {
                        joint: 2,
                        parent: None,
                        shape: RagdollShape::Box {
                            centre: [0.5, 1.0, 0.0],
                            rotation: [0.0, 0.0, 0.0, 1.0],
                            half: [0.05, 0.6, 0.05],
                        },
                        mass: 2.0,
                    },
                ],
                joint_part: vec![0, 1, 2],
            },
            origins: vec![[0.0, 0.0, 0.0], [0.0, 1.4, 0.0], [0.5, 1.0, 0.0]],
            joints: 3,
            frames: 1,
            matrices: vec![identity; 3],
            height: 2.0,
        })
    }

    /// Whether every part of body `id` sleeps.
    fn asleep(ragdolls: &mut Ragdolls, id: u32) -> bool {
        let physics = ragdolls.physics();
        physics.rest[&id]
            .parts
            .iter()
            .all(|&h| physics.world.bodies[h].is_sleeping())
    }

    /// A body that has come to rest sleeps, and from then on its pose is the same to the
    /// bit: nothing pops, and nothing is left to compute.
    #[test]
    fn a_body_that_has_settled_sleeps_where_it_lies() {
        let mut ragdolls = Ragdolls::default();
        let n = 41;
        ragdolls.set_ground(n, &vec![1.0; n * n], -20.0);
        let model = model();
        let pushes = [0.0, PUSH, 9.0];
        for (k, push) in pushes.iter().enumerate() {
            let at = Vec3::new(k as f32 * 5.0 - 5.0, 1.0, 0.0);
            let turn = Quat::from_rotation_y(k as f32);
            ragdolls.spawn(
                k as u32,
                &model,
                (Vec3::ONE, turn, at),
                0.0,
                Vec3::X * *push,
                k as u32,
            );
        }
        let mut slept: BTreeMap<u32, RagdollPose> = BTreeMap::new();
        for tick in 0..240 {
            ragdolls.step(1);
            for id in 0..pushes.len() as u32 {
                let pose = ragdolls.pose(id, 0.0).expect("falling");
                if let Some(then) = slept.get(&id) {
                    assert!(asleep(&mut ragdolls, id), "body {id} woke at {tick}");
                    assert_eq!(
                        (then.position, then.rotation, &then.parts),
                        (pose.position, pose.rotation, &pose.parts)
                    );
                } else if asleep(&mut ragdolls, id) {
                    slept.insert(id, pose);
                }
            }
        }
        assert_eq!(
            slept.len(),
            pushes.len(),
            "every body settled: {:?}",
            slept.keys()
        );
    }

    #[test]
    fn a_body_topples_away_from_the_blow_and_comes_to_rest_on_the_ground() {
        let mut ragdolls = Ragdolls::default();
        let n = 41;
        ragdolls.set_ground(n, &vec![1.0; n * n], -20.0);
        let model = model();
        let at = Vec3::new(0.0, 1.0, 0.0);
        assert!(ragdolls.spawn(
            7,
            &model,
            (Vec3::ONE, Quat::IDENTITY, at),
            0.0,
            Vec3::X * PUSH,
            1
        ));
        ragdolls.step(4);
        for _ in 0..100 {
            ragdolls.step(1);
        }
        let pose = ragdolls.pose(7, 0.0).expect("falling");
        // The head part ended up low and on the side the blow pushed to.
        let mut palette = Vec::new();
        pose.palette(&mut palette);
        let instance = Mat4::from_rotation_translation(pose.rotation, pose.position);
        let head =
            instance.transform_point3(mat4(&palette[1]).transform_point3(Vec3::new(0.0, 1.7, 0.0)));
        assert!(head.x > 0.5, "the body fell along the push: {head}");
        assert!(
            head.y < 1.6 && head.y > 0.9,
            "the head lies near the ground: {head}"
        );
        // Nothing sank through the ground.
        let stick =
            instance.transform_point3(mat4(&palette[2]).transform_point3(Vec3::new(0.5, 1.0, 0.0)));
        assert!(stick.y > 0.95, "the stick lies on the ground: {stick}");
        ragdolls.remove(7);
        assert!(ragdolls.is_empty());
    }

    /// Steps taken ahead on the stepper's thread are the steps taken in place: the same
    /// poses to the bit, with bodies arriving, leaving and falling asleep between them.
    #[test]
    fn steps_taken_ahead_fall_exactly_as_steps_taken_in_place() {
        let run = |ahead: bool| {
            let mut ragdolls = Ragdolls::default();
            let n = 41;
            ragdolls.set_ground(n, &vec![1.0; n * n], -20.0);
            let model = model();
            let mut poses = Vec::new();
            let mut slept = 0;
            for tick in 0..120u32 {
                ragdolls.step(if tick % 7 == 3 { 2 } else { 1 });
                if tick % 10 == 0 {
                    let at = Vec3::new(tick as f32 * 0.1, 1.0, 0.0);
                    ragdolls.spawn(
                        tick,
                        &model,
                        (Vec3::ONE, Quat::IDENTITY, at),
                        0.0,
                        Vec3::X * PUSH,
                        tick,
                    );
                }
                if tick == 35 {
                    ragdolls.remove(10);
                }
                for id in (0..120).step_by(10) {
                    if let Some(pose) = ragdolls.pose(id, 0.0) {
                        slept += usize::from(asleep(&mut ragdolls, id));
                        poses.push((tick, id, pose.position, pose.rotation, pose.parts));
                    }
                }
                if ahead {
                    ragdolls.step_ahead();
                }
            }
            assert!(slept > 0, "bodies fell asleep");
            poses
        };
        let (in_place, ahead) = (run(false), run(true));
        assert!(!in_place.is_empty());
        assert_eq!(in_place, ahead);
    }

    #[test]
    fn a_blow_knocks_the_body_back_and_a_hard_one_throws_it() {
        // How high the feet rose and how far they ended up along the blow.
        let fall = |push: f32| {
            let mut ragdolls = Ragdolls::default();
            let n = 41;
            ragdolls.set_ground(n, &vec![1.0; n * n], -20.0);
            let at = Vec3::new(0.0, 1.0, 0.0);
            ragdolls.spawn(
                1,
                &model(),
                (Vec3::ONE, Quat::IDENTITY, at),
                0.0,
                Vec3::X * push,
                1,
            );
            let (mut peak, mut back): (f32, f32) = (0.0, 0.0);
            for _ in 0..40 {
                ragdolls.step(1);
                let pose = ragdolls.pose(1, 0.0).unwrap();
                let mut palette = Vec::new();
                pose.palette(&mut palette);
                let instance = Mat4::from_rotation_translation(pose.rotation, pose.position);
                let foot =
                    instance.transform_point3(mat4(&palette[0]).transform_point3(Vec3::ZERO));
                peak = peak.max(foot.y - 1.0);
                back = foot.x - at.x;
            }
            (peak, back)
        };
        let (sword_peak, sword_back) = fall(PUSH);
        let (fire_peak, fire_back) = fall(9.0);
        assert!(
            sword_back > 0.8,
            "a sword's kill knocks the body back: {sword_back}"
        );
        assert!(
            fire_peak > sword_peak && fire_back > sword_back,
            "a fireball throws it higher and further: {fire_peak} {fire_back}, a sword {sword_peak} {sword_back}"
        );
    }

    #[test]
    fn a_pose_blends_between_ticks_and_its_palette_starts_at_the_handover() {
        let mut ragdolls = Ragdolls::default();
        ragdolls.set_ground(3, &[0.0; 9], -1.0);
        let model = model();
        ragdolls.spawn(
            1,
            &model,
            (Vec3::ONE, Quat::IDENTITY, Vec3::ZERO),
            0.0,
            Vec3::ZERO,
            0,
        );
        let a = ragdolls.pose(1, 0.0).unwrap();
        let mut palette = Vec::new();
        a.palette(&mut palette);
        for m in &palette {
            let d = mat4(m).transform_point3(Vec3::new(0.3, 0.7, 0.1)) - Vec3::new(0.3, 0.7, 0.1);
            assert!(
                d.length() < 1e-4,
                "at the handover the palette is the clip's"
            );
        }
        ragdolls.step(4);
        let b = ragdolls.pose(1, 0.0).unwrap();
        let mid = b.blend(&a, 0.5);
        assert!((mid.position - (a.position + b.position) * 0.5).length() < 1e-4);
    }
}
