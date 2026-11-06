//! The authoring nodes (D115, docs/effect-graph.md "Authoring nodes"): high-level nodes on a
//! particle flow, as PopcornFX v2's templates. A layer is a chain from its Spawner to its
//! renderers along `in`/`out` particle wires; each node's settings are values on the node
//! or wires from value nodes (Random range, Curve over life, Gradient over life, Particle
//! value). The chain's order is the order things happen to the particles.
//!
//! [`expand`] turns such a layer into the compiler's primitives ([`crate::catalogue`]):
//! each node adds the primitives it stands for, and the appearance nodes (size, colour,
//! spin, flipbook) hand their latest values down the chain to the renderers, which draw
//! them (anything not set yet draws as small white dots). The GPU runtime is unchanged.

use crate::{
    Graph, Node, Value, Wire,
    check::Problem,
    graph::{Curve, CurveKey},
};
use std::collections::BTreeMap;

/// How a setting is given.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Kind {
    /// A random range per particle: `[min, max]` (or one number); a wire overrides it.
    Range,
    /// One number.
    Number,
    /// Three numbers.
    Vector,
    /// A curve over 0–1 (one component).
    Curve,
    /// A colour-and-alpha curve over 0–1 (HDR).
    Gradient,
    /// One of `choices`.
    Choice(&'static [&'static str]),
    /// Free text (an asset id, a layer or event name).
    Text,
}

/// A setting's default.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Default {
    Number(f64),
    Numbers(&'static [f64]),
    Text(&'static str),
    /// Keys as (t, values).
    Keys(&'static [(f64, &'static [f64])]),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Setting {
    pub name: &'static str,
    pub kind: Kind,
    pub default: Default,
    /// Takes a wire from a value node.
    pub wired: bool,
}

/// Where a node sits on the particle flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flow {
    /// Starts the flow (`out` only): the Spawner.
    Source,
    /// On the flow (`in` and `out`).
    Through,
    /// Ends the flow (`in` only): renderers.
    Sink,
    /// Off the flow: feeds settings (`value` out).
    Value,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Spawn,
    Birth,
    Motion,
    Appearance,
    Events,
    Draw,
    Values,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Spawn => "Spawn",
            Group::Birth => "Birth",
            Group::Motion => "Motion",
            Group::Appearance => "Appearance",
            Group::Events => "Events",
            Group::Draw => "Draw",
            Group::Values => "Values",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct FlowDef {
    pub kind: &'static str,
    pub title: &'static str,
    pub group: Group,
    pub flow: Flow,
    pub settings: &'static [Setting],
    pub help: &'static str,
}

const fn range(name: &'static str, d: &'static [f64]) -> Setting {
    Setting {
        name,
        kind: Kind::Range,
        default: Default::Numbers(d),
        wired: true,
    }
}
const fn number(name: &'static str, d: f64) -> Setting {
    Setting {
        name,
        kind: Kind::Number,
        default: Default::Number(d),
        wired: false,
    }
}
const fn vector(name: &'static str, d: &'static [f64]) -> Setting {
    Setting {
        name,
        kind: Kind::Vector,
        default: Default::Numbers(d),
        wired: true,
    }
}
const fn choice(name: &'static str, d: &'static str, c: &'static [&'static str]) -> Setting {
    Setting {
        name,
        kind: Kind::Choice(c),
        default: Default::Text(d),
        wired: false,
    }
}
const fn text(name: &'static str, d: &'static str) -> Setting {
    Setting {
        name,
        kind: Kind::Text,
        default: Default::Text(d),
        wired: false,
    }
}
const fn curve(name: &'static str, k: &'static [(f64, &'static [f64])]) -> Setting {
    Setting {
        name,
        kind: Kind::Curve,
        default: Default::Keys(k),
        wired: true,
    }
}
const fn gradient(name: &'static str, k: &'static [(f64, &'static [f64])]) -> Setting {
    Setting {
        name,
        kind: Kind::Gradient,
        default: Default::Keys(k),
        wired: true,
    }
}

const FADE_IN_OUT: &[(f64, &[f64])] = &[(0.0, &[0.0]), (0.2, &[1.0]), (1.0, &[0.0])];
const WHITE_FADE: &[(f64, &[f64])] = &[
    (0.0, &[1.0, 1.0, 1.0, 0.0]),
    (0.15, &[1.0, 1.0, 1.0, 1.0]),
    (1.0, &[1.0, 1.0, 1.0, 0.0]),
];
const FLAT: &[(f64, &[f64])] = &[(0.0, &[1.0]), (1.0, &[1.0])];
const LINEAR: &[(f64, &[f64])] = &[(0.0, &[0.0]), (1.0, &[1.0])];

/// The authoring nodes.
pub static NODES: &[FlowDef] = &[
    FlowDef {
        kind: "spawner",
        title: "Spawner",
        group: Group::Spawn,
        flow: Flow::Source,
        settings: &[
            choice("mode", "burst", &["burst", "stream", "event"]),
            range("life", &[1.0, 1.0]),
            number("count", 10.0),
            number("time", 0.0),
            number("rate", 20.0),
            number("delay", 0.0),
            number("duration", 0.0),
            text("layer", ""),
            text("event", ""),
            number("spray", 0.0),
            Setting {
                name: "rateCurve",
                kind: Kind::Curve,
                default: Default::Keys(FLAT),
                wired: false,
            },
            choice("ends", "life", &["life", "effect"]),
            number("fade", 0.0),
        ],
        help: "Births and lives: a burst of `count` at `time`; a stream of `rate` a second after `delay` for `duration` (0: while the effect plays), times `rateCurve` over that span (a spray that tapers off); or at another layer's Emit, one per event or, with `spray` seconds, a stream of `rate` a second from each event for that long. Each particle lives `life` seconds, or `ends` with the effect, fading out over `fade` seconds.",
    },
    FlowDef {
        kind: "shape",
        title: "Shape",
        group: Group::Birth,
        flow: Flow::Through,
        settings: &[
            choice(
                "shape",
                "sphere",
                &[
                    "point",
                    "sphere",
                    "disc",
                    "ring",
                    "cylinder",
                    "box",
                    "ellipsoid",
                ],
            ),
            number("radius", 1.0),
            number("inner", 0.0),
            number("height", 0.0),
            vector("size", &[1.0, 1.0, 1.0]),
            choice("fill", "volume", &["volume", "surface"]),
            vector("offset", &[0.0, 0.0, 0.0]),
            choice("follow", "none", &["none", "parent", "emitter", "target"]),
        ],
        help: "Where particles start: a point, inside or on a sphere, a disc, a ring, a cylinder, a box, or an ellipsoid (a sphere of `radius` scaled by `size`), at `offset` from the emitter (or the event); `follow` keeps them there as the parent particle (dying with it), the emitter or the target moves.",
    },
    FlowDef {
        kind: "launch",
        title: "Launch",
        group: Group::Birth,
        flow: Flow::Through,
        settings: &[
            choice(
                "direction",
                "outward",
                &["outward", "up", "axis", "cone", "random"],
            ),
            vector("axis", &[0.0, 1.0, 0.0]),
            number("angle", 30.0),
            range("speed", &[2.0, 4.0]),
            number("inherit", 0.0),
        ],
        help: "Start velocity: outward from the shape, up, along `axis`, in a cone of `angle` degrees around it, or random; `speed` metres a second; `inherit` the share of the parent's velocity.",
    },
    FlowDef {
        kind: "physics",
        title: "Physics",
        group: Group::Motion,
        flow: Flow::Through,
        settings: &[
            vector("gravity", &[0.0, 0.0, 0.0]),
            number("drag", 0.0),
            vector("wind", &[0.0, 0.0, 0.0]),
        ],
        help: "Gravity (metres a second², up is +y), drag (how fast velocity eases to the wind) and wind.",
    },
    FlowDef {
        kind: "turbulence",
        title: "Turbulence",
        group: Group::Motion,
        flow: Flow::Through,
        settings: &[
            number("strength", 4.0),
            number("scale", 2.0),
            number("speed", 1.0),
        ],
        help: "A swirling push: `strength` metres a second², whirls `scale` metres across, changing at `speed`.",
    },
    FlowDef {
        kind: "orbit",
        title: "Orbit",
        group: Group::Motion,
        flow: Flow::Through,
        settings: &[
            range("speed", &[90.0, 90.0]),
            vector("axis", &[0.0, 1.0, 0.0]),
        ],
        help: "Swirls particles around the emitter's `axis`, degrees a second (negative turns the other way).",
    },
    FlowDef {
        kind: "path",
        title: "Path",
        group: Group::Motion,
        flow: Flow::Through,
        settings: &[
            choice("from", "emitter", &["emitter", "caster", "target"]),
            vector("fromOffset", &[0.0, 0.0, 0.0]),
            choice("to", "target", &["emitter", "caster", "target"]),
            vector("toOffset", &[0.0, 0.0, 0.0]),
            curve("progress", LINEAR),
            number("arc", 0.0),
        ],
        help: "Moves particles from one place to another over their life: from and to the emitter, the caster or the target (each with an offset), along a progress curve, `arc` metres high in the middle. A falling star, a homing mote.",
    },
    FlowDef {
        kind: "beam",
        title: "Beam",
        group: Group::Motion,
        flow: Flow::Through,
        settings: &[
            number("amplitude", 0.0),
            number("waves", 1.0),
            number("speed", 1.0),
            number("jitter", 0.0),
        ],
        help: "Lays particles along the line from the caster to the target by their share of the batch, swaying sideways in `waves` waves of `amplitude` metres travelling at `speed`, flickering by `jitter` metres.",
    },
    FlowDef {
        kind: "bounce",
        title: "Bounce",
        group: Group::Motion,
        flow: Flow::Through,
        settings: &[
            number("restitution", 0.5),
            number("friction", 0.2),
            choice("onHit", "bounce", &["bounce", "die"]),
        ],
        help: "Particles meeting the ground bounce back up (`restitution` of their speed, `friction` lost along the ground) or die; an Emit after it can fire on each hit.",
    },
    FlowDef {
        kind: "sizeOverLife",
        title: "Size over life",
        group: Group::Appearance,
        flow: Flow::Through,
        settings: &[
            range("size", &[1.0, 1.0]),
            curve("curve", FLAT),
            number("stretch", 0.0),
        ],
        help: "Size in metres (full width) times a curve over life; `stretch` lengthens particles along their velocity (seconds of travel).",
    },
    FlowDef {
        kind: "colourOverLife",
        title: "Colour over life",
        group: Group::Appearance,
        flow: Flow::Through,
        settings: &[
            gradient("gradient", WHITE_FADE),
            range("brightness", &[1.0, 1.0]),
            vector("shift", &[0.0, 0.0, 0.0]),
            number("hue", 0.0),
            choice("team", "off", &["off", "on"]),
        ],
        help: "Colour and alpha over life (values above 1 glow), times a brightness per particle, shifted in hue, saturation and value; `hue` turns each particle's hue by a random amount up to that share of the colour wheel; `team` on tints it by the owner's player colour.",
    },
    FlowDef {
        kind: "spin",
        title: "Spin",
        group: Group::Appearance,
        flow: Flow::Through,
        settings: &[range("angle", &[0.0, 360.0]), range("speed", &[0.0, 0.0])],
        help: "Start angle and spin, degrees and degrees a second.",
    },
    FlowDef {
        kind: "flipbook",
        title: "Flipbook",
        group: Group::Appearance,
        flow: Flow::Through,
        settings: &[
            number("columns", 8.0),
            number("rows", 8.0),
            number("first", 0.0),
            number("last", 63.0),
            choice("play", "life", &["life", "rate", "random"]),
            number("loops", 1.0),
            number("fps", 30.0),
            choice("start", "first", &["first", "random"]),
        ],
        help: "Frames of an atlas: played over life (`loops` times), at `fps`, or one random frame.",
    },
    FlowDef {
        kind: "emit",
        title: "Emit",
        group: Group::Events,
        flow: Flow::Through,
        settings: &[
            choice(
                "when",
                "death",
                &[
                    "death", "birth", "interval", "distance", "step", "bounce", "end",
                ],
            ),
            number("interval", 0.1),
            range("count", &[1.0, 1.0]),
            number("chance", 1.0),
            Setting {
                name: "rateCurve",
                kind: Kind::Curve,
                default: Default::Keys(FLAT),
                wired: false,
            },
        ],
        help: "Fires an event other layers' Spawners listen to: when a particle dies or is born, every `interval` seconds or metres, every step, on a bounce, or once when the effect ends; `chance` per firing; `rateCurve` scales how often over the particle's life (a spray tapering off). Children start where it happened.",
    },
    FlowDef {
        kind: "billboard",
        title: "Billboard",
        group: Group::Draw,
        flow: Flow::Sink,
        settings: &[
            text("texture", ""),
            choice("blend", "additive", &["additive", "alpha", "distort"]),
            choice(
                "facing",
                "camera",
                &["camera", "ground", "axis", "velocity"],
            ),
            vector("axis", &[0.0, 1.0, 0.0]),
            number("soft", 0.0),
        ],
        help: "Textured sprites with the particles' size, colour, spin and frame: facing the camera, flat on the ground, along `axis` or along their velocity.",
    },
    FlowDef {
        kind: "ribbon",
        title: "Ribbon",
        group: Group::Draw,
        flow: Flow::Sink,
        settings: &[
            text("texture", ""),
            choice("blend", "additive", &["additive", "alpha"]),
            number("tiling", 1.0),
        ],
        help: "A strip through the particles in birth order (each parent's children a strip of their own), as wide as their size.",
    },
    // Values.
    FlowDef {
        kind: "randomRange",
        title: "Random range",
        group: Group::Values,
        flow: Flow::Value,
        settings: &[vector("min", &[0.0]), vector("max", &[1.0])],
        help: "A random value per particle between `min` and `max` (per component).",
    },
    FlowDef {
        kind: "curveOverLife",
        title: "Curve over life",
        group: Group::Values,
        flow: Flow::Value,
        settings: &[curve("curve", FADE_IN_OUT)],
        help: "A curve sampled by the particle's life.",
    },
    FlowDef {
        kind: "gradientOverLife",
        title: "Gradient over life",
        group: Group::Values,
        flow: Flow::Value,
        settings: &[gradient("gradient", WHITE_FADE)],
        help: "Colour and alpha over the particle's life.",
    },
    FlowDef {
        kind: "particleValue",
        title: "Particle value",
        group: Group::Values,
        flow: Flow::Value,
        settings: &[choice(
            "value",
            "life",
            &["life", "age", "speed", "height", "position", "velocity"],
        )],
        help: "One of the particle's own values: its life share (0 at birth, 1 at death), age in seconds, speed, height, position or velocity.",
    },
];

/// The authoring node `kind`, if there is one.
pub fn find(kind: &str) -> Option<&'static FlowDef> {
    NODES.iter().find(|n| n.kind == kind)
}

/// Whether `graph` is written in authoring nodes: it has a Spawner (no primitive is
/// called that; Billboard, Ribbon, Emit and Orbit share their names with primitives).
pub fn is_flow(graph: &Graph) -> bool {
    graph.nodes.iter().any(|n| n.kind == "spawner")
}

/// Whether every node and wire of a layer is an authoring one (D115): authoring kinds
/// only, particle wires from `out` to `in`, value wires from a value node to a setting
/// that takes one, settings the node has.
pub fn authored(graph: &Graph) -> Result<(), Vec<Problem>> {
    let mut problems = Vec::new();
    for node in &graph.nodes {
        let Some(def) = find(&node.kind) else {
            problems.push(problem(
                &node.id,
                &format!("{} is not an effect node", node.kind),
            ));
            continue;
        };
        for name in node.inputs.keys() {
            if !def.settings.iter().any(|s| s.name == name) {
                problems.push(problem(
                    &node.id,
                    &format!("{} has no setting {name}", def.title),
                ));
            }
        }
    }
    let kind = |id: &str| {
        graph
            .nodes
            .iter()
            .find(|n| n.id == id)
            .and_then(|n| find(&n.kind))
    };
    for wire in &graph.wires {
        let (Some((a, ap)), Some((b, bp))) = (Wire::split(&wire.from), Wire::split(&wire.to))
        else {
            continue;
        };
        let (Some(da), Some(db)) = (kind(a), kind(b)) else {
            problems.push(problem(b, "A wire to or from no node"));
            continue;
        };
        let fits = match (da.flow, ap) {
            (Flow::Value, "value") => db.settings.iter().any(|s| s.name == bp && s.wired),
            (Flow::Source | Flow::Through, "out") => {
                bp == "in" && matches!(db.flow, Flow::Through | Flow::Sink)
            }
            _ => false,
        };
        if !fits {
            problems.push(problem(b, &format!("{a}.{ap} cannot feed {b}.{bp}")));
        }
    }
    if problems.is_empty() {
        Ok(())
    } else {
        Err(problems)
    }
}

/// What a hidden sprayer is called: the layer it streams for, and the event it fires.
pub const SPRAY: &str = "~spray";

/// The compiled layers an authoring layer makes, in the order they are cooked: itself, or,
/// for a Spawner born of events with `spray` seconds, a hidden sprayer first (born of each
/// event, living `spray` seconds and firing `rate` a second along `rateCurve`) and the layer
/// listening to it instead. The sprayer is `<id>~spray`.
pub fn parts(id: &str, graph: &Graph) -> Vec<(String, Graph)> {
    let spawner = graph.nodes.iter().find(|n| n.kind == "spawner");
    let text = |n: &Node, name: &str| match n.inputs.get(name) {
        Some(Value::Text(t)) => t.clone(),
        _ => String::new(),
    };
    let number = |n: &Node, name: &str, d: f64| match n.inputs.get(name) {
        Some(Value::Number(x)) => *x,
        Some(Value::Vector(v)) => v.first().copied().unwrap_or(d),
        _ => d,
    };
    let Some(spawner) =
        spawner.filter(|n| text(n, "mode") == "event" && number(n, "spray", 0.0) > 0.0)
    else {
        return vec![(id.to_string(), graph.clone())];
    };
    let spray = number(spawner, "spray", 0.0);
    let rate = number(spawner, "rate", 20.0).max(1e-3);
    let make = |id: &str, kind: &str, inputs: Vec<(&str, Value)>| Node {
        id: id.into(),
        kind: kind.into(),
        name: None,
        at: [0.0, 0.0],
        inputs: inputs
            .into_iter()
            .map(|(k, v)| (k.to_string(), v))
            .collect(),
        at_birth: false,
        collapsed: false,
    };
    let mut emit = vec![
        ("when", Value::Text("interval".into())),
        ("interval", Value::Number(1.0 / rate)),
    ];
    if let Some(c) = spawner.inputs.get("rateCurve") {
        emit.push(("rateCurve", c.clone()));
    }
    let sprayer = Graph {
        nodes: vec![
            make(
                "spawner",
                "spawner",
                vec![
                    ("mode", Value::Text("event".into())),
                    ("layer", Value::Text(text(spawner, "layer"))),
                    ("event", Value::Text(text(spawner, "event"))),
                    ("life", Value::Vector(vec![spray, spray])),
                ],
            ),
            make(
                "shape",
                "shape",
                vec![("shape", Value::Text("point".into()))],
            ),
            make("spray", "emit", emit),
        ],
        wires: vec![
            Wire {
                from: "spawner.out".into(),
                to: "shape.in".into(),
            },
            Wire {
                from: "shape.out".into(),
                to: "spray.in".into(),
            },
        ],
        backdrops: Vec::new(),
    };
    let hidden = format!("{id}{SPRAY}");
    let mut layer = graph.clone();
    if let Some(n) = layer.nodes.iter_mut().find(|n| n.kind == "spawner") {
        n.inputs.insert("layer".into(), Value::Text(hidden.clone()));
        n.inputs.insert("event".into(), Value::Text("spray".into()));
        n.inputs.remove("spray");
    }
    vec![(hidden, sprayer), (id.to_string(), layer)]
}

/// The layer's nodes in the order things happen to the particles: the Spawner, then along
/// the particle wires (branches one after another), then nodes on no chain. Value nodes
/// are left out ([`driven`] says where each goes).
pub fn chain(graph: &Graph) -> Vec<&Node> {
    let flow_node = |n: &&Node| find(&n.kind).is_some_and(|d| d.flow != Flow::Value);
    let mut order: Vec<&Node> = Vec::new();
    let mut stack: Vec<&str> = graph
        .nodes
        .iter()
        .filter(|n| n.kind == "spawner")
        .map(|n| n.id.as_str())
        .collect();
    stack.reverse();
    while let Some(id) = stack.pop() {
        if order.iter().any(|n| n.id == id) {
            continue;
        }
        let Some(node) = graph.nodes.iter().find(|n| n.id == id) else {
            continue;
        };
        order.push(node);
        for n in next(graph, id).into_iter().rev() {
            stack.push(n);
        }
    }
    for node in graph.nodes.iter().filter(flow_node) {
        if !order.iter().any(|n| n.id == node.id) {
            order.push(node);
        }
    }
    order
}

/// The node and setting value node `id` drives, if it is wired.
pub fn driven<'a>(graph: &'a Graph, id: &str) -> Option<(&'a str, &'a str)> {
    let from = format!("{id}.value");
    graph
        .wires
        .iter()
        .find(|w| w.from == from)
        .and_then(|w| Wire::split(&w.to))
}

/// A value source in the expansion: numbers set on a primitive, or a primitive's output.
#[derive(Debug, Clone, PartialEq)]
enum Val {
    Lit(Vec<f64>),
    Out(String),
}

/// What the chain has set so far, handed down to the renderers.
#[derive(Debug, Clone, Default)]
struct Look {
    size: Option<Val>,
    colour: Option<Val>,
    rotation: Option<Val>,
    frame: Option<Val>,
    flipbook: Option<[f64; 2]>,
    stretch: Option<Val>,
    /// The Shape's sample before its offset (Launch's "outward").
    shape: Option<Val>,
    /// The start position before an event's (Follow's offset).
    birth: Option<Val>,
    /// A burst's count (Beam's share for particles not born of events).
    count: Option<f64>,
    /// Bounce's hit this step (Emit "bounce").
    hit: Option<Val>,
    /// Born of another layer's events.
    event_born: bool,
    /// What is left of the particle while it fades out with the effect (0–1).
    keep: Option<Val>,
}

struct Out<'g> {
    graph: &'g Graph,
    nodes: Vec<Node>,
    wires: Vec<Wire>,
    values: BTreeMap<String, Val>,
    problems: Vec<Problem>,
    serial: usize,
}

impl<'g> Out<'g> {
    fn prim(&mut self, owner: &str, kind: &str, inputs: &[(&str, Val)]) -> String {
        self.serial += 1;
        let id = format!("{owner}~{}{}", kind, self.serial);
        self.nodes.push(Node {
            id: id.clone(),
            kind: kind.into(),
            name: None,
            at: [0.0, 0.0],
            inputs: BTreeMap::new(),
            at_birth: false,
            collapsed: false,
        });
        for (pin, v) in inputs {
            self.feed(&id, pin, v.clone());
        }
        id
    }

    /// A primitive with a fixed id (Emit: its children name it).
    fn named(&mut self, id: &str, kind: &str, inputs: &[(&str, Val)]) -> String {
        self.nodes.push(Node {
            id: id.into(),
            kind: kind.into(),
            name: None,
            at: [0.0, 0.0],
            inputs: BTreeMap::new(),
            at_birth: false,
            collapsed: false,
        });
        for (pin, v) in inputs {
            self.feed(id, pin, v.clone());
        }
        id.into()
    }

    fn text(&mut self, id: &str, pin: &str, text: &str) {
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            n.inputs.insert(pin.into(), Value::Text(text.into()));
        }
    }

    fn feed(&mut self, id: &str, pin: &str, v: Val) {
        match v {
            Val::Out(from) => self.wires.push(Wire {
                from,
                to: format!("{id}.{pin}"),
            }),
            Val::Lit(n) => {
                let value = if n.len() == 1 {
                    Value::Number(n[0])
                } else {
                    Value::Vector(n)
                };
                if let Some(node) = self.nodes.iter_mut().find(|x| x.id == id) {
                    node.inputs.insert(pin.into(), value);
                }
            }
        }
    }

    fn out(&mut self, owner: &str, kind: &str, inputs: &[(&str, Val)]) -> Val {
        let id = self.prim(owner, kind, inputs);
        let pin = match kind {
            "sphere" | "cylinder" | "box" | "ellipsoid" | "emitter" => "position",
            _ => "value",
        };
        Val::Out(format!("{id}.{pin}"))
    }

    fn mul(&mut self, owner: &str, a: Val, b: Val) -> Val {
        match (&a, &b) {
            (Val::Lit(x), _) if x.iter().all(|&v| v == 1.0) => return b,
            (_, Val::Lit(y)) if y.iter().all(|&v| v == 1.0) => return a,
            _ => {}
        }
        self.out(owner, "multiply", &[("a", a), ("b", b)])
    }

    fn add(&mut self, owner: &str, a: Val, b: Val) -> Val {
        match (&a, &b) {
            (Val::Lit(x), _) if x.iter().all(|&v| v == 0.0) => return b,
            (_, Val::Lit(y)) if y.iter().all(|&v| v == 0.0) => return a,
            _ => {}
        }
        self.out(owner, "add", &[("a", a), ("b", b)])
    }

    fn setting(&self, node: &Node, name: &str) -> Option<&'g Value> {
        let n = self.graph.nodes.iter().find(|n| n.id == node.id)?;
        n.inputs.get(name)
    }

    fn wired(&self, node: &Node, pin: &str) -> Option<String> {
        let to = format!("{}.{pin}", node.id);
        self.graph
            .wires
            .iter()
            .find(|w| w.to == to)
            .map(|w| w.from.clone())
    }

    fn numbers(&self, node: &Node, name: &str) -> Vec<f64> {
        match self.setting(node, name) {
            Some(Value::Number(n)) => vec![*n],
            Some(Value::Vector(v)) => v.clone(),
            _ => {
                let def = find(&node.kind).and_then(|d| d.settings.iter().find(|s| s.name == name));
                match def.map(|s| s.default) {
                    Some(Default::Number(n)) => vec![n],
                    Some(Default::Numbers(n)) => n.to_vec(),
                    _ => vec![0.0],
                }
            }
        }
    }

    fn number(&self, node: &Node, name: &str) -> f64 {
        self.numbers(node, name).first().copied().unwrap_or(0.0)
    }

    fn choice(&self, node: &Node, name: &str) -> String {
        match self.setting(node, name) {
            Some(Value::Text(t)) => t.clone(),
            _ => find(&node.kind)
                .and_then(|d| d.settings.iter().find(|s| s.name == name))
                .and_then(|s| match s.default {
                    Default::Text(t) => Some(t.to_owned()),
                    _ => None,
                })
                .unwrap_or_default(),
        }
    }

    fn keys(&self, node: &Node, name: &str) -> Curve {
        if let Some(Value::Curve(c)) = self.setting(node, name) {
            return c.clone();
        }
        let keys = find(&node.kind)
            .and_then(|d| d.settings.iter().find(|s| s.name == name))
            .and_then(|s| match s.default {
                Default::Keys(k) => Some(k),
                _ => None,
            })
            .unwrap_or(FLAT);
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

    /// A value node's output (expanded once).
    fn value_node(&mut self, end: &str) -> Option<Val> {
        let (id, _) = Wire::split(end)?;
        if let Some(v) = self.values.get(id) {
            return Some(v.clone());
        }
        let node = self.graph.nodes.iter().find(|n| n.id == id)?.clone();
        let v = match node.kind.as_str() {
            "randomRange" => {
                let (min, max) = (self.numbers(&node, "min"), self.numbers(&node, "max"));
                self.out(
                    &node.id,
                    "random",
                    &[("min", Val::Lit(min)), ("max", Val::Lit(max))],
                )
            }
            "curveOverLife" | "gradientOverLife" => {
                let name = if node.kind == "curveOverLife" {
                    "curve"
                } else {
                    "gradient"
                };
                let c = self.keys(&node, name);
                let id = self.prim(&node.id, "curve", &[]);
                if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
                    n.inputs.insert("curve".into(), Value::Curve(c));
                }
                Val::Out(format!("{id}.value"))
            }
            "particleValue" => match self.choice(&node, "value").as_str() {
                "age" => self.out(&node.id, "age", &[]),
                "speed" => {
                    let v = self.out(&node.id, "velocity", &[]);
                    self.out(&node.id, "length", &[("x", v)])
                }
                "height" => {
                    let p = self.out(&node.id, "position", &[]);
                    let id = self.prim(&node.id, "split", &[("value", p)]);
                    Val::Out(format!("{id}.y"))
                }
                "position" => self.out(&node.id, "position", &[]),
                "velocity" => self.out(&node.id, "velocity", &[]),
                _ => self.out(&node.id, "lifeRatio", &[]),
            },
            other => {
                self.problems
                    .push(problem(&node.id, &format!("{other} is not a value node")));
                return None;
            }
        };
        self.values.insert(id.to_owned(), v.clone());
        Some(v)
    }

    /// A setting as a value: its wire, else its range (a random roll when min ≠ max).
    fn range(&mut self, node: &Node, name: &str) -> Val {
        if let Some(from) = self.wired(node, name)
            && let Some(v) = self.value_node(&from)
        {
            return v;
        }
        let n = self.numbers(node, name);
        match n[..] {
            [min, max] if min != max => self.out(
                &node.id,
                "random",
                &[("min", Val::Lit(vec![min])), ("max", Val::Lit(vec![max]))],
            ),
            [v, ..] => Val::Lit(vec![v]),
            [] => Val::Lit(vec![0.0]),
        }
    }

    /// A vector or curve setting as a value: its wire, else its numbers.
    fn plain(&mut self, node: &Node, name: &str) -> Val {
        if let Some(from) = self.wired(node, name)
            && let Some(v) = self.value_node(&from)
        {
            return v;
        }
        Val::Lit(self.numbers(node, name))
    }

    /// A curve or gradient setting sampled by life: its wire, else the curve.
    fn over_life(&mut self, node: &Node, name: &str) -> Val {
        if let Some(from) = self.wired(node, name)
            && let Some(v) = self.value_node(&from)
        {
            return v;
        }
        let c = self.keys(node, name);
        let id = self.prim(&node.id, "curve", &[]);
        if let Some(n) = self.nodes.iter_mut().find(|n| n.id == id) {
            n.inputs.insert("curve".into(), Value::Curve(c));
        }
        Val::Out(format!("{id}.value"))
    }
}

fn problem(node: &str, message: &str) -> Problem {
    Problem {
        node: (!node.is_empty()).then(|| node.to_owned()),
        pin: None,
        message: message.into(),
    }
}

/// The node after `id` on the flow (its `out` wired to their `in`).
fn next<'a>(graph: &'a Graph, id: &str) -> Vec<&'a str> {
    let from = format!("{id}.out");
    graph
        .wires
        .iter()
        .filter(|w| w.from == from)
        .filter_map(|w| {
            Wire::split(&w.to)
                .filter(|(_, p)| *p == "in")
                .map(|(n, _)| n)
        })
        .collect()
}

/// The layer in primitives (see the module docs). With `until`, the chain stops after that
/// node and draws what it has as small white dots (the node's preview).
pub fn expand(graph: &Graph, until: Option<&str>) -> Result<Graph, Vec<Problem>> {
    let mut out = Out {
        graph,
        nodes: Vec::new(),
        wires: Vec::new(),
        values: BTreeMap::new(),
        problems: Vec::new(),
        serial: 0,
    };
    let sources: Vec<&Node> = graph
        .nodes
        .iter()
        .filter(|n| find(&n.kind).is_some_and(|d| d.flow == Flow::Source))
        .collect();
    let [source] = sources[..] else {
        return Err(vec![problem("", "A layer needs exactly one Spawner")]);
    };
    for n in &graph.nodes {
        if find(&n.kind).is_none() {
            out.problems.push(problem(
                &n.id,
                &format!("{} is not an authoring node", n.kind),
            ));
        }
    }
    // Walk the chain depth first; each node once.
    let mut stack: Vec<(String, Look)> = vec![(source.id.clone(), Look::default())];
    let mut seen: Vec<String> = Vec::new();
    while let Some((id, mut look)) = stack.pop() {
        if seen.contains(&id) {
            continue;
        }
        seen.push(id.clone());
        let node = graph
            .nodes
            .iter()
            .find(|n| n.id == id)
            .expect("wired node")
            .clone();
        step(&mut out, &node, &mut look);
        let stop = until == Some(id.as_str());
        let after = if stop { Vec::new() } else { next(graph, &id) };
        let open = after.is_empty() && find(&node.kind).is_some_and(|d| d.flow != Flow::Sink);
        if stop || (open && until.is_some()) {
            // A preview, or a chain that ends without a renderer previewed: draw what it
            // has. In the effect such a layer draws nothing (it only emits).
            draw(&mut out, &node.id, &look, None);
        }
        for n in after.into_iter().rev() {
            stack.push((n.to_owned(), look.clone()));
        }
    }
    if !out.problems.is_empty() {
        return Err(out.problems);
    }
    Ok(Graph {
        nodes: out.nodes,
        wires: out.wires,
        backdrops: Vec::new(),
    })
}

fn step(out: &mut Out, node: &Node, look: &mut Look) {
    let id = node.id.as_str();
    match node.kind.as_str() {
        "spawner" => {
            match out.choice(node, "mode").as_str() {
                "stream" => {
                    let (rate, delay, duration) = (
                        out.number(node, "rate"),
                        out.number(node, "delay"),
                        out.number(node, "duration"),
                    );
                    let p = out.prim(
                        id,
                        "rate",
                        &[
                            ("rate", Val::Lit(vec![rate])),
                            ("start", Val::Lit(vec![delay])),
                            ("duration", Val::Lit(vec![duration])),
                        ],
                    );
                    if out.setting(node, "rateCurve").is_some() {
                        let c = out.keys(node, "rateCurve");
                        if let Some(n) = out.nodes.iter_mut().find(|n| n.id == p) {
                            n.inputs.insert("curve".into(), Value::Curve(c));
                        }
                    }
                }
                "event" => {
                    let (layer, event) = (out.choice(node, "layer"), out.choice(node, "event"));
                    let p = out.prim(id, "onEvent", &[]);
                    out.text(&p, "layer", &layer);
                    out.text(&p, "event", &event);
                    look.event_born = true;
                }
                _ => {
                    let (count, time) = (out.number(node, "count"), out.number(node, "time"));
                    out.prim(
                        id,
                        "burst",
                        &[
                            ("count", Val::Lit(vec![count])),
                            ("time", Val::Lit(vec![time])),
                        ],
                    );
                    look.count = Some(count);
                }
            }
            let life = out.range(node, "life");
            out.prim(id, "setLife", &[("life", life)]);
            if out.choice(node, "ends") == "effect" {
                let fade = out.number(node, "fade");
                ends(out, id, fade, look);
            }
        }
        "shape" => {
            let (radius, inner, height) = (
                out.number(node, "radius"),
                out.number(node, "inner"),
                out.number(node, "height"),
            );
            let surface = out.choice(node, "fill") == "surface";
            let inner = if surface { 1.0 } else { inner };
            let r = |v: f64| Val::Lit(vec![v]);
            let sample = match out.choice(node, "shape").as_str() {
                "point" => None,
                "disc" => Some(out.out(
                    id,
                    "cylinder",
                    &[
                        ("radius", r(radius)),
                        ("height", r(0.0)),
                        ("inner", r(inner)),
                    ],
                )),
                "ring" => Some(out.out(
                    id,
                    "cylinder",
                    &[
                        ("radius", r(radius)),
                        ("height", r(height)),
                        ("inner", r(inner.max(0.95))),
                    ],
                )),
                "cylinder" => Some(out.out(
                    id,
                    "cylinder",
                    &[
                        ("radius", r(radius)),
                        ("height", r(height)),
                        ("inner", r(inner)),
                    ],
                )),
                "box" => {
                    let size = out.numbers(node, "size");
                    Some(out.out(id, "box", &[("size", Val::Lit(size))]))
                }
                "ellipsoid" => {
                    let size = out.numbers(node, "size");
                    let radii = (0..3)
                        .map(|i| radius * size.get(i).copied().unwrap_or(1.0))
                        .collect();
                    Some(out.out(
                        id,
                        "ellipsoid",
                        &[("radius", Val::Lit(radii)), ("inner", r(inner))],
                    ))
                }
                _ => Some(out.out(id, "sphere", &[("radius", r(radius)), ("inner", r(inner))])),
            };
            let offset = out.plain(node, "offset");
            let mut at = match &sample {
                Some(s) => out.add(id, s.clone(), offset),
                None => offset,
            };
            // Births happen where the emitter is now (a missile's head as it flies; the
            // effect's origin otherwise), or where the event happened.
            let base = if look.event_born {
                let e = out.prim(id, "event", &[]);
                Val::Out(format!("{e}.position"))
            } else {
                out.out(id, "emitter", &[])
            };
            at = out.add(id, at, base);
            let offset = out.plain(node, "offset");
            look.birth = Some(match &sample {
                Some(s) => out.add(id, s.clone(), offset),
                None => offset,
            });
            look.shape = sample;
            out.prim(id, "setPosition", &[("position", at)]);
            let with = out.choice(node, "follow");
            if with != "none" {
                follow(out, id, &with, look);
            }
        }
        "launch" => {
            let axis = out.numbers(node, "axis");
            let dir = match out.choice(node, "direction").as_str() {
                "up" => Val::Lit(vec![0.0, 1.0, 0.0]),
                "axis" => Val::Lit(normalized(&axis)),
                "random" => out.out(id, "randomDirection", &[]),
                "cone" => {
                    let spread = (out.number(node, "angle").to_radians() * 0.5).tan();
                    let d = out.out(id, "randomDirection", &[]);
                    let d = out.mul(id, d, Val::Lit(vec![spread]));
                    let d = out.add(id, d, Val::Lit(normalized(&axis)));
                    out.out(id, "normalize", &[("x", d)])
                }
                _ => match look.shape.clone() {
                    Some(s) => out.out(id, "normalize", &[("x", s)]),
                    None => out.out(id, "randomDirection", &[]),
                },
            };
            let speed = out.range(node, "speed");
            let mut v = out.mul(id, dir, speed);
            let inherit = out.number(node, "inherit");
            if look.event_born && inherit != 0.0 {
                let e = out.prim(id, "event", &[]);
                let pv = out.mul(
                    id,
                    Val::Out(format!("{e}.velocity")),
                    Val::Lit(vec![inherit]),
                );
                v = out.add(id, v, pv);
            }
            out.prim(id, "setVelocity", &[("velocity", v)]);
        }
        "physics" => {
            let (gravity, wind, drag) = (
                out.numbers(node, "gravity"),
                out.numbers(node, "wind"),
                out.number(node, "drag"),
            );
            // Drag eases velocity to the wind: dv = −k(v − w) = −kv + kw.
            let push: Vec<f64> = (0..3)
                .map(|i| {
                    gravity.get(i).copied().unwrap_or(0.0)
                        + drag * wind.get(i).copied().unwrap_or(0.0)
                })
                .collect();
            if push.iter().any(|&x| x != 0.0) {
                out.prim(id, "acceleration", &[("acceleration", Val::Lit(push))]);
            }
            if drag > 0.0 {
                out.prim(id, "drag", &[("strength", Val::Lit(vec![drag]))]);
            }
        }
        "turbulence" => {
            let (strength, scale, speed) = (
                out.number(node, "strength"),
                out.number(node, "scale"),
                out.number(node, "speed"),
            );
            let p = out.out(id, "position", &[]);
            let n = out.out(
                id,
                "noise",
                &[
                    ("position", p),
                    ("scale", Val::Lit(vec![1.0 / scale.max(0.01)])),
                    ("speed", Val::Lit(vec![speed])),
                ],
            );
            let a = out.mul(id, n, Val::Lit(vec![strength]));
            out.prim(id, "acceleration", &[("acceleration", a)]);
        }
        "orbit" => {
            let speed = out.range(node, "speed");
            let speed = out.mul(id, speed, Val::Lit(vec![std::f64::consts::PI / 180.0]));
            let axis = out.plain(node, "axis");
            out.prim(id, "orbit", &[("speed", speed), ("axis", axis)]);
        }
        "path" => {
            let from = out.choice(node, "from");
            let a = anchor(out, id, &from);
            let a_off = out.plain(node, "fromOffset");
            let a = out.add(id, a, a_off);
            let to = out.choice(node, "to");
            let b = anchor(out, id, &to);
            let b_off = out.plain(node, "toOffset");
            let b = out.add(id, b, b_off);
            let t = out.over_life(node, "progress");
            let mut at = out.out(id, "lerp", &[("a", a), ("b", b), ("t", t.clone())]);
            let arc = out.number(node, "arc");
            if arc != 0.0 {
                let rest = out.out(
                    id,
                    "subtract",
                    &[("a", Val::Lit(vec![1.0])), ("b", t.clone())],
                );
                let bump = out.mul(id, t, rest);
                let bump = out.mul(id, bump, Val::Lit(vec![4.0 * arc]));
                let zero = || Val::Lit(vec![0.0]);
                let lift = out.out(
                    id,
                    "makeVector",
                    &[("x", zero()), ("y", bump), ("z", zero())],
                );
                at = out.add(id, at, lift);
            }
            out.prim(id, "setPosition", &[("position", at)]);
        }
        "beam" => {
            let share = if look.event_born {
                let e = out.prim(id, "event", &[]);
                Val::Out(format!("{e}.share"))
            } else {
                let i = out.out(id, "spawnIndex", &[]);
                let n = (look.count.unwrap_or(2.0) - 1.0).max(1.0);
                out.mul(id, i, Val::Lit(vec![1.0 / n]))
            };
            let t = out.prim(id, "target", &[]);
            let (a, b) = (
                Val::Out(format!("{t}.source")),
                Val::Out(format!("{t}.position")),
            );
            let mut at = out.out(
                id,
                "lerp",
                &[("a", a.clone()), ("b", b.clone()), ("t", share.clone())],
            );
            let amplitude = out.number(node, "amplitude");
            if amplitude != 0.0 {
                let line = out.out(id, "subtract", &[("a", b), ("b", a)]);
                let side = out.out(
                    id,
                    "cross",
                    &[("a", line), ("b", Val::Lit(vec![0.0, 1.0, 0.0]))],
                );
                let side = out.out(id, "normalize", &[("x", side)]);
                let tau = std::f64::consts::TAU;
                let (waves, speed) = (out.number(node, "waves"), out.number(node, "speed"));
                let along = out.mul(id, share, Val::Lit(vec![waves * tau]));
                let time = out.out(id, "effectTime", &[]);
                let time = out.mul(id, time, Val::Lit(vec![speed * tau]));
                let phase = out.add(id, along, time);
                let wave = out.out(id, "sin", &[("x", phase)]);
                let wave = out.mul(id, wave, Val::Lit(vec![amplitude]));
                let sway = out.mul(id, side, wave);
                at = out.add(id, at, sway);
            }
            let jitter = out.number(node, "jitter");
            if jitter != 0.0 {
                let p = out.out(id, "position", &[]);
                let n = out.out(
                    id,
                    "noise",
                    &[
                        ("position", p),
                        ("scale", Val::Lit(vec![1.0])),
                        ("speed", Val::Lit(vec![6.0])),
                    ],
                );
                let n = out.mul(id, n, Val::Lit(vec![jitter]));
                at = out.add(id, at, n);
            }
            out.prim(id, "setPosition", &[("position", at)]);
        }
        "bounce" => {
            let (restitution, friction) = (
                out.number(node, "restitution"),
                out.number(node, "friction"),
            );
            let p = out.out(id, "position", &[]);
            let v = out.out(id, "velocity", &[]);
            let ps = out.prim(id, "split", &[("value", p.clone())]);
            let vs = out.prim(id, "split", &[("value", v.clone())]);
            let pin = |n: &str, c: &str| Val::Out(format!("{n}.{c}"));
            let below = out.prim(
                id,
                "compare",
                &[("a", pin(&ps, "y")), ("b", Val::Lit(vec![0.0]))],
            );
            out.text(&below, "op", "<");
            let falling = out.prim(
                id,
                "compare",
                &[("a", pin(&vs, "y")), ("b", Val::Lit(vec![0.0]))],
            );
            out.text(&falling, "op", "<");
            let hit = out.out(
                id,
                "and",
                &[("a", pin(&below, "value")), ("b", pin(&falling, "value"))],
            );
            if out.choice(node, "onHit") == "die" {
                out.prim(id, "kill", &[("condition", hit.clone())]);
            } else {
                let keep = 1.0 - friction;
                let vx = out.mul(id, pin(&vs, "x"), Val::Lit(vec![keep]));
                let vy = out.mul(id, pin(&vs, "y"), Val::Lit(vec![-restitution]));
                let vz = out.mul(id, pin(&vs, "z"), Val::Lit(vec![keep]));
                let bounced = out.out(id, "makeVector", &[("x", vx), ("y", vy), ("z", vz)]);
                let nv = out.out(
                    id,
                    "select",
                    &[("condition", hit.clone()), ("a", bounced), ("b", v)],
                );
                out.prim(id, "setVelocity", &[("velocity", nv)]);
                let ground = out.out(
                    id,
                    "makeVector",
                    &[
                        ("x", pin(&ps, "x")),
                        ("y", Val::Lit(vec![0.0])),
                        ("z", pin(&ps, "z")),
                    ],
                );
                let np = out.out(
                    id,
                    "select",
                    &[("condition", hit.clone()), ("a", ground), ("b", p)],
                );
                out.prim(id, "setPosition", &[("position", np)]);
            }
            look.hit = Some(hit);
        }
        "sizeOverLife" => {
            let size = out.range(node, "size");
            let curve = out.over_life(node, "curve");
            look.size = Some(out.mul(id, size, curve));
            let stretch = out.number(node, "stretch");
            look.stretch = (stretch > 0.0).then(|| Val::Lit(vec![stretch]));
        }
        "colourOverLife" => {
            let gradient = out.over_life(node, "gradient");
            let brightness = out.range(node, "brightness");
            let mut c = out.mul(id, gradient, brightness);
            if out.choice(node, "team") == "on" {
                let team = out.out(id, "teamColour", &[]);
                c = out.mul(id, c, team);
            }
            let mut shift = out.numbers(node, "shift");
            shift.resize(3, 0.0);
            let hue = out.number(node, "hue").abs();
            if hue > 0.0 {
                // Each particle's hue turned by up to `hue` either way.
                let (mut lo, mut hi) = (shift.clone(), shift.clone());
                lo[0] -= hue;
                hi[0] += hue;
                let roll = out.out(
                    id,
                    "random",
                    &[("min", Val::Lit(lo)), ("max", Val::Lit(hi))],
                );
                c = out.out(id, "hsvShift", &[("colour", c), ("shift", roll)]);
            } else if shift.iter().any(|&x| x != 0.0) {
                c = out.out(id, "hsvShift", &[("colour", c), ("shift", Val::Lit(shift))]);
            }
            look.colour = Some(c);
        }
        "spin" => {
            let angle = out.range(node, "angle");
            let speed = out.range(node, "speed");
            let rot = if speed == Val::Lit(vec![0.0]) {
                angle
            } else {
                let age = out.out(id, "age", &[]);
                let turn = out.mul(id, age, speed);
                out.add(id, angle, turn)
            };
            look.rotation = Some(rot);
        }
        "flipbook" => {
            let (columns, rows) = (
                out.number(node, "columns").max(1.0),
                out.number(node, "rows").max(1.0),
            );
            let (first, last) = (out.number(node, "first"), out.number(node, "last"));
            let count = (last - first + 1.0).max(1.0);
            let start = if out.choice(node, "start") == "random" {
                out.out(
                    id,
                    "random",
                    &[("min", Val::Lit(vec![0.0])), ("max", Val::Lit(vec![1.0]))],
                )
            } else {
                Val::Lit(vec![0.0])
            };
            let frame = match out.choice(node, "play").as_str() {
                "random" => {
                    let r = out.out(
                        id,
                        "random",
                        &[
                            ("min", Val::Lit(vec![0.0])),
                            ("max", Val::Lit(vec![count - 0.001])),
                        ],
                    );
                    out.add(id, r, Val::Lit(vec![first]))
                }
                play => {
                    let (clock, per) = if play == "rate" {
                        (out.out(id, "age", &[]), out.number(node, "fps") / count)
                    } else {
                        (out.out(id, "lifeRatio", &[]), out.number(node, "loops"))
                    };
                    let t = out.mul(id, clock, Val::Lit(vec![per]));
                    let t = out.add(id, t, start);
                    let t = out.out(id, "fract", &[("x", t)]);
                    let t = out.mul(id, t, Val::Lit(vec![count]));
                    out.add(id, t, Val::Lit(vec![first]))
                }
            };
            look.frame = Some(frame);
            look.flipbook = Some([columns, rows]);
        }
        "emit" => {
            let interval = out.number(node, "interval");
            let count = out.range(node, "count");
            let p = out.named(
                id,
                "emit",
                &[("interval", Val::Lit(vec![interval])), ("count", count)],
            );
            if out.setting(node, "rateCurve").is_some() {
                // Faster or slower over the particle's life: the interval divided by it.
                let c = out.keys(node, "rateCurve");
                let curve = out.prim(id, "curve", &[]);
                if let Some(n) = out.nodes.iter_mut().find(|n| n.id == curve) {
                    n.inputs.insert("curve".into(), Value::Curve(c));
                }
                let k = out.out(
                    id,
                    "max",
                    &[
                        ("a", Val::Out(format!("{curve}.value"))),
                        ("b", Val::Lit(vec![1e-4])),
                    ],
                );
                let every = out.out(id, "divide", &[("a", Val::Lit(vec![interval])), ("b", k)]);
                out.feed(&p, "interval", every);
            }
            let mut when = out.choice(node, "when");
            let mut gate = None;
            if when == "bounce" {
                when = "step".into();
                gate = look.hit.clone();
                if gate.is_none() {
                    out.problems
                        .push(problem(id, "Emit on bounce needs a Bounce before it"));
                }
            }
            if when == "end" {
                // Once, at the first step after the effect stops: a flag on the particle.
                when = "step".into();
                let name = format!(
                    "ended_{}",
                    id.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
                );
                let running = out.out(id, "effectRunning", &[]);
                let stopped = out.out(id, "not", &[("x", running)]);
                let flag = out.prim(id, "attribute", &[]);
                out.text(&flag, "name", &name);
                let fresh = out.prim(
                    id,
                    "compare",
                    &[
                        ("a", Val::Out(format!("{flag}.x"))),
                        ("b", Val::Lit(vec![0.5])),
                    ],
                );
                out.text(&fresh, "op", "<");
                let mark = out.out(
                    id,
                    "select",
                    &[
                        ("condition", stopped.clone()),
                        ("a", Val::Lit(vec![1.0])),
                        ("b", Val::Lit(vec![0.0])),
                    ],
                );
                let set = out.prim(id, "setAttribute", &[("value", mark)]);
                out.text(&set, "name", &name);
                gate = Some(out.out(
                    id,
                    "and",
                    &[("a", stopped), ("b", Val::Out(format!("{fresh}.value")))],
                ));
            }
            out.text(&p, "when", &when);
            if let Some(c) = look.colour.clone() {
                out.feed(&p, "colour", c);
            }
            let chance = out.number(node, "chance");
            if chance < 1.0 {
                let r = out.out(
                    id,
                    "random",
                    &[("min", Val::Lit(vec![0.0])), ("max", Val::Lit(vec![1.0]))],
                );
                let c = out.prim(id, "compare", &[("a", r), ("b", Val::Lit(vec![chance]))]);
                out.text(&c, "op", "<");
                let lucky = Val::Out(format!("{c}.value"));
                gate = Some(match gate {
                    Some(g) => out.out(id, "and", &[("a", g), ("b", lucky)]),
                    None => lucky,
                });
            }
            if let Some(g) = gate {
                out.feed(&p, "condition", g);
            }
        }
        "billboard" | "ribbon" => draw(out, id, look, Some(node)),
        _ => {}
    }
}

/// Keeps particles at their start offset from `with` (the parent particle, dying with it;
/// the emitter; or the target) as it moves.
fn follow(out: &mut Out, id: &str, with: &str, look: &Look) {
    let base = match with {
        "emitter" => out.out(id, "emitter", &[]),
        "target" => {
            let t = out.prim(id, "target", &[]);
            Val::Out(format!("{t}.position"))
        }
        _ => {
            let p = out.prim(id, "parent", &[]);
            let gone = out.out(id, "not", &[("x", Val::Out(format!("{p}.alive")))]);
            out.prim(id, "kill", &[("condition", gone)]);
            Val::Out(format!("{p}.position"))
        }
    };
    let offset = look.birth.clone().unwrap_or(Val::Lit(vec![0.0, 0.0, 0.0]));
    let at = out.add(id, base, offset);
    out.prim(id, "setPosition", &[("position", at)]);
}

/// Particles end with the effect: at once, or fading out over `fade` seconds (the
/// renderer's colour takes the fade).
fn ends(out: &mut Out, id: &str, fade: f64, look: &mut Look) {
    let running = out.out(id, "effectRunning", &[]);
    if fade <= 0.0 {
        let stopped = out.out(id, "not", &[("x", running)]);
        out.prim(id, "kill", &[("condition", stopped)]);
    } else {
        // A timer that runs once the effect stops: 0 → 1 over `fade`.
        let name = format!(
            "ends_{}",
            id.replace(|c: char| !c.is_ascii_alphanumeric(), "_")
        );
        let t = out.prim(id, "attribute", &[]);
        out.text(&t, "name", &name);
        let rate = out.prim(
            id,
            "select",
            &[
                ("a", Val::Lit(vec![1.0 / (60.0 * fade)])),
                ("b", Val::Lit(vec![0.0])),
            ],
        );
        out.feed(&rate, "condition", running);
        let next = out.add(
            id,
            Val::Out(format!("{t}.x")),
            Val::Out(format!("{rate}.value")),
        );
        let next = out.out(id, "clamp", &[("x", next)]);
        let set = out.prim(id, "setAttribute", &[("value", next.clone())]);
        out.text(&set, "name", &name);
        let done = out.prim(
            id,
            "compare",
            &[("a", next.clone()), ("b", Val::Lit(vec![1.0]))],
        );
        out.text(&done, "op", ">=");
        out.prim(
            id,
            "kill",
            &[("condition", Val::Out(format!("{done}.value")))],
        );
        let keep = out.out(id, "subtract", &[("a", Val::Lit(vec![1.0])), ("b", next)]);
        look.keep = Some(keep);
    }
}

/// The renderer at the end of the chain (`node`), or the default white dots.
fn draw(out: &mut Out, owner: &str, look: &Look, node: Option<&Node>) {
    let kind = node.map_or("billboard", |n| n.kind.as_str());
    let size = look.size.clone().unwrap_or(Val::Lit(vec![0.3]));
    let mut colour = look
        .colour
        .clone()
        .unwrap_or(Val::Lit(vec![1.0, 1.0, 1.0, 1.0]));
    if let Some(keep) = look.keep.clone() {
        colour = out.mul(owner, colour, keep);
    }
    if kind == "ribbon" {
        let node = node.expect("ribbon node");
        let p = out.prim(owner, "ribbon", &[("width", size), ("colour", colour)]);
        let (texture, blend, tiling) = (
            out.choice(node, "texture"),
            out.choice(node, "blend"),
            out.number(node, "tiling"),
        );
        out.text(
            &p,
            "texture",
            if texture.is_empty() { DOT } else { &texture },
        );
        out.text(&p, "blend", &blend);
        out.feed(&p, "along", Val::Lit(vec![tiling]));
        return;
    }
    let p = out.prim(owner, "billboard", &[("size", size), ("colour", colour)]);
    if let Some(r) = look.rotation.clone() {
        out.feed(&p, "rotation", r);
    }
    if let (Some(f), Some(fb)) = (look.frame.clone(), look.flipbook) {
        out.feed(&p, "frame", f);
        out.feed(&p, "flipbook", Val::Lit(fb.to_vec()));
    }
    let Some(node) = node else {
        out.text(&p, "texture", DOT);
        return;
    };
    let texture = out.choice(node, "texture");
    out.text(
        &p,
        "texture",
        if texture.is_empty() { DOT } else { &texture },
    );
    let blend = out.choice(node, "blend");
    out.text(&p, "blend", &blend);
    let soft = out.number(node, "soft");
    out.feed(&p, "soft", Val::Lit(vec![soft]));
    let facing = out.choice(node, "facing");
    let (mode, axis) = match facing.as_str() {
        "ground" => ("plane", Some(Val::Lit(vec![0.0, 1.0, 0.0]))),
        "axis" => ("axis", Some(out.plain(node, "axis"))),
        "velocity" => {
            let v = out.out(owner, "velocity", &[]);
            let k = look.stretch.clone().unwrap_or(Val::Lit(vec![0.05]));
            ("spheroid", Some(out.mul(owner, v, k)))
        }
        _ => match look.stretch.clone() {
            Some(k) => {
                let v = out.out(owner, "velocity", &[]);
                ("spheroid", Some(out.mul(owner, v, k)))
            }
            None => ("camera", None),
        },
    };
    out.text(&p, "mode", mode);
    if let Some(a) = axis {
        out.feed(&p, "axis", a);
    }
}

/// Where an anchor is now, in the emitter's space.
fn anchor(out: &mut Out, owner: &str, which: &str) -> Val {
    match which {
        "caster" | "target" => {
            let t = out.prim(owner, "target", &[]);
            let pin = if which == "caster" {
                "source"
            } else {
                "position"
            };
            Val::Out(format!("{t}.{pin}"))
        }
        _ => out.out(owner, "emitter", &[]),
    }
}

/// The soft dot drawn where a layer has no texture yet.
pub const DOT: &str = "asset.effects.fx.soft-dot";

fn normalized(v: &[f64]) -> Vec<f64> {
    let l = v.iter().map(|x| x * x).sum::<f64>().sqrt();
    if l == 0.0 {
        vec![0.0, 1.0, 0.0]
    } else {
        v.iter().map(|x| x / l).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{check, compile};

    /// A flame-like layer: a stream from a ring, launched up, pulled by gravity and drag,
    /// swirling, growing and cooling from orange to red, spinning, on a flipbook, firing
    /// an event as each dies.
    const FLAME: &str = r##"{
      "nodes": [
        {"id": "spawn", "kind": "spawner", "inputs": {"mode": "stream", "rate": 40, "duration": 1.5, "life": [0.6, 1.0]}},
        {"id": "shape", "kind": "shape", "inputs": {"shape": "ring", "radius": 2}},
        {"id": "launch", "kind": "launch", "inputs": {"direction": "up", "speed": [6, 12]}},
        {"id": "physics", "kind": "physics", "inputs": {"gravity": [0, 4, 0], "drag": 1.5}},
        {"id": "swirl", "kind": "orbit", "inputs": {"speed": [-60, 60]}},
        {"id": "size", "kind": "sizeOverLife", "inputs": {"size": [2, 4]}},
        {"id": "colour", "kind": "colourOverLife", "inputs": {"brightness": [0.5, 2], "shift": [0, 0.2, 1]}},
        {"id": "spin", "kind": "spin", "inputs": {"speed": [-90, 90]}},
        {"id": "frames", "kind": "flipbook"},
        {"id": "pop", "kind": "emit", "inputs": {"when": "death", "chance": 0.3}},
        {"id": "draw", "kind": "billboard", "inputs": {"texture": "asset.test.flame"}},
        {"id": "heat", "kind": "curveOverLife"}
      ],
      "wires": [
        {"from": "spawn.out", "to": "shape.in"},
        {"from": "shape.out", "to": "launch.in"},
        {"from": "launch.out", "to": "physics.in"},
        {"from": "physics.out", "to": "swirl.in"},
        {"from": "swirl.out", "to": "size.in"},
        {"from": "size.out", "to": "colour.in"},
        {"from": "colour.out", "to": "spin.in"},
        {"from": "spin.out", "to": "frames.in"},
        {"from": "frames.out", "to": "pop.in"},
        {"from": "pop.out", "to": "draw.in"},
        {"from": "heat.value", "to": "size.curve"}
      ]
    }"##;

    fn validate(wgsl: &str) {
        let module = naga::front::wgsl::parse_str(wgsl)
            .unwrap_or_else(|e| panic!("{}\n{wgsl}", e.emit_to_string(wgsl)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .unwrap_or_else(|e| panic!("{e:?}\n{wgsl}"));
    }

    #[test]
    fn a_chain_of_authoring_nodes_compiles_to_valid_wgsl() {
        let graph: Graph = serde_json::from_str(FLAME).unwrap();
        assert!(is_flow(&graph));
        let program = check(&graph).unwrap_or_else(|p| panic!("{p:?}"));
        let layer = compile::compile(&program, &[]);
        validate(&layer.wgsl);
        // The Emit keeps its id, so other layers' Spawners can name it.
        assert!(program.index("pop").is_some());
    }

    #[test]
    fn every_step_of_the_chain_previews_something_new() {
        let graph: Graph = serde_json::from_str(FLAME).unwrap();
        let mut previews: Vec<String> = Vec::new();
        for id in [
            "spawn", "shape", "launch", "physics", "swirl", "size", "colour", "spin", "frames",
        ] {
            let g = expand(&graph, Some(id)).unwrap_or_else(|p| panic!("{id}: {p:?}"));
            let program = crate::check(&g).unwrap_or_else(|p| panic!("{id}: {p:?}"));
            let layer = compile::compile(&program, &[]);
            validate(&layer.wgsl);
            assert!(
                !previews.contains(&layer.wgsl),
                "{id} adds nothing to the preview"
            );
            previews.push(layer.wgsl);
        }
    }

    fn compiles(json: &str) {
        let graph: Graph = serde_json::from_str(json).unwrap();
        let program = check(&graph).unwrap_or_else(|p| panic!("{p:?}"));
        validate(&compile::compile(&program, &[]).wgsl);
    }

    #[test]
    fn paths_beams_bounces_and_followers_compile() {
        // A star falling from 30 m above the target onto it.
        compiles(
            r#"{"nodes": [
              {"id": "s", "kind": "spawner"},
              {"id": "p", "kind": "path", "inputs": {"from": "target", "fromOffset": [0, 30, 0], "to": "target", "arc": 2}},
              {"id": "b", "kind": "billboard"}],
             "wires": [{"from": "s.out", "to": "p.in"}, {"from": "p.out", "to": "b.in"}]}"#,
        );
        // A drain: 30 points from the caster to the target, swaying and flickering.
        compiles(
            r#"{"nodes": [
              {"id": "s", "kind": "spawner", "inputs": {"count": 30, "ends": "effect", "fade": 0.3}},
              {"id": "beam", "kind": "beam", "inputs": {"amplitude": 0.5, "waves": 2, "jitter": 0.1}},
              {"id": "r", "kind": "ribbon"}],
             "wires": [{"from": "s.out", "to": "beam.in"}, {"from": "beam.out", "to": "r.in"}]}"#,
        );
        // Debris thrown up that bounces, firing an event on each hit.
        compiles(
            r#"{"nodes": [
              {"id": "s", "kind": "spawner"},
              {"id": "go", "kind": "launch", "inputs": {"direction": "cone", "angle": 40}},
              {"id": "fall", "kind": "physics", "inputs": {"gravity": [0, -20, 0]}},
              {"id": "hit", "kind": "bounce"},
              {"id": "dust", "kind": "emit", "inputs": {"when": "bounce", "chance": 0.5}},
              {"id": "b", "kind": "billboard"}],
             "wires": [{"from": "s.out", "to": "go.in"}, {"from": "go.out", "to": "fall.in"}, {"from": "fall.out", "to": "hit.in"},
                       {"from": "hit.out", "to": "dust.in"}, {"from": "dust.out", "to": "b.in"}]}"#,
        );
        // Motes riding the particle whose event bore them.
        compiles(
            r#"{"nodes": [
              {"id": "s", "kind": "spawner", "inputs": {"mode": "event", "layer": "head", "event": "trail"}},
              {"id": "around", "kind": "shape", "inputs": {"radius": 0.5, "follow": "parent"}},
              {"id": "b", "kind": "billboard"}],
             "wires": [{"from": "s.out", "to": "around.in"}, {"from": "around.out", "to": "b.in"}]}"#,
        );
    }

    #[test]
    fn tapering_streams_and_events_ellipsoids_hue_spread_and_emit_at_the_end_compile() {
        let graph: Graph = serde_json::from_str(
            r#"{
          "nodes": [
            {"id": "spawn", "kind": "spawner", "inputs": {"mode": "stream", "rate": 40, "duration": 0.5,
              "rateCurve": {"keys": [{"t": 0, "v": [1]}, {"t": 1, "v": [0]}]}}},
            {"id": "shape", "kind": "shape", "inputs": {"shape": "ellipsoid", "radius": 2, "size": [1, 0.3, 1]}},
            {"id": "colour", "kind": "colourOverLife", "inputs": {"hue": 0.05, "shift": [0, 0.2, 1], "team": "on"}},
            {"id": "end", "kind": "emit", "inputs": {"when": "end"}},
            {"id": "drip", "kind": "emit", "inputs": {"when": "interval", "interval": 0.05,
              "rateCurve": {"keys": [{"t": 0, "v": [1]}, {"t": 1, "v": [0]}]}}},
            {"id": "draw", "kind": "billboard", "inputs": {"texture": "t"}}
          ],
          "wires": [
            {"from": "spawn.out", "to": "shape.in"},
            {"from": "shape.out", "to": "colour.in"},
            {"from": "colour.out", "to": "end.in"},
            {"from": "end.out", "to": "drip.in"},
            {"from": "drip.out", "to": "draw.in"}
          ]
        }"#,
        )
        .unwrap();
        let program = check(&graph).unwrap_or_else(|p| panic!("{p:?}"));
        let layer = compile::compile(&program, &[]);
        let module = naga::front::wgsl::parse_str(&layer.wgsl).expect("parses");
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::empty(),
        )
        .validate(&module)
        .expect("valid");
        match layer.spawners[..] {
            [compile::Spawner::Rate { curve: Some(c), .. }] => {
                assert!(c[0] > 0.99 && c[16] < 0.01, "the rate tapers")
            }
            ref other => panic!("{other:?}"),
        }
    }

    #[test]
    fn a_spray_from_each_event_is_a_hidden_sprayer_and_the_layer_listening_to_it() {
        let graph: Graph = serde_json::from_str(
            r#"{"nodes": [
              {"id": "s", "kind": "spawner", "inputs": {"mode": "event", "layer": "drop", "event": "land",
                "spray": 0.5, "rate": 40, "rateCurve": {"keys": [{"t": 0, "v": [1]}, {"t": 1, "v": [0]}]}}},
              {"id": "b", "kind": "billboard"}],
             "wires": [{"from": "s.out", "to": "b.in"}]}"#,
        )
        .unwrap();
        let parts = parts("blood", &graph);
        let ids: Vec<&str> = parts.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(ids, ["blood~spray", "blood"]);
        for (_, g) in &parts {
            let program = check(g).unwrap_or_else(|p| panic!("{p:?}"));
            validate(&compile::compile(&program, &[]).wgsl);
        }
        let listens = |g: &Graph, name: &str| match g.nodes[0].inputs.get(name) {
            Some(Value::Text(t)) => t.clone(),
            _ => String::new(),
        };
        assert_eq!(listens(&parts[0].1, "layer"), "drop");
        assert_eq!(listens(&parts[1].1, "layer"), "blood~spray");
        assert!(!parts[1].1.nodes[0].inputs.contains_key("spray"));
        // A layer without a spray is itself.
        assert_eq!(super::parts("x", &parts[1].1).len(), 1);
    }

    #[test]
    fn a_layer_needs_one_spawner() {
        let graph: Graph = serde_json::from_str(
            r#"{"nodes": [{"id": "a", "kind": "spawner"}, {"id": "b", "kind": "spawner"}]}"#,
        )
        .unwrap();
        assert!(expand(&graph, None).is_err());
    }
}
