// Joint palettes of the frame's posed actors (renderer §4.5): every visible instance gets
// one model-space skinning matrix per joint, blended from up to three poses (the current
// clip and two fading ones), each sampled between two baked 30 Hz frames.

struct Pose {
    // first matrix of frame a and of frame b (joint 0) in `clips`, frame mix, weight
    frames: vec2u,
    mix_weight: vec2f,
}

struct Job {
    // palette base, joints, poses, aimed (1)
    header: vec4u,
    poses: array<Pose, 3>,
    // joints the aim turns, a bit each
    mask: array<vec4u, 2>,
    // cos, sin of the aim, pivot x, z
    aim: vec4f,
}

@group(0) @binding(0) var<storage, read> jobs: array<Job>;
// Baked clip matrices: three rows (3×4, row-major) per matrix.
@group(0) @binding(1) var<storage, read> clips: array<vec4f>;
@group(0) @binding(2) var<storage, read_write> palette: array<vec4f>;

@compute @workgroup_size(64)
fn skin(@builtin(global_invocation_id) id: vec3u) {
    let job = jobs[id.y];
    let joint = id.x;
    if (joint >= job.header.y) {
        return;
    }
    var r0 = vec4f(0.0);
    var r1 = vec4f(0.0);
    var r2 = vec4f(0.0);
    let count = job.header.z;
    if (count == 0u) {
        r0 = vec4f(1.0, 0.0, 0.0, 0.0);
        r1 = vec4f(0.0, 1.0, 0.0, 0.0);
        r2 = vec4f(0.0, 0.0, 1.0, 0.0);
    }
    for (var p = 0u; p < count; p++) {
        let pose = job.poses[p];
        let a = (pose.frames.x + joint) * 3u;
        let b = (pose.frames.y + joint) * 3u;
        let t = pose.mix_weight.x;
        let w = pose.mix_weight.y;
        r0 += mix(clips[a], clips[b], t) * w;
        r1 += mix(clips[a + 1u], clips[b + 1u], t) * w;
        r2 += mix(clips[a + 2u], clips[b + 2u], t) * w;
    }
    let word = job.mask[joint / 128u][(joint % 128u) / 32u];
    if (job.header.w == 1u && (word & (1u << (joint % 32u))) != 0u) {
        // A turret aims: turn about the vertical axis through its pivot.
        let c = job.aim.x;
        let s = job.aim.y;
        let p = job.aim.zw;
        let a0 = r0;
        let a2 = r2;
        r0 = c * a0 + s * a2;
        r2 = -s * a0 + c * a2;
        r0.w += p.x - (c * p.x + s * p.y);
        r2.w += p.y - (-s * p.x + c * p.y);
    }
    let out = (job.header.x + joint) * 3u;
    palette[out] = r0;
    palette[out + 1u] = r1;
    palette[out + 2u] = r2;
}
