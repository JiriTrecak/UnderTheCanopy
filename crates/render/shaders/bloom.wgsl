// Bloom (old `BeautyPass` bloom): three levels at the post-effect resolution, each a
// 3×3 tent of the one before; the first keeps only what exceeds a soft threshold.

struct Bloom {
    // threshold, knee, tap radius, 1 on the first level
    params: vec4f,
    // source texel size
    texel: vec4f,
}

@group(1) @binding(2) var<uniform> bloom: Bloom;
@group(1) @binding(3) var source: texture_2d<f32>;
@group(1) @binding(4) var source_sampler: sampler;

fn bright(uv: vec2f) -> vec3f {
    let color = max(textureSampleLevel(source, source_sampler, uv, 0.0).rgb, vec3f(0.0));
    if (bloom.params.w < 0.5) {
        return color;
    }
    let threshold = bloom.params.x;
    let knee = bloom.params.y;
    let peak = max(color.r, max(color.g, color.b));
    var soft = clamp(peak - threshold + knee, 0.0, 2.0 * knee);
    soft = soft * soft / (4.0 * knee + 0.00001);
    return color * (max(peak - threshold, soft) / max(peak, 0.00001));
}

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4f {
    var sum = vec3f(0.0);
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let weight = select(1.0, 2.0, x == 0) * select(1.0, 2.0, y == 0);
            sum += bright(in.uv + vec2f(f32(x), f32(y)) * bloom.texel.xy * bloom.params.z) * weight;
        }
    }
    return vec4f(sum / 16.0, 1.0);
}
