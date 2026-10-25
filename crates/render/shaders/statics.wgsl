// Static models (scenery, stamps, standing resources): instanced per model section,
// drawn from the GPU-culled visible lists, shaded by the shared material (lib/material).

struct ModelInfo {
    // xyz quantisation origin; w: top of the model (sway height)
    position_min: vec4f,
    // xyz quantisation extent; w: 1 when the trunk sways
    position_extent: vec4f,
    // uv origin xy, uv extent zw
    uv: vec4f,
    // leaf flutter amplitude, speed, canopy height (0: no cutaway), crown plant height
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

// `VsOut` for the lit pass after a depth prepass: it must place every vertex exactly as
// the prepass (it tests depth for equality), so the position is invariant.
struct VsOutInvariant {
    @builtin(position) @invariant clip: vec4f,
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
    return vertex(in, slot);
}

@vertex
fn vs_lit(in: VertexIn, @builtin(instance_index) slot: u32) -> VsOutInvariant {
    let v = vertex(in, slot);
    return VsOutInvariant(v.clip, v.world, v.normal, v.uv, v.color, v.instance);
}

fn vertex(in: VertexIn, slot: u32) -> VsOut {
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

// The shaded fragment of a section (cutaway and alpha test aside).
fn lit(in: VsOut, front: bool) -> vec4f {
    var f: MaterialFragment;
    f.frag = in.clip;
    f.world = in.world;
    f.normal = in.normal;
    f.uv = in.uv;
    f.color = in.color;
    f.owner = instances[in.instance].color;
    f.front = front;
    f.tint = vec3f(1.0);
    f.frost = frost_amount();
    f.unit = 0.0;
    // Scenery never works: active emission stays dark (D65).
    f.activity = 0.0;
    if (has(FOLIAGE)) {
        f.tint = foliage_variation(instances[in.instance]);
    }
    return shade_material(f);
}

fn cut(instance: u32, clip: vec4f, world: vec3f) -> bool {
    return cut_away(instance, clip.xy, world) || above_rts(instance, clip.xy, world);
}

// Sections the cutaway may open, alpha-tested and blended sections.
@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> SceneOut {
    if (cut(in.instance, in.clip, in.world)) {
        discard;
    }
    let shaded = lit(in, front);
    if (has(ALPHA_MASK) && shaded.a <= 0.0) {
        discard;
    }
    return scene_out(shaded, in.world);
}

// Opaque sections nothing can cut: no discard, so the GPU shades only the visible
// surface, never hidden back faces or models behind models.
@fragment
fn fs_opaque(in: VsOut, @builtin(front_facing) front: bool) -> SceneOut {
    return scene_out(lit(in, front), in.world);
}

// Opaque sections of canopy models the cutaway or the RTS ceiling may cut this frame draw
// twice: a depth prepass (`fs_prepass_cut`, which carries the discards) settles which
// samples they cover, then this pass shades exactly those samples (depth equal, no
// writes) without a discard.
@fragment
fn fs_lit(in: VsOutInvariant, @builtin(front_facing) front: bool) -> SceneOut {
    let v = VsOut(in.clip, in.world, in.normal, in.uv, in.color, in.instance);
    return scene_out(lit(v, front), in.world);
}

// The prepass places vertices exactly as `vs_lit` (the same computation, invariant) and
// passes on only what the cutaway reads.
struct PrepassOut {
    @builtin(position) @invariant clip: vec4f,
    @location(0) world: vec3f,
    @location(1) @interpolate(flat) instance: u32,
}

@vertex
fn vs_prepass(in: VertexIn, @builtin(instance_index) slot: u32) -> PrepassOut {
    let id = visible[slot];
    let inst = instances[id];
    let m = Affine(inst.r0, inst.r1, inst.r2);
    let placed = place(m, in.position.xyz, in.leaf, has(FOLIAGE));
    var out: PrepassOut;
    out.world = placed.world;
    out.clip = frame.view_proj * vec4f(placed.world, 1.0);
    out.instance = id;
    return out;
}

@fragment
fn fs_prepass_cut(in: PrepassOut) {
    if (cut(in.instance, in.clip, in.world)) {
        discard;
    }
}

// Foliage variation: every instance of a foliage model gets its own value and warmth from
// a hash of where it stands, so a field of one leaf or fern model never reads as a
// repeated pattern. Value ±18 %, a shift between cooler-green-brown and warmer-red.
fn foliage_variation(inst: Instance) -> vec3f {
    let at = vec2f(inst.r0.w, inst.r2.w);
    let h1 = fract(sin(dot(at, vec2f(12.9898, 78.233))) * 43758.5453);
    let h2 = fract(sin(dot(at, vec2f(39.3468, 11.135))) * 24634.6345);
    let value = 0.82 + 0.36 * h1;
    let warm = h2 * 2.0 - 1.0;
    return value * vec3f(1.0 + 0.08 * warm, 1.0, 1.0 - 0.14 * warm);
}

// The scenery cutaway (cutaway.rs, old `sceneryCutaway`): a canopy model at least 7.5 m
// tall stipples away in front of the viewer's units, never behind them or under their
// feet; the mask is filtered by hand (its texels are unfilterable floats).
fn cut_away(id: u32, frag: vec2f, world: vec3f) -> bool {
    if (model.wind.z <= 0.0 || frame.cutaway.x < 0.5) {
        return false;
    }
    let inst = instances[id];
    let scale_y = length(vec3f(inst.r0.y, inst.r1.y, inst.r2.y));
    if (model.wind.z * scale_y < 7.5) {
        return false;
    }
    let size = vec2f(textureDimensions(cutaway_mask));
    let pixel = frag * frame.viewport.zw * size - 0.5;
    let base = floor(pixel);
    let f = pixel - base;
    let depth = view_depth(world);
    var amount = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let offset = vec2f(f32(x), f32(y));
            let w = mix(1.0 - f, f, offset);
            let texel = clamp(vec2i(base + offset), vec2i(0), vec2i(size) - 1);
            let cut = textureLoad(cutaway_mask, texel, 0).rg;
            amount += w.x * w.y * cut.g * smoothstep(0.4, 1.2, cut.r - depth);
        }
    }
    return dither_discard(amount, frag);
}

// Giants above the RTS view: a canopy model's parts more than `RTS_CEILING` above its
// base stipple away as the camera pitches down (the surroundings' rule, surroundings.wgsl),
// so a tree can rise into the canopy for close cameras without covering the map from
// above. statics.rs keeps its own `RTS_CEILING` in step.
// Shadows keep the whole tree, so its crown still shades the ground.
const RTS_CEILING: f32 = 75.0;

fn above_rts(id: u32, frag: vec2f, world: vec3f) -> bool {
    if (model.wind.z <= 0.0) {
        return false;
    }
    if (world.y - instances[id].r1.w < RTS_CEILING) {
        return false;
    }
    return dither_discard(1.0 - view_fade(), frag);
}

// --- Shadows -------------------------------------------------------------------------

struct DepthOut {
    @builtin(position) clip: vec4f,
    // Height above the instance's base (the giants' shadow ceiling).
    @location(0) height: f32,
}

@vertex
fn vs_depth(@location(0) position: vec4f, @builtin(instance_index) slot: u32) -> DepthOut {
    let inst = instances[visible[slot]];
    let m = Affine(inst.r0, inst.r1, inst.r2);
    let placed = place(m, position.xyz, vec4f(0.0), false);
    var out: DepthOut;
    out.clip = frame.view_proj * vec4f(placed.world, 1.0);
    out.height = placed.world.y - inst.r1.w;
    return out;
}

// A giant's parts above the biome's shadow ceiling cast no shadow: the playable ground's
// light is the map's (and the drifting canopy's), never a crown's or a cap's; the bole or
// stalk below keeps its long shadow.
fn above_shadow_ceiling(height: f32) -> bool {
    return model.wind.z > 0.0 && height > frame.haze_range.w;
}

@fragment
fn fs_depth(in: DepthOut) {
    if (above_shadow_ceiling(in.height)) {
        discard;
    }
}

struct MaskedOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
    @location(1) height: f32,
}

@vertex
fn vs_masked(in: VertexIn, @builtin(instance_index) slot: u32) -> MaskedOut {
    let inst = instances[visible[slot]];
    let m = Affine(inst.r0, inst.r1, inst.r2);
    let placed = place(m, in.position.xyz, in.leaf, has(FOLIAGE));
    var out: MaskedOut;
    out.clip = frame.view_proj * vec4f(placed.world, 1.0);
    out.uv = model.uv.xy + in.uv * model.uv.zw;
    out.height = placed.world.y - inst.r1.w;
    return out;
}

@fragment
fn fs_masked(in: MaskedOut) {
    if (masked_out(in.uv) || above_shadow_ceiling(in.height)) {
        discard;
    }
}
