// Static models (scenery, resources, buildings): instanced per model section, drawn
// from the GPU-culled visible lists, lit by the shared lighting model.

const VERTEX_TINT: u32 = 1u;
const VERTEX_OCCLUSION: u32 = 2u;
const HAS_TEAM: u32 = 1u;
const HAS_TEAM_MASK: u32 = 2u;
const FOLIAGE: u32 = 4u;
const DOUBLE_SIDED: u32 = 8u;
const HAS_NORMAL_MAP: u32 = 16u;
const HAS_OCCLUSION_MAP: u32 = 32u;
const PAINTED: u32 = 64u;
const ALPHA_MASK: u32 = 128u;
const ALPHA_BLEND: u32 = 256u;

struct Material {
    base_color: vec4f,
    // rgb emissive, a: normal scale
    emissive: vec4f,
    // roughness, metalness, alpha cutoff, occlusion strength
    params: vec4f,
    // backside lighting, crown normal weight, team source max, 0
    light: vec4f,
    // rgb: colour without an owner
    team_default: vec4f,
    // x: vertex colour use, y: flags
    flags: vec4u,
    // rgb colour, a strength (0: none)
    glow: vec4f,
    // per term: albedo weights xyz, w: smoothstep start
    glow_terms: array<vec4f, 3>,
    // smoothstep ends per term
    glow_to: vec4f,
}

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

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var team_mask: texture_2d<f32>;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var metal_rough_texture: texture_2d<f32>;
@group(1) @binding(5) var occlusion_texture: texture_2d<f32>;
@group(1) @binding(6) var emissive_texture: texture_2d<f32>;
@group(1) @binding(7) var material_sampler: sampler;

@group(2) @binding(0) var<storage, read> instances: array<Instance>;
@group(2) @binding(1) var<storage, read> visible: array<u32>;
@group(2) @binding(2) var<uniform> model: ModelInfo;

fn has(flag: u32) -> bool {
    return (material.flags.y & flag) != 0u;
}

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

fn albedo_of(uv: vec2f, color: vec4f, instance: u32) -> vec4f {
    let texel = textureSample(base_texture, material_sampler, uv);
    // Team-coloured materials keep the authored texture outside the mask: the factor is
    // the colour without an owner (old `maskedTeamColor` replaces the map lookup).
    var base = select(material.base_color * texel, vec4f(texel.rgb, material.base_color.a * texel.a), has(HAS_TEAM));
    if (material.flags.x == VERTEX_TINT) {
        base = vec4f(base.rgb * color.rgb, base.a);
    }
    let mask = textureSample(team_mask, material_sampler, uv).r;
    if (has(HAS_TEAM)) {
        let owner = instances[instance].color;
        let player = select(material.team_default.rgb, owner.rgb, owner.a > 0.5);
        let m = select(1.0, mask, has(HAS_TEAM_MASK));
        base = vec4f(team_color(base.rgb, m, player, material.light.z), base.a);
    }
    return base;
}

// Alpha-tested coverage: a sharpened alpha for alpha-to-coverage, so MSAA smooths the
// cut edge (D24); fully transparent texels are discarded.
fn coverage(alpha: f32) -> f32 {
    let cutoff = material.params.z;
    return clamp((alpha - cutoff) / max(fwidth(alpha), 1e-4) + 0.5, 0.0, 1.0);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> @location(0) vec4f {
    let frag = in.clip;
    let world_dx = dpdx(in.world);
    let world_dy = dpdy(in.world);
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);
    let base = albedo_of(in.uv, in.color, in.instance);
    let mr = textureSample(metal_rough_texture, material_sampler, in.uv);
    let occlusion_map = textureSample(occlusion_texture, material_sampler, in.uv).r;
    let normal_map = textureSample(normal_texture, material_sampler, in.uv).xyz * 2.0 - 1.0;
    let emissive = textureSample(emissive_texture, material_sampler, in.uv).rgb * material.emissive.rgb;
    var alpha = base.a;
    if (has(ALPHA_MASK)) {
        alpha = coverage(base.a);
        if (alpha <= 0.0) {
            discard;
        }
    } else if (!has(ALPHA_BLEND)) {
        alpha = 1.0;
    }

    var n = normalize(in.normal);
    if (has(DOUBLE_SIDED) && !front) {
        n = -n;
    }
    if (has(HAS_NORMAL_MAP)) {
        // Cotangent frame from screen derivatives (the pack has no tangents).
        let dp2perp = cross(world_dy, n);
        let dp1perp = cross(n, world_dx);
        let t = dp2perp * uv_dx.x + dp1perp * uv_dy.x;
        let b = dp2perp * uv_dx.y + dp1perp * uv_dy.y;
        let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-12));
        let scaled = vec3f(normal_map.xy * material.emissive.a, normal_map.z);
        n = normalize(mat3x3f(t * inv, b * inv, n) * scaled);
    }
    var occlusion = 1.0;
    if (material.flags.x == VERTEX_OCCLUSION) {
        occlusion = in.color.r;
    }
    if (has(HAS_OCCLUSION_MAP)) {
        occlusion *= mix(1.0, occlusion_map, material.params.w);
    }
    var surface: Surface;
    surface.position = in.world;
    surface.normal = n;
    surface.albedo = base.rgb;
    surface.roughness = material.params.x * mr.g;
    surface.metalness = material.params.y * mr.b;
    surface.occlusion = occlusion;
    surface.backside = material.light.x;
    surface.specular = select(1.0, 0.0, has(PAINTED));
    surface.model = LIGHT_STANDARD;
    let view = normalize(frame.eye.xyz - in.world);
    var glow = vec3f(0.0);
    if (material.glow.a > 0.0) {
        var amount = material.glow.a;
        for (var i = 0; i < 3; i++) {
            let term = material.glow_terms[i];
            amount *= smoothstep(term.w, material.glow_to[i], dot(base.rgb, term.xyz));
        }
        glow = material.glow.rgb * amount;
    }
    let color = surface_light(surface, view, frag.xy) + emissive + glow;
    return vec4f(color, alpha);
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
    let alpha = material.base_color.a * textureSample(base_texture, material_sampler, in.uv).a;
    if (alpha < material.params.z) {
        discard;
    }
}
