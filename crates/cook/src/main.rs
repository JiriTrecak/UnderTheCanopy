//! `canopy-cook`: cooks the published data into `target/cooked/canopy.pack`.
//!
//! ```text
//! canopy-cook [--data <folder>] [--out <folder>] [--force] [--allow-lfs-pointers] [--verbose]
//! canopy-cook --inspect <entry, asset id or scenery id> [--out <folder>]
//! canopy-cook --stats [--out <folder>]
//! ```
//!
//! Without `--data`, the data folder is the nearest ancestor of the working directory that
//! holds `content/` and `assets/`; without `--out` it is `<data>/target/cooked`.

// The prebuilt ISPC kernels lack a macOS platform load command; the linker notes it.
#![allow(linker_messages)]

use anyhow::{Context, Result, bail};
use canopy_cook::{Options, PACK_FILE, diag::Severity};
use std::{path::PathBuf, process::ExitCode};

enum Command {
    Cook,
    Inspect(String),
    Stats,
}

struct Args {
    command: Command,
    data: Option<PathBuf>,
    out: Option<PathBuf>,
    force: bool,
    allow_lfs_pointers: bool,
    verbose: bool,
}

fn parse(mut args: impl Iterator<Item = String>) -> Result<Args> {
    let mut parsed = Args {
        command: Command::Cook,
        data: None,
        out: None,
        force: false,
        allow_lfs_pointers: false,
        verbose: false,
    };
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().with_context(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--data" => parsed.data = Some(value("--data")?.into()),
            "--out" => parsed.out = Some(value("--out")?.into()),
            "--inspect" => parsed.command = Command::Inspect(value("--inspect")?),
            "--stats" => parsed.command = Command::Stats,
            "--force" => parsed.force = true,
            "--allow-lfs-pointers" => parsed.allow_lfs_pointers = true,
            "--verbose" | "-v" => parsed.verbose = true,
            other => bail!(
                "unknown option {other} (expected --data, --out, --force, --allow-lfs-pointers, --verbose, --inspect <id>, --stats)"
            ),
        }
    }
    Ok(parsed)
}

fn main() -> ExitCode {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "warn".into()),
        )
        .init();
    match run() {
        Ok(code) => code,
        Err(error) => {
            eprintln!("cook: {error:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode> {
    let args = parse(std::env::args().skip(1))?;
    let find_data = || -> Result<PathBuf> {
        match &args.data {
            Some(data) => Ok(data.clone()),
            None => canopy_cook::find_data(&std::env::current_dir()?)
                .context("no data folder here or above (pass --data <folder>)"),
        }
    };
    let out = match &args.out {
        Some(out) => out.clone(),
        None => find_data()?.join("target/cooked"),
    };
    match args.command {
        Command::Inspect(query) => {
            print!("{}", canopy_cook::inspect(&out.join(PACK_FILE), &query)?);
            return Ok(ExitCode::SUCCESS);
        }
        Command::Stats => {
            print!("{}", canopy_cook::stats(&out.join(PACK_FILE))?);
            return Ok(ExitCode::SUCCESS);
        }
        Command::Cook => {}
    }
    let report = canopy_cook::cook(&Options {
        data: find_data()?,
        out,
        force: args.force,
        allow_lfs_pointers: args.allow_lfs_pointers,
    })?;
    let warnings = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Warning)
        .count();
    // Warnings are listed when their asset was cooked this run, or with --verbose.
    let fresh = report.cooked > 0 && !report.up_to_date;
    for diagnostic in &report.diagnostics {
        if diagnostic.severity == Severity::Error || args.verbose || fresh {
            eprintln!("{diagnostic}");
        }
    }
    if args.verbose {
        for (path, reason) in &report.skipped {
            eprintln!("skipped: {path}: {reason}");
        }
    }
    if report.errors() > 0 {
        eprintln!(
            "cook: {} errors in the inputs; the pack was not written",
            report.errors()
        );
        return Ok(ExitCode::FAILURE);
    }
    let t = &report.timings;
    if report.up_to_date {
        eprintln!(
            "cook: up to date ({} entries, {:.1} MiB; {warnings} warnings, --verbose lists them) in {:.2} s",
            report.entries,
            report.pack_bytes as f64 / 1048576.0,
            t.plan + t.hash
        );
    } else {
        eprintln!(
            "cook: {} of {} jobs cooked, {} entries, {:.1} MiB from {:.1} MiB of sources ({} outputs skipped, {warnings} warnings) in {:.2} s (plan {:.2}, hash {:.2}, cook {:.2}, link {:.2})",
            report.cooked,
            report.jobs,
            report.entries,
            report.pack_bytes as f64 / 1048576.0,
            report.source_bytes as f64 / 1048576.0,
            report.skipped.len(),
            t.plan + t.hash + t.cook + t.link,
            t.plan,
            t.hash,
            t.cook,
            t.link
        );
    }
    Ok(ExitCode::SUCCESS)
}
