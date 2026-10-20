// Effects (renderer §4.8): GPU particles evaluated in the vertex shader from their emitter,
// index, seed and age with the old effect player's formulas (billboards, bursts, streaks,
// emitters, rings, glows, the pillar, travelling missiles and crests, hit sparks), the
// damage effects (D46: flames and smoke of burning buildings, dust of melee hits), and
// ribbons (trails, melee blade ribbons and textured strips). Mirrors effects.rs. Needs
// Frame.
//
// Drawn in their own pass over the resolved scene: fragments read the scene's linear
// depth to hide behind surfaces and fade softly into them, and blend premultiplied with
// their additivity (1 adds light, 0 covers). Emitters may carry curve blocks (colour,
// opacity, size, additivity, erosion and heat over life, and a ramp colouring texture
// brightness) and a path (world-space particles stay on the path where they were born).

struct Emitter {
    // xyz: root (world)
    position: vec4f,
    // root rotation quaternion xyzw; travelling kinds: unit direction xyz
    rotation: vec4f,
    // rgb colour; a: opacity
    color: vec4f,
    // rgb accent; a: size
    accent: vec4f,
    // t (0–1), height, spread, length
    shape: vec4f,
    // motion scale, motion rotation, phase (radians), flipbook age (s)
    motion: vec4f,
    // kind, flags, count, seed
    ints: vec4u,
    // flipbook columns, rows, first frame, frames
    flip: vec4u,
    // frame time (s), play mode, elapsed (s), particles emitted
    times: vec4f,
    // emitter: direction xyz, cone
    e0: vec4f,
    // speed, variation, drag, gravity
    e1: vec4f,
    // lifetime (s), rate (1/s), spawn radius, duration (s)
    e2: vec4f,
    // start size, end size, spin, fade in
    e3: vec4f,
    // fade out, sustained, burst, spark power
    e4: vec4f,
    // white-hot core (0–1), streak seconds along the velocity
    e5: vec4f,
    // curve block (0xffffffff: none), curve flags (C_*), first path sample, path samples
    ints2: vec4u,
    // seconds of the first path sample, seconds between samples, inherited velocity,
    // soft fade (m)
    e6: vec4f,
    // turbulence strength (m), cell (m), churn (1/s); landing friction (< 0: none)
    e7: vec4f,
    // inner cone (rad), inner spawn share, erosion softness, ground height (world y)
    e8: vec4f,
}

struct Ribbon {
    // mode 0: a (w: fade), b (w: fade), (width, 0, 0, 0); mode 1: four corners (w: alpha);
    // mode 2: a (w: width), b (w: width), tangent at a (w: u), tangent at b (w: u)
    p0: vec4f,
    p1: vec4f,
    p2: vec4f,
    p3: vec4f,
    // rgb (mode 2: at a); a: mode
    color: vec4f,
    // mode 0: opacity; mode 2: opacity at a, additivity, soft fade (m)
    look: vec4f,
    // mode 2: colour and opacity at b
    end: vec4f,
}

struct EffectView {
    // camera right (world), w unused
    right: vec4f,
    // camera up (world)
    up: vec4f,
}

@group(1) @binding(0) var effect_texture: texture_2d<f32>;
@group(1) @binding(1) var effect_sampler: sampler;
@group(2) @binding(0) var<storage, read> emitters: array<Emitter>;
@group(2) @binding(1) var<storage, read> parts: array<u32>;
@group(2) @binding(2) var<storage, read> ribbons: array<Ribbon>;
@group(2) @binding(3) var<uniform> view: EffectView;
@group(2) @binding(4) var<storage, read> curves: array<vec4f>;
@group(2) @binding(5) var<storage, read> paths: array<vec4f>;
@group(3) @binding(0) var scene_depth: texture_2d<f32>;

const CURVE_SAMPLES: u32 = 32u;
const NO_CURVES: u32 = 0xffffffffu;
const C_COLOUR: u32 = 1u;
const C_OPACITY: u32 = 2u;
const C_SIZE: u32 = 4u;
const C_ADDITIVE: u32 = 8u;
const C_EROSION: u32 = 16u;
const C_HEAT: u32 = 32u;
const C_RAMP: u32 = 64u;

const K_BILLBOARD: u32 = 0u;
const K_BURST: u32 = 1u;
const K_STREAKS: u32 = 2u;
const K_EMITTER: u32 = 3u;
const K_RING: u32 = 4u;
const K_GLOW: u32 = 5u;
const K_PILLAR: u32 = 6u;
const K_MISSILE: u32 = 7u;
const K_WAVEFRONT: u32 = 8u;
const K_SPARK: u32 = 9u;
const K_FLAME: u32 = 10u;
const K_SMOKE: u32 = 11u;
const K_DUST: u32 = 12u;
const K_RAIN: u32 = 13u;
const K_RAIN_SPLASH: u32 = 14u;
const K_RAIN_FLASH: u32 = 15u;

const F_TEXTURED: u32 = 1u;
const F_GROUND: u32 = 2u;
const F_FRESNEL: u32 = 8u;
const F_SPIN: u32 = 16u;
const F_DISC: u32 = 32u;
const F_NORMAL: u32 = 64u;
const F_RANDOM_START: u32 = 128u;
const F_OVER_LIFE: u32 = 256u;
const F_FLIP_BLEND: u32 = 512u;
const F_WORLD: u32 = 1024u;

// Fragment masks.
const M_TEXTURE: u32 = 0u;
const M_SPHERE: u32 = 1u;
const M_FRESNEL: u32 = 2u;
const M_RING: u32 = 3u;
const M_GLOW: u32 = 4u;
const M_SPARK: u32 = 5u;
const M_PILLAR: u32 = 6u;
const M_FLAME: u32 = 7u;
const M_SMOKE: u32 = 8u;

const PI: f32 = 3.141592653589793;
const TAU: f32 = 6.283185307179586;
const GOLDEN: f32 = 2.399963;

// Visual-only stable randomness (old `noise`): the same seed gives the same value.
fn noise(seed: u32) -> f32 {
    var n = seed;
    n = (n ^ (n >> 16u)) * 0x45d9f3bu;
    n = (n ^ (n >> 16u)) * 0x45d9f3bu;
    n = n ^ (n >> 16u);
    return f32(n >> 8u) / 16777216.0;
}

fn rotate(q: vec4f, v: vec3f) -> vec3f {
    let t = 2.0 * cross(q.xyz, v);
    return v + q.w * t + cross(q.xyz, t);
}

// A flipbook frame's texture rectangle (scale xy, offset zw), old `flipbookFrame`.
fn frame_rect(e: Emitter, frame: u32) -> vec4f {
    let cols = max(e.flip.x, 1u);
    let rows = max(e.flip.y, 1u);
    let f = e.flip.z + frame;
    return vec4f(1.0 / f32(cols), 1.0 / f32(rows), f32(f % cols) / f32(cols), f32(f / cols) / f32(rows));
}

struct Flip {
    a: vec4f,
    b: vec4f,
    // how far towards `b`
    blend: f32,
}

// The flipbook frames at `age` seconds (or, spanning each particle's life, at `life`),
// and how far the first has faded into the next.
fn flip_frames(e: Emitter, age: f32, life: f32, seed: f32) -> Flip {
    var out: Flip;
    out.a = vec4f(1.0, 1.0, 0.0, 0.0);
    out.b = out.a;
    out.blend = 0.0;
    let frames = max(e.flip.w, 1u);
    let over_life = (e.ints.y & F_OVER_LIFE) != 0u;
    if (e.times.x <= 0.0 && !over_life) {
        return out;
    }
    var phase = 0u;
    if ((e.ints.y & F_RANDOM_START) != 0u) {
        phase = u32(floor(clamp(seed, 0.0, 0.999999) * f32(frames)));
    }
    var position = max(age, 0.0) / max(e.times.x, 1e-4);
    if (over_life) {
        position = clamp(life, 0.0, 0.999999) * f32(frames);
    }
    let step = u32(floor(position)) + phase;
    let next = step + 1u;
    out.blend = select(0.0, fract(position), (e.ints.y & F_FLIP_BLEND) != 0u);
    let mode = u32(e.times.y);
    out.a = frame_rect(e, play_frame(step, frames, mode));
    out.b = frame_rect(e, play_frame(next, frames, mode));
    return out;
}

fn play_frame(step: u32, frames: u32, mode: u32) -> u32 {
    let cycle = max(frames * 2u - 2u, 1u);
    if (mode == 0u) {
        return min(frames - 1u, step);
    } else if (mode == 1u) {
        return step % frames;
    }
    let ping = step % cycle;
    return min(ping, cycle - ping);
}

// A curve block's row sampled at `t` (0–1), linearly between its samples.
fn curve(base: u32, row: u32, t: f32) -> vec4f {
    let x = clamp(t, 0.0, 1.0) * f32(CURVE_SAMPLES - 1u);
    let i = min(u32(floor(x)), CURVE_SAMPLES - 2u);
    let at = base + row * CURVE_SAMPLES + i;
    return mix(curves[at], curves[at + 1u], x - f32(i));
}

// Smooth 3D value noise in 0–1.
fn value_noise3(p: vec3f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let h = vec3u(vec3i(i) + vec3i(65536));
    let hx = array<u32, 2>(h.x * 73856093u, (h.x + 1u) * 73856093u);
    let hy = array<u32, 2>(h.y * 19349663u, (h.y + 1u) * 19349663u);
    let hz = array<u32, 2>(h.z * 83492791u, (h.z + 1u) * 83492791u);
    let c000 = noise(hx[0] ^ hy[0] ^ hz[0]);
    let c100 = noise(hx[1] ^ hy[0] ^ hz[0]);
    let c010 = noise(hx[0] ^ hy[1] ^ hz[0]);
    let c110 = noise(hx[1] ^ hy[1] ^ hz[0]);
    let c001 = noise(hx[0] ^ hy[0] ^ hz[1]);
    let c101 = noise(hx[1] ^ hy[0] ^ hz[1]);
    let c011 = noise(hx[0] ^ hy[1] ^ hz[1]);
    let c111 = noise(hx[1] ^ hy[1] ^ hz[1]);
    let x0 = mix(mix(c000, c100, u.x), mix(c010, c110, u.x), u.y);
    let x1 = mix(mix(c001, c101, u.x), mix(c011, c111, u.x), u.y);
    return mix(x0, x1, u.z);
}

// A swirling displacement (−1…1 each way) of the noise field at `p`, churning with `t`.
fn swirl(p: vec3f, t: f32) -> vec3f {
    return vec3f(
        value_noise3(p + vec3f(0.0, 0.0, t)),
        value_noise3(p + vec3f(17.3, 5.1, t * 0.9)),
        value_noise3(p + vec3f(3.7, 29.9, t * 1.1)),
    ) * 2.0 - 1.0;
}

// A moving emitter's position at `born` seconds and its velocity there, from its path
// samples (`ints2.zw`, from `e6.x` every `e6.y` seconds).
struct PathPoint {
    position: vec3f,
    velocity: vec3f,
}

fn path_at(e: Emitter, born: f32) -> PathPoint {
    var out: PathPoint;
    let n = e.ints2.w;
    if (n == 0u) {
        out.position = e.position.xyz;
        out.velocity = vec3f(0.0);
        return out;
    }
    let dt = max(e.e6.y, 1e-5);
    let x = clamp((born - e.e6.x) / dt, 0.0, f32(n - 1u));
    let i = min(u32(floor(x)), max(n, 2u) - 2u);
    let a = paths[e.ints2.z + i].xyz;
    let b = paths[e.ints2.z + min(i + 1u, n - 1u)].xyz;
    out.position = mix(a, b, x - f32(i));
    out.velocity = (b - a) / dt;
    return out;
}

struct Part {
    local: vec3f,
    // world-space particles: where the emitter was at birth (otherwise its root)
    base: vec3f,
    // additivity, erosion threshold, heat
    look: vec3f,
    // flipbook position over life (−1: by age)
    flip_life: f32,
    // quad scale (sprites: full size; spheres and discs: radius); with an axis, x is the
    // width and y the length along it
    scale: vec2f,
    // world direction the quad's length follows on screen (zero: none, the quad spins)
    axis: vec3f,
    rotation: f32,
    color: vec3f,
    alpha: f32,
    visible: bool,
    flip_age: f32,
    flip_seed: f32,
}

// Where rain drop `i` of `count` lands (xyz, in the cue's frame) and how late it launches
// (w, seconds).
fn drop_of(e: Emitter, i: u32, count: u32) -> vec4f {
    let seed = e.ints.w + i * 7919u;
    let angle = (f32(i) + noise(seed + 1u) * 0.6) * GOLDEN + noise(e.ints.w) * TAU;
    let radius = sqrt((f32(i) + noise(seed + 2u)) / f32(max(count, 1u))) * e.shape.z;
    return vec4f(cos(angle) * radius, 0.02, sin(angle) * radius, noise(seed + 3u) * e.e1.x);
}

// Distance along an initial velocity of 1 after `age` seconds under linear drag.
fn travel_of(drag: f32, age: f32) -> f32 {
    return select(age, (1.0 - exp(-drag * age)) / drag, drag > 0.0);
}

fn evaluate(e: Emitter, i: u32) -> Part {
    let kind = e.ints.x;
    let flags = e.ints.y;
    let count = max(e.ints.z, 1u);
    let t = e.shape.x;
    let height = e.shape.y;
    let spread = e.shape.z;
    let len = e.shape.w;
    let size = e.accent.a;
    let fi = f32(i);
    var p: Part;
    p.local = vec3f(0.0);
    p.base = e.position.xyz;
    p.look = vec3f(select(1.0, 0.0, (flags & F_NORMAL) != 0u), 0.0, 1.0);
    p.flip_life = t;
    p.scale = vec2f(size);
    p.axis = vec3f(0.0);
    p.rotation = select(0.0, e.motion.y, (flags & F_SPIN) != 0u);
    p.color = e.color.rgb;
    p.alpha = e.color.a;
    p.visible = true;
    p.flip_age = e.motion.w;
    p.flip_seed = noise(e.ints.w + i * 97u);
    switch kind {
        case K_BILLBOARD: {
            p.scale = vec2f(size, size * len);
            p.local.y = height + t * 0.15;
        }
        case K_BURST: {
            let angle = fi * GOLDEN + e.motion.z;
            let disc = (flags & F_DISC) != 0u;
            let s = spread * (0.45 + f32(i % 7u) / 9.0) * (t + 0.12);
            let base = select(0.0, sqrt((fi + 0.5) / f32(count)) * spread, disc);
            p.local = vec3f(
                cos(angle) * (s + base),
                height * t * (0.5 + f32(i % 5u) / 8.0) + select(1.2, 0.15, disc),
                sin(angle) * (s + base),
            );
            p.scale = vec2f(size * (1.0 - t * 0.65));
        }
        case K_STREAKS: {
            let age = fract(t * 1.4 + fi / f32(count));
            let angle = fi * GOLDEN + e.motion.z;
            let radius = spread * (0.5 + f32(i % 5u) / 10.0);
            p.local = vec3f(cos(angle) * radius, 0.25 + age * height, sin(angle) * radius);
            p.scale = vec2f(size, len * (0.7 + f32(i % 3u) * 0.15));
            p.alpha *= sin(age * PI);
        }
        case K_EMITTER: {
            let elapsed = e.times.z;
            let lifetime = max(e.e2.x, 1e-4);
            let rate = max(e.e2.y, 1e-4);
            let sustained = e.e4.y > 0.5;
            let burst = e.e4.z > 0.5;
            var ordinal = i;
            var born = 0.0;
            if (!burst) {
                let cycles = max(0.0, floor((e.times.w - fi) / f32(count)));
                ordinal = i + u32(cycles) * count;
                born = f32(ordinal) / rate;
            }
            let age = elapsed - born;
            let life = age / lifetime;
            let life01 = clamp(life, 0.0, 1.0);
            p.visible = age >= 0.0 && life < 1.0 && (sustained || born <= e.e2.w + 1e-5);
            let seed = e.ints.w + ordinal * 97u;
            p.flip_age = age;
            p.flip_life = life01;
            p.flip_seed = noise(seed);
            let dir = e.e0.xyz;
            let reference = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 1.0, 0.0), abs(dir.y) < 0.99);
            let side = normalize(cross(dir, reference));
            let up = normalize(cross(side, dir));
            let azimuth = noise(seed + 1u) * TAU;
            // Directions between the inner and outer cone, uniform over the sphere.
            let cos_t = mix(cos(e.e8.x), cos(e.e0.w), noise(seed + 2u));
            let sin_t = sqrt(max(0.0, 1.0 - cos_t * cos_t));
            var velocity = (dir * cos_t + side * cos(azimuth) * sin_t + up * sin(azimuth) * sin_t)
                * e.e1.x * (1.0 - e.e1.y + 2.0 * e.e1.y * noise(seed + 3u));
            let drag = e.e1.z;
            let world = (flags & F_WORLD) != 0u;
            var inherited = vec3f(0.0);
            if (world) {
                let at = path_at(e, born);
                p.base = at.position;
                // The emitter's velocity in its own frame, so the root rotation turns it back.
                inherited = rotate(vec4f(-e.rotation.xyz, e.rotation.w), at.velocity) * e.e6.z;
            }
            velocity += inherited;
            let angle = noise(seed + 4u) * TAU;
            let inner = e.e8.y * e.e8.y;
            let radius = sqrt(mix(inner, 1.0, noise(seed + 5u))) * e.e2.z;
            let spawn = vec3f(cos(angle) * radius, height, sin(angle) * radius);
            p.local = spawn + velocity * travel_of(drag, age);
            // Gravity pulls down (negative values lift: steam, embers).
            p.local.y -= 0.5 * e.e1.w * age * age;
            if (e.e7.x > 0.0) {
                // Swirling drift through a noise field, growing in as the particle lives.
                let field = (p.base + p.local) / max(e.e7.y, 1e-3) + vec3f(noise(seed + 7u) * 0.35);
                p.local += swirl(field, age * e.e7.z) * e.e7.x * min(1.0, life01 * 4.0);
            }
            let curves_on = e.ints2.y;
            let block = e.ints2.x;
            var size = e.e3.x + (e.e3.y - e.e3.x) * life;
            if ((curves_on & C_SIZE) != 0u) {
                size = curve(block, 1u, life01).x;
            }
            p.scale = vec2f(size);
            var landed = false;
            if (e.e7.w >= 0.0) {
                // Landing on the ground under the emitter: the moment it lands is found by
                // halving the time, then it slides on with the friction left.
                let floor_y = e.e8.w - p.base.y + 0.02;
                if (p.local.y < floor_y) {
                    var lo = 0.0;
                    var hi = age;
                    for (var k = 0u; k < 7u; k++) {
                        let mid = 0.5 * (lo + hi);
                        let y = spawn.y + velocity.y * travel_of(drag, mid) - 0.5 * e.e1.w * mid * mid;
                        if (y < floor_y) {
                            hi = mid;
                        } else {
                            lo = mid;
                        }
                    }
                    let at_land = spawn + velocity * travel_of(drag, hi);
                    let slide = (p.local - at_land) * (1.0 - e.e7.w);
                    p.local = vec3f(at_land.x + slide.x, floor_y, at_land.z + slide.z);
                    if ((flags & F_GROUND) == 0u) {
                        // Camera-facing sprites rest on the ground, not in it.
                        p.local.y += size * 0.3;
                    }
                    landed = true;
                }
            }
            if (e.e5.y > 0.0 && !landed) {
                // A streak: the sprite spans the last `stretch` seconds of its path, its
                // head at the particle.
                let now = velocity * select(1.0, exp(-drag * age), drag > 0.0)
                    - vec3f(0.0, e.e1.w * age, 0.0);
                let trail = rotate(e.rotation, now) * e.e5.y;
                let reach = length(trail);
                if (reach > 1e-4) {
                    p.axis = trail / reach;
                    p.scale.y += reach;
                    p.local -= now * e.e5.y * 0.5;
                }
            }
            if ((curves_on & C_OPACITY) != 0u) {
                p.alpha = curve(block, 0u, life01).w * e.color.a;
            } else {
                let fade_in = select(1.0, min(1.0, life / e.e3.w), e.e3.w > 0.0);
                let fade_out = select(1.0, min(1.0, (1.0 - life) / e.e4.x), e.e4.x > 0.0);
                p.alpha = fade_in * fade_out * e.color.a;
            }
            if ((curves_on & C_COLOUR) != 0u) {
                p.color = curve(block, 0u, life01).rgb;
            } else {
                p.color = mix(e.color.rgb, e.accent.rgb, life01);
            }
            if ((curves_on & (C_ADDITIVE | C_EROSION | C_HEAT)) != 0u) {
                let look = curve(block, 1u, life01);
                if ((curves_on & C_ADDITIVE) != 0u) {
                    p.look.x = look.y;
                }
                if ((curves_on & C_EROSION) != 0u) {
                    p.look.y = look.z;
                }
                if ((curves_on & C_HEAT) != 0u) {
                    p.look.z = look.w;
                }
            }
            p.rotation = noise(seed + 6u) * TAU + e.e3.z * age;
        }
        case K_RING, K_GLOW: {
            p.local.y = height;
        }
        case K_MISSILE, K_WAVEFRONT: {
            p.scale = vec2f(size, size * len);
            if (kind == K_MISSILE && len > 1.01) {
                // An elongated head (a tracer): its length along the flight.
                p.axis = e.rotation.xyz;
            } else if (kind == K_MISSILE && (flags & F_TEXTURED) != 0u) {
                p.rotation = t * 8.0;
            }
        }
        case K_SPARK: {
            let power = e.e4.w;
            let a = e.motion.z + (fi - 3.5) * 0.34;
            let r = t * (0.4 + fi * 0.08) * power;
            p.local = vec3f(sin(a) * r, sin(PI * t) * (0.3 + f32(i % 3u) * 0.1), cos(a) * r);
            p.scale = vec2f((0.04 + f32(i % 3u) * 0.012) * power * (1.0 - t));
            p.color = (1.0 - t) * vec3f(1.0, 0.58 + f32(i % 2u) * 0.2, 0.2);
        }
        case K_FLAME: {
            // shape: time (s), heat (0–1); accent.a: flame size (m). Parts 0 and 1 are a
            // flickering glow at the base; the rest are short licks, each looping on its
            // own short period and re-seeded every cycle, so no two repeat.
            let time = e.shape.x;
            let heat = e.shape.y;
            let base = e.ints.w + i * 97u;
            if (i < 2u) {
                let flicker = 0.75 + 0.15 * sin(time * 13.0 + fi * 2.1 + noise(base) * TAU)
                    + 0.1 * sin(time * 29.0 + fi);
                p.local = vec3f(0.0, size * (0.2 + fi * 0.25), 0.0);
                p.scale = vec2f(size * (1.0 + fi * 0.5) * flicker);
                p.color = vec3f(1.0, 0.42, 0.08) * (0.9 + heat * 0.4);
                p.alpha = (0.5 - fi * 0.2) * flicker * e.color.a;
                p.flip_seed = noise(base);
                p.flip_age = -1.0;
                break;
            }
            let period = 0.26 + noise(base) * 0.26;
            let cycles = time / period + fi / f32(count);
            let life = fract(cycles);
            let seed = base + u32(floor(cycles)) * 7919u;
            let angle = noise(seed + 1u) * TAU;
            let radius = sqrt(noise(seed + 2u)) * size * 0.5 * (1.0 - life * 0.6);
            let rise = life * size * (0.7 + noise(seed + 3u) * 1.0);
            let sway = sin(time * 9.0 + noise(seed + 4u) * TAU) * 0.1 * size * life;
            p.local = vec3f(cos(angle) * radius + sway, rise + size * 0.05, sin(angle) * radius);
            let s = size * (0.22 + noise(seed + 5u) * 0.36) * (1.0 - life * 0.55);
            p.scale = vec2f(s * 0.62, s * 1.15);
            // Yellow at the root, orange, then a dull red as the lick dies (HDR: the
            // roots bloom).
            let hot = vec3f(1.0, 0.6, 0.16) * 1.65;
            let warm = vec3f(1.0, 0.3, 0.035) * 1.35;
            let dull = vec3f(0.55, 0.07, 0.01);
            p.color = select(
                mix(warm, dull, (life - 0.4) / 0.6),
                mix(hot, warm, life / 0.4),
                life < 0.4,
            ) * (0.85 + heat * 0.3);
            p.alpha = smoothstep(0.0, 0.08, life) * pow(1.0 - life, 1.2) * e.color.a;
            p.flip_seed = noise(seed + 6u);
            p.flip_age = life;
        }
        case K_SMOKE: {
            // shape: time (s), density (0–1), wind x, wind z; accent.a: flame size (m).
            // Thin wisps rising from the flames, leaning downwind, widening as they climb.
            let time = e.shape.x;
            let density = e.shape.y;
            let base = e.ints.w + i * 131u;
            let period = 2.4 + noise(base) * 1.2;
            let cycles = time / period + fi / f32(count);
            let life = fract(cycles);
            let seed = base + u32(floor(cycles)) * 7919u;
            let angle = noise(seed + 1u) * TAU;
            let radius = size * (0.06 + life * 0.3) * sqrt(noise(seed + 2u));
            let rise = size * 1.1 + life * (2.8 + 3.6 * density) * (0.55 + noise(seed + 3u) * 0.9);
            let drift = e.shape.zw * life * (1.6 + density * 2.4);
            p.local = vec3f(cos(angle) * radius + drift.x, rise, sin(angle) * radius + drift.y);
            p.scale = vec2f(size * (0.3 + life * (0.7 + density * 0.5)) * (0.7 + noise(seed + 4u) * 0.6));
            p.rotation = noise(seed + 5u) * TAU + life * 1.2 * (noise(seed + 8u) - 0.5);
            p.alpha = smoothstep(0.0, 0.15, life) * pow(1.0 - life, 1.45) * e.color.a;
            // Darkest over the fire, greying as it thins.
            p.color = mix(e.color.rgb, e.accent.rgb, life * life) * (0.8 + noise(seed + 6u) * 0.4);
            p.flip_seed = noise(seed + 7u);
            p.flip_age = life;
        }
        case K_DUST: {
            // shape: age (s), lifetime (s), height above the ground; e0: swing direction;
            // accent.a: the swinging body's size (1 for infantry). Two thirds puff out at
            // the blow, a third is kicked up from the ground under it; all are thrown along
            // the swing with a wide fan, slow down and settle.
            let age = e.shape.x;
            let life = age / max(e.shape.y, 1e-4);
            p.visible = life < 1.0;
            let power = size;
            let seed = e.ints.w + i * 97u;
            let dir = e.e0.xyz;
            let side = vec3f(-dir.z, 0.0, dir.x);
            let low = i % 3u == 2u;
            let forward = (0.9 + noise(seed + 1u) * 1.9) * power;
            let lateral = (noise(seed + 2u) * 2.0 - 1.0) * 1.6 * power;
            let up = (0.2 + noise(seed + 3u) * 0.9) * power;
            let velocity = dir * forward + side * lateral + vec3f(0.0, up, 0.0);
            let drag = 4.0;
            let travel = (1.0 - exp(-drag * age)) / drag;
            let jitter = vec3f(noise(seed + 4u) - 0.5, noise(seed + 5u) - 0.5, noise(seed + 6u) - 0.5)
                * 0.3 * power;
            let start = select(vec3f(0.0), vec3f(0.0, 0.15 - e.shape.z, 0.0), low);
            p.local = start + jitter + velocity * travel;
            // Settles, never below the ground.
            p.local.y = max(p.local.y - 0.6 * age * age, 0.08 - e.shape.z);
            let grow = 0.22 + 0.5 * sqrt(clamp(life, 0.0, 1.0));
            p.scale = vec2f(power * grow * (0.75 + noise(seed + 7u) * 0.5));
            p.rotation = noise(seed + 8u) * TAU + age * 0.6;
            p.alpha = min(1.0, life / 0.05) * pow(max(1.0 - life, 0.0), 1.2) * e.color.a;
            p.color = e.color.rgb * (0.85 + noise(seed + 9u) * 0.3);
            p.flip_seed = noise(seed + 10u);
            p.flip_age = life;
        }
        case K_RAIN, K_RAIN_SPLASH, K_RAIN_FLASH: {
            // shape: 0, fall height, spread, shard length; motion.w: seconds since the cue
            // started; e0: the path's back direction (unit), fall seconds; e1: launch
            // delay, impact seconds, impact spread, impact height; e2: impact size,
            // impact count, drops. Old `updateRain`: drops land on a golden-angle disc, each
            // launching up to the delay late; a shard's tip meets its landing point.
            let impacts = max(u32(e.e2.y), 1u);
            var drop = i;
            var j = 0u;
            if (kind == K_RAIN_SPLASH) {
                drop = i / impacts;
                j = i % impacts;
            }
            let d = drop_of(e, drop, max(u32(e.e2.z), 1u));
            let age = e.motion.w - d.w;
            let fall = max(e.e0.w, 1e-4);
            let back = e.e0.xyz;
            let landing = d.xyz;
            if (kind == K_RAIN) {
                let flight = age / fall;
                p.visible = age >= 0.0 && flight < 1.0;
                let scale = 0.8 + noise(e.ints.w + drop * 97u + 4u) * 0.35;
                let long = size * len * scale;
                p.local = landing + back * (height * (1.0 - flight) + long * 0.5);
                p.scale = vec2f(size * scale, long);
                p.axis = back;
                p.alpha = min(1.0, age * 80.0) * e.color.a;
                p.flip_age = age;
                p.flip_seed = noise(e.ints.w + drop * 97u);
                break;
            }
            let impact = max(e.e1.y, 1e-4);
            let q = (age - fall) / impact;
            p.visible = q >= 0.0 && q < 1.0;
            if (kind == K_RAIN_FLASH) {
                p.local = landing + vec3f(0.0, 0.05, 0.0);
                p.scale = vec2f(e.e1.z * (0.35 + q));
                p.alpha = pow(max(1.0 - q, 0.0), 3.0) * 0.9 * e.color.a;
                break;
            }
            let angle = f32(j) * GOLDEN + f32(drop) * 1.7;
            let speed = 0.55 + noise(e.ints.w + drop * 37u + j * 97u) * 0.45;
            p.local = landing + vec3f(
                cos(angle) * e.e1.z * q * speed,
                0.08 + e.e1.w * 4.0 * q * (1.0 - q) * speed,
                sin(angle) * e.e1.z * q * speed,
            );
            p.scale = vec2f(e.e2.x * (1.0 - q * 0.6), e.e2.x * (1.4 - q * 0.6));
            p.alpha = pow(max(1.0 - q, 0.0), 1.5) * e.color.a;
            p.rotation = angle + q * 3.0;
            p.color = mix(e.color.rgb, e.accent.rgb, q);
            p.flip_age = age - fall;
            p.flip_seed = noise(e.ints.w + drop * 97u + j);
        }
        default: {}
    }
    if (kind <= K_PILLAR && kind != K_EMITTER) {
        p.scale *= e.motion.x;
    }
    return p;
}

fn mask_of(e: Emitter) -> u32 {
    let kind = e.ints.x;
    let flags = e.ints.y;
    if ((flags & F_TEXTURED) != 0u) {
        return M_TEXTURE;
    }
    if (kind == K_RING) {
        return M_RING;
    }
    if (kind == K_GLOW) {
        return M_GLOW;
    }
    if (kind == K_SPARK) {
        return M_SPARK;
    }
    if (kind == K_FLAME) {
        return M_FLAME;
    }
    if (kind == K_SMOKE || kind == K_DUST) {
        return M_SMOKE;
    }
    if (kind == K_RAIN_FLASH) {
        return M_GLOW;
    }
    if ((flags & F_FRESNEL) != 0u) {
        return M_FRESNEL;
    }
    return M_SPHERE;
}

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    // xy: quad coordinate (−1…1); zw: texture uv
    @location(1) coord: vec4f,
    @location(2) color: vec4f,
    @location(3) rect: vec4f,
    // mask, flags, curve block (ramp), curve flags
    @location(4) @interpolate(flat) mask: vec4u,
    // white-hot core, soft fade (m), flipbook blend, 0
    @location(5) @interpolate(flat) look: vec4f,
    // the next flipbook frame's rectangle
    @location(6) rect2: vec4f,
    // additivity, erosion threshold, erosion softness, heat
    @location(7) @interpolate(flat) shade: vec4f,
}

const CORNERS = array<vec2f, 6>(
    vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(-1.0, 1.0),
    vec2f(-1.0, 1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
);

fn hidden() -> VsOut {
    var out: VsOut;
    out.clip = vec4f(0.0, 0.0, -2.0, 1.0);
    return out;
}

@vertex
fn vs_quad(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    let packed = parts[ii];
    let e = emitters[packed >> 8u];
    let i = packed & 255u;
    let p = evaluate(e, i);
    if (!p.visible || p.alpha <= 0.0) {
        return hidden();
    }
    let kind = e.ints.x;
    let flags = e.ints.y;
    let mask = mask_of(e);
    let c = CORNERS[vi];
    var centre = p.base;
    if (kind != K_MISSILE && kind != K_WAVEFRONT) {
        centre += rotate(e.rotation, p.local);
    }
    var offset: vec3f;
    // Textured quads are 1×1 scaled (half the scale each way); spheres and discs have
    // radius 1.
    let half = select(p.scale, p.scale * 0.5, mask == M_TEXTURE);
    if ((flags & F_GROUND) != 0u || kind == K_RAIN_FLASH) {
        // Lying on the ground: the quad's x along the spin (or the direction of travel),
        // its y towards −z, turned by the root.
        var ax = vec3f(cos(p.rotation), 0.0, -sin(p.rotation));
        var ay = vec3f(-sin(p.rotation), 0.0, -cos(p.rotation));
        if (kind == K_MISSILE || kind == K_WAVEFRONT) {
            let d = e.rotation.xyz;
            let flat_dir = normalize(vec2f(d.x, d.z) + vec2f(1e-6, 0.0));
            ax = vec3f(flat_dir.x, 0.0, flat_dir.y);
            ay = vec3f(flat_dir.y, 0.0, -flat_dir.x);
        } else if (dot(p.axis.xz, p.axis.xz) > 1e-8) {
            // A streak lying on the ground: its length along its travel over the ground.
            let along = normalize(p.axis.xz);
            ay = vec3f(along.x, 0.0, along.y);
            ax = vec3f(along.y, 0.0, -along.x);
        } else {
            ax = rotate(e.rotation, ax);
            ay = rotate(e.rotation, ay);
        }
        offset = ax * c.x * half.x + ay * c.y * half.y;
    } else if (dot(p.axis, p.axis) > 0.0) {
        // Along an axis (streaks, rain): the quad's length follows the axis as the eye sees
        // it, its width across.
        var along = vec2f(dot(p.axis, view.right.xyz), dot(p.axis, view.up.xyz));
        along = select(vec2f(0.0, 1.0), normalize(along), dot(along, along) > 1e-8);
        let across = vec2f(-along.y, along.x);
        let r = across * c.x * half.x + along * c.y * half.y;
        offset = view.right.xyz * r.x + view.up.xyz * r.y;
    } else {
        let s = sin(p.rotation);
        let co = cos(p.rotation);
        let q = c * half;
        let r = vec2f(co * q.x - s * q.y, s * q.x + co * q.y);
        offset = view.right.xyz * r.x + view.up.xyz * r.y;
    }
    let world = centre + offset;
    var out: VsOut;
    out.clip = frame.view_proj * vec4f(world, 1.0);
    out.world = world;
    out.coord = vec4f(c, c.x * 0.5 + 0.5, 0.5 - c.y * 0.5);
    out.color = vec4f(p.color, p.alpha);
    var blend = 0.0;
    if (mask == M_SMOKE || mask == M_FLAME) {
        // Procedural masks: the part's seed and life instead of a flipbook frame.
        out.rect = vec4f(p.flip_seed, p.flip_age, 0.0, 0.0);
        out.rect2 = out.rect;
    } else {
        let flip = flip_frames(e, p.flip_age, p.flip_life, p.flip_seed);
        out.rect = flip.a;
        out.rect2 = flip.b;
        blend = flip.blend;
    }
    out.mask = vec4u(mask, flags, e.ints2.x, e.ints2.y);
    out.look = vec4f(e.e5.x, e.e6.w, blend, 0.0);
    out.shade = vec4f(p.look.x, p.look.y, e.e8.z, p.look.z);
    return out;
}

// The pillar: an open cone (radius 1 at the base, 0.3 at the top) of 24 sides, both faces.
@vertex
fn vs_pillar(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VsOut {
    let packed = parts[ii];
    let e = emitters[packed >> 8u];
    let p = evaluate(e, packed & 255u);
    if (p.alpha <= 0.0) {
        return hidden();
    }
    let side = vi / 6u;
    let corner = CORNERS[vi % 6u];
    let a = (f32(side) + corner.x * 0.5 + 0.5) / 24.0 * TAU;
    let v = corner.y * 0.5 + 0.5;
    let radius = mix(1.0, 0.3, v) * p.scale.x;
    let height = e.shape.y * e.motion.x;
    let local = vec3f(cos(a) * radius, (v - 0.5) * height + e.shape.y * 0.5, sin(a) * radius);
    let world = e.position.xyz + rotate(e.rotation, local);
    var out: VsOut;
    out.clip = frame.view_proj * vec4f(world, 1.0);
    out.world = world;
    out.coord = vec4f(0.0, v, 0.0, 0.0);
    out.color = vec4f(p.color, p.alpha);
    out.rect = vec4f(1.0, 1.0, 0.0, 0.0);
    out.rect2 = out.rect;
    out.mask = vec4u(M_PILLAR, e.ints.y, NO_CURVES, 0u);
    out.look = vec4f(e.e5.x, e.e6.w, 0.0, 0.0);
    out.shade = vec4f(p.look.x, 0.0, 0.1, 1.0);
    return out;
}

// Smooth value noise for the soft masks.
fn value_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    let h = vec2u(vec2i(i) + vec2i(4096));
    let x0 = h.x * 73856093u;
    let x1 = (h.x + 1u) * 73856093u;
    let y0 = h.y * 19349663u;
    let y1 = (h.y + 1u) * 19349663u;
    let a = noise(x0 ^ y0);
    let b = noise(x1 ^ y0);
    let c = noise(x0 ^ y1);
    let d = noise(x1 ^ y1);
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

// How much of a fragment at `world` shows in front of the scene: 0 behind a surface,
// rising to 1 over `soft` metres in front of it (0: a hard cut).
fn depth_fade(clip: vec4f, world: vec3f, soft: f32) -> f32 {
    let stored = textureLoad(scene_depth, vec2i(clip.xy), 0).rg;
    let gap = stored.r + stored.g - view_depth(world);
    if (soft <= 0.0) {
        // A hard cut, with a hair of slack for marks lying on surfaces.
        return select(0.0, 1.0, gap >= -0.02);
    }
    return clamp(gap / soft, 0.0, 1.0);
}

// What an effect writes: its colour after the fog of war and the frame shade,
// premultiplied, with alpha that covers by the part's non-additive share. Alpha above 1
// brightens additive light (an intensity), never covers more than everything.
fn effect_out(color: vec3f, alpha: f32, additive: f32, world: vec3f) -> vec4f {
    let lit = apply_fog(world, color * frame_shade(world));
    let a = max(alpha, 0.0);
    return vec4f(lit * a, min(a, 1.0) * (1.0 - clamp(additive, 0.0, 1.0)));
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let r = length(in.coord.xy);
    let aa = max(fwidth(r), 1e-4);
    let inside = 1.0 - smoothstep(1.0 - aa, 1.0, r);
    var color = in.color.rgb;
    var alpha = in.color.a;
    let normal_blend = (in.mask.y & F_NORMAL) != 0u;
    switch in.mask.x {
        case M_TEXTURE: {
            var texel = textureSample(effect_texture, effect_sampler, in.coord.zw * in.rect.xy + in.rect.zw);
            if (in.look.z > 0.0) {
                let next = textureSample(effect_texture, effect_sampler, in.coord.zw * in.rect2.xy + in.rect2.zw);
                texel = mix(texel, next, in.look.z);
            }
            if ((in.mask.w & C_RAMP) != 0u) {
                // The texture's brightness, heated, picks the colour from the ramp.
                let value = clamp(texel.r * in.shade.w, 0.0, 1.0);
                color *= curve(in.mask.z, 2u, value).rgb;
            } else {
                color *= texel.rgb;
            }
            alpha *= texel.a;
            let threshold = in.shade.y;
            if (threshold > 0.0) {
                // Erosion: thin parts of the sprite go first, its dense centre last.
                let w = max(in.shade.z, 1e-3);
                alpha *= smoothstep(threshold - w, threshold + w, texel.a);
            }
        }
        case M_SPHERE: {
            // An unlit sphere seen from outside: its front and back faces both blend.
            alpha = select(2.0 * alpha, 1.0 - (1.0 - alpha) * (1.0 - alpha), normal_blend) * inside;
        }
        case M_FRESNEL: {
            alpha *= (1.0 - min(r * r, 1.0)) * inside;
        }
        case M_RING: {
            alpha *= smoothstep(0.83, 0.89, r) * (1.0 - smoothstep(0.93, 1.0, r));
        }
        case M_GLOW: {
            let g = 1.0 - smoothstep(0.0, 1.0, r);
            alpha *= g * g;
        }
        case M_SPARK: {
            alpha *= inside;
        }
        case M_PILLAR: {
            let y = in.coord.y;
            alpha *= smoothstep(0.0, 0.18, y) * (1.0 - smoothstep(0.55, 1.0, y));
        }
        case M_FLAME: {
            let seed = in.rect.x * 61.0;
            if (in.rect.y < 0.0) {
                // The glow at the base.
                let g = 1.0 - smoothstep(0.0, 1.0, r);
                alpha *= g * g;
            } else {
                // A lick: round at the root, narrowing upwards, its edge torn by noise
                // that streams upwards as it lives; a hot core, redder tips.
                let life = in.rect.y;
                let y = in.coord.y * 0.5 + 0.5;
                let width = mix(1.0, 0.22, y);
                let n = value_noise(in.coord.xy * vec2f(2.2, 1.6) + vec2f(seed, seed * 0.7 - life * 5.0))
                    + 0.5 * value_noise(in.coord.xy * 4.5 + vec2f(seed * 1.9, -life * 9.0));
                let d = length(vec2f(in.coord.x / width, in.coord.y)) + (n / 1.5 - 0.5) * 0.75;
                alpha *= 1.0 - smoothstep(0.3, 0.9, d);
                let core = (1.0 - smoothstep(0.0, 0.55, d)) * (1.0 - y);
                color = mix(color * mix(vec3f(1.0), vec3f(0.95, 0.55, 0.4), y), vec3f(1.0, 0.78, 0.4) * 1.7, core * 0.5);
            }
        }
        case M_SMOKE: {
            // A soft billow with a ragged edge that churns as it rises.
            let seed = in.rect.x * 97.0;
            let churn = in.rect.y * 1.5;
            let n = value_noise(in.coord.xy * 1.8 + vec2f(seed, seed * 1.7 - churn))
                + 0.5 * value_noise(in.coord.xy * 3.9 + vec2f(seed * 2.3 + churn, seed))
                + 0.25 * value_noise(in.coord.xy * 8.1 + vec2f(seed * 0.7, seed * 3.1 + churn));
            let d = r + (n / 1.75 - 0.5) * 0.55;
            let soft = 1.0 - smoothstep(0.15, 1.0, d);
            alpha *= soft;
            // Lit from above: the lower edge darker, the billows' tops lighter.
            color *= 0.7 + 0.3 * (in.coord.y * 0.5 + 0.5) + (n / 1.75 - 0.5) * 0.5;
        }
        default: {}
    }
    if (alpha <= 0.0) {
        discard;
    }
    if (in.look.x > 0.0 && !normal_blend) {
        // A white-hot core: where the sprite is densest its colour burns towards white at
        // its brightest channel, so sparks read crisp with their colour at the fringe.
        var density = alpha / max(in.color.a, 1e-4);
        if (in.mask.x == M_SPHERE || in.mask.x == M_FRESNEL || in.mask.x == M_GLOW) {
            density = 1.0 - smoothstep(0.0, 0.8, r);
        }
        let heat = in.look.x * smoothstep(0.35, 0.95, density);
        let peak = max(color.r, max(color.g, color.b));
        color = mix(color, vec3f(peak), heat);
    }
    alpha *= depth_fade(in.clip, in.world, in.look.y);
    if (alpha <= 0.0) {
        discard;
    }
    return effect_out(color, alpha, in.shade.x, in.world);
}

// --- Ribbons ---------------------------------------------------------------------------

struct RibbonOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) color: vec4f,
    // across the strip (−1…1), along it (texture u), mode
    @location(2) edge: vec3f,
    // additivity, soft fade (m)
    @location(3) @interpolate(flat) look: vec2f,
}

@vertex
fn vs_ribbon(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> RibbonOut {
    let r = ribbons[ii];
    var out: RibbonOut;
    var world: vec3f;
    // Corners a+, a−, b+, b− as [0, 1, 2, 2, 1, 3].
    let corners = array<u32, 6>(0u, 1u, 2u, 2u, 1u, 3u);
    if (r.color.a < 0.5) {
        // A camera-facing trail segment.
        let k = corners[vi];
        let a = r.p0.xyz;
        let b = r.p1.xyz;
        var side = cross(b - a, frame.eye.xyz - a);
        side = select(vec3f(1.0, 0.0, 0.0), normalize(side), dot(side, side) > 1e-12);
        let fade = select(r.p1.w, r.p0.w, k < 2u);
        let sign = select(-1.0, 1.0, k % 2u == 0u);
        world = select(b, a, k < 2u) + side * sign * r.p2.x * 0.5 * fade;
        out.color = vec4f(r.color.rgb * fade * fade, r.look.x);
        out.edge = vec3f(sign, 0.0, 0.0);
        out.look = vec2f(1.0, 0.4);
    } else if (r.color.a < 1.5) {
        // A blade ribbon quad: base a, tip a, tip b, base b.
        let quad = array<u32, 6>(0u, 1u, 2u, 0u, 2u, 3u);
        let points = array<vec4f, 4>(r.p0, r.p1, r.p2, r.p3);
        let q = points[quad[vi]];
        world = q.xyz;
        out.color = vec4f(r.color.rgb, q.w);
        out.edge = vec3f(0.0, 0.0, 1.0);
        out.look = vec2f(1.0, 0.4);
    } else {
        // A textured strip segment: each end turned across its own tangent towards the
        // eye, so neighbouring segments share their edges.
        let k = corners[vi];
        let at_a = k < 2u;
        let p = select(r.p1, r.p0, at_a);
        let tangent = select(r.p3, r.p2, at_a);
        var side = cross(tangent.xyz, frame.eye.xyz - p.xyz);
        side = select(vec3f(1.0, 0.0, 0.0), normalize(side), dot(side, side) > 1e-12);
        let sign = select(-1.0, 1.0, k % 2u == 0u);
        world = p.xyz + side * sign * p.w * 0.5;
        out.color = select(r.end, vec4f(r.color.rgb, r.look.x), at_a);
        out.edge = vec3f(sign, tangent.w, 2.0);
        out.look = r.look.yz;
    }
    out.clip = frame.view_proj * vec4f(world, 1.0);
    out.world = world;
    return out;
}

@fragment
fn fs_ribbon(in: RibbonOut) -> @location(0) vec4f {
    var color = in.color.rgb;
    var alpha = in.color.a;
    // Textured strips repeat along their length: sampled in uniform control flow (the
    // derivatives of the unwrapped coordinate pick the mip), used by strips only.
    let uv = vec2f(in.edge.y, 0.5 - in.edge.x * 0.5);
    let texel = textureSampleGrad(effect_texture, effect_sampler, vec2f(fract(uv.x), uv.y), dpdx(uv), dpdy(uv));
    if (in.edge.z > 1.5) {
        color *= texel.rgb;
        alpha *= texel.a;
    } else if (in.edge.z < 0.5) {
        alpha *= 1.0 - smoothstep(0.15, 1.0, abs(in.edge.x));
    }
    alpha *= depth_fade(in.clip, in.world, in.look.y);
    if (alpha <= 0.0) {
        discard;
    }
    return effect_out(color, alpha, in.look.x, in.world);
}
