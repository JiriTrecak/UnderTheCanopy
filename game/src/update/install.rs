//! Installing a verified update package over the running copy, and starting the new one.
//!
//! The package is the old Tauri updater's macOS format: `<Name>.app.tar.gz`, a gzip tar
//! whose one top-level entry is the `.app` bundle. It is unpacked next to the installed
//! bundle (same volume, so the swap is two renames), the installed bundle is moved aside,
//! the new one moved into its place, and the old one removed. If moving the new bundle in
//! fails, the old one is moved back. Only macOS installs for now; other platforms check
//! and show updates (the feed has no packages for them yet).

use flate2::read::GzDecoder;
use std::ffi::OsString;
use std::io;
use std::path::{Component, Path, PathBuf};

/// The `.app` bundle an executable runs from (`<X>.app/Contents/MacOS/<exe>`).
pub fn bundle_of(exe: &Path) -> Option<PathBuf> {
    let macos = exe.parent()?;
    let contents = macos.parent()?;
    let bundle = contents.parent()?;
    let is = |p: &Path, name: &str| p.file_name().is_some_and(|n| n == name);
    let app = bundle
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("app"));
    (is(macos, "MacOS") && is(contents, "Contents") && app).then(|| bundle.to_path_buf())
}

/// Why the copy running as `exe` cannot install updates, if it cannot.
pub fn blocker(exe: &Path) -> Option<String> {
    if !cfg!(target_os = "macos") {
        return Some("Updates cannot be installed from the game on this platform yet.".into());
    }
    let Some(bundle) = bundle_of(exe) else {
        return Some(
            "This copy does not run from an installed app (a development build), so it cannot \
             install updates."
                .into(),
        );
    };
    // A quarantined app opened where it was downloaded runs from a read-only copy.
    if bundle
        .components()
        .any(|c| c.as_os_str() == "AppTranslocation")
    {
        return Some("Move Under the Canopy to the Applications folder to install updates.".into());
    }
    None
}

/// Installs the verified package `archive` over the bundle at `bundle`.
pub fn install_app_archive(archive: &Path, bundle: &Path) -> Result<(), String> {
    install_with(archive, bundle, &mut |from, to| std::fs::rename(from, to))
}

/// [`install_app_archive`] with the move step given (tests make it fail).
fn install_with(
    archive: &Path,
    bundle: &Path,
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), String> {
    let (Some(parent), Some(name)) = (bundle.parent(), bundle.file_name()) else {
        return Err(format!("{} is not an app bundle", bundle.display()));
    };
    if !bundle.is_dir() {
        return Err(format!("{} is not installed", bundle.display()));
    }
    let staging = parent.join(format!(
        ".{}.update-{}",
        name.to_string_lossy(),
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&staging);
    std::fs::create_dir(&staging).map_err(|e| format!("{}: {e}", staging.display()))?;
    let result = unpack(archive, &staging.join("new"))
        .map_err(Swap::Failed)
        .and_then(|fresh| swap(bundle, &fresh, &staging.join("old"), rename));
    match &result {
        // The installed bundle is stranded in the staging folder: keep it.
        Err(Swap::Stranded(_)) => {}
        _ => {
            if let Err(error) = std::fs::remove_dir_all(&staging) {
                tracing::warn!(%error, staging = %staging.display(), "cannot remove the update's staging folder");
            }
        }
    }
    result.map_err(|e| match e {
        Swap::Failed(message) => message,
        Swap::Stranded(message) => format!(
            "{message}; the previous version is at {}",
            staging.join("old").display()
        ),
    })
}

enum Swap {
    /// Nothing changed.
    Failed(String),
    /// The installed bundle was moved aside and could not be moved back.
    Stranded(String),
}

/// Moves `installed` to `aside` and `fresh` into its place; moves it back on failure.
fn swap(
    installed: &Path,
    fresh: &Path,
    aside: &Path,
    rename: &mut dyn FnMut(&Path, &Path) -> io::Result<()>,
) -> Result<(), Swap> {
    rename(installed, aside).map_err(|e| Swap::Failed(format!("moving the installed app: {e}")))?;
    if let Err(error) = rename(fresh, installed) {
        let message = format!("moving the new app into place: {error}");
        return match rename(aside, installed) {
            Ok(()) => Err(Swap::Failed(message)),
            Err(back) => Err(Swap::Stranded(format!("{message}; moving it back: {back}"))),
        };
    }
    Ok(())
}

/// Unpacks the package into `into`; returns the bundle it holds.
fn unpack(archive: &Path, into: &Path) -> Result<PathBuf, String> {
    let io = |e: io::Error| format!("unpacking {}: {e}", archive.display());
    std::fs::create_dir_all(into).map_err(io)?;
    let file = std::fs::File::open(archive).map_err(io)?;
    let mut tar = tar::Archive::new(GzDecoder::new(io::BufReader::new(file)));
    let mut top: Option<OsString> = None;
    for entry in tar.entries().map_err(io)? {
        let mut entry = entry.map_err(io)?;
        let path = entry.path().map_err(io)?.into_owned();
        let mut parts = path.components().filter(|c| *c != Component::CurDir);
        let first = match parts.next() {
            None => continue,
            Some(Component::Normal(first)) => first.to_owned(),
            Some(_) => return Err(format!("the package has an unsafe path {}", path.display())),
        };
        let is_app = Path::new(&first)
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("app"));
        if !is_app {
            return Err(format!("the package holds {}, not an app", path.display()));
        }
        match &top {
            None => top = Some(first),
            Some(t) if *t != first => return Err("the package holds more than one app".into()),
            Some(_) => {}
        }
        if !entry.unpack_in(into).map_err(io)? {
            return Err(format!("the package has an unsafe path {}", path.display()));
        }
    }
    let app = into.join(top.ok_or("the package is empty")?);
    if !app.join("Contents/Info.plist").is_file() || !app.join("Contents/MacOS").is_dir() {
        return Err("the package's app has no Contents/Info.plist or Contents/MacOS".into());
    }
    Ok(app)
}

/// Starts the app at `bundle` as a new instance (the running one then exits).
#[cfg(target_os = "macos")]
pub fn relaunch(bundle: &Path) -> Result<(), String> {
    let status = std::process::Command::new("/usr/bin/open")
        .arg("-n")
        .arg(bundle)
        .status()
        .map_err(|e| format!("open: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("open -n {} failed: {status}", bundle.display()))
    }
}

#[cfg(not(target_os = "macos"))]
pub fn relaunch(_bundle: &Path) -> Result<(), String> {
    Err("restarting into an update is not supported on this platform yet".into())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("canopy-install-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    /// A minimal bundle whose executable says `marker`.
    fn bundle(at: &Path, marker: &str) {
        fs::create_dir_all(at.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(at.join("Contents/Resources/data")).unwrap();
        fs::write(at.join("Contents/Info.plist"), "<plist/>").unwrap();
        fs::write(at.join("Contents/MacOS/canopy"), marker).unwrap();
        fs::write(at.join("Contents/Resources/data/version"), marker).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                at.join("Contents/MacOS/canopy"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
        }
    }

    /// A `.tar.gz` holding `entries` (archive name, source folder).
    fn package(out: &Path, entries: &[(&str, &Path)]) {
        let file = fs::File::create(out).unwrap();
        let gz = flate2::write::GzEncoder::new(file, flate2::Compression::fast());
        let mut tar = tar::Builder::new(gz);
        for (name, dir) in entries {
            tar.append_dir_all(name, dir).unwrap();
        }
        tar.into_inner().unwrap().finish().unwrap();
    }

    fn marker(bundle: &Path) -> String {
        fs::read_to_string(bundle.join("Contents/MacOS/canopy")).unwrap()
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn finds_the_bundle_of_an_executable() {
        assert_eq!(
            bundle_of(Path::new(
                "/Applications/Under the Canopy.app/Contents/MacOS/canopy"
            )),
            Some(PathBuf::from("/Applications/Under the Canopy.app"))
        );
        assert_eq!(bundle_of(Path::new("/repo/target/debug/canopy")), None);
        assert_eq!(bundle_of(Path::new("/x/Contents/MacOS/canopy")), None);
        assert!(blocker(Path::new("/repo/target/debug/canopy")).is_some());
    }

    #[test]
    fn installs_a_package_over_the_installed_bundle() {
        let root = scratch("ok");
        let source = root.join("build/Under the Canopy.app");
        bundle(&source, "new");
        let archive = root.join("p.app.tar.gz");
        package(&archive, &[("Under the Canopy.app", &source)]);
        // The player renamed the installed copy: it is updated where it is.
        let apps = root.join("Applications");
        let installed = apps.join("Canopy.app");
        bundle(&installed, "old");

        install_app_archive(&archive, &installed).unwrap();
        assert_eq!(marker(&installed), "new");
        assert_eq!(
            fs::read_to_string(installed.join("Contents/Resources/data/version")).unwrap(),
            "new"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(installed.join("Contents/MacOS/canopy"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111, "the executable stays executable");
        }
        // No staging folder or old copy is left behind.
        assert_eq!(leftovers(&apps), ["Canopy.app"]);
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn a_failed_move_rolls_back() {
        let root = scratch("rollback");
        let source = root.join("build/Under the Canopy.app");
        bundle(&source, "new");
        let archive = root.join("p.app.tar.gz");
        package(&archive, &[("Under the Canopy.app", &source)]);
        let apps = root.join("Applications");
        let installed = apps.join("Under the Canopy.app");
        bundle(&installed, "old");

        // Moving the new bundle in (the second move) fails.
        let mut moves = 0;
        let error = install_with(&archive, &installed, &mut |from, to| {
            moves += 1;
            if moves == 2 {
                Err(io::Error::other("disk full"))
            } else {
                fs::rename(from, to)
            }
        })
        .unwrap_err();
        assert!(error.contains("disk full"), "{error}");
        assert_eq!(moves, 3, "moved aside, failed, moved back");
        assert_eq!(marker(&installed), "old");
        assert_eq!(leftovers(&apps), ["Under the Canopy.app"]);

        // Moving it back fails too: the old copy is kept in the staging folder.
        let mut moves = 0;
        let error = install_with(&archive, &installed, &mut |from, to| {
            moves += 1;
            if moves >= 2 {
                Err(io::Error::other("gone"))
            } else {
                fs::rename(from, to)
            }
        })
        .unwrap_err();
        assert!(error.contains("previous version is at"), "{error}");
        let staging = apps.join(format!(
            ".Under the Canopy.app.update-{}",
            std::process::id()
        ));
        assert_eq!(marker(&staging.join("old")), "old");
        let _ = fs::remove_dir_all(&root);
    }

    #[test]
    fn refuses_packages_that_are_not_one_app() {
        let root = scratch("refuse");
        let apps = root.join("Applications");
        let installed = apps.join("Under the Canopy.app");
        bundle(&installed, "old");
        let a = root.join("build/A.app");
        let b = root.join("build/B.app");
        bundle(&a, "a");
        bundle(&b, "b");
        let loose = root.join("build/loose");
        fs::create_dir_all(&loose).unwrap();
        fs::write(loose.join("readme"), "x").unwrap();
        let empty_app = root.join("build/Empty.app");
        fs::create_dir_all(&empty_app).unwrap();

        let cases: [(&str, Vec<(&str, &Path)>); 3] = [
            ("two", vec![("A.app", &a), ("B.app", &b)]),
            ("loose", vec![("loose", &loose)]),
            ("empty", vec![("Empty.app", &empty_app)]),
        ];
        for (name, entries) in cases {
            let archive = root.join(format!("{name}.tar.gz"));
            package(&archive, &entries);
            assert!(install_app_archive(&archive, &installed).is_err(), "{name}");
            assert_eq!(marker(&installed), "old", "{name}");
            assert_eq!(leftovers(&apps), ["Under the Canopy.app"], "{name}");
        }
        // Not a gzip tar at all.
        let junk = root.join("junk.tar.gz");
        fs::write(&junk, b"not a package").unwrap();
        assert!(install_app_archive(&junk, &installed).is_err());
        assert_eq!(marker(&installed), "old");
        let _ = fs::remove_dir_all(&root);
    }
}
