// Static models (scenery, stamps, standing resources): instanced per model section,
// drawn from the GPU-culled visible lists, shaded by the shared material (lib/material).

struct ModelInfo {
    // xyz quantisation origin; w: top of the model (sway height)
    position_min: vec4f,
    // xyz quantisation extent; w: 1 when the trunk sways
    position_extent: vec4f,
    // uv origin xy, uv extent zw
    uv: vec4f,
    // leaf flutter amplitude, 0, 0, crown plant height
    wind: vec4f,
}

struct Instance {
    r0: vec4f,
    r1: vec4f,
    r2: vec4f,
    // rgb: owner colour; a: 1 when owned
    color: vec4f,
}

@group(2) @binding(0) var<storage, read> instances: array<Instance>;
@group(2) @binding(1) var<storage, read> visible: array<u32>;
@group(2) @binding(2) var<uniform> model: ModelInfo;

struct VertexIn {
    @location(0) position: vec4f,
    @location(1) normal: vec2f,
    @location(2) uv: vec2f,
    @location(3) color: vec4f,
    @location(4) leaf: vec4f,
}

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) color: vec4f,
    @location(4) @interpolate(flat) instance: u32,
}

struct Placed {
    world: vec3f,
    local: vec3f,
}

// Model space → world, with leaf flutter and trunk sway.
fn place(m: Affine, q: vec3f, leaf: vec4f, foliage: bool) -> Placed {
    let t = time_seconds();
    let anchor = affine_translation(m);
    var local = dequantize(q, model.position_min.xyz, model.position_extent.xyz);
    if (foliage) {
        local = leaf_flutter(local, leaf, anchor, model.wind.x, t);
    }
    var world = affine_point(m, local);
    if (model.position_extent.w > 0.5) {
        let scale_y = length(vec3f(m.r0.y, m.r1.y, m.r2.y));
        world = trunk_sway(world, anchor, local.y, max(model.position_min.w, 0.1), scale_y, t);
    }
    var out: Placed;
    out.world = world;
    out.local = local;
    return out;
}

// Inverse-transpose of the instance's linear part (correct under non-uniform scale).
fn normal_matrix(m: Affine) -> mat3x3f {
    let a = vec3f(m.r0.x, m.r1.x, m.r2.x);
    let b = vec3f(m.r0.y, m.r1.y, m.r2.y);
    let c = vec3f(m.r0.z, m.r1.z, m.r2.z);
    return mat3x3f(cross(b, c), cross(c, a), cross(a, b));
}

@vertex
fn vs_main(in: VertexIn, @builtin(instance_index) slot: u32) -> VsOut {
    let id = visible[slot];
    let inst = instances[id];
    let m = Affine(inst.r0, inst.r1, inst.r2);
    let foliage = has(FOLIAGE);
    let placed = place(m, in.position.xyz, in.leaf, foliage);
    var normal = normalize(normal_matrix(m) * oct_decode(in.normal));
    if (foliage && material.light.y > 0.0 && in.leaf.a > 0.0) {
        let rotation = mat3x3f(
            normalize(vec3f(m.r0.x, m.r1.x, m.r2.x)),
            normalize(vec3f(m.r0.y, m.r1.y, m.r2.y)),
            normalize(vec3f(m.r0.z, m.r1.z, m.r2.z)),
        );
        var radial = vec3f(placed.local.x, 0.0, placed.local.z);
        radial = select(vec3f(1.0, 0.0, 0.0), normalize(radial), length(radial) > 1e-4);
        let up = normalize(rotation * radial + vec3f(0.0, 1.0, 0.0));
        let crown = normalize(mix(vec3f(0.0, 1.0, 0.0), up, clamp((model.wind.w - 0.8) / 2.6, 0.0, 1.0)));
        normal = normalize(mix(normal, crown, material.light.y));
    }
    var out: VsOut;
    out.world = placed.world;
    out.clip = frame.view_proj * vec4f(placed.world, 1.0);
    out.normal = normal;
    out.uv = model.uv.xy + in.uv * model.uv.zw;
    out.color = in.color;
    out.instance = id;
    return out;
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> SceneOut {
    var f: MaterialFragment;
    f.frag = in.clip;
    f.world = in.world;
    f.normal = in.normal;
    f.uv = in.uv;
    f.color = in.color;
    f.owner = instances[in.instance].color;
    f.front = front;
    let shaded = shade_material(f);
    if (has(ALPHA_MASK) && shaded.a <= 0.0) {
        discard;
    }
    return scene_out(shaded, in.world);
}

// --- Shadows -------------------------------------------------------------------------

@vertex
fn vs_depth(@location(0) position: vec4f, @builtin(instance_index) slot: u32) -> @builtin(position) vec4f {
    let inst = instances[visible[slot]];
    let m = Affine(inst.r0, inst.r1, inst.r2);
    let placed = place(m, position.xyz, vec4f(0.0), false);
    return frame.view_proj * vec4f(placed.world, 1.0);
}

struct MaskedOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_masked(in: VertexIn, @builtin(instance_index) slot: u32) -> MaskedOut {
    let inst = instances[visible[slot]];
    let m = Affine(inst.r0, inst.r1, inst.r2);
    let placed = place(m, in.position.xyz, in.leaf, has(FOLIAGE));
    var out: MaskedOut;
    out.clip = frame.view_proj * vec4f(placed.world, 1.0);
    out.uv = model.uv.xy + in.uv * model.uv.zw;
    return out;
}

@fragment
fn fs_masked(in: MaskedOut) {
    if (masked_out(in.uv)) {
        discard;
    }
}
