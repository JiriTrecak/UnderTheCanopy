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
    // cloud strength, flow speed, glint, 0
    extra: vec4f,
    // swell height, length, speed, depth end
    swell: vec4f,
    // depth start, shallow strength, shore width, crest strength
    shore: vec4f,
    // frozen water: shelf width (m), floes, cracks, 0 (none when the width is 0)
    ice: vec4f,
    // rgb the ice's colour (linear)
    ice_color: vec4f,
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

// Distance to dry ground at a world position (metres, at most 64).
fn shore_at(p: vec2f) -> f32 {
    let uv = (p - water.grid.x + 0.5) / water.grid.z;
    return textureSampleLevel(depths, clamp_linear, uv, 0.0).g;
}

// Cells of a cracked sheet: the distance to the nearest of a cell's points (x), to the
// second nearest (y) and the nearest cell's hash (z); the edge between cells is where
// y - x is small.
fn ice_cells(p: vec2f) -> vec3f {
    let cell = floor(p);
    let f = fract(p);
    var first = 4.0;
    var second = 4.0;
    var id = 0.0;
    for (var y = -1; y <= 1; y++) {
        for (var x = -1; x <= 1; x++) {
            let o = vec2f(f32(x), f32(y));
            let h = textureLoad(hash_map, vec2i(cell + o) & vec2i(255), 0).rg;
            let d = length(o + 0.15 + h * 0.7 - f);
            if (d < first) {
                second = first;
                first = d;
                id = h.x;
            } else if (d < second) {
                second = d;
            }
        }
    }
    return vec3f(first, second, id);
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
    // Sun glitter: sparse points on the ripples that flare and fade, a stylised stand-in
    // for the sun mirrored in moving water that a top-down camera never sees.
    if (pr.extra.z > 0.0) {
        // Stretched along the current (or a fixed axis on still water) into short streaks.
        let along = normalize(flow_dir + vec2f(0.0001, 0.0));
        let q = vec2f(dot(p, along) * 0.9, dot(p, vec2f(-along.y, along.x)) * 4.5);
        let gp = q + slope * 3.0 - vec2f(t * 0.6, 0.0);
        let flare = water_noise(gp + vec2f(t * 0.9, -t * 0.6)) * water_noise(gp * 1.7 - vec2f(t * 0.5, t * 0.8));
        // Broad drifting streaks gate the sparkles, so they gather in runs, not a sheet.
        let runs = smoothstep(0.55, 0.8, water_noise(p * vec2f(0.18, 0.18) + slope * 2.0 - flow_dir * t * 0.15));
        let glitter = smoothstep(0.5, 0.7, flare) * runs * smoothstep(0.02, 0.12, column) * (1.0 - smoothstep(0.3, 0.8, pixel));
        a += vec3f(0.85, 0.93, 1.0) * glitter * pr.extra.z * shadow * dot(body_light, vec3f(0.2126, 0.7152, 0.0722));
    }

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

    // Frozen water: a shelf of ice along the shore, its edge wandering, cracked into
    // plates and frosted; beyond it floes drift on the dark water. The ice is opaque.
    if (pr.ice.x > 0.0) {
        let shore = shore_at(p);
        let reach = pr.ice.x * (0.65 + 0.7 * water_noise(p * 0.11)) + (water_noise(p * 0.6) - 0.5) * 0.9;
        let aa = max(pixel * 1.5, 0.08);
        let shelf = 1.0 - smoothstep(reach - aa, reach + aa, shore);
        // Floes: plates of a slowly drifting cell sheet, kept by their hash, more of them
        // nearer the shelf.
        let drift = vec2f(t * 0.05, t * 0.03) + flow_dir * t * 0.2;
        // Two sheets, coarse and fine, so floes come in many sizes.
        let coarse = ice_cells((p + drift) * 0.2);
        let fine = ice_cells((p + drift * 1.3) * 0.45 + 17.0);
        let big = step(0.5, water_noise(p * 0.05 + 3.0));
        let floe_cells = select(fine, coarse, big > 0.5);
        let near = 1.0 - smoothstep(reach, reach + 16.0, shore);
        let kept = step(1.0 - pr.ice.y * mix(0.05, 0.5, near * near), floe_cells.z);
        // A floe is a whole cell of a cracked sheet, apart from its neighbours by a lead
        // of open water: angular plates, not discs.
        let lead = smoothstep(0.1, 0.22 + aa * 0.3, floe_cells.y - floe_cells.x);
        let floe = kept * lead * near * (1.0 - shelf);
        let ice = max(shelf, floe);
        // Open water is lighter and colder near the ice, darkest far out.
        let far_out = smoothstep(reach, reach + 26.0, shore);
        a += pr.shallow.rgb * body_light * (1.0 - far_out) * 0.22 * m;
        if (ice > 0.0) {
            // Cracks between the plates of the shelf, and fine frost over everything.
            let crack_cells = ice_cells(p * 0.45);
            let crack = (1.0 - smoothstep(0.0, 0.06 + aa * 0.1, crack_cells.y - crack_cells.x)) * pr.ice.z;
            let frost = water_noise(p * 2.3) * 0.5 + water_noise(p * 0.7) * 0.5;
            // Thinner, darker ice towards its edge, where the water shows through.
            let thick = smoothstep(0.0, 1.6, reach - shore) * shelf + floe * 0.7;
            var albedo = mix(pr.ice_color.rgb * 0.55, pr.ice_color.rgb, clamp(thick + 0.25, 0.0, 1.0));
            // Each floe its own thickness: thin ones darker and bluer, sunk in the water.
            let thin = floe * (1.0 - floe_cells.z) * (1.0 - shelf);
            albedo = mix(albedo, mix(pr.deep.rgb, pr.ice_color.rgb, 0.45), thin * 0.6);
            albedo = mix(albedo, vec3f(0.92, 0.96, 1.0), smoothstep(0.55, 0.9, frost) * 0.45);
            albedo *= 1.0 - crack * 0.55;
            // A bright rim where the shelf meets open water.
            let rim = (smoothstep(reach - 0.9, reach - 0.15, shore) * (1.0 - smoothstep(reach - 0.15, reach + aa, shore))) * shelf;
            albedo = mix(albedo, vec3f(0.95, 0.98, 1.0), rim * 0.6);
            // Lit as the water's body is (light normalised to the sunlit), a touch of sheen.
            let ice_light = body_light * mix(0.62, 1.0, shadow) * canopy;
            let sheen = pow(max(dot(vec3f(0.0, 1.0, 0.0), half_light), 0.0), 60.0) * 0.08 * shadow;
            let ice_color = albedo * ice_light * 0.85 + vec3f(sheen) * body_light;
            a = mix(a, ice_color, ice);
            m = mix(m, 0.0, ice);
        }
    }

    return scene_out(vec4f(a, 1.0 - m), in.world);
}
