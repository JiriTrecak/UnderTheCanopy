// The final pass (renderer §4.1, step 9): upscale from the render scale with light
// sharpening, the biome's post effects (contact shade, bloom, volumetric haze; old
// atmosphere composite), then display encoding → the one tone mapper → the baked grade
// volume → vignette.
//
// The tone mapper is the old game's hue-preserving highlight shoulder in display space;
// at knee 0 it is bypassed and values clip at 1, exactly as today's outdoor frames.

struct Post {
    // x: vignette, y: shoulder knee (0 = clip), z: 1 when the output view encodes sRGB,
    // w: sharpening (0 at native scale)
    params: vec4f,
    // x: contact strength, y: bloom strength, z: 1 with haze, w: 0
    effects: vec4f,
    // contact and haze texture sizes (texels)
    sizes: vec4f,
}

@group(1) @binding(2) var scene: texture_2d<f32>;
@group(1) @binding(3) var grade: texture_3d<f32>;
@group(1) @binding(4) var linear_sampler: sampler;
@group(1) @binding(5) var<uniform> post: Post;
@group(1) @binding(6) var contact_map: texture_2d<f32>;
@group(1) @binding(7) var bloom_near: texture_2d<f32>;
@group(1) @binding(8) var bloom_mid: texture_2d<f32>;
@group(1) @binding(9) var bloom_far: texture_2d<f32>;
@group(1) @binding(10) var haze_map: texture_2d<f32>;

fn shoulder(c: vec3f, knee: f32) -> vec3f {
    let peak = max(c.r, max(c.g, c.b));
    if (knee <= 0.0 || peak <= knee) {
        return c;
    }
    let room = max(0.01, 1.0 - knee);
    let compressed = knee + room * (1.0 - exp(-(peak - knee) / room));
    return c * compressed / peak;
}

// The scene at `uv`: one texel at native scale; bilinear with a light unsharp mask
// when the scene renders at a lower resolution.
fn scene_color(uv: vec2f, frag: vec2f) -> vec3f {
    if (post.params.w <= 0.0) {
        return textureLoad(scene, vec2i(frag), 0).rgb;
    }
    let t = view.scene.zw;
    let c = textureSampleLevel(scene, linear_sampler, uv, 0.0).rgb;
    let n = textureSampleLevel(scene, linear_sampler, uv + vec2f(0.0, -t.y), 0.0).rgb
        + textureSampleLevel(scene, linear_sampler, uv + vec2f(0.0, t.y), 0.0).rgb
        + textureSampleLevel(scene, linear_sampler, uv + vec2f(-t.x, 0.0), 0.0).rgb
        + textureSampleLevel(scene, linear_sampler, uv + vec2f(t.x, 0.0), 0.0).rgb;
    return max(c + (c - n * 0.25) * post.params.w, vec3f(0.0));
}

// Contact shade upsampled with bilinear weights times surface similarity (old
// `contactShade`).
fn contact_shade(uv: vec2f, surface: vec3f) -> f32 {
    let size = post.sizes.xy;
    let pixel = uv * size - 0.5;
    let base = floor(pixel);
    let f = pixel - base;
    var sum = 0.0;
    var weight = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let offset = vec2f(f32(x), f32(y));
            let tuv = (base + offset + 0.5) / size;
            let bilinear = mix(1.0 - f, f, offset);
            let w = bilinear.x * bilinear.y * exp(-length(surface_at(tuv) - surface) * 2.0) + 0.00001;
            let texel = clamp(vec2i(base + offset), vec2i(0), vec2i(size) - 1);
            sum += textureLoad(contact_map, texel, 0).r * w;
            weight += w;
        }
    }
    return sum / weight;
}

// The haze upsampled the same way.
fn haze_at(uv: vec2f, surface: vec3f) -> vec4f {
    let size = post.sizes.zw;
    let pixel = uv * size - 0.5;
    let base = floor(pixel);
    let f = pixel - base;
    var sum = vec4f(0.0);
    var weight = 0.0;
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let offset = vec2f(f32(x), f32(y));
            let tuv = (base + offset + 0.5) / size;
            let bilinear = mix(1.0 - f, f, offset);
            let w = bilinear.x * bilinear.y * exp(-length(surface_at(tuv) - surface) * 1.5) + 0.00001;
            let texel = clamp(vec2i(base + offset), vec2i(0), vec2i(size) - 1);
            sum += textureLoad(haze_map, texel, 0) * w;
            weight += w;
        }
    }
    return sum / weight;
}

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4f {
    var hdr = scene_color(in.uv, in.clip.xy);
    let effects = post.effects;
    if (effects.x > 0.0 || effects.y > 0.0 || effects.z > 0.0) {
        let surface = surface_at(in.uv);
        if (effects.x > 0.0) {
            hdr *= mix(1.0, contact_shade(in.uv, surface), effects.x);
        }
        if (effects.y > 0.0) {
            // Bloom carries the canopy shade so highlights cannot light the shade back up.
            let glow = textureSampleLevel(bloom_near, linear_sampler, in.uv, 0.0).rgb * 0.2
                + textureSampleLevel(bloom_mid, linear_sampler, in.uv, 0.0).rgb * 0.3
                + textureSampleLevel(bloom_far, linear_sampler, in.uv, 0.0).rgb * 0.5;
            hdr += glow * effects.y * canopy_light(surface);
        }
        if (effects.z > 0.0) {
            let fog = haze_at(in.uv, surface);
            hdr = hdr * fog.a + fog.rgb;
        }
    }
    var display = srgb_encode(max(hdr, vec3f(0.0)));
    display = clamp(shoulder(display, post.params.y), vec3f(0.0), vec3f(1.0));
    let size = f32(textureDimensions(grade).x);
    let lut_uv = display * ((size - 1.0) / size) + 0.5 / size;
    display = textureSampleLevel(grade, linear_sampler, lut_uv, 0.0).rgb;
    display *= 1.0 - smoothstep(0.15, 0.72, length(in.uv - 0.5)) * post.params.x;
    if (post.params.z > 0.5) {
        return vec4f(srgb_decode(display), 1.0);
    }
    return vec4f(display, 1.0);
}
