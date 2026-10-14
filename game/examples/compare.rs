//! Side-by-side review images: `cargo run --release -p canopy --example compare -- <old.png>
//! <new.png> <out.png>` writes old (left) and new (right) next to each other, the new one
//! scaled to the old one's height, and prints both pictures' mean colours.

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
    let [old, new, out] = &args[..] else {
        anyhow::bail!("usage: compare <old.png> <new.png> <out.png>");
    };
    let old = image::open(old).with_context(|| old.clone())?.to_rgba8();
    let mut new = image::open(new).with_context(|| new.clone())?.to_rgba8();
    if new.height() != old.height() {
        let width = new.width() * old.height() / new.height();
        new = image::imageops::resize(&new, width, old.height(), FilterType::Triangle);
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
