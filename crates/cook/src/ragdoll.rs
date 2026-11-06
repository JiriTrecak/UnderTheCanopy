//! Ragdolls fitted to a skin (D114): how a unit's body falls when it dies.
//!
//! Every vertex belongs to the joint that weighs most on it. Joints that carry enough of
//! the body become parts; the rest ride the part of their nearest ancestor that is one
//! (fingers ride the hand, the armature root rides the hips). Each part is a capsule along
//! the longest axis of its vertices, hanging from its parent part at the joint that starts
//! it. Held equipment (a rigid node, never skinned) comes loose as its own box. Nothing is
//! authored per unit: the skin is the input.

use crate::math::Vec3;
use canopy_assets::{Joint, Ragdoll, RagdollPart, RagdollShape, SkinVertex};

/// Most parts a body keeps; the largest win.
const MAX_PARTS: usize = 16;
/// A joint becomes a part with at least this share of the vertices …
const PART_SHARE: f64 = 0.004;
/// … and no fewer than this many.
const PART_VERTICES: usize = 8;
/// … spread over at least this share of the model's height.
const PART_EXTENT: f64 = 0.11;
/// Loose equipment smaller than this share of the height stays on its part.
const LOOSE_EXTENT: f64 = 0.12;
/// Kilograms per cubic model metre.
const DENSITY: f64 = 1000.0;
/// The share of its box a held weapon fills (blades and hafts are thin).
const LOOSE_FILL: f64 = 0.3;

/// The ragdoll of a skinned model, or none when too little of it is skinned to fall.
/// `positions` and `skin` are the cooked vertices (model space, rest pose); `rigid` marks
/// the joints that are rigid mesh nodes (equipment held in a hand).
pub fn fit(
    joints: &[Joint],
    positions: &[Vec3],
    skin: &[SkinVertex],
    rigid: &[bool],
    height: f64,
) -> Option<Ragdoll> {
    if joints.is_empty() || positions.len() != skin.len() || height <= 0.0 {
        return None;
    }
    let n = joints.len();
    let mut points: Vec<Vec<Vec3>> = vec![Vec::new(); n];
    for (p, s) in positions.iter().zip(skin) {
        let (mut best, mut weight) = (0usize, 0u8);
        for k in 0..4 {
            if s.weights[k] > weight {
                weight = s.weights[k];
                best = usize::from(s.joints[k]);
            }
        }
        if best < n {
            points[best].push(*p);
        }
    }
    let least = PART_VERTICES.max((positions.len() as f64 * PART_SHARE) as usize);
    let extent = |pts: &[Vec3]| {
        let (lo, hi) = bounds(pts);
        dist(lo, hi)
    };
    let loose =
        |j: usize| rigid[j] && points[j].len() >= 4 && extent(&points[j]) >= LOOSE_EXTENT * height;
    let mut candidates: Vec<usize> = (0..n)
        .filter(|&j| {
            loose(j)
                || (!rigid[j]
                    && points[j].len() >= least
                    && extent(&points[j]) >= PART_EXTENT * height)
        })
        .collect();
    if candidates.iter().filter(|&&j| !rigid[j]).count() < 2 {
        return None;
    }
    if candidates.len() > MAX_PARTS {
        candidates.sort_by_key(|&j| std::cmp::Reverse(points[j].len()));
        candidates.truncate(MAX_PARTS);
        candidates.sort_unstable();
    }
    let is_part = |j: usize| candidates.binary_search(&j).is_ok();
    // The nearest ancestor (or self) that is a part.
    let owner = |mut j: usize| -> Option<usize> {
        for _ in 0..=n {
            if is_part(j) {
                return Some(j);
            }
            j = joints[j].parent? as usize;
        }
        None
    };
    // The root: the body part without a part above it that carries the most.
    let root = candidates
        .iter()
        .copied()
        .filter(|&j| !rigid[j])
        .filter(|&j| joints[j].parent.and_then(|p| owner(p as usize)).is_none())
        .max_by_key(|&j| points[j].len())?;
    let index = |j: usize| candidates.binary_search(&j).expect("a part") as u32;
    let joint_part: Vec<u32> = (0..n).map(|j| index(owner(j).unwrap_or(root))).collect();

    let mut gathered: Vec<Vec<Vec3>> = vec![Vec::new(); candidates.len()];
    for (j, pts) in points.iter().enumerate() {
        gathered[joint_part[j] as usize].extend_from_slice(pts);
    }
    let parts = candidates
        .iter()
        .zip(&gathered)
        .map(|(&j, pts)| {
            let parent = if rigid[j] || j == root {
                None
            } else {
                Some(index(
                    joints[j]
                        .parent
                        .and_then(|p| owner(p as usize))
                        .unwrap_or(root),
                ))
            };
            let (shape, volume) = if rigid[j] {
                let (shape, volume) = fit_box(pts);
                (shape, volume * LOOSE_FILL)
            } else {
                fit_capsule(pts, 0.015 * height)
            };
            RagdollPart {
                joint: j as u32,
                parent,
                shape,
                mass: (volume * DENSITY).max(0.01) as f32,
            }
        })
        .collect();
    Some(Ragdoll { parts, joint_part })
}

/// A capsule along the points' longest axis: its ends at the 2nd and 98th percentiles
/// along it, its radius the 75th percentile of the distances from it.
fn fit_capsule(points: &[Vec3], least_radius: f64) -> (RagdollShape, f64) {
    let (mean, axes) = principal_axes(points);
    let axis = axes[0];
    let mut along: Vec<f64> = points.iter().map(|p| dot(sub(*p, mean), axis)).collect();
    let mut across: Vec<f64> = points
        .iter()
        .zip(&along)
        .map(|(p, &t)| dist(*p, add(mean, scale(axis, t))))
        .collect();
    along.sort_by(f64::total_cmp);
    across.sort_by(f64::total_cmp);
    let (lo, hi) = (percentile(&along, 0.02), percentile(&along, 0.98));
    let radius = percentile(&across, 0.75)
        .max(least_radius)
        .min(((hi - lo) * 0.5).max(least_radius));
    let (mut a, mut b) = (lo + radius, hi - radius);
    if a > b {
        let mid = (lo + hi) * 0.5;
        (a, b) = (mid, mid);
    }
    let length = b - a;
    let volume = std::f64::consts::PI * radius * radius * (length + 4.0 / 3.0 * radius);
    let point = |t: f64| add(mean, scale(axis, t)).map(|v| v as f32);
    (
        RagdollShape::Capsule {
            a: point(a),
            b: point(b),
            radius: radius as f32,
        },
        volume,
    )
}

/// A box along the points' principal axes, over their 1st to 99th percentiles.
fn fit_box(points: &[Vec3]) -> (RagdollShape, f64) {
    let (mean, axes) = principal_axes(points);
    let mut centre = mean;
    let mut half = [0.0; 3];
    for (k, axis) in axes.iter().enumerate() {
        let mut t: Vec<f64> = points.iter().map(|p| dot(sub(*p, mean), *axis)).collect();
        t.sort_by(f64::total_cmp);
        let (lo, hi) = (percentile(&t, 0.01), percentile(&t, 0.99));
        half[k] = ((hi - lo) * 0.5).max(0.005);
        centre = add(centre, scale(*axis, (lo + hi) * 0.5));
    }
    let volume = 8.0 * half[0] * half[1] * half[2];
    (
        RagdollShape::Box {
            centre: centre.map(|v| v as f32),
            rotation: quat_from_axes(axes).map(|v| v as f32),
            half: half.map(|v| v as f32),
        },
        volume,
    )
}

/// The points' mean and their principal axes, longest first, right-handed.
fn principal_axes(points: &[Vec3]) -> (Vec3, [Vec3; 3]) {
    let count = points.len().max(1) as f64;
    let mean = scale(points.iter().fold([0.0; 3], |s, p| add(s, *p)), 1.0 / count);
    let mut c = [[0.0; 3]; 3];
    for p in points {
        let d = sub(*p, mean);
        for i in 0..3 {
            for j in 0..3 {
                c[i][j] += d[i] * d[j] / count;
            }
        }
    }
    let (values, vectors) = jacobi(c);
    let mut order = [0usize, 1, 2];
    order.sort_by(|&a, &b| values[b].total_cmp(&values[a]));
    let column = |k: usize| [vectors[0][k], vectors[1][k], vectors[2][k]];
    let x = normalize(column(order[0]));
    let y = normalize(column(order[1]));
    let z = cross(x, y);
    (mean, [x, y, z])
}

/// Eigenvalues and eigenvectors (columns) of a symmetric 3 × 3 matrix by Jacobi rotations:
/// only square roots, so every platform cooks the same shapes.
#[allow(clippy::needless_range_loop)]
fn jacobi(mut a: [[f64; 3]; 3]) -> ([f64; 3], [[f64; 3]; 3]) {
    let mut v = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    for _ in 0..32 {
        let mut off = 0.0;
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            off += a[p][q] * a[p][q];
        }
        if off < 1e-24 {
            break;
        }
        for (p, q) in [(0, 1), (0, 2), (1, 2)] {
            if a[p][q].abs() < 1e-30 {
                continue;
            }
            let theta = (a[q][q] - a[p][p]) / (2.0 * a[p][q]);
            let t = theta.signum() / (theta.abs() + (theta * theta + 1.0).sqrt());
            let t = if theta == 0.0 { 1.0 } else { t };
            let c = 1.0 / (t * t + 1.0).sqrt();
            let s = t * c;
            for k in 0..3 {
                let (akp, akq) = (a[k][p], a[k][q]);
                a[k][p] = c * akp - s * akq;
                a[k][q] = s * akp + c * akq;
            }
            for k in 0..3 {
                let (apk, aqk) = (a[p][k], a[q][k]);
                a[p][k] = c * apk - s * aqk;
                a[q][k] = s * apk + c * aqk;
            }
            for row in &mut v {
                let (vp, vq) = (row[p], row[q]);
                row[p] = c * vp - s * vq;
                row[q] = s * vp + c * vq;
            }
        }
    }
    ([a[0][0], a[1][1], a[2][2]], v)
}

/// The rotation (x, y, z, w) whose columns are `axes`.
fn quat_from_axes(axes: [Vec3; 3]) -> [f64; 4] {
    let m = |r: usize, c: usize| axes[c][r];
    let trace = m(0, 0) + m(1, 1) + m(2, 2);
    let q = if trace > 0.0 {
        let s = (trace + 1.0).sqrt() * 2.0;
        [
            (m(2, 1) - m(1, 2)) / s,
            (m(0, 2) - m(2, 0)) / s,
            (m(1, 0) - m(0, 1)) / s,
            0.25 * s,
        ]
    } else if m(0, 0) > m(1, 1) && m(0, 0) > m(2, 2) {
        let s = (1.0 + m(0, 0) - m(1, 1) - m(2, 2)).sqrt() * 2.0;
        [
            0.25 * s,
            (m(0, 1) + m(1, 0)) / s,
            (m(0, 2) + m(2, 0)) / s,
            (m(2, 1) - m(1, 2)) / s,
        ]
    } else if m(1, 1) > m(2, 2) {
        let s = (1.0 + m(1, 1) - m(0, 0) - m(2, 2)).sqrt() * 2.0;
        [
            (m(0, 1) + m(1, 0)) / s,
            0.25 * s,
            (m(1, 2) + m(2, 1)) / s,
            (m(0, 2) - m(2, 0)) / s,
        ]
    } else {
        let s = (1.0 + m(2, 2) - m(0, 0) - m(1, 1)).sqrt() * 2.0;
        [
            (m(0, 2) + m(2, 0)) / s,
            (m(1, 2) + m(2, 1)) / s,
            0.25 * s,
            (m(1, 0) - m(0, 1)) / s,
        ]
    };
    crate::math::quat_normalize(q)
}

fn percentile(sorted: &[f64], share: f64) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    sorted[((sorted.len() - 1) as f64 * share).round() as usize]
}

fn bounds(points: &[Vec3]) -> (Vec3, Vec3) {
    points.iter().fold(
        ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]),
        |(lo, hi), p| {
            (
                [lo[0].min(p[0]), lo[1].min(p[1]), lo[2].min(p[2])],
                [hi[0].max(p[0]), hi[1].max(p[1]), hi[2].max(p[2])],
            )
        },
    )
}

fn add(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}
fn sub(a: Vec3, b: Vec3) -> Vec3 {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}
fn scale(a: Vec3, s: f64) -> Vec3 {
    [a[0] * s, a[1] * s, a[2] * s]
}
fn dot(a: Vec3, b: Vec3) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: Vec3, b: Vec3) -> Vec3 {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}
fn dist(a: Vec3, b: Vec3) -> f64 {
    dot(sub(a, b), sub(a, b)).sqrt()
}
fn normalize(v: Vec3) -> Vec3 {
    crate::math::normalize(v)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn joint(name: &str, parent: Option<u32>, origin: [f32; 3]) -> Joint {
        Joint {
            name: name.into(),
            parent,
            origin,
        }
    }

    /// Points along a segment with a little thickness, all weighed to `joint`.
    fn limb(
        from: Vec3,
        to: Vec3,
        radius: f64,
        joint: u8,
        positions: &mut Vec<Vec3>,
        skin: &mut Vec<SkinVertex>,
    ) {
        for i in 0..=20 {
            let t = f64::from(i) / 20.0;
            let c = add(from, scale(sub(to, from), t));
            for (dx, dz) in [(radius, 0.0), (-radius, 0.0), (0.0, radius), (0.0, -radius)] {
                positions.push([c[0] + dx, c[1], c[2] + dz]);
                skin.push(SkinVertex {
                    joints: [joint, 0, 0, 0],
                    weights: [255, 0, 0, 0],
                });
            }
        }
    }

    #[test]
    fn a_body_falls_in_parts_and_its_weapon_comes_loose() {
        // An armature root, hips and a standing torso, one leg, a finger too small to be a
        // part, and a sword held in the hand.
        let joints = vec![
            joint("Armature", None, [0.0, 0.0, 0.0]),
            joint("Hips", Some(0), [0.0, 0.5, 0.0]),
            joint("Spine", Some(1), [0.0, 0.6, 0.0]),
            joint("Leg", Some(1), [0.1, 0.5, 0.0]),
            joint("Hand", Some(2), [0.3, 0.8, 0.0]),
            joint("Finger", Some(4), [0.5, 0.8, 0.0]),
            joint("Sword", Some(4), [0.3, 0.8, 0.0]),
        ];
        let (mut positions, mut skin) = (Vec::new(), Vec::new());
        limb(
            [0.0, 0.45, 0.0],
            [0.0, 0.6, 0.0],
            0.1,
            1,
            &mut positions,
            &mut skin,
        );
        limb(
            [0.0, 0.6, 0.0],
            [0.0, 1.0, 0.0],
            0.12,
            2,
            &mut positions,
            &mut skin,
        );
        limb(
            [0.1, 0.5, 0.0],
            [0.1, 0.0, 0.0],
            0.05,
            3,
            &mut positions,
            &mut skin,
        );
        limb(
            [0.3, 0.8, 0.0],
            [0.5, 0.8, 0.0],
            0.03,
            4,
            &mut positions,
            &mut skin,
        );
        positions.push([0.52, 0.8, 0.0]);
        skin.push(SkinVertex {
            joints: [5, 0, 0, 0],
            weights: [255, 0, 0, 0],
        });
        limb(
            [0.3, 0.8, 0.0],
            [0.3, 1.4, 0.0],
            0.02,
            6,
            &mut positions,
            &mut skin,
        );
        let rigid = [false, false, false, false, false, false, true];
        let ragdoll = fit(&joints, &positions, &skin, &rigid, 1.4).expect("a ragdoll");

        let part_of = |j: usize| &ragdoll.parts[ragdoll.joint_part[j] as usize];
        assert_eq!(part_of(1).joint, 1, "the hips are the root");
        assert_eq!(part_of(1).parent, None);
        assert_eq!(part_of(0).joint, 1, "the armature rides the hips");
        assert_eq!(part_of(5).joint, 4, "the finger rides the hand");
        let spine = ragdoll.joint_part[2];
        assert_eq!(
            part_of(4).parent,
            Some(spine),
            "the hand hangs from the spine"
        );
        let sword = part_of(6);
        assert_eq!(sword.parent, None, "the sword comes loose");
        let RagdollShape::Box { half, .. } = sword.shape else {
            panic!("a held weapon is a box")
        };
        let longest = half.iter().copied().fold(0.0f32, f32::max);
        assert!((longest - 0.3).abs() < 0.02, "{half:?}");
        let RagdollShape::Capsule { a, b, radius } = part_of(3).shape else {
            panic!("a leg is a capsule")
        };
        assert!((a[1] - b[1]).abs() > 0.3, "the leg's capsule runs down it");
        assert!((radius - 0.05).abs() < 0.02, "{radius}");
        assert!(
            sword.mass < part_of(2).mass,
            "the sword is lighter than the torso"
        );
    }

    #[test]
    fn principal_axes_find_the_long_side() {
        let points: Vec<Vec3> = (0..50)
            .flat_map(|i| {
                let t = f64::from(i) / 49.0;
                [[t, t, 0.0], [t + 0.05, t, 0.0], [t, t, 0.05]]
            })
            .collect();
        let (_, axes) = principal_axes(&points);
        let s = std::f64::consts::FRAC_1_SQRT_2;
        assert!(dot(axes[0], [s, s, 0.0]).abs() > 0.99);
        assert!((dot(cross(axes[0], axes[1]), axes[2]) - 1.0).abs() < 1e-9);
    }
}
