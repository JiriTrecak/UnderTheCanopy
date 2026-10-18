// Volumetric haze and light shafts (old `AtmospherePass`): a bounded raymarch towards
// each surface at reduced resolution through a height-falling mist with drifting noise,
// lit by the sun where the shadow map and the canopy's openings let it through; then a
// depth-aware 3×3 filter. The final pass upsamples it with depth weights. No history.

struct Haze {
    // rgb fog colour (linear), a density
    fog: vec4f,
    // rgb shaft colour (sun colour × tint × intensity), a shaft density
    sun: vec4f,
    // base height, height falloff, sun strength, steps
    shape: vec4f,
    // noise scale, noise strength, drift speed, overhead strength
    noise: vec4f,
    // wind x, z; fog texel size
    wind: vec4f,
    // x: sine of the shafts' own elevation, y: 1 when set (else they follow the sun)
    beam: vec4f,
}

@group(1) @binding(2) var<uniform> haze: Haze;

const HAZE_SKY_DEPTH: f32 = 4000.0;
@group(1) @binding(3) var noise_volume: texture_3d<f32>;
@group(1) @binding(4) var openings: texture_2d<f32>;
@group(1) @binding(5) var haze_map: texture_2d<f32>;
@group(1) @binding(6) var clamp_sampler: sampler;

fn noise3(p: vec3f) -> f32 {
    let i = floor(p);
    var f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    return textureSampleLevel(noise_volume, repeat_sampler, (i + f + 0.5) / 32.0, 0.0).r;
}

// Single compare tap of the sun's map; outside it the mist is unlit (as the old pass).
fn sunlight(p: vec3f) -> f32 {
    if (frame.sun_direction.w < 0.5) {
        return 0.0;
    }
    let clip = frame.shadow_matrix * vec4f(p, 1.0);
    let uv = clip.xy * vec2f(0.5, -0.5) + 0.5;
    if (any(uv < vec2f(0.0)) || any(uv > vec2f(1.0)) || clip.z < 0.0 || clip.z > 1.0) {
        return 0.0;
    }
    return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, clip.z - 0.00025);
}

// The shafts' light direction: the sun's, or the sun's bearing at the biome's steeper
// elevation, so the beams fall from the canopy overhead under a low evening sun.
fn beam_direction() -> vec3f {
    let s = frame.sun_direction.xyz;
    if (haze.beam.y < 0.5) {
        return s;
    }
    let flat = normalize(vec2f(s.x, s.z) + vec2f(0.0001, 0.0));
    let up = haze.beam.x;
    let across = sqrt(max(0.0, 1.0 - up * up));
    return vec3f(flat.x * across, up, flat.y * across);
}

// Where the canopy lets light along `s` through (old `overheadOpening`).
fn opening(p: vec3f, s: vec3f) -> f32 {
    if (frame.canopy.w < 0.5 || p.y >= frame.canopy.y) {
        return 1.0;
    }
    if (s.y <= 0.01) {
        return 0.0;
    }
    let uv = (p.xz + s.xz * (frame.canopy.y - p.y) / s.y) * frame.canopy.x + frame.canopy_offset.xy;
    return textureSampleLevel(openings, repeat_sampler, uv, 0.0).r;
}

@fragment
fn fs_march(in: FullscreenOut) -> @location(0) vec4f {
    let close = view_fade();
    let depth = depth_at_uv(in.uv);
    // Past any camera's far plane is sky; the resolve averages edge pixels between a surface
    // and the background into such depths too.
    let sky = depth > HAZE_SKY_DEPTH;
    if (sky && close < 0.01) {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    // Close cameras look along the forest floor: the light hangs in the whole view, the
    // sky included, out to where the haze takes over.
    let reach = mix(96.0, 420.0, close);
    let end = world_at(in.uv, select(depth, reach, sky));
    let origin = view.eye.xyz;
    let ray = normalize(end - origin);
    let to_surface = length(end - origin);
    // Only a bounded segment near the surface, and only inside the mist layer.
    var start = max(0.0, to_surface - reach);
    let strength = mix(haze.noise.w, 1.0, close);
    let base = haze.shape.x;
    let falloff = haze.shape.y;
    let ceiling = base + falloff * 5.0;
    if (ray.y < -0.001) {
        start = max(start, (ceiling - origin.y) / ray.y);
    }
    if (start >= to_surface) {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    let steps = i32(haze.shape.w);
    let step_size = (to_surface - start) / f32(steps);
    let jitter = interleaved_gradient(in.clip.xy);
    let beam = beam_direction();
    let steep = haze.beam.y > 0.5;
    let phase = 0.65 + 0.7 * pow(max(0.0, dot(ray, beam)), 4.0);
    let time = view.eye.w;
    var transmission = 1.0;
    var scattered = vec3f(0.0);
    for (var i = 0; i < steps; i++) {
        let p = origin + ray * (start + (f32(i) + jitter) * step_size);
        let falling = exp(-max(0.0, p.y - base) / falloff);
        var amount = haze.fog.a * falling * strength;
        if (amount < 0.0001 && haze.sun.a < 0.00001) {
            continue;
        }
        let flow = vec3f(haze.wind.x, 0.08, haze.wind.y) * time * haze.noise.z;
        let drift = mix(1.0, 0.25 + 1.5 * noise3((p - flow) * haze.noise.x), haze.noise.y);
        // No haze or shafts where the viewer has not looked (old `visibleAt`).
        let seen = fog_light(p);
        amount *= drift * seen;
        let extinction = 1.0 - exp(-amount * step_size);
        // Steep beams follow the canopy's gaps alone: the shadow map holds the low sun's.
        // Their gaps are sharpened into separate columns of light.
        var gap = opening(p, beam);
        if (steep) {
            gap = smoothstep(0.6, 0.85, gap);
        }
        let shadowed = select(sunlight(p), 1.0, steep);
        let lit = shadowed * gap * haze.shape.z * phase;
        // Clear-air shafts stay within canopy openings: a source separate from the
        // extinction, so beams show without a uniform veil.
        let shaft = haze.sun.a * strength * step_size * falling * smoothstep(1.0, 6.0, p.y - base) * drift * seen;
        scattered += transmission * (extinction * haze.fog.rgb * 0.2 + (extinction + shaft) * haze.sun.rgb * lit);
        transmission *= 1.0 - extinction;
    }
    let revealed = select(fog_light(end), 1.0, sky);
    return vec4f(scattered * revealed, mix(1.0, transmission, revealed));
}

// Denoises the low-resolution haze, keeping depth discontinuities.
@fragment
fn fs_filter(in: FullscreenOut) -> @location(0) vec4f {
    let surface = surface_at(in.uv);
    let tolerance = max(0.67, length(surface - view.eye.xyz) * 0.03);
    let texel = haze.wind.zw;
    var sum = vec4f(0.0);
    var weight = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let uv = clamp(in.uv + vec2f(f32(x), f32(y)) * texel, texel * 0.5, vec2f(1.0) - texel * 0.5);
            let spatial = select(1.0, 2.0, x == 0) * select(1.0, 2.0, y == 0);
            let w = spatial * exp(-length(surface_at(uv) - surface) / tolerance);
            sum += textureSampleLevel(haze_map, clamp_sampler, uv, 0.0) * w;
            weight += w;
        }
    }
    return sum / max(weight, 0.00001);
}
