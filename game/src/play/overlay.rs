//! A screen-space stand-in for the world UI: selection and hover rings, order markers,
//! the placement footprint, the rally point and the drag box, drawn as UI quads under
//! the HUD from [`WorldUi`].
//!
//! The renderer's world UI pass (terrain-draped rings, health bars, placement grid;
//! renderer §4.9) consumes the same [`WorldUi`]; once it draws these, everything but
//! the drag box here goes away. The rings are screen ellipses at the feet, close to the
//! old terrain rings at the 56° play pitch.

use super::{
    WorldUi,
    know::Knowledge,
    orders::Marker,
    picking::{Body, Screen},
};
use canopy_content::placement::Owner;
use canopy_render::camera::Ground;
use canopy_sim::teams::Relation;
use canopy_ui::{Batch, Color, DrawList, Quad, QuadKind, Rect};
use glam::Vec3;

/// Ring thickness, logical pixels.
const RING: f32 = 2.0;

fn rgba(rgb: [u8; 3], alpha: f32) -> Color {
    Color::rgba(
        rgb[0],
        rgb[1],
        rgb[2],
        (alpha.clamp(0.0, 1.0) * 255.0) as u8,
    )
}

/// An ellipse outline around a ground circle of `radius` at `center`.
fn ring(
    quads: &mut Vec<Quad>,
    screen: &Screen,
    center: Vec3,
    radius: f32,
    color: Color,
    width: f32,
) {
    let Some((c, _)) = screen.project(center) else {
        return;
    };
    let (Some((rx, _)), Some((rz, _))) = (
        screen.project(center + Vec3::X * radius),
        screen.project(center + Vec3::Z * radius),
    ) else {
        return;
    };
    let w = (rx.x - c.x).abs().max(4.0);
    let h = (rz.y - c.y).abs().max(3.0);
    let mut q = Quad::new(
        QuadKind::Shape,
        Rect::new(c.x - w, c.y - h, 2.0 * w, 2.0 * h),
        Color::TRANSPARENT,
    );
    q.color2 = color.to_array();
    q.border = width;
    q.radius = h;
    quads.push(q);
}

fn rect(quads: &mut Vec<Quad>, r: Rect, fill: Color, border: Option<(Color, f32)>, radius: f32) {
    let mut q = Quad::new(QuadKind::Shape, r, fill);
    if let Some((c, w)) = border {
        q.color2 = c.to_array();
        q.border = w;
    }
    q.radius = radius;
    quads.push(q);
}

/// Ring colour by relation (old selection circle colours).
fn relation_color(k: &Knowledge, id: u32) -> [u8; 3] {
    match k.entity(id) {
        Some(e) if k.owner().is_some_and(|o| e.public.owner == o) => [0x7c, 0xe0, 0x5a],
        Some(e) if e.public.owner == Owner::Nobody => [0xe8, 0xd0, 0x5a],
        Some(e) if e.relation == Relation::Ally => [0x6c, 0xb8, 0xf0],
        Some(_) => [0xf0, 0x52, 0x4c],
        None => [0xff, 0xff, 0xff],
    }
}

/// Appends the overlay to `list` (before the HUD's quads, so the HUD stays on top).
pub fn draw(
    list: &mut DrawList,
    screen: &Screen,
    ui: &WorldUi,
    k: &Knowledge,
    bodies: &[Body],
    ground: &dyn Ground,
) {
    let mut quads = Vec::new();
    let s = screen.scale;
    let at = |x: f32, z: f32| Vec3::new(x, ground.height(x, z).max(0.0) + 0.05, z);
    let body_of = |id: u32| bodies.iter().find(|b| b.id == id);
    let circle = |id: u32| -> Option<(Vec3, f32)> {
        if let Some(b) = body_of(id) {
            return Some((b.foot + Vec3::Y * 0.05, b.radius + 0.32));
        }
        let e = k.entity(id)?;
        let [x, z] = k.position(e);
        let d = k.def(e);
        let r = d
            .footprint
            .as_ref()
            .map_or(0.9, |f| f.width.max(f.depth) as f32 / 2.0 + 0.35);
        Some((at(x, z), r))
    };
    for &id in &ui.selected {
        if let Some((c, r)) = circle(id) {
            ring(
                &mut quads,
                screen,
                c,
                r,
                rgba(relation_color(k, id), 0.95),
                RING * s,
            );
        }
    }
    if let Some(id) = ui.hovered.filter(|id| !ui.selected.contains(id))
        && let Some((c, r)) = circle(id)
    {
        ring(
            &mut quads,
            screen,
            c,
            r,
            rgba(relation_color(k, id), 0.45),
            RING * s * 0.75,
        );
    }
    for (m, age) in &ui.markers {
        marker(&mut quads, screen, m, *age, &at, s);
    }
    if let Some(p) = ui.rally {
        ring(
            &mut quads,
            screen,
            at(p[0], p[1]),
            0.5,
            rgba([0xff, 0xe0, 0x8a], 0.9),
            RING * s,
        );
    }
    if let Some(g) = &ui.ghost {
        let color = if g.problem.is_some() {
            [0xf2, 0x69, 0x60]
        } else {
            [0xff, 0xff, 0xff]
        };
        for (cx, cy) in g.cells.cells() {
            let (x, z) = (cx as f32, cy as f32);
            let Some((c, _)) = screen.project(at(x, z)) else {
                continue;
            };
            let Some((e, _)) = screen.project(at(x + 0.5, z)) else {
                continue;
            };
            let half = ((e.x - c.x).abs() * 0.85).max(1.5);
            rect(
                &mut quads,
                Rect::new(c.x - half, c.y - half * 0.85, 2.0 * half, 1.7 * half),
                rgba(color, 0.16),
                Some((rgba(color, 0.55), 1.0 * s)),
                1.0,
            );
        }
        if let Some((d, _)) = screen.project(at(g.entrance[0], g.entrance[1])) {
            let r = 5.0 * s;
            rect(
                &mut quads,
                Rect::new(d.x - r, d.y - r, 2.0 * r, 2.0 * r),
                rgba([0xff, 0xed, 0x9d], 0.9),
                None,
                2.0,
            );
        }
    }
    if let Some([a, b]) = ui.drag {
        let (min, max) = (a.min(b), a.max(b));
        rect(
            &mut quads,
            Rect::new(min.x, min.y, max.x - min.x, max.y - min.y),
            Color::rgba(0xb9, 0xdf, 0x80, 0x30),
            Some((Color::rgba(0xd8, 0xef, 0xb0, 0xff), 1.0 * s)),
            0.0,
        );
    }
    if quads.is_empty() {
        return;
    }
    // Insert before the HUD: shift the HUD's batches.
    let n = quads.len() as u32;
    for batch in &mut list.batches {
        batch.quads = batch.quads.start + n..batch.quads.end + n;
    }
    list.batches.insert(
        0,
        Batch {
            texture: None,
            scissor: None,
            quads: 0..n,
        },
    );
    quads.append(&mut list.quads);
    list.quads = quads;
}

fn marker(
    quads: &mut Vec<Quad>,
    screen: &Screen,
    m: &Marker,
    age: f32,
    at: &dyn Fn(f32, f32) -> Vec3,
    scale: f32,
) {
    let opacity = m.opacity(age);
    if opacity <= 0.0 {
        return;
    }
    ring(
        quads,
        screen,
        at(m.position[0], m.position[1]),
        m.radius,
        rgba(m.kind.color(), opacity),
        3.0 * scale,
    );
}
