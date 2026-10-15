// Live models (units, buildings, items, felled trees, corpses, projectiles): instanced per
// model section from CPU-culled ranges, skinned on the GPU from the frame's joint palettes
// (skin.wgsl), shaded by the shared material (lib/material). Rigid models skin with one
// identity joint, so every live model takes the same path.

struct ModelInfo {
    // xyz quantisation origin
    position_min: vec4f,
    // xyz quantisation extent
    position_extent: vec4f,
    // uv origin xy, uv extent zw
    uv: vec4f,
    // leaf flutter amplitude, 0, 0, 0
    wind: vec4f,
}

struct Actor {
    r0: vec4f,
    r1: vec4f,
    r2: vec4f,
    // rgb: owner colour; a: 1 when owned
    color: vec4f,
    // palette base, joints, 0, 0
    palette: vec4u,
    // opacity (screen-door below 1), 0, 0, 0
    look: vec4f,
    // multiplies the shaded colour (rgb) and coverage (a): effect models
    tint: vec4f,
}

@group(2) @binding(0) var<storage, read> actors: array<Actor>;
@group(2) @binding(1) var<storage, read> palette: array<vec4f>;
@group(2) @binding(2) var<uniform> model: ModelInfo;

struct VertexIn {
    @location(0) position: vec4f,
    @location(1) normal: vec2f,
    @location(2) uv: vec2f,
    @location(3) color: vec4f,
    @location(4) leaf: vec4f,
    @location(5) joints: vec4u,
    @location(6) weights: vec4f,
}

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) color: vec4f,
    @location(4) @interpolate(flat) instance: u32,
}

// The vertex's blended skinning matrix.
fn skin_matrix(base: u32, joints: vec4u, weights: vec4f) -> Affine {
    var r0 = vec4f(0.0);
    var r1 = vec4f(0.0);
    var r2 = vec4f(0.0);
    for (var i = 0u; i < 4u; i++) {
        let w = weights[i];
        if (w > 0.0) {
            let at = (base + joints[i]) * 3u;
            r0 += palette[at] * w;
            r1 += palette[at + 1u] * w;
            r2 += palette[at + 2u] * w;
        }
    }
    return Affine(r0, r1, r2);
}

// Inverse-transpose of an affine transform's linear part (correct under non-uniform scale).
fn normal_matrix(m: Affine) -> mat3x3f {
    let a = vec3f(m.r0.x, m.r1.x, m.r2.x);
    let b = vec3f(m.r0.y, m.r1.y, m.r2.y);
    let c = vec3f(m.r0.z, m.r1.z, m.r2.z);
    return mat3x3f(cross(b, c), cross(c, a), cross(a, b));
}

fn actor_affine(a: Actor) -> Affine {
    return Affine(a.r0, a.r1, a.r2);
}

@vertex
fn vs_main(in: VertexIn, @builtin(instance_index) id: u32) -> VsOut {
    let actor = actors[id];
    let m = actor_affine(actor);
    var local = dequantize(in.position.xyz, model.position_min.xyz, model.position_extent.xyz);
    if (has(FOLIAGE)) {
        local = leaf_flutter(local, in.leaf, affine_translation(m), model.wind.x, time_seconds());
    }
    let skin = skin_matrix(actor.palette.x, in.joints, in.weights);
    let posed = affine_point(skin, local);
    let world = affine_point(m, posed);
    let n = normal_matrix(m) * (normal_matrix(skin) * oct_decode(in.normal));
    var out: VsOut;
    out.world = world;
    out.clip = frame.view_proj * vec4f(world, 1.0);
    out.normal = normalize(n);
    out.uv = model.uv.xy + in.uv * model.uv.zw;
    out.color = in.color;
    out.instance = id;
    return out;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> SceneOut {
    let actor = actors[in.instance];
    var f: MaterialFragment;
    f.frag = in.clip;
    f.world = in.world;
    f.normal = in.normal;
    f.uv = in.uv;
    f.color = in.color;
    f.owner = actor.color;
    f.front = front;
    let shaded = shade_material(f) * actor.tint;
    if ((has(ALPHA_MASK) && shaded.a <= 0.0) || dither_discard(1.0 - actor.look.x, in.clip.xy)) {
        discard;
    }
    return scene_out(shaded, in.world);
}

// --- Shadows -------------------------------------------------------------------------

@vertex
fn vs_depth(
    @location(0) position: vec4f,
    @location(5) joints: vec4u,
    @location(6) weights: vec4f,
    @builtin(instance_index) id: u32,
) -> @builtin(position) vec4f {
    let actor = actors[id];
    let local = dequantize(position.xyz, model.position_min.xyz, model.position_extent.xyz);
    let skin = skin_matrix(actor.palette.x, joints, weights);
    let world = affine_point(actor_affine(actor), affine_point(skin, local));
    return frame.view_proj * vec4f(world, 1.0);
}

struct MaskedOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_masked(in: VertexIn, @builtin(instance_index) id: u32) -> MaskedOut {
    let actor = actors[id];
    let m = actor_affine(actor);
    var local = dequantize(in.position.xyz, model.position_min.xyz, model.position_extent.xyz);
    if (has(FOLIAGE)) {
        local = leaf_flutter(local, in.leaf, affine_translation(m), model.wind.x, time_seconds());
    }
    let skin = skin_matrix(actor.palette.x, in.joints, in.weights);
    let world = affine_point(m, affine_point(skin, local));
    var out: MaskedOut;
    out.clip = frame.view_proj * vec4f(world, 1.0);
    out.uv = model.uv.xy + in.uv * model.uv.zw;
    return out;
}

@fragment
fn fs_masked(in: MaskedOut) {
    if (masked_out(in.uv)) {
        discard;
    }
}
