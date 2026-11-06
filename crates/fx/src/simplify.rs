//! Simplifies a layer graph without changing what it draws: the clean-up a compiler runs,
//! done on the graph itself so people read the logic, not the bookkeeping (imported
//! Reforged layers are their scripts transcribed statement by statement).
//!
//! - **Birth values wired through:** an attribute set once at birth and only read is its
//!   value (the compiler keeps birth values a particle reads later by itself).
//! - **Constants folded:** maths on constants becomes the number.
//! - **Do-nothing steps dropped:** multiplying by one, adding or subtracting zero.
//! - **Duplicates merged:** the same calculation built twice (never two random rolls).
//! - **Dead nodes removed:** anything no writer, spawner or renderer depends on, and
//!   attributes nothing reads.
//!
//! The result is checked: when the simplified graph does not check, the original comes
//! back unchanged.

use crate::{
    Graph, Node, Value, Wire,
    catalogue::{Role, Var},
    check::{Program, Source, check},
};
use std::collections::{BTreeMap, BTreeSet};

/// `graph` simplified (or unchanged when nothing applies or the result would not check).
pub fn simplify(graph: &Graph) -> Graph {
    if crate::flow::is_flow(graph) {
        // Authoring nodes are already the plain form; the primitives they expand to are
        // never stored.
        return graph.clone();
    }
    let mut g = graph.clone();
    for _ in 0..16 {
        let Ok(program) = check(&g) else {
            return graph.clone();
        };
        let mut changed = fold_birth_attributes(&mut g, &program);
        changed |= drop_identities(&mut g, &program);
        changed |= fold_constants(&mut g, &program);
        changed |= merge_duplicates(&mut g, &program);
        changed |= remove_dead(&mut g, &program);
        if !changed {
            break;
        }
    }
    if check(&g).is_ok() { g } else { graph.clone() }
}

/// Where input `node.pin` is fed from: a wire's source end, if any.
fn feeder<'a>(g: &'a Graph, node: &str, pin: &str) -> Option<&'a str> {
    let to = format!("{node}.{pin}");
    g.wires.iter().find(|w| w.to == to).map(|w| w.from.as_str())
}

/// The literal on `node.pin` (set on the node, else the catalogue's default).
fn literal(p: &Program, node: usize, pin: &str) -> Option<Vec<f32>> {
    let c = &p.nodes[node];
    let i = c.def.inputs.iter().position(|x| x.name == pin)?;
    match &c.inputs[i] {
        Source::Numbers(n) => Some(n.clone()),
        _ => None,
    }
}

fn value_of(numbers: &[f32]) -> Value {
    if numbers.len() == 1 {
        Value::Number(f64::from(numbers[0]))
    } else {
        Value::Vector(numbers.iter().map(|&x| f64::from(x)).collect())
    }
}

/// What feeds consumers of `from` instead: another output, or a literal.
#[derive(Clone)]
enum Replacement {
    Output(String),
    Literal(Vec<f32>),
}

/// Rewires every consumer of output `from` to `with`.
fn replace(g: &mut Graph, from: &str, with: &Replacement) -> bool {
    let mut literals: Vec<(String, String)> = Vec::new();
    let mut changed = false;
    for w in &mut g.wires {
        if w.from != from {
            continue;
        }
        changed = true;
        match with {
            Replacement::Output(o) => w.from = o.clone(),
            Replacement::Literal(_) => literals.push(
                Wire::split(&w.to)
                    .map(|(n, p)| (n.to_owned(), p.to_owned()))
                    .expect("wire end"),
            ),
        }
    }
    if let Replacement::Literal(numbers) = with {
        g.wires.retain(|w| w.from != from);
        for (node, pin) in literals {
            if let Some(n) = g.nodes.iter_mut().find(|n| n.id == node) {
                n.inputs.insert(pin, value_of(numbers));
            }
        }
    }
    changed
}

/// The width of output `pin` of node `id` (1 for anything that is not numbers).
fn width_of(p: &Program, end: &str) -> u8 {
    let Some((id, pin)) = Wire::split(end) else {
        return 1;
    };
    let Some(i) = p.index(id) else {
        return 1;
    };
    let c = &p.nodes[i];
    c.def
        .outputs
        .iter()
        .position(|o| o.name == pin)
        .and_then(|o| c.output_ty(o).width())
        .unwrap_or(1)
}

fn fold_birth_attributes(g: &mut Graph, p: &Program) -> bool {
    let mut sets: BTreeMap<String, Vec<usize>> = BTreeMap::new();
    for (i, n) in g.nodes.iter().enumerate() {
        if n.kind == "setAttribute"
            && let Some(Value::Text(name)) = n.inputs.get("name")
        {
            sets.entry(name.clone()).or_default().push(i);
        }
    }
    let mut changed = false;
    for (name, writers) in sets {
        let [w] = writers[..] else { continue };
        let writer = g.nodes[w].clone();
        let Some(pi) = p.index(&writer.id) else {
            continue;
        };
        // Set once, from what is fixed at birth: the attribute is that value.
        if p.nodes[pi].var > Var::Birth && !writer.at_birth {
            continue;
        }
        let source = match feeder(g, &writer.id, "value") {
            Some(from) => {
                let Some((id, _)) = Wire::split(from) else {
                    continue;
                };
                let Some(si) = p.index(id) else { continue };
                if p.nodes[si].var > Var::Birth {
                    continue;
                }
                Replacement::Output(from.to_owned())
            }
            None => match literal(p, pi, "value") {
                Some(n) => Replacement::Literal(n),
                None => continue,
            },
        };
        let width = match &source {
            Replacement::Output(o) => width_of(p, o),
            Replacement::Literal(n) => n.len() as u8,
        };
        let readers: Vec<String> = g
            .nodes
            .iter()
            .filter(|n| {
                n.kind == "attribute"
                    && matches!(n.inputs.get("name"), Some(Value::Text(t)) if *t == name)
            })
            .map(|n| n.id.clone())
            .collect();
        for r in readers {
            for pin in ["value", "xyz", "x"] {
                let end = format!("{r}.{pin}");
                let with = match (pin, &source) {
                    ("value", s) => s.clone(),
                    ("xyz", Replacement::Literal(n)) => {
                        Replacement::Literal(n.iter().copied().take(3).collect())
                    }
                    ("xyz", s) if width <= 3 => s.clone(),
                    ("x", Replacement::Literal(n)) => {
                        Replacement::Literal(vec![n.first().copied().unwrap_or(0.0)])
                    }
                    ("x", s) if width == 1 => s.clone(),
                    _ => continue,
                };
                changed |= replace(g, &end, &with);
            }
        }
    }
    changed
}

fn is_all(numbers: &[f32], value: f32) -> bool {
    !numbers.is_empty() && numbers.iter().all(|&x| x == value)
}

fn drop_identities(g: &mut Graph, p: &Program) -> bool {
    let mut changed = false;
    for (i, c) in p.nodes.iter().enumerate() {
        // (the constant side, the side kept, the constant that does nothing)
        let cases: &[(&str, &str, f32)] = match c.def.kind {
            "multiply" => &[("b", "a", 1.0), ("a", "b", 1.0)],
            "add" => &[("b", "a", 0.0), ("a", "b", 0.0)],
            "subtract" => &[("b", "a", 0.0)],
            _ => continue,
        };
        for &(constant, kept, unit) in cases {
            if feeder(g, &c.id, constant).is_some()
                || !literal(p, i, constant).is_some_and(|n| is_all(&n, unit))
            {
                continue;
            }
            let Some(source) = feeder(g, &c.id, kept).map(str::to_owned) else {
                continue;
            };
            // When the kept side is narrower (a number widened to a vector), every reader
            // must take a number where it wants the vector: a typed input converts it,
            // and WGSL adds, subtracts and multiplies a number with a vector.
            if width_of(p, &source) != c.width && !broadcasts(g, p, &c.id) {
                continue;
            }
            changed |= replace(g, &format!("{}.value", c.id), &Replacement::Output(source));
            break;
        }
    }
    changed
}

/// Whether every reader of `node`'s value takes a single number in its place.
fn broadcasts(g: &Graph, p: &Program, node: &str) -> bool {
    let from = format!("{node}.value");
    g.wires.iter().filter(|w| w.from == from).all(|w| {
        let Some((id, pin)) = Wire::split(&w.to) else {
            return false;
        };
        let Some(i) = p.index(id) else {
            return false;
        };
        let def = p.nodes[i].def;
        let Some(input) = def.inputs.iter().find(|x| x.name == pin) else {
            return false;
        };
        input.ty != crate::catalogue::Ty::Num || matches!(def.kind, "add" | "subtract" | "multiply")
    })
}

fn fold_constants(g: &mut Graph, p: &Program) -> bool {
    let mut changed = false;
    for (i, c) in p.nodes.iter().enumerate() {
        let op: fn(f32, f32) -> f32 = match c.def.kind {
            "add" => |a, b| a + b,
            "subtract" => |a, b| a - b,
            "multiply" => |a, b| a * b,
            "divide" => |a, b| if b == 0.0 { 0.0 } else { a / b },
            "min" => f32::min,
            "max" => f32::max,
            _ => continue,
        };
        let (Some(a), Some(b)) = (literal(p, i, "a"), literal(p, i, "b")) else {
            continue;
        };
        if feeder(g, &c.id, "a").is_some() || feeder(g, &c.id, "b").is_some() {
            continue;
        }
        let w = usize::from(c.width.max(1));
        let at = |v: &[f32], k: usize| {
            if v.len() == 1 {
                v[0]
            } else {
                v.get(k).copied().unwrap_or(0.0)
            }
        };
        let out: Vec<f32> = (0..w).map(|k| op(at(&a, k), at(&b, k))).collect();
        changed |= replace(g, &format!("{}.value", c.id), &Replacement::Literal(out));
    }
    changed
}

fn merge_duplicates(g: &mut Graph, p: &Program) -> bool {
    let mut seen: BTreeMap<String, String> = BTreeMap::new();
    let mut changed = false;
    for c in &p.nodes {
        // Two random rolls (or spawn shapes) are different values.
        if c.def.role != Role::Value || c.def.var == Var::Birth || c.def.kind == "attribute" {
            continue;
        }
        let Some(node) = g.nodes.iter().find(|n| n.id == c.id) else {
            continue;
        };
        let mut feeds: Vec<String> = g
            .wires
            .iter()
            .filter(|w| Wire::split(&w.to).is_some_and(|(n, _)| n == c.id))
            .map(|w| format!("{}<{}", Wire::split(&w.to).expect("end").1, w.from))
            .collect();
        feeds.sort();
        let key = format!(
            "{}|{}|{}|{}",
            node.kind,
            serde_json::to_string(&node.inputs).unwrap_or_default(),
            node.at_birth,
            feeds.join(",")
        );
        match seen.get(&key) {
            Some(first) => {
                for o in c.def.outputs {
                    changed |= replace(
                        g,
                        &format!("{}.{}", c.id, o.name),
                        &Replacement::Output(format!("{first}.{}", o.name)),
                    );
                }
            }
            None => {
                seen.insert(key, c.id.clone());
            }
        }
    }
    changed
}

fn remove_dead(g: &mut Graph, p: &Program) -> bool {
    // Attributes nothing reads are not worth writing.
    let read: BTreeSet<String> = g
        .nodes
        .iter()
        .filter(|n| n.kind == "attribute")
        .filter_map(|n| match n.inputs.get("name") {
            Some(Value::Text(t)) => Some(t.clone()),
            _ => None,
        })
        .collect();
    let unread = |n: &Node| {
        n.kind == "setAttribute"
            && matches!(n.inputs.get("name"), Some(Value::Text(t)) if !read.contains(t))
    };
    let mut live: BTreeSet<String> = g
        .nodes
        .iter()
        .filter(|n| {
            p.index(&n.id)
                .is_some_and(|i| p.nodes[i].def.role != Role::Value)
                && !unread(n)
        })
        .map(|n| n.id.clone())
        .collect();
    loop {
        let more: Vec<String> = g
            .wires
            .iter()
            .filter(|w| Wire::split(&w.to).is_some_and(|(n, _)| live.contains(n)))
            .filter_map(|w| Wire::split(&w.from).map(|(n, _)| n.to_owned()))
            .filter(|n| !live.contains(n))
            .collect();
        if more.is_empty() {
            break;
        }
        live.extend(more);
    }
    let before = g.nodes.len();
    g.nodes.retain(|n| live.contains(&n.id));
    g.wires.retain(|w| {
        Wire::split(&w.from).is_some_and(|(n, _)| live.contains(n))
            && Wire::split(&w.to).is_some_and(|(n, _)| live.contains(n))
    });
    g.nodes.len() != before
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A spark whose size is a random roll stored at birth and read back each step, times
    /// one, plus a constant sum and a calculation nothing draws.
    const BOOKKEEPING: &str = r##"{
      "nodes": [
        {"id": "spawn", "kind": "burst", "inputs": {"count": 4}},
        {"id": "life", "kind": "setLife", "inputs": {"life": 1}},
        {"id": "roll", "kind": "random", "inputs": {"min": 0.5, "max": 1.5}},
        {"id": "store", "kind": "setAttribute", "inputs": {"name": "size0"}},
        {"id": "read", "kind": "attribute", "inputs": {"name": "size0"}},
        {"id": "one", "kind": "multiply", "inputs": {"b": 1}},
        {"id": "sum", "kind": "add", "inputs": {"a": 2, "b": 3}},
        {"id": "times", "kind": "multiply"},
        {"id": "unused", "kind": "multiply", "inputs": {"b": 4}},
        {"id": "draw", "kind": "billboard", "inputs": {"texture": "asset.test.dot"}}
      ],
      "wires": [
        {"from": "roll.value", "to": "store.value"},
        {"from": "read.x", "to": "one.a"},
        {"from": "one.value", "to": "times.a"},
        {"from": "sum.value", "to": "times.b"},
        {"from": "roll.value", "to": "unused.a"},
        {"from": "times.value", "to": "draw.size"}
      ]
    }"##;

    #[test]
    fn bookkeeping_folds_away_and_the_layer_still_draws_the_same_value() {
        let graph: Graph = serde_json::from_str(BOOKKEEPING).unwrap();
        let simple = simplify(&graph);
        let kinds: Vec<&str> = simple.nodes.iter().map(|n| n.kind.as_str()).collect();
        assert_eq!(
            kinds,
            ["burst", "setLife", "random", "multiply", "billboard"]
        );
        // The size is the roll times the folded constant 5.
        let times = simple.nodes.iter().find(|n| n.id == "times").unwrap();
        assert_eq!(times.inputs.get("b"), Some(&Value::Number(5.0)));
        assert!(
            simple
                .wires
                .iter()
                .any(|w| w.from == "roll.value" && w.to == "times.a")
        );
        check(&simple).expect("the simplified layer checks");
    }
}
