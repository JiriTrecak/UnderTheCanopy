// The one lighting model (renderer §4.2). Every lit material builds a `Surface` and
// calls `surface_light`; the material model picks how ambient light is gathered:
//
// - LIGHT_STANDARD (units, buildings, props, foliage): Lambert + GGX sun, flat ambient
//   and sky/ground hemisphere over π, environment irradiance and reflections.
// - LIGHT_GROUND (terrain): the old game's ground lighting: energy-conserving GGX sun,
//   ambient tinted by the ground probe, environment reflections.
//
// Both receive the sun's PCF shadow and the canopy's dappled shade.

const PI: f32 = 3.14159265;
const LIGHT_STANDARD: u32 = 0u;
const LIGHT_GROUND: u32 = 1u;

struct Surface {
    position: vec3f,
    normal: vec3f,
    albedo: vec3f,
    roughness: f32,
    metalness: f32,
    // Ambient occlusion, applied to indirect light only.
    occlusion: f32,
    // Wrap lighting for thin foliage, 0–1.
    backside: f32,
    // 1 with specular highlights and reflections, 0 for hand-painted surfaces.
    specular: f32,
    model: u32,
}

// Average radiance over the hemisphere around `n` (cosine weighted) from the
// environment's spherical harmonics.
fn environment_irradiance(n: vec3f) -> vec3f {
    let e = frame.environment;
    let c1 = 2.0 / 3.0;
    let c2 = 0.25;
    var r = e[0].rgb * 0.282095;
    r += c1 * 0.488603 * (e[1].rgb * n.y + e[2].rgb * n.z + e[3].rgb * n.x);
    r += c2 * 1.092548 * (e[4].rgb * n.x * n.y + e[5].rgb * n.y * n.z + e[7].rgb * n.x * n.z);
    r += c2 * 0.315392 * e[6].rgb * (3.0 * n.z * n.z - 1.0);
    r += c2 * 0.546274 * e[8].rgb * (n.x * n.x - n.y * n.y);
    return max(r, vec3f(0.0));
}

fn ground_probe(n: vec3f) -> vec3f {
    if (n.y >= 0.0) {
        return mix(frame.probe_side.rgb, frame.probe_up.rgb, n.y);
    }
    return mix(frame.probe_side.rgb, frame.probe_down.rgb, -n.y);
}

fn hemisphere(n: vec3f) -> vec3f {
    return mix(frame.ground.rgb, frame.sky.rgb, 0.5 * n.y + 0.5);
}

// Split-sum environment BRDF, analytic fit (Karis).
fn environment_brdf(f0: vec3f, roughness: f32, nv: f32) -> vec3f {
    let c0 = vec4f(-1.0, -0.0275, -0.572, 0.022);
    let c1 = vec4f(1.0, 0.0425, 1.04, -0.04);
    let r = roughness * c0 + c1;
    let a004 = min(r.x * r.x, exp2(-9.28 * nv)) * r.x + r.y;
    let ab = vec2f(-1.04, 1.04) * a004 + r.zw;
    return f0 * ab.x + ab.y;
}

const VOGEL_TAPS: i32 = 8;

// The Vogel disk, unrotated: tap i at radius sqrt((i + 0.5) / 8) and angle i · 2.3999632
// (the golden angle). Each pixel rotates it by one angle, so a pixel evaluates one sine
// and one cosine instead of one per tap.
const VOGEL_DISK = array<vec2f, 8>(
    vec2f(0.250000000, 0.000000000),
    vec2f(-0.319290081, 0.292495887),
    vec2f(0.048872433, -0.556876544),
    vec2f(0.402444525, 0.524917521),
    vec2f(-0.738535130, -0.130636375),
    vec2f(0.699604866, -0.445031495),
    vec2f(-0.234004003, 0.870483846),
    vec2f(-0.446271487, -0.859268154),
);

// Fraction of sunlight reaching `world`: an 8-tap rotated Vogel disk of hardware PCF
// lookups in the view-fitted sun map.
fn sun_visibility(world: vec3f, normal: vec3f, frag: vec2f) -> f32 {
    if (frame.sun_direction.w < 0.5) {
        return 1.0;
    }
    let p = world + normal * frame.shadow.z;
    let clip = frame.shadow_matrix * vec4f(p, 1.0);
    let uv = clip.xy * vec2f(0.5, -0.5) + 0.5;
    let depth = clip.z - frame.shadow.w;
    if (any(uv < vec2f(0.0)) || any(uv > vec2f(1.0)) || depth >= 1.0) {
        return 1.0;
    }
    let angle = interleaved_gradient(frag) * 6.2831853;
    let radius = max(frame.shadow.y, 0.5) * frame.shadow.x;
    let c = cos(angle) * radius;
    let s = sin(angle) * radius;
    var lit = 0.0;
    for (var i = 0; i < VOGEL_TAPS; i++) {
        let d = VOGEL_DISK[i];
        let offset = vec2f(d.x * c - d.y * s, d.x * s + d.y * c);
        lit += textureSampleCompareLevel(shadow_map, shadow_sampler, uv + offset, depth);
    }
    return lit / f32(VOGEL_TAPS);
}

// The overhead canopy's shade projected along the sun (old `overheadShade`): 0 in the
// open, the biome's strength under the crowns.
fn canopy_shade(p: vec3f) -> f32 {
    let s = frame.sun_direction.xyz;
    if (frame.canopy.w < 0.5 || p.y >= frame.canopy.y || s.y <= 0.01) {
        return 0.0;
    }
    let uv = (p.xz + s.xz * (frame.canopy.y - p.y) / s.y) * frame.canopy.x + frame.canopy_offset.xy;
    return textureSampleLevel(canopy_pattern, repeat_sampler, uv, 0.0).r * frame.canopy.z;
}

// The sun's share that comes through the canopy at `p`.
fn canopy_light(p: vec3f) -> f32 {
    return 1.0 - canopy_shade(p);
}

fn point_light_sum(s: Surface, diffuse: vec3f) -> vec3f {
    var sum = vec3f(0.0);
    for (var i = 0u; i < frame.counts.x; i++) {
        let light = point_lights[i];
        let to_light = light.position.xyz - s.position;
        let d = length(to_light);
        let range = light.position.w;
        if (d >= range) {
            continue;
        }
        let falloff = pow(clamp(1.0 - pow(d / range, 4.0), 0.0, 1.0), 2.0) / max(d * d, 0.01);
        let nl = max(dot(s.normal, to_light / max(d, 1e-4)), 0.0);
        sum += light.color.rgb * falloff * nl * diffuse / PI;
    }
    return sum;
}

// Outgoing linear radiance of a surface lit by the frame's environment.
fn surface_light(s: Surface, view: vec3f, frag: vec2f) -> vec3f {
    let n = s.normal;
    let l = frame.sun_direction.xyz;
    let h = normalize(l + view);
    let nl = mix(clamp(dot(n, l), 0.0, 1.0), 1.0, s.backside);
    let nv = abs(dot(n, view)) + 1e-5;
    let nh = clamp(dot(n, h), 0.0, 1.0);
    let hv = clamp(dot(h, view), 0.0, 1.0);
    let r = max(s.roughness, 0.001);
    let a2 = r * r * r * r;
    let dd = (nh * a2 - nh) * nh + 1.0;
    let distribution = min(a2 / (PI * dd * dd), 32.0);
    let k = (r + 1.0) * (r + 1.0) / 8.0;
    let visibility = (nv / (nv * (1.0 - k) + k)) * (nl / (nl * (1.0 - k) + k));
    let f0 = mix(vec3f(0.04), s.albedo, s.metalness);
    let fresnel = f0 + (1.0 - f0) * pow(1.0 - hv, 5.0);
    let specular = distribution * visibility * fresnel * nl / (4.0 * nv * nl + 1e-5) * s.specular;
    let diffuse = s.albedo * (1.0 - s.metalness);
    // Surfaces facing away from the sun receive no direct light: skip the shadow taps.
    var shadowed = 0.0;
    if (nl > 0.0) {
        shadowed = sun_visibility(s.position, n, frag);
    }
    let sun = frame.sun.rgb * shadowed;
    let reflection = environment_irradiance(reflect(-view, n)) * environment_brdf(f0, r, nv) * s.specular;

    var direct: vec3f;
    var indirect: vec3f;
    if (s.model == LIGHT_GROUND) {
        direct = sun * (diffuse * (1.0 - fresnel) * (1.0 - s.metalness) * nl / PI + specular);
        let fs = exp2((-5.55473 * nv - 6.98316) * nv);
        let rough_f = vec3f((1.0 - r) * (1.0 - r));
        let fi = f0 + (max(rough_f, f0) - f0) * fs;
        let probe = mix(frame.probe_down.rgb, ground_probe(n), s.occlusion) * s.occlusion;
        indirect = frame.ambient.rgb * (1.0 - fi) * diffuse * (1.0 - s.metalness) * probe + reflection;
    } else {
        direct = sun * (diffuse * nl / PI + specular);
        let fe = environment_brdf(f0, r, nv);
        indirect = ((frame.ambient.rgb + hemisphere(n)) * diffuse / PI
            + diffuse * environment_irradiance(n) * (1.0 - fe)) * s.occlusion
            + reflection * s.occlusion;
    }
    // The crowns take the sun; the cool sky still reaches the shade beneath them, and
    // the lights of spells and fires shine in it undimmed.
    let shade = canopy_shade(s.position);
    return direct * (1.0 - shade) + indirect * (1.0 - shade * frame.canopy_offset.z)
        + point_light_sum(s, diffuse);
}

// Distance haze towards the surroundings colour (beyond the map only).
fn haze(color: vec3f, distance: f32) -> vec3f {
    let t = smoothstep(frame.haze_range.x, frame.haze_range.y, distance) * 0.82;
    return mix(color, frame.haze.rgb, t);
}
