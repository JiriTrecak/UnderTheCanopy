// Ambient motes (motes.rs): dust that glints only where light falls through the canopy,
// and leaves drifting down. Generated from the instance index alone, in fields that
// follow the camera and wrap around it, so they cost no buffers and never run out.

struct Motes {
    // xyz centre of the dust field (ahead of the eye); w: its half size
    dust: vec4f,
    // xyz centre of the leaf field (the camera's target); w: its horizontal half size
    leaves: vec4f,
    // x: dust count, y: leaf count, z: leaf size (m), w: close view (0 RTS … 1)
    counts: vec4f,
    // x: sine of the light's elevation (0: the sun's), y: leaf field height, z: dust
    // brightness, w: seconds
    params: vec4f,
    // rgb: the light's colour (linear)
    color: vec4f,
    // xyz camera right, up (billboards)
    right: vec4f,
    up: vec4f,
}

@group(1) @binding(0) var<uniform> motes: Motes;
@group(1) @binding(1) var leaf_texture: texture_2d<f32>;
@group(1) @binding(2) var leaf_sampler: sampler;

fn hash3(n: u32) -> vec3f {
    var v = vec3u(n * 1664525u + 1013904223u, n * 22695477u + 1u, n * 1103515245u + 12345u);
    v = v * 1664525u + 1013904223u;
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    v ^= v >> vec3u(16u);
    v.x += v.y * v.z;
    v.y += v.z * v.x;
    v.z += v.x * v.y;
    return vec3f(v & vec3u(0xffffu)) / 65535.0;
}

// The corner of a two-triangle quad for vertex 0…5, in −1…1.
fn corner(vertex: u32) -> vec2f {
    let corners = array<vec2f, 6>(
        vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
        vec2f(-1.0, -1.0), vec2f(1.0, 1.0), vec2f(-1.0, 1.0),
    );
    return corners[vertex];
}

// A point of the field around `centre` with half size `half` that a mote at `seed`
// drifting by `drift` occupies: the field wraps, so motes stream through the view.
fn wrapped(seed: vec3f, drift: vec3f, centre: vec3f, half: vec3f) -> vec3f {
    let lo = centre - half;
    let q = fract((seed * 2.0 * half + drift - lo) / (2.0 * half));
    return lo + q * 2.0 * half;
}

// Light through the canopy at `p` along the light's direction (shafts fall steeply when
// the biome gives them their own elevation, haze.wgsl).
fn beam_light(p: vec3f) -> f32 {
    var s = frame.sun_direction.xyz;
    if (motes.params.x > 0.0) {
        let flat = normalize(vec2f(s.x, s.z) + vec2f(0.0001, 0.0));
        let across = sqrt(max(0.0, 1.0 - motes.params.x * motes.params.x));
        s = vec3f(flat.x * across, motes.params.x, flat.y * across);
    }
    if (frame.canopy.w < 0.5 || p.y >= frame.canopy.y || s.y <= 0.01) {
        return 1.0;
    }
    let uv = (p.xz + s.xz * (frame.canopy.y - p.y) / s.y) * frame.canopy.x + frame.canopy_offset.xy;
    let open = 1.0 - textureSampleLevel(canopy_pattern, repeat_sampler, uv, 0.0).r;
    return smoothstep(0.45, 0.8, open);
}

struct DustOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) quad: vec2f,
    @location(2) glow: f32,
    @location(3) @interpolate(flat) centre: vec3f,
}

@vertex
fn vs_dust(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> DustOut {
    let t = motes.params.w;
    let seed = hash3(index * 3u + 7u);
    let phase = seed * 6.2831853;
    let drift = vec3f(sin(t * 0.13 + phase.x) * 1.5 + t * 0.12, sin(t * 0.09 + phase.y) * 1.2 - t * 0.05, cos(t * 0.11 + phase.z) * 1.5);
    let half = vec3f(motes.dust.w);
    let p = wrapped(seed, drift, motes.dust.xyz, half);
    // Fade towards the field's faces so the wrap never pops.
    let edge = abs(p - motes.dust.xyz) / half;
    let fade = 1.0 - smoothstep(0.7, 1.0, max(edge.x, max(edge.y, edge.z)));
    let twinkle = 0.55 + 0.45 * sin(t * (1.3 + seed.x * 2.0) + phase.y);
    // A mote is a glint of one or two pixels wherever it floats: its size follows its
    // distance, and none come close enough to the lens to swell into a disc.
    let distance = length(p - frame.eye.xyz);
    let size = distance * (0.0012 + 0.0012 * seed.z);
    let near = smoothstep(3.0, 7.0, distance);
    let c = corner(vertex);
    var out: DustOut;
    out.world = p + (motes.right.xyz * c.x + motes.up.xyz * c.y) * size;
    out.clip = frame.view_proj * vec4f(out.world, 1.0);
    out.quad = c;
    out.glow = fade * near * twinkle * motes.counts.w;
    out.centre = p;
    if (out.glow < 0.002) {
        out.clip = vec4f(0.0, 0.0, -1.0, 1.0);
    }
    return out;
}

@fragment
fn fs_dust(in: DustOut) -> SceneOut {
    // Only motes in a beam of light show (the canopy pattern is a fragment-stage texture).
    let glow = in.glow * beam_light(in.centre);
    if (glow < 0.002) {
        discard;
    }
    let falloff = max(0.0, 1.0 - dot(in.quad, in.quad));
    let light = motes.color.rgb * motes.params.z * glow * falloff * falloff;
    return scene_out(vec4f(light, 0.0), in.world);
}

struct LeafOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) uv: vec2f,
    @location(2) normal: vec3f,
    @location(3) fade: f32,
}

@vertex
fn vs_leaf(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> LeafOut {
    let t = motes.params.w;
    let seed = hash3(index * 5u + 101u);
    let seed2 = hash3(index * 5u + 211u);
    let phase = seed2 * 6.2831853;
    let height = motes.params.y;
    // Falls at its own pace, sways and drifts with the breeze.
    let fall = 0.6 + 0.6 * seed2.x;
    let sway = vec3f(sin(t * 0.8 + phase.x) * 2.5 + t * 0.6, 0.0, cos(t * 0.6 + phase.y) * 2.0 + t * 0.25);
    let half = vec3f(motes.leaves.w, height * 0.5, motes.leaves.w);
    let centre = motes.leaves.xyz + vec3f(0.0, height * 0.5, 0.0);
    let p = wrapped(seed, sway - vec3f(0.0, t * fall, 0.0), centre, half);
    let edge = abs(p - centre) / half;
    let fade = 1.0 - smoothstep(0.8, 1.0, max(edge.x, max(edge.y, edge.z)));
    // Tumbling: two rotations at their own rates.
    let a = t * (0.9 + seed2.y) + phase.z;
    let b = t * (0.6 + seed2.z * 0.8) + phase.x;
    let ca = cos(a);
    let sa = sin(a);
    let cb = cos(b);
    let sb = sin(b);
    let axis_u = vec3f(ca, sa * cb, sa * sb);
    let axis_v = vec3f(-sa, ca * cb, ca * sb);
    let u = normalize(axis_u);
    let v = normalize(axis_v - u * dot(axis_v, u));
    let size = motes.counts.z * (0.7 + 0.6 * seed.z) * fade;
    let c = corner(vertex);
    var out: LeafOut;
    out.world = p + (u * c.x + v * c.y) * size * 0.5;
    out.clip = frame.view_proj * vec4f(out.world, 1.0);
    let flip = select(1.0, -1.0, seed2.x > 0.5);
    out.uv = vec2f(c.x * flip, -c.y) * 0.5 + 0.5;
    out.normal = cross(u, v);
    out.fade = fade;
    return out;
}

@fragment
fn fs_leaf(in: LeafOut, @builtin(front_facing) front: bool) -> SceneOut {
    let texel = textureSample(leaf_texture, leaf_sampler, in.uv);
    if (texel.a < 0.5 || in.fade < 0.02) {
        discard;
    }
    var n = normalize(in.normal);
    if (!front) {
        n = -n;
    }
    var surface: Surface;
    surface.position = in.world;
    surface.normal = n;
    surface.albedo = texel.rgb;
    surface.roughness = 0.7;
    surface.metalness = 0.0;
    surface.occlusion = 1.0;
    surface.backside = 0.6;
    surface.specular = 0.5;
    surface.model = LIGHT_STANDARD;
    let color = surface_light(surface, normalize(frame.eye.xyz - in.world), in.clip.xy);
    return scene_out(vec4f(color, 1.0), in.world);
}
