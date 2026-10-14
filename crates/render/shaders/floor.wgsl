// The ground beyond the map's halo (biome surroundings): the floor texture tinted by
// the floor colour, lit like painted ground, hazed with distance.

struct Look {
    shallow: vec4f,
    deep: vec4f,
    // rgb floor colour
    floor_color: vec4f,
}

@group(1) @binding(1) var<uniform> look: Look;
@group(1) @binding(2) var floor_texture: texture_2d<f32>;
@group(1) @binding(3) var floor_sampler: sampler;

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

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let frag = in.clip;
    let albedo = textureSample(floor_texture, floor_sampler, in.world.xz * 0.06).rgb * look.floor_color.rgb;
    var surface: Surface;
    surface.position = in.world;
    surface.normal = vec3f(0.0, 1.0, 0.0);
    surface.albedo = albedo;
    surface.roughness = 1.0;
    surface.metalness = 0.0;
    surface.occlusion = 1.0;
    surface.backside = 0.0;
    surface.specular = 1.0;
    surface.model = LIGHT_STANDARD;
    let view = frame.eye.xyz - in.world;
    let color = surface_light(surface, normalize(view), frag.xy);
    return vec4f(haze(color, length(view)), 1.0);
}
