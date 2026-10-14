// Placeholder water: a flat surface over wet cells, tinted by the water column like the
// old water body colour (no reflections, waves or ripples yet).

struct Look {
    // rgb shallow colour, a clarity
    shallow: vec4f,
    deep: vec4f,
    floor_color: vec4f,
}

struct AuxTerrain {
    // origin, span, vertices, 1 / vertices
    grid: vec4f,
}

@group(1) @binding(0) var heights: texture_2d<f32>;
@group(1) @binding(1) var<uniform> look: Look;
@group(1) @binding(4) var<uniform> aux: AuxTerrain;

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
}

@vertex
fn vs_main(@location(0) position: vec3f) -> VsOut {
    var out: VsOut;
    out.world = position;
    out.clip = frame.view_proj * vec4f(position, 1.0);
    return out;
}

fn ground(xz: vec2f) -> f32 {
    let last = aux.grid.y;
    let p = clamp(xz - aux.grid.x, vec2f(0.0), vec2f(last));
    let i = vec2u(min(floor(p), vec2f(last - 1.0)));
    let t = p - vec2f(i);
    let a = textureLoad(heights, i, 0).r;
    let b = textureLoad(heights, i + vec2u(1u, 0u), 0).r;
    let c = textureLoad(heights, i + vec2u(0u, 1u), 0).r;
    let d = textureLoad(heights, i + vec2u(1u, 1u), 0).r;
    return mix(mix(a, b, t.x), mix(c, d, t.x), t.y);
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let frag = in.clip;
    let column = in.world.y - ground(in.world.xz);
    if (column < 0.005) {
        discard;
    }
    let clarity = look.shallow.a;
    let transmittance = exp(-column / clarity * vec3f(1.7, 0.9, 0.65));
    let body = mix(look.shallow.rgb, look.deep.rgb, 1.0 - exp(-column / (clarity * 1.5)));
    let lit = body * (frame.sun.rgb * max(frame.sun_direction.y, 0.0) / PI * sun_visibility(in.world, vec3f(0.0, 1.0, 0.0), frag.xy)
        + frame.ambient.rgb) * canopy_light(in.world);
    let alpha = 1.0 - dot(transmittance, vec3f(1.0 / 3.0));
    return vec4f(lit, clamp(alpha, 0.0, 1.0));
}
