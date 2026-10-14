// Caustics on the water bed (old `SourceCaustics`, `WaterCompute.fx::causticsPS`): the
// woodland caustics texture sampled with mirrored tiling, its red and green channels
// distorting two scrolling reads of its blue channel. Rendered every frame into a small
// repeating texture the water shader samples twice. The pack stores the image as sRGB
// colour; the old game read its raw values, so samples are re-encoded.

struct Params {
    // x: seconds
    time: vec4f,
}

@group(0) @binding(0) var source: texture_2d<f32>;
@group(0) @binding(1) var source_sampler: sampler;
@group(0) @binding(2) var<uniform> params: Params;

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    let uv = vec2f(f32((vid << 1u) & 2u), f32(vid & 2u));
    var out: VsOut;
    out.clip = vec4f(uv * 2.0 - 1.0, 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn mirrored(quad: vec2f, tc: vec2f, scale: f32) -> vec4f {
    let d = 1.0 - clamp(quad / 0.25, vec2f(0.0), vec2f(1.0));
    let both = d.x * d.y;
    var w = vec4f(max(d.x - both, 0.0), max(d.y - both, 0.0), both, 0.0);
    w.w = max(1.0 - w.x - w.y - w.z, 0.0);
    let s = tc * scale;
    let c = textureSampleLevel(source, source_sampler, s, 0.0).rgb * w.w
        + textureSampleLevel(source, source_sampler, s + vec2f(scale, 0.0), 0.0).rgb * w.x
        + textureSampleLevel(source, source_sampler, s + vec2f(0.0, scale), 0.0).rgb * w.y
        + textureSampleLevel(source, source_sampler, s + vec2f(scale), 0.0).rgb * w.z;
    // The image is data the old game read without colour decoding.
    return vec4f(srgb_encode(c), 1.0);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let t = params.time.x;
    let uv = in.uv;
    let distort = mirrored(uv, uv + vec2f(0.5, t * 0.1), 0.16).rg + mirrored(uv, uv - vec2f(0.0, t * 0.1), 0.16).rg;
    let light = mirrored(uv, uv + distort * 0.15 + vec2f(0.1, -0.035) * t, 1.12).b
        + mirrored(uv, uv - distort.yx * 0.15 + vec2f(-0.17, 0.0) * t, 1.4).b;
    return vec4f(light, 0.0, 0.0, 1.0);
}
