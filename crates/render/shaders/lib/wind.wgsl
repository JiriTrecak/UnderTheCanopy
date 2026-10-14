// Foliage motion shared by every pass that draws foliage (so shadows sway too).

// Trunk sway (the old "source" plant sway): a length-preserving bend about the
// instance origin. `local_y` is the model-space height, `height` the model's top,
// `scale_y` the instance's vertical scale.
fn trunk_sway(world: vec3f, origin: vec3f, local_y: f32, height: f32, scale_y: f32, t: f32) -> vec3f {
    let bend_scale = clamp((height * scale_y - 4.0) / 10.0, 0.0, 1.0);
    if (bend_scale <= 0.0) {
        return world;
    }
    let phase = t + origin.x * 0.16 + origin.z * 0.24;
    let harmonic = cos(phase * 0.4) * cos(phase * 0.4 * 1.3);
    let tt = phase * 0.5;
    let noise = harmonic * cos(tt * 1.7) + 0.25 * sin(tt * 4.7);
    let bend = pow(clamp(local_y / height, 0.0, 1.0), 2.0);
    let offset = vec3f(noise, 0.0, harmonic) * (0.5 * bend_scale) * bend;
    let arm = world - origin;
    let moved = arm + offset;
    return origin + moved * (length(arm) / max(length(moved), 1e-5));
}

// Leaf flutter about each leaf's pivot (`_LEAF`: rgb = offset from the pivot,
// (rgb·2 − 1)·4 metres; a > 0 on leaf vertices). Length-preserving, in model space.
fn leaf_flutter(local: vec3f, leaf: vec4f, anchor: vec3f, amplitude: f32, t: f32) -> vec3f {
    if (leaf.a <= 0.001) {
        return local;
    }
    let offset = (leaf.rgb * 2.0 - 1.0) * 4.0;
    let leaf_length = length(offset);
    let mask = clamp(leaf_length / 1.5, 0.0, 1.0);
    let pivot = local - offset;
    let phase = t * 2.1 + dot(pivot + anchor, vec3f(0.73, 0.41, 0.57));
    let travel = vec3f(sin(phase), sin(phase * 0.63) * 0.2, cos(phase * 0.81)) * amplitude * 0.45 * mask;
    return local + normalize(offset + travel + vec3f(1e-6)) * leaf_length - offset;
}
