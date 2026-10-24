//! Runs combat lab scenarios headless and prints what they measured (`just lab`;
//! docs/editor.md, "The Combat Lab"): every scenario in `labs/`, or those named.
//!
//! ```text
//! cargo run --release -p canopy-sim --example lab -- --data . [scenario …] [--json]
//!     [--approach spots|direct|ring] [--lead] [--facts]
//!
//! `--facts` prints the game's messages as they happen and where each group ends.
//! ```
#![allow(clippy::disallowed_methods)] // wall-clock timing of the run itself

use canopy_content::{Content, lab};
use canopy_sim::lab::Run;
use std::{path::PathBuf, sync::Arc, time::Instant};

fn main() {
    if let Err(e) = run() {
        eprintln!("lab: {e}");
        std::process::exit(1);
    }
}

fn run() -> Result<(), String> {
    let mut data = PathBuf::from(".");
    let mut names = Vec::new();
    let mut json = false;
    let mut facts = false;
    let mut tuning = canopy_sim::movement::tuning::Tuning::default();
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => data = args.next().ok_or("--data needs a folder")?.into(),
            "--json" => json = true,
            "--facts" => facts = true,
            "--lead" => tuning.lead_fleeing = true,
            "--approach" => {
                use canopy_sim::movement::tuning::Approach;
                tuning.approach = match args.next().as_deref() {
                    Some("spots") => Approach::Spots,
                    Some("direct") => Approach::Direct,
                    Some("ring") => Approach::Ring,
                    other => return Err(format!("--approach {other:?}: spots, direct or ring")),
                };
            }
            other => names.push(other.to_string()),
        }
    }
    let content = Arc::new(Content::load(&data).map_err(|e| format!("content: {e}"))?);
    let scenarios: Vec<_> = lab::library(&data)
        .into_iter()
        .filter(|(id, _)| names.is_empty() || names.contains(id))
        .collect();
    if scenarios.is_empty() {
        return Err(format!(
            "no scenarios in {}",
            data.join(lab::LABS_FOLDER).display()
        ));
    }
    let mut failed = false;
    for (id, path) in scenarios {
        let scenario = lab::read(&path)?;
        let problems = scenario.problems(&content);
        if !problems.is_empty() {
            println!("{id}: {}", problems.join("; "));
            failed = true;
            continue;
        }
        let mut run = match Run::tuned(content.clone(), &scenario, tuning) {
            Ok(run) => run,
            Err(e) => {
                println!("{id}: {}", e.0);
                failed = true;
                continue;
            }
        };
        let (mut total, mut worst) = (0.0f64, 0.0f64);
        let mut seen = 0;
        while !run.finished() {
            let started = Instant::now();
            run.step();
            if facts {
                let from = seen;
                for f in run.world.state().facts.iter().filter(|f| f.id >= from) {
                    println!(
                        "  [{:>5}] {:?} {:?}: {}",
                        f.tick, f.owner, f.kind, f.message
                    );
                    seen = seen.max(f.id + 1);
                }
            }
            let ms = started.elapsed().as_secs_f64() * 1000.0;
            total += ms;
            worst = worst.max(ms);
        }
        if facts {
            for g in &run.measures().groups {
                let xs: Vec<i32> = run
                    .group(&g.name)
                    .iter()
                    .filter_map(|&id| run.world.state().entities.get(id))
                    .map(|e| e.position.x / 1000)
                    .collect();
                println!("  {} ends at x {:?}", g.name, xs);
            }
        }
        let m = run.measures();
        if json {
            println!(
                "{}",
                serde_json::json!({"scenario": id, "measures": m, "msMean": total / f64::from(m.tick.max(1)), "msWorst": worst})
            );
            continue;
        }
        println!(
            "{id} — {} ({:.0} s): step {:.2} ms mean, {:.2} ms worst; worst overlap {:.2} cells",
            scenario.name,
            f64::from(m.tick) / 40.0,
            total / f64::from(m.tick.max(1)),
            worst,
            m.worst_overlap
        );
        let seconds =
            |t: Option<u32>| t.map_or("—".into(), |t| format!("{:.1}s", f64::from(t) / 40.0));
        for g in &m.groups {
            println!(
                "  {:<12} {:>3}/{:<3} alive  arrived {:>3} (first {}, last {})  held up {:.1} unit-s (longest {:.1}s)  flips {}  damage taken {}  first swing {}  most in reach {}",
                g.name,
                g.alive,
                g.units,
                g.arrived,
                seconds(g.first_arrival),
                seconds(g.last_arrival),
                g.stuck_ticks as f64 / 40.0,
                f64::from(g.longest_stuck) / 40.0,
                g.flips,
                g.damage_taken,
                seconds(g.first_swing),
                g.most_in_reach
            );
        }
    }
    if failed {
        return Err("some scenarios did not run".into());
    }
    Ok(())
}
