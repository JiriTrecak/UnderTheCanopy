// The minimap composite (minimap.rs): the baked overview darkened by the fog of war, then
// markers and the camera's footprint in minimap pixels.

struct Minimap {
    // 2 / size, −2 / size, fog on, 0
    scale: vec4f,
}

@group(0) @binding(0) var base_map: texture_2d<f32>;
@group(0) @binding(1) var fog_map: texture_2d<f32>;
@group(0) @binding(2) var map_sampler: sampler;
@group(0) @binding(3) var<uniform> minimap: Minimap;

struct BackgroundOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_background(@builtin(vertex_index) i: u32) -> BackgroundOut {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    var out: BackgroundOut;
    out.clip = vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

// The old overlay's darkness from the fog texture's light: in sight 0, explored 0.65
// (alpha 166), unexplored 1.
fn darkness(light: f32) -> f32 {
    let l = light * 255.0;
    if (l <= 90.0) {
        return 1.0 - l / 90.0 * (1.0 - 166.0 / 255.0);
    }
    return 166.0 / 255.0 * (255.0 - l) / 165.0;
}

@fragment
fn fs_background(in: BackgroundOut) -> @location(0) vec4f {
    let base = textureSampleLevel(base_map, map_sampler, in.uv, 0.0).rgb;
    var dark = 0.0;
    if (minimap.scale.z > 0.5) {
        dark = darkness(textureSampleLevel(fog_map, map_sampler, in.uv, 0.0).r);
    }
    return vec4f(base * (1.0 - dark), 1.0);
}

struct MarkerOut {
    @builtin(position) clip: vec4f,
    @location(0) color: vec4f,
}

@vertex
fn vs_marker(@location(0) position: vec2f, @location(1) color: vec4f) -> MarkerOut {
    var out: MarkerOut;
    out.clip = vec4f(position * minimap.scale.xy + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.color = color;
    return out;
}

@fragment
fn fs_marker(in: MarkerOut) -> @location(0) vec4f {
    return in.color;
}
