// What post passes know about the view (group 1, bindings 0 and 1): reconstructing
// surfaces from the resolved linear depth (targets.rs, LINEAR_DEPTH_FORMAT).

struct View {
    inv_view_proj: mat4x4f,
    // xyz camera position; w: seconds
    eye: vec4f,
    // xyz view direction
    forward: vec4f,
    // scene (internal) resolution: width, height, 1/width, 1/height
    scene: vec4f,
    // output resolution: width, height, 1/width, 1/height
    output: vec4f,
    // projection y scale (cot of half the vertical field of view), 0, 0, 0
    projection: vec4f,
}

@group(1) @binding(0) var<uniform> view: View;
@group(1) @binding(1) var depth_map: texture_2d<f32>;

const BACKGROUND_DEPTH: f32 = 30000.0;

fn linear_depth(texel: vec2i) -> f32 {
    let d = textureLoad(depth_map, texel, 0).rg;
    return d.x + d.y;
}

// Linear depth at the scene texel under `uv`.
fn depth_at_uv(uv: vec2f) -> f32 {
    let size = vec2i(view.scene.xy);
    return linear_depth(clamp(vec2i(uv * view.scene.xy), vec2i(0), size - 1));
}

fn view_ray(uv: vec2f) -> vec3f {
    let p = view.inv_view_proj * vec4f(uv.x * 2.0 - 1.0, 1.0 - uv.y * 2.0, 0.5, 1.0);
    return normalize(p.xyz / p.w - view.eye.xyz);
}

fn world_at(uv: vec2f, depth: f32) -> vec3f {
    let ray = view_ray(uv);
    return view.eye.xyz + ray * (depth / dot(ray, view.forward.xyz));
}

fn surface_at(uv: vec2f) -> vec3f {
    return world_at(uv, depth_at_uv(uv));
}

struct FullscreenOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) vid: u32) -> FullscreenOut {
    let uv = vec2f(f32((vid << 1u) & 2u), f32(vid & 2u));
    var out: FullscreenOut;
    out.clip = vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}
