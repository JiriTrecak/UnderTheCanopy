// Effect graph particles (D113, crates/fx): the part records the graph kernels write each
// step (world position and size, colour, world axis and rotation, frame and visibility)
// drawn as billboards facing the camera, stretched along an axis (as a strip, or a
// spheroid that keeps its ends round), or flat across an axis. Shades like effects.wgsl:
// textures times colour, soft where they meet the scene, premultiplied with their
// additivity, under the fog of war. Needs Frame.

struct GraphDraw {
    // First part record, this renderer's index, renderers per particle, mode.
    ints: vec4u,
    // Flipbook columns and rows, textured (0/1), 0.
    flip: vec4u,
    // Additivity (1 adds light), soft fade (m), 0, 0.
    look: vec4f,
}

struct EffectView {
    right: vec4f,
    up: vec4f,
}

@group(1) @binding(0) var effect_texture: texture_2d<f32>;
@group(1) @binding(1) var effect_sampler: sampler;
@group(2) @binding(0) var<storage, read> parts: array<vec4f>;
@group(2) @binding(1) var<uniform> draw: GraphDraw;
@group(2) @binding(2) var<uniform> view: EffectView;
@group(3) @binding(0) var scene_depth: texture_2d<f32>;
@group(3) @binding(1) var scene_copy: texture_2d<f32>;

const MODE_CAMERA: u32 = 0u;
const MODE_AXIS: u32 = 1u;
const MODE_SPHEROID: u32 = 2u;
const MODE_PLANE: u32 = 3u;
const MODE_RIBBON: u32 = 4u;

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) uv: vec2f,
    @location(2) color: vec4f,
}

var<private> CORNERS: array<vec2f, 6> = array<vec2f, 6>(
    vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0),
    vec2f(-1.0, -1.0), vec2f(1.0, 1.0), vec2f(-1.0, 1.0),
);

fn rotate2(v: vec2f, angle: f32) -> vec2f {
    let c = cos(angle);
    let s = sin(angle);
    return vec2f(v.x * c - v.y * s, v.x * s + v.y * c);
}

@vertex
fn vs_graph(@builtin(vertex_index) vertex: u32, @builtin(instance_index) instance: u32) -> VsOut {
    var out: VsOut;
    let r = draw.ints.x + (instance * draw.ints.z + draw.ints.y) * 4u;
    let a = parts[r];
    let colour = parts[r + 1u];
    let b = parts[r + 2u];
    let c = parts[r + 3u];
    if (c.y < 0.5) {
        out.clip = vec4f(2.0, 2.0, 2.0, 1.0);
        return out;
    }
    let corner = CORNERS[vertex];
    if (draw.ints.w == MODE_RIBBON) {
        // A strip segment from this point to the next of its chain.
        let rn = draw.ints.x + (bitcast<u32>(b.x) * draw.ints.z + draw.ints.y) * 4u;
        let a2 = parts[rn];
        let colour2 = parts[rn + 1u];
        let b2 = parts[rn + 2u];
        let c2 = parts[rn + 3u];
        if (c2.y < 0.5 || bitcast<u32>(c2.z) != bitcast<u32>(c.z) + 1u) {
            out.clip = vec4f(2.0, 2.0, 2.0, 1.0);
            return out;
        }
        let t = corner.y * 0.5 + 0.5;
        let along = a2.xyz - a.xyz;
        let length_ = length(along);
        let dir = select(view.up.xyz, along / max(length_, 1e-6), length_ > 1e-6);
        let point = mix(a.xyz, a2.xyz, t);
        let to_eye = normalize(frame.eye.xyz - point);
        var across = cross(dir, to_eye);
        let across_length = length(across);
        across = select(view.right.xyz, across / max(across_length, 1e-6), across_length > 1e-5);
        let world_r = point + across * corner.x * mix(a.w, a2.w, t) * 0.5;
        out.clip = frame.view_proj * vec4f(world_r, 1.0);
        out.world = world_r;
        out.uv = vec2f(mix(b.w, b2.w, t), 0.5 - corner.x * 0.5);
        out.color = mix(colour, colour2, t);
        return out;
    }
    let center = a.xyz;
    let half = a.w * 0.5;
    let axis = b.xyz;
    let rotation = radians(b.w);
    var world = center;
    switch draw.ints.w {
        case MODE_AXIS, MODE_SPHEROID: {
            // Along the axis (its full length), `size` wide across it and the view.
            let length_ = length(axis);
            let along = select(view.up.xyz, axis / length_, length_ > 1e-6);
            let to_eye = normalize(frame.eye.xyz - center);
            var across = cross(along, to_eye);
            let across_length = length(across);
            across = select(view.right.xyz, across / across_length, across_length > 1e-5);
            var reach = length_ * 0.5;
            if (draw.ints.w == MODE_SPHEROID) {
                reach += half;
            }
            world = center + along * corner.y * reach + across * corner.x * half;
        }
        case MODE_PLANE: {
            // Flat across the axis (its normal), turned by the rotation.
            let normal = select(vec3f(0.0, 1.0, 0.0), normalize(axis), length(axis) > 1e-6);
            let helper = select(vec3f(1.0, 0.0, 0.0), vec3f(0.0, 0.0, 1.0), abs(normal.x) > 0.9);
            let t = normalize(cross(normal, helper));
            let bt = cross(normal, t);
            let q = rotate2(corner, rotation) * half;
            world = center + t * q.x + bt * q.y;
        }
        default: {
            let q = rotate2(corner, rotation) * half;
            world = center + view.right.xyz * q.x + view.up.xyz * q.y;
        }
    }
    var uv = vec2f(corner.x * 0.5 + 0.5, 0.5 - corner.y * 0.5);
    let columns = max(draw.flip.x, 1u);
    let rows = max(draw.flip.y, 1u);
    if (columns * rows > 1u) {
        let frame_index = u32(max(c.x, 0.0)) % (columns * rows);
        let cell = vec2f(f32(frame_index % columns), f32(frame_index / columns));
        uv = (uv + cell) / vec2f(f32(columns), f32(rows));
    }
    out.clip = frame.view_proj * vec4f(world, 1.0);
    out.world = world;
    out.uv = uv;
    out.color = colour;
    return out;
}

// How much of a fragment at `world` shows in front of the scene (effects.wgsl).
fn depth_fade(clip: vec4f, world: vec3f, soft: f32) -> f32 {
    let stored = textureLoad(scene_depth, vec2i(clip.xy), 0).rg;
    let gap = stored.r + stored.g - view_depth(world);
    if (soft <= 0.0) {
        return select(0.0, 1.0, gap >= -0.02);
    }
    return clamp(gap / soft, 0.0, 1.0);
}

@fragment
fn fs_graph(in: VsOut) -> @location(0) vec4f {
    var texel = vec4f(1.0);
    if (draw.flip.z != 0u) {
        texel = textureSample(effect_texture, effect_sampler, in.uv);
    } else {
        let r = length(in.uv * 2.0 - 1.0);
        texel = vec4f(vec3f(1.0 - smoothstep(0.0, 1.0, r)), 1.0 - smoothstep(0.7, 1.0, r));
    }
    if (draw.look.z > 0.5) {
        // Distortion: the scene behind, pushed by the texture's red and green (0.5 none)
        // as far as the colour's alpha says.
        let push = (texel.rg * 2.0 - 1.0) * in.color.a;
        let size = vec2f(textureDimensions(scene_copy));
        let at = clamp(in.clip.xy + push * size.y * 0.03, vec2f(0.0), size - 1.0);
        let fade = depth_fade(in.clip, in.world, draw.look.y);
        if (fade <= 0.0) {
            discard;
        }
        return vec4f(textureLoad(scene_copy, vec2i(at), 0).rgb * fade, fade);
    }
    let additive = draw.look.x;
    // Additive parts add their colour times the texture's colour, weighted by its alpha
    // (as PopcornFX's Additive: a coloured flipbook's background is bled colour at alpha
    // 0); covering parts use the alpha as coverage.
    var color = in.color.rgb * texel.rgb;
    var alpha = in.color.a * texel.a;
    alpha *= depth_fade(in.clip, in.world, draw.look.y);
    if (alpha <= 0.0) {
        discard;
    }
    let lit = apply_fog(in.world, color * frame_shade(in.world));
    let a = max(alpha, 0.0);
    return vec4f(lit * a, min(a, 1.0) * (1.0 - clamp(additive, 0.0, 1.0)));
}
