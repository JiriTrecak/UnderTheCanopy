//! The draw list: what a renderer needs to draw a UI frame in one quad pass.
//!
//! Every primitive is one instanced [`Quad`] (64 bytes): rounded boxes with borders,
//! shadows, gradients, sweeps, images and glyphs share a single pipeline. Quads are
//! grouped into [`Batch`]es only where the bound image texture or the scissor rectangle
//! changes; glyphs and shapes never break a batch (the glyph atlas is always bound).

use crate::geom::{Color, Rect};
use bytemuck::{Pod, Zeroable};

/// A texture the game owns: a UI image, an icon, or a render target such as the
/// minimap or the portrait. The renderer maps ids to its texture views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureId(pub u32);

/// What a quad draws. Stored in the low byte of [`Quad::flags`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum QuadKind {
    /// Rounded box: `color` fill, `border` px of `color2` inside the edge.
    Shape = 0,
    /// Image texel × `color` (tint). Corners rounded by `radius`.
    Image = 1,
    /// Glyph-atlas coverage × `color`. With `soft` > 0, a blurred shadow.
    Glyph = 2,
    /// Outer box shadow: `uv.xy` offset, `border` spread, `soft` blur. Drawn only
    /// outside the box, as in CSS.
    Shadow = 3,
    /// Inset box shadow, drawn only inside the box.
    InsetShadow = 4,
    /// Conic sweep clockwise from twelve o'clock: `color` up to `param` (0–1), `color2`
    /// after. With `border` > 0 only a ring of that thickness.
    Sweep = 5,
    /// Linear gradient `color` → `color2` at CSS angle `param` degrees.
    Linear = 6,
    /// Radial gradient `color` (centre) → `color2` (farthest corner).
    Radial = 7,
}

/// Flag: image texels are premultiplied by alpha.
pub const FLAG_PREMULTIPLIED: u32 = 1 << 8;
/// Flag: image converted to luminance before tinting.
pub const FLAG_GRAYSCALE: u32 = 1 << 9;

/// One instance of the UI pipeline. Positions are physical pixels, origin top left.
/// Colours are sRGB with straight alpha; the pass blends in sRGB space.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Quad {
    /// `x0, y0, x1, y1`.
    pub rect: [f32; 4],
    /// Texture coordinates `u0, v0, u1, v1` (normalised) for images and glyphs;
    /// the offset for shadows.
    pub uv: [f32; 4],
    pub color: [u8; 4],
    pub color2: [u8; 4],
    pub radius: f32,
    pub border: f32,
    pub soft: f32,
    pub param: f32,
    /// Brightness multiplier (CSS `filter: brightness()`).
    pub shade: f32,
    /// [`QuadKind`] in the low byte, `FLAG_*` above.
    pub flags: u32,
}

const _: () = assert!(std::mem::size_of::<Quad>() == 64);

impl Quad {
    pub fn kind(&self) -> u32 {
        self.flags & 0xff
    }

    pub fn new(kind: QuadKind, rect: Rect, color: Color) -> Self {
        Self {
            rect: [rect.x, rect.y, rect.right(), rect.bottom()],
            uv: [0.0, 0.0, 1.0, 1.0],
            color: color.to_array(),
            color2: [0; 4],
            radius: 0.0,
            border: 0.0,
            soft: 0.0,
            param: 0.0,
            shade: 1.0,
            flags: kind as u32,
        }
    }
}

/// A run of quads drawn with one image texture and one scissor rectangle.
#[derive(Debug, Clone, PartialEq)]
pub struct Batch {
    /// The image texture bound for the run; `None` when it draws no images.
    pub texture: Option<TextureId>,
    /// Physical pixels `x, y, w, h`; `None` for the whole target.
    pub scissor: Option<[u32; 4]>,
    pub quads: std::ops::Range<u32>,
}

/// A rectangle of new glyph-atlas texels (R8, tightly packed rows).
#[derive(Debug, Clone, PartialEq)]
pub struct AtlasUpdate {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Glyph atlas changes since the previous frame.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct AtlasChanges {
    /// The atlas size; when it differs from last frame (or `reset`), recreate the
    /// texture cleared to zero before applying `updates`.
    pub size: u32,
    pub reset: bool,
    pub updates: Vec<AtlasUpdate>,
}

/// One frame of UI drawing.
#[derive(Debug, Clone, Default)]
pub struct DrawList {
    /// Target size in physical pixels.
    pub viewport: [u32; 2],
    pub quads: Vec<Quad>,
    pub batches: Vec<Batch>,
    pub atlas: AtlasChanges,
}

impl DrawList {
    pub(crate) fn clear(&mut self, viewport: [u32; 2]) {
        self.viewport = viewport;
        self.quads.clear();
        self.batches.clear();
        self.atlas.reset = false;
        self.atlas.updates.clear();
    }

    /// Every image texture the frame uses.
    pub fn textures(&self) -> impl Iterator<Item = TextureId> + '_ {
        self.batches.iter().filter_map(|b| b.texture)
    }
}

/// Appends quads and maintains batches.
pub(crate) struct Painter<'a> {
    pub list: &'a mut DrawList,
    scissor: Option<[u32; 4]>,
    clip: Option<Rect>,
}

impl<'a> Painter<'a> {
    pub fn new(list: &'a mut DrawList) -> Self {
        Self {
            list,
            scissor: None,
            clip: None,
        }
    }

    pub fn clip(&self) -> Option<Rect> {
        self.clip
    }

    /// Sets the clip rectangle (physical pixels) for following quads.
    pub fn set_clip(&mut self, clip: Option<Rect>) {
        self.clip = clip;
        self.scissor = clip.map(|r| {
            let x0 = r.x.max(0.0).floor();
            let y0 = r.y.max(0.0).floor();
            let x1 = r.right().max(x0).ceil();
            let y1 = r.bottom().max(y0).ceil();
            [x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32]
        });
    }

    /// Whether a rectangle is entirely clipped away (or off screen).
    pub fn culled(&self, r: &Rect) -> bool {
        let screen = Rect::new(
            0.0,
            0.0,
            self.list.viewport[0] as f32,
            self.list.viewport[1] as f32,
        );
        let visible = match &self.clip {
            Some(c) => c.intersect(&screen),
            None => screen,
        };
        r.right() <= visible.x
            || r.bottom() <= visible.y
            || r.x >= visible.right()
            || r.y >= visible.bottom()
    }

    pub fn push(&mut self, quad: Quad, texture: Option<TextureId>) {
        let index = self.list.quads.len() as u32;
        let scissor = self.scissor;
        let start_new = match self.list.batches.last_mut() {
            None => true,
            Some(b) => {
                if b.scissor != scissor {
                    true
                } else {
                    match (b.texture, texture) {
                        (_, None) => false,
                        (None, Some(t)) => {
                            b.texture = Some(t);
                            false
                        }
                        (Some(a), Some(t)) => a != t,
                    }
                }
            }
        };
        if start_new {
            self.list.batches.push(Batch {
                texture,
                scissor,
                quads: index..index,
            });
        }
        self.list.quads.push(quad);
        if let Some(b) = self.list.batches.last_mut() {
            b.quads.end = index + 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_split_only_on_texture_or_scissor() {
        let mut list = DrawList::default();
        list.clear([100, 100]);
        let mut p = Painter::new(&mut list);
        let q = Quad::new(QuadKind::Shape, Rect::new(0.0, 0.0, 1.0, 1.0), Color::WHITE);
        p.push(q, None);
        p.push(q, Some(TextureId(1)));
        p.push(q, None);
        p.push(q, Some(TextureId(1)));
        p.push(q, Some(TextureId(2)));
        p.set_clip(Some(Rect::new(0.0, 0.0, 10.0, 10.0)));
        p.push(q, None);
        assert_eq!(list.batches.len(), 3);
        assert_eq!(list.batches[0].texture, Some(TextureId(1)));
        assert_eq!(list.batches[0].quads, 0..4);
        assert_eq!(list.batches[1].quads, 4..5);
        assert_eq!(list.batches[2].scissor, Some([0, 0, 10, 10]));
    }
}
