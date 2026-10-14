//! Where the match server is (old `src/shared/net/endpoint.ts`).
//!
//! The deployed server is reached over HTTPS and WSS on port 443 only; there is no
//! plaintext fallback. An override (game setting or `CANOPY_MATCH_SERVER`) may name another
//! server; plaintext `http://` is accepted only for a loopback address, for running the old
//! TypeScript server locally.

use crate::wire::encode_uri_component;
use std::fmt;
use std::net::IpAddr;

/// The deployed match server (`MATCH_HOST`).
pub const MATCH_HOST: &str = "18.134.138.1";
/// Environment variable that overrides the server, ahead of the game setting.
pub const ENV: &str = "CANOPY_MATCH_SERVER";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddress {
    /// TLS (`https`/`wss`) or loopback plaintext (`http`/`ws`).
    pub secure: bool,
    /// `host` or `host:port`.
    pub authority: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AddressError {
    #[error("empty server address")]
    Empty,
    #[error("unsupported scheme in {0:?}; use https:// (or http:// for a loopback test server)")]
    Scheme(String),
    #[error("{0:?} is not a server address: no path, query or user may follow the host")]
    Shape(String),
    #[error("plaintext is only allowed for a loopback server, not {0:?}")]
    Plaintext(String),
}

impl Default for ServerAddress {
    fn default() -> Self {
        Self {
            secure: true,
            authority: MATCH_HOST.into(),
        }
    }
}

impl ServerAddress {
    /// Reads `host`, `host:port`, `https://host[:port]`, `wss://…`, or for loopback hosts
    /// `http://…`/`ws://…`. A trailing `/` is allowed.
    pub fn parse(text: &str) -> Result<Self, AddressError> {
        let text = text.trim();
        if text.is_empty() {
            return Err(AddressError::Empty);
        }
        let (secure, rest) = match text.split_once("://") {
            None => (true, text),
            Some(("https" | "wss", rest)) => (true, rest),
            Some(("http" | "ws", rest)) => (false, rest),
            Some(_) => return Err(AddressError::Scheme(text.into())),
        };
        let authority = rest.strip_suffix('/').unwrap_or(rest);
        if authority.is_empty()
            || authority
                .chars()
                .any(|c| matches!(c, '/' | '?' | '#' | '@') || c.is_whitespace())
        {
            return Err(AddressError::Shape(text.into()));
        }
        let address = Self {
            secure,
            authority: authority.into(),
        };
        if !secure && !address.loopback() {
            return Err(AddressError::Plaintext(text.into()));
        }
        Ok(address)
    }

    /// The environment override, then the game setting, then the deployed server.
    pub fn resolve(setting: Option<&str>) -> Result<Self, AddressError> {
        match std::env::var(ENV) {
            Ok(value) if !value.trim().is_empty() => Self::parse(&value),
            _ => match setting {
                Some(value) if !value.trim().is_empty() => Self::parse(value),
                _ => Ok(Self::default()),
            },
        }
    }

    /// The host without port or IPv6 brackets.
    pub fn host(&self) -> &str {
        let authority = self.authority.as_str();
        if let Some(rest) = authority.strip_prefix('[') {
            return rest.split(']').next().unwrap_or(rest);
        }
        match authority.rsplit_once(':') {
            Some((host, port)) if port.bytes().all(|b| b.is_ascii_digit()) => host,
            _ => authority,
        }
    }

    /// The port, or the scheme's default (443 or 80).
    pub fn port(&self) -> u16 {
        let after_host = match self.authority.strip_prefix('[') {
            Some(rest) => rest.split_once(']').map_or("", |(_, tail)| tail),
            None => self.authority.rsplit_once(':').map_or("", |(_, port)| port),
        };
        let port = after_host.strip_prefix(':').unwrap_or(after_host);
        port.parse().unwrap_or(if self.secure { 443 } else { 80 })
    }

    fn loopback(&self) -> bool {
        let host = self.host();
        host.eq_ignore_ascii_case("localhost")
            || host.parse::<IpAddr>().is_ok_and(|ip| ip.is_loopback())
    }

    /// `matchHttp(path)`.
    pub fn http(&self, path: &str) -> String {
        let scheme = if self.secure { "https" } else { "http" };
        format!("{scheme}://{}{path}", self.authority)
    }

    /// `matchWs(path)`.
    pub fn ws(&self, path: &str) -> String {
        let scheme = if self.secure { "wss" } else { "ws" };
        format!("{scheme}://{}{path}", self.authority)
    }

    /// `matchUrl(roomId, token)`: the room's WebSocket.
    pub fn match_url(&self, room: &str, token: &str) -> String {
        self.ws(&format!(
            "/match/{}?token={}",
            encode_uri_component(room),
            encode_uri_component(token)
        ))
    }
}

impl fmt::Display for ServerAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.http(""))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_to_the_deployed_server_over_tls() {
        let server = ServerAddress::default();
        assert_eq!(server.http("/api/rooms"), "https://18.134.138.1/api/rooms");
        assert_eq!(
            server.match_url("1", "a b"),
            "wss://18.134.138.1/match/1?token=a%20b"
        );
    }

    #[test]
    fn parses_overrides() {
        let parse = |s| ServerAddress::parse(s);
        assert_eq!(
            parse("games.example.org").unwrap().http("/x"),
            "https://games.example.org/x"
        );
        assert_eq!(
            parse("wss://games.example.org:8443/").unwrap().ws("/m"),
            "wss://games.example.org:8443/m"
        );
        assert_eq!(
            parse("http://127.0.0.1:8787").unwrap().ws("/m"),
            "ws://127.0.0.1:8787/m"
        );
        assert!(parse("http://localhost:1").is_ok());
        assert!(parse("http://[::1]:1").is_ok());
        assert_eq!(parse("[::1]:9").unwrap().host(), "::1");
        assert_eq!(parse("[::1]:9").unwrap().port(), 9);
        assert_eq!(parse("http://localhost:8787").unwrap().port(), 8787);
        assert_eq!(parse("example.org").unwrap().port(), 443);
        assert!(matches!(
            parse("http://games.example.org"),
            Err(AddressError::Plaintext(_))
        ));
        assert!(matches!(parse("ftp://x"), Err(AddressError::Scheme(_))));
        assert!(matches!(
            parse("https://x/api"),
            Err(AddressError::Shape(_))
        ));
        assert!(matches!(parse(" "), Err(AddressError::Empty)));
    }
}
