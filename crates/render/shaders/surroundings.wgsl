// The forest beyond the map border (old `ForestSurroundingsLayer` materials): giant
// trunks, mushrooms and logs from the pack, generated limbs and broad leaves. Lit by the
// shared model, hazed towards the biome's haze colour with a glow towards the sun. Tall
// parts beyond the border dissolve at RTS pitches; leaves keep only the canopy's shaded
// crowns over the map.

const KIND_MODEL: f32 = 0.0;
const KIND_LIMB: f32 = 1.0;
const KIND_LEAF: f32 = 2.0;

struct ForestFrame {
    // map size, view fade, daylight, 0
    params: vec4f,
    haze: vec4f,
    // haze start, end
    range: vec4f,
    // rgb sun tint
    sun: vec4f,
}

struct Part {
    // xyz quantisation origin; w: kind
    position_min: vec4f,
    // xyz quantisation extent; w: 1 with a normal map
    position_extent: vec4f,
    uv: vec4f,
    base_color: vec4f,
    // roughness, metalness, normal scale, painted
    material: vec4f,
}

struct Instance {
    r0: vec4f,
    r1: vec4f,
    r2: vec4f,
    color: vec4f,
}

@group(1) @binding(0) var<uniform> forest: ForestFrame;
@group(1) @binding(1) var<uniform> part: Part;
@group(1) @binding(2) var<storage, read> instances: array<Instance>;
@group(1) @binding(3) var base_texture: texture_2d<f32>;
@group(1) @binding(4) var normal_texture: texture_2d<f32>;
@group(1) @binding(5) var metal_rough_texture: texture_2d<f32>;
@group(1) @binding(6) var material_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) uv: vec2f,
    @location(3) shade: f32,
    @location(4) @interpolate(flat) instance: u32,
}

fn placed(local: vec3f, normal: vec3f, uv: vec2f, shade: f32, inst: u32) -> VsOut {
    let i = instances[inst];
    let m = Affine(i.r0, i.r1, i.r2);
    let a = vec3f(m.r0.x, m.r1.x, m.r2.x);
    let b = vec3f(m.r0.y, m.r1.y, m.r2.y);
    let c = vec3f(m.r0.z, m.r1.z, m.r2.z);
    var out: VsOut;
    out.world = affine_point(m, local);
    out.normal = normalize(mat3x3f(cross(b, c), cross(c, a), cross(a, b)) * normal);
    out.uv = uv;
    out.shade = shade;
    out.instance = inst;
    out.clip = frame.view_proj * vec4f(out.world, 1.0);
    return out;
}

@vertex
fn vs_model(
    @location(0) position: vec4f,
    @location(1) normal: vec2f,
    @location(2) uv: vec2f,
    @builtin(instance_index) inst: u32,
) -> VsOut {
    let local = dequantize(position.xyz, part.position_min.xyz, part.position_extent.xyz);
    return placed(local, oct_decode(normal), part.uv.xy + uv * part.uv.zw, 1.0, inst);
}

@vertex
fn vs_mesh(
    @location(0) position: vec4f,
    @location(1) normal: vec4f,
    @location(2) uv: vec4f,
    @builtin(instance_index) inst: u32,
) -> VsOut {
    var local = position.xyz;
    if (part.position_min.w == KIND_LEAF) {
        // Leaves breathe slightly along their length.
        let i = instances[inst];
        let anchor = vec3f(i.r0.w, i.r1.w, i.r2.w);
        local.y += sin(time_seconds() * 0.65 + anchor.x * 0.025 + anchor.z * 0.03) * 0.014 * sin(uv.y * 3.14159);
    }
    return placed(local, normal.xyz, uv.xy, position.w, inst);
}

@fragment
fn fs_main(in: VsOut, @builtin(front_facing) front: bool) -> SceneOut {
    let kind = part.position_min.w;
    let fade = forest.params.y;
    let size = forest.params.x;
    let world_dx = dpdx(in.world);
    let world_dy = dpdy(in.world);
    let uv_dx = dpdx(in.uv);
    let uv_dy = dpdy(in.uv);
    let texel = textureSample(base_texture, material_sampler, in.uv);
    let normal_map = textureSample(normal_texture, material_sampler, in.uv).xyz * 2.0 - 1.0;
    let mr = textureSample(metal_rough_texture, material_sampler, in.uv);
    let inst = instances[in.instance];
    let anchor = vec2f(inst.r0.w, inst.r2.w);
    let crown = textureSampleLevel(canopy_pattern, repeat_sampler, anchor * frame.canopy.x + frame.canopy_offset.xy, 0.0).r;
    let dither = fract(dot(floor(in.clip.xy), vec2f(0.75487766, 0.56984029)));

    if (kind == KIND_LEAF) {
        // Over the map only the canopy's crowns remain; everything fades out at RTS pitches.
        let in_map = all(anchor >= vec2f(0.0)) && all(anchor <= vec2f(size));
        if ((in_map && crown < 0.38) || fade < 0.01 || dither > fade) {
            discard;
        }
    } else if (fade < 1.0) {
        // Tall trunks beyond the border dissolve before they project over the map.
        let w = in.world;
        let outside = max(max(-w.x, w.x - size), max(-w.z, w.z - size));
        if (outside > 24.0 && w.y > 24.0 && dither > fade) {
            discard;
        }
    }

    var n = normalize(in.normal);
    if (kind == KIND_LEAF && !front) {
        n = -n;
    }
    if (part.position_extent.w > 0.5) {
        let dp2perp = cross(world_dy, n);
        let dp1perp = cross(n, world_dx);
        let t = dp2perp * uv_dx.x + dp1perp * uv_dy.x;
        let b = dp2perp * uv_dx.y + dp1perp * uv_dy.y;
        let inv = inverseSqrt(max(max(dot(t, t), dot(b, b)), 1e-12));
        let scaled = vec3f(normal_map.xy * part.material.z, normal_map.z);
        n = normalize(mat3x3f(t * inv, b * inv, n) * scaled);
    }
    var albedo = (texel * part.base_color).rgb;
    if (kind == KIND_LEAF) {
        albedo = inst.color.rgb * in.shade;
    }
    var surface: Surface;
    surface.position = in.world;
    surface.normal = n;
    surface.albedo = albedo;
    surface.roughness = part.material.x * select(1.0, mr.g, kind == KIND_MODEL);
    surface.metalness = part.material.y * select(1.0, mr.b, kind == KIND_MODEL);
    surface.occlusion = 1.0;
    surface.backside = 0.0;
    surface.specular = 1.0 - part.material.w;
    surface.model = LIGHT_STANDARD;
    let to_eye = frame.eye.xyz - in.world;
    var color = surface_light(surface, normalize(to_eye), in.clip.xy);

    let sight = normalize(-to_eye);
    let glow = pow(max(0.0, dot(sight, frame.sun_direction.xyz)), 6.0);
    let distance_haze = smoothstep(forest.range.x, forest.range.y, length(to_eye));
    color = mix(color, forest.haze.rgb * forest.params.z + forest.sun.rgb * glow * 0.22, distance_haze * 0.82);
    if (kind == KIND_LEAF) {
        let midrib = 1.0 - smoothstep(0.008, 0.017, abs(in.uv.x - 0.5));
        let vein = 1.0 - smoothstep(0.018, 0.045, abs(fract(in.uv.y * 6.0 - abs(in.uv.x - 0.5) * 3.0) - 0.5));
        color *= 0.83 + 0.09 * sin(in.uv.y * 3.14159) + midrib * 0.22 + vein * 0.07;
        color += albedo * forest.sun.rgb * (0.07 + 0.16 * glow);
    }
    return scene_out(vec4f(color, 1.0), in.world);
}
