// Decoding of the pack's quantised vertex data (docs/decisions.md D26).

// Octahedral unit vector from snorm16 ×2 (already in -1..1).
fn oct_decode(e: vec2f) -> vec3f {
    var n = vec3f(e.x, e.y, 1.0 - abs(e.x) - abs(e.y));
    let t = max(-n.z, 0.0);
    n.x += select(t, -t, n.x >= 0.0);
    n.y += select(t, -t, n.y >= 0.0);
    return normalize(n);
}

// Model-space position from unorm16 inside the model's quantisation box.
fn dequantize(q: vec3f, box_min: vec3f, box_extent: vec3f) -> vec3f {
    return box_min + q * box_extent;
}

// A 3×4 row-major affine transform stored as three vec4 rows.
struct Affine {
    r0: vec4f,
    r1: vec4f,
    r2: vec4f,
}

fn affine_point(m: Affine, p: vec3f) -> vec3f {
    let h = vec4f(p, 1.0);
    return vec3f(dot(m.r0, h), dot(m.r1, h), dot(m.r2, h));
}

fn affine_vector(m: Affine, v: vec3f) -> vec3f {
    return vec3f(dot(m.r0.xyz, v), dot(m.r1.xyz, v), dot(m.r2.xyz, v));
}

fn affine_translation(m: Affine) -> vec3f {
    return vec3f(m.r0.w, m.r1.w, m.r2.w);
}
