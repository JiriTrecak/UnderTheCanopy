//! The main menu's living background (old `menuEffects.ts`): glowing fires with rising
//! embers, drifting dust motes and slow light shafts over the forest art, laid out in the
//! art's own 1672 × 941 space and mapped through its `cover` fit. Boxes of the UI stand in
//! for the old canvas drawing: radial glows flicker, small dots drift and fade. The old
//! bezier flame tongues and smoke sprites are not drawn.

use super::ui_host::UiHost;

/// The art's size, in which effect positions are given.
const ART: [f32; 2] = [1672.0, 941.0];
/// Fire centres in art pixels (old `FIRES`).
const FIRES: [[f32; 2]; 3] = [[1453.0, 474.0], [1340.0, 599.0], [1116.0, 479.0]];
/// Light shafts: art x at the top and their sway phase.
const RAYS: [(f32, f32); 5] = [
    (560.0, 0.0),
    (700.0, 1.3),
    (840.0, 2.1),
    (980.0, 3.4),
    (1120.0, 4.2),
];
const MOTES: usize = 24;
const EMBERS: usize = 24;

/// Deterministic hash noise in 0–1.
fn noise(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65535.0
}

/// Smooth flicker in 0–1 from a few sines.
fn flicker(t: f32, seed: f32) -> f32 {
    let v = (t * 7.3 + seed).sin() * 0.5
        + (t * 13.1 + seed * 2.0).sin() * 0.3
        + (t * 2.1 + seed).sin() * 0.2;
    0.5 + 0.5 * v
}

/// Maps art coordinates to logical screen coordinates under `object-fit: cover`.
fn cover(view: [f32; 2]) -> impl Fn([f32; 2]) -> [f32; 2] {
    let scale = (view[0] / ART[0]).max(view[1] / ART[1]);
    let offset = [
        (view[0] - ART[0] * scale) / 2.0,
        (view[1] - ART[1] * scale) / 2.0,
    ];
    move |[x, y]| [offset[0] + x * scale, offset[1] + y * scale]
}

/// Animates the effects for time `t` (seconds) in a view of `view` logical pixels.
pub fn animate(host: &mut UiHost, t: f32, view: [f32; 2]) {
    let map = cover(view);
    let scale = (view[0] / ART[0]).max(view[1] / ART[1]);
    let ui = &mut host.ui;
    if let Some(list) = ui.find("main-menu/effects/glows") {
        for (i, fire) in FIRES.iter().enumerate() {
            if let Some(node) = ui.child(list, i) {
                let [x, y] = map(*fire);
                ui.set_shift(node, x - 140.0, y - 150.0);
                ui.set_opacity(node, 0.6 + 0.4 * flicker(t, i as f32 * 1.7));
            }
        }
    }
    if let Some(list) = ui.find("main-menu/effects/cores") {
        for (i, fire) in FIRES.iter().enumerate() {
            if let Some(node) = ui.child(list, i) {
                let [x, y] = map(*fire);
                let f = flicker(t * 1.6, i as f32 * 3.1);
                ui.set_shift(node, x - 35.0, y - 60.0 - f * 4.0);
                ui.set_opacity(node, 0.5 + 0.5 * f);
            }
        }
    }
    if let Some(list) = ui.find("main-menu/effects/rays") {
        for (i, (x, phase)) in RAYS.iter().enumerate() {
            if let Some(node) = ui.child(list, i) {
                let sway = (t * 0.17 + phase).sin() * 24.0;
                let [sx, _] = map([*x + sway, 0.0]);
                ui.set_shift(node, sx, 0.0);
                ui.set_opacity(node, 0.35 + 0.3 * (t * 0.23 + phase * 1.7).sin().abs());
            }
        }
    }
    if let Some(list) = ui.find("main-menu/effects/embers") {
        for i in 0..EMBERS {
            let Some(node) = ui.child(list, i) else {
                continue;
            };
            let fire = FIRES[i % FIRES.len()];
            let period = 2.2 + noise(i as u32, 1) * 1.8;
            let age = ((t + noise(i as u32, 2) * period) % period) / period;
            let drift = (noise(i as u32, 3) - 0.5) * 60.0 + (t * 1.3 + i as f32).sin() * 8.0 * age;
            let [x, y] = map([fire[0] + drift * age, fire[1] - 20.0 - age * 160.0]);
            ui.set_shift(node, x, y);
            ui.set_opacity(node, (1.0 - age) * (age * 8.0).min(1.0));
        }
    }
    if let Some(list) = ui.find("main-menu/effects/motes") {
        for i in 0..MOTES {
            let Some(node) = ui.child(list, i) else {
                continue;
            };
            let base = [
                noise(i as u32, 4) * ART[0] * 0.75 + ART[0] * 0.2,
                noise(i as u32, 5) * ART[1] * 0.8,
            ];
            let speed = 6.0 + noise(i as u32, 6) * 10.0;
            let x = base[0] + (t * 0.11 + i as f32).sin() * 30.0 + t * speed * 0.4;
            let y = base[1] + (t * 0.07 + i as f32 * 2.0).cos() * 18.0 - t * speed * 0.2;
            let wrapped = [x.rem_euclid(ART[0]), y.rem_euclid(ART[1])];
            let [sx, sy] = map(wrapped);
            ui.set_shift(node, sx, sy);
            let twinkle = 0.5 + 0.5 * (t * (0.6 + noise(i as u32, 7)) + i as f32).sin();
            ui.set_opacity(node, 0.12 + 0.35 * twinkle * scale.min(1.5));
        }
    }
}
