// Per-frame values every pass sees (group 0). Mirrors `FrameUniform` in frame.rs.

struct Frame {
    view_proj: mat4x4f,
    shadow_matrix: mat4x4f,
    // xyz: camera position; w: seconds (wind, canopy drift)
    eye: vec4f,
    // width, height, 1/width, 1/height
    viewport: vec4f,
    // xyz towards the sun; w: shadows enabled (0/1)
    sun_direction: vec4f,
    // rgb: linear sun irradiance (after canopy clouds)
    sun: vec4f,
    ambient: vec4f,
    sky: vec4f,
    ground: vec4f,
    probe_up: vec4f,
    probe_side: vec4f,
    probe_down: vec4f,
    // environment radiance, order-2 spherical harmonics (y up), times its intensity
    environment: array<vec4f, 9>,
    // x: 1/scale, y: height, z: strength, w: enabled
    canopy: vec4f,
    // xy: drift offset in pattern units
    canopy_offset: vec4f,
    // x: 1/resolution, y: filter radius (texels), z: normal offset (world), w: depth bias
    shadow: vec4f,
    // rgb: background; w: unused
    background: vec4f,
    // rgb: haze colour; z/w unused; x of haze_range: start, y: end
    haze: vec4f,
    haze_range: vec4f,
    // x: point light count
    counts: vec4u,
    // x: 1 / fog size, y: fog on (0/1), z: fog size
    fog: vec4f,
    // x: scenery cutaway on (0/1)
    cutaway: vec4f,
}

struct PointLight {
    // xyz position, w range
    position: vec4f,
    // rgb linear colour × intensity
    color: vec4f,
}

@group(0) @binding(0) var<uniform> frame: Frame;
@group(0) @binding(1) var shadow_map: texture_depth_2d;
@group(0) @binding(2) var shadow_sampler: sampler_comparison;
@group(0) @binding(3) var canopy_pattern: texture_2d<f32>;
@group(0) @binding(4) var repeat_sampler: sampler;
@group(0) @binding(5) var<storage, read> point_lights: array<PointLight>;
@group(0) @binding(6) var fog_map: texture_2d<f32>;
@group(0) @binding(7) var fog_sampler: sampler;
// Scenery cutaway mask (cutaway.rs): r the covering unit's view depth, g its weight.
@group(0) @binding(8) var cutaway_mask: texture_2d<f32>;

fn time_seconds() -> f32 {
    return frame.eye.w;
}

// What an opaque surface writes in the forward pass: colour, and its linear view depth
// split into a half-precision value and the remainder (targets.rs, LINEAR_DEPTH_FORMAT).
struct SceneOut {
    @location(0) color: vec4f,
    @location(1) depth: vec2f,
}

fn encode_depth(d: f32) -> vec2f {
    // Keep 11 significant bits (exact in half precision); the remainder is far smaller.
    let hi = bitcast<f32>(bitcast<u32>(d) & 0xffffe000u);
    return vec2f(hi, d - hi);
}

// Distance of `world` along the camera's view direction (clip w).
fn view_depth(world: vec3f) -> f32 {
    return (frame.view_proj * vec4f(world, 1.0)).w;
}

// The fog of war's light at a world position: 1 in sight, about 0.35 explored, 0
// unexplored or outside the map (old `FogOfWar`); 1 everywhere without fog.
fn fog_light(world: vec3f) -> f32 {
    if (frame.fog.y < 0.5) {
        return 1.0;
    }
    let uv = (world.xz + 0.5) * frame.fog.x;
    if (any(uv < vec2f(0.0)) || any(uv > vec2f(1.0))) {
        return 0.0;
    }
    return textureSampleLevel(fog_map, fog_sampler, uv, 0.0).r;
}

// The old fog look: explored ground desaturates and cools, unexplored goes black.
fn apply_fog(world: vec3f, color: vec3f) -> vec3f {
    if (frame.fog.y < 0.5) {
        return color;
    }
    let light = fog_light(world);
    let gray = dot(color, vec3f(0.2126, 0.7152, 0.0722));
    return mix(vec3f(gray) * vec3f(0.82, 0.91, 1.0), color, smoothstep(0.25, 0.85, light)) * light;
}

fn scene_out(color: vec4f, world: vec3f) -> SceneOut {
    var out: SceneOut;
    out.color = vec4f(apply_fog(world, color.rgb), color.a);
    out.depth = encode_depth(view_depth(world));
    return out;
}
