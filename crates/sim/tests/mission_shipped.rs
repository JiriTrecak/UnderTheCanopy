//! Every shipped campaign mission passes the mission check (docs/campaign.md "Checking a
//! mission"): no stranded unit, no wave region in a building or on closed ground, every
//! region and wave target open to the real pathfinder, no scripted crate walling a
//! corridor, and every scene playing skipped without a script error and ending with
//! cinematic mode off. No test here names a shipped map.
//!
//! `cargo test -p canopy-sim --test mission_shipped -- --nocapture` prints each mission's
//! routes with their narrowest width, and what is flagged.
#![allow(clippy::disallowed_methods)] // wall-clock timing of the check, printed only

use canopy_content::{BASE_DIR, Content, assets::Files, campaign, map, package::Package};
use canopy_sim::mission::check::{self, Report};
use std::{fmt::Write, path::Path, sync::Arc, time::Instant};

fn data() -> &'static Path {
    Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."))
}

/// What a mission's check prints.
fn summary(name: &str, report: &Report, seconds: f64) -> String {
    let mut out = format!(
        "{name}: {} scenes, {} routes, {} findings ({seconds:.1} s)\n",
        report.scenes.len(),
        report.routes.len(),
        report.findings.len(),
    );
    for route in &report.routes {
        let width = route
            .width
            .map_or("no ground".into(), |(w, (x, y))| format!("{w} at {x},{y}"));
        let state = match (route.reached, route.large, route.walled) {
            (false, _, _) => "NO WAY",
            (true, false, _) => "narrow",
            (true, true, true) => "walled",
            (true, true, false) => "open",
        };
        let _ = writeln!(
            out,
            "  {:<52} -> {:<22} r {:<4} {state:<7} narrowest {width}",
            route.from, route.to, route.radius
        );
    }
    for (id, width) in &report.spawns {
        let _ = writeln!(
            out,
            "  scripted `{id}` spawns in a corridor {width} cells wide"
        );
    }
    for warning in &report.warnings {
        let _ = writeln!(out, "  flagged: {warning}");
    }
    out
}

#[test]
fn shipped_missions_pass_the_mission_check() {
    let base = Arc::new(Content::load(data()).unwrap_or_else(|errors| panic!("{errors}")));
    let mut missions = Vec::new();
    for package in campaign::library(data()).unwrap_or_else(|errors| panic!("{errors}")) {
        for file in &package.missions {
            let map = map::package::load(&file.path).unwrap_or_else(|errors| panic!("{errors}"));
            let own = Package::open(&file.path).unwrap_or_else(|e| panic!("{e}"));
            // A campaign or map that changes game data plays on it, layered over the base.
            let content = if package.has_content() || campaign::has_content(&own) {
                let base = Package::open(&data().join(BASE_DIR)).unwrap_or_else(|e| panic!("{e}"));
                Arc::new(
                    Content::from_packages(&[base, package.package.clone(), own], Files::Skip)
                        .unwrap_or_else(|errors| panic!("{errors}")),
                )
            } else {
                base.clone()
            };
            missions.push((format!("{}/{}", package.id(), file.id), map, content));
        }
    }
    // One mission a thread: each check is independent.
    let reports: Vec<(String, Report)> = std::thread::scope(|scope| {
        let runs: Vec<_> = missions
            .iter()
            .map(|(name, map, content)| {
                scope.spawn(move || {
                    let started = Instant::now();
                    let report = check::check(content, map, name);
                    let text = summary(name, &report, started.elapsed().as_secs_f64());
                    (text, report)
                })
            })
            .collect();
        runs.into_iter()
            .map(|run| run.join().expect("the check runs"))
            .collect()
    });
    let mut findings = Vec::new();
    for (text, report) in reports {
        print!("{text}");
        findings.extend(report.findings);
    }
    assert!(
        findings.is_empty(),
        "The mission check found:\n{}",
        findings.join("\n")
    );
}
