//! The effect's graph (D113, D115; docs/effect-graph.md "Authoring nodes"): the whole effect
//! on one screen, read left to right. Every layer is a row: its chain of authoring nodes from
//! the Spawner to the renderer, box, wire, box, in flow order; every renderer wires into the
//! one Effect node on the right, which plays the whole effect. A layer born of another's
//! events hangs under the Emit that fires it. Nodes that change the look show the layer as
//! the chain makes it up to there; value nodes sit under the node they drive. Nodes are
//! inserted with the `+` on a wire (or a right-click on a row), moved and removed from their
//! context menu, and edited in the inspector beside the canvas, where a setting can also be
//! driven by a value node. Every change is an editor command on the document, so it undoes
//! and the previews replay it at once.

use crate::document::Command;
use crate::forms::Change;
use crate::theme::*;
use canopy_fx::flow::{self, Default as Fallback, Flow, FlowDef, Kind, Setting};
use canopy_fx::graph::{Curve, CurveKey, Graph, Node, Value as FxValue, Wire};
use egui::epaint::CubicBezierShape;
use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use serde_json::{Value, json};

const NODE_WIDTH: f32 = 150.0;
const VALUE_WIDTH: f32 = 150.0;
const TITLE: f32 = 20.0;
const ROW: f32 = 16.0;
const CURVE_ROW: f32 = 26.0;
/// Between the boxes of a row (the wire and its `+`).
const GAP: f32 = 40.0;
/// The row's name above its boxes.
const LANE_HEAD: f32 = 22.0;
const ROW_GAP: f32 = 26.0;
/// The live preview on a node (the atlas cells' aspect).
const PREVIEW: [f32; 2] = [138.0, 86.0];
/// The Effect node's width, at least and at most: it grows with the effect.
const EFFECT_WIDTH: [f32; 2] = [320.0, 760.0];
/// The preview's aspect (the atlas cells').
const EFFECT_ASPECT: f32 = 184.0 / 115.0;
/// How far right of the rows the Effect node stands (the wires merging into it).
const EFFECT_GAP: f32 = 160.0;

/// Nodes whose preview would show nothing new: they decide when and how long particles
/// live, not how they look.
fn previewed(kind: &str) -> bool {
    !matches!(kind, "spawner" | "emit")
}

/// Where the palette opened (on screen), the lane, the node to insert after (`None`: the
/// lane's end), and its search.
type Palette = (Pos2, usize, Option<String>, String);

/// What the inspector shows: a node, or a lane's own settings.
#[derive(Debug, Clone, PartialEq)]
enum Selection {
    Node(usize, String),
    Lane(usize),
}

/// The canvas's view and interaction state, per document.
pub struct Canvas {
    pan: Vec2,
    zoom: f32,
    selected: Option<Selection>,
    palette: Option<Palette>,
    /// What the view was last fitted to; it follows the content (previews arriving make
    /// cards taller) until the user pans or zooms.
    fitted: Option<Rect>,
    moved: bool,
}

impl Default for Canvas {
    fn default() -> Self {
        Self {
            pan: Vec2::ZERO,
            zoom: 1.0,
            selected: None,
            palette: None,
            fitted: None,
            moved: false,
        }
    }
}

/// What one frame of the canvas asks the shell for.
#[derive(Default)]
pub struct Out {
    pub changes: Vec<Change>,
    /// The previews' camera moves in (below 1) or out: a scroll over a node's preview.
    pub preview_zoom: Option<f32>,
}

const fn hex_rgb(rgb: u32) -> Color32 {
    Color32::from_rgb((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

/// A node group's header colour.
fn group_colour(def: &FlowDef) -> Color32 {
    use flow::Group::*;
    match def.group {
        Spawn => hex_rgb(0x4c1d95),
        Birth => hex_rgb(0x14532d),
        Motion => hex_rgb(0x164e63),
        Appearance => hex_rgb(0x7c2d12),
        Events => hex_rgb(0x701a75),
        Draw => hex_rgb(0x854d0e),
        Values => hex_rgb(0x3f3f46),
    }
}

fn short(text: &str) -> &str {
    text.rsplit('.').next().unwrap_or(text)
}

/// Linear to sRGB bytes (curve colours are linear).
fn srgb(c: f64) -> u8 {
    let c = c.clamp(0.0, 1.0);
    let s = if c <= 0.0031308 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    };
    (s * 255.0).round() as u8
}

fn number(x: f64) -> String {
    let t = format!("{x:.3}");
    t.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A setting's keys as set on the node, or its default.
fn keys_of(setting: &Setting, node: &Node) -> Curve {
    if let Some(FxValue::Curve(c)) = node.inputs.get(setting.name) {
        return c.clone();
    }
    let keys = match setting.default {
        Fallback::Keys(k) => k,
        _ => &[],
    };
    Curve {
        keys: keys
            .iter()
            .map(|(t, v)| CurveKey {
                t: *t,
                v: v.to_vec(),
                tangent_in: None,
                tangent_out: None,
            })
            .collect(),
    }
}

/// A setting's numbers as set on the node, or its default.
fn numbers_of(setting: &Setting, node: &Node) -> Vec<f64> {
    match node.inputs.get(setting.name) {
        Some(FxValue::Number(n)) => vec![*n],
        Some(FxValue::Vector(v)) => v.clone(),
        _ => match setting.default {
            Fallback::Number(n) => vec![n],
            Fallback::Numbers(n) => n.to_vec(),
            _ => vec![0.0],
        },
    }
}

fn text_of(setting: &Setting, node: &Node) -> String {
    match node.inputs.get(setting.name) {
        Some(FxValue::Text(t)) => t.clone(),
        _ => match setting.default {
            Fallback::Text(t) => t.to_string(),
            _ => String::new(),
        },
    }
}

/// The settings a node card shows: what is set or wired, and what its mode needs.
fn shown(def: &FlowDef, node: &Node, graph: &Graph) -> Vec<&'static Setting> {
    let mode = def
        .settings
        .iter()
        .find(|s| s.name == "mode")
        .map(|s| text_of(s, node));
    def.settings
        .iter()
        .filter(|s| {
            let always = match (def.kind, mode.as_deref()) {
                ("spawner", Some("burst")) => matches!(s.name, "mode" | "count" | "life"),
                ("spawner", Some("stream")) => {
                    matches!(s.name, "mode" | "rate" | "duration" | "life")
                }
                ("spawner", Some("event")) => matches!(s.name, "mode" | "layer" | "life"),
                ("billboard" | "ribbon", _) => s.name == "texture",
                _ => false,
            };
            always
                || node.inputs.contains_key(s.name)
                || wire_into(graph, &node.id, s.name).is_some()
        })
        .collect()
}

fn row_height(setting: &Setting) -> f32 {
    match setting.kind {
        Kind::Curve | Kind::Gradient => CURVE_ROW,
        _ => ROW,
    }
}

/// The wire into `node.pin`, by index.
fn wire_into(graph: &Graph, node: &str, pin: &str) -> Option<usize> {
    let end = format!("{node}.{pin}");
    graph.wires.iter().position(|w| w.to == end)
}

/// A node on the canvas, in canvas units.
struct Card {
    id: String,
    def: &'static FlowDef,
    rect: Rect,
    rows: Vec<&'static Setting>,
    preview: Option<u32>,
}

impl Card {
    /// Where a setting's row sits (its middle), in canvas units.
    fn row_y(&self, name: &str) -> f32 {
        let mut y = self.rect.min.y + TITLE;
        for s in &self.rows {
            if s.name == name {
                return y + row_height(s) * 0.5;
            }
            y += row_height(s);
        }
        self.rect.min.y + TITLE * 0.5
    }
}

/// A layer's row, in canvas units.
struct Lane {
    id: String,
    name: String,
    graph: Graph,
    rect: Rect,
    chain: Vec<Card>,
    values: Vec<Card>,
    problems: Vec<canopy_fx::Problem>,
    error: Option<String>,
}

impl Lane {
    fn card(&self, id: &str) -> Option<&Card> {
        self.chain.iter().chain(&self.values).find(|c| c.id == id)
    }

    /// The renderer the row ends in, if it draws.
    fn sink(&self) -> Option<&Card> {
        self.chain.iter().find(|c| c.def.flow == Flow::Sink)
    }
}

/// Everything on the canvas: the rows (by layer index) and the Effect node they feed.
struct Layout {
    lanes: Vec<Lane>,
    effect: Rect,
    /// Right of every row: renderer wires run straight to here, then bend to their pin.
    bus: f32,
    /// Where each drawing row's wire enters the Effect node: (layer, canvas point).
    pins: Vec<(usize, Pos2)>,
    preview: Option<u32>,
}

fn card_height(rows: &[&'static Setting], preview: bool) -> f32 {
    TITLE
        + rows.iter().map(|s| row_height(s)).sum::<f32>()
        + if preview { PREVIEW[1] + 8.0 } else { 0.0 }
        + 6.0
}

/// The layer and Emit an event-born layer's Spawner listens to.
fn parent_of(graph: &Graph) -> Option<(String, String)> {
    let spawner = graph.nodes.iter().find(|n| n.kind == "spawner")?;
    let def = flow::find("spawner")?;
    let setting = |name: &str| {
        def.settings
            .iter()
            .find(|s| s.name == name)
            .map(|s| text_of(s, spawner))
            .unwrap_or_default()
    };
    (setting("mode") == "event").then(|| (setting("layer"), setting("event")))
}

/// Every layer as a row, top to bottom: each root layer followed by the layers born of it
/// (each under the Emit that fires it), and the Effect node right of them all.
fn lay_out(layers: &[&Value], previews: Option<&crate::node_previews::NodePreviews>) -> Layout {
    let parsed: Vec<(String, String, Graph, Option<String>)> = layers
        .iter()
        .map(|layer| {
            let id = layer
                .get("id")
                .and_then(Value::as_str)
                .unwrap_or("layer")
                .to_string();
            let name = layer
                .get("name")
                .and_then(Value::as_str)
                .unwrap_or(&id)
                .to_string();
            let (graph, error) = match layer
                .get("graph")
                .map(|g| serde_json::from_value(g.clone()))
            {
                Some(Ok(graph)) => (graph, None),
                Some(Err(e)) => (Graph::default(), Some(e.to_string())),
                None => (Graph::default(), None),
            };
            (id, name, graph, error)
        })
        .collect();
    let parent: Vec<Option<(usize, String)>> = parsed
        .iter()
        .enumerate()
        .map(|(l, (_, _, graph, _))| {
            let (layer, event) = parent_of(graph)?;
            let p = parsed.iter().position(|x| x.0 == layer)?;
            (p != l).then_some((p, event))
        })
        .collect();
    // Roots in document order, each followed by what is born of it.
    let mut order: Vec<usize> = Vec::new();
    let mut stack: Vec<usize> = (0..parsed.len())
        .rev()
        .filter(|&l| parent[l].is_none())
        .collect();
    loop {
        while let Some(l) = stack.pop() {
            if order.contains(&l) {
                continue;
            }
            order.push(l);
            for c in (0..parsed.len()).rev() {
                if parent[c].as_ref().is_some_and(|(p, _)| *p == l) {
                    stack.push(c);
                }
            }
        }
        // Layers in a loop of events start rows of their own.
        match (0..parsed.len()).find(|l| !order.contains(l)) {
            Some(l) => stack.push(l),
            None => break,
        }
    }
    let mut placed: Vec<Option<Lane>> = (0..parsed.len()).map(|_| None).collect();
    let mut y = 0.0f32;
    for &l in &order {
        let (id, name, graph, error) = parsed[l].clone();
        let problems = if graph.nodes.is_empty() {
            Vec::new()
        } else {
            canopy_fx::check(&graph).err().unwrap_or_default()
        };
        let cell = |node: &str| previews.and_then(|p| p.cell(l, node));
        // Under the Emit that fires it, starting just right of the link coming down.
        let indent = parent[l]
            .as_ref()
            .and_then(|(p, emit)| placed[*p].as_ref()?.card(emit))
            .map_or(0.0, |c| c.rect.center().x + 26.0);
        let top = y + LANE_HEAD;
        let mut x = indent;
        let mut bottom = top + TITLE;
        let mut chain: Vec<Card> = Vec::new();
        for node in flow::chain(&graph) {
            let Some(def) = flow::find(&node.kind) else {
                continue;
            };
            let rows = shown(def, node, &graph);
            let preview = if previewed(def.kind) {
                cell(&node.id)
            } else {
                None
            };
            let h = card_height(&rows, preview.is_some());
            let rect = Rect::from_min_size(pos2(x, top), vec2(NODE_WIDTH, h));
            bottom = bottom.max(rect.max.y);
            chain.push(Card {
                id: node.id.clone(),
                def,
                rect,
                rows,
                preview,
            });
            x += NODE_WIDTH + GAP;
        }
        // Value nodes under the node they drive, stacked.
        let mut values: Vec<Card> = Vec::new();
        let mut below: Vec<(String, f32)> = Vec::new();
        let mut lowest = bottom;
        for node in &graph.nodes {
            let Some(def) = flow::find(&node.kind).filter(|d| d.flow == Flow::Value) else {
                continue;
            };
            let rows = shown(def, node, &graph);
            let h = card_height(&rows, false);
            let target = flow::driven(&graph, &node.id)
                .and_then(|(t, _)| chain.iter().find(|c| c.id == t))
                .or(chain.last());
            let (tx, tid, start) = target.map_or((x, String::new(), bottom), |c| {
                (c.rect.min.x, c.id.clone(), c.rect.max.y)
            });
            let free = match below.iter_mut().find(|(t, _)| *t == tid) {
                Some((_, at)) => at,
                None => {
                    below.push((tid, start + 18.0));
                    &mut below.last_mut().expect("pushed").1
                }
            };
            let rect = Rect::from_min_size(pos2(tx, *free), vec2(VALUE_WIDTH, h));
            *free = rect.max.y + 10.0;
            lowest = lowest.max(rect.max.y);
            values.push(Card {
                id: node.id.clone(),
                def,
                rect,
                rows,
                preview: None,
            });
        }
        let right = if chain.is_empty() {
            indent + NODE_WIDTH
        } else {
            x - GAP
        };
        let rect = Rect::from_min_max(pos2(indent - 12.0, y), pos2(right + 12.0, lowest + 12.0));
        y = rect.max.y + ROW_GAP;
        placed[l] = Some(Lane {
            id,
            name,
            graph,
            rect,
            chain,
            values,
            problems,
            error,
        });
    }
    let lanes: Vec<Lane> = placed
        .into_iter()
        .map(|l| l.expect("every row placed"))
        .collect();
    let right = lanes.iter().map(|l| l.rect.max.x).fold(0.0, f32::max);
    let rows = y - ROW_GAP;
    let width = (rows * 0.5).clamp(EFFECT_WIDTH[0], EFFECT_WIDTH[1]);
    let height = TITLE + 3.0 * ROW + 12.0 + (width - 16.0) / EFFECT_ASPECT + 8.0;
    let total = rows.max(height);
    let effect = Rect::from_min_size(
        pos2(right + EFFECT_GAP, (total - height) * 0.5),
        vec2(width, height),
    );
    let drawing: Vec<usize> = order
        .iter()
        .copied()
        .filter(|&l| lanes[l].sink().is_some())
        .collect();
    let span = (effect.min.y + TITLE + 12.0, effect.max.y - 12.0);
    let pins = drawing
        .iter()
        .enumerate()
        .map(|(i, &l)| {
            let t = if drawing.len() > 1 {
                i as f32 / (drawing.len() - 1) as f32
            } else {
                0.5
            };
            (l, pos2(effect.min.x, span.0 + (span.1 - span.0) * t))
        })
        .collect();
    Layout {
        lanes,
        effect,
        bus: right + 24.0,
        pins,
        preview: previews.and_then(|p| p.effect_cell()),
    }
}

fn unique_id(graph: &Graph, kind: &str) -> String {
    (1..)
        .map(|n| {
            if n == 1 {
                kind.to_string()
            } else {
                format!("{kind}{n}")
            }
        })
        .find(|id| !graph.nodes.iter().any(|node| &node.id == id))
        .unwrap_or_else(|| kind.to_string())
}

fn node(id: &str, kind: &str, inputs: Value) -> Node {
    serde_json::from_value(json!({"id": id, "kind": kind, "inputs": inputs})).expect("a node reads")
}

fn wire(from: &str, to: &str) -> Wire {
    Wire {
        from: from.into(),
        to: to.into(),
    }
}

/// A new layer: twenty soft dots a second for half a second, living a second, from a
/// small sphere.
fn starter_layer(id: &str) -> Value {
    json!({
        "id": id,
        "graph": {
            "nodes": [
                {"id": "spawner", "kind": "spawner", "inputs": {"mode": "stream", "rate": 20, "duration": 0.5}},
                {"id": "shape", "kind": "shape", "inputs": {"radius": 0.5}},
                {"id": "billboard", "kind": "billboard", "inputs": {"texture": "asset.effects.fx.soft-dot"}}
            ],
            "wires": [
                {"from": "spawner.out", "to": "shape.in"},
                {"from": "shape.out", "to": "billboard.in"}
            ]
        }
    })
}

/// The layer's nodes and wires replaced, as one undo step.
fn replace(document: &str, layer: usize, graph: &Graph, label: String) -> Change {
    let base = format!("/graphs/{layer}/graph");
    Change {
        label,
        pointer: format!("{base}/nodes"),
        commands: vec![
            Command::Set {
                document: document.into(),
                pointer: format!("{base}/nodes"),
                value: serde_json::to_value(&graph.nodes).unwrap_or(Value::Null),
                at: None,
            },
            Command::Set {
                document: document.into(),
                pointer: format!("{base}/wires"),
                value: serde_json::to_value(&graph.wires).unwrap_or(Value::Null),
                at: None,
            },
        ],
        merge: false,
    }
}

/// Inserts a `kind` node after `after` on the chain (its followers now follow the new
/// node), or at the chain's end before its renderers.
fn insert(graph: &Graph, after: Option<&str>, kind: &str) -> Graph {
    let mut g = graph.clone();
    let def = flow::find(kind).expect("an authoring node");
    let id = unique_id(&g, kind);
    g.nodes.push(node(&id, kind, json!({})));
    let after = after.map(str::to_string).or_else(|| {
        // The end: after the last node that is not a renderer.
        flow::chain(graph)
            .iter()
            .rev()
            .find(|n| flow::find(&n.kind).is_some_and(|d| d.flow != Flow::Sink))
            .map(|n| n.id.clone())
    });
    let Some(after) = after else { return g };
    let out = format!("{after}.out");
    if def.flow == Flow::Sink || def.flow == Flow::Source {
        // A renderer branches off; a spawner starts a chain of its own.
        if def.flow == Flow::Sink {
            g.wires.push(wire(&out, &format!("{id}.in")));
        }
        return g;
    }
    for w in g.wires.iter_mut().filter(|w| w.from == out) {
        w.from = format!("{id}.out");
    }
    g.wires.push(wire(&out, &format!("{id}.in")));
    g
}

/// Removes node `id`: what came before it now feeds what came after, and the value nodes
/// driving only it go with it.
fn remove(graph: &Graph, id: &str) -> Graph {
    let mut g = graph.clone();
    let before: Vec<String> = g
        .wires
        .iter()
        .filter(|w| w.to == format!("{id}.in"))
        .map(|w| w.from.clone())
        .collect();
    let after: Vec<String> = g
        .wires
        .iter()
        .filter(|w| w.from == format!("{id}.out"))
        .map(|w| w.to.clone())
        .collect();
    let drivers: Vec<String> = g
        .wires
        .iter()
        .filter(|w| Wire::split(&w.to).is_some_and(|(n, p)| n == id && p != "in"))
        .filter_map(|w| Wire::split(&w.from).map(|(n, _)| n.to_string()))
        .collect();
    let gone = |n: &str| n == id || drivers.iter().any(|d| d == n);
    g.wires.retain(|w| {
        ![&w.from, &w.to]
            .iter()
            .any(|end| Wire::split(end).is_some_and(|(n, _)| gone(n)))
    });
    for from in &before {
        for to in &after {
            g.wires.push(wire(from, to));
        }
    }
    g.nodes.retain(|n| !gone(&n.id));
    g
}

/// Swaps node `id` with its neighbour on a straight stretch of the chain (`up`: the one
/// before it).
fn shift(graph: &Graph, id: &str, up: bool) -> Option<Graph> {
    let prev = |n: &str| -> Option<String> {
        graph
            .wires
            .iter()
            .find(|w| w.to == format!("{n}.in"))
            .and_then(|w| Wire::split(&w.from).map(|(a, _)| a.to_string()))
    };
    let (a, b) = if up {
        (prev(id)?, id.to_string())
    } else {
        let next = graph
            .wires
            .iter()
            .filter(|w| w.from == format!("{id}.out"))
            .filter_map(|w| Wire::split(&w.to).map(|(n, _)| n.to_string()))
            .collect::<Vec<_>>();
        let [n] = &next[..] else { return None };
        (id.to_string(), n.clone())
    };
    // ... p → a → b → n ...  becomes  ... p → b → a → n ...
    let kind = |n: &str| {
        graph
            .nodes
            .iter()
            .find(|x| x.id == n)
            .and_then(|x| flow::find(&x.kind))
    };
    if kind(&a)?.flow != Flow::Through || kind(&b)?.flow != Flow::Through {
        return None;
    }
    let mut g = graph.clone();
    let (ao, bo, ai, bi) = (
        format!("{a}.out"),
        format!("{b}.out"),
        format!("{a}.in"),
        format!("{b}.in"),
    );
    for w in &mut g.wires {
        if w.to == ai {
            w.to = bi.clone();
        } else if w.from == ao && w.to == bi {
            w.from = bo.clone();
            w.to = ai.clone();
        } else if w.from == bo {
            w.from = ao.clone();
        }
    }
    Some(g)
}

/// A value node of `kind` driving `target.setting`, starting from what the setting holds.
fn drive(graph: &Graph, target: &str, setting: &Setting, kind: &str) -> Graph {
    let mut g = graph.clone();
    let Some(node_now) = graph.nodes.iter().find(|n| n.id == target) else {
        return g;
    };
    let inputs = match kind {
        "randomRange" => {
            let n = numbers_of(setting, node_now);
            let (lo, hi) = match setting.kind {
                Kind::Range => (vec![n[0]], vec![*n.get(1).unwrap_or(&n[0])]),
                _ => (n.clone(), n),
            };
            json!({"min": lo, "max": hi})
        }
        "curveOverLife" if setting.kind == Kind::Curve => {
            json!({"curve": serde_json::to_value(keys_of(setting, node_now)).unwrap_or(Value::Null)})
        }
        "gradientOverLife" if setting.kind == Kind::Gradient => {
            json!({"gradient": serde_json::to_value(keys_of(setting, node_now)).unwrap_or(Value::Null)})
        }
        _ => json!({}),
    };
    let id = unique_id(&g, kind);
    g.nodes.push(node(&id, kind, inputs));
    let to = format!("{target}.{}", setting.name);
    g.wires.retain(|w| w.to != to);
    g.wires.push(wire(&format!("{id}.value"), &to));
    g
}

/// Frees `target.setting`, removing the value node when it drives nothing else.
fn unwire(graph: &Graph, target: &str, setting: &str) -> Graph {
    let mut g = graph.clone();
    let to = format!("{target}.{setting}");
    let Some(w) = g.wires.iter().position(|w| w.to == to) else {
        return g;
    };
    let from = g.wires.remove(w).from;
    if let Some((driver, _)) = Wire::split(&from)
        && !g.wires.iter().any(|w| w.from == from)
    {
        let driver = driver.to_string();
        g.nodes.retain(|n| n.id != driver);
    }
    g
}

impl Canvas {
    fn to_screen(&self, origin: Pos2, p: Pos2) -> Pos2 {
        origin + (p.to_vec2() + self.pan) * self.zoom
    }

    fn screen_rect(&self, origin: Pos2, r: Rect) -> Rect {
        Rect::from_min_max(self.to_screen(origin, r.min), self.to_screen(origin, r.max))
    }

    fn to_canvas(&self, origin: Pos2, p: Pos2) -> Pos2 {
        ((p - origin) / self.zoom - self.pan).to_pos2()
    }

    /// Shows the effect `document` (its JSON `value`): its lanes, and the selection's
    /// inspector beside them.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        document: &str,
        value: &Value,
        previews: Option<(&crate::node_previews::NodePreviews, Option<egui::TextureId>)>,
    ) -> Out {
        let mut out = Out::default();
        let layers: Vec<&Value> = value
            .get("graphs")
            .and_then(Value::as_array)
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        ui.horizontal(|ui| {
            if ui.button("+ Layer").clicked() {
                let id = (1..)
                    .map(|n| format!("layer{n}"))
                    .find(|id| !layers.iter().any(|l| l.get("id").and_then(Value::as_str) == Some(id)))
                    .unwrap_or_default();
                let command = if value.get("graphs").is_some() {
                    Command::Insert {
                        document: document.into(),
                        pointer: "/graphs".into(),
                        index: layers.len(),
                        value: starter_layer(&id),
                    }
                } else {
                    Command::Set {
                        document: document.into(),
                        pointer: "/graphs".into(),
                        value: json!([starter_layer(&id)]),
                        at: None,
                    }
                };
                out.changes.push(Change {
                    label: "Add a layer".into(),
                    pointer: "/graphs".into(),
                    commands: vec![command],
                    merge: false,
                });
                self.selected = Some(Selection::Lane(layers.len()));
            }
            if ui.button("Fit").clicked() {
                self.fitted = None;
                self.moved = false;
            }
            ui.label(
                egui::RichText::new(
                    "Each row is a layer, left to right into the Effect. + on a wire inserts a node; right-click a node to move or remove it.",
                )
                .color(FAINT)
                .size(11.0),
            );
        });
        if layers.is_empty() {
            ui.label(
                egui::RichText::new("This effect has no layers yet. Add one to start.")
                    .color(MUTED),
            );
            return out;
        }
        let layout = lay_out(&layers, previews.map(|(p, _)| p));
        let lanes = &layout.lanes;
        let texture = previews.and_then(|(_, t)| t);
        let ticks = value
            .get("durationTicks")
            .and_then(Value::as_u64)
            .unwrap_or(40);
        let facts = [
            ("length", format!("{} s", number(ticks as f64 / 40.0))),
            (
                "loops",
                if value.get("loop").and_then(Value::as_bool) == Some(true) {
                    "yes".to_string()
                } else {
                    "no".to_string()
                },
            ),
            ("layers", layers.len().to_string()),
        ];
        let inspector_width = if self.selected.is_some() { 300.0 } else { 0.0 };
        let avail = ui.available_rect_before_wrap();
        let canvas_rect =
            Rect::from_min_max(avail.min, pos2(avail.max.x - inspector_width, avail.max.y));
        let inspector_rect = Rect::from_min_max(pos2(canvas_rect.max.x, avail.min.y), avail.max);
        ui.allocate_rect(avail, Sense::hover());
        self.canvas(
            ui,
            canvas_rect,
            document,
            &layout,
            &facts,
            texture,
            &mut out,
        );
        match self.selected.clone() {
            Some(Selection::Node(l, id)) => {
                match lanes.get(l).filter(|lane| lane.card(&id).is_some()) {
                    Some(lane) => {
                        let mut child = ui
                            .new_child(egui::UiBuilder::new().max_rect(inspector_rect.shrink(8.0)));
                        self.node_inspector(&mut child, document, l, lane, lanes, &id, &mut out);
                    }
                    None => self.selected = None,
                }
            }
            Some(Selection::Lane(l)) => match lanes.get(l) {
                Some(lane) => {
                    let mut child =
                        ui.new_child(egui::UiBuilder::new().max_rect(inspector_rect.shrink(8.0)));
                    self.lane_inspector(&mut child, document, l, lane, &mut out);
                }
                None => self.selected = None,
            },
            None => {}
        }
        out
    }

    #[allow(clippy::too_many_arguments)]
    fn canvas(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        document: &str,
        layout: &Layout,
        facts: &[(&str, String)],
        texture: Option<egui::TextureId>,
        out: &mut Out,
    ) {
        let lanes = &layout.lanes[..];
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, SUNK);
        let background = ui.interact(rect, ui.id().with("graph-canvas"), Sense::click_and_drag());
        if background.dragged_by(egui::PointerButton::Middle)
            || background.dragged_by(egui::PointerButton::Primary)
        {
            self.pan += background.drag_delta() / self.zoom;
            self.moved |= background.drag_delta() != Vec2::ZERO;
        }
        let pointer = ui.input(|i| i.pointer.hover_pos());
        let over_preview = pointer.is_some_and(|p| {
            self.screen_rect(rect.min, layout.effect).contains(p)
                || lanes.iter().any(|lane| {
                    lane.chain.iter().any(|c| {
                        c.preview.is_some()
                            && self.screen_rect(rect.min, preview_rect(c)).contains(p)
                    })
                })
        });
        if background.hovered() || ui.rect_contains_pointer(rect) {
            let (scroll, zoom) = ui.input(|i| (i.smooth_scroll_delta, i.zoom_delta()));
            if zoom != 1.0
                && let Some(p) = pointer
            {
                let before = self.to_canvas(rect.min, p);
                self.zoom = (self.zoom * zoom).clamp(0.2, 2.0);
                let after = self.to_canvas(rect.min, p);
                self.pan += after - before;
                self.moved = true;
            } else if over_preview && scroll.y != 0.0 {
                // A scroll over a preview moves the previews' camera.
                out.preview_zoom = Some((-scroll.y * 0.004).exp());
            } else if scroll != Vec2::ZERO {
                self.pan += scroll / self.zoom;
                self.moved = true;
            }
        }
        if background.clicked() {
            self.selected = None;
            self.palette = None;
        }
        if background.secondary_clicked()
            && let Some(at) = background.interact_pointer_pos()
        {
            let p = self.to_canvas(rect.min, at);
            if let Some(l) = lanes.iter().position(|lane| lane.rect.contains(p)) {
                self.palette = Some((at, l, None, String::new()));
            }
        }
        // Fit everything: the whole effect on one screen.
        let all = lanes
            .iter()
            .fold(layout.effect, |r, lane| r.union(lane.rect));
        if !self.moved && self.fitted != Some(all) {
            let size = all.size() + vec2(40.0, 40.0);
            self.zoom = (rect.width() / size.x)
                .min(rect.height() / size.y)
                .clamp(0.15, 1.0);
            self.pan = -all.min.to_vec2()
                + vec2(20.0, 20.0)
                + (vec2(rect.width(), rect.height()) / self.zoom - size).max(Vec2::ZERO) * 0.5;
            self.fitted = Some(all);
        }
        let z = self.zoom;
        let font = FontId::proportional(12.0 * z);
        let small = FontId::proportional(10.5 * z);
        // Rows: a quiet band with the layer's name over its boxes.
        for (l, lane) in lanes.iter().enumerate() {
            let r = self.screen_rect(rect.min, lane.rect);
            if !r.intersects(rect) {
                continue;
            }
            let chosen = self.selected == Some(Selection::Lane(l));
            painter.rect_filled(r, 8.0 * z, MAIN);
            painter.rect_stroke(
                r,
                8.0 * z,
                Stroke::new(1.0, if chosen { SOFT } else { BORDER }),
                egui::StrokeKind::Inside,
            );
            let head = Rect::from_min_size(r.min, vec2(r.width(), LANE_HEAD * z));
            let label = painter.text(
                head.left_center() + vec2(12.0 * z, 0.0),
                Align2::LEFT_CENTER,
                &lane.name,
                FontId::proportional(12.0 * z),
                if chosen { TEXT } else { SOFT },
            );
            let flag = if let Some(e) = &lane.error {
                Some(e.clone())
            } else {
                lane.problems.first().map(ToString::to_string)
            };
            if let Some(flag) = flag {
                painter.text(
                    label.right_center() + vec2(10.0 * z, 0.0),
                    Align2::LEFT_CENTER,
                    flag,
                    small.clone(),
                    ERROR,
                );
            }
            let grab = ui.interact(head, ui.id().with(("lane", l)), Sense::click());
            if grab.clicked() {
                self.selected = Some(Selection::Lane(l));
                self.palette = None;
            }
        }
        // Every renderer into the Effect node: the layers merging into the whole.
        let into = hex_rgb(0xca8a04);
        for &(l, pin) in &layout.pins {
            if let Some(sink) = lanes[l].sink() {
                // Straight along its own row, then bending into its pin.
                let y = title_y(sink);
                let p = self.to_screen(rect.min, pos2(sink.rect.max.x, y));
                let bus = self.to_screen(rect.min, pos2(layout.bus, y));
                let q = self.to_screen(rect.min, pin);
                painter.line_segment([p, bus], Stroke::new(1.5 * z.max(0.6), into));
                side_wire(&painter, bus, q, into, z);
                painter.circle_filled(q, 3.0 * z, into);
            }
        }
        // Particle wires along each row, with a + to insert on each.
        for (l, lane) in lanes.iter().enumerate() {
            for w in &lane.graph.wires {
                let (Some((a, ap)), Some((b, bp))) = (Wire::split(&w.from), Wire::split(&w.to))
                else {
                    continue;
                };
                let (Some(ca), Some(cb)) = (lane.card(a), lane.card(b)) else {
                    continue;
                };
                if ap == "out" && bp == "in" {
                    let p = self.to_screen(rect.min, pos2(ca.rect.max.x, title_y(ca)));
                    let q = self.to_screen(rect.min, pos2(cb.rect.min.x, title_y(cb)));
                    side_wire(&painter, p, q, MUTED, z);
                    let mid = pos2((p.x + q.x) * 0.5, (p.y + q.y) * 0.5);
                    if self.plus(ui, &painter, mid, ("insert", l, a), z) {
                        self.palette = Some((mid, l, Some(a.to_string()), String::new()));
                    }
                } else if ap == "value" {
                    // Up from the value node to the node it drives.
                    let p = self.to_screen(rect.min, pos2(ca.rect.min.x + 14.0, ca.rect.min.y));
                    let q = self.to_screen(rect.min, pos2(cb.rect.min.x, cb.row_y(bp)));
                    let reach = 18.0 * z;
                    painter.add(CubicBezierShape::from_points_stroke(
                        [p, p - vec2(0.0, reach), q - vec2(reach, 0.0), q],
                        false,
                        Color32::TRANSPARENT,
                        Stroke::new(1.5 * z.max(0.6), hex_rgb(0x71717a)),
                    ));
                }
            }
            // A + after a chain that draws nothing yet.
            if lane.sink().is_none()
                && let Some(last) = lane.chain.last()
            {
                let at = self.to_screen(rect.min, pos2(last.rect.max.x + GAP * 0.5, title_y(last)));
                if self.plus(ui, &painter, at, ("append", l, ""), z) {
                    self.palette = Some((at, l, None, String::new()));
                }
            }
        }
        // Event links: down from an Emit to the rows born of it.
        let magenta = hex_rgb(0xd946ef);
        for lane in lanes {
            let Some(spawner) = lane.chain.iter().find(|c| c.def.kind == "spawner") else {
                continue;
            };
            let Some((parent, event)) = parent_of(&lane.graph) else {
                continue;
            };
            let Some(source) = lanes
                .iter()
                .find(|x| x.id == parent)
                .and_then(|x| x.card(&event))
            else {
                continue;
            };
            let p = pos2(source.rect.center().x, source.rect.max.y);
            let q = pos2(spawner.rect.min.x, title_y(spawner));
            let corner = pos2(p.x, q.y);
            let points: Vec<Pos2> = [p, corner, q]
                .iter()
                .map(|&c| self.to_screen(rect.min, c))
                .collect();
            painter.add(egui::Shape::line(
                points,
                Stroke::new(1.5 * z.max(0.6), magenta),
            ));
            painter.circle_filled(self.to_screen(rect.min, q), 3.5 * z, magenta);
        }
        // Nodes.
        let delete_pressed = ui
            .input(|i| i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace))
            && !ui.ctx().egui_wants_keyboard_input();
        for (l, lane) in lanes.iter().enumerate() {
            for card in lane.chain.iter().chain(&lane.values) {
                let Some(node) = lane.graph.nodes.iter().find(|n| n.id == card.id) else {
                    continue;
                };
                let body = self.screen_rect(rect.min, card.rect);
                if !body.intersects(rect) {
                    continue;
                }
                let selected = self.selected == Some(Selection::Node(l, card.id.clone()));
                let problem = lane
                    .problems
                    .iter()
                    .any(|p| p.node.as_deref() == Some(card.id.as_str()));
                painter.rect_filled(body, 6.0 * z, RAISED);
                let header = Rect::from_min_size(body.min, vec2(body.width(), TITLE * z));
                let r = (6.0 * z) as u8;
                painter.rect_filled(
                    header,
                    egui::CornerRadius {
                        nw: r,
                        ne: r,
                        sw: 0,
                        se: 0,
                    },
                    group_colour(card.def),
                );
                painter.rect_stroke(
                    body,
                    6.0 * z,
                    Stroke::new(
                        if selected { 1.5 } else { 1.0 },
                        if problem {
                            ERROR
                        } else if selected {
                            TEXT
                        } else {
                            BORDER
                        },
                    ),
                    egui::StrokeKind::Inside,
                );
                painter.text(
                    header.left_center() + vec2(8.0 * z, 0.0),
                    Align2::LEFT_CENTER,
                    node.name.as_deref().unwrap_or(card.def.title),
                    font.clone(),
                    TEXT,
                );
                // Rows.
                let mut y = card.rect.min.y + TITLE;
                for s in &card.rows {
                    let h = row_height(s);
                    let row = self.screen_rect(
                        rect.min,
                        Rect::from_min_size(pos2(card.rect.min.x, y), vec2(card.rect.width(), h)),
                    );
                    self.row(&painter, row, s, node, &lane.graph, z);
                    y += h;
                }
                // The layer as the chain makes it so far.
                if let Some(cell) = card.preview {
                    let area = self.screen_rect(rect.min, preview_rect(card));
                    match texture {
                        Some(texture) => {
                            let [min, max] = canopy_render::graph_thumbs::GraphThumbs::uv(cell);
                            painter.image(
                                texture,
                                area,
                                Rect::from_min_max(pos2(min[0], min[1]), pos2(max[0], max[1])),
                                Color32::WHITE,
                            );
                        }
                        None => {
                            painter.rect_filled(area, 3.0, SUNK);
                        }
                    }
                    painter.rect_stroke(
                        area,
                        3.0,
                        Stroke::new(1.0, BORDER),
                        egui::StrokeKind::Inside,
                    );
                }
                let grab = ui.interact(body, ui.id().with(("node", l, &card.id)), Sense::click());
                if grab.clicked() {
                    self.selected = Some(Selection::Node(l, card.id.clone()));
                    self.palette = None;
                }
                grab.context_menu(|ui| {
                    if card.def.flow != Flow::Value
                        && card.def.flow != Flow::Sink
                        && ui.button("Insert a node after").clicked()
                    {
                        self.palette =
                            Some((body.right_center(), l, Some(card.id.clone()), String::new()));
                        ui.close();
                    }
                    if card.def.flow == Flow::Through {
                        for (up, label) in [(true, "Move left"), (false, "Move right")] {
                            if let Some(g) = shift(&lane.graph, &card.id, up)
                                && ui.button(label).clicked()
                            {
                                out.changes.push(replace(
                                    document,
                                    l,
                                    &g,
                                    format!("{label}: {}", card.def.title),
                                ));
                                ui.close();
                            }
                        }
                    }
                    if ui.button("Remove").clicked() {
                        out.changes.push(replace(
                            document,
                            l,
                            &remove(&lane.graph, &card.id),
                            format!("Remove {}", card.def.title),
                        ));
                        self.selected = None;
                        ui.close();
                    }
                });
                if selected && delete_pressed {
                    out.changes.push(replace(
                        document,
                        l,
                        &remove(&lane.graph, &card.id),
                        format!("Remove {}", card.def.title),
                    ));
                    self.selected = None;
                }
            }
        }
        self.effect_node(&painter, rect, layout, facts, texture);
        self.palette_ui(ui, document, lanes, out);
    }

    /// The node every layer merges into: the whole effect playing, and what it is.
    fn effect_node(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        layout: &Layout,
        facts: &[(&str, String)],
        texture: Option<egui::TextureId>,
    ) {
        let z = self.zoom;
        let body = self.screen_rect(rect.min, layout.effect);
        if !body.intersects(rect) {
            return;
        }
        let gold = hex_rgb(0xca8a04);
        painter.rect_filled(body, 8.0 * z, RAISED);
        let header = Rect::from_min_size(body.min, vec2(body.width(), TITLE * z));
        let r = (8.0 * z) as u8;
        painter.rect_filled(
            header,
            egui::CornerRadius {
                nw: r,
                ne: r,
                sw: 0,
                se: 0,
            },
            hex_rgb(0x713f12),
        );
        painter.rect_stroke(
            body,
            8.0 * z,
            Stroke::new(1.5, gold),
            egui::StrokeKind::Inside,
        );
        painter.text(
            header.left_center() + vec2(8.0 * z, 0.0),
            Align2::LEFT_CENTER,
            "Effect",
            FontId::proportional(12.0 * z),
            TEXT,
        );
        let mut y = layout.effect.min.y + TITLE + 4.0;
        for (name, value) in facts {
            let row = self.screen_rect(
                rect.min,
                Rect::from_min_size(
                    pos2(layout.effect.min.x, y),
                    vec2(layout.effect.width(), ROW),
                ),
            );
            painter.text(
                row.left_center() + vec2(8.0 * z, 0.0),
                Align2::LEFT_CENTER,
                *name,
                FontId::proportional(10.5 * z),
                SOFT,
            );
            painter.text(
                pos2(row.min.x + row.width() * 0.3, row.center().y),
                Align2::LEFT_CENTER,
                value,
                FontId::proportional(10.5 * z),
                BODY,
            );
            y += ROW;
        }
        let size = vec2(
            layout.effect.width() - 16.0,
            (layout.effect.width() - 16.0) / EFFECT_ASPECT,
        );
        let area = self.screen_rect(
            rect.min,
            Rect::from_min_size(
                pos2(
                    layout.effect.min.x + 8.0,
                    layout.effect.max.y - size.y - 8.0,
                ),
                size,
            ),
        );
        match (texture, layout.preview) {
            (Some(texture), Some(cell)) => {
                let [min, max] = canopy_render::graph_thumbs::GraphThumbs::uv(cell);
                painter.image(
                    texture,
                    area,
                    Rect::from_min_max(pos2(min[0], min[1]), pos2(max[0], max[1])),
                    Color32::WHITE,
                );
            }
            _ => {
                painter.rect_filled(area, 3.0, SUNK);
            }
        }
        painter.rect_stroke(
            area,
            3.0,
            Stroke::new(1.0, BORDER),
            egui::StrokeKind::Inside,
        );
    }

    /// A small + button; true when clicked.
    fn plus(
        &self,
        ui: &Ui,
        painter: &egui::Painter,
        at: Pos2,
        salt: (&str, usize, &str),
        z: f32,
    ) -> bool {
        let r = Rect::from_center_size(at, vec2(16.0, 16.0) * z.max(0.6));
        let hit = ui.interact(r, ui.id().with(salt), Sense::click());
        let colour = if hit.hovered() { TEXT } else { FAINT };
        painter.circle(at, 7.0 * z.max(0.6), RAISED, Stroke::new(1.0, colour));
        painter.text(
            at,
            Align2::CENTER_CENTER,
            "+",
            FontId::proportional(12.0 * z.max(0.6)),
            colour,
        );
        hit.clicked()
    }

    /// One setting's row on a card: its name, and its value, curve or the node driving it.
    fn row(
        &self,
        painter: &egui::Painter,
        row: Rect,
        s: &Setting,
        node: &Node,
        graph: &Graph,
        z: f32,
    ) {
        let small = FontId::proportional(10.5 * z);
        let label = row.left_center() + vec2(8.0 * z, 0.0);
        painter.text(label, Align2::LEFT_CENTER, s.name, small.clone(), SOFT);
        let value_at = Rect::from_min_max(
            pos2(row.min.x + row.width() * 0.42, row.min.y),
            row.max - vec2(6.0 * z, 0.0),
        );
        if let Some(w) = wire_into(graph, &node.id, s.name) {
            let from = Wire::split(&graph.wires[w].from)
                .map(|(n, _)| n)
                .unwrap_or("");
            let title = graph
                .nodes
                .iter()
                .find(|n| n.id == from)
                .and_then(|n| flow::find(&n.kind))
                .map_or(from, |d| d.title);
            painter.circle_filled(row.left_center(), 3.0 * z, hex_rgb(0x71717a));
            painter.text(
                value_at.left_center(),
                Align2::LEFT_CENTER,
                format!("← {title}"),
                small,
                MUTED,
            );
            return;
        }
        match s.kind {
            Kind::Curve | Kind::Gradient => {
                painter.rect_filled(value_at.shrink(2.0 * z), 2.0, SUNK);
                plot_curve(painter, value_at.shrink(4.0 * z), &keys_of(s, node));
            }
            Kind::Choice(_) | Kind::Text => {
                painter.text(
                    value_at.left_center(),
                    Align2::LEFT_CENTER,
                    short(&text_of(s, node)),
                    small,
                    BODY,
                );
            }
            Kind::Range => {
                let n = numbers_of(s, node);
                let text = match &n[..] {
                    [a, b] if a != b => format!("{} – {}", number(*a), number(*b)),
                    [a, ..] => number(*a),
                    [] => String::new(),
                };
                painter.text(
                    value_at.left_center(),
                    Align2::LEFT_CENTER,
                    text,
                    small,
                    BODY,
                );
            }
            Kind::Number | Kind::Vector => {
                let text = numbers_of(s, node)
                    .iter()
                    .map(|x| number(*x))
                    .collect::<Vec<_>>()
                    .join("  ");
                painter.text(
                    value_at.left_center(),
                    Align2::LEFT_CENTER,
                    text,
                    small,
                    BODY,
                );
            }
        }
    }

    fn palette_ui(&mut self, ui: &mut Ui, document: &str, lanes: &[Lane], out: &mut Out) {
        let Some((screen, l, after, mut search)) = self.palette.take() else {
            return;
        };
        let Some(lane) = lanes.get(l) else { return };
        let mut keep = true;
        let mut chosen: Option<&'static FlowDef> = None;
        let has_spawner = lane.graph.nodes.iter().any(|n| n.kind == "spawner");
        egui::Area::new(ui.id().with("graph-palette"))
            .fixed_pos(screen)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).fill(PANEL).show(ui, |ui| {
                    ui.set_width(220.0);
                    let edit =
                        ui.add(egui::TextEdit::singleline(&mut search).hint_text("Search nodes"));
                    edit.request_focus();
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        keep = false;
                    }
                    let needle = search.to_lowercase();
                    egui::ScrollArea::vertical()
                        .max_height(340.0)
                        .show(ui, |ui| {
                            let mut group = None;
                            for def in flow::NODES {
                                if def.flow == Flow::Value
                                    || (def.flow == Flow::Source && has_spawner)
                                {
                                    continue;
                                }
                                if !needle.is_empty()
                                    && !def.title.to_lowercase().contains(&needle)
                                    && !def.kind.to_lowercase().contains(&needle)
                                {
                                    continue;
                                }
                                if group != Some(def.group) {
                                    ui.label(
                                        egui::RichText::new(def.group.title())
                                            .color(FAINT)
                                            .size(11.0),
                                    );
                                    group = Some(def.group);
                                }
                                if ui
                                    .selectable_label(false, def.title)
                                    .on_hover_text(def.help)
                                    .clicked()
                                    || (edit.lost_focus()
                                        && ui.input(|i| i.key_pressed(egui::Key::Enter))
                                        && chosen.is_none())
                                {
                                    chosen = Some(def);
                                }
                            }
                        });
                });
            });
        if let Some(def) = chosen {
            let g = insert(&lane.graph, after.as_deref(), def.kind);
            let id = g.nodes.last().map(|n| n.id.clone()).unwrap_or_default();
            out.changes
                .push(replace(document, l, &g, format!("Add {}", def.title)));
            self.selected = Some(Selection::Node(l, id));
            keep = false;
        }
        if keep {
            self.palette = Some((screen, l, after, search));
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn node_inspector(
        &mut self,
        ui: &mut Ui,
        document: &str,
        l: usize,
        lane: &Lane,
        lanes: &[Lane],
        id: &str,
        out: &mut Out,
    ) {
        let Some(index) = lane.graph.nodes.iter().position(|n| n.id == id) else {
            return;
        };
        let node = &lane.graph.nodes[index];
        let Some(def) = flow::find(&node.kind) else {
            return;
        };
        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(egui::Margin::same(10))
            .corner_radius(RADIUS)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(
                    egui::RichText::new(node.name.as_deref().unwrap_or(def.title))
                        .color(TEXT)
                        .strong(),
                );
                ui.label(egui::RichText::new(def.help).color(MUTED).size(11.5));
                for p in lane
                    .problems
                    .iter()
                    .filter(|p| p.node.as_deref() == Some(id))
                {
                    ui.colored_label(ERROR, p.message.clone());
                }
                ui.add_space(6.0);
                egui::ScrollArea::vertical()
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        let mode = def
                            .settings
                            .iter()
                            .find(|s| s.name == "mode")
                            .map(|s| text_of(s, node));
                        for s in def.settings {
                            if def.kind == "spawner" {
                                let wanted = match mode.as_deref() {
                                    Some("burst") => &["count", "time", "life", "ends", "fade"][..],
                                    Some("stream") => &[
                                        "rate",
                                        "delay",
                                        "duration",
                                        "rateCurve",
                                        "life",
                                        "ends",
                                        "fade",
                                    ][..],
                                    _ => &[
                                        "layer",
                                        "event",
                                        "spray",
                                        "rate",
                                        "rateCurve",
                                        "life",
                                        "ends",
                                        "fade",
                                    ][..],
                                };
                                if s.name != "mode" && !wanted.contains(&s.name) {
                                    continue;
                                }
                            }
                            let pointer =
                                format!("/graphs/{l}/graph/nodes/{index}/inputs/{}", s.name);
                            ui.add_space(2.0);
                            if let Some(w) = wire_into(&lane.graph, id, s.name) {
                                let from = Wire::split(&lane.graph.wires[w].from)
                                    .map(|(n, _)| n.to_string())
                                    .unwrap_or_default();
                                ui.horizontal(|ui| {
                                    ui.label(egui::RichText::new(s.name).color(SOFT));
                                    if ui.link(format!("← {from}")).clicked() {
                                        self.selected = Some(Selection::Node(l, from.clone()));
                                    }
                                    if ui.small_button("Unwire").clicked() {
                                        out.changes.push(replace(
                                            document,
                                            l,
                                            &unwire(&lane.graph, id, s.name),
                                            format!("Unwire {}", s.name),
                                        ));
                                    }
                                });
                                continue;
                            }
                            let choices = event_choices(def, s, node, lanes);
                            if let Some(value) = edit_setting(ui, s, node, choices.as_deref()) {
                                let command = if node.inputs.is_empty() {
                                    Command::Set {
                                        document: document.into(),
                                        pointer: format!("/graphs/{l}/graph/nodes/{index}/inputs"),
                                        value: json!({ s.name: value }),
                                        at: None,
                                    }
                                } else {
                                    Command::Set {
                                        document: document.into(),
                                        pointer: pointer.clone(),
                                        value,
                                        at: None,
                                    }
                                };
                                out.changes.push(Change {
                                    label: format!("{} {}", def.title, s.name),
                                    pointer,
                                    commands: vec![command],
                                    merge: true,
                                });
                            }
                            if s.wired {
                                ui.horizontal(|ui| {
                                    ui.add_space(8.0);
                                    ui.menu_button(
                                        egui::RichText::new("Drive by…").size(11.0).color(FAINT),
                                        |ui| {
                                            for &kind in drivers(s) {
                                                let title =
                                                    flow::find(kind).map_or(kind, |d| d.title);
                                                if ui.button(title).clicked() {
                                                    out.changes.push(replace(
                                                        document,
                                                        l,
                                                        &drive(&lane.graph, id, s, kind),
                                                        format!("Drive {} by {title}", s.name),
                                                    ));
                                                    ui.close();
                                                }
                                            }
                                        },
                                    );
                                });
                            }
                        }
                    });
            });
    }

    fn lane_inspector(
        &mut self,
        ui: &mut Ui,
        document: &str,
        l: usize,
        lane: &Lane,
        out: &mut Out,
    ) {
        egui::Frame::new()
            .fill(PANEL)
            .inner_margin(egui::Margin::same(10))
            .corner_radius(RADIUS)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.label(egui::RichText::new("Layer").color(TEXT).strong());
                ui.label(
                    egui::RichText::new(format!("Id {}", lane.id))
                        .color(MUTED)
                        .size(11.5),
                );
                ui.add_space(6.0);
                let mut name = lane.name.clone();
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Name").color(SOFT));
                    if ui.text_edit_singleline(&mut name).changed() {
                        out.changes.push(Change {
                            label: "Rename the layer".into(),
                            pointer: format!("/graphs/{l}/name"),
                            commands: vec![Command::Set {
                                document: document.into(),
                                pointer: format!("/graphs/{l}/name"),
                                value: json!(name),
                                at: None,
                            }],
                            merge: true,
                        });
                    }
                });
                for p in &lane.problems {
                    ui.colored_label(ERROR, p.to_string());
                }
                ui.add_space(10.0);
                if ui.button("Remove the layer").clicked() {
                    out.changes.push(Change {
                        label: format!("Remove layer {}", lane.name),
                        pointer: "/graphs".into(),
                        commands: vec![Command::Remove {
                            document: document.into(),
                            pointer: format!("/graphs/{l}"),
                        }],
                        merge: false,
                    });
                    self.selected = None;
                }
            });
    }
}

/// The value nodes that can drive a setting.
fn drivers(s: &Setting) -> &'static [&'static str] {
    match s.kind {
        Kind::Curve => &["curveOverLife", "randomRange", "particleValue"],
        Kind::Gradient => &["gradientOverLife"],
        _ => &["randomRange", "curveOverLife", "particleValue"],
    }
}

/// For an event Spawner: the layers it can listen to, or the events of the chosen layer.
fn event_choices(def: &FlowDef, s: &Setting, node: &Node, lanes: &[Lane]) -> Option<Vec<String>> {
    if def.kind != "spawner" {
        return None;
    }
    match s.name {
        "layer" => Some(lanes.iter().map(|l| l.id.clone()).collect()),
        "event" => {
            let layer = def
                .settings
                .iter()
                .find(|x| x.name == "layer")
                .map(|x| text_of(x, node))?;
            let lane = lanes.iter().find(|l| l.id == layer)?;
            Some(
                lane.graph
                    .nodes
                    .iter()
                    .filter(|n| n.kind == "emit")
                    .map(|n| n.id.clone())
                    .collect(),
            )
        }
        _ => None,
    }
}

fn preview_rect(card: &Card) -> Rect {
    Rect::from_min_size(
        pos2(
            card.rect.center().x - PREVIEW[0] * 0.5,
            card.rect.max.y - PREVIEW[1] - 8.0,
        ),
        vec2(PREVIEW[0], PREVIEW[1]),
    )
}

/// The height particle wires meet a node at: its title.
fn title_y(card: &Card) -> f32 {
    card.rect.min.y + TITLE * 0.5
}

/// A setting's editor in the inspector; the new value when it changed.
fn edit_setting(
    ui: &mut Ui,
    s: &Setting,
    node: &Node,
    choices: Option<&[String]>,
) -> Option<Value> {
    let mut changed = None;
    ui.horizontal(|ui| {
        ui.label(egui::RichText::new(s.name).color(SOFT));
        match s.kind {
            Kind::Choice(options) => {
                let now = text_of(s, node);
                egui::ComboBox::from_id_salt((s.name, &node.id))
                    .selected_text(&now)
                    .show_ui(ui, |ui| {
                        for choice in options {
                            if ui.selectable_label(now == *choice, *choice).clicked() {
                                changed = Some(json!(choice));
                            }
                        }
                    });
            }
            Kind::Text => {
                let mut text = text_of(s, node);
                match choices {
                    Some(options) => {
                        egui::ComboBox::from_id_salt((s.name, &node.id))
                            .selected_text(&text)
                            .show_ui(ui, |ui| {
                                for choice in options {
                                    if ui.selectable_label(&text == choice, choice).clicked() {
                                        changed = Some(json!(choice));
                                    }
                                }
                            });
                    }
                    None => {
                        if ui
                            .add(egui::TextEdit::singleline(&mut text).desired_width(f32::INFINITY))
                            .changed()
                        {
                            changed = Some(json!(text));
                        }
                    }
                }
            }
            Kind::Curve | Kind::Gradient => {}
            Kind::Range | Kind::Number | Kind::Vector => {
                let mut n = numbers_of(s, node);
                let count = match s.kind {
                    Kind::Range => 2,
                    Kind::Number => 1,
                    _ => 3,
                };
                if n.len() == 1 && count > 1 {
                    n = vec![n[0]; count];
                }
                n.resize(count, 0.0);
                let mut any = false;
                for x in &mut n {
                    any |= ui
                        .add(egui::DragValue::new(x).speed(0.01).max_decimals(3))
                        .changed();
                }
                if any {
                    changed = Some(if count == 1 { json!(n[0]) } else { json!(n) });
                }
            }
        }
    });
    if matches!(s.kind, Kind::Curve | Kind::Gradient) {
        changed = edit_curve(ui, &keys_of(s, node), s.kind == Kind::Gradient).or(changed);
    }
    changed
}

/// The curve, plotted large, with its keys' times and values to edit, and keys to add or
/// take away.
fn edit_curve(ui: &mut Ui, curve: &Curve, colour: bool) -> Option<Value> {
    let (rect, _) = ui.allocate_exact_size(
        vec2(ui.available_width(), if colour { 56.0 } else { 90.0 }),
        Sense::hover(),
    );
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, SUNK);
    plot_curve(&painter, rect.shrink(6.0), curve);
    let mut keys = curve.keys.clone();
    let mut changed = false;
    let mut drop = None;
    for (i, key) in keys.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            changed |= ui
                .add(
                    egui::DragValue::new(&mut key.t)
                        .speed(0.005)
                        .range(0.0..=1.0)
                        .max_decimals(3),
                )
                .changed();
            for v in &mut key.v {
                changed |= ui
                    .add(egui::DragValue::new(v).speed(0.01).max_decimals(3))
                    .changed();
            }
            if colour && key.v.len() >= 3 {
                let c = Color32::from_rgb(srgb(key.v[0]), srgb(key.v[1]), srgb(key.v[2]));
                let (r, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                ui.painter().rect_filled(r, 2.0, c);
            }
            if ui.small_button("−").clicked() {
                drop = Some(i);
            }
        });
    }
    if let Some(i) = drop
        && keys.len() > 1
    {
        keys.remove(i);
        changed = true;
    }
    if ui.small_button("+ key").clicked()
        && let Some(last) = keys.last().cloned()
    {
        let t = keys
            .get(keys.len().wrapping_sub(2))
            .map_or(0.5, |k| (k.t + last.t) * 0.5);
        let v = curve.sample(t);
        keys.push(CurveKey {
            t,
            v,
            tangent_in: None,
            tangent_out: None,
        });
        keys.sort_by(|a, b| a.t.total_cmp(&b.t));
        changed = true;
    }
    changed.then(|| serde_json::to_value(Curve { keys }).unwrap_or(Value::Null))
}

fn side_wire(painter: &egui::Painter, a: Pos2, b: Pos2, colour: Color32, zoom: f32) {
    let reach = ((b.x - a.x).abs() * 0.5).max(30.0 * zoom);
    painter.add(CubicBezierShape::from_points_stroke(
        [a, a + vec2(reach, 0.0), b - vec2(reach, 0.0), b],
        false,
        Color32::TRANSPARENT,
        Stroke::new(1.5 * zoom.max(0.6), colour),
    ));
}

/// A curve's plot: one line per component, or a colour strip with its alpha for colours.
fn plot_curve(painter: &egui::Painter, rect: Rect, curve: &Curve) {
    if curve.keys.is_empty() {
        return;
    }
    let width = curve.width();
    if width >= 3 {
        let strip = Rect::from_min_size(rect.min, vec2(rect.width(), rect.height() * 0.6));
        let steps = 40;
        for s in 0..steps {
            let t = s as f64 / (steps - 1) as f64;
            let c = curve.sample(t);
            let x0 = strip.min.x + strip.width() * s as f32 / steps as f32;
            let r = Rect::from_min_max(
                pos2(x0, strip.min.y),
                pos2(x0 + strip.width() / steps as f32 + 0.5, strip.max.y),
            );
            painter.rect_filled(
                r,
                0.0,
                Color32::from_rgb(srgb(c[0]), srgb(c[1]), srgb(c[2])),
            );
        }
        if width == 4 {
            let lane = Rect::from_min_max(pos2(rect.min.x, strip.max.y + 2.0), rect.max);
            line(painter, lane, curve, 3, MUTED);
        }
        return;
    }
    for c in 0..width {
        line(
            painter,
            rect,
            curve,
            c,
            [hex_rgb(0xfdba74), hex_rgb(0x86efac)][c.min(1)],
        );
    }
}

fn line(painter: &egui::Painter, rect: Rect, curve: &Curve, component: usize, colour: Color32) {
    let samples: Vec<f64> = (0..=40)
        .map(|s| {
            curve
                .sample(f64::from(s) / 40.0)
                .get(component)
                .copied()
                .unwrap_or(0.0)
        })
        .collect();
    let (lo, hi) = samples
        .iter()
        .fold((0.0f64, f64::MIN), |(lo, hi), &v| (lo.min(v), hi.max(v)));
    let span = (hi - lo).max(1e-6);
    let points: Vec<Pos2> = samples
        .iter()
        .enumerate()
        .map(|(s, &v)| {
            pos2(
                rect.min.x + rect.width() * s as f32 / 40.0,
                rect.max.y - rect.height() * ((v - lo) / span) as f32,
            )
        })
        .collect();
    painter.add(egui::Shape::line(points, Stroke::new(1.5, colour)));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layer() -> Graph {
        serde_json::from_value(starter_layer("a")["graph"].clone()).expect("graph")
    }

    fn order(g: &Graph) -> Vec<String> {
        flow::chain(g).iter().map(|n| n.kind.clone()).collect()
    }

    #[test]
    fn nodes_insert_move_and_leave_the_chain_whole() {
        let g = insert(&layer(), Some("shape"), "launch");
        assert_eq!(order(&g), ["spawner", "shape", "launch", "billboard"]);
        let g = insert(&g, None, "colourOverLife");
        assert_eq!(
            order(&g),
            ["spawner", "shape", "launch", "colourOverLife", "billboard"]
        );
        let g = shift(&g, "launch", true).expect("moves");
        assert_eq!(
            order(&g),
            ["spawner", "launch", "shape", "colourOverLife", "billboard"]
        );
        let s = flow::find("launch")
            .and_then(|d| d.settings.iter().find(|s| s.name == "speed"))
            .expect("speed");
        let g = drive(&g, "launch", s, "randomRange");
        assert!(flow::driven(&g, "randomRange").is_some());
        let g = remove(&g, "launch");
        assert_eq!(
            order(&g),
            ["spawner", "shape", "colourOverLife", "billboard"]
        );
        assert!(
            !g.nodes.iter().any(|n| n.kind == "randomRange"),
            "its value node goes with it"
        );
        assert!(canopy_fx::check(&g).is_ok());
    }
}
