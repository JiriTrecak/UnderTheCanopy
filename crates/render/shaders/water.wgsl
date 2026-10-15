// Stylised water (renderer §4.6): the map's water surface per vertex with depth-scaled
// swells, a body colour from the water column (absorption, shallow and deep colours of
// the cell's profile), caustics on the bed, the old reflection cube and sun glints,
// shoreline, crest and flow foam, and the unit wakes from the ripple field.
//
// Drawn in the forward pass after every opaque surface, depth-tested, without depth
// writes, in two draws over the same fragments and no copy of the scene: `fs_absorb`
// multiplies the bed by the hue of the water column's absorption (luminance 1), then
// `fs_main` blends the surface over it with premultiplied alpha, whose alpha carries
// everything else that dims the bed. Together: bed · absorption · dimming + surface,
// the old water's per-channel transmittance exactly.

struct WaterInfo {
    // origin (world x/z of vertex 0), cells per side, vertices per side, 0
    grid: vec4f,
    // ripple field: min x, min z, 1 / span, 0
    ripples: vec4f,
    // rgb: the daylight's ambient irradiance (unscaled by the biome)
    ambient: vec4f,
}

struct Profile {
    // rgb shallow colour (linear), a clarity
    shallow: vec4f,
    // rgb deep colour, a reflection strength
    deep: vec4f,
    // ripple scale, ripple strength, caustic strength, foam strength
    effects: vec4f,
    // cloud strength, flow speed, 0, 0
    extra: vec4f,
    // swell height, length, speed, depth end
    swell: vec4f,
    // depth start, shallow strength, shore width, crest strength
    shore: vec4f,
}

@group(1) @binding(0) var levels: texture_2d<f32>;
@group(1) @binding(1) var depths: texture_2d<f32>;
@group(1) @binding(2) var flow_map: texture_2d<f32>;
@group(1) @binding(3) var hash_map: texture_2d<f32>;
@group(1) @binding(4) var caustics: texture_2d<f32>;
@group(1) @binding(5) var ripple_field: texture_2d<f32>;
@group(1) @binding(6) var reflection: texture_cube<f32>;
@group(1) @binding(7) var<storage, read> profiles: array<Profile>;
@group(1) @binding(8) var<uniform> water: WaterInfo;
@group(1) @binding(9) var clamp_linear: sampler;

const TAU: f32 = 6.2831853;

fn water_time() -> f32 {
    return frame.eye.w;
}

// --- Fields -------------------------------------------------------------------------

// Value noise with smooth interpolation from a 256² hash lattice (one filtered fetch).
fn water_noise(p: vec2f) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * (3.0 - 2.0 * f);
    return textureSampleLevel(hash_map, repeat_sampler, (i + u + 0.5) / 256.0, 0.0).r;
}

// Water above the bed at a world position (bilinear; negative where dry).
fn depth_at(p: vec2f) -> f32 {
    let uv = (p - water.grid.x + 0.5) / water.grid.z;
    return textureSampleLevel(depths, clamp_linear, uv, 0.0).r;
}

fn flow_at(p: vec2f) -> vec4f {
    return textureSampleLevel(flow_map, clamp_linear, (p - water.grid.x) / water.grid.y, 0.0);
}

fn profile_at(p: vec2f) -> Profile {
    let last = i32(water.grid.y) - 1;
    let cell = clamp(vec2i(floor(p - water.grid.x)), vec2i(0), vec2i(last));
    let index = u32(textureLoad(flow_map, cell, 0).a * 255.0 + 0.5);
    return profiles[min(index, arrayLength(&profiles) - 1u)];
}

fn openness(depth: f32, pr: Profile) -> f32 {
    return smoothstep(pr.shore.x, pr.swell.w, depth);
}

// Swell amplitude: scales with depth without changing phase; never exposes a shallow
// bed with a trough or moves the dry coastline.
fn swell_amplitude(depth: f32, pr: Profile) -> f32 {
    let open = openness(depth, pr);
    return min(max(depth, 0.0) * 0.18, pr.swell.x * mix(pr.shore.y, 1.0, open)) * smoothstep(0.0, 0.16, depth);
}

// One world-space wave field, bent and varied in broad moving patches, advected by
// the flow. Bounded by 1.
fn swell_field(world: vec2f, flow: vec2f, pr: Profile) -> f32 {
    let t = water_time();
    let p = (world - flow * t * 0.5) * (TAU / pr.swell.y);
    let tt = t * pr.swell.z;
    let drift = vec2f(tt * 0.035, -tt * 0.025);
    let warp = vec2f(water_noise(p * 0.38 + drift), water_noise(p * 0.31 - drift + 17.0)) - 0.5;
    let q = p + warp * 2.4;
    let packet = water_noise(p * 0.21 + vec2f(-tt * 0.045, tt * 0.018) + 31.0);
    let a = sin(dot(q, vec2f(0.93, 0.37)) - tt);
    let b = sin(dot(q, vec2f(-0.48, 0.88)) * 1.31 + tt * 0.73 + warp.x);
    let c = sin(dot(p, vec2f(0.3, 0.95)) * 1.87 - tt * 1.09 + warp.y * 3.0);
    let primary = mix(0.38, 0.58, packet);
    return (primary * a + (0.8 - primary) * b + 0.2 * c) * mix(0.8, 1.0, packet);
}

// --- Vertex -------------------------------------------------------------------------

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    // the undisplaced water level
    @location(1) level: f32,
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    let n = u32(water.grid.z);
    let cell = vec2u(vid % n, vid / n);
    let level = textureLoad(levels, cell, 0).r;
    let depth = textureLoad(depths, cell, 0).r;
    let xz = water.grid.x + vec2f(cell);
    let pr = profile_at(xz + 0.5);
    let flow = flow_at(xz).rg * 2.0 - 1.0;
    let height = swell_field(xz, flow, pr) * swell_amplitude(depth, pr);
    var out: VsOut;
    out.world = vec3f(xz.x, level + height, xz.y);
    out.level = level;
    out.clip = frame.view_proj * vec4f(out.world, 1.0);
    return out;
}

// --- Fragment -----------------------------------------------------------------------

// One filtered compare in the sun's map (the old water read one VSM sample); the water
// softens it further.
fn water_shadow(world: vec3f) -> f32 {
    if (frame.sun_direction.w < 0.5) {
        return 1.0;
    }
    let clip = frame.shadow_matrix * vec4f(world, 1.0);
    let uv = clip.xy * vec2f(0.5, -0.5) + 0.5;
    if (any(uv < vec2f(0.0)) || any(uv > vec2f(1.0)) || clip.z >= 1.0) {
        return 1.0;
    }
    return textureSampleCompareLevel(shadow_map, shadow_sampler, uv, clip.z - frame.shadow.w);
}

// Rounded foam islands from a moving cell pattern (distance to the nearest point).
fn foam_cells(p: vec2f) -> f32 {
    let cell = floor(p);
    let f = fract(p);
    var nearest = 2.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2f(f32(x), f32(y));
            let h = textureLoad(hash_map, vec2i(cell + o) & vec2i(255), 0).rg;
            let point = 0.5 + 0.3 * sin(h * TAU + water_time() * 0.25);
            nearest = min(nearest, length(o + point - f));
        }
    }
    return nearest;
}

// What the water column does to the bed's colour: the shallow colour's tint and the
// per-channel absorption.
fn absorption(column: f32, pr: Profile) -> vec3f {
    let clarity = max(0.2, pr.shallow.a);
    let transmittance = exp(-column / clarity * vec3f(1.7, 0.9, 0.65));
    let tint = pr.shallow.rgb / max(max(pr.shallow.r, pr.shallow.g), max(pr.shallow.b, 0.001));
    return mix(vec3f(1.0), tint, smoothstep(0.0, 0.35, column) * 0.32) * transmittance;
}

fn luminance(c: vec3f) -> f32 {
    return dot(c, vec3f(0.2126, 0.7152, 0.0722));
}

// First draw: multiplies the bed by the absorption's hue.
@fragment
fn fs_absorb(in: VsOut) -> SceneOut {
    let p = in.world.xz;
    let depth = max(depth_at(p), 0.0);
    let column = in.world.y - (in.level - depth);
    if (column < 0.005) {
        discard;
    }
    let bed = absorption(column, profile_at(p));
    var out: SceneOut;
    out.color = vec4f(bed / max(luminance(bed), 0.0001), 1.0);
    out.depth = vec2f(0.0);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> SceneOut {
    let p = in.world.xz;
    let t = water_time();
    let pixel = max(length(dpdx(p)), length(dpdy(p)));
    let depth = max(depth_at(p), 0.0);
    let bed = in.level - depth;
    let column = in.world.y - bed;

    let pr = profile_at(p);
    let flow = flow_at(p);
    let flow_dir = flow.rg * 2.0 - 1.0;
    let open = openness(depth, pr);

    // Swell normal by forward differences of the displaced height; the amplitude is
    // evaluated at the neighbours too, so normals follow the shallow/deep blend.
    let e = max(0.16, pixel * 0.75);
    let amplitude = swell_amplitude(depth, pr);
    let h0 = swell_field(p, flow_dir, pr);
    let hx = swell_field(p + vec2f(e, 0.0), flow_dir, pr) * swell_amplitude(depth_at(p + vec2f(e, 0.0)), pr);
    let hz = swell_field(p + vec2f(0.0, e), flow_dir, pr) * swell_amplitude(depth_at(p + vec2f(0.0, e)), pr);
    var slope = (vec2f(hx, hz) - h0 * amplitude) / e;
    let fine = pr.effects.y * mix(0.3, 0.65, open) * (1.0 - smoothstep(0.15, 0.75, pixel)) * smoothstep(0.0, 0.2, depth);
    let rq = p * pr.effects.x * 5.0 + vec2f(t * 0.14, -t * 0.09);
    slope += vec2f(cos(rq.x + sin(rq.y)), cos(rq.y * 0.87 + sin(rq.x))) * fine;
    let crest = smoothstep(0.48, 0.88, h0) * open * smoothstep(0.005, 0.12, amplitude);
    var normal = normalize(vec3f(-slope.x, 1.0, -slope.y));

    // Unit wakes: slopes of rings on this water level, saturated, faded in shallows.
    let ruv = (p - water.ripples.xy) * water.ripples.z;
    let disturbance = textureSampleLevel(ripple_field, clamp_linear, clamp(ruv, vec2f(0.0), vec2f(1.0)), 0.0);
    let edge = min(min(ruv.x, ruv.y), min(1.0 - ruv.x, 1.0 - ruv.y));
    let same_level = 1.0 - smoothstep(0.08, 0.25, abs(in.level - disturbance.b / max(disturbance.a, 0.00001)));
    var ripple = disturbance.rg / (1.0 + length(disturbance.rg) * 3.0);
    ripple *= same_level * smoothstep(0.0, 0.06, edge) * smoothstep(0.01, 0.12, depth);
    normal = normalize(normal + vec3f(-ripple.x, 0.0, -ripple.y));

    // Shoreline: distance to the bank from the bed slope (flat crossings get none).
    let bed_slope = -vec2f(
        depth_at(p + vec2f(0.5, 0.0)) - depth_at(p - vec2f(0.5, 0.0)),
        depth_at(p + vec2f(0.0, 0.5)) - depth_at(p - vec2f(0.0, 0.5)),
    );
    let bank = length(bed_slope);
    let to_shore = depth / max(bank, 0.001);
    let shore_aa = max(fwidth(to_shore), 0.04);
    let noise = water_noise(p * 0.65 + vec2f(t * 0.07, -t * 0.05));
    let foam_uv = p * 0.9 + vec2f(t * 0.035, -t * 0.024) + vec2f(noise, water_noise(p * 0.4 + 7.0)) * 0.5;
    // The cell distance changes by about its uv's rate per pixel.
    let cell_aa = max(pixel * 1.1, 0.025);

    if (column < 0.005) {
        discard;
    }

    // Light: the old water's sun and ambient terms, normalised for the body colour.
    let sun_dir = frame.sun_direction.xyz;
    let direct = frame.sun.rgb / PI;
    let shadow = water_shadow(in.world);
    let ambient = water.ambient.rgb * 0.6;
    let light = direct * max(sun_dir.y, 0.0) * mix(0.38, 1.0, shadow) + ambient;
    let full_sun = direct * max(sun_dir.y, 0.0) + ambient;
    let body_light = light / max(dot(full_sun, vec3f(0.2126, 0.7152, 0.0722)), 0.001);
    let canopy = canopy_light(in.world);

    let clarity = max(0.2, pr.shallow.a);
    let transmittance = exp(-column / clarity * vec3f(1.7, 0.9, 0.65));
    let absorbed = absorption(column, pr);
    // Caustics on the bed, faded at the shore and with depth.
    let cuv = vec2f(p.x, p.y) * 0.075;
    let ca = textureSampleLevel(caustics, repeat_sampler, cuv + vec2f(t * 0.006, 0.0), 0.0).r;
    let cb = textureSampleLevel(caustics, repeat_sampler, cuv * 0.83 - vec2f(0.0, t * 0.005), 0.0).r;
    let caustic_fade = smoothstep(0.08, 0.5, column) * exp(-column * 0.7);
    let caustic = ca * cb * pr.effects.z * 0.12 * light * caustic_fade * shadow;
    let body = mix(pr.shallow.rgb, pr.deep.rgb, 1.0 - exp(-column / (clarity * 1.5))) * body_light;

    // colour = bed · bed_tint · m + a: the absorption (first draw, its hue; here its
    // luminance), the scalar dimming `m` and what the water adds `a`.
    var m = luminance(absorbed);
    var a = caustic * absorbed + body * (1.0 - transmittance);
    // Small ripples catch light even in transparent shallows; broad swells shade deep
    // water in rounded bands.
    let ripple_light = clamp(1.0 + dot(ripple, normalize(sun_dir.xz + vec2f(0.001))) * 5.0, 0.7, 1.3);
    let ndl = max(dot(normal, sun_dir), 0.0);
    let bands = mix(1.0, 0.68 + ndl * 0.55, smoothstep(0.4, 2.0, column));
    m *= ripple_light * bands;
    a *= ripple_light * bands;

    // Reflections of the old woodland sky, bounded so the lake never turns milky.
    let to_eye = normalize(frame.eye.xyz - in.world);
    var reflected = textureSampleLevel(reflection, clamp_linear, reflect(-to_eye, normal), 0.0).rgb;
    reflected = reflected / (1.0 + reflected) * body_light * 0.45;
    let fresnel = 0.035 + 0.965 * pow(1.0 - clamp(dot(normal, to_eye), 0.0, 1.0), 5.0);
    let r = clamp((fresnel + 0.045 * open) * pr.deep.a, 0.0, 0.7);
    m *= 1.0 - r;
    a = a * (1.0 - r) + reflected * r;
    let half_light = normalize(to_eye + sun_dir);
    let highlight = pow(max(dot(normal, half_light), 0.0), 120.0);
    a += vec3f(0.65, 0.86, 0.94) * body_light * highlight * pr.deep.a * 0.10 * shadow;

    // Foam: broken islands along banks, streaks on swell crests, and flow foam.
    let width = pr.shore.z * (0.65 + 0.35 * sin(t * 0.8 + noise * 4.0));
    let shore_mask = (1.0 - smoothstep(width - shore_aa, width + shore_aa, to_shore)) * smoothstep(0.025, 0.14, bank);
    // Foam islands only where shore or flow foam can show (most water has neither).
    var islands = 0.0;
    if (shore_mask * pr.effects.w > 0.0005 || flow.b > 0.0) {
        let cells = foam_cells(foam_uv);
        islands = 1.0 - smoothstep(0.24 - cell_aa, 0.24 + cell_aa, cells);
    }
    let shore_foam = shore_mask * mix(0.18, islands, 0.82) * pr.effects.w;
    let crest_uv = vec2f(dot(p, vec2f(0.93, 0.37)) * 2.2, dot(p, vec2f(-0.37, 0.93)) * 0.55) + vec2f(noise, t * 0.06);
    let streaks = smoothstep(0.36, 0.65, water_noise(crest_uv));
    let crest_foam = crest * pr.shore.w * smoothstep(0.3, 0.64, noise) * streaks;
    let foam = clamp(shore_foam + crest_foam + flow.b * islands * pr.effects.w * 0.35, 0.0, 0.85);
    let foam_color = vec3f(0.76, 0.86, 0.84) * dot(body_light, vec3f(0.2126, 0.7152, 0.0722));
    m *= 1.0 - foam;
    a = a * (1.0 - foam) + foam_color * foam;

    // Slow cloud shading of the profile.
    let cloud = 1.0 - pr.extra.x * (0.5 + 0.5 * sin(p.x * 0.07 + p.y * 0.04 + t * 0.04));
    m *= cloud;
    a *= cloud * canopy;

    return scene_out(vec4f(a, 1.0 - m), in.world);
}
