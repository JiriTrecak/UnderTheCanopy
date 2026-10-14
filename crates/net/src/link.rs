//! One room socket, run as a task on the network thread (old `src/net/ws.ts` plus the
//! confirming half of the old runtime).
//!
//! The task owns the player's [`Outbox`]: commands from any thread go straight to it and
//! are confirmed at once, as the old client's `send` → `pulseConfirm` did, so a click never
//! waits for the simulation thread. The simulation thread reports each simulated beat
//! (with its hash when due); the task then keeps confirmations `delay` beats ahead.
//! Latency probes are answered here, before anything else sees them.

use crate::lockstep::{Outbox, pulse};
use crate::net::Net;
use crate::wire::{ClientMsg, Role, ServerMsg};
use canopy_sim::Action;
use futures_util::{SinkExt, StreamExt};
use std::sync::{Arc, mpsc};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tokio_tungstenite::WebSocketStream;
use tokio_tungstenite::tungstenite::Message;
use tokio_tungstenite::tungstenite::client::IntoClientRequest;

/// Longest probe nonce echoed (the old client's limit, in UTF-16 units).
const MAX_PROBE_ID: usize = 128;

/// Simulation thread → socket task.
#[derive(Debug)]
pub(crate) enum ToIo {
    /// A message as is (lobby choices, start, ready, chat, ended).
    Send(ClientMsg),
    /// A player command for the outbox.
    Command(Action),
    /// The simulation finished beat `tick` (`World::tick()` after the step); `hash` is the
    /// desync hash to send first, when due.
    Clock {
        tick: u32,
        hash: Option<u32>,
    },
    Close,
}

/// Socket task → simulation thread.
#[derive(Debug)]
pub(crate) enum FromIo {
    Open,
    Message(Box<ServerMsg>),
    Malformed { text: String, error: String },
    Closed(String),
}

pub(crate) struct Link {
    pub to_io: UnboundedSender<ToIo>,
    pub from_io: mpsc::Receiver<FromIo>,
}

pub(crate) fn spawn(net: &Net, url: String) -> Link {
    let (to_io, rx) = unbounded_channel();
    let (tx, from_io) = mpsc::channel();
    let server = net.inner.server.clone();
    let tls = net.inner.tls.clone();
    net.inner.runtime.spawn(async move {
        let reason = match connect(&server, &url, tls, rx, &tx).await {
            Ok(reason) => reason,
            Err(error) => error,
        };
        let _ = tx.send(FromIo::Closed(reason));
    });
    Link { to_io, from_io }
}

async fn connect(
    server: &crate::ServerAddress,
    url: &str,
    tls: Arc<rustls::ClientConfig>,
    rx: UnboundedReceiver<ToIo>,
    tx: &mpsc::Sender<FromIo>,
) -> Result<String, String> {
    let request = url.into_client_request().map_err(|e| e.to_string())?;
    let tcp = tokio::net::TcpStream::connect((server.host(), server.port()))
        .await
        .map_err(|e| format!("cannot reach {server}: {e}"))?;
    // Lockstep messages are small and latency-bound.
    tcp.set_nodelay(true).map_err(|e| e.to_string())?;
    if server.secure {
        let name = rustls::pki_types::ServerName::try_from(server.host().to_string())
            .map_err(|e| e.to_string())?;
        let stream = tokio_rustls::TlsConnector::from(tls)
            .connect(name, tcp)
            .await
            .map_err(|e| format!("TLS with {server} failed: {e}"))?;
        let (ws, _) = tokio_tungstenite::client_async(request, stream)
            .await
            .map_err(|e| e.to_string())?;
        Ok(run(ws, rx, tx).await)
    } else {
        let (ws, _) = tokio_tungstenite::client_async(request, tcp)
            .await
            .map_err(|e| e.to_string())?;
        Ok(run(ws, rx, tx).await)
    }
}

/// The player's lockstep state once a match has started.
struct Confirming {
    outbox: Outbox,
    delay: u32,
    /// The last simulated beat, once the simulation runs.
    clock: Option<u32>,
    stopped: bool,
}

impl Confirming {
    fn pulse(&mut self) -> Option<ClientMsg> {
        match self.clock {
            Some(clock) if !self.stopped => pulse(&mut self.outbox, clock, self.delay),
            _ => None,
        }
    }
}

/// Runs the socket until it closes or the session goes away; returns why it ended.
async fn run<S: AsyncRead + AsyncWrite + Unpin>(
    ws: WebSocketStream<S>,
    mut rx: UnboundedReceiver<ToIo>,
    tx: &mpsc::Sender<FromIo>,
) -> String {
    let (mut sink, mut stream) = ws.split();
    if tx.send(FromIo::Open).is_err() {
        return "session dropped".into();
    }
    let mut confirming: Option<Confirming> = None;
    loop {
        let mut outgoing: Vec<ClientMsg> = Vec::new();
        tokio::select! {
            incoming = stream.next() => match incoming {
                Some(Ok(Message::Text(text))) => {
                    let text = text.as_str();
                    if let Some(id) = probe(text) {
                        if let Some(id) = id.filter(|id| id.encode_utf16().count() <= MAX_PROBE_ID) {
                            outgoing.push(ClientMsg::LatencyReply { id });
                        }
                    } else {
                        let event = match ServerMsg::parse(text) {
                            Ok(message) => {
                                observe(&mut confirming, &message);
                                FromIo::Message(Box::new(message))
                            }
                            Err(error) => FromIo::Malformed {
                                text: text.to_string(),
                                error: error.to_string(),
                            },
                        };
                        if tx.send(event).is_err() {
                            let _ = sink.close().await;
                            return "session dropped".into();
                        }
                    }
                }
                Some(Ok(Message::Close(frame))) => {
                    return match frame {
                        Some(frame) if !frame.reason.is_empty() => frame.reason.to_string(),
                        _ => "connection closed".into(),
                    };
                }
                Some(Ok(_)) => {}
                Some(Err(error)) => return error.to_string(),
                None => return "connection closed".into(),
            },
            command = rx.recv() => match command {
                None | Some(ToIo::Close) => {
                    let _ = sink.close().await;
                    return "closed by the client".into();
                }
                Some(ToIo::Send(message)) => outgoing.push(message),
                Some(ToIo::Command(action)) => {
                    if let Some(c) = confirming.as_mut().filter(|c| !c.stopped) {
                        c.outbox.send(action);
                        outgoing.extend(c.pulse());
                    }
                }
                Some(ToIo::Clock { tick, hash }) => {
                    if let Some(c) = confirming.as_mut().filter(|c| !c.stopped) {
                        if let Some(checksum) = hash {
                            outgoing.push(ClientMsg::Hash { tick, checksum });
                        }
                        c.clock = Some(tick);
                        outgoing.extend(c.pulse());
                    }
                }
            },
        }
        for message in outgoing {
            if let Err(error) = sink.send(Message::text(message.to_json())).await {
                return error.to_string();
            }
        }
    }
}

/// A `latencyProbe` (which never reaches the session) and its nonce, if it is a string.
fn probe(text: &str) -> Option<Option<String>> {
    // Cheap pre-check: commits dominate the traffic.
    if !text.contains("latencyProbe") {
        return None;
    }
    let value: serde_json::Value = serde_json::from_str(text).ok()?;
    if value.get("type")?.as_str()? != "latencyProbe" {
        return None;
    }
    Some(
        value
            .get("id")
            .and_then(|id| id.as_str())
            .map(str::to_string),
    )
}

/// Follows the match lifecycle the socket task must know about.
fn observe(confirming: &mut Option<Confirming>, message: &ServerMsg) {
    match message {
        ServerMsg::Start { config, you, .. } => {
            *confirming = (you.role == Role::Player).then(|| Confirming {
                outbox: Outbox::default(),
                delay: config.delay,
                clock: None,
                stopped: false,
            });
        }
        // The old client stops sending on a desync; restarts and live loads are not
        // supported (the session stops), and an ended match takes nothing more.
        ServerMsg::Desync { .. }
        | ServerMsg::Restart { .. }
        | ServerMsg::Load { .. }
        | ServerMsg::Ended { .. } => {
            if let Some(c) = confirming.as_mut() {
                c.stopped = true;
                c.outbox.clear();
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::probe;

    #[test]
    fn recognises_probes_only() {
        assert_eq!(
            probe(r#"{"type":"latencyProbe","id":"a-1"}"#),
            Some(Some("a-1".into()))
        );
        assert_eq!(probe(r#"{"type":"latencyProbe","id":3}"#), Some(None));
        assert_eq!(
            probe(r#"{"type":"chat","message":{"name":"x","player":0,"text":"latencyProbe"}}"#),
            None
        );
    }
}
