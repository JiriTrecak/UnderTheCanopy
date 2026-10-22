//! Shot presets (`mission.shot`, docs/campaign.md "Scenes"): the camera of a single, an
//! over-the-shoulder, a two shot or a wide shot, found from the actors as the picture
//! draws them (where they stand, which way they face, their face as posed and their
//! height), so a line is seen on its speaker wherever a walk left them.
//!
//! The simulation keeps only what the script asked for ([`super::shots::Preset`]); this
//! is a pure function of the drawn actors, the ground and what stops a line of sight, so
//! saves and replays frame alike. When a scene leaves the side open, the side whose line
//! of sight scenery and buildings block less wins; a pair keeps the side it took for the
//! rest of its exchange (either way round), so a conversation stays on one side of the
//! line between its speakers.

use super::shots::{Preset, Saved};
use canopy_render::camera::Ground;
use canopy_sim::mission::cinema::{PresetKind, Side};
use glam::Vec3;
use std::collections::BTreeMap;

/// The letterbox's bars (config/ui/layouts/mission-hud.json), shares of the screen's
/// height: a shot frames its subject in the picture between them.
const BAR_TOP: f32 = 0.10;
const BAR_BOTTOM: f32 = 0.21;
/// How far a single's camera turns from the face toward the side, radians.
const SINGLE_TURN: f32 = 0.61;
/// How far a wide shot on a unit turns from its front, radians.
const WIDE_TURN: f32 = 0.7;
/// An over-the-shoulder looks past a point this far beside and above the listener's face
/// (in the listener's statures), from far enough behind it that the listener's face
/// lands this far across the picture's half width from the speaker's side.
const OVER_ASIDE: f32 = 0.45;
const OVER_RISE: f32 = 0.1;
const OVER_EDGE: f32 = 0.75;

/// An actor as the picture draws it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Actor {
    /// Its model's origin: where its feet stand.
    pub feet: Vec3,
    /// Where it faces, a horizontal unit vector.
    pub forward: Vec3,
    /// Its face as posed now (a kneel lowers it).
    pub face: Vec3,
    /// Its face's height above its feet standing: the shot's measure, so a kneeling
    /// speaker is framed as close as a standing one.
    pub stature: f32,
    /// The side its face reads best from, radians from its front toward its right: its
    /// model's portrait turn (`display.portrait.turn`), when it gives one.
    pub best: Option<f32>,
}

/// What a preset needs of the picture.
pub trait Stage {
    /// Entity `id` as drawn; `None` when it is not.
    fn actor(&self, id: u32) -> Option<Actor>;
    fn ground(&self) -> &dyn Ground;
    /// How far a ray from `origin` along unit `dir` runs before scenery, a building or a
    /// unit other than `except` stops it, up to `far`.
    fn clear(&self, origin: Vec3, dir: Vec3, far: f32, except: &[u32]) -> f32;
    /// Whether a camera may stand at `point`: over the map, not out in the forest that
    /// frames it.
    fn inside(&self, _point: Vec3) -> bool {
        true
    }
}

/// How a preset was framed when its scene left the side open: the side (seen from its
/// family's first actor) and how far round it turned.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Taken {
    side: Side,
    spread: f32,
}

/// The framings presets took when their scene left the side open, by preset family
/// ([`Preset::key`]); forgotten when a cinematic ends.
#[derive(Debug, Clone, Default)]
pub struct Sides(BTreeMap<u64, Taken>);

impl Sides {
    pub fn clear(&mut self) {
        self.0.clear();
    }
}

/// The framings tried when the side is open, in order of preference: each side at the
/// preset's own angle, then each turned further round (past a unit or a trunk in the way).
/// A single whose model names the side its face reads best from tries that first.
const TRIES: [(f32, f32); 4] = [(1.0, 1.0), (-1.0, 1.0), (1.0, 1.6), (-1.0, 1.6)];
/// A line of sight this clear counts as open.
const OPEN: f32 = 0.98;

/// The camera of `preset` on its actors; `None` when they are not drawn.
pub fn place(
    preset: &Preset,
    stage: &dyn Stage,
    aspect: f32,
    sides: Option<&mut Sides>,
) -> Option<Saved> {
    let actors = [
        preset.actors[0].and_then(|id| stage.actor(id)),
        preset.actors[1].and_then(|id| stage.actor(id)),
    ];
    let except: Vec<u32> = preset.actors.iter().flatten().copied().collect();
    let candidate =
        |sign: f32, spread: f32| frame(preset, &actors, sign, spread, aspect, stage.ground());
    // How clear the picture is: the least of five lines of sight, from the subject and
    // from points across the middle of the frame round it, back to the eye.
    let openness = |shot: &Framed| {
        let view = shot.subject - shot.eye;
        let distance = view.length();
        if !stage.inside(shot.eye) {
            return 0.0;
        }
        if distance <= 0.01 {
            return 1.0;
        }
        let across = view.cross(Vec3::Y).normalize_or_zero();
        let up = across.cross(view / distance);
        let tan = (shot.fov * 0.5).tan() * distance;
        let (wide, tall) = (tan * aspect * 0.55, tan * 0.4);
        let from = shot.subject + Vec3::Y * shot.raise;
        [
            from,
            from + across * wide,
            from - across * wide,
            from + up * tall,
            from - up * tall,
        ]
        .into_iter()
        .map(|origin| {
            let offset = shot.eye - origin;
            let length = offset.length().max(0.01);
            stage.clear(origin, offset / length, length, &except) / length
        })
        .fold(1.0, f32::min)
    };
    // A side of the pair's line flips with the speaker it is seen from.
    let seen = if preset.first { 1.0 } else { -1.0 };
    let kept = sides.as_ref().and_then(|s| s.0.get(&preset.key)).copied();
    let (sign, spread) = match (preset.side, kept) {
        (Some(side), _) => {
            // The side the scene asks for, turned further round if that sees clearer.
            let sign = sign_of(side);
            let near = openness(&candidate(sign, 1.0)?);
            let far = openness(&candidate(sign, 1.6)?);
            (
                sign,
                if near < OPEN && far > near + 0.05 {
                    1.6
                } else {
                    1.0
                },
            )
        }
        (None, Some(taken)) => (sign_of(taken.side) * seen, taken.spread),
        (None, None) => {
            let favoured = actors[0]
                .and_then(|a| a.best)
                .filter(|t| preset.kind == PresetKind::Single && t.abs() > 0.01)
                .map(|t| (t.signum(), t.abs() / SINGLE_TURN));
            // Seen from the family's first actor, so a pair takes the same side either way round.
            let tries = TRIES.map(|(sign, spread)| (sign * seen, spread));
            let mut best = (favoured.unwrap_or(tries[0]), -1.0);
            for (sign, spread) in favoured.into_iter().chain(tries) {
                let open = openness(&candidate(sign, spread)?);
                if open >= OPEN {
                    best = ((sign, spread), open);
                    break;
                }
                if open > best.1 + 0.05 {
                    best = ((sign, spread), open);
                }
            }
            let (sign, spread) = best.0;
            if let Some(sides) = sides {
                let side = if sign * seen > 0.0 {
                    Side::Right
                } else {
                    Side::Left
                };
                sides.0.insert(preset.key, Taken { side, spread });
            }
            (sign, spread)
        }
    };
    let mut shot = candidate(sign, spread)?;
    // Something still in the way of a close shot: the eye comes in front of it.
    let offset = shot.eye - shot.subject;
    let length = offset.length();
    if length > 0.01 && preset.kind != PresetKind::Wide {
        let dir = offset / length;
        let open = stage.clear(shot.subject, dir, length, &except);
        if open < length {
            shot.eye = shot.subject + dir * (open - 0.5).max(length * 0.2);
        }
    }
    Some(shot.saved(preset.roll, aspect, stage.ground()))
}

fn sign_of(side: Side) -> f32 {
    match side {
        Side::Right => 1.0,
        Side::Left => -1.0,
    }
}

/// A camera before it is aimed: its eye, the point it frames, its lens, and where across
/// the picture that point sits (−1 to 1 of the half width, + to the right).
#[derive(Debug, Clone, Copy, PartialEq)]
struct Framed {
    eye: Vec3,
    subject: Vec3,
    fov: f32,
    across: f32,
    /// How far above the subject its lines of sight are tested from (a wide shot's
    /// ground point would see every pebble as a wall).
    raise: f32,
}

impl Framed {
    /// Aimed so the subject sits in the middle of the picture between the letterbox's
    /// bars, `across` the width.
    fn saved(&self, roll: f32, aspect: f32, ground: &dyn Ground) -> Saved {
        let d = self.subject - self.eye;
        let distance = d.length().max(0.01);
        let tan = (self.fov * 0.5).tan();
        // The bars' middle stands this share of the half height above the screen's.
        let lift = 1.0 - BAR_TOP - (1.0 - BAR_TOP - BAR_BOTTOM) * 0.5 - 0.5;
        let down = (2.0 * lift * tan).atan();
        let turn = (self.across * aspect * tan).atan();
        let yaw = d.x.atan2(d.z) + turn;
        let pitch = (d.y / distance).clamp(-1.0, 1.0).asin() - down;
        let dir = Vec3::new(
            yaw.sin() * pitch.cos(),
            pitch.sin(),
            yaw.cos() * pitch.cos(),
        );
        Saved::looking(self.eye, self.eye + dir * distance, self.fov, roll, ground)
    }
}

/// The share of the screen's height the picture shows between the bars.
fn visible() -> f32 {
    1.0 - BAR_TOP - BAR_BOTTOM
}

/// The distance at which a lens of `fov` shows `span` between the bars.
fn distance_for(span: f32, fov: f32) -> f32 {
    span / (visible() * 2.0 * (fov * 0.5).tan())
}

/// The lens that shows `span` between the bars from `distance` away.
fn fov_for(span: f32, distance: f32) -> f32 {
    2.0 * (span / (visible() * 2.0 * distance.max(0.01))).atan()
}

/// The right of a horizontal direction (Y up).
fn right(forward: Vec3) -> Vec3 {
    Vec3::new(-forward.z, 0.0, forward.x)
}

/// `forward` turned `angle` radians toward its right.
fn turned(forward: Vec3, angle: f32) -> Vec3 {
    forward * angle.cos() + right(forward) * angle.sin()
}

fn horizontal(v: Vec3) -> Option<Vec3> {
    Vec3::new(v.x, 0.0, v.z).try_normalize()
}

/// Where a subject's look puts it across the picture from `eye`: away from where it
/// looks, so it looks into the frame.
fn look_room(eye: Vec3, subject: Vec3, gaze: Vec3, share: f32) -> f32 {
    let Some(view) = horizontal(subject - eye) else {
        return 0.0;
    };
    -share * gaze.dot(right(view)).clamp(-1.0, 1.0)
}

/// The eye at horizontal direction `dir` from `subject`, `distance` from it, at height
/// `y`.
fn eye_at(subject: Vec3, dir: Vec3, distance: f32, y: f32) -> Vec3 {
    let rise = (y - subject.y).clamp(-distance * 0.9, distance * 0.9);
    let across = (distance * distance - rise * rise).max(0.0).sqrt();
    Vec3::new(subject.x, y, subject.z) + dir * across
}

/// The preset's camera on the `sign` side (+1 right, −1 left, seen from its first actor),
/// turned `spread` times its usual angle round.
fn frame(
    preset: &Preset,
    actors: &[Option<Actor>; 2],
    sign: f32,
    spread: f32,
    aspect: f32,
    ground: &dyn Ground,
) -> Option<Framed> {
    match preset.kind {
        PresetKind::Single => Some(single(preset, &actors[0]?, sign, spread)),
        PresetKind::Over => Some(over(preset, &actors[0]?, &actors[1]?, sign, spread, aspect)),
        PresetKind::Two => Some(two(preset, &actors[0]?, &actors[1]?, sign, spread, aspect)),
        PresetKind::Wide => match (preset.radius, actors[0]) {
            (Some(radius), _) => Some(wide_region(preset, radius, sign, spread, ground)),
            (None, Some(actor)) => Some(wide_unit(preset, &actor, sign, spread)),
            (None, None) => None,
        },
    }
}

/// Head and shoulders to the waist, from the front `sign` side.
fn single(p: &Preset, a: &Actor, sign: f32, spread: f32) -> Framed {
    let h = a.stature.max(0.1);
    let fov = p.fov.unwrap_or(30f32.to_radians());
    // The face in the picture's upper third: the waist at the lower bar.
    let subject = a.face - Vec3::Y * (0.12 * h);
    let distance = p.distance.unwrap_or_else(|| distance_for(0.7 * h, fov));
    let y = a.feet.y + p.height.unwrap_or(0.92 * (a.face.y - a.feet.y));
    let turn = sign * SINGLE_TURN * spread;
    let eye = eye_at(subject, turned(a.forward, turn), distance, y);
    Framed {
        eye,
        subject,
        fov,
        across: look_room(eye, subject, a.forward, 0.3),
        raise: 0.0,
    }
}

/// Over `from`'s `sign` shoulder onto `to`'s face; `from`'s shoulder at the frame's edge.
fn over(p: &Preset, from: &Actor, to: &Actor, sign: f32, spread: f32, aspect: f32) -> Framed {
    let u = horizontal(to.feet - from.feet).unwrap_or(from.forward);
    let h = from.stature.max(0.1);
    let subject = to.face - Vec3::Y * (0.12 * to.stature.max(0.1));
    let aside = OVER_ASIDE * spread * h;
    // The line of sight passes beside the listener's head, so the head stands just off
    // it, toward the picture's edge.
    let past = from.face + right(u) * (sign * aside) + Vec3::Y * (OVER_RISE * h);
    let dir = (past - subject).normalize_or(-u);
    let lens = |distance: f32| {
        p.fov.unwrap_or_else(|| {
            fov_for(0.9 * to.stature.max(0.1), distance)
                .clamp(14f32.to_radians(), 42f32.to_radians())
        })
    };
    // Back from there until the head lands at the edge: further for a long lens (a
    // speaker far away), closer for a wide one. The lens follows the distance.
    let reach = past.distance(subject);
    let across = 0.3 * sign;
    let mut back = 0.5 * h;
    for _ in 0..3 {
        let half = aspect * (lens(reach + back) * 0.5).tan();
        back = (aside / ((OVER_EDGE + across.abs()) * half)).clamp(0.3 * h, 4.0 * h);
    }
    let mut eye = past + dir * back;
    if let Some(height) = p.height {
        eye.y = from.feet.y + height;
    }
    if let Some(distance) = p.distance {
        eye = subject + (eye - subject).normalize_or_zero() * distance;
    }
    Framed {
        eye,
        subject,
        fov: lens(eye.distance(subject)),
        across,
        raise: 0.0,
    }
}

/// Both from the `sign` side of the line between them, from the thighs up.
fn two(p: &Preset, a: &Actor, b: &Actor, sign: f32, spread: f32, aspect: f32) -> Framed {
    let u = horizontal(b.feet - a.feet).unwrap_or(a.forward);
    let h = a.stature.max(b.stature).max(0.1);
    let fov = p.fov.unwrap_or(36f32.to_radians());
    let faces = (a.face + b.face) * 0.5;
    let subject = faces - Vec3::Y * (0.3 * h);
    let apart = Vec3::new(b.feet.x - a.feet.x, 0.0, b.feet.z - a.feet.z).length();
    let tall = distance_for(0.85 * h, fov);
    let wide = (apart + 0.9 * h) / (2.0 * aspect * (fov * 0.5).tan());
    let distance = p.distance.unwrap_or(tall.max(wide));
    let y = (a.feet.y + b.feet.y) * 0.5 + p.height.unwrap_or(0.95 * h);
    let dir = turned(right(u) * sign, (spread - 1.0) * 0.6);
    Framed {
        eye: eye_at(subject, dir, distance, y),
        subject,
        fov,
        across: 0.0,
        raise: 0.0,
    }
}

/// A unit in its surroundings, from its front `sign` side and above.
fn wide_unit(p: &Preset, a: &Actor, sign: f32, spread: f32) -> Framed {
    let h = a.stature.max(0.1);
    let fov = p.fov.unwrap_or(40f32.to_radians());
    let subject = a.feet + Vec3::Y * (0.5 * h);
    let distance = p.distance.unwrap_or(6.5 * h);
    let y = a.feet.y + p.height.unwrap_or(distance * 0.3);
    let turn = sign * WIDE_TURN * spread;
    Framed {
        eye: eye_at(subject, turned(a.forward, turn), distance, y),
        subject,
        fov,
        across: 0.0,
        raise: 0.0,
    }
}

/// A region from the south, turned toward the `sign` side, the whole of it in view.
fn wide_region(p: &Preset, radius: f32, sign: f32, spread: f32, ground: &dyn Ground) -> Framed {
    let fov = p.fov.unwrap_or(40f32.to_radians());
    let [x, z] = p.at;
    let subject = Vec3::new(x, ground.height(x, z).max(0.0), z);
    let distance = p.distance.unwrap_or((radius * 2.2).max(24.0));
    let y = subject.y + p.height.unwrap_or(distance * 0.36);
    let dir = turned(Vec3::Z, -sign * 0.5 * spread);
    Framed {
        eye: eye_at(subject, dir, distance, y),
        subject,
        fov,
        across: 0.0,
        raise: radius * 0.3,
    }
}

/// An actor from the model the picture draws: `transform` places it, `rest` is its
/// bind-pose bounds, `head` its humanoid neck and head top at rest and as posed, and
/// `portrait` its model's own portrait framing (all model space).
pub fn actor(
    transform: glam::Mat4,
    rest: (Vec3, Vec3),
    head: Option<((Vec3, Vec3), (Vec3, Vec3))>,
    portrait: Option<canopy_render::PortraitFraming>,
) -> Actor {
    let focus = portrait.map(|p| p.focus);
    let feet = transform.w_axis.truncate();
    let forward = horizontal(transform.transform_vector3(Vec3::Z)).unwrap_or(Vec3::Z);
    let point = |p: Vec3| transform.transform_point3(p);
    // The face sits a little above the middle of the neck to the head's top.
    let face_of = |(neck, top): (Vec3, Vec3)| neck.lerp(top, 0.6);
    let (face, standing) = match (head, focus) {
        (Some((rest_head, posed)), _) => (point(face_of(posed)), point(face_of(rest_head))),
        (None, Some(focus)) => (point(focus), point(focus)),
        (None, None) => {
            let (lo, hi) = rest;
            let middle = (lo + hi) * 0.5;
            let face = Vec3::new(
                middle.x,
                lo.y + 0.85 * (hi.y - lo.y),
                middle.z + 0.3 * (hi.z - middle.z),
            );
            (point(face), point(face))
        }
    };
    Actor {
        feet,
        forward,
        face,
        stature: (standing.y - feet.y).max(0.1),
        // A portrait turns its model the other way to show that side.
        best: portrait.and_then(|p| p.turn),
    }
}

/// The game's stage: the live scene's models as the renderer has them, the map's ground,
/// and its scenery and buildings in the way of a line of sight.
pub struct Live<'a> {
    pub live: &'a crate::live::LiveScene,
    pub actors: Option<&'a canopy_render::actors::ActorsGpu>,
    pub obstacles: &'a canopy_scene::obstruction::Obstacles,
    pub ground: &'a dyn Ground,
    /// The map's side, cells.
    pub size: f32,
}

impl Stage for Live<'_> {
    fn actor(&self, id: u32) -> Option<Actor> {
        let (instance, portrait) = self.live.entity_actor(id)?;
        let models = self.actors?;
        let rest = models.rest_bounds(instance.model)?;
        let head = models.head(instance.model).zip(models.posed_head(instance));
        Some(actor(instance.transform, rest, head, portrait))
    }

    fn ground(&self) -> &dyn Ground {
        self.ground
    }

    fn inside(&self, point: Vec3) -> bool {
        let margin = 2.0;
        let span = margin..=self.size - margin;
        span.contains(&point.x) && span.contains(&point.z)
    }

    fn clear(&self, origin: Vec3, dir: Vec3, far: f32, except: &[u32]) -> f32 {
        let scenery = self
            .live
            .camera_obstruction(self.obstacles, origin, dir, far);
        // Units stand as upright cylinders a little wider than their selection circle
        // (wings, banners, staffs).
        self.live
            .bodies()
            .iter()
            .filter(|(id, ..)| !except.contains(id))
            .filter_map(|&(_, [radius, height], foot)| {
                upright(origin, dir, foot, radius * UNIT_GIRTH, height)
            })
            .fold(scenery, f32::min)
    }
}

/// How much wider than its selection circle a unit stands in a line of sight.
const UNIT_GIRTH: f32 = 1.4;

/// Where a ray from `origin` along unit `dir` enters the upright cylinder of `radius`
/// standing `height` on `foot`; `None` when it misses or starts inside.
fn upright(origin: Vec3, dir: Vec3, foot: Vec3, radius: f32, height: f32) -> Option<f32> {
    let (ox, oz) = (origin.x - foot.x, origin.z - foot.z);
    let a = dir.x * dir.x + dir.z * dir.z;
    if a < 1e-8 {
        return None;
    }
    let b = ox * dir.x + oz * dir.z;
    let c = ox * ox + oz * oz - radius * radius;
    let disc = b * b - a * c;
    if c <= 0.0 || disc < 0.0 {
        return None;
    }
    let t = (-b - disc.sqrt()) / a;
    let y = origin.y + dir.y * t;
    (t > 0.0 && y >= foot.y && y <= foot.y + height).then_some(t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_render::camera::FlatGround;

    /// Open ground with an optional wall: a ray stops where it crosses `x = wall` (and
    /// stands below its top).
    struct Field {
        actors: Vec<Actor>,
        wall: Option<(f32, f32)>,
    }

    impl Stage for Field {
        fn actor(&self, id: u32) -> Option<Actor> {
            self.actors.get(id as usize).copied()
        }
        fn ground(&self) -> &dyn Ground {
            &FlatGround
        }
        fn clear(&self, origin: Vec3, dir: Vec3, far: f32, _: &[u32]) -> f32 {
            let Some((x, top)) = self.wall else {
                return far;
            };
            if dir.x.abs() < 1e-6 {
                return far;
            }
            let t = (x - origin.x) / dir.x;
            if t > 0.0 && t < far && origin.y + dir.y * t < top {
                t
            } else {
                far
            }
        }
    }

    fn standing(x: f32, z: f32, facing: Vec3) -> Actor {
        Actor {
            feet: Vec3::new(x, 0.0, z),
            forward: facing,
            face: Vec3::new(x, 10.0, z) + facing * 0.5,
            stature: 10.0,
            best: None,
        }
    }

    fn preset(kind: PresetKind, actors: [Option<u32>; 2]) -> Preset {
        Preset {
            kind,
            actors,
            at: [0.0, 0.0],
            radius: None,
            reach: 0.0,
            side: None,
            distance: None,
            height: None,
            fov: None,
            roll: 0.0,
            key: 1,
            first: true,
        }
    }

    /// Where a world point lands in the picture: x and y from −1 to 1 (y up), and
    /// whether it is in front of the camera.
    fn project(saved: &Saved, point: Vec3) -> (f32, f32, bool) {
        let view = saved.view(16.0 / 9.0, &FlatGround);
        let clip = view.view_projection * point.extend(1.0);
        (clip.x / clip.w, clip.y / clip.w, clip.w > 0.0)
    }

    /// Whether a picture point is between the letterbox's bars.
    fn between_bars(y: f32) -> bool {
        let top = 1.0 - 2.0 * BAR_TOP;
        let bottom = -1.0 + 2.0 * BAR_BOTTOM;
        y < top && y > bottom
    }

    /// A single finds its speaker wherever it stands, its face in the picture's upper
    /// half between the bars, seen from its front.
    #[test]
    fn a_single_frames_the_face_from_the_front() {
        for (x, z, facing) in [
            (10.0, 20.0, Vec3::Z),
            (-40.0, 5.0, Vec3::X),
            (3.0, -8.0, Vec3::new(-0.6, 0.0, -0.8)),
        ] {
            let a = standing(x, z, facing);
            let field = Field {
                actors: vec![a],
                wall: None,
            };
            let p = preset(PresetKind::Single, [Some(0), None]);
            let saved = place(&p, &field, 16.0 / 9.0, None).unwrap();
            let view = saved.view(16.0 / 9.0, &FlatGround);
            let (fx, fy, front) = project(&saved, a.face);
            assert!(front && fx.abs() < 0.5, "{fx}");
            assert!(
                between_bars(fy) && fy > 0.0,
                "the face high in the picture: {fy}"
            );
            // The waist low in the picture, the feet out of it.
            let (_, wy, _) = project(&saved, a.feet + Vec3::Y * 6.0);
            assert!(between_bars(wy) && wy < 0.0, "{wy}");
            let (_, feet, _) = project(&saved, a.feet);
            assert!(!between_bars(feet), "{feet}");
            // From the front: the eye stands where the face looks.
            assert!((view.eye - a.feet).dot(facing) > 0.0);
        }
    }

    /// An over-the-shoulder looks past the listener onto the speaker's face, and the
    /// pair keeps the side it took, either way round.
    #[test]
    fn over_the_shoulder_keeps_to_one_side_of_the_line() {
        let a = standing(0.0, 0.0, Vec3::Z);
        let b = standing(0.0, 20.0, -Vec3::Z);
        let field = Field {
            actors: vec![a, b],
            wall: None,
        };
        let mut sides = Sides::default();
        let onto_b = preset(PresetKind::Over, [Some(0), Some(1)]);
        let saved = place(&onto_b, &field, 16.0 / 9.0, Some(&mut sides)).unwrap();
        let eye = saved.view(16.0 / 9.0, &FlatGround).eye;
        let (bx, by, front) = project(&saved, b.face);
        assert!(front && bx.abs() < 0.6 && between_bars(by), "{bx} {by}");
        // Behind the listener, beside the line, the listener's head at the other edge.
        assert!(eye.z < 0.0 && eye.x.abs() > 1.0, "{eye}");
        let (lx, _, front) = project(&saved, a.face);
        assert!(
            front && lx * bx < 0.0 && (0.5..1.2).contains(&lx.abs()),
            "{lx} {bx}"
        );
        // A speaker far off (on a wall across a yard): a longer lens from further back,
        // the listener still at the edge.
        let far = standing(0.0, 60.0, -Vec3::Z);
        let yard = Field {
            actors: vec![a, far],
            wall: None,
        };
        let long = place(&onto_b, &yard, 16.0 / 9.0, None).unwrap();
        let (fx, fy, front) = project(&long, far.face);
        let (lx, _, _) = project(&long, a.face);
        assert!(front && fx.abs() < 0.6 && between_bars(fy), "{fx} {fy}");
        assert!(lx * fx < 0.0 && (0.5..1.2).contains(&lx.abs()), "{lx}");
        assert!(long.fov < saved.fov && long.view(16.0 / 9.0, &FlatGround).eye.z < eye.z);
        // The reverse: from behind the speaker, on the same side of the line.
        let onto_a = Preset {
            actors: [Some(1), Some(0)],
            first: false,
            ..onto_b
        };
        let reverse = place(&onto_a, &field, 16.0 / 9.0, Some(&mut sides)).unwrap();
        let back = reverse.view(16.0 / 9.0, &FlatGround).eye;
        assert!(back.z > 20.0 && back.x * eye.x > 0.0, "{eye} {back}");
        let (ax, ay, front) = project(&reverse, a.face);
        assert!(front && ax.abs() < 0.6 && between_bars(ay));
        // A single frame (a capture, a loaded save) keeps to the same side unprompted.
        let fresh = place(&onto_a, &field, 16.0 / 9.0, None).unwrap();
        assert_eq!(fresh, reverse);
    }

    /// Left open, a shot takes the side scenery leaves clear; asked for, the side it is
    /// told, the eye then brought in front of what blocks it.
    #[test]
    fn the_side_with_the_clearer_view_wins() {
        let a = standing(0.0, 0.0, Vec3::Z);
        // A tall wall on the unit's right front (−x, for a unit facing +z).
        let walled = Field {
            actors: vec![a],
            wall: Some((-3.0, 50.0)),
        };
        let p = preset(PresetKind::Single, [Some(0), None]);
        let eye = |field: &Field, p: &Preset| {
            place(p, field, 16.0 / 9.0, None)
                .unwrap()
                .view(16.0 / 9.0, &FlatGround)
                .eye
        };
        assert!(eye(&walled, &p).x > 0.0, "the open left");
        let open = Field {
            actors: vec![a],
            wall: None,
        };
        assert!(eye(&open, &p).x < 0.0, "the right by default");
        let told = Preset {
            side: Some(Side::Right),
            ..p
        };
        let pulled = eye(&walled, &told);
        assert!(pulled.x >= -3.0 && pulled.x < 0.0, "{pulled}");
    }

    /// A two shot shows both faces; a wide shot of a region shows all of it.
    #[test]
    fn two_and_wide_shots_frame_their_subjects() {
        let a = standing(0.0, 0.0, Vec3::X);
        let b = standing(14.0, 0.0, -Vec3::X);
        let field = Field {
            actors: vec![a, b],
            wall: None,
        };
        let saved = place(
            &preset(PresetKind::Two, [Some(0), Some(1)]),
            &field,
            16.0 / 9.0,
            None,
        )
        .unwrap();
        for actor in [a, b] {
            let (x, y, front) = project(&saved, actor.face);
            assert!(front && x.abs() < 0.95 && between_bars(y), "{x} {y}");
        }
        let region = Preset {
            radius: Some(12.0),
            at: [50.0, 50.0],
            ..preset(PresetKind::Wide, [None, None])
        };
        let saved = place(&region, &field, 16.0 / 9.0, None).unwrap();
        for edge in [Vec3::new(38.0, 0.0, 50.0), Vec3::new(62.0, 0.0, 50.0)] {
            let (x, y, front) = project(&saved, edge);
            assert!(front && x.abs() < 1.0 && between_bars(y), "{x} {y}");
        }
        // No actor drawn: nothing to frame.
        let gone = preset(PresetKind::Single, [Some(7), None]);
        assert!(place(&gone, &field, 16.0 / 9.0, None).is_none());
    }
}
