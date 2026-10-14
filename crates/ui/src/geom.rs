//! Small geometry and colour types shared by layout, painting and input.

/// A point or size in pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }
}

/// An axis-aligned rectangle: origin at the top left, y down.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0, 0.0);

    pub const fn new(x: f32, y: f32, w: f32, h: f32) -> Self {
        Self { x, y, w, h }
    }

    pub fn right(&self) -> f32 {
        self.x + self.w
    }

    pub fn bottom(&self) -> f32 {
        self.y + self.h
    }

    pub fn contains(&self, p: Vec2) -> bool {
        p.x >= self.x && p.y >= self.y && p.x < self.right() && p.y < self.bottom()
    }

    pub fn is_empty(&self) -> bool {
        self.w <= 0.0 || self.h <= 0.0
    }

    pub fn intersect(&self, other: &Rect) -> Rect {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let r = self.right().min(other.right());
        let b = self.bottom().min(other.bottom());
        Rect::new(x, y, (r - x).max(0.0), (b - y).max(0.0))
    }

    /// Grows each edge outwards by `[top, right, bottom, left]` (negative shrinks).
    pub fn outset(&self, edges: [f32; 4]) -> Rect {
        Rect::new(
            self.x - edges[3],
            self.y - edges[0],
            (self.w + edges[1] + edges[3]).max(0.0),
            (self.h + edges[0] + edges[2]).max(0.0),
        )
    }

    pub fn expand(&self, by: f32) -> Rect {
        self.outset([by; 4])
    }

    pub fn translate(&self, d: Vec2) -> Rect {
        Rect::new(self.x + d.x, self.y + d.y, self.w, self.h)
    }

    pub fn center(&self) -> Vec2 {
        Vec2::new(self.x + self.w * 0.5, self.y + self.h * 0.5)
    }
}

/// An sRGB colour with straight (not premultiplied) alpha, as written in CSS.
///
/// The UI composites in sRGB space like a browser, so colours from the old game's
/// stylesheets come out the same (see `docs/ui.md`).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
    pub a: u8,
}

impl Color {
    pub const TRANSPARENT: Self = Self::rgba(0, 0, 0, 0);
    pub const BLACK: Self = Self::rgba(0, 0, 0, 255);
    pub const WHITE: Self = Self::rgba(255, 255, 255, 255);

    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// `0xRRGGBB`, opaque.
    pub const fn hex(rgb: u32) -> Self {
        Self::rgba((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8, 255)
    }

    pub fn with_alpha(self, a: f32) -> Self {
        Self {
            a: (self.a as f32 * a.clamp(0.0, 1.0)).round() as u8,
            ..self
        }
    }

    pub fn to_array(self) -> [u8; 4] {
        [self.r, self.g, self.b, self.a]
    }

    pub fn is_transparent(self) -> bool {
        self.a == 0
    }
}

/// A 2D scale-and-translate transform from layout units to physical pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Xform {
    pub scale: f32,
    pub tx: f32,
    pub ty: f32,
}

impl Xform {
    pub fn scaling(scale: f32) -> Self {
        Self {
            scale,
            tx: 0.0,
            ty: 0.0,
        }
    }

    pub fn point(&self, p: Vec2) -> Vec2 {
        Vec2::new(p.x * self.scale + self.tx, p.y * self.scale + self.ty)
    }

    pub fn rect(&self, r: Rect) -> Rect {
        let p = self.point(Vec2::new(r.x, r.y));
        Rect::new(p.x, p.y, r.w * self.scale, r.h * self.scale)
    }

    /// This transform followed by a scale of `s` about `origin` (in layout units).
    pub fn then_scale_about(&self, s: f32, origin: Vec2) -> Self {
        // x' = self(origin + (x - origin) * s)
        Self {
            scale: self.scale * s,
            tx: self.tx + self.scale * origin.x * (1.0 - s),
            ty: self.ty + self.scale * origin.y * (1.0 - s),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_about_keeps_origin_fixed() {
        let xf = Xform::scaling(2.0).then_scale_about(0.5, Vec2::new(100.0, 50.0));
        assert_eq!(xf.point(Vec2::new(100.0, 50.0)), Vec2::new(200.0, 100.0));
        assert_eq!(xf.point(Vec2::new(110.0, 50.0)), Vec2::new(210.0, 100.0));
    }

    #[test]
    fn intersect_clamps_to_empty() {
        let a = Rect::new(0.0, 0.0, 10.0, 10.0);
        let b = Rect::new(20.0, 0.0, 10.0, 10.0);
        assert!(a.intersect(&b).is_empty());
        assert_eq!(
            a.intersect(&Rect::new(5.0, 5.0, 10.0, 10.0)),
            Rect::new(5.0, 5.0, 5.0, 5.0)
        );
    }
}
