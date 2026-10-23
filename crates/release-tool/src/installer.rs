//! First-install disk images (`under-the-canopy-<version>-<platform>.dmg`). Friends install
//! the game from a familiar DMG: the app beside an `Applications` link to drag it onto. The
//! `.app.tar.gz` stays the update package; the DMG is made from that package, unpacked, so
//! first installs get exactly the app updaters get.
//!
//! `release-build` makes it beside the package; `canopy-release installer --version X`
//! backfills it for a release published without one.

use crate::{
    package::digest_file,
    publish::{self, Deployment, verify_staged},
    repo::{self, Repo},
    stage::json,
};
use anyhow::{Context, Result, anyhow, bail};
use canopy_release::{Installer, PUBLIC_KEY, Version};
use std::{
    ffi::OsString,
    fs,
    path::{Path, PathBuf},
};

/// The mounted volume's name.
pub const VOLUME: &str = "Under the Canopy";
/// LZFSE-compressed and read-only (`ULFO`, macOS 10.11 and later). For the 0.4.2 app it
/// came out 4% smaller than zlib (`UDZO`) and twice as fast to make; zlib at level 9 is
/// only as small and three times slower.
pub const FORMAT: &str = "ULFO";
/// The disk image's MIME type, as browsers expect it.
pub const CONTENT_TYPE: &str = "application/x-apple-diskimage";

/// `under-the-canopy-<version>-<platform>.dmg`.
pub fn filename(version: Version, platform: &str) -> String {
    format!("under-the-canopy-{version}-{platform}.dmg")
}

/// Its public URL, beside the package.
pub fn url(base_url: &str, version: Version, filename: &str) -> String {
    format!("{base_url}/releases/{version}/{filename}")
}

/// Lays out the disk image's contents in `folder` (which must not exist): the app unpacked
/// from `package` exactly as the updater unpacks it (modes, times and symlinks kept), and an
/// `Applications` link. The package must hold `app_name` and nothing else. Returns the app.
pub fn stage_folder(package: &Path, app_name: &str, folder: &Path) -> Result<PathBuf> {
    if folder.exists() {
        bail!("{} already exists", folder.display());
    }
    fs::create_dir_all(folder)?;
    let file = fs::File::open(package).with_context(|| format!("opening {}", package.display()))?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    archive.set_preserve_permissions(true);
    archive.set_preserve_mtime(true);
    archive
        .unpack(folder)
        .with_context(|| format!("unpacking {}", package.display()))?;
    let mut entries: Vec<_> = fs::read_dir(folder)?
        .map(|e| e.map(|e| e.file_name()))
        .collect::<Result<_, _>>()?;
    entries.sort();
    if entries != [OsString::from(app_name)] {
        bail!("the package must hold only {app_name}, not {entries:?}");
    }
    let app = folder.join(app_name);
    if !app.is_dir() {
        bail!("{app_name} in the package is not a bundle folder");
    }
    applications_link(&folder.join("Applications"))?;
    Ok(app)
}

#[cfg(unix)]
fn applications_link(at: &Path) -> Result<()> {
    std::os::unix::fs::symlink("/Applications", at).context("linking Applications")
}

#[cfg(not(unix))]
fn applications_link(_at: &Path) -> Result<()> {
    bail!("disk images are made on macOS")
}

/// `hdiutil` arguments that make the compressed image of `folder` at `out`.
pub fn create_args(folder: &Path, out: &Path) -> Vec<OsString> {
    let mut args: Vec<OsString> = [
        "create",
        "-quiet",
        "-volname",
        VOLUME,
        "-fs",
        "HFS+",
        "-format",
        FORMAT,
        "-ov",
        "-srcfolder",
    ]
    .map(OsString::from)
    .into();
    args.push(folder.into());
    args.push(out.into());
    args
}

/// What a mounted disk image must show: the app and the `Applications` link, nothing else
/// visible. `listing` is the volume's top level (hidden system folders included).
pub fn check_layout(listing: &[(String, Option<PathBuf>)], app_name: &str) -> Result<()> {
    let visible: Vec<_> = listing
        .iter()
        .filter(|(name, _)| !name.starts_with('.'))
        .collect();
    let app = visible
        .iter()
        .any(|(n, link)| n == app_name && link.is_none());
    let link = visible.iter().any(|(n, link)| {
        n == "Applications" && link.as_deref() == Some(Path::new("/Applications"))
    });
    if !app || !link || visible.len() != 2 {
        bail!("the disk image shows {visible:?}; expected {app_name} and an Applications link");
    }
    Ok(())
}

/// Makes the disk image `out` from the update package `package` (the app `release-build`
/// sealed and archived), checks it mounts with the app and the link, and returns its hash
/// and size.
#[cfg(target_os = "macos")]
pub fn make(package: &Path, out: &Path, version: Version) -> Result<(String, u64)> {
    use crate::stage::{PRODUCT, check_bundle, codesign_verify};
    use std::process::Command;

    let app_name = format!("{PRODUCT}.app");
    let work = out.with_extension("dmg-contents");
    if work.exists() {
        fs::remove_dir_all(&work)?;
    }
    if out.exists() {
        fs::remove_file(out)?;
    }
    let made = (|| -> Result<String> {
        println!("Unpacking the update package for the disk image…");
        let app = stage_folder(package, &app_name, &work)?;
        // The unpacked app must be the sealed bundle, byte for byte.
        let executable = check_bundle(&app, version)?;
        codesign_verify(&app)?;
        println!("Making {}…", out.display());
        let made = Command::new("hdiutil")
            .args(create_args(&work, out))
            .output()
            .context("running hdiutil")?;
        if !made.status.success() {
            bail!(
                "hdiutil create failed: {}",
                String::from_utf8_lossy(&made.stderr).trim()
            );
        }
        Ok(executable)
    })();
    let _ = fs::remove_dir_all(&work);
    let checked = made.and_then(|executable| {
        // Ad hoc, as the app: there is no Developer ID yet.
        let signed = Command::new("codesign")
            .args(["--force", "--sign", "-"])
            .arg(out)
            .output()?;
        if !signed.status.success() {
            bail!(
                "ad-hoc signing the disk image failed: {}",
                String::from_utf8_lossy(&signed.stderr).trim()
            );
        }
        codesign_verify(out)?;
        mount_check(out, &app_name, &executable)?;
        digest_file(out)
    });
    if checked.is_err() {
        // Never leave a disk image that failed its checks beside the package.
        let _ = fs::remove_file(out);
    }
    checked
}

#[cfg(not(target_os = "macos"))]
pub fn make(_package: &Path, _out: &Path, _version: Version) -> Result<(String, u64)> {
    bail!("disk images are made on macOS")
}

/// Attaches `dmg` read-only (hidden from Finder), checks the layout, the app's executable
/// and its code signature, and detaches it again in every case.
#[cfg(target_os = "macos")]
pub fn mount_check(dmg: &Path, app_name: &str, executable: &str) -> Result<()> {
    use crate::stage::codesign_verify;
    use std::{os::unix::fs::PermissionsExt, process::Command};

    let mount = std::env::temp_dir().join(format!("canopy-dmg-{}", std::process::id()));
    let _ = fs::remove_dir(&mount);
    fs::create_dir_all(&mount)?;
    let attached = Command::new("hdiutil")
        .args([
            "attach",
            "-readonly",
            "-nobrowse",
            "-noautoopen",
            "-mountpoint",
        ])
        .arg(&mount)
        .arg(dmg)
        .output()
        .context("running hdiutil attach")?;
    if !attached.status.success() {
        let _ = fs::remove_dir(&mount);
        bail!(
            "the disk image does not attach: {}",
            String::from_utf8_lossy(&attached.stderr).trim()
        );
    }
    let checked = (|| -> Result<()> {
        let mut listing = Vec::new();
        for entry in fs::read_dir(&mount)? {
            let entry = entry?;
            let link = entry
                .file_type()?
                .is_symlink()
                .then(|| fs::read_link(entry.path()))
                .transpose()?;
            listing.push((entry.file_name().to_string_lossy().into_owned(), link));
        }
        check_layout(&listing, app_name)?;
        let app = mount.join(app_name);
        let binary = app.join("Contents/MacOS").join(executable);
        if fs::metadata(&binary)?.permissions().mode() & 0o111 == 0 {
            bail!("{} is not executable in the disk image", binary.display());
        }
        codesign_verify(&app)
    })();
    let mut detached = false;
    for force in [false, true] {
        let mut command = Command::new("hdiutil");
        command.arg("detach").arg(&mount).arg("-quiet");
        if force {
            command.arg("-force");
        }
        if command.status().is_ok_and(|s| s.success()) {
            detached = true;
            break;
        }
    }
    let _ = fs::remove_dir(&mount);
    checked.context("the mounted disk image")?;
    if !detached {
        bail!("could not detach {}", mount.display());
    }
    println!(
        "Mounted {} read-only: {app_name} and the Applications link are there, the app verifies.",
        dmg.display()
    );
    Ok(())
}

/// `canopy-release installer --version X [--apply]`: adds the disk image to a release
/// already published without one. The staged package is verified (hash and signature, as
/// publishing does) and unpacked into the image; with `--apply` only the image is uploaded
/// and the version's `<platform>.json` rewritten to add `installer`. The package, its
/// signature and the feed never change.
pub fn cmd_installer(repo: &Repo, version: &str, apply: bool, profile: Option<&str>) -> Result<()> {
    let version: Version = version.parse()?;
    let platform = canopy_release::host_platform();
    let dir = repo
        .root
        .join("build/releases")
        .join(version.to_string())
        .join(&platform);
    let artifact_path = dir.join("artifact.json");
    if !artifact_path.is_file() {
        bail!(
            "no staged release at {}; the backfill needs the published build",
            artifact_path.display()
        );
    }
    let deployment = Deployment::load(repo)?;
    let log = repo.read_log()?;
    println!("Verifying the staged package…");
    let mut staged = verify_staged(&artifact_path, &log, PUBLIC_KEY)?;
    match &staged.artifact.installer {
        Some(installer) => println!(
            "The staged disk image {} verified ({} MiB).",
            installer.filename,
            installer.bytes / 1024 / 1024
        ),
        None => {
            let name = filename(version, &platform);
            let (sha256, bytes) = make(&staged.package, &dir.join(&name), version)?;
            let mut artifact = staged.artifact.clone();
            artifact.installer = Some(Installer {
                url: url(&deployment.base_url, version, &name),
                filename: name,
                sha256,
                bytes,
            });
            artifact.validate().map_err(|e| anyhow!("artifact: {e}"))?;
            repo::write_atomic(&artifact_path, json(&artifact).as_bytes())?;
            staged = verify_staged(&artifact_path, &log, PUBLIC_KEY)?;
            println!(
                "Staged {} ({} MiB) in {}.",
                artifact.installer.as_ref().expect("set").filename,
                bytes / 1024 / 1024,
                artifact_path.display()
            );
        }
    }
    if !apply {
        println!(
            "No uploads performed. Add --apply to upload the disk image and add it to releases/{version}/{platform}.json."
        );
        return Ok(());
    }
    let aws = publish::AwsCli {
        region: deployment.region.clone(),
        profile: profile.map(str::to_owned),
    };
    publish::check_identity(&aws, &deployment)?;
    let cdn = publish::HttpCdn::new()?;
    publish::Publisher::new(&aws, &cdn, &deployment).add_installer(&staged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn the_image_holds_the_unpacked_app_and_an_applications_link() {
        use std::os::unix::fs::PermissionsExt;
        let dir = std::env::temp_dir().join(format!("canopy-installer-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        let app = dir.join("Game.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::write(app.join("Contents/MacOS/game"), b"binary").unwrap();
        fs::set_permissions(
            app.join("Contents/MacOS/game"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        std::os::unix::fs::symlink("MacOS/game", app.join("Contents/link")).unwrap();
        let package = dir.join("Game.app.tar.gz");
        crate::package::create_tar_gz(&app, &package).unwrap();

        let folder = dir.join("image");
        let unpacked = stage_folder(&package, "Game.app", &folder).unwrap();
        assert_eq!(unpacked, folder.join("Game.app"));
        assert_eq!(
            fs::read(unpacked.join("Contents/MacOS/game")).unwrap(),
            b"binary"
        );
        let mode = fs::metadata(unpacked.join("Contents/MacOS/game"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
        assert_eq!(
            fs::read_link(unpacked.join("Contents/link")).unwrap(),
            Path::new("MacOS/game")
        );
        assert_eq!(
            fs::read_link(folder.join("Applications")).unwrap(),
            Path::new("/Applications")
        );
        // A package for another app, or an existing folder, is refused.
        assert!(stage_folder(&package, "Other.app", &dir.join("other")).is_err());
        assert!(stage_folder(&package, "Game.app", &folder).is_err());

        let args = create_args(&folder, &dir.join("x.dmg"));
        let at = |flag: &str| args.iter().position(|a| a == flag).map(|i| &args[i + 1]);
        assert_eq!(at("-volname").unwrap(), VOLUME);
        assert_eq!(at("-format").unwrap(), FORMAT);
        assert_eq!(at("-srcfolder").unwrap(), folder.as_os_str());
        assert_eq!(args.last().unwrap(), dir.join("x.dmg").as_os_str());
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn mounted_images_must_show_the_app_and_the_link_only() {
        let entry = |n: &str, link: Option<&str>| (n.to_owned(), link.map(PathBuf::from));
        let good = [
            entry("Game.app", None),
            entry("Applications", Some("/Applications")),
            entry(".fseventsd", None),
        ];
        check_layout(&good, "Game.app").unwrap();
        assert!(check_layout(&good[..1], "Game.app").is_err());
        assert!(check_layout(&good, "Other.app").is_err());
        let wrong_link = [entry("Game.app", None), entry("Applications", Some("/tmp"))];
        assert!(check_layout(&wrong_link, "Game.app").is_err());
        let extra = [good[0].clone(), good[1].clone(), entry("Readme", None)];
        assert!(check_layout(&extra, "Game.app").is_err());
        assert_eq!(
            url(
                "https://cdn.test",
                "0.4.2".parse().unwrap(),
                &filename("0.4.2".parse().unwrap(), "darwin-aarch64")
            ),
            "https://cdn.test/releases/0.4.2/under-the-canopy-0.4.2-darwin-aarch64.dmg"
        );
    }
}
