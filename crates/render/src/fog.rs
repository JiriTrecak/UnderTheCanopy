//! Fog of war (renderer §4.7): the viewer's cells (unexplored, explored, in sight) become
//! an R8 light texture — in sight 255, explored 90, unexplored 0, softened by a 5×5 box
//! filter and sampled bilinearly (old `FogRaster`, `FogOfWar`). Only the rectangle around
//! changed cells is filtered again and uploaded. Every forward surface darkens by it in
//! `scene_out` (`frame.wgsl`, the old fog look: desaturated and cooled towards the
//! explored level, black where unexplored and outside the map); the volumetric haze is
//! masked by it as well. Without fog (observers) the texture is a white texel and the
//! function returns the colour unchanged.

/// Light of a cell in sight, explored and unexplored (old raster values).
pub const LIGHT: [u8; 3] = [0, 90, 255];
/// Half the box filter's side.
const RADIUS: i32 = 2;

/// A rectangle of cells, inclusive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rect {
    pub x0: u32,
    pub z0: u32,
    pub x1: u32,
    pub z1: u32,
}

/// The CPU side: the filtered light of every cell, kept up to date by rectangle.
#[derive(Debug, Clone)]
pub struct FogRaster {
    size: u32,
    cells: Vec<u8>,
    horizontal: Vec<u16>,
    pub light: Vec<u8>,
}

impl FogRaster {
    pub fn new(size: u32) -> Self {
        let n = (size * size) as usize;
        Self {
            size,
            // Nothing seen yet; the first update covers every cell that differs.
            cells: vec![0; n],
            horizontal: vec![0; n],
            light: vec![0; n],
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }

    /// Takes the viewer's cells (0 unexplored, 1 explored, 2 in sight; `size²`). With
    /// `changed`, only those cells are compared; otherwise all. Returns the rectangle of
    /// light that changed, if any.
    pub fn update(&mut self, cells: &[u8], changed: Option<&[u32]>) -> Option<Rect> {
        let size = self.size as usize;
        if cells.len() < size * size {
            return None;
        }
        let (mut x0, mut z0, mut x1, mut z1) = (usize::MAX, usize::MAX, 0, 0);
        let mut mark = |i: usize| {
            let (x, z) = (i % size, i / size);
            x0 = x0.min(x);
            x1 = x1.max(x);
            z0 = z0.min(z);
            z1 = z1.max(z);
        };
        match changed {
            Some(list) => {
                for &i in list {
                    let i = i as usize;
                    if i < size * size && self.cells[i] != cells[i] {
                        self.cells[i] = cells[i];
                        mark(i);
                    }
                }
            }
            None => {
                for (i, (old, &new)) in self.cells.iter_mut().zip(cells).enumerate() {
                    if *old != new {
                        *old = new;
                        mark(i);
                    }
                }
            }
        }
        if x0 == usize::MAX {
            return None;
        }
        let r = RADIUS as usize;
        let (x0, x1) = (x0.saturating_sub(r), (x1 + r).min(size - 1));
        let (hz0, hz1) = (z0, z1);
        let (z0, z1) = (z0.saturating_sub(r), (z1 + r).min(size - 1));
        let value = |c: u8| u16::from(LIGHT[usize::from(c.min(2))]);
        for z in hz0..=hz1 {
            for x in x0..=x1 {
                let mut sum = 0;
                for dx in -RADIUS..=RADIUS {
                    let xx = x as i32 + dx;
                    if xx >= 0 && (xx as usize) < size {
                        sum += value(self.cells[z * size + xx as usize]);
                    }
                }
                self.horizontal[z * size + x] = sum;
            }
        }
        for z in z0..=z1 {
            for x in x0..=x1 {
                let mut sum = 0u32;
                for dz in -RADIUS..=RADIUS {
                    let zz = z as i32 + dz;
                    if zz >= 0 && (zz as usize) < size {
                        sum += u32::from(self.horizontal[zz as usize * size + x]);
                    }
                }
                // The old raster divides by the full 25 taps at the map's edge too.
                self.light[z * size + x] = ((sum as f32 / 25.0).round()) as u8;
            }
        }
        Some(Rect {
            x0: x0 as u32,
            z0: z0 as u32,
            x1: x1 as u32,
            z1: z1 as u32,
        })
    }
}

/// The viewer's cells: `size²` of them (0 unexplored, 1 explored, 2 in sight), and the
/// cells that changed since the previous update when known.
#[derive(Debug, Clone, Copy)]
pub struct FogCells<'a> {
    pub size: u32,
    pub cells: &'a [u8],
    pub changed: Option<&'a [u32]>,
}

/// The fog texture on the GPU.
pub struct FogGpu {
    pub raster: Option<FogRaster>,
    texture: wgpu::Texture,
    pub view: wgpu::TextureView,
    /// Bytes uploaded by the latest update (for measurements).
    pub uploaded: u64,
}

impl FogGpu {
    /// No fog: one white texel.
    pub fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let texture = crate::texture::solid(
            device,
            queue,
            "no fog",
            wgpu::TextureFormat::R8Unorm,
            [255; 4],
        );
        Self {
            raster: None,
            view: texture.create_view(&Default::default()),
            texture,
            uploaded: 0,
        }
    }

    /// Whether fog applies.
    pub fn enabled(&self) -> bool {
        self.raster.is_some()
    }

    /// Takes the viewer's cells; `None` turns fog off. True when the texture was
    /// replaced (bind groups must be rebuilt).
    pub fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        fog: Option<FogCells<'_>>,
    ) -> bool {
        self.uploaded = 0;
        let Some(FogCells {
            size,
            cells,
            changed,
        }) = fog
        else {
            let replaced = self.raster.is_some();
            if replaced {
                *self = Self::new(device, queue);
            }
            return replaced;
        };
        let mut replaced = false;
        if self.raster.as_ref().is_none_or(|r| r.size() != size) {
            self.raster = Some(FogRaster::new(size));
            self.texture = device.create_texture(&wgpu::TextureDescriptor {
                label: Some("fog of war"),
                size: wgpu::Extent3d {
                    width: size,
                    height: size,
                    depth_or_array_layers: 1,
                },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R8Unorm,
                usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                view_formats: &[],
            });
            self.view = self.texture.create_view(&Default::default());
            replaced = true;
        }
        let raster = self.raster.as_mut().expect("made above");
        // A new texture needs every cell.
        let changed = if replaced { None } else { changed };
        let rect = raster.update(cells, changed);
        let rect = if replaced {
            Some(Rect {
                x0: 0,
                z0: 0,
                x1: size - 1,
                z1: size - 1,
            })
        } else {
            rect
        };
        if let Some(r) = rect {
            let width = r.x1 - r.x0 + 1;
            let height = r.z1 - r.z0 + 1;
            let mut rows = Vec::with_capacity((width * height) as usize);
            for z in r.z0..=r.z1 {
                let start = (z * size + r.x0) as usize;
                rows.extend_from_slice(&raster.light[start..start + width as usize]);
            }
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d {
                        x: r.x0,
                        y: r.z0,
                        z: 0,
                    },
                    aspect: wgpu::TextureAspect::All,
                },
                &rows,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
            self.uploaded = rows.len() as u64;
        }
        replaced
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn light_is_a_box_filtered_level_updated_by_rectangle() {
        let size = 16;
        let mut raster = FogRaster::new(size);
        let mut cells = vec![0u8; (size * size) as usize];
        // Nothing changed: nothing to upload.
        assert_eq!(raster.update(&cells, None), None);
        // One cell in sight in the middle: its 5×5 neighbourhood lights up.
        cells[(8 * size + 8) as usize] = 2;
        let rect = raster.update(&cells, None).unwrap();
        assert_eq!(
            rect,
            Rect {
                x0: 6,
                z0: 6,
                x1: 10,
                z1: 10
            }
        );
        assert_eq!(raster.light[(8 * size + 8) as usize], 10); // 255 / 25
        assert_eq!(raster.light[(8 * size + 10) as usize], 10);
        assert_eq!(raster.light[(8 * size + 11) as usize], 0);
        // A whole explored field: 90 everywhere away from the edges.
        let explored = vec![1u8; (size * size) as usize];
        raster.update(&explored, None).unwrap();
        assert_eq!(raster.light[(8 * size + 8) as usize], 90);
        // Changes listed by cell only look at those cells.
        let mut seen = explored.clone();
        seen[(3 * size + 3) as usize] = 2;
        seen[(12 * size + 12) as usize] = 2;
        let rect = raster.update(&seen, Some(&[3 * size + 3])).unwrap();
        assert_eq!((rect.x0, rect.z0, rect.x1, rect.z1), (1, 1, 5, 5));
        assert_eq!(raster.light[(12 * size + 12) as usize], 90);
        let lit = raster.light[(3 * size + 3) as usize];
        assert_eq!(lit, ((24.0 * 90.0 + 255.0) / 25.0f32).round() as u8);
    }
}
