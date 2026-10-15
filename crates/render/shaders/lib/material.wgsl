// Cooked model materials (group 1), shared by every model pass: statics and actors
// shade with the same function, so a material looks the same wherever it is drawn.
// Mirrors `MaterialUniform` in material.rs. Needs Frame, Team, Dither and Lighting.

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

@group(1) @binding(0) var<uniform> material: Material;
@group(1) @binding(1) var base_texture: texture_2d<f32>;
@group(1) @binding(2) var team_mask: texture_2d<f32>;
@group(1) @binding(3) var normal_texture: texture_2d<f32>;
@group(1) @binding(4) var metal_rough_texture: texture_2d<f32>;
@group(1) @binding(5) var occlusion_texture: texture_2d<f32>;
@group(1) @binding(6) var emissive_texture: texture_2d<f32>;
@group(1) @binding(7) var material_sampler: sampler;

fn has(flag: u32) -> bool {
    return (material.flags.y & flag) != 0u;
}

// `owner`: rgb owner colour, a > 0.5 when owned.
fn albedo_of(uv: vec2f, color: vec4f, owner: vec4f) -> vec4f {
    let texel = textureSample(base_texture, material_sampler, uv);
    // Team-coloured materials keep the authored texture outside the mask: the factor is
    // the colour without an owner (old `maskedTeamColor` replaces the map lookup).
    var base = select(material.base_color * texel, vec4f(texel.rgb, material.base_color.a * texel.a), has(HAS_TEAM));
    if (material.flags.x == VERTEX_TINT) {
        base = vec4f(base.rgb * color.rgb, base.a);
    }
    if (has(HAS_TEAM)) {
        let player = select(material.team_default.rgb, owner.rgb, owner.a > 0.5);
        // Maps a material lacks are not sampled (uniform branches on its flags).
        var m = 1.0;
        if (has(HAS_TEAM_MASK)) {
            m = textureSample(team_mask, material_sampler, uv).r;
        }
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

// What a material fragment needs from its pass.
struct MaterialFragment {
    // Fragment position (`@builtin(position)`).
    frag: vec4f,
    world: vec3f,
    normal: vec3f,
    uv: vec2f,
    color: vec4f,
    owner: vec4f,
    front: bool,
}

// Lit colour and coverage of one fragment; alpha 0 means discard.
fn shade_material(f: MaterialFragment) -> vec4f {
    let world_dx = dpdx(f.world);
    let world_dy = dpdy(f.world);
    let uv_dx = dpdx(f.uv);
    let uv_dy = dpdy(f.uv);
    let base = albedo_of(f.uv, f.color, f.owner);
    let mr = textureSample(metal_rough_texture, material_sampler, f.uv);
    // Maps a material lacks are not sampled (uniform branches on its parameters).
    var emissive = vec3f(0.0);
    if (any(material.emissive.rgb != vec3f(0.0))) {
        emissive = textureSample(emissive_texture, material_sampler, f.uv).rgb * material.emissive.rgb;
    }
    var alpha = base.a;
    if (has(ALPHA_MASK)) {
        alpha = coverage(base.a);
    } else if (!has(ALPHA_BLEND)) {
        alpha = 1.0;
    }

    var n = normalize(f.normal);
    if (has(DOUBLE_SIDED) && !f.front) {
        n = -n;
    }
    if (has(HAS_NORMAL_MAP)) {
        let normal_map = textureSample(normal_texture, material_sampler, f.uv).xyz * 2.0 - 1.0;
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
        occlusion = f.color.r;
    }
    if (has(HAS_OCCLUSION_MAP)) {
        let occlusion_map = textureSample(occlusion_texture, material_sampler, f.uv).r;
        occlusion *= mix(1.0, occlusion_map, material.params.w);
    }
    var surface: Surface;
    surface.position = f.world;
    surface.normal = n;
    surface.albedo = base.rgb;
    surface.roughness = material.params.x * mr.g;
    surface.metalness = material.params.y * mr.b;
    surface.occlusion = occlusion;
    surface.backside = material.light.x;
    surface.specular = select(1.0, 0.0, has(PAINTED));
    surface.model = LIGHT_STANDARD;
    let view = normalize(frame.eye.xyz - f.world);
    var glow = vec3f(0.0);
    if (material.glow.a > 0.0) {
        var amount = material.glow.a;
        for (var i = 0; i < 3; i++) {
            let term = material.glow_terms[i];
            amount *= smoothstep(term.w, material.glow_to[i], dot(base.rgb, term.xyz));
        }
        glow = material.glow.rgb * amount;
    }
    let color = surface_light(surface, view, f.frag.xy) + emissive + glow;
    return vec4f(color, alpha);
}

// Alpha test of the depth passes.
fn masked_out(uv: vec2f) -> bool {
    let alpha = material.base_color.a * textureSample(base_texture, material_sampler, uv).a;
    return alpha < material.params.z;
}
