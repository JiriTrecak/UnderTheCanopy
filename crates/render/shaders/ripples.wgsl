// Unit wakes (old `WaterInteraction`): every live ring is one instanced quad drawn
// additively into the ripple field around the camera focus. A derivative-of-Gaussian
// crest `exp(−2q²)` with an antialiased width; RG holds the signed slope, B the ring's
// water level times its weight and A the weight, so the water shader can reject rings
// of a different water level.

const EXPANSION: f32 = 1.1;
const WIDTH: f32 = 0.28;
const LIFETIME: f32 = 2.8;

struct Field {
    // min x, min z, span, 0
    bounds: vec4f,
}

struct Ring {
    // x, z, age, level
    a: vec4f,
    // direction x, z, strength, stretch
    b: vec4f,
}

@group(0) @binding(0) var<uniform> field: Field;
@group(0) @binding(1) var<storage, read> rings: array<Ring>;

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) local: vec2f,
    // age, level, strength, stretch
    @location(1) params: vec4f,
    @location(2) direction: vec2f,
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, @builtin(instance_index) inst: u32) -> VsOut {
    let corners = array<vec2f, 6>(
        vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0),
        vec2f(-1.0, 1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
    );
    let ring = rings[inst];
    let local = corners[vid];
    let d = ring.b.xy;
    let radius = 0.45 + ring.a.z * EXPANSION + WIDTH * 3.0;
    var q = local * radius;
    q.y *= ring.b.w;
    let world = ring.a.xy + vec2f(d.y * q.x + d.x * q.y, -d.x * q.x + d.y * q.y);
    let ndc = (world - field.bounds.xy) / field.bounds.z * 2.0 - 1.0;
    var out: VsOut;
    // Texel rows run along +z: flip y so row 0 is the field's minimum z.
    out.clip = vec4f(ndc.x, -ndc.y, 0.0, 1.0);
    out.local = local;
    out.params = vec4f(ring.a.z, ring.a.w, ring.b.z, ring.b.w);
    out.direction = d;
    return out;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let age = in.params.x;
    let radius = 0.45 + age * EXPANSION;
    let bound = radius + WIDTH * 3.0;
    let p = in.local * bound;
    let d = length(p);
    let w = max(WIDTH, fwidth(d) * 1.2);
    let q = (d - radius) / w;
    let ring = exp(-q * q * 2.0);
    let fade = smoothstep(0.0, 0.12, age) * pow(max(0.0, 1.0 - age / LIFETIME), 1.5);
    if (ring * fade < 0.002) {
        discard;
    }
    var n = p / max(d, 0.001);
    n.y /= in.params.w;
    let dir = in.direction;
    n = vec2f(dir.y * n.x + dir.x * n.y, -dir.x * n.x + dir.y * n.y);
    let weight = ring * fade * in.params.z;
    return vec4f(n * q * weight, in.params.y * weight, weight);
}
