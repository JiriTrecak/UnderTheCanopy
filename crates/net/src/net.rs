//! The network thread and the lobby's HTTP calls (old `src/net/lobby.ts`).
//!
//! [`Net`] owns one thread named `canopy-net` running a single-threaded tokio runtime. All
//! sockets live there: lobby requests and every match WebSocket. Callers never block: a
//! lobby call returns a [`Pending`] reply to poll from the UI, and a match [`Session`]
//! exchanges messages with its socket task through channels.
//!
//! Why an async runtime: a match socket must react at once both to server messages and to
//! the player's commands (the old client confirms a click immediately). One task selecting
//! over the socket and a command channel does that without polling timeouts or splitting a
//! TLS stream across threads, and the maintained rustls-capable HTTP and WebSocket clients
//! (reqwest, tokio-tungstenite) are built on it. It stays inside this crate: the game sees
//! plain synchronous handles.

use crate::server::ServerAddress;
use crate::session::Session;
use crate::wire::{CreateRoom, Health, JoinRoom, Joined, RoomView, encode_uri_component};
use crate::{link, tls};
use serde::de::DeserializeOwned;
use std::sync::{Arc, mpsc};
use std::thread;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum NetError {
    /// The server answered with an error; `message` is what the old client would show.
    #[error("{message}")]
    Server { status: u16, message: String },
    /// No usable answer: connection, TLS or decoding failure.
    #[error("{0}")]
    Transport(String),
    /// The network thread is gone.
    #[error("the network thread has stopped")]
    Stopped,
}

/// A reply that arrives later. Poll it from the UI; [`Pending::wait`] blocks (tools, tests).
pub struct Pending<T> {
    rx: mpsc::Receiver<Result<T, NetError>>,
}

impl<T> Pending<T> {
    /// The reply, once it has arrived. After it has been returned, further polls report
    /// [`NetError::Stopped`].
    pub fn poll(&self) -> Option<Result<T, NetError>> {
        match self.rx.try_recv() {
            Ok(result) => Some(result),
            Err(mpsc::TryRecvError::Empty) => None,
            Err(mpsc::TryRecvError::Disconnected) => Some(Err(NetError::Stopped)),
        }
    }

    pub fn wait(self) -> Result<T, NetError> {
        self.rx.recv().unwrap_or(Err(NetError::Stopped))
    }
}

pub(crate) struct Inner {
    pub(crate) server: ServerAddress,
    pub(crate) runtime: tokio::runtime::Handle,
    pub(crate) tls: Arc<rustls::ClientConfig>,
    http: reqwest::Client,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    thread: Option<thread::JoinHandle<()>>,
}

impl Drop for Inner {
    fn drop(&mut self) {
        if let Some(stop) = self.stop.take() {
            let _ = stop.send(());
        }
        // Never join from the network thread itself (a task dropping the last handle).
        if let Some(thread) = self.thread.take()
            && thread.thread().id() != thread::current().id()
        {
            let _ = thread.join();
        }
    }
}

/// The client's connection to one match server. Cheap to clone; the network thread stops
/// when the last clone and every session are dropped.
#[derive(Clone)]
pub struct Net {
    pub(crate) inner: Arc<Inner>,
}

impl Net {
    /// Starts the network thread for `server`.
    pub fn new(server: ServerAddress) -> Result<Self, NetError> {
        let tls = tls::client_config().map_err(|e| NetError::Transport(e.to_string()))?;
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| NetError::Transport(e.to_string()))?;
        let handle = runtime.handle().clone();
        let http = {
            let _enter = handle.enter();
            reqwest::Client::builder()
                .use_preconfigured_tls((*tls).clone())
                .build()
                .map_err(|e| NetError::Transport(e.to_string()))?
        };
        let (stop, stopped) = tokio::sync::oneshot::channel::<()>();
        let thread = thread::Builder::new()
            .name("canopy-net".into())
            .spawn(move || {
                runtime.block_on(async {
                    let _ = stopped.await;
                });
            })
            .map_err(|e| NetError::Transport(e.to_string()))?;
        Ok(Self {
            inner: Arc::new(Inner {
                server,
                runtime: handle,
                tls,
                http,
                stop: Some(stop),
                thread: Some(thread),
            }),
        })
    }

    pub fn server(&self) -> &ServerAddress {
        &self.inner.server
    }

    /// `GET /api/health`.
    pub fn health(&self) -> Pending<Health> {
        self.json(self.get("/api/health"))
    }

    /// `GET /api/rooms`: rooms waiting or playing.
    pub fn rooms(&self) -> Pending<Vec<RoomView>> {
        self.json(self.get("/api/rooms"))
    }

    /// `GET /api/rooms/:id`.
    pub fn room(&self, id: &str) -> Pending<RoomView> {
        self.json(self.get(&format!("/api/rooms/{}", encode_uri_component(id))))
    }

    /// `POST /api/rooms`: creates a room and seats its host as player 0.
    pub fn create_room(&self, body: &CreateRoom) -> Pending<Joined> {
        self.json(self.post("/api/rooms").json(body))
    }

    /// `POST /api/rooms/:id/join`.
    pub fn join_room(&self, id: &str, body: &JoinRoom) -> Pending<Joined> {
        let path = format!("/api/rooms/{}/join", encode_uri_component(id));
        self.json(self.post(&path).json(body))
    }

    /// `POST /api/rooms/clear`: ends every session on the server (maintenance).
    pub fn clear_rooms(&self) -> Pending<u32> {
        #[derive(serde::Deserialize)]
        struct Deleted {
            deleted: u32,
        }
        let request = self.post("/api/rooms/clear");
        self.run(async move {
            let response = request.send().await.map_err(transport)?;
            if response.status() == reqwest::StatusCode::NOT_FOUND {
                return Err(NetError::Server {
                    status: 404,
                    message: "Session cleanup requires an updated MatchHost server.".into(),
                });
            }
            Ok(decode::<Deleted>(response).await?.deleted)
        })
    }

    /// `POST /api/rooms/:id/start` (host only). The old client starts over the room's
    /// socket instead ([`Session::start_match`]), so a last selection cannot be overtaken.
    pub fn start_room(&self, id: &str, token: &str) -> Pending<()> {
        self.text_ok(self.authorized("start", id, token))
    }

    /// `POST /api/rooms/:id/restart` (host only).
    pub fn restart_room(&self, id: &str, token: &str) -> Pending<()> {
        self.text_ok(self.authorized("restart", id, token))
    }

    /// `POST /api/rooms/:id/load` with a multiplayer save in the old game's envelope.
    pub fn load_room(&self, id: &str, token: &str, save: &serde_json::Value) -> Pending<()> {
        self.text_ok(self.authorized("load", id, token).json(save))
    }

    /// `POST /api/rooms/:id/leave`. Errors are ignored, as the old client does. The
    /// request completes even if every other handle is dropped meanwhile.
    pub fn leave_room(&self, id: &str, token: &str) -> Pending<()> {
        let request = self.authorized("leave", id, token);
        let keep = self.clone();
        self.run(async move {
            let _ = request.send().await;
            drop(keep);
            Ok(())
        })
    }

    /// `POST /api/rooms/:id/end`: discards the room (any member). A missing room is fine.
    pub fn end_room(&self, id: &str, token: &str) -> Pending<()> {
        let request = self.authorized("end", id, token);
        self.run(async move {
            let response = request.send().await.map_err(transport)?;
            let status = response.status();
            if status.is_success() || status == reqwest::StatusCode::NOT_FOUND {
                Ok(())
            } else {
                Err(NetError::Server {
                    status: status.as_u16(),
                    message: response.text().await.unwrap_or_default(),
                })
            }
        })
    }

    /// Opens the room's socket with a seat from [`Net::create_room`] or [`Net::join_room`].
    pub fn enter(&self, joined: &Joined) -> Session {
        let url = self.inner.server.match_url(&joined.room.id, &joined.token);
        let link = link::spawn(self, url);
        Session::remote(self.clone(), link, joined)
    }

    fn get(&self, path: &str) -> reqwest::RequestBuilder {
        self.inner.http.get(self.inner.server.http(path))
    }

    fn post(&self, path: &str) -> reqwest::RequestBuilder {
        self.inner.http.post(self.inner.server.http(path))
    }

    fn authorized(&self, action: &str, id: &str, token: &str) -> reqwest::RequestBuilder {
        let path = format!("/api/rooms/{}/{action}", encode_uri_component(id));
        self.post(&path)
            .header(reqwest::header::AUTHORIZATION, format!("Bearer {token}"))
    }

    fn run<T: Send + 'static>(
        &self,
        work: impl Future<Output = Result<T, NetError>> + Send + 'static,
    ) -> Pending<T> {
        let (tx, rx) = mpsc::channel();
        self.inner.runtime.spawn(async move {
            let _ = tx.send(work.await);
        });
        Pending { rx }
    }

    fn json<T: DeserializeOwned + Send + 'static>(
        &self,
        request: reqwest::RequestBuilder,
    ) -> Pending<T> {
        self.run(async move { decode(request.send().await.map_err(transport)?).await })
    }

    /// Success, or the body text as the error (old `startRoom`, `loadRoom`, `restartRoom`).
    fn text_ok(&self, request: reqwest::RequestBuilder) -> Pending<()> {
        self.run(async move {
            let response = request.send().await.map_err(transport)?;
            let status = response.status();
            if status.is_success() {
                Ok(())
            } else {
                Err(NetError::Server {
                    status: status.as_u16(),
                    message: response.text().await.unwrap_or_default(),
                })
            }
        })
    }
}

fn transport(error: reqwest::Error) -> NetError {
    NetError::Transport(error.to_string())
}

/// The old `json<T>(res)`: decode a success; otherwise show `Room not found` for
/// `not_found`, else the body's `message`, else its `error`, else the raw body or status.
async fn decode<T: DeserializeOwned>(response: reqwest::Response) -> Result<T, NetError> {
    let status = response.status();
    let body = response.text().await.map_err(transport)?;
    if !status.is_success() {
        return Err(NetError::Server {
            status: status.as_u16(),
            message: error_message(status.as_u16(), &body),
        });
    }
    serde_json::from_str(&body).map_err(|e| NetError::Transport(format!("bad reply: {e}")))
}

pub(crate) fn error_message(status: u16, body: &str) -> String {
    let fallback = if body.is_empty() {
        status.to_string()
    } else {
        body.to_string()
    };
    let Ok(serde_json::Value::Object(parsed)) = serde_json::from_str(body) else {
        return fallback;
    };
    let text = |key: &str| {
        parsed
            .get(key)
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
    };
    if text("error") == Some("not_found") {
        "Room not found".into()
    } else if let Some(message) = text("message") {
        message.into()
    } else if let Some(error) = text("error") {
        error.into()
    } else {
        fallback
    }
}

#[cfg(test)]
mod tests {
    use super::error_message;

    #[test]
    fn error_messages_follow_the_old_client() {
        assert_eq!(
            error_message(404, r#"{"error":"not_found"}"#),
            "Room not found"
        );
        assert_eq!(error_message(400, r#"{"error":"full"}"#), "full");
        assert_eq!(
            error_message(500, r#"{"error":"x","message":"Boom"}"#),
            "Boom"
        );
        assert_eq!(error_message(502, "Bad gateway"), "Bad gateway");
        assert_eq!(error_message(503, ""), "503");
    }
}
