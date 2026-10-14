// Resolve → tone map → grade (renderer §4.1, step 9). The one tone mapper is the old
// game's hue-preserving highlight shoulder in display space; at knee 0 it is bypassed
// and values clip at 1, exactly as today's outdoor frames. The biome grade (and the
// optional daytime lookup) is one baked volume; the vignette depends on the position.

struct Post {
    // x: vignette, y: shoulder knee (0 = clip), z: 1 when the output view encodes sRGB
    params: vec4f,
}

@group(0) @binding(0) var scene: texture_2d<f32>;
@group(0) @binding(1) var grade: texture_3d<f32>;
@group(0) @binding(2) var grade_sampler: sampler;
@group(0) @binding(3) var<uniform> post: Post;

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32) -> VsOut {
    let uv = vec2f(f32((vid << 1u) & 2u), f32(vid & 2u));
    var out: VsOut;
    out.clip = vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.uv = uv;
    return out;
}

fn shoulder(c: vec3f, knee: f32) -> vec3f {
    let peak = max(c.r, max(c.g, c.b));
    if (knee <= 0.0 || peak <= knee) {
        return c;
    }
    let room = max(0.01, 1.0 - knee);
    let compressed = knee + room * (1.0 - exp(-(peak - knee) / room));
    return c * compressed / peak;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4f {
    let hdr = textureLoad(scene, vec2i(in.clip.xy), 0).rgb;
    var display = srgb_encode(max(hdr, vec3f(0.0)));
    display = clamp(shoulder(display, post.params.y), vec3f(0.0), vec3f(1.0));
    let size = f32(textureDimensions(grade).x);
    let lut_uv = display * ((size - 1.0) / size) + 0.5 / size;
    display = textureSampleLevel(grade, grade_sampler, lut_uv, 0.0).rgb;
    display *= 1.0 - smoothstep(0.15, 0.72, length(in.uv - 0.5)) * post.params.x;
    if (post.params.z > 0.5) {
        return vec4f(srgb_decode(display), 1.0);
    }
    return vec4f(display, 1.0);
}
