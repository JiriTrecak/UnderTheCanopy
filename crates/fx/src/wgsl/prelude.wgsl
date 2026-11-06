// Shared by every compiled effect layer (crates/fx/src/compile.rs appends the layer's
// own `birth` and `update` kernels). Particles live in a ring of `u.capacity` records of
// `u.stride` vec4s from `u.base`: position and age, velocity and life, bookkeeping, then
// the values the layer stored at birth. Each step `update` writes, per renderer, one part
// record of four vec4s that the effects pass draws (world position and size, colour,
// world axis and rotation, frame and visibility).

struct Layer {
    // Seconds since the effect started, at the end of this step.
    effect_time: f32,
    dt: f32,
    seed: u32,
    base: u32,
    capacity: u32,
    stride: u32,
    // Spawn index of the first particle born this step, and how many are born.
    spawn_first: u32,
    spawn_count: u32,
    // Effect time of this step's first birth, and the seconds between births.
    spawn_start: f32,
    spawn_spacing: f32,
    // The instance's first part record, in vec4s.
    render_base: u32,
    renderers: u32,
    // Events (D113): this layer's event records (vec4s into the pool), and for a layer
    // spawned by another's events, the parent's records, which of its channels, and the
    // child slots each parent particle owns. A child's `spawn_count` is its parent's
    // capacity: its birth kernel runs once per parent slot.
    events_out: u32,
    events_in: u32,
    channel: u32,
    per_parent: u32,
    // 1 while the effect plays (0 once its cue has ended and only its particles live on).
    running: u32,
    // An event-born layer's parent instance: its records and their stride (vec4s).
    parent_base: u32,
    parent_stride: u32,
    pad0: u32,
    // The emitter's local-to-world transform.
    emitter: mat4x4f,
    // The effect's parameters, in the compiled layer's order.
    params: array<vec4f, 2>,
    // The cue's two ends in the emitter's space (beams, drains, links): where the caster
    // and the target are now; w 1 when known.
    end_source: vec4f,
    end_target: vec4f,
    // The owner's team colour (rgb, linear), alpha 1.
    team: vec4f,
    // Where the emitter has travelled to in its own space: a missile's head as it flies
    // (Warcraft's local-to-world places points there; particles born there stay behind).
    carrier: vec4f,
}

struct Particle {
    position: vec3f,
    age: f32,
    velocity: vec3f,
    life: f32,
    index: u32,
    seed: u32,
    alive: u32,
    born: f32,
}

@group(0) @binding(0) var<uniform> u: Layer;
@group(0) @binding(1) var<storage, read_write> particles: array<vec4f>;
@group(0) @binding(2) var<storage, read_write> parts: array<vec4f>;

const TAU: f32 = 6.28318530718;

fn record(slot: u32) -> u32 {
    return u.base + slot * u.stride;
}

fn load_particle(slot: u32) -> Particle {
    let b = record(slot);
    let a = particles[b];
    let v = particles[b + 1u];
    let m = particles[b + 2u];
    return Particle(a.xyz, a.w, v.xyz, v.w, bitcast<u32>(m.x), bitcast<u32>(m.y), bitcast<u32>(m.z), m.w);
}

fn store_particle(slot: u32, p: Particle) {
    let b = record(slot);
    particles[b] = vec4f(p.position, p.age);
    particles[b + 1u] = vec4f(p.velocity, p.life);
    particles[b + 2u] = vec4f(bitcast<f32>(p.index), bitcast<f32>(p.seed), bitcast<f32>(p.alive), p.born);
}

fn load_stored(slot: u32, k: u32) -> vec4f {
    return particles[record(slot) + 3u + k];
}

fn store_value(slot: u32, k: u32, value: vec4f) {
    particles[record(slot) + 3u + k] = value;
}

// An event record: position and count, velocity and the parent's children so far, colour,
// value, the parent's spawn index and whether a birth event waits for the update, then
// where the position was at the step's start (children spread along the way).
const EVENT_VEC4S: u32 = 6u;

fn event_record(base: u32, channel: u32, capacity: u32, slot: u32) -> u32 {
    return base + (channel * capacity + slot) * EVENT_VEC4S;
}

fn write_event(r: u32, count: u32, total: u32, index: u32, pending: bool, start: vec3f, position: vec3f, velocity: vec3f, colour: vec4f, value: vec4f) {
    particles[r] = vec4f(position, f32(count));
    particles[r + 1u] = vec4f(velocity, bitcast<f32>(total));
    particles[r + 2u] = colour;
    particles[r + 3u] = value;
    particles[r + 4u] = vec4f(bitcast<f32>(index), select(0.0, 1.0, pending), 0.0, 0.0);
    particles[r + 5u] = vec4f(start, 0.0);
}

// The parent particle of an event-born particle in `slot` (its record), or none.
fn parent_record(slot: u32) -> u32 {
    return u.parent_base + (slot / max(u.per_parent, 1u)) * u.parent_stride;
}

fn has_parent() -> bool {
    return u.per_parent > 0u;
}

fn part_record(slot: u32, renderer: u32) -> u32 {
    return u.render_base + (slot * u.renderers + renderer) * 4u;
}

fn write_part(slot: u32, renderer: u32, position: vec3f, size: f32, colour: vec4f, axis: vec3f, rotation: f32, frame: f32, shown: bool) {
    let r = part_record(slot, renderer);
    parts[r] = vec4f((u.emitter * vec4f(position, 1.0)).xyz, size);
    parts[r + 1u] = colour;
    parts[r + 2u] = vec4f((u.emitter * vec4f(axis, 0.0)).xyz, rotation);
    parts[r + 3u] = vec4f(frame, select(0.0, 1.0, shown), 0.0, 0.0);
}

// The slot of the particle born after the one with spawn index `index` in `slot`'s chain
// (each parent's children are a chain of their own).
fn ribbon_next(slot: u32, index: u32) -> u32 {
    if (u.per_parent > 0u) {
        return (slot / u.per_parent) * u.per_parent + (index + 1u) % u.per_parent;
    }
    return (index + 1u) % u.capacity;
}

// A ribbon's point: its world position and width, colour, the next point's slot and the
// texture coordinate along the ribbon, and its spawn index (the draw joins it to the next
// point only when that is still the particle born after it).
fn write_ribbon(slot: u32, renderer: u32, index: u32, position: vec3f, width: f32, colour: vec4f, along: f32, shown: bool) {
    let r = part_record(slot, renderer);
    parts[r] = vec4f((u.emitter * vec4f(position, 1.0)).xyz, width);
    parts[r + 1u] = colour;
    parts[r + 2u] = vec4f(bitcast<f32>(ribbon_next(slot, index)), 0.0, 0.0, along);
    parts[r + 3u] = vec4f(0.0, select(0.0, 1.0, shown), bitcast<f32>(index), 0.0);
}

fn hide_parts(slot: u32) {
    for (var i = 0u; i < u.renderers; i += 1u) {
        parts[part_record(slot, i) + 3u] = vec4f(0.0);
    }
}

fn p_life_ratio(p: Particle) -> f32 {
    return clamp(p.age / max(p.life, 1e-6), 0.0, 1.0);
}

// PCG hash (Jarzynski and Olano): one random stream per particle from its seed.
fn pcg(v: u32) -> u32 {
    let s = v * 747796405u + 2891336453u;
    let w = ((s >> ((s >> 28u) + 4u)) ^ s) * 277803737u;
    return (w >> 22u) ^ w;
}

fn rand_unit(rng: ptr<function, u32>) -> f32 {
    *rng = pcg(*rng);
    return f32(*rng >> 8u) / 16777216.0;
}

fn rand1(rng: ptr<function, u32>, a: f32, b: f32) -> f32 {
    return mix(a, b, rand_unit(rng));
}

fn rand2(rng: ptr<function, u32>, a: vec2f, b: vec2f) -> vec2f {
    let x = rand_unit(rng);
    let y = rand_unit(rng);
    return mix(a, b, vec2f(x, y));
}

fn rand3(rng: ptr<function, u32>, a: vec3f, b: vec3f) -> vec3f {
    let x = rand_unit(rng);
    let y = rand_unit(rng);
    let z = rand_unit(rng);
    return mix(a, b, vec3f(x, y, z));
}

fn rand4(rng: ptr<function, u32>, a: vec4f, b: vec4f) -> vec4f {
    let x = rand_unit(rng);
    let y = rand_unit(rng);
    let z = rand_unit(rng);
    let w = rand_unit(rng);
    return mix(a, b, vec4f(x, y, z, w));
}

fn rand_direction(rng: ptr<function, u32>) -> vec3f {
    let y = rand_unit(rng) * 2.0 - 1.0;
    let a = rand_unit(rng) * TAU;
    let r = sqrt(max(1.0 - y * y, 0.0));
    return vec3f(r * cos(a), y, r * sin(a));
}

// An upright cylinder from the ground (y 0) to `height`; `inner` 0–1 hollows it.
fn sample_cylinder(rng: ptr<function, u32>, radius: f32, height: f32, inner: f32) -> vec3f {
    let a = rand_unit(rng) * TAU;
    let i = clamp(inner, 0.0, 1.0);
    let r = radius * sqrt(mix(i * i, 1.0, rand_unit(rng)));
    let y = rand_unit(rng) * height;
    return vec3f(r * cos(a), y, r * sin(a));
}

fn sample_sphere(rng: ptr<function, u32>, radius: f32, inner: f32) -> vec3f {
    let d = rand_direction(rng);
    let i = clamp(inner, 0.0, 1.0);
    return d * radius * pow(mix(i * i * i, 1.0, rand_unit(rng)), 1.0 / 3.0);
}

fn sample_box(rng: ptr<function, u32>, size: vec3f) -> vec3f {
    return (rand3(rng, vec3f(0.0), vec3f(1.0)) - 0.5) * size;
}

// The ground under the emitter is its plane y = 0: where a ray from `o` along `d` (a unit
// vector) meets it within `len`, as the ground's normal and the distance (0: no hit).
fn ground_hit(o: vec3f, d: vec3f, len: f32) -> vec4f {
    if (d.y > -1e-6) {
        return vec4f(0.0);
    }
    let t = -o.y / d.y;
    if (t < 0.0 || t > len) {
        return vec4f(0.0);
    }
    return vec4f(0.0, 1.0, 0.0, max(t, 1e-4));
}

fn sample_ellipsoid(rng: ptr<function, u32>, radius: vec3f, inner: f32) -> vec3f {
    return sample_sphere(rng, 1.0, inner) * radius;
}

// Smooth value noise in -1..1, three independent channels (turbulence, wobble).
fn hash3(p: vec3i) -> vec3f {
    let h = pcg(bitcast<u32>(p.x) ^ pcg(bitcast<u32>(p.y) ^ pcg(bitcast<u32>(p.z))));
    let a = pcg(h);
    let b = pcg(a);
    return vec3f(f32(h >> 8u), f32(a >> 8u), f32(b >> 8u)) / 8388608.0 - 1.0;
}

fn noise3(p: vec3f, t: f32) -> vec3f {
    let q = p + vec3f(t * 0.37, t * 0.61, t * 0.23);
    let i = vec3i(floor(q));
    let f = fract(q);
    let w = f * f * (3.0 - 2.0 * f);
    let x00 = mix(hash3(i), hash3(i + vec3i(1, 0, 0)), w.x);
    let x10 = mix(hash3(i + vec3i(0, 1, 0)), hash3(i + vec3i(1, 1, 0)), w.x);
    let x01 = mix(hash3(i + vec3i(0, 0, 1)), hash3(i + vec3i(1, 0, 1)), w.x);
    let x11 = mix(hash3(i + vec3i(0, 1, 1)), hash3(i + vec3i(1, 1, 1)), w.x);
    return mix(mix(x00, x10, w.y), mix(x01, x11, w.y), w.z);
}

fn safe_div1(a: f32, b: f32) -> f32 {
    return select(a / b, 0.0, b == 0.0);
}

fn safe_div2(a: vec2f, b: vec2f) -> vec2f {
    return select(a / b, vec2f(0.0), b == vec2f(0.0));
}

fn safe_div3(a: vec3f, b: vec3f) -> vec3f {
    return select(a / b, vec3f(0.0), b == vec3f(0.0));
}

fn safe_div4(a: vec4f, b: vec4f) -> vec4f {
    return select(a / b, vec4f(0.0), b == vec4f(0.0));
}

fn safe_normalize1(x: f32) -> f32 {
    return sign(x);
}

fn safe_normalize2(x: vec2f) -> vec2f {
    let l = length(x);
    return select(x / l, vec2f(0.0), l < 1e-9);
}

fn safe_normalize3(x: vec3f) -> vec3f {
    let l = length(x);
    return select(x / l, vec3f(0.0), l < 1e-9);
}

fn safe_normalize4(x: vec4f) -> vec4f {
    let l = length(x);
    return select(x / l, vec4f(0.0), l < 1e-9);
}

fn dot1(a: f32, b: f32) -> f32 {
    return a * b;
}

fn dot2(a: vec2f, b: vec2f) -> f32 {
    return dot(a, b);
}

fn dot3(a: vec3f, b: vec3f) -> f32 {
    return dot(a, b);
}

fn dot4(a: vec4f, b: vec4f) -> f32 {
    return dot(a, b);
}

// Rodrigues' rotation of `v` by `angle` radians around `axis`.
fn rotate_axis(v: vec3f, axis: vec3f, angle: f32) -> vec3f {
    let k = safe_normalize3(axis);
    let c = cos(angle);
    let s = sin(angle);
    return v * c + cross(k, v) * s + k * dot(k, v) * (1.0 - c);
}

// Hue 0–1, saturation 0–1, value unbounded (colours above 1 glow).
fn rgb_to_hsv(c: vec3f) -> vec3f {
    let hi = max(c.r, max(c.g, c.b));
    let lo = min(c.r, min(c.g, c.b));
    let d = hi - lo;
    var h = 0.0;
    if (d > 1e-6) {
        if (hi == c.r) {
            h = (c.g - c.b) / d;
        } else if (hi == c.g) {
            h = (c.b - c.r) / d + 2.0;
        } else {
            h = (c.r - c.g) / d + 4.0;
        }
        h = fract(h / 6.0);
    }
    return vec3f(h, select(0.0, d / hi, hi > 1e-6), hi);
}

fn rgb_to_hsv4(c: vec4f) -> vec4f {
    return vec4f(rgb_to_hsv(c.rgb), c.a);
}

fn hsv_to_rgb4(c: vec4f) -> vec4f {
    return vec4f(hsv_to_rgb(vec3f(fract(c.x), clamp(c.y, 0.0, 1.0), max(c.z, 0.0))), c.a);
}

fn hsv_to_rgb(c: vec3f) -> vec3f {
    let k = vec3f(1.0, 2.0 / 3.0, 1.0 / 3.0);
    let p = abs(fract(vec3f(c.x) + k) * 6.0 - 3.0);
    return c.z * mix(vec3f(1.0), clamp(p - 1.0, vec3f(0.0), vec3f(1.0)), c.y);
}

fn hsv_shift(colour: vec4f, shift: vec3f) -> vec4f {
    let hsv = rgb_to_hsv(colour.rgb) + shift;
    return vec4f(hsv_to_rgb(vec3f(fract(hsv.x), clamp(hsv.y, 0.0, 1.0), max(hsv.z, 0.0))), colour.a);
}

// The shift by width: one number brightens (value), two shift hue and saturation, four
// use their first three.
fn hsv_shift1(colour: vec4f, shift: f32) -> vec4f {
    return hsv_shift(colour, vec3f(0.0, 0.0, shift));
}

fn hsv_shift2(colour: vec4f, shift: vec2f) -> vec4f {
    return hsv_shift(colour, vec3f(shift, 0.0));
}

fn hsv_shift3(colour: vec4f, shift: vec3f) -> vec4f {
    return hsv_shift(colour, shift);
}

fn hsv_shift4(colour: vec4f, shift: vec4f) -> vec4f {
    return hsv_shift(colour, shift.xyz);
}

// Hermite between two keys of a curve, `u` 0–1 across the segment.
fn hermite(u: f32, p0: vec4f, m0: vec4f, p1: vec4f, m1: vec4f) -> vec4f {
    let u2 = u * u;
    let u3 = u2 * u;
    return (2.0 * u3 - 3.0 * u2 + 1.0) * p0 + (u3 - 2.0 * u2 + u) * m0 + (-2.0 * u3 + 3.0 * u2) * p1 + (u3 - u2) * m1;
}
