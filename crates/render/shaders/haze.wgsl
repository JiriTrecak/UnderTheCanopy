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
    // noise scale, noise strength, drift speed, 0
    noise: vec4f,
    // wind x, z; fog texel size
    wind: vec4f,
}

@group(1) @binding(2) var<uniform> haze: Haze;
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

// Where the canopy lets sunlight through (old `overheadOpening`).
fn opening(p: vec3f) -> f32 {
    let s = frame.sun_direction.xyz;
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
    let depth = depth_at_uv(in.uv);
    if (depth > BACKGROUND_DEPTH) {
        return vec4f(0.0, 0.0, 0.0, 1.0);
    }
    let end = world_at(in.uv, depth);
    let origin = view.eye.xyz;
    let ray = normalize(end - origin);
    let to_surface = length(end - origin);
    // Only a bounded segment near the surface, and only inside the mist layer.
    var start = max(0.0, to_surface - 96.0);
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
    let phase = 0.65 + 0.7 * pow(max(0.0, dot(ray, frame.sun_direction.xyz)), 4.0);
    let time = view.eye.w;
    var transmission = 1.0;
    var scattered = vec3f(0.0);
    for (var i = 0; i < steps; i++) {
        let p = origin + ray * (start + (f32(i) + jitter) * step_size);
        let falling = exp(-max(0.0, p.y - base) / falloff);
        var amount = haze.fog.a * falling;
        if (amount < 0.0001 && haze.sun.a < 0.00001) {
            continue;
        }
        let flow = vec3f(haze.wind.x, 0.08, haze.wind.y) * time * haze.noise.z;
        let drift = mix(1.0, 0.25 + 1.5 * noise3((p - flow) * haze.noise.x), haze.noise.y);
        // No haze or shafts where the viewer has not looked (old `visibleAt`).
        let seen = fog_light(p);
        amount *= drift * seen;
        let extinction = 1.0 - exp(-amount * step_size);
        let lit = sunlight(p) * opening(p) * haze.shape.z * phase;
        // Clear-air shafts stay within canopy openings: a source separate from the
        // extinction, so beams show without a uniform veil.
        let shaft = haze.sun.a * step_size * falling * smoothstep(1.0, 6.0, p.y - base) * drift * seen;
        scattered += transmission * (extinction * haze.fog.rgb * 0.2 + (extinction + shaft) * haze.sun.rgb * lit);
        transmission *= 1.0 - extinction;
    }
    let revealed = fog_light(end);
    return vec4f(scattered * revealed, mix(1.0, transmission, revealed));
}

// Denoises the low-resolution haze, keeping depth discontinuities.
@fragment
fn fs_filter(in: FullscreenOut) -> @location(0) vec4f {
    let surface = surface_at(in.uv);
    let texel = haze.wind.zw;
    var sum = vec4f(0.0);
    var weight = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let uv = clamp(in.uv + vec2f(f32(x), f32(y)) * texel, texel * 0.5, vec2f(1.0) - texel * 0.5);
            let spatial = select(1.0, 2.0, x == 0) * select(1.0, 2.0, y == 0);
            let w = spatial * exp(-length(surface_at(uv) - surface) * 1.5);
            sum += textureSampleLevel(haze_map, clamp_sampler, uv, 0.0) * w;
            weight += w;
        }
    }
    return sum / max(weight, 0.00001);
}
