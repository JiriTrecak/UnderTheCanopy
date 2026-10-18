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

// Contact shade upsampled with bilinear weights times depth similarity (old
// `contactShade`, which compared reconstructed positions). Most of the screen has no
// shade: when all four texels are unshaded the weights are skipped.
fn contact_shade(uv: vec2f, depth: f32) -> f32 {
    let size = post.sizes.xy;
    let pixel = uv * size - 0.5;
    let base = floor(pixel);
    let f = pixel - base;
    let last = vec2i(size) - 1;
    var shades: array<f32, 4>;
    var lowest = 1.0;
    for (var i = 0; i < 4; i++) {
        let texel = clamp(vec2i(base) + vec2i(i % 2, i / 2), vec2i(0), last);
        shades[i] = textureLoad(contact_map, texel, 0).r;
        lowest = min(lowest, shades[i]);
    }
    if (lowest > 0.996) {
        return 1.0;
    }
    var sum = 0.0;
    var weight = 0.0;
    for (var i = 0; i < 4; i++) {
        let offset = vec2f(f32(i % 2), f32(i / 2));
        let tuv = (base + offset + 0.5) / size;
        let bilinear = mix(1.0 - f, f, offset);
        let w = bilinear.x * bilinear.y * exp(-abs(depth_at_uv(tuv) - depth) * 2.0) + 0.00001;
        sum += shades[i] * w;
        weight += w;
    }
    return sum / weight;
}

// Depth of field for close cameras: the world beyond a few dozen metres goes soft, as a
// small creature's eye focused near it would see the forest, and the third-person view
// reads as tiny against giant scenery. Off at RTS pitches (the view fade). A golden-angle
// disk over the scene, each tap weighted by its own blur so the sharp foreground never
// smears into the background.
const DOF_START: f32 = 40.0;
const DOF_FULL: f32 = 260.0;
const DOF_RADIUS: f32 = 0.006;
const DOF_TAPS: i32 = 24;

fn dof_luminance(c: vec3f) -> f32 {
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

fn close_view() -> f32 {
    return clamp((view.forward.y + 0.65) / 0.3, 0.0, 1.0);
}

fn blur_of(depth: f32) -> f32 {
    return smoothstep(DOF_START, DOF_FULL, min(depth, 4000.0));
}

fn depth_of_field(uv: vec2f, frag: vec2f, color: vec3f, depth: f32, close: f32) -> vec3f {
    let blur = blur_of(depth) * close;
    if (blur < 0.01) {
        return color;
    }
    let radius = DOF_RADIUS * blur * vec2f(view.scene.y * view.scene.z, 1.0);
    // The disk turns per pixel, so its taps never line up into a repeated pattern, and
    // bright taps count less (a Karis average), so a single glint cannot stamp copies of
    // itself across the blur.
    let spin = fract(52.9829189 * fract(dot(floor(frag), vec2f(0.06711056, 0.00583715)))) * 6.2831853;
    let centre = 1.0 / (1.0 + dof_luminance(color));
    var sum = color * centre;
    var weight = centre;
    for (var i = 0; i < DOF_TAPS; i++) {
        let r = sqrt((f32(i) + 0.5) / f32(DOF_TAPS));
        let a = f32(i) * 2.39996 + spin;
        let tuv = uv + vec2f(cos(a), sin(a)) * r * radius;
        let tap = textureSampleLevel(scene, linear_sampler, tuv, 0.0).rgb;
        let w = blur_of(depth_at_uv(tuv)) / (1.0 + dof_luminance(tap));
        sum += tap * w;
        weight += w;
    }
    return mix(color, sum / weight, blur);
}

// The haze upsampled the same way.
fn haze_at(uv: vec2f, depth: f32) -> vec4f {
    let size = post.sizes.zw;
    let pixel = uv * size - 0.5;
    let base = floor(pixel);
    let f = pixel - base;
    var sum = vec4f(0.0);
    var weight = 0.0;
    var nearest = vec4f(0.0, 0.0, 0.0, 1.0);
    var nearest_gap = 1e30;
    // Depth differences count relative to the distance, so far views (close cameras looking
    // along the forest) keep their edges; a pixel no texel matches takes the closest one.
    let tolerance = max(0.67, min(depth, 4000.0) * 0.03);
    for (var y = 0; y < 2; y++) {
        for (var x = 0; x < 2; x++) {
            let offset = vec2f(f32(x), f32(y));
            let tuv = (base + offset + 0.5) / size;
            let bilinear = mix(1.0 - f, f, offset);
            // Beyond 4 km is sky (haze.wgsl), resolved edge pixels included.
            let gap = abs(min(depth_at_uv(tuv), 4000.0) - min(depth, 4000.0));
            let w = bilinear.x * bilinear.y * exp(-gap / tolerance);
            let texel = clamp(vec2i(base + offset), vec2i(0), vec2i(size) - 1);
            let value = textureLoad(haze_map, texel, 0);
            sum += value * w;
            weight += w;
            if (gap < nearest_gap) {
                nearest_gap = gap;
                nearest = value;
            }
        }
    }
    if (weight < 0.001) {
        return nearest;
    }
    return sum / weight;
}

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4f {
    var hdr = scene_color(in.uv, in.clip.xy);
    let effects = post.effects;
    let close = close_view();
    if (close > 0.0) {
        hdr = depth_of_field(in.uv, in.clip.xy, hdr, depth_at_uv(in.uv), close);
    }
    if (effects.x > 0.0 || effects.y > 0.0 || effects.z > 0.0) {
        let depth = depth_at_uv(in.uv);
        if (effects.x > 0.0) {
            hdr *= mix(1.0, contact_shade(in.uv, depth), effects.x);
        }
        if (effects.y > 0.0) {
            // Bloom carries the canopy shade so highlights cannot light the shade back up.
            let glow = textureSampleLevel(bloom_near, linear_sampler, in.uv, 0.0).rgb * 0.2
                + textureSampleLevel(bloom_mid, linear_sampler, in.uv, 0.0).rgb * 0.3
                + textureSampleLevel(bloom_far, linear_sampler, in.uv, 0.0).rgb * 0.5;
            hdr += glow * effects.y * canopy_light(world_at(in.uv, depth));
        }
        if (effects.z > 0.0) {
            let fog = haze_at(in.uv, depth);
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
