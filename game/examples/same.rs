//! Are two sets of captures the same? `cargo run --release -p canopy --example same --
//! <before folder> <after folder>` compares every PNG of the first folder with the one of
//! the same name in the second and reports identical, or how many pixels differ and by how
//! much (largest channel difference). Exits with failure if any differ or are missing.

use anyhow::{Context, Result};
use std::{path::Path, process::ExitCode};

fn compare(a: &Path, b: &Path) -> Result<Option<(u64, u8)>> {
    let a = image::open(a)
        .with_context(|| a.display().to_string())?
        .to_rgba8();
    let b = image::open(b)
        .with_context(|| b.display().to_string())?
        .to_rgba8();
    if a.dimensions() != b.dimensions() {
        anyhow::bail!("sizes differ: {:?} vs {:?}", a.dimensions(), b.dimensions());
    }
    let mut differing = 0;
    let mut largest = 0;
    for (p, q) in a.pixels().zip(b.pixels()) {
        let delta =
            p.0.iter()
                .zip(q.0)
                .map(|(x, y)| x.abs_diff(y))
                .max()
                .unwrap_or(0);
        if delta > 0 {
            differing += 1;
            largest = largest.max(delta);
        }
    }
    Ok((differing > 0).then_some((differing, largest)))
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
                Some((n, largest)) => {
                    all_same = false;
                    format!("{n} pixels differ (largest difference {largest})")
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
