// A map's dressing (dressing.rs): the generated cliffs, textured by projection from the
// three axes and lit like scenery, frosted with the biome; and decals, flat cut-outs on the
// ground, lit with an upward normal, faded under ice. Decal sheets carry their own frost.

struct CliffInfo {
    // 1 / tile, roughness, has normal map, frost share
    params: vec4f,
}

struct DecalInfo {
    // cells, 1 / cells, 0, 0
    params: vec4f,
}

struct Decal {
    position: vec3f,
    yaw: f32,
    size: f32,
    cell: f32,
    under: f32,
    pad: f32,
    tint: vec4f,
}

// --- Cliffs ---------------------------------------------------------------------------

@group(1) @binding(0) var<uniform> cliff: CliffInfo;
@group(1) @binding(1) var cliff_albedo: texture_2d<f32>;
@group(1) @binding(2) var cliff_normal: texture_2d<f32>;
@group(1) @binding(3) var cliff_sampler: sampler;

struct CliffOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) occlusion: f32,
}

@vertex
fn vs_cliff(@location(0) position: vec3f, @location(1) normal: vec3f, @location(2) occlusion: f32) -> CliffOut {
    var out: CliffOut;
    out.world = position;
    out.normal = normal;
    out.occlusion = occlusion;
    out.clip = frame.view_proj * vec4f(position, 1.0);
    return out;
}

@vertex
fn vs_cliff_shadow(@location(0) position: vec3f, @location(1) normal: vec3f, @location(2) occlusion: f32) -> @builtin(position) vec4f {
    return frame.view_proj * vec4f(position, 1.0);
}

@fragment
fn fs_cliff(in: CliffOut) -> SceneOut {
    var n = normalize(in.normal);
    // Triplanar: each axis's projection, weighted by how much the surface faces it.
    var w = pow(abs(n), vec3f(4.0));
    w = w / (w.x + w.y + w.z);
    let p = in.world * cliff.params.x;
    let ax = textureSample(cliff_albedo, cliff_sampler, p.zy);
    let ay = textureSample(cliff_albedo, cliff_sampler, p.xz);
    let az = textureSample(cliff_albedo, cliff_sampler, p.xy);
    let albedo = ax * w.x + ay * w.y + az * w.z;
    if (cliff.params.z > 0.5) {
        // The tangent-space normals of the three projections, swizzled onto their axes.
        let nx = textureSample(cliff_normal, cliff_sampler, p.zy).xyz * 2.0 - 1.0;
        let ny = textureSample(cliff_normal, cliff_sampler, p.xz).xyz * 2.0 - 1.0;
        let nz = textureSample(cliff_normal, cliff_sampler, p.xy).xyz * 2.0 - 1.0;
        let tx = vec3f(0.0, nx.y, nx.x) * sign(n.x);
        let ty = vec3f(ny.x, 0.0, ny.y) * sign(n.y);
        let tz = vec3f(nz.x, nz.y, 0.0) * sign(n.z);
        n = normalize(n + tx * w.x + ty * w.y + tz * w.z);
    }
    var surface: Surface;
    surface.position = in.world;
    surface.normal = n;
    // Grooves between stones are dark in both the albedo and the bounce they get.
    surface.albedo = frosted(albedo.rgb, n, in.world, frost_amount() * cliff.params.w) * mix(0.35, 1.0, in.occlusion);
    surface.roughness = cliff.params.y;
    surface.metalness = 0.0;
    surface.occlusion = in.occlusion;
    surface.backside = 0.0;
    surface.specular = 1.0;
    surface.model = LIGHT_STANDARD;
    let color = surface_light(surface, normalize(frame.eye.xyz - in.world), in.clip.xy);
    return scene_out(vec4f(color, 1.0), in.world);
}

// --- Decals ---------------------------------------------------------------------------

@group(1) @binding(0) var<uniform> decal_info: DecalInfo;
@group(1) @binding(1) var<storage, read> decals: array<Decal>;
@group(1) @binding(2) var decal_atlas: texture_2d<f32>;
@group(1) @binding(3) var decal_sampler: sampler;

struct DecalOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) uv: vec2f,
    @location(2) @interpolate(flat) index: u32,
}

@vertex
fn vs_decal(@builtin(vertex_index) vertex: u32, @builtin(instance_index) index: u32) -> DecalOut {
    let corners = array<vec2f, 6>(
        vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
        vec2f(-1.0, -1.0), vec2f(1.0, 1.0), vec2f(-1.0, 1.0),
    );
    let d = decals[index];
    let c = corners[vertex];
    let s = sin(d.yaw);
    let k = cos(d.yaw);
    let local = vec2f(c.x * k - c.y * s, c.x * s + c.y * k) * d.size * 0.5;
    var out: DecalOut;
    out.world = d.position + vec3f(local.x, 0.03, local.y);
    out.clip = frame.view_proj * vec4f(out.world, 1.0);
    let cells = decal_info.params.x;
    let cell = vec2f(d.cell % cells, floor(d.cell / cells));
    out.uv = (cell + c * vec2f(0.5, -0.5) + 0.5) * decal_info.params.y;
    out.index = index;
    return out;
}

@fragment
fn fs_decal(in: DecalOut) -> SceneOut {
    let d = decals[in.index];
    let texel = textureSample(decal_atlas, decal_sampler, in.uv);
    if (texel.a < 0.02) {
        discard;
    }
    // The atlas is premultiplied; under ice the cut-out sinks towards the ice's colour.
    var albedo = texel.rgb / max(texel.a, 0.001) * d.tint.rgb;
    let grey = dot(albedo, vec3f(0.2126, 0.7152, 0.0722));
    albedo = mix(albedo, vec3f(grey) * vec3f(0.75, 0.9, 1.0), d.under * 0.4);
    let up = vec3f(0.0, 1.0, 0.0);
    var surface: Surface;
    surface.position = in.world;
    surface.normal = up;
    surface.albedo = albedo;
    surface.roughness = mix(0.8, 0.2, d.under);
    surface.metalness = 0.0;
    surface.occlusion = 1.0;
    surface.backside = 0.0;
    surface.specular = 1.0;
    surface.model = LIGHT_STANDARD;
    let color = surface_light(surface, normalize(frame.eye.xyz - in.world), in.clip.xy);
    let alpha = texel.a * (1.0 - d.under * 0.45);
    return scene_out(vec4f(color * alpha, alpha), in.world);
}
