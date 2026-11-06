//! Every node an effect graph may use: its pins, what it varies with, and the WGSL it
//! compiles to. The checker, the compiler, the editor's palette and its forms all read
//! this one table.
//!
//! Code templates name inputs as `{pin}`, the node's numeric width as `{W}` (1–4, for
//! the per-width helpers in `wgsl/prelude.wgsl`) and its WGSL type as `{T}`.

/// A pin's value type. `Num` is any width 1–4: a node's `Num` inputs agree on the widest
/// of them (single numbers broadcast) and its `Num` outputs take that width.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ty {
    F1,
    F2,
    F3,
    F4,
    Num,
    Bool,
    /// A curve value (only as an unwired setting).
    Curve,
    /// A choice or an asset id (only as an unwired setting).
    Text,
}

impl Ty {
    /// Components of a numeric type (1 for `Num` until resolved).
    pub fn width(self) -> Option<u8> {
        match self {
            Ty::F1 | Ty::Num | Ty::Bool => Some(1),
            Ty::F2 => Some(2),
            Ty::F3 => Some(3),
            Ty::F4 => Some(4),
            Ty::Curve | Ty::Text => None,
        }
    }

    pub fn of_width(width: u8) -> Ty {
        match width {
            1 => Ty::F1,
            2 => Ty::F2,
            3 => Ty::F3,
            _ => Ty::F4,
        }
    }

    pub fn wgsl(self) -> &'static str {
        match self {
            Ty::F1 | Ty::Num => "f32",
            Ty::F2 => "vec2f",
            Ty::F3 => "vec3f",
            Ty::F4 => "vec4f",
            Ty::Bool => "bool",
            Ty::Curve | Ty::Text => "",
        }
    }
}

/// How often a value can change: the compiler evaluates a node at the latest of its
/// inputs' and its own variability.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Var {
    /// The same for every particle at every step.
    Const,
    /// Fixed for a particle from its birth (randoms, spawn shapes, the spawn index).
    Birth,
    /// Changes as the particle lives (its age, position, a curve of its life).
    Step,
}

/// What an unwired input takes when the node gives it no value.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Fallback {
    /// Required: the node is incomplete without a wire or a value.
    Required,
    Numbers(&'static [f32]),
    /// A particle attribute (`lifeRatio`, `position` …), read at the node's stage.
    Attribute(&'static str),
    Text(&'static str),
}

#[derive(Debug, Clone, Copy)]
pub struct Pin {
    pub name: &'static str,
    pub ty: Ty,
    pub default: Fallback,
    /// Takes a wire. Settings (curves, choices, textures) are values only.
    pub wired: bool,
    /// The choices of a `Text` setting; empty: free text (an asset id).
    pub choices: &'static [&'static str],
}

const fn input(name: &'static str, ty: Ty, default: Fallback) -> Pin {
    Pin {
        name,
        ty,
        default,
        wired: true,
        choices: &[],
    }
}

const fn setting(name: &'static str, ty: Ty, default: Fallback) -> Pin {
    Pin {
        name,
        ty,
        default,
        wired: false,
        choices: &[],
    }
}

const fn choice(
    name: &'static str,
    default: &'static str,
    choices: &'static [&'static str],
) -> Pin {
    Pin {
        name,
        ty: Ty::Text,
        default: Fallback::Text(default),
        wired: false,
        choices,
    }
}

const fn output(name: &'static str, ty: Ty) -> Pin {
    Pin {
        name,
        ty,
        default: Fallback::Required,
        wired: true,
        choices: &[],
    }
}

/// What a node is for, which decides how the compiler treats it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// Computes outputs from inputs.
    Value,
    /// Decides when particles are born; evaluated on the CPU, no outputs.
    Spawner,
    /// Writes a particle attribute or acts on the particle (forces); no outputs.
    Writer,
    /// Draws the particles from its inputs.
    Renderer,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Group {
    Spawn,
    Events,
    Particle,
    Values,
    Random,
    Shapes,
    Curves,
    Colour,
    Maths,
    Forces,
    Render,
}

impl Group {
    pub fn title(self) -> &'static str {
        match self {
            Group::Spawn => "Spawn",
            Group::Events => "Events",
            Group::Particle => "Particle",
            Group::Values => "Values",
            Group::Random => "Random",
            Group::Shapes => "Shapes",
            Group::Curves => "Curves",
            Group::Colour => "Colour",
            Group::Maths => "Maths",
            Group::Forces => "Forces",
            Group::Render => "Render",
        }
    }
}

/// How a node becomes WGSL.
#[derive(Debug, Clone, Copy)]
pub enum Code {
    /// One expression for the node's single output.
    Expr(&'static str),
    /// Statements run for their effect (writers).
    Stmt(&'static str),
    /// The compiler handles it itself (constants, curves, spawners, renderers).
    Builtin,
}

#[derive(Debug, Clone, Copy)]
pub struct NodeDef {
    pub kind: &'static str,
    pub title: &'static str,
    pub group: Group,
    pub role: Role,
    pub inputs: &'static [Pin],
    pub outputs: &'static [Pin],
    /// Its own variability, before its inputs': randoms are `Birth`, `age` is `Step`.
    pub var: Var,
    pub code: Code,
    /// One line for the palette's tooltip.
    pub help: &'static str,
}

/// The node `kind`, if the catalogue has it.
pub fn find(kind: &str) -> Option<&'static NodeDef> {
    NODES.iter().find(|n| n.kind == kind)
}

use Fallback::{Attribute, Numbers, Required};
use Ty::{Bool, F1, F2, F3, F4, Num};

const VALUE: &[Pin] = &[output("value", Num)];
const VALUE1: &[Pin] = &[output("value", F1)];
const VALUE3: &[Pin] = &[output("value", F3)];
const VALUE4: &[Pin] = &[output("value", F4)];
const BOOL: &[Pin] = &[output("value", Bool)];
const AB: &[Pin] = &[input("a", Num, Required), input("b", Num, Required)];
const X: &[Pin] = &[input("x", Num, Required)];
const NONE: &[Pin] = &[];

macro_rules! maths {
    ($kind:literal, $title:literal, $inputs:expr, $expr:literal, $help:literal) => {
        NodeDef {
            kind: $kind,
            title: $title,
            group: Group::Maths,
            role: Role::Value,
            inputs: $inputs,
            outputs: VALUE,
            var: Var::Const,
            code: Code::Expr($expr),
            help: $help,
        }
    };
}

/// The catalogue.
pub static NODES: &[NodeDef] = &[
    // Spawn.
    NodeDef {
        kind: "burst",
        title: "Burst",
        group: Group::Spawn,
        role: Role::Spawner,
        inputs: &[
            setting("count", F1, Numbers(&[10.0])),
            setting("time", F1, Numbers(&[0.0])),
        ],
        outputs: NONE,
        var: Var::Const,
        code: Code::Builtin,
        help: "Particles born at once, `time` seconds into the effect.",
    },
    NodeDef {
        kind: "rate",
        title: "Rate",
        group: Group::Spawn,
        role: Role::Spawner,
        inputs: &[
            setting("rate", F1, Numbers(&[10.0])),
            setting("start", F1, Numbers(&[0.0])),
            setting("duration", F1, Numbers(&[0.0])),
            setting("curve", Ty::Curve, Numbers(&[1.0])),
        ],
        outputs: NONE,
        var: Var::Const,
        code: Code::Builtin,
        help: "Particles born per second from `start` for `duration` seconds (0: while the effect plays), times `curve` over that span (a spray that tapers off).",
    },
    // Events between layers (D113).
    NodeDef {
        kind: "emit",
        title: "Emit event",
        group: Group::Events,
        role: Role::Writer,
        inputs: &[
            choice(
                "when",
                "death",
                &["death", "birth", "interval", "distance", "step"],
            ),
            input("interval", F1, Numbers(&[0.1])),
            input("count", F1, Numbers(&[1.0])),
            input("condition", Bool, Numbers(&[1.0])),
            input("position", F3, Attribute("position")),
            input("velocity", F3, Attribute("velocity")),
            input("colour", F4, Numbers(&[1.0, 1.0, 1.0, 1.0])),
            input("value", F4, Numbers(&[0.0, 0.0, 0.0, 0.0])),
        ],
        outputs: NONE,
        var: Var::Const,
        code: Code::Builtin,
        help: "Spawns `count` particles of the layers listening to this node: when a particle dies, is born, every `interval` seconds or metres travelled, or every step; `condition` gates it. The payload goes to the children.",
    },
    NodeDef {
        kind: "onEvent",
        title: "On event",
        group: Group::Events,
        role: Role::Spawner,
        inputs: &[
            setting("layer", Ty::Text, Required),
            setting("event", Ty::Text, Required),
        ],
        outputs: NONE,
        var: Var::Const,
        code: Code::Builtin,
        help: "Particles born from another layer's events: `layer` is its id, `event` its Emit event node's id. They start where the event happened.",
    },
    NodeDef {
        kind: "event",
        title: "Event payload",
        group: Group::Events,
        role: Role::Value,
        inputs: NONE,
        outputs: &[
            output("position", F3),
            output("velocity", F3),
            output("colour", F4),
            output("value", F4),
            output("order", F1),
            output("share", F1),
        ],
        var: Var::Birth,
        code: Code::Builtin,
        help: "What the event that bore this particle carried (zero for particles not born of an event), and where it came in its batch: `order` 0, 1, 2 … and `share` 0 at the first to 1 at the last (a burst's particles are a batch too): lay a beam's points along it.",
    },
    // Particle attributes and writers.
    NodeDef {
        kind: "lifeRatio",
        title: "Life ratio",
        group: Group::Particle,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE1,
        var: Var::Step,
        code: Code::Expr("p_life_ratio(p)"),
        help: "0 at birth, 1 at death.",
    },
    NodeDef {
        kind: "age",
        title: "Age",
        group: Group::Particle,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE1,
        var: Var::Step,
        code: Code::Expr("max(p.age, 0.0)"),
        help: "Seconds since the particle was born.",
    },
    NodeDef {
        kind: "life",
        title: "Life",
        group: Group::Particle,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE1,
        var: Var::Birth,
        code: Code::Expr("p.life"),
        help: "The particle's lifespan in seconds.",
    },
    NodeDef {
        kind: "position",
        title: "Position",
        group: Group::Particle,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE3,
        var: Var::Step,
        code: Code::Expr("p.position"),
        help: "The particle's position, relative to the emitter.",
    },
    NodeDef {
        kind: "velocity",
        title: "Velocity",
        group: Group::Particle,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE3,
        var: Var::Step,
        code: Code::Expr("p.velocity"),
        help: "Metres per second.",
    },
    NodeDef {
        kind: "spawnIndex",
        title: "Spawn index",
        group: Group::Particle,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE1,
        var: Var::Birth,
        code: Code::Expr("f32(p.index)"),
        help: "0 for the layer's first particle, then 1, 2 …",
    },
    NodeDef {
        kind: "effectTime",
        title: "Effect time",
        group: Group::Particle,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE1,
        var: Var::Step,
        code: Code::Expr("effect_time"),
        help: "Seconds since the effect started.",
    },
    NodeDef {
        kind: "parent",
        title: "Parent",
        group: Group::Events,
        role: Role::Value,
        inputs: NONE,
        outputs: &[
            output("position", F3),
            output("velocity", F3),
            output("alive", Bool),
        ],
        var: Var::Step,
        code: Code::Builtin,
        help: "The particle whose event bore this one, as it is now: follow it, or die with it.",
    },
    NodeDef {
        kind: "target",
        title: "Ends",
        group: Group::Values,
        role: Role::Value,
        inputs: NONE,
        outputs: &[output("position", F3), output("source", F3)],
        var: Var::Const,
        code: Code::Builtin,
        help: "Where the effect's target and caster are now, in the emitter's space: beams, drains and links run between them.",
    },
    NodeDef {
        kind: "emitter",
        title: "Emitter",
        group: Group::Values,
        role: Role::Value,
        inputs: NONE,
        outputs: &[output("position", F3)],
        var: Var::Const,
        code: Code::Expr("u.carrier.xyz"),
        help: "Where the emitter is now, in the effect's space: a missile's head as it flies (0 for an effect that stays put). Particles born there stay behind as a trail; parts placed there every step ride along.",
    },
    NodeDef {
        kind: "teamColour",
        title: "Team colour",
        group: Group::Colour,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE4,
        var: Var::Const,
        code: Code::Expr("u.team"),
        help: "The owner's player colour (white when it has none).",
    },
    NodeDef {
        kind: "kill",
        title: "Kill",
        group: Group::Particle,
        role: Role::Writer,
        inputs: &[input("condition", Bool, Numbers(&[1.0]))],
        outputs: NONE,
        var: Var::Step,
        code: Code::Stmt("dying = dying || ({condition});"),
        help: "Ends the particle this step when the condition holds (its death events fire).",
    },
    NodeDef {
        kind: "groundHit",
        title: "Ground hit",
        group: Group::Forces,
        role: Role::Value,
        inputs: &[
            input("origin", F3, Attribute("position")),
            input("direction", F3, Numbers(&[0.0, -1.0, 0.0])),
            input("length", F1, Numbers(&[1.0])),
        ],
        outputs: VALUE4,
        var: Var::Const,
        code: Code::Expr("ground_hit({origin}, {direction}, {length})"),
        help: "Where a ray meets the ground (the emitter's plane y = 0): the ground's normal and the distance, 0 when it misses.",
    },
    NodeDef {
        kind: "effectRunning",
        title: "Effect running",
        group: Group::Particle,
        role: Role::Value,
        inputs: NONE,
        outputs: BOOL,
        var: Var::Step,
        code: Code::Expr("(u.running != 0u)"),
        help: "Whether the effect still plays (false once its cue ended and its particles live out).",
    },
    NodeDef {
        kind: "setLife",
        title: "Set life",
        group: Group::Particle,
        role: Role::Writer,
        inputs: &[input("life", F1, Numbers(&[1.0]))],
        outputs: NONE,
        var: Var::Birth,
        code: Code::Stmt("p.life = max({life}, 0.001);"),
        help: "The particle's lifespan in seconds, set at birth.",
    },
    NodeDef {
        kind: "setPosition",
        title: "Set position",
        group: Group::Particle,
        role: Role::Writer,
        inputs: &[input("position", F3, Required)],
        outputs: NONE,
        var: Var::Const,
        code: Code::Stmt("p.position = {position};"),
        help: "At birth: where the particle starts. Wired to something that changes: where it is.",
    },
    NodeDef {
        kind: "setVelocity",
        title: "Set velocity",
        group: Group::Particle,
        role: Role::Writer,
        inputs: &[input("velocity", F3, Required)],
        outputs: NONE,
        var: Var::Const,
        code: Code::Stmt("p.velocity = {velocity};"),
        help: "At birth: the starting velocity. Wired to something that changes: the velocity.",
    },
    NodeDef {
        kind: "setAttribute",
        title: "Set attribute",
        group: Group::Particle,
        role: Role::Writer,
        inputs: &[
            setting("name", Ty::Text, Required),
            input("value", Num, Required),
        ],
        outputs: NONE,
        var: Var::Const,
        code: Code::Builtin,
        help: "Stores a value of the particle's own under `name`: at birth when wired to something fixed at birth, every step when wired to something that changes (read it back with Attribute).",
    },
    NodeDef {
        kind: "attribute",
        title: "Attribute",
        group: Group::Particle,
        role: Role::Value,
        inputs: &[setting("name", Ty::Text, Required)],
        outputs: &[output("value", F4), output("xyz", F3), output("x", F1)],
        var: Var::Step,
        code: Code::Builtin,
        help: "A value Set attribute stored on the particle (as it was before this step's Set).",
    },
    // Values.
    NodeDef {
        kind: "float",
        title: "Number",
        group: Group::Values,
        role: Role::Value,
        inputs: &[setting("value", F1, Numbers(&[0.0]))],
        outputs: VALUE1,
        var: Var::Const,
        code: Code::Builtin,
        help: "A number.",
    },
    NodeDef {
        kind: "vector",
        title: "Vector",
        group: Group::Values,
        role: Role::Value,
        inputs: &[setting("value", F3, Numbers(&[0.0, 0.0, 0.0]))],
        outputs: VALUE3,
        var: Var::Const,
        code: Code::Builtin,
        help: "Three numbers: a direction, offset or size.",
    },
    NodeDef {
        kind: "colour",
        title: "Colour",
        group: Group::Colour,
        role: Role::Value,
        inputs: &[setting("value", F4, Numbers(&[1.0, 1.0, 1.0, 1.0]))],
        outputs: VALUE4,
        var: Var::Const,
        code: Code::Builtin,
        help: "`#rrggbb` or linear RGBA; values above 1 glow.",
    },
    NodeDef {
        kind: "parameter",
        title: "Parameter",
        group: Group::Values,
        role: Role::Value,
        inputs: &[setting("name", Ty::Text, Fallback::Text("radius"))],
        outputs: VALUE1,
        var: Var::Const,
        code: Code::Builtin,
        help: "One of the effect's parameters (radius, duration …), filled by whoever plays it.",
    },
    // Random.
    NodeDef {
        kind: "random",
        title: "Random",
        group: Group::Random,
        role: Role::Value,
        inputs: &[
            input("min", Num, Numbers(&[0.0])),
            input("max", Num, Numbers(&[1.0])),
        ],
        outputs: VALUE,
        var: Var::Birth,
        code: Code::Expr("rand{W}(&rng, {min}, {max})"),
        help: "A random value between min and max, per particle (per component for vectors).",
    },
    NodeDef {
        kind: "randomDirection",
        title: "Random direction",
        group: Group::Random,
        role: Role::Value,
        inputs: NONE,
        outputs: VALUE3,
        var: Var::Birth,
        code: Code::Expr("rand_direction(&rng)"),
        help: "A random unit vector, evenly over the sphere.",
    },
    // Shapes (positions sampled at birth, y up).
    NodeDef {
        kind: "cylinder",
        title: "Cylinder",
        group: Group::Shapes,
        role: Role::Value,
        inputs: &[
            input("radius", F1, Numbers(&[1.0])),
            input("height", F1, Numbers(&[0.0])),
            input("inner", F1, Numbers(&[0.0])),
        ],
        outputs: &[output("position", F3)],
        var: Var::Birth,
        code: Code::Expr("sample_cylinder(&rng, {radius}, {height}, {inner})"),
        help: "A point in an upright cylinder from the ground up to `height`; `inner` 0–1 hollows it.",
    },
    NodeDef {
        kind: "sphere",
        title: "Sphere",
        group: Group::Shapes,
        role: Role::Value,
        inputs: &[
            input("radius", F1, Numbers(&[1.0])),
            input("inner", F1, Numbers(&[0.0])),
        ],
        outputs: &[output("position", F3)],
        var: Var::Birth,
        code: Code::Expr("sample_sphere(&rng, {radius}, {inner})"),
        help: "A point in a sphere; `inner` 0–1 hollows it (1: on the surface).",
    },
    NodeDef {
        kind: "box",
        title: "Box",
        group: Group::Shapes,
        role: Role::Value,
        inputs: &[input("size", F3, Numbers(&[1.0, 1.0, 1.0]))],
        outputs: &[output("position", F3)],
        var: Var::Birth,
        code: Code::Expr("sample_box(&rng, {size})"),
        help: "A point in a box centred on the emitter.",
    },
    NodeDef {
        kind: "ellipsoid",
        title: "Ellipsoid",
        group: Group::Shapes,
        role: Role::Value,
        inputs: &[
            input("radius", F3, Numbers(&[1.0, 1.0, 1.0])),
            input("inner", F1, Numbers(&[0.0])),
        ],
        outputs: &[output("position", F3)],
        var: Var::Birth,
        code: Code::Expr("sample_ellipsoid(&rng, {radius}, {inner})"),
        help: "A point in an ellipsoid of these radii; `inner` 0–1 hollows it.",
    },
    NodeDef {
        kind: "noise",
        title: "Noise",
        group: Group::Random,
        role: Role::Value,
        inputs: &[
            input("position", F3, Attribute("position")),
            input("scale", F1, Numbers(&[0.5])),
            input("speed", F1, Numbers(&[1.0])),
        ],
        outputs: VALUE3,
        var: Var::Const,
        code: Code::Expr("noise3(({position}) * {scale}, effect_time * {speed})"),
        help: "Smooth noise in −1…1 around the position, drifting with time: wire it to an acceleration for turbulence.",
    },
    // Curves.
    NodeDef {
        kind: "curve",
        title: "Curve",
        group: Group::Curves,
        role: Role::Value,
        inputs: &[
            setting("curve", Ty::Curve, Required),
            input("x", F1, Attribute("lifeRatio")),
        ],
        outputs: VALUE,
        var: Var::Const,
        code: Code::Builtin,
        help: "The curve's value at x (the particle's life ratio unless wired).",
    },
    // Colour.
    NodeDef {
        kind: "hsvShift",
        title: "HSV shift",
        group: Group::Colour,
        role: Role::Value,
        inputs: &[
            input("colour", F4, Required),
            input("shift", Num, Numbers(&[0.0, 0.0, 0.0])),
        ],
        outputs: VALUE4,
        var: Var::Const,
        code: Code::Expr("hsv_shift{W}({colour}, {shift})"),
        help: "Adds to the colour's hue (0–1 is a full turn), saturation and value; alpha is kept.",
    },
    NodeDef {
        kind: "rgbToHsv",
        title: "RGB to HSV",
        group: Group::Colour,
        role: Role::Value,
        inputs: &[input("colour", F4, Required)],
        outputs: VALUE4,
        var: Var::Const,
        code: Code::Expr("rgb_to_hsv4({colour})"),
        help: "Hue 0–1, saturation 0–1 and value in the first three components; alpha kept.",
    },
    NodeDef {
        kind: "hsvToRgb",
        title: "HSV to RGB",
        group: Group::Colour,
        role: Role::Value,
        inputs: &[input("colour", F4, Required)],
        outputs: VALUE4,
        var: Var::Const,
        code: Code::Expr("hsv_to_rgb4({colour})"),
        help: "Back from hue, saturation and value; alpha kept.",
    },
    // Maths.
    maths!("add", "Add", AB, "({a} + {b})", "a + b"),
    maths!("subtract", "Subtract", AB, "({a} - {b})", "a − b"),
    maths!("multiply", "Multiply", AB, "({a} * {b})", "a × b"),
    maths!(
        "divide",
        "Divide",
        AB,
        "safe_div{W}({a}, {b})",
        "a ÷ b (0 where b is 0)"
    ),
    maths!("min", "Min", AB, "min({a}, {b})", "The smaller of a and b"),
    maths!("max", "Max", AB, "max({a}, {b})", "The larger of a and b"),
    maths!(
        "lerp",
        "Lerp",
        &[
            input("a", Num, Required),
            input("b", Num, Required),
            input("t", Num, Numbers(&[0.5]))
        ],
        "mix({a}, {b}, {t})",
        "a at t = 0, b at t = 1"
    ),
    maths!(
        "clamp",
        "Clamp",
        &[
            input("x", Num, Required),
            input("min", Num, Numbers(&[0.0])),
            input("max", Num, Numbers(&[1.0]))
        ],
        "clamp({x}, {min}, {max})",
        "x held between min and max"
    ),
    maths!(
        "remap",
        "Remap",
        &[
            input("x", Num, Required),
            input("fromMin", Num, Numbers(&[0.0])),
            input("fromMax", Num, Numbers(&[1.0])),
            input("toMin", Num, Numbers(&[0.0])),
            input("toMax", Num, Numbers(&[1.0])),
        ],
        "({toMin} + ({toMax} - {toMin}) * safe_div{W}({x} - {fromMin}, {fromMax} - {fromMin}))",
        "x moved from one range to another"
    ),
    NodeDef {
        kind: "compare",
        title: "Compare",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[
            input("a", F1, Required),
            input("b", F1, Numbers(&[0.0])),
            choice("op", "<", &["<", "<=", ">", ">=", "==", "!="]),
        ],
        outputs: BOOL,
        var: Var::Const,
        code: Code::Builtin,
        help: "Whether a compares to b as `op` says.",
    },
    NodeDef {
        kind: "select",
        title: "Select",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[
            input("condition", Bool, Required),
            input("a", Num, Required),
            input("b", Num, Required),
        ],
        outputs: VALUE,
        var: Var::Const,
        code: Code::Expr("select({b}, {a}, {condition})"),
        help: "a where the condition holds, else b.",
    },
    NodeDef {
        kind: "and",
        title: "And",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[input("a", Bool, Required), input("b", Bool, Required)],
        outputs: BOOL,
        var: Var::Const,
        code: Code::Expr("({a} && {b})"),
        help: "Both hold.",
    },
    NodeDef {
        kind: "or",
        title: "Or",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[input("a", Bool, Required), input("b", Bool, Required)],
        outputs: BOOL,
        var: Var::Const,
        code: Code::Expr("({a} || {b})"),
        help: "Either holds.",
    },
    NodeDef {
        kind: "not",
        title: "Not",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[input("x", Bool, Required)],
        outputs: BOOL,
        var: Var::Const,
        code: Code::Expr("(!{x})"),
        help: "The opposite.",
    },
    maths!("abs", "Absolute", X, "abs({x})", "|x|"),
    maths!("negate", "Negate", X, "(-{x})", "−x"),
    maths!("oneMinus", "One minus", X, "(1.0 - {x})", "1 − x"),
    maths!("sin", "Sine", X, "sin({x})", "sin x (radians)"),
    maths!("cos", "Cosine", X, "cos({x})", "cos x (radians)"),
    maths!("sqrt", "Square root", X, "sqrt(max({x}, {T}(0.0)))", "√x"),
    maths!("exp", "Exponential", X, "exp({x})", "eˣ"),
    maths!(
        "fract",
        "Fraction",
        X,
        "fract({x})",
        "x − floor(x): loops 0 → 1"
    ),
    maths!("floor", "Floor", X, "floor({x})", "x rounded down"),
    maths!(
        "pow",
        "Power",
        &[
            input("x", Num, Required),
            input("exponent", Num, Numbers(&[2.0]))
        ],
        "pow(max({x}, {T}(0.0)), {exponent})",
        "xⁿ"
    ),
    NodeDef {
        kind: "length",
        title: "Length",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[input("x", Num, Required)],
        outputs: VALUE1,
        var: Var::Const,
        code: Code::Expr("length({x})"),
        help: "A vector's length.",
    },
    NodeDef {
        kind: "normalize",
        title: "Normalize",
        group: Group::Maths,
        role: Role::Value,
        inputs: X,
        outputs: VALUE,
        var: Var::Const,
        code: Code::Expr("safe_normalize{W}({x})"),
        help: "The vector scaled to length 1.",
    },
    NodeDef {
        kind: "dot",
        title: "Dot",
        group: Group::Maths,
        role: Role::Value,
        inputs: AB,
        outputs: VALUE1,
        var: Var::Const,
        code: Code::Expr("dot{W}({a}, {b})"),
        help: "a · b",
    },
    NodeDef {
        kind: "cross",
        title: "Cross",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[input("a", F3, Required), input("b", F3, Required)],
        outputs: VALUE3,
        var: Var::Const,
        code: Code::Expr("cross({a}, {b})"),
        help: "a × b",
    },
    NodeDef {
        kind: "compose",
        title: "Compose",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[
            input("x", F1, Numbers(&[0.0])),
            input("y", F1, Numbers(&[0.0])),
            input("z", F1, Numbers(&[0.0])),
            input("w", F1, Numbers(&[1.0])),
        ],
        outputs: VALUE4,
        var: Var::Const,
        code: Code::Expr("vec4f({x}, {y}, {z}, {w})"),
        help: "Four numbers into a vector or colour.",
    },
    NodeDef {
        kind: "makeVector",
        title: "Make vector",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[
            input("x", F1, Numbers(&[0.0])),
            input("y", F1, Numbers(&[0.0])),
            input("z", F1, Numbers(&[0.0])),
        ],
        outputs: VALUE3,
        var: Var::Const,
        code: Code::Expr("vec3f({x}, {y}, {z})"),
        help: "Three numbers into a vector.",
    },
    NodeDef {
        kind: "split",
        title: "Split",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[input("value", Num, Required)],
        outputs: &[
            output("x", F1),
            output("y", F1),
            output("z", F1),
            output("w", F1),
        ],
        var: Var::Const,
        code: Code::Builtin,
        help: "A vector's components.",
    },
    NodeDef {
        kind: "rotate",
        title: "Rotate",
        group: Group::Maths,
        role: Role::Value,
        inputs: &[
            input("vector", F3, Required),
            input("axis", F3, Numbers(&[0.0, 1.0, 0.0])),
            input("angle", F1, Numbers(&[0.0])),
        ],
        outputs: VALUE3,
        var: Var::Const,
        code: Code::Expr("rotate_axis({vector}, {axis}, {angle})"),
        help: "The vector turned `angle` radians around the axis.",
    },
    // Forces (every step, after the update's nodes; positions then move by velocity).
    NodeDef {
        kind: "acceleration",
        title: "Acceleration",
        group: Group::Forces,
        role: Role::Writer,
        inputs: &[input("acceleration", F3, Numbers(&[0.0, -9.8, 0.0]))],
        outputs: NONE,
        var: Var::Step,
        code: Code::Stmt("p.velocity += {acceleration} * dt;"),
        help: "Gravity or any constant push, metres per second squared.",
    },
    NodeDef {
        kind: "drag",
        title: "Drag",
        group: Group::Forces,
        role: Role::Writer,
        inputs: &[input("strength", F1, Numbers(&[1.0]))],
        outputs: NONE,
        var: Var::Step,
        code: Code::Stmt("p.velocity *= exp(-{strength} * dt);"),
        help: "Slows the particle exponentially.",
    },
    NodeDef {
        kind: "orbit",
        title: "Orbit",
        group: Group::Forces,
        role: Role::Writer,
        inputs: &[
            input("speed", F1, Numbers(&[1.0])),
            input("axis", F3, Numbers(&[0.0, 1.0, 0.0])),
        ],
        outputs: NONE,
        var: Var::Step,
        code: Code::Stmt(
            "p.position = rotate_axis(p.position, {axis}, {speed} * dt); p.velocity = rotate_axis(p.velocity, {axis}, {speed} * dt);",
        ),
        help: "Swirls the particle around the emitter's axis, `speed` radians per second.",
    },
    // Renderers.
    NodeDef {
        kind: "billboard",
        title: "Billboard",
        group: Group::Render,
        role: Role::Renderer,
        inputs: &[
            input("position", F3, Attribute("position")),
            input("size", F1, Numbers(&[1.0])),
            input("colour", F4, Numbers(&[1.0, 1.0, 1.0, 1.0])),
            input("rotation", F1, Numbers(&[0.0])),
            input("axis", F3, Numbers(&[0.0, 1.0, 0.0])),
            input("frame", F1, Numbers(&[0.0])),
            input("enabled", Bool, Numbers(&[1.0])),
            setting("texture", Ty::Text, Required),
            choice(
                "mode",
                "camera",
                &["camera", "axis", "spheroid", "plane", "velocity"],
            ),
            choice("blend", "additive", &["additive", "alpha", "distort"]),
            setting("soft", F1, Numbers(&[0.0])),
            setting("flipbook", F2, Numbers(&[1.0, 1.0])),
        ],
        outputs: NONE,
        var: Var::Step,
        code: Code::Builtin,
        help: "Textured sprites. Modes: facing the camera; stretched along `axis` (`axis`, or `spheroid` widening with the view); flat across `axis` (`plane`); stretched along the velocity. Blend `distort` bends what is behind (heat haze, shock waves): the texture's red and green push the view, the colour's alpha says how far.",
    },
    NodeDef {
        kind: "ribbon",
        title: "Ribbon",
        group: Group::Render,
        role: Role::Renderer,
        inputs: &[
            input("position", F3, Attribute("position")),
            input("width", F1, Numbers(&[1.0])),
            input("colour", F4, Numbers(&[1.0, 1.0, 1.0, 1.0])),
            input("along", F1, Attribute("lifeRatio")),
            input("enabled", Bool, Numbers(&[1.0])),
            setting("texture", Ty::Text, Required),
            choice("blend", "additive", &["additive", "alpha"]),
            setting("soft", F1, Numbers(&[0.0])),
        ],
        outputs: NONE,
        var: Var::Step,
        code: Code::Builtin,
        help: "A strip through the layer's particles in the order they were born (each parent's children apart): trails and beams. `along` is the texture's coordinate down its length.",
    },
];

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_catalogue_is_well_formed() {
        let mut kinds = BTreeSet::new();
        for node in NODES {
            assert!(kinds.insert(node.kind), "{} twice", node.kind);
            // Inputs and outputs are separate: a wire's `from` is an output, its `to` an input.
            for pins in [node.inputs, node.outputs] {
                let mut names = BTreeSet::new();
                for pin in pins {
                    assert!(names.insert(pin.name), "{}.{} twice", node.kind, pin.name);
                }
            }
            for pin in node.inputs {
                assert!(
                    pin.wired || matches!(pin.ty, Ty::Curve | Ty::Text | F1 | F2 | F3 | F4),
                    "{}.{}",
                    node.kind,
                    pin.name
                );
                if let Fallback::Text(choice) = pin.default
                    && !pin.choices.is_empty()
                {
                    assert!(pin.choices.contains(&choice), "{}.{}", node.kind, pin.name);
                }
                if let Fallback::Attribute(attribute) = pin.default {
                    assert!(find(attribute).is_some(), "{}.{}", node.kind, pin.name);
                }
            }
            let template = match node.code {
                Code::Expr(t) | Code::Stmt(t) => t,
                Code::Builtin => continue,
            };
            for part in template.split('{').skip(1) {
                let name = part.split('}').next().unwrap();
                assert!(
                    name == "W" || name == "T" || node.inputs.iter().any(|p| p.name == name),
                    "{}: {{{name}}} is no input",
                    node.kind
                );
            }
            match node.role {
                Role::Value => assert!(!node.outputs.is_empty(), "{}", node.kind),
                _ => assert!(node.outputs.is_empty(), "{}", node.kind),
            }
        }
    }
}
