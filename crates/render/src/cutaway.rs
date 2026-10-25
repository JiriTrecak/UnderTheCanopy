//! The scenery cutaway (old `sceneryCutaway.ts`): giant canopy models (`Model::canopy`)
//! open around the viewer's units behind them. A small screen-space mask holds, per texel,
//! the farthest covering unit's view depth and a coverage weight, so an army of any size
//! costs the shader four texel loads; the static models' fragment stipples away where a
//! unit stands at least about a metre behind it. Shadows never see the mask, so the
//! forest still blocks the sun.

use crate::camera::CameraView;
use glam::Vec3;

/// Mask size (old 192 × 108): coarse on purpose, sampled with a manual bilinear filter.
pub const WIDTH: usize = 192;
pub const HEIGHT: usize = 108;
/// The mask's texture format: depth and weight, read with `textureLoad`.
pub const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rg32Float;

/// A unit the canopy opens around: where it stands and how tall it is (pick height).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CutawaySubject {
    pub position: Vec3,
    pub height: f32,
}

/// The mask on the CPU, rebuilt each frame; only rows touched in either of the last two
/// frames are cleared and compared.
pub struct CutawayMask {
    data: Vec<[f32; 2]>,
    next: Vec<[f32; 2]>,
    /// Row span `[lo, hi)` written last frame; everything outside is zero.
    written: (usize, usize),
    /// Some unit covers the screen this frame.
    pub active: bool,
    /// What the mask was last made from: the same camera and units make the same mask.
    made_from: Option<([glam::Mat4; 3], Vec<CutawaySubject>)>,
}

impl Default for CutawayMask {
    fn default() -> Self {
        Self {
            data: vec![[0.0; 2]; WIDTH * HEIGHT],
            next: vec![[0.0; 2]; WIDTH * HEIGHT],
            written: (0, 0),
            active: false,
            made_from: None,
        }
    }
}

impl CutawayMask {
    /// Rasterises `subjects` seen by `camera`; returns the row span whose texels changed
    /// (to upload), if any. Rows run from the top of the screen down.
    pub fn update(
        &mut self,
        camera: &CameraView,
        subjects: &[CutawaySubject],
    ) -> Option<(usize, usize)> {
        let matrices = [camera.view_projection, camera.view, camera.projection];
        if let Some((made, units)) = &mut self.made_from {
            if *made == matrices && units.as_slice() == subjects {
                return None;
            }
            *made = matrices;
            units.clear();
            units.extend_from_slice(subjects);
        } else {
            self.made_from = Some((matrices, subjects.to_vec()));
        }
        let previous = self.written;
        self.next[previous.0 * WIDTH..previous.1 * WIDTH].fill([0.0; 2]);
        let (mut lo, mut hi, mut count) = (HEIGHT, 0, 0);
        // Horizontal stretch of a screen-space circle (old projection elements 5 / 0).
        let aspect = camera.projection.y_axis.y / camera.projection.x_axis.x;
        let ndc = |p: Vec3| {
            let c = camera.view_projection * p.extend(1.0);
            (c.w > 1e-4).then(|| Vec3::new(c.x / c.w, c.y / c.w, c.z / c.w))
        };
        for s in subjects {
            let h = s.height.max(0.8);
            let centre = s.position + Vec3::Y * (h * 0.5);
            let depth = (-camera.view.transform_point3(centre).z * 10.0).round() / 10.0;
            if depth <= 0.0 {
                continue;
            }
            let (Some(point), Some(top)) = (ndc(centre), ndc(centre + Vec3::Y * h)) else {
                continue;
            };
            if !(0.0..=1.0).contains(&point.z) {
                continue;
            }
            let ry = ((top.y - point.y).abs() * HEIGHT as f32 * 0.75).max(2.5);
            let rx = ry * WIDTH as f32 / HEIGHT as f32 / aspect;
            let cx = (point.x * 0.5 + 0.5) * WIDTH as f32;
            let cy = (0.5 - point.y * 0.5) * HEIGHT as f32;
            if cx + rx < 0.0 || cy + ry < 0.0 || cx - rx >= WIDTH as f32 || cy - ry >= HEIGHT as f32
            {
                continue;
            }
            count += 1;
            let y0 = (cy - ry).floor().max(0.0) as usize;
            let y1 = ((cy + ry).ceil() as usize).min(HEIGHT - 1);
            let x0 = (cx - rx).floor().max(0.0) as usize;
            let x1 = ((cx + rx).ceil() as usize).min(WIDTH - 1);
            lo = lo.min(y0);
            hi = hi.max(y1 + 1);
            for y in y0..=y1 {
                let dy = (y as f32 + 0.5 - cy) / ry;
                if dy * dy >= 1.0 {
                    continue;
                }
                for x in x0..=x1 {
                    let dx = (x as f32 + 0.5 - cx) / rx;
                    let d2 = dx * dx + dy * dy;
                    if d2 >= 1.0 {
                        continue;
                    }
                    let t = ((1.0 - d2.sqrt()) / 0.36).min(1.0);
                    let weight = t * t * (3.0 - 2.0 * t);
                    let texel = &mut self.next[y * WIDTH + x];
                    texel[0] = texel[0].max(depth);
                    texel[1] = texel[1].max(weight);
                }
            }
        }
        self.active = count > 0;
        self.written = if self.active { (lo, hi) } else { (0, 0) };
        // Idle armies do not upload the same mask again.
        let (from, to) = match (previous.0 < previous.1, self.active) {
            (true, true) => (previous.0.min(lo), previous.1.max(hi)),
            (true, false) => previous,
            (false, true) => (lo, hi),
            (false, false) => return None,
        };
        let span = from * WIDTH..to * WIDTH;
        if self.data[span.clone()] == self.next[span.clone()] {
            return None;
        }
        self.data[span.clone()].copy_from_slice(&self.next[span]);
        Some((from, to))
    }

    /// Rows `from..to` of the mask as texture bytes.
    pub fn rows(&self, from: usize, to: usize) -> &[u8] {
        bytemuck::cast_slice(&self.data[from * WIDTH..to * WIDTH])
    }

    /// The texel at column `x`, row `y` (from the top): depth and weight.
    pub fn texel(&self, x: usize, y: usize) -> [f32; 2] {
        self.data[y * WIDTH + x]
    }

    /// The farthest covering unit's depth in each [`BLOCK`]-texel square of the mask (0
    /// where no unit covers it), rows from the top: what the static culling reads to
    /// leave out of the cutaway's path every canopy instance no unit stands behind.
    pub fn block_depths(&self) -> [f32; BLOCKS[0] * BLOCKS[1]] {
        let mut blocks = [0.0f32; BLOCKS[0] * BLOCKS[1]];
        let (lo, hi) = self.written;
        for y in lo..hi {
            for (x, texel) in self.data[y * WIDTH..(y + 1) * WIDTH].iter().enumerate() {
                if texel[1] > 0.0 {
                    let block = &mut blocks[(y / BLOCK) * BLOCKS[0] + x / BLOCK];
                    *block = block.max(texel[0]);
                }
            }
        }
        blocks
    }
}

/// Texels on a side of a block of [`CutawayMask::block_depths`].
pub const BLOCK: usize = 8;
/// Blocks across and down the mask.
pub const BLOCKS: [usize; 2] = [WIDTH.div_ceil(BLOCK), HEIGHT.div_ceil(BLOCK)];

/// The mask's texture.
pub fn texture(device: &wgpu::Device) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("scenery cutaway"),
        size: wgpu::Extent3d {
            width: WIDTH as u32,
            height: HEIGHT as u32,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Writes rows `from..to` of `mask` into `texture`.
pub fn upload(
    queue: &impl crate::upload::Writes,
    texture: &wgpu::Texture,
    mask: &CutawayMask,
    rows: (usize, usize),
) {
    let (from, to) = rows;
    queue.write_texture(
        wgpu::TexelCopyTextureInfo {
            texture,
            mip_level: 0,
            origin: wgpu::Origin3d {
                x: 0,
                y: from as u32,
                z: 0,
            },
            aspect: wgpu::TextureAspect::All,
        },
        mask.rows(from, to),
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some((WIDTH * 8) as u32),
            rows_per_image: None,
        },
        wgpu::Extent3d {
            width: WIDTH as u32,
            height: (to - from) as u32,
            depth_or_array_layers: 1,
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> CameraView {
        CameraView::look_at(
            Vec3::new(0.0, 20.0, 20.0),
            Vec3::ZERO,
            0.8,
            16.0 / 9.0,
            0.5,
            500.0,
        )
    }

    #[test]
    fn a_unit_covers_texels_around_it_and_clears_when_gone() {
        let mut mask = CutawayMask::default();
        let unit = CutawaySubject {
            position: Vec3::ZERO,
            height: 2.0,
        };
        let rows = mask.update(&camera(), &[unit]).expect("uploaded");
        assert!(mask.active);
        // The unit stands at the middle of the screen.
        let [depth, weight] = mask.texel(WIDTH / 2, HEIGHT / 2);
        assert!(depth > 20.0 && depth < 40.0, "{depth}");
        assert!(weight > 0.9, "{weight}");
        assert_eq!(mask.texel(0, 0), [0.0, 0.0]);
        assert!(rows.0 < HEIGHT / 2 && rows.1 > HEIGHT / 2);
        // The same frame again uploads nothing.
        assert_eq!(mask.update(&camera(), &[unit]), None);
        // Gone: the rows clear once.
        assert_eq!(mask.update(&camera(), &[]), Some(rows));
        assert!(!mask.active);
        assert_eq!(mask.texel(WIDTH / 2, HEIGHT / 2), [0.0, 0.0]);
        assert_eq!(mask.update(&camera(), &[]), None);
    }

    #[test]
    fn blocks_hold_the_farthest_unit_they_cover() {
        let mut mask = CutawayMask::default();
        let unit = CutawaySubject {
            position: Vec3::ZERO,
            height: 2.0,
        };
        mask.update(&camera(), &[unit]);
        let blocks = mask.block_depths();
        let [depth, _] = mask.texel(WIDTH / 2, HEIGHT / 2);
        let centre = (HEIGHT / 2 / BLOCK) * BLOCKS[0] + WIDTH / 2 / BLOCK;
        assert_eq!(blocks[centre], depth);
        // Every texel a unit covers lies under a block holding at least its depth.
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let [d, w] = mask.texel(x, y);
                let block = blocks[(y / BLOCK) * BLOCKS[0] + x / BLOCK];
                assert!(w <= 0.0 || block >= d);
            }
        }
        assert_eq!(blocks[0], 0.0);
    }

    #[test]
    fn units_behind_the_camera_are_ignored() {
        let mut mask = CutawayMask::default();
        let behind = CutawaySubject {
            position: Vec3::new(0.0, 0.0, 60.0),
            height: 2.0,
        };
        assert_eq!(mask.update(&camera(), &[behind]), None);
        assert!(!mask.active);
    }
}
