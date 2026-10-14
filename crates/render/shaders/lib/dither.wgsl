// Screen-door helpers: interleaved gradient noise in 0–1 per pixel.

fn interleaved_gradient(frag: vec2f) -> f32 {
    return fract(52.9829189 * fract(dot(floor(frag), vec2f(0.06711056, 0.00583715))));
}

// Discards a fraction `amount` of pixels in a stable stipple pattern.
fn dither_discard(amount: f32, frag: vec2f) -> bool {
    return interleaved_gradient(frag) < amount;
}
