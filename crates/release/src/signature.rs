//! Package signatures: minisign prehashed Ed25519 over the package, whose trusted comment
//! names the version (`timestamp:…\tfile:…\tversion:0.3.0`). Both the old desktop app
//! (`requireSignedVersion`) and this game refuse a package whose signature does not bind
//! the version the feed advertises, so an old signed package cannot be replayed as new.

use base64::Engine;
use minisign_verify::{PublicKey, Signature};
use std::io::Read;

#[derive(Debug, thiserror::Error)]
pub enum SignatureError {
    #[error("the key or signature is not valid base64 minisign text")]
    Encoding,
    #[error("the signature does not match the package or key")]
    Mismatch,
    #[error("the signature binds version {found:?}, not {expected}")]
    Version {
        expected: String,
        found: Option<String>,
    },
    #[error("reading the package: {0}")]
    Io(#[from] std::io::Error),
}

fn text(b64: &str) -> Result<String, SignatureError> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(b64.trim())
        .map_err(|_| SignatureError::Encoding)?;
    String::from_utf8(bytes).map_err(|_| SignatureError::Encoding)
}

/// The version a signature's trusted comment binds, if any.
pub fn signed_version(signature_b64: &str) -> Result<Option<String>, SignatureError> {
    let sig = Signature::decode(&text(signature_b64)?).map_err(|_| SignatureError::Encoding)?;
    Ok(sig
        .trusted_comment()
        .split('\t')
        .find_map(|f| f.strip_prefix("version:"))
        .map(str::to_owned))
}

/// Verifies `package` (read to the end) against `signature_b64` and `public_key_b64`
/// (both base64 of minisign text, as Tauri stores them), and that the signature binds
/// `version`. `progress` sees each chunk's length.
pub fn verify_package(
    public_key_b64: &str,
    signature_b64: &str,
    version: &str,
    mut package: impl Read,
    mut progress: impl FnMut(usize),
) -> Result<(), SignatureError> {
    let key = PublicKey::decode(&text(public_key_b64)?).map_err(|_| SignatureError::Encoding)?;
    let sig = Signature::decode(&text(signature_b64)?).map_err(|_| SignatureError::Encoding)?;
    let mut stream = key
        .verify_stream(&sig)
        .map_err(|_| SignatureError::Mismatch)?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let n = package.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        stream.update(&buffer[..n]);
        progress(n);
    }
    stream.finalize().map_err(|_| SignatureError::Mismatch)?;
    let found = signed_version(signature_b64)?;
    if found.as_deref() != Some(version) {
        return Err(SignatureError::Version {
            expected: version.to_owned(),
            found,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live 0.2.3 package's signature (from the stable feed).
    const SIGNATURE_0_2_3: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTSzQ2dG5RbEZ3SDNWMHI4UlBQbkdjakdWVXREU2ZUMndOR1FZemFkbGYvcXhBZW4ycTNOdWpIcDdEelNnSFRhQXZWNVpQVWxhNFhBUC9RYnhJaGR5K1YzZjBuYWJ5ekFNPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkxOTA2NDAzCWZpbGU6VW5kZXIgdGhlIENhbm9weS5hcHAudGFyLmd6CXZlcnNpb246MC4yLjMKMUFQdDBpRGtSNm51SFlMUE13TzVqakUvenBjVVpVRnlINExESlNUUGhjSkdFUUVvOFZyUTUwTEl5TXBadGN3dnYxOVdPUnVGOFk4aDZaT2dPQlZQQUE9PQo=";

    #[test]
    fn the_live_signature_binds_its_version() {
        assert_eq!(
            signed_version(SIGNATURE_0_2_3).unwrap().as_deref(),
            Some("0.2.3")
        );
    }

    #[test]
    fn signed_packages_verify_and_anything_else_is_refused() {
        let keys = minisign::KeyPair::generate_unencrypted_keypair().unwrap();
        let public =
            base64::engine::general_purpose::STANDARD.encode(keys.pk.to_box().unwrap().to_string());
        let sign = |data: &[u8], comment: &str| {
            let sig = minisign::sign(
                Some(&keys.pk),
                &keys.sk,
                data,
                Some(comment),
                Some("signature from tauri secret key"),
            )
            .unwrap();
            base64::engine::general_purpose::STANDARD.encode(sig.to_string())
        };
        let package = b"package bytes".repeat(100_000);
        let sig = sign(
            &package,
            "timestamp:1\tfile:Under the Canopy.app.tar.gz\tversion:0.3.0",
        );
        let mut seen = 0;
        verify_package(&public, &sig, "0.3.0", &package[..], |n| seen += n).unwrap();
        assert_eq!(seen, package.len());
        // Another version, other bytes, another key, no version binding.
        assert!(matches!(
            verify_package(&public, &sig, "0.3.1", &package[..], |_| {}),
            Err(SignatureError::Version { .. })
        ));
        assert!(matches!(
            verify_package(&public, &sig, "0.3.0", &b"other"[..], |_| {}),
            Err(SignatureError::Mismatch)
        ));
        assert!(verify_package(crate::PUBLIC_KEY, &sig, "0.3.0", &package[..], |_| {}).is_err());
        let unbound = sign(&package, "timestamp:1\tfile:x");
        assert!(matches!(
            verify_package(&public, &unbound, "0.3.0", &package[..], |_| {}),
            Err(SignatureError::Version { found: None, .. })
        ));
        assert!(matches!(
            verify_package(
                &public,
                "bm90IGEgc2lnbmF0dXJl",
                "0.3.0",
                &package[..],
                |_| {}
            ),
            Err(SignatureError::Encoding)
        ));
    }
}
