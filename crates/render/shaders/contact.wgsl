// Contact shading (old `BeautyPass` contact): screen-space ambient shade from the
// resolved depth at the post-effect resolution. Twelve golden-angle taps within a fixed
// world radius test how far nearby surfaces rise above the pixel's tangent plane.
// Water writes its surface into the depth, so wading objects shade the water around
// them and nothing below it is sampled.

struct Contact {
    // world radius, bias, 0, 0
    params: vec4f,
}

@group(1) @binding(2) var<uniform> contact: Contact;

@fragment
fn fs_main(in: FullscreenOut) -> @location(0) vec4f {
    let depth = depth_at_uv(in.uv);
    let p = world_at(in.uv, depth);
    var normal = cross(dpdx(p), dpdy(p));
    let normal_length = length(normal);
    if (depth > BACKGROUND_DEPTH || normal_length < 0.000001) {
        return vec4f(1.0);
    }
    normal /= normal_length;
    if (dot(normal, view.eye.xyz - p) < 0.0) {
        normal = -normal;
    }
    let radius = contact.params.x;
    let bias = contact.params.y;
    // A fixed world radius in pixels, stable across zoom.
    let pixels = clamp(radius * view.projection.x * 0.5 * view.scene.y / max(0.1, depth), 1.0, 80.0);
    var amount = 0.0;
    for (var i = 0; i < 12; i++) {
        let angle = f32(i) * 2.39996323;
        let delta = vec2f(cos(angle), sin(angle)) * sqrt((f32(i) + 0.5) / 12.0) * pixels * view.scene.zw;
        let uv = in.uv + delta;
        if (any(uv < vec2f(0.0)) || any(uv > vec2f(1.0))) {
            continue;
        }
        let d = depth_at_uv(uv);
        if (d > BACKGROUND_DEPTH) {
            continue;
        }
        let v = world_at(uv, d) - p;
        let distance = length(v);
        let horizon = max(0.0, dot(normal, v) - bias) / max(distance, 0.001);
        amount += horizon * (1.0 - smoothstep(radius * 0.25, radius, distance));
    }
    return vec4f(vec3f(clamp(1.0 - amount / 12.0 * 3.0, 0.15, 1.0)), 1.0);
}
