//! Node animation: sampling glTF channels and posing the node hierarchy.

use crate::math::{Mat4, Quat, Vec3, lerp3, quat_normalize, slerp};
use gltf::animation::{Interpolation, util::ReadOutputs};

/// Local transform of a node.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Trs {
    pub translation: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
}

impl Trs {
    pub fn matrix(&self) -> Mat4 {
        Mat4::from_trs(self.translation, self.rotation, self.scale)
    }
}

/// A node's place in the hierarchy and its rest transform.
#[derive(Debug, Clone)]
pub struct Node {
    pub name: String,
    pub parent: Option<usize>,
    /// `None` when the node stores a matrix (cannot be animated).
    pub rest: Option<Trs>,
    pub rest_matrix: Mat4,
}

/// Every node of the document in index order, with parents.
pub fn nodes(document: &gltf::Document) -> Vec<Node> {
    let mut nodes: Vec<Node> = document
        .nodes()
        .map(|node| {
            let (rest, rest_matrix) = match node.transform() {
                gltf::scene::Transform::Matrix { matrix } => (None, Mat4::from_cols_f32(matrix)),
                gltf::scene::Transform::Decomposed {
                    translation,
                    rotation,
                    scale,
                } => {
                    let trs = Trs {
                        translation: translation.map(f64::from),
                        rotation: quat_normalize(rotation.map(f64::from)),
                        scale: scale.map(f64::from),
                    };
                    (Some(trs), trs.matrix())
                }
            };
            Node {
                name: node.name().unwrap_or_default().to_owned(),
                parent: None,
                rest,
                rest_matrix,
            }
        })
        .collect();
    for node in document.nodes() {
        for child in node.children() {
            nodes[child.index()].parent = Some(node.index());
        }
    }
    nodes
}

/// Global transforms from local ones (parents may come after children in the list).
pub fn globals(nodes: &[Node], locals: &[Mat4]) -> Vec<Mat4> {
    let mut globals: Vec<Option<Mat4>> = vec![None; nodes.len()];
    fn resolve(i: usize, nodes: &[Node], locals: &[Mat4], out: &mut [Option<Mat4>]) -> Mat4 {
        if let Some(m) = out[i] {
            return m;
        }
        let m = match nodes[i].parent {
            Some(p) => resolve(p, nodes, locals, out) * locals[i],
            None => locals[i],
        };
        out[i] = Some(m);
        m
    }
    for i in 0..nodes.len() {
        resolve(i, nodes, locals, &mut globals);
    }
    globals
        .into_iter()
        .map(|m| m.unwrap_or(Mat4::IDENTITY))
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Property {
    Translation,
    Rotation,
    Scale,
}

/// One sampled channel.
#[derive(Debug, Clone)]
pub struct Channel {
    pub node: usize,
    pub property: Property,
    pub interpolation: Interpolation,
    pub times: Vec<f64>,
    /// Keyframe values (xyz or xyzw); cubic splines store in-tangent, value, out-tangent.
    pub values: Vec<[f64; 4]>,
}

/// A clip ready to sample.
#[derive(Debug, Clone)]
pub struct Animation {
    pub name: String,
    pub duration: f64,
    pub channels: Vec<Channel>,
}

/// Reads every animation; returns notes about what was skipped.
pub fn animations(
    document: &gltf::Document,
    blob: &[u8],
    notes: &mut Vec<String>,
) -> Vec<Animation> {
    let mut out = Vec::new();
    for (index, animation) in document.animations().enumerate() {
        let name = animation
            .name()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("animation{index}"));
        let mut channels = Vec::new();
        for channel in animation.channels() {
            let reader = channel.reader(|_| Some(blob));
            let Some(times) = reader.read_inputs() else {
                notes.push(format!("clip {name}: channel without keyframe times"));
                continue;
            };
            let times: Vec<f64> = times.map(f64::from).collect();
            let (property, values): (Property, Vec<[f64; 4]>) = match reader.read_outputs() {
                Some(ReadOutputs::Translations(v)) => (
                    Property::Translation,
                    v.map(|t| [t[0], t[1], t[2], 0.0].map(f64::from)).collect(),
                ),
                Some(ReadOutputs::Scales(v)) => (
                    Property::Scale,
                    v.map(|s| [s[0], s[1], s[2], 0.0].map(f64::from)).collect(),
                ),
                Some(ReadOutputs::Rotations(r)) => (
                    Property::Rotation,
                    r.into_f32().map(|q| q.map(f64::from)).collect(),
                ),
                Some(ReadOutputs::MorphTargetWeights(_)) => {
                    notes.push(format!(
                        "clip {name}: morph target weights are not supported"
                    ));
                    continue;
                }
                None => {
                    notes.push(format!("clip {name}: channel without values"));
                    continue;
                }
            };
            let interpolation = channel.sampler().interpolation();
            let per_key = if interpolation == Interpolation::CubicSpline {
                3
            } else {
                1
            };
            if times.is_empty() || values.len() != times.len() * per_key {
                notes.push(format!("clip {name}: keyframe count mismatch"));
                continue;
            }
            channels.push(Channel {
                node: channel.target().node().index(),
                property,
                interpolation,
                times,
                values,
            });
        }
        let duration = channels
            .iter()
            .filter_map(|c| c.times.last().copied())
            .fold(0.0, f64::max);
        out.push(Animation {
            name,
            duration,
            channels,
        });
    }
    out
}

impl Channel {
    pub fn sample(&self, t: f64) -> [f64; 4] {
        let times = &self.times;
        let cubic = self.interpolation == Interpolation::CubicSpline;
        let value = |k: usize| {
            if cubic {
                self.values[k * 3 + 1]
            } else {
                self.values[k]
            }
        };
        let last = times.len() - 1;
        if t <= times[0] {
            return self.finish(value(0));
        }
        if t >= times[last] {
            return self.finish(value(last));
        }
        let k = times.partition_point(|&x| x <= t) - 1;
        let (t0, t1) = (times[k], times[k + 1]);
        let dt = t1 - t0;
        let u = if dt > 0.0 { (t - t0) / dt } else { 0.0 };
        let v = match self.interpolation {
            Interpolation::Step => value(k),
            Interpolation::Linear => {
                let (a, b) = (value(k), value(k + 1));
                if self.property == Property::Rotation {
                    slerp(a, b, u)
                } else {
                    let p = lerp3([a[0], a[1], a[2]], [b[0], b[1], b[2]], u);
                    [p[0], p[1], p[2], 0.0]
                }
            }
            Interpolation::CubicSpline => {
                let p0 = self.values[k * 3 + 1];
                let m0 = self.values[k * 3 + 2];
                let m1 = self.values[(k + 1) * 3];
                let p1 = self.values[(k + 1) * 3 + 1];
                let (u2, u3) = (u * u, u * u * u);
                let h00 = 2.0 * u3 - 3.0 * u2 + 1.0;
                let h10 = u3 - 2.0 * u2 + u;
                let h01 = -2.0 * u3 + 3.0 * u2;
                let h11 = u3 - u2;
                std::array::from_fn(|i| {
                    h00 * p0[i] + h10 * dt * m0[i] + h01 * p1[i] + h11 * dt * m1[i]
                })
            }
        };
        self.finish(v)
    }

    fn finish(&self, v: [f64; 4]) -> [f64; 4] {
        if self.property == Property::Rotation {
            quat_normalize(v)
        } else {
            v
        }
    }
}

impl Animation {
    /// Local matrices of every node at time `t`; unanimated nodes keep their rest pose.
    pub fn locals(&self, nodes: &[Node], t: f64) -> Vec<Mat4> {
        let mut trs: Vec<Option<Trs>> = nodes.iter().map(|n| n.rest).collect();
        for channel in &self.channels {
            let Some(local) = trs[channel.node].as_mut() else {
                continue;
            };
            let v = channel.sample(t);
            match channel.property {
                Property::Translation => local.translation = [v[0], v[1], v[2]],
                Property::Rotation => local.rotation = v,
                Property::Scale => local.scale = [v[0], v[1], v[2]],
            }
        }
        trs.iter()
            .zip(nodes)
            .map(|(t, n)| t.map_or(n.rest_matrix, |t| t.matrix()))
            .collect()
    }

    /// Nodes this clip moves.
    pub fn animated_nodes(&self) -> impl Iterator<Item = usize> + '_ {
        self.channels.iter().map(|c| c.node)
    }
}

/// Frames of a clip baked at `rate`: `max(2, ceil(duration · rate) + 1)`, frame `f` at
/// `min(duration, f / rate)` (the old game's bake).
pub fn frame_times(duration: f64, rate: f64) -> Vec<f64> {
    let count = ((duration * rate).ceil() as usize + 1).max(2);
    (0..count)
        .map(|f| (f as f64 / rate).min(duration))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn channel(interpolation: Interpolation, property: Property, values: Vec<[f64; 4]>) -> Channel {
        Channel {
            node: 0,
            property,
            interpolation,
            times: vec![0.0, 1.0],
            values,
        }
    }

    #[test]
    fn linear_step_and_clamping() {
        let c = channel(
            Interpolation::Linear,
            Property::Translation,
            vec![[0.0, 0.0, 0.0, 0.0], [2.0, 4.0, 6.0, 0.0]],
        );
        assert_eq!(c.sample(0.25), [0.5, 1.0, 1.5, 0.0]);
        assert_eq!(c.sample(-1.0), [0.0, 0.0, 0.0, 0.0]);
        assert_eq!(c.sample(9.0), [2.0, 4.0, 6.0, 0.0]);
        let s = Channel {
            interpolation: Interpolation::Step,
            ..c
        };
        assert_eq!(s.sample(0.99), [0.0, 0.0, 0.0, 0.0]);
    }

    #[test]
    fn cubic_splines_pass_through_keys() {
        let c = channel(
            Interpolation::CubicSpline,
            Property::Translation,
            vec![
                [0.0; 4],
                [1.0, 0.0, 0.0, 0.0],
                [3.0, 0.0, 0.0, 0.0],
                [3.0, 0.0, 0.0, 0.0],
                [5.0, 0.0, 0.0, 0.0],
                [0.0; 4],
            ],
        );
        assert_eq!(c.sample(0.0)[0], 1.0);
        assert_eq!(c.sample(1.0)[0], 5.0);
        assert!((c.sample(0.5)[0] - 3.0).abs() < 1e-12);
    }

    #[test]
    fn bake_frames_cover_the_clip() {
        assert_eq!(frame_times(0.0, 30.0), vec![0.0, 0.0]);
        let times = frame_times(1.0, 30.0);
        assert_eq!(times.len(), 31);
        assert_eq!(*times.last().unwrap(), 1.0);
        let times = frame_times(0.05, 30.0);
        assert_eq!(times, vec![0.0, 1.0 / 30.0, 0.05]);
    }
}
