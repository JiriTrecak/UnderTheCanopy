//! Player feedback: `POST /api/feedback` on the match server (docs/server.md, D59), which
//! files the report as an issue in Linear. A lobby route of its own, not
//! part of the frozen match protocol (D10).

use crate::net::{Net, NetError, Pending};
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// How long a report may take to send before it waits for a later try.
const TIMEOUT: Duration = Duration::from_secs(20);

/// A report as the server takes it (field names are the server's).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackReport {
    /// The game's own id for the report, so a resend after a lost reply is filed once.
    pub report_id: String,
    /// A random id per installation.
    pub install_id: String,
    /// `bug`, `idea`, `balance` or `other`.
    pub category: String,
    pub description: String,
    pub meta: FeedbackMeta,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screenshot: Option<FeedbackScreenshot>,
    /// The replay of the match so far, from inside a match (D60).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replay: Option<FeedbackFile>,
    /// A save of the moment the form opened, from inside a match.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub save: Option<FeedbackFile>,
}

/// The replay's media type.
pub const REPLAY_TYPE: &str = "application/vnd.canopy.replay";
/// An exported save's media type.
pub const SAVE_TYPE: &str = "application/gzip";
/// The largest replay the server takes, decoded.
pub const MAX_REPLAY: usize = 1 << 20;
/// The largest save the server takes, decoded.
pub const MAX_SAVE: usize = 4 << 20;

/// A file a report carries besides its screenshot.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedbackFile {
    /// [`REPLAY_TYPE`] or [`SAVE_TYPE`].
    #[serde(rename = "type")]
    pub mime: String,
    /// The file, base64.
    pub data: String,
    /// Its file name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FeedbackMeta {
    pub version: String,
    pub os: String,
    pub gpu: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preset: Option<String>,
    /// What showed: `menu` or `match`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub screen: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub map: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tick: Option<u32>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct FeedbackScreenshot {
    /// `image/jpeg`, `image/png` or `image/webp`.
    #[serde(rename = "type")]
    pub mime: String,
    /// The image file, base64.
    pub data: String,
    /// What it shows ("Screenshot of the menu").
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

/// The server took the report; `id` is what it filed it under.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct FeedbackReceipt {
    pub id: String,
}

impl Net {
    /// `POST /api/feedback`. A refusal that is not [`NetError::retryable`] means the
    /// server will never take this report.
    pub fn send_feedback(&self, report: &FeedbackReport) -> Pending<FeedbackReceipt> {
        let request = self.post("/api/feedback").timeout(TIMEOUT).json(report);
        self.json(request)
    }
}

impl NetError {
    /// Whether sending the same request again later may succeed: anything but a refusal
    /// of the request itself. 404 is retried: a server not yet updated lacks the route.
    pub fn retryable(&self) -> bool {
        match self {
            Self::Server { status, .. } => {
                !(400..500).contains(status) || matches!(status, 404 | 408 | 429)
            }
            Self::Transport(_) | Self::Stopped => true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_use_the_servers_field_names() {
        let report = FeedbackReport {
            report_id: "r1234567".into(),
            install_id: "i1234567".into(),
            category: "bug".into(),
            description: "Stuck".into(),
            meta: FeedbackMeta {
                version: "0.3.7".into(),
                tick: Some(40),
                ..Default::default()
            },
            screenshot: Some(FeedbackScreenshot {
                mime: "image/jpeg".into(),
                data: "AA==".into(),
                label: None,
            }),
            replay: Some(FeedbackFile {
                mime: REPLAY_TYPE.into(),
                data: "VVRDUg==".into(),
                name: Some("Echo.utcr".into()),
            }),
            save: None,
        };
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["reportId"], "r1234567");
        assert_eq!(json["installId"], "i1234567");
        assert_eq!(json["meta"]["tick"], 40);
        assert!(json["meta"].get("map").is_none());
        assert_eq!(json["screenshot"]["type"], "image/jpeg");
        assert_eq!(json["replay"]["type"], REPLAY_TYPE);
        assert_eq!(json["replay"]["name"], "Echo.utcr");
        assert!(json.get("save").is_none(), "absent files are left out");
    }

    #[test]
    fn refusals_are_final_but_limits_and_outages_are_not() {
        let server = |status| NetError::Server {
            status,
            message: String::new(),
        };
        assert!(!server(400).retryable());
        assert!(!server(413).retryable());
        assert!(server(404).retryable());
        assert!(server(429).retryable());
        assert!(server(502).retryable());
        assert!(NetError::Transport("offline".into()).retryable());
    }
}
