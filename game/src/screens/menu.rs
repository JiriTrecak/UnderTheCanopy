//! The main menu's drifting pollen motes and slow light shafts over the forest art.
//! Positions use a virtual 3840 × 2160 canvas mapped through the background's cover fit.

use super::ui_host::UiHost;

/// The art's size, in which effect positions are given.
const ART: [f32; 2] = [3840.0, 2160.0];
/// Light shafts: art x at the top (the god rays through the canopy) and their sway phase.
const RAYS: [(f32, f32); 5] = [
    (1536.0, 0.0),
    (1848.0, 1.3),
    (2160.0, 2.1),
    (2424.0, 3.4),
    (2640.0, 4.2),
];
/// Art pixels per pixel of the old 1672-wide art, for drifts given in its units.
const UNIT: f32 = ART[0] / 1672.0;
const MOTES: usize = 24;

/// Deterministic hash noise in 0–1.
fn noise(i: u32, salt: u32) -> f32 {
    let mut x = i.wrapping_mul(0x9e37_79b9) ^ salt.wrapping_mul(0x85eb_ca6b);
    x ^= x >> 15;
    x = x.wrapping_mul(0x2c1b_3c6d);
    x ^= x >> 12;
    (x & 0xffff) as f32 / 65535.0
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
    if let Some(list) = ui.find("main-menu/effects/rays") {
        for (i, (x, phase)) in RAYS.iter().enumerate() {
            if let Some(node) = ui.child(list, i) {
                let sway = (t * 0.17 + phase).sin() * 24.0 * UNIT;
                let [sx, sy] = map([*x + sway, -120.0 * UNIT]);
                ui.set_shift(node, sx - 60.0, sy);
                ui.set_opacity(node, 0.35 + 0.3 * (t * 0.23 + phase * 1.7).sin().abs());
            }
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
            let x = base[0] + ((t * 0.11 + i as f32).sin() * 30.0 + t * speed * 0.4) * UNIT;
            let y = base[1] + ((t * 0.07 + i as f32 * 2.0).cos() * 18.0 - t * speed * 0.2) * UNIT;
            let wrapped = [x.rem_euclid(ART[0]), y.rem_euclid(ART[1])];
            let [sx, sy] = map(wrapped);
            ui.set_shift(node, sx, sy);
            let twinkle = 0.5 + 0.5 * (t * (0.6 + noise(i as u32, 7)) + i as f32).sin();
            ui.set_opacity(node, 0.12 + 0.35 * twinkle * scale.min(1.5));
        }
    }
}
