//! Are two sets of captures the same? `cargo run --release -p canopy --example same --
//! <before folder> <after folder>` compares every PNG of the first folder with the one of
//! the same name in the second and reports identical, or how many pixels differ and by how
//! much: the largest and the mean channel difference, and the share of pixels whose
//! largest channel difference exceeds 2/255. Exits with failure if any differ or are
//! missing. With `SAME_DIFF=<folder>` it also writes each before image there, dimmed,
//! with the pixels differing by more than 2/255 in magenta.

use anyhow::{Context, Result};
use std::{path::Path, process::ExitCode};

/// How two captures differ: pixels that differ, the largest channel difference, the mean
/// channel difference over every channel, and pixels differing by more than 2/255.
struct Difference {
    differing: u64,
    largest: u8,
    mean: f64,
    above_two: u64,
    pixels: u64,
}

fn compare(a: &Path, b: &Path) -> Result<Option<Difference>> {
    let name = a.file_name().map(|n| n.to_owned()).unwrap_or_default();
    let a = image::open(a)
        .with_context(|| a.display().to_string())?
        .to_rgba8();
    let b = image::open(b)
        .with_context(|| b.display().to_string())?
        .to_rgba8();
    if a.dimensions() != b.dimensions() {
        anyhow::bail!("sizes differ: {:?} vs {:?}", a.dimensions(), b.dimensions());
    }
    if let Ok(dir) = std::env::var("SAME_DIFF") {
        let mut out = a.clone();
        let (mut lo, mut hi) = ([u32::MAX; 2], [0u32; 2]);
        for (x, y, p) in out.enumerate_pixels_mut() {
            let q = b.get_pixel(x, y);
            let d =
                p.0.iter()
                    .zip(q.0)
                    .map(|(x, y)| x.abs_diff(y))
                    .max()
                    .unwrap_or(0);
            if d > 2 {
                *p = image::Rgba([255, 0, 255, 255]);
                lo = [lo[0].min(x), lo[1].min(y)];
                hi = [hi[0].max(x), hi[1].max(y)];
            } else {
                p.0[0] /= 3;
                p.0[1] /= 3;
                p.0[2] /= 3;
            }
        }
        eprintln!("diff box {lo:?}..{hi:?}");
        out.save(Path::new(&dir).join(&name))?;
    }
    let (mut differing, mut largest, mut sum, mut above_two) = (0, 0, 0u64, 0);
    for (p, q) in a.pixels().zip(b.pixels()) {
        let mut delta = 0;
        for (x, y) in p.0.iter().zip(q.0) {
            let d = x.abs_diff(y);
            sum += u64::from(d);
            delta = delta.max(d);
        }
        if delta > 0 {
            differing += 1;
            largest = largest.max(delta);
        }
        if delta > 2 {
            above_two += 1;
        }
    }
    let pixels = u64::from(a.width()) * u64::from(a.height());
    Ok((differing > 0).then_some(Difference {
        differing,
        largest,
        mean: sum as f64 / (pixels * 4) as f64,
        above_two,
        pixels,
    }))
}

fn main() -> Result<ExitCode> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [before, after] = args.as_slice() else {
        anyhow::bail!("usage: same <before folder> <after folder>");
    };
    let mut names: Vec<String> = std::fs::read_dir(before)?
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".png"))
        .collect();
    names.sort();
    let mut all_same = true;
    for name in names {
        let b = Path::new(after).join(&name);
        let verdict = if !b.exists() {
            all_same = false;
            "missing".to_string()
        } else {
            match compare(&Path::new(before).join(&name), &b)? {
                None => "identical".to_string(),
                Some(d) => {
                    all_same = false;
                    format!(
                        "{} pixels differ (largest difference {}, mean {:.4}, {:.4} % above 2)",
                        d.differing,
                        d.largest,
                        d.mean,
                        100.0 * d.above_two as f64 / d.pixels as f64
                    )
                }
            }
        };
        println!("{name}: {verdict}");
    }
    Ok(if all_same {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}
