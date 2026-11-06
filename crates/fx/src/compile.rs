//! A checked layer graph to one WGSL module: the prelude (`wgsl/prelude.wgsl`) plus the
//! layer's `birth` and `update` kernels, and what the runtime needs beside it (record
//! stride, spawners, renderers, parameter order).
//!
//! - `birth` runs once per particle born this step: every node its writers (life, start
//!   position, velocity) need, then the values later steps read from birth, stored in the
//!   particle's record.
//! - `update` runs every step for every slot: ages the particle, moves it by its velocity,
//!   evaluates the nodes that change over its life (birth values loaded, constants
//!   recomputed), runs writers and forces, and writes each renderer's part record.

use crate::catalogue::{Code, NodeDef, Role, Ty, Var};
use crate::check::{Program, Source, attribute};
use crate::graph::Curve;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

pub const PRELUDE: &str = include_str!("wgsl/prelude.wgsl");
/// Parameters a layer may read (two vec4s of the layer uniform).
pub const MAX_PARAMETERS: usize = 8;
/// Threads per workgroup of both kernels.
pub const WORKGROUP: u32 = 64;
/// Vec4s of a particle record before its stored values.
pub const HEADER_VEC4S: u32 = 3;
/// Vec4s of a renderer's part record.
pub const PART_VEC4S: u32 = 4;

/// When particles are born, evaluated by the runtime on the CPU.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Spawner {
    /// `count` particles at `time` seconds into the effect.
    Burst { count: f32, time: f32 },
    /// `rate` a second from `start` for `duration` seconds (0: while the effect plays),
    /// times `curve` over that span when it has one ([`RATE_SAMPLES`] even samples).
    Rate {
        rate: f32,
        start: f32,
        duration: f32,
        curve: Option<[f32; RATE_SAMPLES]>,
    },
}

/// Samples of a rate curve, evenly from the start of a stream to its end.
pub const RATE_SAMPLES: usize = 17;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Camera,
    Axis,
    Spheroid,
    Plane,
    Velocity,
    /// A strip joining the particles in the order they were born.
    Ribbon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Blend {
    Additive,
    Alpha,
    /// Bends the scene behind by the texture's red and green.
    Distort,
}

/// When an Emit event node fires.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum When {
    Death,
    Birth,
    Interval,
    Distance,
    Step,
}

/// One Emit event node of a layer: a channel of event records the listening layers read.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// The Emit event node's id (what an On event spawner names).
    pub node: String,
    pub when: When,
    /// Seconds (interval) or metres (distance) between events.
    pub interval: f32,
    /// The most children one event may ask for.
    pub max_count: f32,
}

/// The layer and Emit event node an event-born layer listens to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventSource {
    pub layer: String,
    pub event: String,
}

/// Vec4s of one event record (`EVENT_VEC4S` in the prelude).
pub const EVENT_VEC4S: u32 = 6;

/// A renderer node's settings; its per-particle inputs are in the part records.
#[derive(Debug, Clone, PartialEq)]
pub struct Renderer {
    pub node: String,
    pub texture: String,
    pub mode: Mode,
    pub blend: Blend,
    /// Soft-particle distance, metres (0: hard).
    pub soft: f32,
    /// Flipbook columns and rows (1 × 1: a still texture).
    pub flipbook: [u32; 2],
}

/// A compiled layer.
#[derive(Debug, Clone, PartialEq)]
pub struct Layer {
    pub wgsl: String,
    /// Vec4s per particle record.
    pub stride: u32,
    pub spawners: Vec<Spawner>,
    pub renderers: Vec<Renderer>,
    /// Parameter names in the order the layer uniform carries them.
    pub parameters: Vec<String>,
    /// The longest a particle can live, seconds, as far as its life node tells (a number,
    /// or a random's larger end); [`DEFAULT_MAX_LIFE`] when it is computed.
    pub max_life: f32,
    /// Its Emit event nodes, in channel order.
    pub channels: Vec<Channel>,
    /// For a layer born of another's events: which.
    pub parent: Option<EventSource>,
}

/// The assumed longest life when a layer computes its particles' lives.
pub const DEFAULT_MAX_LIFE: f32 = 2.0;
/// Particles a layer instance may hold at once.
pub const MAX_CAPACITY: u32 = 4096;

impl Layer {
    /// Ring capacity for an instance playing `duration` seconds: everything born within
    /// one longest life, with a margin.
    pub fn capacity(&self, duration: f32) -> u32 {
        let alive: f32 = self
            .spawners
            .iter()
            .map(|s| match *s {
                Spawner::Burst { count, .. } => count.max(0.0),
                Spawner::Rate {
                    rate,
                    duration: d,
                    curve,
                    ..
                } => {
                    let span = if d > 0.0 { d } else { duration.max(0.1) };
                    let peak = curve.map_or(1.0, |c| c.iter().fold(0.0f32, |a, &b| a.max(b)));
                    rate.max(0.0) * peak * span.min(self.max_life + 0.1)
                }
            })
            .sum();
        ((alive * 1.25).ceil() as u32 + 8).min(MAX_CAPACITY)
    }
}

/// The longest life the layer's `setLife` allows.
fn max_life(program: &Program) -> f32 {
    let mut longest: Option<f32> = None;
    for node in program
        .of_role(Role::Writer)
        .filter(|n| n.def.kind == "setLife")
    {
        let life = match node.input("life") {
            Source::Numbers(n) => n.first().copied(),
            Source::Wire(a, _) => {
                let from = &program.nodes[*a];
                if from.def.kind != "random" {
                    None
                } else {
                    match (from.input("min"), from.input("max")) {
                        (Source::Numbers(lo), Source::Numbers(hi)) => {
                            lo.first().zip(hi.first()).map(|(a, b)| a.max(*b))
                        }
                        _ => None,
                    }
                }
            }
            _ => None,
        };
        let life = life.unwrap_or(DEFAULT_MAX_LIFE);
        longest = Some(longest.map_or(life, |l: f32| l.max(life)));
    }
    longest.unwrap_or(1.0)
}

/// A renderer input fed from somewhere else than its graph says: the node editor's
/// previews show the effect as if a node's output went straight into the renderer.
#[derive(Debug, Clone)]
pub struct Override {
    /// The renderer node's index in the program.
    pub renderer: usize,
    pub pin: &'static str,
    pub source: Source,
}

fn literal(numbers: &[f32], ty: Ty) -> String {
    let n = |i: usize| -> String {
        let x = numbers
            .get(i)
            .copied()
            .unwrap_or(if i == 3 { 1.0 } else { 0.0 });
        let x = if numbers.len() == 1 && i < 3 {
            numbers[0]
        } else {
            x
        };
        let x = if x.is_finite() { x } else { 0.0 };
        format!("{x:?}f")
    };
    match ty {
        Ty::F1 | Ty::Num => n(0),
        Ty::Bool => (numbers.first().copied().unwrap_or(0.0) > 0.5).to_string(),
        Ty::F2 => format!("vec2f({}, {})", n(0), n(1)),
        Ty::F3 => format!("vec3f({}, {}, {})", n(0), n(1), n(2)),
        Ty::F4 => format!("vec4f({}, {}, {}, {})", n(0), n(1), n(2), n(3)),
        Ty::Curve | Ty::Text => String::new(),
    }
}

/// `expr` of type `have` as type `want` (single numbers broadcast; numbers to booleans).
fn convert(expr: String, have: Ty, want: Ty) -> String {
    if have == want || (want == Ty::Num && have == Ty::F1) {
        return expr;
    }
    if have == Ty::Bool {
        let number = format!("select(0.0, 1.0, {expr})");
        return match want {
            Ty::F2 | Ty::F3 | Ty::F4 => format!("{}({number})", want.wgsl()),
            _ => number,
        };
    }
    match (have.width(), want) {
        (Some(1), Ty::Bool) => format!("({expr} > 0.5)"),
        (Some(1), Ty::F2 | Ty::F3 | Ty::F4) => format!("{}({expr})", want.wgsl()),
        _ => expr,
    }
}

/// `expr` of type `have` as any type `want`, for previews: a number becomes a height (an
/// upward offset or axis) or a grey colour, a vector its length, a colour its first three
/// components, three numbers a colour with full alpha.
fn convert_loose(expr: String, have: Ty, want: Ty) -> String {
    if have == Ty::Bool && want != Ty::Bool {
        return convert_loose(format!("select(0.0, 1.0, {expr})"), Ty::F1, want);
    }
    let (h, w) = (have.width().unwrap_or(1), want.width().unwrap_or(1));
    if h == w || want == Ty::Num {
        return expr;
    }
    match (h, want) {
        (1, Ty::F2) => format!("vec2f({expr})"),
        (1, Ty::F3) => format!("vec3f(0.0, {expr}, 0.0)"),
        (1, Ty::F4) => format!("vec4f(vec3f({expr}), 1.0)"),
        (1, Ty::Bool) => format!("({expr} > 0.5)"),
        (2, Ty::F3) => format!("vec3f({expr}, 0.0)"),
        (2, Ty::F4) => format!("vec4f({expr}, 0.0, 1.0)"),
        (3, Ty::F4) => format!("vec4f({expr}, 1.0)"),
        (4, Ty::F3) => format!("({expr}).xyz"),
        (4, Ty::F2) | (3, Ty::F2) => format!("({expr}).xy"),
        (_, Ty::F1) => format!("length({expr})"),
        (_, Ty::Bool) => format!("(length({expr}) > 0.5)"),
        _ => expr,
    }
}

/// What a node's preview shows (the node editor, D113).
#[derive(Debug, Clone)]
pub enum Preview {
    /// The finished layer (the renderer).
    Finished,
    /// Plain particles: every renderer input at its default but the position (shapes,
    /// spawners, writers and forces show where particles are born and how they move).
    Bare(Vec<Override>),
    /// The layer as if the node's output went straight into the renderer inputs it
    /// reaches.
    Feeds(Vec<Override>),
}

/// The preview of node `node` (an index into `program.nodes`).
pub fn preview(program: &Program, node: usize) -> Preview {
    let nodes = &program.nodes;
    if nodes[node].def.role == Role::Renderer {
        return Preview::Finished;
    }
    // Every node downstream of `node`, itself included.
    let mut downstream = BTreeSet::from([node]);
    for &i in &program.order {
        if nodes[i]
            .inputs
            .iter()
            .any(|s| matches!(s, Source::Wire(a, _) if downstream.contains(a)))
        {
            downstream.insert(i);
        }
    }
    let mut feeds = Vec::new();
    if nodes[node].def.role == Role::Value {
        for (r, renderer) in nodes
            .iter()
            .enumerate()
            .filter(|(_, n)| n.def.role == Role::Renderer)
        {
            for (pin, source) in renderer.def.inputs.iter().zip(&renderer.inputs) {
                if matches!(source, Source::Wire(a, _) if downstream.contains(a)) {
                    feeds.push(Override {
                        renderer: r,
                        pin: pin.name,
                        source: Source::Wire(node, 0),
                    });
                }
            }
        }
    }
    if !feeds.is_empty() {
        return Preview::Feeds(feeds);
    }
    let mut bare = Vec::new();
    for (r, renderer) in nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.def.role == Role::Renderer)
    {
        for (pin, source) in renderer.def.inputs.iter().zip(&renderer.inputs) {
            if pin.name == "position" || !matches!(source, Source::Wire(..)) {
                continue;
            }
            let value = match pin.name {
                "size" => vec![0.5],
                "colour" => vec![1.0, 0.85, 0.6, 1.0],
                "axis" => vec![0.0, 0.5, 0.0],
                _ => match pin.default {
                    crate::catalogue::Fallback::Numbers(n) => n.to_vec(),
                    _ => continue,
                },
            };
            bare.push(Override {
                renderer: r,
                pin: pin.name,
                source: Source::Numbers(value),
            });
        }
    }
    Preview::Bare(bare)
}

fn swizzle(width: u8) -> &'static str {
    match width {
        1 => ".x",
        2 => ".xy",
        3 => ".xyz",
        _ => "",
    }
}

fn as_vec4(expr: &str, ty: Ty) -> String {
    match ty {
        Ty::F1 | Ty::Num => format!("vec4f({expr}, 0.0, 0.0, 0.0)"),
        Ty::Bool => format!("vec4f(select(0.0, 1.0, {expr}), 0.0, 0.0, 0.0)"),
        Ty::F2 => format!("vec4f({expr}, 0.0, 0.0)"),
        Ty::F3 => format!("vec4f({expr}, 0.0)"),
        _ => expr.to_string(),
    }
}

fn curve_function(name: &str, curve: &Curve) -> String {
    let v = |values: &[f64]| -> String {
        let mut c: Vec<f32> = values.iter().map(|&x| x as f32).collect();
        c.resize(4, 0.0);
        literal(&c, Ty::F4)
    };
    let keys = &curve.keys;
    let mut out = format!("fn {name}(x: f32) -> vec4f {{\n");
    let _ = writeln!(
        out,
        "    let t = clamp(x, {:?}f, {:?}f);",
        keys[0].t as f32,
        keys[keys.len() - 1].t as f32
    );
    for w in keys.windows(2) {
        let (a, b) = (&w[0], &w[1]);
        let _ = writeln!(out, "    if (t <= {:?}f) {{", b.t as f32);
        let _ = writeln!(
            out,
            "        let s = (t - {:?}f) / {:?}f;",
            a.t as f32,
            ((b.t - a.t) as f32).max(1e-9)
        );
        match (&a.tangent_out, &b.tangent_in) {
            (Some(m0), Some(m1)) => {
                let _ = writeln!(
                    out,
                    "        return hermite(s, {}, {}, {}, {});",
                    v(&a.v),
                    v(m0),
                    v(&b.v),
                    v(m1)
                );
            }
            _ => {
                let _ = writeln!(out, "        return mix({}, {}, s);", v(&a.v), v(&b.v));
            }
        }
        out.push_str("    }\n");
    }
    let _ = writeln!(out, "    return {};\n}}", v(&keys[keys.len() - 1].v));
    out
}

struct Emitter<'a> {
    program: &'a Program,
    sources: Vec<Vec<Source>>,
    parameters: Vec<String>,
    /// Names of the particle's own attributes (Set attribute), in record order.
    attributes: Vec<String>,
    /// Inputs fed by an override (node, pin): converted loosely between any two types.
    loose: BTreeSet<(usize, usize)>,
}

impl Emitter<'_> {
    fn name(node: usize, output: usize) -> String {
        format!("v{node}_{output}")
    }

    fn source_ty(&self, source: &Source) -> Ty {
        match source {
            Source::Wire(a, o) => self.program.nodes[*a].output_ty(*o),
            Source::Attribute(kind) => attribute(kind).outputs[0].ty,
            Source::Numbers(n) => Ty::of_width(n.len().clamp(1, 4) as u8),
            _ => Ty::Text,
        }
    }

    /// An input as WGSL of the pin's resolved type.
    fn input(&self, node: usize, pin: usize) -> String {
        let checked = &self.program.nodes[node];
        let def = checked.def.inputs[pin];
        let want = if def.ty == Ty::Num {
            Ty::of_width(checked.width)
        } else {
            def.ty
        };
        let source = &self.sources[node][pin];
        let expr = match source {
            Source::Wire(a, o) => Self::name(*a, *o),
            Source::Attribute(kind) => match attribute(kind).code {
                Code::Expr(e) => e.to_string(),
                _ => "0.0".into(),
            },
            Source::Numbers(n) => return literal(n, want),
            Source::Text(_) | Source::Curve(_) => return String::new(),
        };
        if self.loose.contains(&(node, pin)) {
            return convert_loose(expr, self.source_ty(source), want);
        }
        convert(expr, self.source_ty(source), want)
    }

    fn template(&self, node: usize, text: &str) -> String {
        let checked = &self.program.nodes[node];
        let mut out = text
            .replace("{W}", &checked.width.to_string())
            .replace("{T}", Ty::of_width(checked.width).wgsl());
        for (i, pin) in checked.def.inputs.iter().enumerate() {
            let key = format!("{{{}}}", pin.name);
            if out.contains(&key) {
                out = out.replace(&key, &self.input(node, i));
            }
        }
        out
    }

    /// The statements evaluating node `i` (its `let`s, or a writer's effect).
    fn node(&self, i: usize, out: &mut String) {
        let checked = &self.program.nodes[i];
        let def: &NodeDef = checked.def;
        match (def.role, def.code) {
            (Role::Value, Code::Expr(text)) => {
                let _ = writeln!(
                    out,
                    "    let {} = {};",
                    Self::name(i, 0),
                    self.template(i, text)
                );
            }
            (Role::Writer, Code::Stmt(text)) => {
                let _ = writeln!(out, "    {}", self.template(i, text));
            }
            (Role::Writer, Code::Builtin) if def.kind == "setAttribute" => {
                let k = self.attribute(checked.text("name"));
                let value = as_vec4(&self.input(i, 1), Ty::of_width(checked.width));
                let _ = writeln!(out, "    attr_{k} = {value};");
            }
            (Role::Value, Code::Builtin) => match def.kind {
                "float" | "vector" | "colour" => {
                    let _ = writeln!(
                        out,
                        "    let {} = {};",
                        Self::name(i, 0),
                        literal(checked.numbers("value"), checked.output_ty(0))
                    );
                }
                "parameter" => {
                    let k = self
                        .parameters
                        .iter()
                        .position(|p| p == checked.text("name"))
                        .unwrap_or(0);
                    let _ = writeln!(
                        out,
                        "    let {} = u.params[{}u][{}];",
                        Self::name(i, 0),
                        k / 4,
                        k % 4
                    );
                }
                "curve" => {
                    let x = self.input(i, 1);
                    let _ = writeln!(
                        out,
                        "    let {} = curve_{i}({x}){};",
                        Self::name(i, 0),
                        swizzle(checked.width)
                    );
                }
                "compare" => {
                    let op = checked.text("op");
                    let op = if op.is_empty() { "<" } else { op };
                    let _ = writeln!(
                        out,
                        "    let {} = ({} {op} {});",
                        Self::name(i, 0),
                        self.input(i, 0),
                        self.input(i, 1)
                    );
                }
                "event" => {
                    for (o, name) in [
                        "ev_position",
                        "ev_velocity",
                        "ev_colour",
                        "ev_value",
                        "ev_order",
                        "ev_share",
                    ]
                    .iter()
                    .enumerate()
                    {
                        let _ = writeln!(out, "    let {} = {name};", Self::name(i, o));
                    }
                }
                "target" => {
                    let _ = writeln!(out, "    let {} = u.end_target.xyz;", Self::name(i, 0));
                    let _ = writeln!(out, "    let {} = u.end_source.xyz;", Self::name(i, 1));
                }
                "parent" => {
                    let _ = writeln!(
                        out,
                        "    let parent_{i} = parent_record(slot);\n    \
                         let {} = select(vec3f(0.0), particles[parent_{i}].xyz, has_parent());\n    \
                         let {} = select(vec3f(0.0), particles[parent_{i} + 1u].xyz, has_parent());\n    \
                         let {} = has_parent() && bitcast<u32>(particles[parent_{i} + 2u].z) != 0u;",
                        Self::name(i, 0),
                        Self::name(i, 1),
                        Self::name(i, 2)
                    );
                }
                "attribute" => {
                    let k = self.attribute(checked.text("name"));
                    let _ = writeln!(out, "    let {} = attr_{k};", Self::name(i, 0));
                    let _ = writeln!(out, "    let {} = attr_{k}.xyz;", Self::name(i, 1));
                    let _ = writeln!(out, "    let {} = attr_{k}.x;", Self::name(i, 2));
                }
                "split" => {
                    let value = self.input(i, 0);
                    for (o, c) in ["x", "y", "z", "w"].iter().enumerate() {
                        let expr = if checked.width == 1 {
                            if o == 0 { value.clone() } else { "0.0".into() }
                        } else if o < checked.width as usize {
                            format!("{value}.{c}")
                        } else {
                            "0.0".into()
                        };
                        let _ = writeln!(out, "    let {} = {expr};", Self::name(i, o));
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// Nodes upstream of `roots` (through wires), stopping at nodes `stop` accepts.
    fn upstream(
        &self,
        roots: impl IntoIterator<Item = usize>,
        stop: impl Fn(usize) -> bool,
    ) -> BTreeSet<usize> {
        let mut seen = BTreeSet::new();
        let mut stack: Vec<usize> = roots.into_iter().collect();
        while let Some(i) = stack.pop() {
            if !seen.insert(i) || stop(i) {
                continue;
            }
            for source in &self.sources[i] {
                if let Source::Wire(a, _) = source {
                    stack.push(*a);
                }
            }
        }
        seen
    }

    /// The record index of attribute `name`.
    fn attribute(&self, name: &str) -> usize {
        self.attributes.iter().position(|a| a == name).unwrap_or(0)
    }

    /// An Emit event node's statements: in the birth kernel (birth events), or in the
    /// update (every other kind, and keeping or clearing a birth event's count).
    fn emit(&self, c: usize, i: usize, birth: bool, out: &mut String) {
        let checked = &self.program.nodes[i];
        let pin = |name: &str| {
            let at = checked
                .def
                .inputs
                .iter()
                .position(|p| p.name == name)
                .expect("emit pin");
            self.input(i, at)
        };
        let count = format!("u32(max({}, 0.0) + 0.5)", pin("count"));
        let condition = pin("condition");
        let position = pin("position");
        // Where the payload position was at the step's start: moved as the particle moved.
        let from = if birth {
            position.clone()
        } else {
            format!("(({position}) - (p.position - moved_from))")
        };
        let payload = format!(
            "{from}, {position}, {}, {}, {}",
            pin("velocity"),
            pin("colour"),
            pin("value")
        );
        let _ = writeln!(
            out,
            "    {{\n        let r = event_record(u.events_out, {c}u, u.capacity, slot);"
        );
        let when = checked.text("when");
        if birth {
            let _ = writeln!(
                out,
                "        let n = select(0u, {count}, {condition});\n        \
                 write_event(r, n, 0u, p.index, true, {payload});\n        \
                 chan_{c} = vec4f(f32(n), 0.0, 0.0, 0.0);\n    }}"
            );
            return;
        }
        if when == "birth" {
            let _ = writeln!(
                out,
                "        if (particles[r + 4u].y > 0.5) {{\n            particles[r + 4u].y = 0.0;\n        \
                 }} else {{\n            particles[r].w = 0.0;\n        }}\n    }}"
            );
            return;
        }
        let fires = match when {
            "death" => format!("select(0u, {count}, dying && {condition})"),
            "interval" | "distance" => {
                let step = if when == "interval" {
                    "dt".to_string()
                } else {
                    "length(p.position - moved_from)".to_string()
                };
                // The phase towards the next event: an interval that changes as the
                // particle lives (a rate tapering by a curve) is followed as it goes.
                let _ = writeln!(
                    out,
                    "        let every = max({}, 1e-4);\n        \
                     chan_{c}.y += {step} / every;\n        \
                     let times = floor(chan_{c}.y);\n        \
                     chan_{c}.y -= times;",
                    pin("interval")
                );
                format!("select(0u, u32(times) * {count}, {condition})")
            }
            _ => format!("select(0u, {count}, {condition})"),
        };
        let _ = writeln!(
            out,
            "        let n = {fires};\n        \
             write_event(r, n, u32(chan_{c}.x), p.index, false, {payload});\n        \
             chan_{c}.x += f32(n);\n    }}"
        );
    }

    fn wired_inputs(&self, i: usize) -> impl Iterator<Item = usize> + '_ {
        self.sources[i].iter().filter_map(|s| match s {
            Source::Wire(a, _) => Some(*a),
            _ => None,
        })
    }
}

/// Compiles a checked layer graph. `overrides` re-route renderer inputs (node previews).
pub fn compile(program: &Program, overrides: &[Override]) -> Layer {
    let mut sources: Vec<Vec<Source>> = program.nodes.iter().map(|n| n.inputs.clone()).collect();
    for o in overrides {
        if let Some(pin) = program.nodes[o.renderer]
            .def
            .inputs
            .iter()
            .position(|p| p.name == o.pin)
        {
            sources[o.renderer][pin] = o.source.clone();
        }
    }
    let mut parameters: Vec<String> = Vec::new();
    for node in program.nodes.iter().filter(|n| n.def.kind == "parameter") {
        let name = node.text("name").to_string();
        if !parameters.contains(&name) && parameters.len() < MAX_PARAMETERS {
            parameters.push(name);
        }
    }
    let loose = overrides
        .iter()
        .filter_map(|o| {
            let pin = program.nodes[o.renderer]
                .def
                .inputs
                .iter()
                .position(|p| p.name == o.pin)?;
            Some((o.renderer, pin))
        })
        .collect();
    let mut attributes: Vec<String> = Vec::new();
    for node in program
        .nodes
        .iter()
        .filter(|n| matches!(n.def.kind, "setAttribute" | "attribute"))
    {
        let name = node.text("name").to_string();
        if !attributes.contains(&name) {
            attributes.push(name);
        }
    }
    let e = Emitter {
        program,
        sources,
        parameters,
        attributes,
        loose,
    };
    let nodes = &program.nodes;
    let renderers: Vec<usize> = program
        .order
        .iter()
        .copied()
        .filter(|&i| nodes[i].def.role == Role::Renderer)
        .collect();
    let emits: Vec<usize> = program
        .order
        .iter()
        .copied()
        .filter(|&i| nodes[i].def.kind == "emit")
        .collect();
    let writers: Vec<usize> = program
        .order
        .iter()
        .copied()
        .filter(|&i| nodes[i].def.role == Role::Writer && nodes[i].def.kind != "emit")
        .collect();
    let birth_writers: Vec<usize> = writers
        .iter()
        .copied()
        .filter(|&i| nodes[i].var <= Var::Birth && nodes[i].def.var != Var::Step)
        .collect();
    let step_writers: Vec<usize> = writers
        .iter()
        .copied()
        .filter(|i| !birth_writers.contains(i))
        .collect();
    let birth_emits: Vec<usize> = emits
        .iter()
        .copied()
        .filter(|&i| nodes[i].text("when") == "birth")
        .collect();
    let step_emits: Vec<usize> = emits
        .iter()
        .copied()
        .filter(|&i| nodes[i].text("when") != "birth")
        .collect();
    let channel = |i: usize| emits.iter().position(|&e| e == i).expect("an emit");

    // The update evaluates what its writers, emits and renderers need, loading birth values.
    let step_roots: Vec<usize> = step_writers
        .iter()
        .chain(&step_emits)
        .chain(&renderers)
        .flat_map(|&i| e.wired_inputs(i).collect::<Vec<_>>())
        .collect();
    let step_set = e.upstream(step_roots, |i| nodes[i].var == Var::Birth);
    let stored: Vec<usize> = step_set
        .iter()
        .copied()
        .filter(|&i| nodes[i].var == Var::Birth)
        .collect();
    // Birth evaluates what its writers and emits need and every value the update loads.
    let birth_roots: Vec<usize> = birth_writers
        .iter()
        .chain(&birth_emits)
        .flat_map(|&i| e.wired_inputs(i).collect::<Vec<_>>())
        .chain(stored.iter().copied())
        .collect();
    let birth_set = e.upstream(birth_roots, |_| false);

    let mut wgsl = String::from(PRELUDE);
    for (i, node) in nodes.iter().enumerate() {
        if let (true, Some(Source::Curve(curve))) = (node.def.kind == "curve", node.inputs.first())
        {
            let _ = write!(wgsl, "\n{}", curve_function(&format!("curve_{i}"), curve));
        }
    }

    // Record layout after the header: stored birth values, the particle's own attributes,
    // then one vec4 of state per channel (children so far, interval or distance carried).
    let mut slots: BTreeMap<(usize, usize), u32> = BTreeMap::new();
    for &i in &stored {
        for o in 0..nodes[i].def.outputs.len() {
            let k = slots.len() as u32;
            slots.insert((i, o), k);
        }
    }
    let attribute_base = slots.len();
    let channel_base = attribute_base + e.attributes.len();
    let state_vars = |load: bool| -> String {
        let mut out = String::new();
        for k in 0..e.attributes.len() {
            let init = if load {
                format!("load_stored(slot, {}u)", attribute_base + k)
            } else {
                "vec4f(0.0)".into()
            };
            let _ = writeln!(out, "    var attr_{k} = {init};");
        }
        for c in 0..emits.len() {
            let init = if load {
                format!("load_stored(slot, {}u)", channel_base + c)
            } else {
                "vec4f(0.0)".into()
            };
            let _ = writeln!(out, "    var chan_{c} = {init};");
        }
        out
    };
    let store_state = || -> String {
        let mut out = String::new();
        for k in 0..e.attributes.len() {
            let _ = writeln!(
                out,
                "    store_value(slot, {}u, attr_{k});",
                attribute_base + k
            );
        }
        for c in 0..emits.len() {
            let _ = writeln!(
                out,
                "    store_value(slot, {}u, chan_{c});",
                channel_base + c
            );
        }
        out
    };

    // Birth: the body runs once per new particle, `p` and `slot` set up by the kernel.
    let mut body = String::from("    var rng = p.seed;\n");
    body.push_str(&state_vars(false));
    for &i in &program.order {
        if birth_set.contains(&i) && nodes[i].def.role == Role::Value {
            e.node(i, &mut body);
        }
        if birth_writers.contains(&i) {
            e.node(i, &mut body);
        }
    }
    for &i in &stored {
        for o in 0..nodes[i].def.outputs.len() {
            let _ = writeln!(
                body,
                "    store_value(slot, {}u, {});",
                slots[&(i, o)],
                as_vec4(&Emitter::name(i, o), nodes[i].output_ty(o))
            );
        }
    }
    for &i in &birth_emits {
        e.emit(channel(i), i, true, &mut body);
    }
    body.push_str(&store_state());
    body.push_str("    store_particle(slot, p);\n");

    let parent = program
        .of_role(Role::Spawner)
        .find(|n| n.def.kind == "onEvent")
        .map(|n| EventSource {
            layer: n.text("layer").to_string(),
            event: n.text("event").to_string(),
        });
    if parent.is_some() {
        // Born of a parent layer's events: one thread per parent slot, its children in the
        // ring of slots that parent slot owns.
        wgsl.push_str(
            "\n@compute @workgroup_size(64)\nfn birth(@builtin(global_invocation_id) id: vec3u) {\n    \
             if (id.x >= u.spawn_count) {\n        return;\n    }\n    let parent = id.x;\n    \
             let r = event_record(u.events_in, u.channel, u.spawn_count, parent);\n    \
             let head = particles[r];\n    let event_count = min(u32(head.w), u.per_parent);\n    \
             let before = bitcast<u32>(particles[r + 1u].w);\n    \
             let parent_index = bitcast<u32>(particles[r + 4u].x);\n    \
             let ev_from = particles[r + 5u].xyz;\n    let ev_velocity = particles[r + 1u].xyz;\n    \
             let ev_colour = particles[r + 2u];\n    let ev_value = particles[r + 3u];\n    \
             for (var j = 0u; j < event_count; j += 1u) {\n    \
             let ev_position = mix(ev_from, head.xyz, f32(j + 1u) / f32(event_count));\n    \
             let ev_order = f32(j);\n    \
             let ev_share = select(0.0, f32(j) / f32(event_count - 1u), event_count > 1u);\n    \
             let index = before + j;\n    let slot = parent * u.per_parent + index % u.per_parent;\n    \
             var p: Particle;\n    p.index = index;\n    \
             p.seed = pcg(u.seed ^ pcg(parent_index ^ pcg(index + 1757159915u)));\n    \
             p.alive = 1u;\n    p.life = 1.0;\n    p.born = u.effect_time - u.dt;\n    p.age = 0.0;\n    \
             p.position = ev_position;\n    p.velocity = vec3f(0.0);\n    let effect_time = p.born;\n",
        );
        wgsl.push_str(&body);
        wgsl.push_str("    }\n}\n");
    } else {
        wgsl.push_str(
            "\n@compute @workgroup_size(64)\nfn birth(@builtin(global_invocation_id) id: vec3u) {\n    \
             if (id.x >= u.spawn_count) {\n        return;\n    }\n    \
             let index = u.spawn_first + id.x;\n    let slot = index % u.capacity;\n    \
             var p: Particle;\n    p.index = index;\n    p.seed = pcg(u.seed ^ pcg(index));\n    \
             p.alive = 1u;\n    p.life = 1.0;\n    p.born = u.spawn_start + f32(id.x) * u.spawn_spacing;\n    \
             p.age = (u.effect_time - u.dt) - p.born;\n    let effect_time = p.born;\n    \
             let ev_position = vec3f(0.0);\n    let ev_velocity = vec3f(0.0);\n    \
             let ev_colour = vec4f(0.0);\n    let ev_value = vec4f(0.0);\n    \
             let ev_order = f32(id.x);\n    \
             let ev_share = select(0.0, f32(id.x) / f32(u.spawn_count - 1u), u.spawn_count > 1u);\n",
        );
        wgsl.push_str(&body);
        wgsl.push_str("}\n");
    }

    // Update. A dead slot clears its event counts; a particle reaching its life takes one
    // last step (for its death events) and is hidden.
    let mut clear = String::new();
    for c in 0..emits.len() {
        let _ = write!(
            clear,
            "        {{\n            let r = event_record(u.events_out, {c}u, u.capacity, slot);\n            \
             particles[r].w = 0.0;\n            particles[r + 4u].y = 0.0;\n        }}\n"
        );
    }
    let _ = write!(
        wgsl,
        "\n@compute @workgroup_size(64)\nfn update(@builtin(global_invocation_id) id: vec3u) {{\n    \
         if (id.x >= u.capacity) {{\n        return;\n    }}\n    let slot = id.x;\n    \
         var p = load_particle(slot);\n    if (p.alive == 0u) {{\n        hide_parts(slot);\n{clear}        return;\n    }}\n    \
         let previous = p.age;\n    p.age += u.dt;\n    var dying = p.age >= p.life;\n    \
         if (dying) {{\n        p.age = p.life;\n    }}\n    \
         let dt = max(p.age - max(previous, 0.0), 0.0);\n    let effect_time = u.effect_time;\n    \
         var rng = pcg(p.seed ^ bitcast<u32>(p.age));\n    let moved_from = p.position;\n    \
         p.position += p.velocity * dt;\n"
    );
    wgsl.push_str(&state_vars(true));
    for (&(i, o), &k) in &slots {
        let ty = nodes[i].output_ty(o);
        let load = match ty {
            Ty::Bool => format!("load_stored(slot, {k}u).x > 0.5"),
            _ => format!(
                "load_stored(slot, {k}u){}",
                swizzle(ty.width().unwrap_or(4))
            ),
        };
        let _ = writeln!(wgsl, "    let {} = {load};", Emitter::name(i, o));
    }
    for &i in &program.order {
        let in_step =
            step_set.contains(&i) && nodes[i].var != Var::Birth && nodes[i].def.role == Role::Value;
        if in_step || step_writers.contains(&i) {
            e.node(i, &mut wgsl);
        }
    }
    for &i in &emits {
        e.emit(channel(i), i, false, &mut wgsl);
    }
    wgsl.push_str(&store_state());
    wgsl.push_str(
        "    if (dying) {\n        p.alive = 0u;\n        store_particle(slot, p);\n        \
         hide_parts(slot);\n        return;\n    }\n",
    );
    for (r, &i) in renderers.iter().enumerate() {
        if nodes[i].def.kind == "ribbon" {
            let pin = |name: &str| {
                let at = nodes[i]
                    .def
                    .inputs
                    .iter()
                    .position(|p| p.name == name)
                    .expect("ribbon pin");
                e.input(i, at)
            };
            let _ = writeln!(
                wgsl,
                "    write_ribbon(slot, {r}u, p.index, {}, {}, {}, {}, {});",
                pin("position"),
                pin("width"),
                pin("colour"),
                pin("along"),
                pin("enabled"),
            );
            continue;
        }
        let pin = |name: &str| {
            let at = nodes[i]
                .def
                .inputs
                .iter()
                .position(|p| p.name == name)
                .expect("billboard pin");
            // Stretched along the velocity: unless wired, the axis is a twentieth of a
            // second of the particle's motion.
            if name == "axis"
                && nodes[i].text("mode") == "velocity"
                && matches!(e.sources[i][at], Source::Numbers(_))
            {
                return "(p.velocity * 0.05)".to_string();
            }
            e.input(i, at)
        };
        let _ = writeln!(
            wgsl,
            "    write_part(slot, {r}u, {}, {}, {}, {}, {}, {}, {});",
            pin("position"),
            pin("size"),
            pin("colour"),
            pin("axis"),
            pin("rotation"),
            pin("frame"),
            pin("enabled"),
        );
    }
    wgsl.push_str("    store_particle(slot, p);\n}\n");

    let channels = emits
        .iter()
        .map(|&i| {
            let n = &nodes[i];
            let number = |name: &str, fallback: f32| match n.input(name) {
                Source::Numbers(v) => v.first().copied().unwrap_or(fallback),
                Source::Wire(a, _) if nodes[*a].def.kind == "random" => {
                    match nodes[*a].input("max") {
                        Source::Numbers(v) => v.first().copied().unwrap_or(fallback),
                        _ => fallback,
                    }
                }
                _ => fallback,
            };
            Channel {
                node: n.id.clone(),
                when: match n.text("when") {
                    "birth" => When::Birth,
                    "interval" => When::Interval,
                    "distance" => When::Distance,
                    "step" => When::Step,
                    _ => When::Death,
                },
                interval: number("interval", 0.1).max(1e-4),
                max_count: number("count", 4.0).max(0.0),
            }
        })
        .collect();
    let spawners = program
        .of_role(Role::Spawner)
        .filter(|n| n.def.kind != "onEvent")
        .map(|n| {
            let first = |name: &str| n.numbers(name).first().copied().unwrap_or(0.0);
            match n.def.kind {
                "burst" => Spawner::Burst {
                    count: first("count"),
                    time: first("time"),
                },
                _ => Spawner::Rate {
                    rate: first("rate"),
                    start: first("start"),
                    duration: first("duration"),
                    curve: match n.input("curve") {
                        Source::Curve(c) => Some(std::array::from_fn(|i| {
                            let u = i as f64 / (RATE_SAMPLES - 1) as f64;
                            c.sample(u).first().copied().unwrap_or(1.0).max(0.0) as f32
                        })),
                        _ => None,
                    },
                },
            }
        })
        .collect();
    let renderers = renderers
        .iter()
        .map(|&i| {
            let n = &nodes[i];
            let ribbon = n.def.kind == "ribbon";
            let flipbook: &[f32] = if ribbon { &[] } else { n.numbers("flipbook") };
            Renderer {
                node: n.id.clone(),
                texture: n.text("texture").to_string(),
                mode: match if ribbon { "ribbon" } else { n.text("mode") } {
                    "ribbon" => Mode::Ribbon,
                    "axis" => Mode::Axis,
                    "spheroid" => Mode::Spheroid,
                    "plane" => Mode::Plane,
                    "velocity" => Mode::Velocity,
                    _ => Mode::Camera,
                },
                blend: match n.text("blend") {
                    "alpha" => Blend::Alpha,
                    "distort" => Blend::Distort,
                    _ => Blend::Additive,
                },
                soft: n.numbers("soft").first().copied().unwrap_or(0.0),
                flipbook: [
                    flipbook.first().copied().unwrap_or(1.0).max(1.0) as u32,
                    flipbook.get(1).copied().unwrap_or(1.0).max(1.0) as u32,
                ],
            }
        })
        .collect();
    Layer {
        wgsl,
        stride: HEADER_VEC4S + (channel_base + emits.len()) as u32,
        spawners,
        renderers,
        parameters: e.parameters,
        max_life: max_life(program),
        channels,
        parent,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::check::check;

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

    /// Holy Light's beam layer as decoded from Reforged (docs/effect-graph.md), in our
    /// coordinates (y up).
    const BEAMS: &str = r##"{
      "nodes": [
        {"id": "spawn", "kind": "rate", "inputs": {"rate": 20, "duration": 0.4}},
        {"id": "life", "kind": "random", "inputs": {"min": 0.4, "max": 0.6}},
        {"id": "setLife", "kind": "setLife"},
        {"id": "shape", "kind": "cylinder", "inputs": {"radius": 0.62}},
        {"id": "start", "kind": "setPosition"},
        {"id": "grow", "kind": "curve", "inputs": {"curve": {"keys": [
          {"t": 0, "v": [0.4117], "in": [0.2866], "out": [0.2866]},
          {"t": 1, "v": [1], "in": [1.1213], "out": [1.1213]}]}}},
        {"id": "length", "kind": "multiply", "inputs": {"b": 41.25}},
        {"id": "up", "kind": "vector", "inputs": {"value": [0, 1, 0]}},
        {"id": "axis", "kind": "multiply"},
        {"id": "half", "kind": "multiply", "inputs": {"b": 0.5}},
        {"id": "lift", "kind": "add"},
        {"id": "width", "kind": "random", "inputs": {"min": 0.31, "max": 1.25}},
        {"id": "colour", "kind": "curve", "inputs": {"curve": {"keys": [
          {"t": 0, "v": [0, 0, 0, 0]},
          {"t": 0.343, "v": [0.927, 0.701, 0.182, 0.831]},
          {"t": 1, "v": [0.741, 0.291, 0, 0]}]}}},
        {"id": "brighter", "kind": "hsvShift", "inputs": {"shift": [0, 0.038, 0.54]}},
        {"id": "where", "kind": "position"},
        {"id": "draw", "kind": "billboard", "inputs": {"texture": "asset.effects.wc3.gradient.softbeam-bw", "mode": "spheroid", "soft": 1}}
      ],
      "wires": [
        {"from": "life.value", "to": "setLife.life"},
        {"from": "shape.position", "to": "start.position"},
        {"from": "grow.value", "to": "length.a"},
        {"from": "up.value", "to": "axis.a"},
        {"from": "length.value", "to": "axis.b"},
        {"from": "axis.value", "to": "half.a"},
        {"from": "where.value", "to": "lift.a"},
        {"from": "half.value", "to": "lift.b"},
        {"from": "lift.value", "to": "draw.position"},
        {"from": "axis.value", "to": "draw.axis"},
        {"from": "width.value", "to": "draw.size"},
        {"from": "colour.value", "to": "brighter.colour"},
        {"from": "brighter.value", "to": "draw.colour"}
      ]
    }"##;

    #[test]
    fn a_decoded_reforged_layer_compiles_to_valid_wgsl() {
        let graph: crate::Graph = serde_json::from_str(BEAMS).unwrap();
        let program = check(&graph).unwrap_or_else(|p| panic!("{p:?}"));
        let layer = compile(&program, &[]);
        validate(&layer.wgsl);
        assert_eq!(
            layer.spawners,
            vec![Spawner::Rate {
                rate: 20.0,
                start: 0.0,
                duration: 0.4,
                curve: None,
            }]
        );
        assert_eq!(layer.renderers.len(), 1);
        assert_eq!(layer.renderers[0].mode, Mode::Spheroid);
        // The random width is read every step from the particle's record.
        assert_eq!(layer.stride, HEADER_VEC4S + 1);
        assert_eq!(layer.max_life, 0.6);
        // 20 a second for 0.4 s, all alive at once, with the margin.
        assert_eq!(layer.capacity(1.0), 18);
    }

    /// A rocket that bursts into sparks when it dies, and leaves a puff every 0.1 s.
    const ROCKET: &str = r##"{
      "nodes": [
        {"id": "spawn", "kind": "burst", "inputs": {"count": 3}},
        {"id": "life", "kind": "random", "inputs": {"min": 0.8, "max": 1.2}},
        {"id": "setLife", "kind": "setLife"},
        {"id": "up", "kind": "random", "inputs": {"min": [-2, 12, -2], "max": [2, 18, 2]}},
        {"id": "launch", "kind": "setVelocity"},
        {"id": "hot", "kind": "colour", "inputs": {"value": [2, 1, 0.4, 1]}},
        {"id": "pop", "kind": "emit", "inputs": {"when": "death", "count": 20}},
        {"id": "puff", "kind": "emit", "inputs": {"when": "interval", "interval": 0.1}},
        {"id": "fuel", "kind": "setAttribute", "inputs": {"name": "fuel", "value": 1}},
        {"id": "fuelLeft", "kind": "attribute", "inputs": {"name": "fuel"}},
        {"id": "burn", "kind": "subtract", "inputs": {"b": 0.01}},
        {"id": "burnt", "kind": "setAttribute", "inputs": {"name": "fuel"}},
        {"id": "lit", "kind": "compare", "inputs": {"op": ">"}},
        {"id": "draw", "kind": "billboard", "inputs": {"texture": "t"}}
      ],
      "wires": [
        {"from": "life.value", "to": "setLife.life"},
        {"from": "up.value", "to": "launch.velocity"},
        {"from": "hot.value", "to": "pop.colour"},
        {"from": "fuelLeft.x", "to": "burn.a"},
        {"from": "burn.value", "to": "burnt.value"},
        {"from": "fuelLeft.x", "to": "lit.a"},
        {"from": "lit.value", "to": "puff.condition"}
      ]
    }"##;

    const SPARKS: &str = r##"{
      "nodes": [
        {"id": "from", "kind": "onEvent", "inputs": {"layer": "rocket", "event": "pop"}},
        {"id": "payload", "kind": "event"},
        {"id": "dir", "kind": "randomDirection"},
        {"id": "speed", "kind": "multiply", "inputs": {"b": 9}},
        {"id": "throw", "kind": "setVelocity"},
        {"id": "drag", "kind": "drag", "inputs": {"strength": 2}},
        {"id": "life", "kind": "random", "inputs": {"min": 0.3, "max": 0.6}},
        {"id": "setLife", "kind": "setLife"},
        {"id": "wobble", "kind": "noise"},
        {"id": "draw", "kind": "billboard", "inputs": {"texture": "t", "mode": "velocity"}}
      ],
      "wires": [
        {"from": "dir.value", "to": "speed.a"},
        {"from": "speed.value", "to": "throw.velocity"},
        {"from": "life.value", "to": "setLife.life"},
        {"from": "payload.colour", "to": "draw.colour"},
        {"from": "wobble.value", "to": "draw.axis"}
      ]
    }"##;

    #[test]
    fn layers_born_of_events_compile_with_their_parents() {
        let rocket = compile(
            &check(&serde_json::from_str(ROCKET).unwrap()).unwrap_or_else(|p| panic!("{p:?}")),
            &[],
        );
        validate(&rocket.wgsl);
        assert_eq!(rocket.channels.len(), 2);
        assert_eq!(rocket.channels[0].node, "pop");
        assert_eq!(rocket.channels[0].when, When::Death);
        assert_eq!(rocket.channels[0].max_count, 20.0);
        assert_eq!(rocket.channels[1].when, When::Interval);
        // One attribute and two channel states after the header.
        assert_eq!(rocket.stride, HEADER_VEC4S + 1 + 2);
        assert!(rocket.parent.is_none());
        let sparks = compile(
            &check(&serde_json::from_str(SPARKS).unwrap()).unwrap_or_else(|p| panic!("{p:?}")),
            &[],
        );
        validate(&sparks.wgsl);
        assert_eq!(
            sparks.parent,
            Some(EventSource {
                layer: "rocket".into(),
                event: "pop".into()
            })
        );
        assert!(sparks.spawners.is_empty());
        // The rocket's puffs drawn as one ribbon per rocket.
        let trail = compile(
            &check(
                &serde_json::from_str(
                    r##"{"nodes": [
                      {"id": "from", "kind": "onEvent", "inputs": {"layer": "rocket", "event": "puff"}},
                      {"id": "life", "kind": "setLife", "inputs": {"life": 0.6}},
                      {"id": "stay", "kind": "parent"},
                      {"id": "out", "kind": "kill"},
                      {"id": "gone", "kind": "not"},
                      {"id": "draw", "kind": "ribbon", "inputs": {"texture": "t", "width": 0.4}}
                    ],
                    "wires": [
                      {"from": "stay.alive", "to": "gone.x"},
                      {"from": "gone.value", "to": "out.condition"}
                    ]}"##,
                )
                .unwrap(),
            )
            .unwrap_or_else(|p| panic!("{p:?}")),
            &[],
        );
        validate(&trail.wgsl);
        assert_eq!(trail.renderers[0].mode, Mode::Ribbon);
    }

    #[test]
    fn beams_run_between_the_ends_and_haze_bends_the_scene() {
        // A beam's points laid from the caster to the target by their share of the burst,
        // tinted by the team; and a haze ring.
        let beam = compile(
            &check(
                &serde_json::from_str(
                    r##"{"nodes": [
                      {"id": "spawn", "kind": "burst", "inputs": {"count": 40}},
                      {"id": "ends", "kind": "target"},
                      {"id": "batch", "kind": "event"},
                      {"id": "along", "kind": "lerp"},
                      {"id": "team", "kind": "teamColour"},
                      {"id": "draw", "kind": "ribbon", "inputs": {"texture": "t"}},
                      {"id": "haze", "kind": "billboard", "inputs": {"texture": "t", "blend": "distort", "mode": "plane"}}
                    ],
                    "wires": [
                      {"from": "ends.source", "to": "along.a"},
                      {"from": "ends.position", "to": "along.b"},
                      {"from": "batch.share", "to": "along.t"},
                      {"from": "along.value", "to": "draw.position"},
                      {"from": "team.value", "to": "draw.colour"}
                    ]}"##,
                )
                .unwrap(),
            )
            .unwrap_or_else(|p| panic!("{p:?}")),
            &[],
        );
        validate(&beam.wgsl);
        let haze = beam.renderers.iter().find(|r| r.node == "haze").unwrap();
        assert_eq!(haze.blend, Blend::Distort);
    }

    #[test]
    fn every_node_of_a_layer_previews_as_valid_wgsl() {
        let graph: crate::Graph = serde_json::from_str(BEAMS).unwrap();
        let program = check(&graph).unwrap();
        let mut kinds = (0, 0, 0);
        for i in 0..program.nodes.len() {
            let overrides = match preview(&program, i) {
                Preview::Finished => {
                    kinds.0 += 1;
                    Vec::new()
                }
                Preview::Bare(o) => {
                    kinds.1 += 1;
                    o
                }
                Preview::Feeds(o) => {
                    kinds.2 += 1;
                    o
                }
            };
            validate(&compile(&program, &overrides).wgsl);
        }
        // The billboard finishes; the spawner, life and shape show bare particles; the
        // curves, randoms and maths feed the billboard.
        assert_eq!(kinds.0, 1);
        assert!(kinds.1 >= 4 && kinds.2 >= 8, "{kinds:?}");
    }

    #[test]
    fn every_catalogue_node_compiles() {
        // One graph per value or writer node, its output (if any) feeding a billboard.
        for def in crate::catalogue::NODES {
            if matches!(def.role, Role::Spawner | Role::Renderer) {
                continue;
            }
            let mut inputs = serde_json::Map::new();
            for pin in def.inputs {
                let value = match pin.ty {
                    Ty::Curve => {
                        serde_json::json!({"keys": [{"t": 0, "v": [0]}, {"t": 0.4, "v": [2], "in": [1], "out": [1]}, {"t": 1, "v": [1]}]})
                    }
                    Ty::Text if !pin.choices.is_empty() => serde_json::json!(pin.choices[0]),
                    Ty::Text => serde_json::json!("radius"),
                    Ty::F2 => serde_json::json!([1, 2]),
                    Ty::F3 => serde_json::json!([1, 2, 3]),
                    Ty::F4 => serde_json::json!([1, 2, 3, 4]),
                    _ => serde_json::json!(0.5),
                };
                inputs.insert(pin.name.into(), value);
            }
            let mut wires = Vec::new();
            if let Some(out) = def.outputs.first() {
                let pin = match out.ty {
                    Ty::F3 => "position",
                    Ty::F4 => "colour",
                    _ => "size",
                };
                wires.push(serde_json::json!({"from": format!("n.{}", out.name), "to": format!("draw.{pin}")}));
            }
            let graph: crate::Graph = serde_json::from_value(serde_json::json!({
                "nodes": [
                    {"id": "n", "kind": def.kind, "inputs": inputs},
                    {"id": "draw", "kind": "billboard", "inputs": {"texture": "t"}}
                ],
                "wires": wires
            }))
            .unwrap();
            let program = check(&graph).unwrap_or_else(|p| panic!("{}: {p:?}", def.kind));
            validate(&compile(&program, &[]).wgsl);
        }
    }
}
