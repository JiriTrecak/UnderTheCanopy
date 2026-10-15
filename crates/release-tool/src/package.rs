//! Update packages as the old Tauri updater installs them: the app bundle in a gzipped tar
//! whose top-level entry is the bundle (`Under the Canopy.app/…`), signed with the updater
//! key as minisign prehashed Ed25519 whose trusted comment binds the version.

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine;
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
};

/// The untrusted comment Tauri writes.
const UNTRUSTED_COMMENT: &str = "signature from tauri secret key";

/// The updater's secret key: `TAURI_SIGNING_PRIVATE_KEY` (a path or the key itself, as the
/// Tauri CLI accepts it) or `~/.config/under-the-canopy/updater.key`, decrypted with
/// `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` (empty when unset). Its contents are never shown.
pub fn load_signing_key() -> Result<minisign::SecretKey> {
    let text = match std::env::var("TAURI_SIGNING_PRIVATE_KEY") {
        Ok(value) if Path::new(&value).is_file() => {
            std::fs::read_to_string(&value).context("reading TAURI_SIGNING_PRIVATE_KEY")?
        }
        Ok(value) => value,
        Err(_) => {
            let path = key_path()?;
            if !path.is_file() {
                bail!(
                    "the release signing key is missing ({}); unsigned builds cannot be published",
                    path.display()
                );
            }
            std::fs::read_to_string(&path).context("reading the release signing key")?
        }
    };
    let password = std::env::var("TAURI_SIGNING_PRIVATE_KEY_PASSWORD").unwrap_or_default();
    secret_key_from_tauri(&text, password)
}

/// `~/.config/under-the-canopy/updater.key` (the same place on every platform, as before).
pub fn key_path() -> Result<PathBuf> {
    let base = directories::BaseDirs::new().ok_or_else(|| anyhow!("no home directory"))?;
    Ok(base.home_dir().join(".config/under-the-canopy/updater.key"))
}

/// Decodes a key as Tauri stores it (base64 of the minisign secret key file).
pub fn secret_key_from_tauri(text: &str, password: String) -> Result<minisign::SecretKey> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(text.trim())
        .map_err(|_| anyhow!("the signing key is not base64 of a minisign key"))?;
    let text = String::from_utf8(bytes)
        .map_err(|_| anyhow!("the signing key is not base64 of a minisign key"))?;
    let key_box = minisign::SecretKeyBox::from_string(&text)
        .map_err(|_| anyhow!("the signing key is not a minisign secret key"))?;
    if let Ok(key) = key_box.into_secret_key(Some(password.clone())) {
        return Ok(key);
    }
    // Older minisign versions (the Tauri CLI's) encrypted keys even with an empty password;
    // current ones skip the empty password's scrypt step, so do it here.
    if password.is_empty()
        && let Some(key) = scrypt_with_empty_password(&text)
    {
        return Ok(key);
    }
    bail!(
        "the signing key cannot be decrypted; set TAURI_SIGNING_PRIVATE_KEY_PASSWORD if it has a password"
    )
}

/// Minisign's secret key layout: algorithms, salt, scrypt limits, then the encrypted key
/// number, key and checksum.
const KEY_BYTES: usize = 158;
const SALT: std::ops::Range<usize> = 6..38;
const ENCRYPTED: std::ops::Range<usize> = 54..158;

/// XORs the key's encrypted part with scrypt of the empty password (as minisign derives it
/// from the key's limits), returning the key if its checksum then matches.
fn scrypt_with_empty_password(key_box: &str) -> Option<minisign::SecretKey> {
    let encoded = key_box.lines().nth(1)?;
    let mut bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .ok()?;
    if bytes.len() != KEY_BYTES || &bytes[2..4] != b"Sc" {
        return None;
    }
    xor_scrypt_stream(&mut bytes, b"")?;
    let decrypted = format!(
        "untrusted comment: decrypted\n{}\n",
        base64::engine::general_purpose::STANDARD.encode(&bytes)
    );
    // An empty password skips decryption and verifies the checksum.
    minisign::SecretKeyBox::from_string(&decrypted)
        .ok()?
        .into_secret_key(Some(String::new()))
        .ok()
}

/// Minisign's key encryption (its own inverse): the scrypt stream of `password` over the
/// key's salt, with N, r and p picked from its ops and memory limits as libsodium does.
fn xor_scrypt_stream(key: &mut [u8], password: &[u8]) -> Option<()> {
    let u64le = |r: std::ops::Range<usize>| u64::from_le_bytes(key[r].try_into().unwrap());
    let opslimit = u64le(38..46).max(32_768);
    let memlimit = u64le(46..54);
    if memlimit > 1 << 30 {
        return None;
    }
    let r = 8u32;
    let n_log2 = |max_n: u64| (1u8..63).find(|&n| 1u64 << n > max_n / 2).unwrap_or(63);
    let (n_log2, p) = if opslimit < memlimit / 32 {
        (n_log2(opslimit / (u64::from(r) * 4)), 1)
    } else {
        let n = n_log2(memlimit / (u64::from(r) * 128));
        let max_rp = ((opslimit / 4) / (1u64 << n)).min(0x3fff_ffff) as u32;
        (n, max_rp / r)
    };
    if n_log2 > 20 {
        return None;
    }
    let params = scrypt::Params::new(n_log2, r, p, scrypt::Params::RECOMMENDED_LEN).ok()?;
    let mut stream = [0u8; ENCRYPTED.end - ENCRYPTED.start];
    scrypt::scrypt(password, &key[SALT], &params, &mut stream).ok()?;
    for (byte, s) in key[ENCRYPTED].iter_mut().zip(stream) {
        *byte ^= s;
    }
    Some(())
}

/// Signs `package` as the Tauri CLI does: the trusted comment names the time, the file
/// Tauri would have signed (`file_name`) and the version. Returns base64 of the signature
/// file (what `.sig` files and the feed hold).
pub fn sign_package(
    key: &minisign::SecretKey,
    package: &Path,
    file_name: &str,
    version: &str,
    timestamp: u64,
) -> Result<String> {
    let trusted = format!("timestamp:{timestamp}\tfile:{file_name}\tversion:{version}");
    let reader = File::open(package).with_context(|| format!("opening {}", package.display()))?;
    let signature = minisign::sign(
        None,
        key,
        std::io::BufReader::new(reader),
        Some(&trusted),
        Some(UNTRUSTED_COMMENT),
    )
    .map_err(|e| anyhow!("signing the package: {e}"))?;
    Ok(base64::engine::general_purpose::STANDARD.encode(signature.to_string()))
}

/// Writes `bundle` (a directory such as `Under the Canopy.app`) into a gzipped tar at `out`
/// with the bundle as its top-level entry, keeping modes (the executable bit) and symlinks.
pub fn create_tar_gz(bundle: &Path, out: &Path) -> Result<()> {
    let name = bundle
        .file_name()
        .ok_or_else(|| anyhow!("{} has no name", bundle.display()))?;
    let file = File::create(out).with_context(|| format!("creating {}", out.display()))?;
    let gz = flate2::write::GzEncoder::new(BufWriter::new(file), flate2::Compression::default());
    let mut tar = tar::Builder::new(gz);
    tar.follow_symlinks(false);
    tar.append_dir_all(name, bundle)
        .with_context(|| format!("archiving {}", bundle.display()))?;
    let mut writer = tar.into_inner()?.finish()?;
    writer.flush()?;
    writer
        .into_inner()
        .map_err(|e| e.into_error())?
        .sync_all()?;
    Ok(())
}

/// Checks a package as the updater will unpack it: every entry under `top`, and `executable`
/// (relative to `top`) present with its executable bit. Returns the number of entries.
pub fn check_tar_gz(package: &Path, top: &str, executable: &str) -> Result<usize> {
    let file = File::open(package)?;
    let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(file));
    let (mut entries, mut found) = (0, false);
    for entry in archive.entries()? {
        let entry = entry?;
        let path = entry.path()?.into_owned();
        let mut parts = path.components();
        if parts.next().map(|c| c.as_os_str()) != Some(top.as_ref()) {
            bail!("package entry {} is outside {top}", path.display());
        }
        if parts.as_path() == Path::new(executable) {
            if entry.header().mode()? & 0o111 == 0 {
                bail!("{executable} lost its executable bit in the package");
            }
            found = true;
        }
        entries += 1;
    }
    if !found {
        bail!("the package has no {top}/{executable}");
    }
    Ok(entries)
}

/// Lowercase hex.
pub fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// SHA-256 (lowercase hex) and length of a file.
pub fn digest_file(path: &Path) -> Result<(String, u64)> {
    let mut file = File::open(path).with_context(|| format!("opening {}", path.display()))?;
    let mut hash = Sha256::new();
    let mut buffer = vec![0u8; 1 << 20];
    let mut bytes = 0u64;
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
        bytes += n as u64;
    }
    Ok((crate::package::hex(&hash.finalize()), bytes))
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::engine::general_purpose::STANDARD;
    use std::fs;

    #[test]
    fn packages_unpack_as_the_updater_expects_and_verify() {
        let dir = std::env::temp_dir().join(format!("canopy-package-{}", std::process::id()));
        let app = dir.join("Game.app");
        fs::create_dir_all(app.join("Contents/MacOS")).unwrap();
        fs::create_dir_all(app.join("Contents/Resources/data")).unwrap();
        fs::write(app.join("Contents/MacOS/game"), b"binary").unwrap();
        fs::write(app.join("Contents/Resources/data/pack"), vec![7u8; 100_000]).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(
                app.join("Contents/MacOS/game"),
                fs::Permissions::from_mode(0o755),
            )
            .unwrap();
            std::os::unix::fs::symlink("data/pack", app.join("Contents/Resources/link")).unwrap();
        }
        let package = dir.join("Game.app.tar.gz");
        create_tar_gz(&app, &package).unwrap();
        assert!(check_tar_gz(&package, "Game.app", "Contents/MacOS/game").unwrap() >= 6);
        assert!(check_tar_gz(&package, "Other.app", "Contents/MacOS/game").is_err());
        assert!(check_tar_gz(&package, "Game.app", "Contents/MacOS/missing").is_err());

        // Unpack as Tauri's macOS updater does: drop the first component.
        let out = dir.join("installed");
        let mut archive =
            tar::Archive::new(flate2::read::GzDecoder::new(File::open(&package).unwrap()));
        for entry in archive.entries().unwrap() {
            let mut entry = entry.unwrap();
            let rest: PathBuf = entry.path().unwrap().iter().skip(1).collect();
            let target = out.join(rest);
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            entry.unpack(&target).unwrap();
        }
        assert_eq!(
            fs::read(out.join("Contents/MacOS/game")).unwrap(),
            b"binary"
        );
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(out.join("Contents/MacOS/game"))
                .unwrap()
                .permissions()
                .mode();
            assert_ne!(mode & 0o111, 0);
            let link = out.join("Contents/Resources/link");
            assert!(fs::symlink_metadata(&link).unwrap().is_symlink());
            assert_eq!(fs::read_link(&link).unwrap(), Path::new("data/pack"));
        }

        // Signed with a key stored as Tauri stores it, verified as the game verifies.
        let keys = minisign::KeyPair::generate_encrypted_keypair(Some(String::new())).unwrap();
        let b64 = |s: String| base64::engine::general_purpose::STANDARD.encode(s);
        let secret = b64(keys.sk.to_box(None).unwrap().to_string());
        let public = b64(keys.pk.to_box().unwrap().to_string());
        let key = secret_key_from_tauri(&secret, String::new()).unwrap();
        let signature = sign_package(&key, &package, "Game.app.tar.gz", "0.3.0", 1).unwrap();
        canopy_release::verify_package(
            &public,
            &signature,
            "0.3.0",
            File::open(&package).unwrap(),
            |_| {},
        )
        .unwrap();
        let text = String::from_utf8(
            base64::engine::general_purpose::STANDARD
                .decode(&signature)
                .unwrap(),
        )
        .unwrap();
        assert!(text.starts_with("untrusted comment: signature from tauri secret key\n"));
        assert!(
            text.contains("trusted comment: timestamp:1\tfile:Game.app.tar.gz\tversion:0.3.0\n")
        );
        assert!(secret_key_from_tauri("not a key", String::new()).is_err());

        // A key an older minisign encrypted with the empty password opens and signs alike.
        let text = String::from_utf8(STANDARD.decode(&secret).unwrap()).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        let mut raw = STANDARD.decode(lines[1]).unwrap();
        xor_scrypt_stream(&mut raw, b"").unwrap();
        let legacy = b64(format!("{}\n{}\n", lines[0], STANDARD.encode(&raw)));
        assert!(
            minisign::SecretKeyBox::from_string(
                std::str::from_utf8(&STANDARD.decode(&legacy).unwrap()).unwrap()
            )
            .unwrap()
            .into_secret_key(Some(String::new()))
            .is_err()
        );
        let key = secret_key_from_tauri(&legacy, String::new()).unwrap();
        let signature = sign_package(&key, &package, "Game.app.tar.gz", "0.3.0", 2).unwrap();
        canopy_release::verify_package(
            &public,
            &signature,
            "0.3.0",
            File::open(&package).unwrap(),
            |_| {},
        )
        .unwrap();

        let (sha, bytes) = digest_file(&package).unwrap();
        assert_eq!(bytes, fs::metadata(&package).unwrap().len());
        assert_eq!(sha, hex(&Sha256::digest(fs::read(&package).unwrap())));
        fs::remove_dir_all(&dir).unwrap();
    }
}
