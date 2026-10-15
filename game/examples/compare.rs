//! Side-by-side review images: `cargo run --release -p canopy --example compare -- <old.png>
//! <new.png> <out.png> [<x> <y> <w> <h> [<scale>]]` writes old (left) and new (right) next
//! to each other, the new one scaled to the old one's height, optionally cropped to the
//! same rectangle of both and enlarged, and prints both pictures' mean colours.

use anyhow::{Context, Result};
use image::{GenericImage, RgbaImage, imageops::FilterType};

fn mean(image: &RgbaImage) -> [f64; 3] {
    let n = f64::from(image.width() * image.height()).max(1.0);
    let mut sum = [0.0; 3];
    for p in image.pixels() {
        for (s, c) in sum.iter_mut().zip(p.0) {
            *s += f64::from(c);
        }
    }
    sum.map(|s| s / n)
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (old, new, out, crop) = match &args[..] {
        [old, new, out] => (old, new, out, None),
        [old, new, out, rest @ ..] if rest.len() == 4 || rest.len() == 5 => {
            let n: Vec<f32> = rest
                .iter()
                .map(|v| v.parse::<f32>())
                .collect::<Result<_, _>>()
                .context("crop: x y w h [scale]")?;
            (old, new, out, Some(n))
        }
        _ => anyhow::bail!("usage: compare <old.png> <new.png> <out.png> [x y w h [scale]]"),
    };
    let mut old = image::open(old).with_context(|| old.clone())?.to_rgba8();
    let mut new = image::open(new).with_context(|| new.clone())?.to_rgba8();
    if new.height() != old.height() {
        let width = new.width() * old.height() / new.height();
        new = image::imageops::resize(&new, width, old.height(), FilterType::Triangle);
    }
    if let Some(c) = crop {
        let (x, y, w, h) = (c[0] as u32, c[1] as u32, c[2] as u32, c[3] as u32);
        let scale = c.get(4).copied().unwrap_or(1.0);
        let cut = |image: &RgbaImage| {
            let part = image::imageops::crop_imm(image, x, y, w, h).to_image();
            let (sw, sh) = ((w as f32 * scale) as u32, (h as f32 * scale) as u32);
            image::imageops::resize(&part, sw.max(1), sh.max(1), FilterType::Lanczos3)
        };
        old = cut(&old);
        new = cut(&new);
    }
    let mut both = RgbaImage::new(old.width() + new.width(), old.height());
    both.copy_from(&old, 0, 0)?;
    both.copy_from(&new, old.width(), 0)?;
    both.save(out).with_context(|| out.clone())?;
    let (a, b) = (mean(&old), mean(&new));
    println!(
        "{out}: mean old {:.0},{:.0},{:.0} new {:.0},{:.0},{:.0}",
        a[0], a[1], a[2], b[0], b[1], b[2]
    );
    Ok(())
}
