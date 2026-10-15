//! `release-build`: packages the prepared version for this platform, signs it with the
//! updater key, verifies it as installed copies will, and stages it with its notes under
//! `build/releases/<version>/<platform>/` for publishing.

use crate::{
    media,
    package::{self, check_tar_gz, create_tar_gz, digest_file, sign_package},
    repo::{self, Repo},
};
use anyhow::{Context, Result, anyhow, bail};
use canopy_release::{Artifact, PUBLIC_KEY, Version, feed::PLATFORMS, verify_package};
use std::{
    fs,
    path::{Path, PathBuf},
    process::Command,
};

/// The packaged app's name (`product-name` in game/Cargo.toml).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const PRODUCT: &str = "Under the Canopy";
/// The old desktop app's bundle identifier: an update replaces the app in place and keeps
/// its identity only if it is unchanged (D43).
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
const BUNDLE_ID: &str = "com.jiritrecak.settlers";

/// A built package, before staging.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
struct Built {
    path: PathBuf,
    /// The name Tauri gave this package; the signature's trusted comment names it.
    signed_name: String,
    /// The staged name's extension (`.app.tar.gz`, `.exe`).
    extension: &'static str,
}

pub fn cmd_build(repo: &Repo) -> Result<()> {
    let log = repo.read_log()?;
    let version = repo.app_version()?;
    let entry = log
        .release(version)
        .ok_or_else(|| {
            anyhow!("the app version is {version}, which the release log has no release for; prepare the release log before building")
        })?
        .clone();
    for release in log.history(version) {
        for image in release.changes.images.iter().flatten() {
            media::verify_image(&repo.media_dir(), image, false)?;
        }
    }
    // Fail before the long build when the key is missing or locked.
    let key = package::load_signing_key()?;
    let platform = canopy_release::host_platform();
    if !PLATFORMS.contains(&platform.as_str()) {
        bail!("{platform} is not a platform the feed knows");
    }
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.root.join("target"));
    let built = build_package(repo, &target.join("release"), version)?;

    println!("Signing {}…", built.signed_name);
    let signature = sign_package(
        &key,
        &built.path,
        &built.signed_name,
        &version.to_string(),
        repo::unix_now(),
    )?;
    let (sha256, bytes) = digest_file(&built.path)?;
    verify_package(
        PUBLIC_KEY,
        &signature,
        &version.to_string(),
        fs::File::open(&built.path)?,
        |_| {},
    )
    .context(
        "the signed package does not verify against the updater's public key; is this the release key?",
    )?;

    let filename = format!("under-the-canopy-{version}-{platform}{}", built.extension);
    let artifact = Artifact {
        schema_version: 1,
        version,
        platform: platform.clone(),
        filename: filename.clone(),
        sha256,
        bytes,
        signature: signature.clone(),
        changelog: entry.clone(),
    };
    artifact.validate().map_err(|e| anyhow!("artifact: {e}"))?;

    let out = repo
        .root
        .join("build/releases")
        .join(version.to_string())
        .join(&platform);
    if out.exists() {
        fs::remove_dir_all(&out).with_context(|| format!("clearing {}", out.display()))?;
    }
    fs::create_dir_all(&out)?;
    if let Some(images) = &entry.changes.images {
        fs::create_dir_all(out.join("media"))?;
        for image in images {
            fs::copy(
                repo.media_dir().join(&image.file),
                out.join("media").join(&image.file),
            )?;
        }
    }
    let staged = out.join(&filename);
    if fs::rename(&built.path, &staged).is_err() {
        fs::copy(&built.path, &staged)?;
        fs::remove_file(&built.path)?;
    }
    fs::write(
        out.join(format!("{filename}.sig")),
        format!("{signature}\n"),
    )?;
    fs::write(out.join("artifact.json"), json(&artifact))?;
    fs::write(out.join("changelog.json"), json(&entry))?;
    // Catch a mismatched or stale output before anything is published from it.
    if fs::metadata(&staged)?.len() != artifact.bytes {
        bail!("staged package size mismatch");
    }
    println!(
        "Verified release staged at {} ({} MiB).",
        out.join("artifact.json").display(),
        bytes / 1024 / 1024
    );
    Ok(())
}

/// Two-space JSON with a final newline (the old tool's layout).
pub fn json(value: &impl serde::Serialize) -> String {
    let mut s = serde_json::to_string_pretty(value).expect("serialises");
    s.push('\n');
    s
}

/// Runs the packaging `just package` runs.
#[cfg_attr(not(any(target_os = "macos", windows)), allow(dead_code))]
fn packager(repo: &Repo, format: &str) -> Result<()> {
    let status = Command::new("cargo")
        .args(["packager", "-p", "canopy", "--release", "--formats", format])
        .current_dir(&repo.root)
        .status()
        .context("running cargo packager (install it with `cargo install cargo-packager`)")?;
    if !status.success() {
        bail!("cargo packager failed");
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn build_package(repo: &Repo, release: &Path, version: Version) -> Result<Built> {
    let app = release.join(format!("{PRODUCT}.app"));
    if app.exists() {
        fs::remove_dir_all(&app).context("removing the previous app bundle")?;
    }
    packager(repo, "app")?;
    if !app.is_dir() {
        bail!("cargo packager made no {}", app.display());
    }
    let plist = app.join("Contents/Info.plist");
    let read = |key: &str| -> Result<String> {
        let out = Command::new("/usr/libexec/PlistBuddy")
            .args(["-c", &format!("Print :{key}")])
            .arg(&plist)
            .output()?;
        if !out.status.success() {
            bail!("Info.plist has no {key}");
        }
        Ok(String::from_utf8_lossy(&out.stdout).trim().to_owned())
    };
    let built_version = read("CFBundleShortVersionString")?;
    if built_version != version.to_string() {
        bail!("the built app is version {built_version}, the release is {version}");
    }
    let identifier = read("CFBundleIdentifier")?;
    if identifier != BUNDLE_ID {
        bail!(
            "the built app's bundle identifier is {identifier}; updates must keep the old app's {BUNDLE_ID} (D43)"
        );
    }
    let executable = read("CFBundleExecutable")?;
    // Seal the bundle as the old packages were: an ad-hoc signature over the whole bundle
    // (binding Info.plist and the resources), unless the packager signed it with an identity.
    let codesign = |args: &[&str]| -> Result<std::process::Output> {
        Ok(Command::new("codesign").args(args).arg(&app).output()?)
    };
    let shown = codesign(&["-dv"])?;
    if !shown.status.success() || String::from_utf8_lossy(&shown.stderr).contains("Signature=adhoc")
    {
        println!("Signing the bundle ad hoc…");
        let signed = codesign(&["--force", "--sign", "-"])?;
        if !signed.status.success() {
            bail!(
                "ad-hoc signing the bundle failed: {}",
                String::from_utf8_lossy(&signed.stderr).trim()
            );
        }
    }
    let verified = codesign(&["--verify", "--strict"])?;
    if !verified.status.success() {
        bail!(
            "the bundle's code signature does not verify: {}",
            String::from_utf8_lossy(&verified.stderr).trim()
        );
    }
    let signed_name = format!("{PRODUCT}.app.tar.gz");
    let path = release.join(&signed_name);
    println!("Archiving {}…", app.display());
    create_tar_gz(&app, &path)?;
    let entries = check_tar_gz(
        &path,
        &format!("{PRODUCT}.app"),
        &format!("Contents/MacOS/{executable}"),
    )?;
    println!("Packaged {entries} entries.");
    Ok(Built {
        path,
        signed_name,
        extension: ".app.tar.gz",
    })
}

/// The NSIS installer is the Windows update package (as before). Untested so far: the
/// first releases are macOS only.
#[cfg(windows)]
fn build_package(repo: &Repo, release: &Path, _version: Version) -> Result<Built> {
    let installers = || -> Result<Vec<PathBuf>> {
        Ok(fs::read_dir(release)?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.to_string_lossy().ends_with("-setup.exe"))
            .collect())
    };
    for stale in installers()? {
        fs::remove_file(stale)?;
    }
    packager(repo, "nsis")?;
    let found = installers()?;
    let [path] = found.as_slice() else {
        bail!("expected one installer in {}", release.display());
    };
    let signed_name = path
        .file_name()
        .expect("a file")
        .to_string_lossy()
        .into_owned();
    Ok(Built {
        path: path.clone(),
        signed_name,
        extension: ".exe",
    })
}

#[cfg(not(any(target_os = "macos", windows)))]
fn build_package(_repo: &Repo, _release: &Path, _version: Version) -> Result<Built> {
    bail!("release builds are made on macOS (and Windows); this platform has no package format")
}
