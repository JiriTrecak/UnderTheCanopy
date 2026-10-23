//! `canopy-release`: the release tool (D43, docs/releases.md). Run from the repository root,
//! normally through `just release-note`, `release-check`, `release-prepare`,
//! `release-build` and `release-publish`.

mod installer;
mod media;
mod notes;
mod package;
mod publish;
mod repo;
mod stage;

use anyhow::{Result, anyhow, bail};
use std::{collections::BTreeMap, path::PathBuf, process::ExitCode};

const USAGE: &str = "\
usage: canopy-release <command>
  note --kind added|improved|fixed|knownIssues --text \"Player-facing change\"
  check [--allow-lfs-pointers]
  prepare VERSION --title \"Short release title\" [--milestone]
  build
  publish --artifact build/releases/VERSION/PLATFORM/artifact.json [--apply] [--profile NAME]
  installer --version VERSION [--apply] [--profile NAME]";

/// Options that take a value, and flags.
const VALUES: [&str; 6] = ["kind", "text", "title", "artifact", "profile", "version"];
const FLAGS: [&str; 3] = ["milestone", "apply", "allow-lfs-pointers"];

struct Args {
    positionals: Vec<String>,
    values: BTreeMap<String, String>,
    flags: Vec<String>,
}

impl Args {
    fn parse(raw: impl IntoIterator<Item = String>) -> Result<Self> {
        let mut args = Args {
            positionals: Vec::new(),
            values: BTreeMap::new(),
            flags: Vec::new(),
        };
        let mut raw = raw.into_iter();
        while let Some(arg) = raw.next() {
            if let Some(name) = arg.strip_prefix("--") {
                let (name, inline) = match name.split_once('=') {
                    Some((n, v)) => (n, Some(v.to_owned())),
                    None => (name, None),
                };
                if VALUES.contains(&name) {
                    let value = match inline {
                        Some(v) => v,
                        None => raw
                            .next()
                            .ok_or_else(|| anyhow!("--{name} needs a value"))?,
                    };
                    args.values.insert(name.to_owned(), value);
                } else if FLAGS.contains(&name) && inline.is_none() {
                    args.flags.push(name.to_owned());
                } else {
                    bail!("unknown option --{name}\n{USAGE}");
                }
            } else {
                args.positionals.push(arg);
            }
        }
        Ok(args)
    }

    fn value(&self, name: &str) -> Option<&str> {
        self.values.get(name).map(String::as_str)
    }

    fn flag(&self, name: &str) -> bool {
        self.flags.iter().any(|f| f == name)
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("release: {e:#}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<()> {
    let args = Args::parse(std::env::args().skip(1))?;
    let Some(command) = args.positionals.first() else {
        bail!("{USAGE}");
    };
    let repo = repo::Repo::open(std::env::current_dir()?)?;
    match command.as_str() {
        "note" => notes::cmd_note(
            &repo,
            args.value("kind").unwrap_or(""),
            args.value("text").unwrap_or(""),
        ),
        "check" => notes::cmd_check(&repo, args.flag("allow-lfs-pointers")),
        "prepare" => {
            let version = args
                .positionals
                .get(1)
                .ok_or_else(|| anyhow!("use prepare VERSION --title \"…\" [--milestone]"))?;
            notes::cmd_prepare(
                &repo,
                version,
                args.value("title").unwrap_or(""),
                args.flag("milestone"),
            )
        }
        "build" => stage::cmd_build(&repo),
        "publish" => cmd_publish(&repo, &args),
        "installer" => installer::cmd_installer(
            &repo,
            args.value("version")
                .ok_or_else(|| anyhow!("use installer --version VERSION [--apply]"))?,
            args.flag("apply"),
            args.value("profile"),
        ),
        other => bail!("unknown command {other:?}\n{USAGE}"),
    }
}

fn cmd_publish(repo: &repo::Repo, args: &Args) -> Result<()> {
    let artifact = args.value("artifact").ok_or_else(|| {
        anyhow!("use publish --artifact build/releases/VERSION/PLATFORM/artifact.json [--apply]")
    })?;
    let deployment = publish::Deployment::load(repo)?;
    let log = repo.read_log()?;
    let staged = publish::verify_staged(
        &repo.root.join(PathBuf::from(artifact)),
        &log,
        canopy_release::PUBLIC_KEY,
    )?;
    let aws = publish::AwsCli {
        region: deployment.region.clone(),
        profile: args.value("profile").map(str::to_owned),
    };
    publish::check_identity(&aws, &deployment)?;
    let a = &staged.artifact;
    println!(
        "Verified {} for {} ({} MiB).",
        a.version,
        a.platform,
        a.bytes / 1024 / 1024
    );
    if let Some(installer) = &a.installer {
        println!(
            "Verified the disk image {} ({} MiB).",
            installer.filename,
            installer.bytes / 1024 / 1024
        );
    }
    if !args.flag("apply") {
        println!("No uploads performed. Add --apply to publish.");
        return Ok(());
    }
    let cdn = publish::HttpCdn::new()?;
    publish::Publisher::new(&aws, &cdn, &deployment).publish(&staged)
}
