//! The drag box of a box selection, drawn as a UI quad under the HUD from [`WorldUi`].
//! Everything else the player sees in the world (selection and hover rings, order
//! markers, the placement grid, rally points, health bars) is the renderer's world UI
//! pass (`canopy_scene::world_ui`).

use super::WorldUi;
use canopy_ui::{Batch, Color, DrawList, Quad, QuadKind, Rect};

/// Appends the drag box to `list` (before the HUD's quads, so the HUD stays on top).
pub fn draw(list: &mut DrawList, scale: f32, ui: &WorldUi) {
    let Some([a, b]) = ui.drag else {
        return;
    };
    let (min, max) = (a.min(b), a.max(b));
    let mut q = Quad::new(
        QuadKind::Shape,
        Rect::new(min.x, min.y, max.x - min.x, max.y - min.y),
        Color::rgba(0xb9, 0xdf, 0x80, 0x30),
    );
    q.color2 = Color::rgba(0xd8, 0xef, 0xb0, 0xff).to_array();
    q.border = scale;
    let mut quads = vec![q];
    // Insert before the HUD: shift the HUD's batches.
    for batch in &mut list.batches {
        batch.quads = batch.quads.start + 1..batch.quads.end + 1;
    }
    list.batches.insert(
        0,
        Batch {
            texture: None,
            scissor: None,
            quads: 0..1,
        },
    );
    quads.append(&mut list.quads);
    list.quads = quads;
}
