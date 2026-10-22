//! Review images of a scene preview: a labelled contact sheet of its frames, and
//! before/after pairs. Neutral zinc panels (AGENTS.md, Visual previews).

use anyhow::{Context, Result};
use cosmic_text::{Attrs, Buffer, Color, Family, FontSystem, Metrics, Shaping, SwashCache};
use image::{RgbaImage, imageops};
use std::path::Path;

const BACKGROUND: [u8; 3] = [0x18, 0x18, 0x1b];
const PANEL: [u8; 3] = [0x27, 0x27, 0x2a];
const BORDER: [u8; 3] = [0x52, 0x52, 0x5b];
const TEXT: [u8; 3] = [0xe4, 0xe4, 0xe7];
const DIM: [u8; 3] = [0xa1, 0xa1, 0xaa];

const GAP: u32 = 12;
const TEXT_SIZE: f32 = 15.0;
const LINE: f32 = 20.0;

/// One picture of a sheet: the image and two lines under it.
pub struct Tile<'a> {
    pub image: Option<&'a Path>,
    pub heading: String,
    pub caption: String,
}

/// Draws text with the system's sans-serif face.
pub struct Painter {
    fonts: FontSystem,
    cache: SwashCache,
}

impl Painter {
    pub fn new() -> Self {
        Self {
            fonts: FontSystem::new(),
            cache: SwashCache::new(),
        }
    }

    /// Writes `text` into the box at `x`, `y` of `width`, wrapped onto at most `lines`.
    #[allow(clippy::too_many_arguments)]
    fn text(
        &mut self,
        image: &mut RgbaImage,
        x: u32,
        y: u32,
        width: u32,
        lines: u32,
        size: f32,
        color: [u8; 3],
        text: &str,
    ) {
        let height = size / TEXT_SIZE * LINE;
        let mut buffer = Buffer::new(&mut self.fonts, Metrics::new(size, height));
        buffer.set_size(Some(width as f32), Some(height * lines as f32));
        buffer.set_text(
            text,
            &Attrs::new().family(Family::SansSerif),
            Shaping::Advanced,
            None,
        );
        let [r, g, b] = color;
        let (w, h) = image.dimensions();
        // Wrapped lines past the box are cut off.
        let bottom = (height * lines as f32) as i32;
        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            Color::rgb(r, g, b),
            |gx, gy, gw, gh, c| {
                let alpha = u32::from(c.a());
                if alpha == 0 {
                    return;
                }
                for py in gy.max(0)..(gy + gh as i32).min(bottom) {
                    for px in gx.max(0)..(gx + gw as i32) {
                        let (ix, iy) = (x as i32 + px, y as i32 + py);
                        if px >= width as i32 || ix as u32 >= w || iy as u32 >= h {
                            continue;
                        }
                        let pixel = image.get_pixel_mut(ix as u32, iy as u32);
                        for (channel, value) in pixel.0.iter_mut().zip([c.r(), c.g(), c.b()]) {
                            *channel = ((u32::from(value) * alpha
                                + u32::from(*channel) * (255 - alpha))
                                / 255) as u8;
                        }
                    }
                }
            },
        );
    }
}

fn filled(width: u32, height: u32, color: [u8; 3]) -> RgbaImage {
    RgbaImage::from_pixel(
        width,
        height,
        image::Rgba([color[0], color[1], color[2], 255]),
    )
}

/// Draws `tile` into `sheet` at `x`, `y`: the picture scaled to `width`×`height`, its
/// heading and caption below it.
#[allow(clippy::too_many_arguments)]
fn tile(
    painter: &mut Painter,
    sheet: &mut RgbaImage,
    tile: &Tile<'_>,
    x: u32,
    y: u32,
    width: u32,
    height: u32,
    caption_lines: u32,
) -> Result<()> {
    let label = (LINE as u32) * (1 + caption_lines) + GAP;
    imageops::replace(
        sheet,
        &filled(width + 2, height + label + 2, BORDER),
        i64::from(x) - 1,
        i64::from(y) - 1,
    );
    imageops::replace(
        sheet,
        &filled(width, height + label, PANEL),
        i64::from(x),
        i64::from(y),
    );
    match tile.image {
        Some(path) => {
            let picture = image::open(path)
                .with_context(|| path.display().to_string())?
                .to_rgba8();
            let scaled = imageops::resize(&picture, width, height, imageops::FilterType::Triangle);
            imageops::replace(sheet, &scaled, i64::from(x), i64::from(y));
        }
        None => painter.text(
            sheet,
            x + GAP,
            y + height / 2 - LINE as u32,
            width - 2 * GAP,
            1,
            TEXT_SIZE,
            DIM,
            "(no frame)",
        ),
    }
    let below = y + height + GAP / 2;
    painter.text(
        sheet,
        x + GAP / 2,
        below,
        width - GAP,
        1,
        TEXT_SIZE,
        TEXT,
        &tile.heading,
    );
    painter.text(
        sheet,
        x + GAP / 2,
        below + LINE as u32,
        width - GAP,
        caption_lines,
        TEXT_SIZE,
        DIM,
        &tile.caption,
    );
    Ok(())
}

/// A contact sheet: `title` over the tiles in rows of `columns`, each `width` wide at the
/// pictures' aspect `aspect` (width over height).
pub fn contact(
    painter: &mut Painter,
    title: &str,
    tiles: &[Tile<'_>],
    columns: u32,
    width: u32,
    aspect: f32,
    out: &Path,
) -> Result<()> {
    let height = (width as f32 / aspect).round() as u32;
    let caption_lines = 2;
    let cell = [
        width + GAP,
        height + (LINE as u32) * (1 + caption_lines) + 2 * GAP,
    ];
    let columns = columns.min(tiles.len().max(1) as u32);
    let rows = (tiles.len() as u32).div_ceil(columns).max(1);
    let header = 2 * LINE as u32 + GAP;
    let mut sheet = filled(
        GAP + columns * cell[0],
        header + GAP + rows * cell[1],
        BACKGROUND,
    );
    painter.text(
        &mut sheet,
        GAP,
        GAP,
        columns * cell[0] - GAP,
        1,
        20.0,
        TEXT,
        title,
    );
    for (i, t) in tiles.iter().enumerate() {
        let (column, row) = (i as u32 % columns, i as u32 / columns);
        tile(
            painter,
            &mut sheet,
            t,
            GAP + column * cell[0],
            header + GAP + row * cell[1],
            width,
            height,
            caption_lines,
        )?;
    }
    sheet.save(out).with_context(|| out.display().to_string())
}

/// Two pictures side by side, each `width` wide: before on the left, after on the right.
pub fn pair(
    painter: &mut Painter,
    before: &Tile<'_>,
    after: &Tile<'_>,
    width: u32,
    aspect: f32,
    out: &Path,
) -> Result<()> {
    let height = (width as f32 / aspect).round() as u32;
    let caption_lines = 2;
    let label = (LINE as u32) * (1 + caption_lines) + GAP;
    let mut sheet = filled(3 * GAP + 2 * width, 2 * GAP + height + label, BACKGROUND);
    tile(
        painter,
        &mut sheet,
        before,
        GAP,
        GAP,
        width,
        height,
        caption_lines,
    )?;
    tile(
        painter,
        &mut sheet,
        after,
        2 * GAP + width,
        GAP,
        width,
        height,
        caption_lines,
    )?;
    sheet.save(out).with_context(|| out.display().to_string())
}
