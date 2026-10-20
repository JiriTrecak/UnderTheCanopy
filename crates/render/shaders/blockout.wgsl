// A map's blockout (blockout.rs): flat-coloured block faces lit by a fixed soft light, with
// a dark outline along every face's edges and diagonal cuts, so each block reads as a block.

struct VsIn {
    @location(0) position: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) edge: vec3f,
}

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) color: vec3f,
    @location(3) edge: vec3f,
}

@vertex
fn vs_main(in: VsIn) -> VsOut {
    var out: VsOut;
    out.world = in.position;
    out.normal = in.normal;
    out.color = in.color;
    out.edge = in.edge;
    out.clip = frame.view_proj * vec4f(in.position, 1.0);
    return out;
}

@fragment
fn fs_main(in: VsOut) -> SceneOut {
    let n = normalize(in.normal);
    // A fixed light from the upper left, so tops are brightest and walls step down by side.
    let light = normalize(vec3f(-0.45, 1.0, -0.3));
    let shade = 0.5 + 0.5 * max(dot(n, light), 0.0) + 0.08 * n.y;
    // Outlines about a pixel and a half wide whatever the distance.
    let w = fwidth(in.edge);
    let border = min(min(in.edge.x, 1.0 - in.edge.x), min(in.edge.y, 1.0 - in.edge.y));
    let border_w = max(w.x, w.y) * 1.5;
    let line = max(
        1.0 - smoothstep(0.0, border_w, border),
        1.0 - smoothstep(0.0, w.z * 1.5, abs(in.edge.z)),
    );
    let color = in.color * shade * mix(1.0, 0.45, line);
    return scene_out(vec4f(color, 1.0), in.world);
}
