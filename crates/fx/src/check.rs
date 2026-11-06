//! Checking a graph: every node known, every value and wire the right type, no cycles;
//! then the generic widths resolved and every node's stage inferred from what it depends
//! on (D113: no stage lanes on the canvas). The result is what the compiler and the
//! editor's previews work from.

use crate::catalogue::{self, Code, Fallback, NodeDef, Role, Ty, Var};
use crate::graph::{Curve, Graph, Value, Wire};
use std::collections::BTreeMap;

/// A problem in a graph, naming the node (and pin) it is about so the editor can mark it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub node: Option<String>,
    pub pin: Option<String>,
    pub message: String,
}

impl std::fmt::Display for Problem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match (&self.node, &self.pin) {
            (Some(node), Some(pin)) => write!(f, "{node}.{pin}: {}", self.message),
            (Some(node), None) => write!(f, "{node}: {}", self.message),
            _ => f.write_str(&self.message),
        }
    }
}

/// Where an input's value comes from.
#[derive(Debug, Clone, PartialEq)]
pub enum Source {
    /// Another node's output (node index, output index).
    Wire(usize, usize),
    /// Numbers given on the node or by the catalogue.
    Numbers(Vec<f32>),
    /// A particle attribute node's value (`lifeRatio` …), read where it is needed.
    Attribute(&'static str),
    Text(String),
    Curve(Curve),
}

#[derive(Debug, Clone)]
pub struct Checked {
    pub id: String,
    pub def: &'static NodeDef,
    /// One per catalogue input, in the catalogue's order.
    pub inputs: Vec<Source>,
    /// The width of the node's `Num` pins (1–4).
    pub width: u8,
    /// When the node is evaluated.
    pub var: Var,
}

impl Checked {
    pub fn input(&self, name: &str) -> &Source {
        let i = self
            .def
            .inputs
            .iter()
            .position(|p| p.name == name)
            .expect("catalogue input");
        &self.inputs[i]
    }

    /// The resolved type of output `i`.
    pub fn output_ty(&self, i: usize) -> Ty {
        match self.def.outputs[i].ty {
            Ty::Num => Ty::of_width(self.width),
            ty => ty,
        }
    }

    pub fn text(&self, name: &str) -> &str {
        match self.input(name) {
            Source::Text(text) => text,
            _ => "",
        }
    }

    pub fn numbers(&self, name: &str) -> &[f32] {
        match self.input(name) {
            Source::Numbers(n) => n,
            _ => &[],
        }
    }
}

/// A checked graph: its nodes in evaluation order (every node after the nodes it reads).
#[derive(Debug, Clone)]
pub struct Program {
    pub nodes: Vec<Checked>,
    /// Node indices in dependency order.
    pub order: Vec<usize>,
}

impl Program {
    pub fn of_role(&self, role: Role) -> impl Iterator<Item = &Checked> {
        self.order
            .iter()
            .map(|&i| &self.nodes[i])
            .filter(move |n| n.def.role == role)
    }

    pub fn index(&self, id: &str) -> Option<usize> {
        self.nodes.iter().position(|n| n.id == id)
    }
}

/// The catalogue entry of an attribute default (`lifeRatio`, `position` …).
pub fn attribute(kind: &str) -> &'static NodeDef {
    catalogue::find(kind).expect("catalogue attributes exist")
}

fn problem(node: &str, pin: Option<&str>, message: impl Into<String>) -> Problem {
    Problem {
        node: Some(node.to_string()),
        pin: pin.map(str::to_string),
        message: message.into(),
    }
}

/// `#rrggbb` (sRGB) to linear RGBA.
pub fn parse_colour(text: &str) -> Option<[f32; 4]> {
    let hex = text.strip_prefix('#')?;
    if hex.len() != 6 {
        return None;
    }
    let channel = |i: usize| -> Option<f32> {
        let c = u8::from_str_radix(&hex[i..i + 2], 16).ok()? as f32 / 255.0;
        Some(if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        })
    };
    Some([channel(0)?, channel(2)?, channel(4)?, 1.0])
}

/// A given value as an input of type `ty`.
fn value_source(value: &Value, ty: Ty) -> Result<Source, String> {
    match (ty, value) {
        (Ty::Curve, Value::Curve(curve)) => {
            let width = curve.width();
            if curve.keys.is_empty() || !(1..=4).contains(&width) {
                return Err("a curve needs keys of 1–4 numbers".into());
            }
            for (i, key) in curve.keys.iter().enumerate() {
                let tangents = [&key.tangent_in, &key.tangent_out];
                if key.v.len() != width
                    || tangents
                        .iter()
                        .any(|t| t.as_ref().is_some_and(|t| t.len() != width))
                {
                    return Err(format!("key {i} has a different number of components"));
                }
                if i > 0 && key.t < curve.keys[i - 1].t {
                    return Err(format!("key {i} comes before the key ahead of it"));
                }
            }
            Ok(Source::Curve(curve.clone()))
        }
        (Ty::Text, Value::Text(text)) => Ok(Source::Text(text.clone())),
        (Ty::F4, Value::Text(text)) => parse_colour(text)
            .map(|c| Source::Numbers(c.to_vec()))
            .ok_or_else(|| format!("`{text}` is no #rrggbb colour")),
        (Ty::Curve | Ty::Text, _) => Err("expects a curve or text, not a number".into()),
        (_, Value::Number(n)) => Ok(Source::Numbers(vec![*n as f32])),
        (_, Value::Vector(v)) => {
            let want = ty.width().unwrap_or(1) as usize;
            if (ty == Ty::Num && (1..=4).contains(&v.len())) || v.len() == want {
                Ok(Source::Numbers(v.iter().map(|&x| x as f32).collect()))
            } else {
                Err(format!("expects {want} numbers, has {}", v.len()))
            }
        }
        _ => Err("has the wrong kind of value".into()),
    }
}

/// Checks `graph` and orders, types and stages its nodes. All problems are returned at
/// once; a graph with problems compiles to nothing.
pub fn check(graph: &Graph) -> Result<Program, Vec<Problem>> {
    // A layer of authoring nodes (D115) checks as the primitives it expands to.
    if crate::flow::is_flow(graph) {
        crate::flow::authored(graph)?;
        return check(&crate::flow::expand(graph, None)?).map_err(|problems| {
            // A primitive's problem is its authoring node's (primitives are `owner~kind`).
            problems
                .into_iter()
                .map(
                    |p| match p.node.as_deref().and_then(|n| n.split_once('~')) {
                        Some((owner, _)) => Problem {
                            node: Some(owner.to_string()),
                            pin: None,
                            message: p.message,
                        },
                        None => p,
                    },
                )
                .collect()
        });
    }
    let mut problems = Vec::new();
    let mut index: BTreeMap<&str, usize> = BTreeMap::new();
    let mut nodes: Vec<Checked> = Vec::new();
    for node in &graph.nodes {
        let Some(def) = catalogue::find(&node.kind) else {
            problems.push(problem(
                &node.id,
                None,
                format!("no node kind `{}`", node.kind),
            ));
            continue;
        };
        if node.id.is_empty() || node.id.contains('.') {
            problems.push(problem(
                &node.id,
                None,
                "ids are non-empty and have no dots",
            ));
        }
        if index.insert(&node.id, nodes.len()).is_some() {
            problems.push(problem(&node.id, None, "two nodes share this id"));
            continue;
        }
        for name in node.inputs.keys() {
            if !def.inputs.iter().any(|p| p.name == name) {
                problems.push(problem(
                    &node.id,
                    Some(name),
                    format!("{} has no input `{name}`", def.title),
                ));
            }
        }
        let inputs = def
            .inputs
            .iter()
            .map(|pin| match node.inputs.get(pin.name) {
                Some(value) => value_source(value, pin.ty).unwrap_or_else(|message| {
                    problems.push(problem(&node.id, Some(pin.name), message));
                    Source::Numbers(vec![0.0])
                }),
                None => match pin.default {
                    Fallback::Numbers(n) => Source::Numbers(n.to_vec()),
                    Fallback::Attribute(kind) => Source::Attribute(kind),
                    Fallback::Text(text) => Source::Text(text.to_string()),
                    Fallback::Required => Source::Numbers(Vec::new()),
                },
            })
            .collect::<Vec<_>>();
        for (pin, source) in def.inputs.iter().zip(&inputs) {
            if let Source::Text(text) = source
                && !pin.choices.is_empty()
                && !pin.choices.contains(&text.as_str())
            {
                problems.push(problem(
                    &node.id,
                    Some(pin.name),
                    format!("`{text}` is not one of {}", pin.choices.join(", ")),
                ));
            }
        }
        nodes.push(Checked {
            id: node.id.clone(),
            def,
            inputs,
            width: 1,
            var: def.var,
        });
    }

    // Wires.
    let mut wired: BTreeMap<(usize, usize), &Wire> = BTreeMap::new();
    for wire in &graph.wires {
        let ends = Wire::split(&wire.from).zip(Wire::split(&wire.to));
        let Some(((from, out), (to, pin))) = ends else {
            problems.push(Problem {
                node: None,
                pin: None,
                message: format!(
                    "wire {} → {}: ends are written `node.pin`",
                    wire.from, wire.to
                ),
            });
            continue;
        };
        let (Some(&a), Some(&b)) = (index.get(from), index.get(to)) else {
            let missing = if index.contains_key(from) { to } else { from };
            problems.push(problem(
                missing,
                None,
                "a wire names this node, which does not exist",
            ));
            continue;
        };
        let Some(o) = nodes[a].def.outputs.iter().position(|p| p.name == out) else {
            problems.push(problem(from, Some(out), "no such output"));
            continue;
        };
        let Some(i) = nodes[b].def.inputs.iter().position(|p| p.name == pin) else {
            problems.push(problem(to, Some(pin), "no such input"));
            continue;
        };
        if !nodes[b].def.inputs[i].wired {
            problems.push(problem(to, Some(pin), "is a setting and takes no wire"));
            continue;
        }
        if wired.insert((b, i), wire).is_some() {
            problems.push(problem(to, Some(pin), "has two wires"));
            continue;
        }
        nodes[b].inputs[i] = Source::Wire(a, o);
    }
    for checked in &nodes {
        for (pin, source) in checked.def.inputs.iter().zip(&checked.inputs) {
            if pin.default == Fallback::Required
                && matches!(source, Source::Numbers(n) if n.is_empty())
            {
                problems.push(problem(
                    &checked.id,
                    Some(pin.name),
                    "needs a wire or a value",
                ));
            }
        }
    }

    // Dependency order (Kahn), which also finds cycles.
    let count = nodes.len();
    let mut readers: Vec<Vec<usize>> = vec![Vec::new(); count];
    let mut pending = vec![0usize; count];
    for (b, node) in nodes.iter().enumerate() {
        for source in &node.inputs {
            if let Source::Wire(a, _) = source {
                readers[*a].push(b);
                pending[b] += 1;
            }
        }
    }
    let mut order: Vec<usize> = (0..count).filter(|&i| pending[i] == 0).collect();
    let mut next = 0;
    while next < order.len() {
        for &b in &readers[order[next]] {
            pending[b] -= 1;
            if pending[b] == 0 {
                order.push(b);
            }
        }
        next += 1;
    }
    if order.len() < count {
        for (i, node) in nodes.iter().enumerate() {
            if pending[i] > 0 {
                problems.push(problem(&node.id, None, "is part of a loop of wires"));
            }
        }
        return Err(problems);
    }

    // Widths and types, in order.
    for &b in &order {
        let mut width = 1u8;
        for (i, pin) in nodes[b].def.inputs.iter().enumerate() {
            if pin.ty != Ty::Num {
                continue;
            }
            width = width.max(match &nodes[b].inputs[i] {
                Source::Wire(a, o) => nodes[*a].output_ty(*o).width().unwrap_or(1),
                Source::Numbers(n) => n.len().clamp(1, 4) as u8,
                Source::Attribute(kind) => attribute(kind).outputs[0].ty.width().unwrap_or(1),
                _ => 1,
            });
        }
        if let Some(Source::Curve(curve)) = nodes[b]
            .inputs
            .iter()
            .find(|s| matches!(s, Source::Curve(_)))
        {
            width = curve.width() as u8;
        }
        nodes[b].width = width;
        for (i, pin) in nodes[b].def.inputs.iter().enumerate() {
            let Source::Wire(a, o) = nodes[b].inputs[i] else {
                continue;
            };
            let have = nodes[a].output_ty(o);
            let want = if pin.ty == Ty::Num {
                Ty::of_width(width)
            } else {
                pin.ty
            };
            let fits = have == want
                || have.width() == Some(1)
                || (want == Ty::Bool && have.width() == Some(1));
            if !fits {
                problems.push(problem(
                    &nodes[b].id,
                    Some(pin.name),
                    format!(
                        "takes {}, but {}.{} gives {}",
                        want.wgsl(),
                        nodes[a].id,
                        nodes[a].def.outputs[o].name,
                        have.wgsl()
                    ),
                ));
            }
        }
    }

    // Stages: a node is evaluated at the latest of its own and its inputs' variability.
    for &b in &order {
        let mut var = nodes[b].def.var;
        for source in &nodes[b].inputs {
            var = var.max(match source {
                Source::Wire(a, _) => nodes[*a].var,
                Source::Attribute(kind) => attribute(kind).var,
                _ => Var::Const,
            });
        }
        if graph
            .nodes
            .iter()
            .any(|n| n.id == nodes[b].id && n.at_birth)
            && var > Var::Birth
        {
            var = Var::Birth;
        }
        if nodes[b].def.kind == "setLife" && var > Var::Birth {
            problems.push(problem(&nodes[b].id, Some("life"), "a particle's life is set at birth: wire something fixed at birth, or mark the node “at birth”"));
        }
        nodes[b].var = var;
    }

    for node in &nodes {
        if node.def.role == Role::Renderer && node.text("texture").is_empty() {
            problems.push(problem(&node.id, Some("texture"), "needs a texture"));
        }
        if let Code::Builtin = node.def.code
            && node.def.kind == "parameter"
            && node.text("name").is_empty()
        {
            problems.push(problem(&node.id, Some("name"), "names no parameter"));
        }
    }
    if problems.is_empty() {
        Ok(Program { nodes, order })
    } else {
        Err(problems)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn graph(json: &str) -> Graph {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn stages_follow_what_nodes_depend_on() {
        let program = check(&graph(
            r##"{
              "nodes": [
                {"id": "spawn", "kind": "rate", "inputs": {"rate": 20}},
                {"id": "life", "kind": "random", "inputs": {"min": 0.4, "max": 0.6}},
                {"id": "setLife", "kind": "setLife"},
                {"id": "grow", "kind": "curve", "inputs": {"curve": {"keys": [{"t": 0, "v": [0.4]}, {"t": 1, "v": [1]}]}}},
                {"id": "size", "kind": "random", "inputs": {"min": 0.1, "max": 0.4}},
                {"id": "scaled", "kind": "multiply"},
                {"id": "tint", "kind": "colour", "inputs": {"value": "#ffd36b"}},
                {"id": "draw", "kind": "billboard", "inputs": {"texture": "asset.effects.flare"}}
              ],
              "wires": [
                {"from": "life.value", "to": "setLife.life"},
                {"from": "grow.value", "to": "scaled.a"},
                {"from": "size.value", "to": "scaled.b"},
                {"from": "scaled.value", "to": "draw.size"},
                {"from": "tint.value", "to": "draw.colour"}
              ]
            }"##,
        ))
        .unwrap();
        let var = |id: &str| program.nodes[program.index(id).unwrap()].var;
        assert_eq!(var("tint"), Var::Const);
        assert_eq!(var("life"), Var::Birth);
        assert_eq!(var("setLife"), Var::Birth);
        // The curve reads the life ratio by default, so it and its product change each step.
        assert_eq!(var("grow"), Var::Step);
        assert_eq!(var("scaled"), Var::Step);
        // Everything comes after what it reads.
        let at = |id: &str| {
            program
                .order
                .iter()
                .position(|&i| program.nodes[i].id == id)
                .unwrap()
        };
        assert!(
            at("grow") < at("scaled") && at("size") < at("scaled") && at("scaled") < at("draw")
        );
    }

    #[test]
    fn widths_broadcast_numbers_into_vectors() {
        let program = check(&graph(
            r##"{
              "nodes": [
                {"id": "dir", "kind": "vector", "inputs": {"value": [0, 1, 0]}},
                {"id": "speed", "kind": "random", "inputs": {"min": 3, "max": 7}},
                {"id": "v", "kind": "multiply"},
                {"id": "set", "kind": "setVelocity"}
              ],
              "wires": [
                {"from": "dir.value", "to": "v.a"},
                {"from": "speed.value", "to": "v.b"},
                {"from": "v.value", "to": "set.velocity"}
              ]
            }"##,
        ))
        .unwrap();
        let v = &program.nodes[program.index("v").unwrap()];
        assert_eq!(v.width, 3);
        assert_eq!(v.output_ty(0), Ty::F3);
    }

    #[test]
    fn problems_name_their_node_and_pin() {
        let problems = check(&graph(
            r##"{
              "nodes": [
                {"id": "a", "kind": "add"},
                {"id": "b", "kind": "add"},
                {"id": "c", "kind": "nonsense"},
                {"id": "d", "kind": "billboard", "inputs": {"mode": "sideways", "texture": "x"}},
                {"id": "e", "kind": "random", "inputs": {"min": [1, 2, 3, 4, 5]}}
              ],
              "wires": [
                {"from": "a.value", "to": "b.a"},
                {"from": "b.value", "to": "a.a"}
              ]
            }"##,
        ))
        .unwrap_err();
        let text: Vec<String> = problems.iter().map(ToString::to_string).collect();
        assert!(
            text.iter().any(|t| t.starts_with("c: no node kind")),
            "{text:?}"
        );
        assert!(text.iter().any(|t| t.starts_with("d.mode:")), "{text:?}");
        assert!(text.iter().any(|t| t.starts_with("e.min:")), "{text:?}");
        assert!(text.iter().any(|t| t.contains("loop")), "{text:?}");
    }

    #[test]
    fn colours_are_read_as_linear() {
        let [r, g, b, a] = parse_colour("#ff8000").unwrap();
        assert_eq!((r, b, a), (1.0, 0.0, 1.0));
        assert!((g - 0.2158605).abs() < 1e-4);
        assert!(parse_colour("ff8000").is_none());
    }
}
