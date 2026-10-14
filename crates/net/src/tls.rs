//! TLS for HTTPS and WSS: rustls with the operating system's certificate verifier.
//!
//! The deployed server presents a publicly trusted Let's Encrypt certificate for its IP
//! address (infrastructure/multiplayer/README.md in the old repository). The platform
//! verifier checks it against the system trust store (Security.framework on macOS,
//! CryptoAPI on Windows), so enterprise roots, revocation and the user's trust settings
//! apply as for any other app. Certificate checks are never disabled.

use std::sync::Arc;

use rustls::ClientConfig;
use rustls_platform_verifier::BuilderVerifierExt;

/// One client configuration shared by the lobby's HTTP client and every match socket.
pub fn client_config() -> Result<Arc<ClientConfig>, rustls::Error> {
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let config = ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_platform_verifier()?
        .with_no_client_auth();
    Ok(Arc::new(config))
}
