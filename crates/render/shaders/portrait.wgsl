// The portrait's resolve (portrait.rs): the multisampled HDR model, already resolved and
// premultiplied by its coverage over a transparent background, clipped like the world's
// tone mapping (every current biome clips) into the sRGB portrait image.

@group(0) @binding(0) var resolved: texture_2d<f32>;

@vertex
fn vs_main(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
    let uv = vec2f(f32((i << 1u) & 2u), f32(i & 2u));
    return vec4f(uv * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0), 0.0, 1.0);
}

@fragment
fn fs_main(@builtin(position) frag: vec4f) -> @location(0) vec4f {
    let c = textureLoad(resolved, vec2i(frag.xy), 0);
    let alpha = clamp(c.a, 0.0, 1.0);
    return vec4f(clamp(c.rgb, vec3f(0.0), vec3f(alpha)), alpha);
}
