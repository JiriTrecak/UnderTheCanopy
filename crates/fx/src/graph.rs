//! The authored form of an effect layer's graph: nodes placed on a canvas, their input
//! values, and the wires between their pins. This is what `base/effects/*.json` holds
//! and what the node editor edits; [`crate::check`] turns it into a typed, staged graph.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One layer's graph: a particle system whose spawners, values, writers and renderers are
/// nodes, connected by wires.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Graph {
    pub nodes: Vec<Node>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wires: Vec<Wire>,
    /// Named frames grouping nodes on the canvas; the game ignores them.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backdrops: Vec<Backdrop>,
}

/// A node: an instance of a catalogue entry ([`crate::catalogue`]) with its unwired
/// inputs' values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Node {
    /// Unique within the graph; wires name pins as `<node id>.<pin>`.
    pub id: String,
    /// The catalogue entry, e.g. `random`, `curve`, `billboard`.
    pub kind: String,
    /// A label shown instead of the catalogue's title.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Where the node sits on the canvas (canvas units; the game ignores it).
    #[serde(default)]
    pub at: [f32; 2],
    /// Values of inputs that no wire feeds; a missing input takes the catalogue default.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub inputs: BTreeMap<String, Value>,
    /// Evaluate once when each particle is born and keep the result, even when the
    /// inputs change over the particle's life (sample a curve once, not every step).
    #[serde(default, skip_serializing_if = "is_false")]
    pub at_birth: bool,
    /// The editor shows the node without its preview.
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
}

fn is_false(value: &bool) -> bool {
    !*value
}

/// A wire from an output pin to an input pin, each written `<node id>.<pin>`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Wire {
    pub from: String,
    pub to: String,
}

impl Wire {
    /// The node id and pin name of one end.
    pub fn split(end: &str) -> Option<(&str, &str)> {
        end.rsplit_once('.')
    }
}

/// A named, coloured frame on the canvas.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Backdrop {
    pub name: String,
    /// `#rrggbb`.
    pub colour: String,
    /// Left, top, width, height in canvas units.
    pub rect: [f32; 4],
}

/// An input's value: a number, a vector or colour as 2–4 numbers, text (an enum choice,
/// an asset id, `#rrggbb`), or a curve.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum Value {
    Number(f64),
    Vector(Vec<f64>),
    Text(String),
    Curve(Curve),
}

/// A curve of 1–4 components over 0–1 (usually a particle's life), Hermite between keys
/// with each key's incoming and outgoing tangents (PopcornFX's curves, so imported
/// effects keep their shape). Missing tangents make the curve linear between keys.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Curve {
    pub keys: Vec<CurveKey>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CurveKey {
    /// Position, 0–1, increasing.
    pub t: f64,
    /// The value, as many components as the curve has.
    pub v: Vec<f64>,
    /// Tangents scaled by the neighbouring segment's length (Hermite); absent: linear.
    #[serde(default, rename = "in", skip_serializing_if = "Option::is_none")]
    pub tangent_in: Option<Vec<f64>>,
    #[serde(default, rename = "out", skip_serializing_if = "Option::is_none")]
    pub tangent_out: Option<Vec<f64>>,
}

impl Curve {
    /// Components per key (all keys agree after checking).
    pub fn width(&self) -> usize {
        self.keys.first().map_or(1, |k| k.v.len())
    }

    /// The value at `t` (clamped to the keys' range): Hermite between the two keys
    /// around it, linear where a key has no tangents.
    pub fn sample(&self, t: f64) -> Vec<f64> {
        let width = self.width();
        let Some(first) = self.keys.first() else {
            return vec![0.0; width];
        };
        if t <= first.t || self.keys.len() == 1 {
            return first.v.clone();
        }
        let last = &self.keys[self.keys.len() - 1];
        if t >= last.t {
            return last.v.clone();
        }
        let i = self.keys.windows(2).position(|w| t <= w[1].t).unwrap_or(0);
        let (a, b) = (&self.keys[i], &self.keys[i + 1]);
        let span = (b.t - a.t).max(1e-9);
        let u = (t - a.t) / span;
        let (h00, h10, h01, h11) = (
            2.0 * u * u * u - 3.0 * u * u + 1.0,
            u * u * u - 2.0 * u * u + u,
            -2.0 * u * u * u + 3.0 * u * u,
            u * u * u - u * u,
        );
        (0..width)
            .map(|c| {
                let (p0, p1) = (a.v[c], b.v[c]);
                match (&a.tangent_out, &b.tangent_in) {
                    (Some(m0), Some(m1)) => h00 * p0 + h10 * m0[c] + h01 * p1 + h11 * m1[c],
                    _ => p0 + (p1 - p0) * u,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(t: f64, v: f64, tangent: Option<f64>) -> CurveKey {
        CurveKey {
            t,
            v: vec![v],
            tangent_in: tangent.map(|m| vec![m]),
            tangent_out: tangent.map(|m| vec![m]),
        }
    }

    #[test]
    fn hermite_curves_pass_through_their_keys_and_follow_their_tangents() {
        // A straight line written as Hermite (tangent = rise over the segment) stays one.
        let line = Curve {
            keys: vec![key(0.0, 0.0, Some(1.0)), key(1.0, 1.0, Some(1.0))],
        };
        for t in [0.0, 0.25, 0.5, 0.9, 1.0] {
            assert!((line.sample(t)[0] - t).abs() < 1e-9);
        }
        // Flat tangents ease in and out: slower than linear near the ends.
        let eased = Curve {
            keys: vec![key(0.0, 0.0, Some(0.0)), key(1.0, 1.0, Some(0.0))],
        };
        assert!(eased.sample(0.1)[0] < 0.1);
        assert!((eased.sample(0.5)[0] - 0.5).abs() < 1e-9);
        // Outside the keys the ends hold.
        assert_eq!(eased.sample(-1.0)[0], 0.0);
        assert_eq!(eased.sample(2.0)[0], 1.0);
    }

    #[test]
    fn graphs_read_the_authored_json() {
        let graph: Graph = serde_json::from_str(
            r##"{
              "nodes": [
                {"id": "life", "kind": "random", "at": [0, 0], "inputs": {"min": 0.4, "max": 0.6}},
                {"id": "tint", "kind": "colour", "inputs": {"value": "#ffd36b"}},
                {"id": "grow", "kind": "curve", "inputs": {"curve": {"keys": [{"t": 0, "v": [0.4]}, {"t": 1, "v": [1]}]}}},
                {"id": "up", "kind": "vector", "inputs": {"value": [0, 1, 0]}}
              ],
              "wires": [{"from": "life.value", "to": "setLife.life"}]
            }"##,
        )
        .unwrap();
        assert_eq!(graph.nodes.len(), 4);
        assert_eq!(graph.nodes[0].inputs["min"], Value::Number(0.4));
        assert!(matches!(graph.nodes[1].inputs["value"], Value::Text(_)));
        assert!(matches!(graph.nodes[2].inputs["curve"], Value::Curve(_)));
        assert!(matches!(graph.nodes[3].inputs["value"], Value::Vector(_)));
        assert_eq!(Wire::split(&graph.wires[0].to), Some(("setLife", "life")));
    }
}
