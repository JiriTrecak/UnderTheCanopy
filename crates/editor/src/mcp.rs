//! The editor's MCP server (docs/editor.md): agents edit the package through the same
//! commands as the editor's own controls, so their changes share the undo history, show
//! at once and save with Cmd+S. `rmcp`'s Streamable HTTP service runs on a thread of its
//! own on loopback (`127.0.0.1`); `rmcp` checks the Host header (loopback only) and every
//! present Origin is refused, and requests must carry the per-install token. Tool calls
//! travel to the editor's thread as [`Call`]s and come back with their answer.

use crate::document::Command;
use bytes::Bytes;
use http_body_util::{BodyExt, Full};
use rmcp::{
    ErrorData, ServerHandler,
    handler::server::wrapper::Parameters,
    model::{CallToolResult, ContentBlock},
    tool, tool_handler, tool_router,
    transport::{
        StreamableHttpServerConfig, StreamableHttpService,
        streamable_http_server::session::local::LocalSessionManager,
    },
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::sync::{Arc, mpsc};
use std::time::Duration;

/// The port the server listens on.
pub const PORT: u16 = 7431;

/// What an agent asked for.
#[derive(Debug, Clone)]
pub enum Tool {
    Status,
    List {
        editor: Option<String>,
    },
    Read {
        document: String,
    },
    Schema {
        document: String,
    },
    Edit {
        label: String,
        commands: Vec<Command>,
    },
    Undo,
    Redo,
    Save {
        force: bool,
    },
    Problems,
    Uses {
        id: String,
    },
    Rename {
        document: String,
        new: String,
    },
}

/// A tool call on its way to the editor, with where its answer goes.
pub struct Call {
    pub tool: Tool,
    pub reply: tokio::sync::oneshot::Sender<Result<Value, String>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct ListArgs {
    /// One editor's elements: maps, units, abilities, effects, buildings, races or assets.
    /// All when absent.
    editor: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DocumentArgs {
    /// The document's path from the package root, as `list_elements` gives it
    /// (`units/ants/warrior.json`).
    document: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct EditArgs {
    /// What the step does, in a few words; undo shows it.
    label: String,
    /// The changes, applied in order as one undo step: all or nothing. Pointers are JSON
    /// pointers into the document (`/ranks/0/damage`; `""` is the whole document).
    commands: Vec<Command>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct SaveArgs {
    /// Save even when the check finds problems.
    #[serde(default)]
    force: bool,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct UsesArgs {
    /// The id to look for.
    id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct RenameArgs {
    /// The document of the element to rename.
    document: String,
    /// Its new id.
    new: String,
}

#[derive(Clone)]
struct Handler {
    calls: mpsc::Sender<Call>,
}

impl Handler {
    async fn ask(&self, tool: Tool) -> Result<CallToolResult, ErrorData> {
        let (reply, answer) = tokio::sync::oneshot::channel();
        if self.calls.send(Call { tool, reply }).is_err() {
            return Ok(failed("The editor is closed."));
        }
        match tokio::time::timeout(Duration::from_secs(60), answer).await {
            Ok(Ok(Ok(value))) => Ok(CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string_pretty(&value).unwrap_or_default(),
            )])),
            Ok(Ok(Err(message))) => Ok(failed(&message)),
            Ok(Err(_)) => Ok(failed("The editor dropped the call.")),
            Err(_) => Ok(failed("The editor did not answer in time.")),
        }
    }
}

fn failed(message: &str) -> CallToolResult {
    CallToolResult::error(vec![ContentBlock::text(message.to_string())])
}

#[tool_router]
impl Handler {
    #[tool(
        description = "The editor's state: the package, unsaved documents, what undo and redo would do, and the problems the last check found."
    )]
    async fn status(&self) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Status).await
    }

    #[tool(
        description = "Every element of the package (or of one editor): its editor, id, name and document path."
    )]
    async fn list_elements(
        &self,
        Parameters(args): Parameters<ListArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::List {
            editor: args.editor,
        })
        .await
    }

    #[tool(
        description = "A document's current value as the editor holds it (unsaved edits included), and whether it has unsaved edits."
    )]
    async fn read_document(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Read {
            document: args.document,
        })
        .await
    }

    #[tool(
        description = "The JSON Schema of a document as the package stores it (null for kinds of document without one). Fields with format canopy-id name another element."
    )]
    async fn document_schema(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Schema {
            document: args.document,
        })
        .await
    }

    #[tool(
        description = "Changes documents as one undo step: set, insert, remove, create or delete. Edits stay in the editor until save; the loader checks the package after every step (see problems)."
    )]
    async fn edit(
        &self,
        Parameters(args): Parameters<EditArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Edit {
            label: args.label,
            commands: args.commands,
        })
        .await
    }

    #[tool(description = "Takes back the last step (whoever made it).")]
    async fn undo(&self) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Undo).await
    }

    #[tool(description = "Does the last undone step again.")]
    async fn redo(&self) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Redo).await
    }

    #[tool(
        description = "Writes every changed document to the package's files. Refused while the check finds problems, unless force is true."
    )]
    async fn save(
        &self,
        Parameters(args): Parameters<SaveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Save { force: args.force }).await
    }

    #[tool(
        description = "Waits for the loader's check of the latest edits and lists its problems: file, location and message."
    )]
    async fn problems(&self) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Problems).await
    }

    #[tool(description = "Every use of an id in the package: document and JSON pointer.")]
    async fn find_uses(
        &self,
        Parameters(args): Parameters<UsesArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Uses { id: args.id }).await
    }

    #[tool(
        description = "Renames an element's id and every use of it across the package, in one undo step."
    )]
    async fn rename_id(
        &self,
        Parameters(args): Parameters<RenameArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Rename {
            document: args.document,
            new: args.new,
        })
        .await
    }
}

#[tool_handler(
    name = "Under the Canopy editor",
    instructions = "Edits the game's base package (units, buildings, items, abilities, effects, races, rules, assets, maps) through the open editor. Find documents with list_elements, read them with read_document and their shape with document_schema, change them with edit (one undo step per call), check with problems, then save. The canopy-* skills in the repository describe the formats."
)]
impl ServerHandler for Handler {}

/// A running server; dropping it stops it.
pub struct Server {
    pub port: u16,
    cancel: tokio_util::sync::CancellationToken,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

/// The command that adds the server to Claude Code.
pub fn claude_command(port: u16, token: &str) -> String {
    format!(
        "claude mcp add --transport http canopy http://127.0.0.1:{port}/mcp --header \"Authorization: Bearer {token}\""
    )
}

/// Starts the server on `port`; tool calls go to `calls`.
pub fn start(port: u16, token: String, calls: mpsc::Sender<Call>) -> std::io::Result<Server> {
    let listener = std::net::TcpListener::bind(("127.0.0.1", port))?;
    listener.set_nonblocking(true)?;
    let config = StreamableHttpServerConfig::default().enforce_origin_validation();
    let cancel = config.cancellation_token.clone();
    let stop = cancel.clone();
    let thread = std::thread::Builder::new()
        .name("editor mcp".into())
        .spawn(move || {
            let runtime = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(runtime) => runtime,
                Err(error) => {
                    tracing::warn!(%error, "editor: MCP runtime");
                    return;
                }
            };
            runtime.block_on(serve(listener, token, calls, config, stop));
        })?;
    tracing::info!(port, "editor: MCP server started");
    Ok(Server {
        port,
        cancel,
        thread: Some(thread),
    })
}

async fn serve(
    listener: std::net::TcpListener,
    token: String,
    calls: mpsc::Sender<Call>,
    config: StreamableHttpServerConfig,
    stop: tokio_util::sync::CancellationToken,
) {
    let Ok(listener) = tokio::net::TcpListener::from_std(listener) else {
        return;
    };
    let service = StreamableHttpService::new(
        move || {
            Ok(Handler {
                calls: calls.clone(),
            })
        },
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let expected = Arc::new(format!("Bearer {token}"));
    loop {
        let stream = tokio::select! {
            _ = stop.cancelled() => return,
            accepted = listener.accept() => match accepted {
                Ok((stream, _)) => stream,
                Err(_) => continue,
            },
        };
        let service = service.clone();
        let expected = expected.clone();
        let stop = stop.clone();
        tokio::spawn(async move {
            let handle =
                hyper::service::service_fn(move |request: http::Request<hyper::body::Incoming>| {
                    let mut service = service.clone();
                    let expected = expected.clone();
                    async move {
                        let authorized = request
                            .headers()
                            .get(http::header::AUTHORIZATION)
                            .and_then(|v| v.to_str().ok())
                            == Some(expected.as_str());
                        if request.uri().path() != "/mcp" {
                            return Ok::<_, std::convert::Infallible>(plain(
                                http::StatusCode::NOT_FOUND,
                                "not found",
                            ));
                        }
                        if !authorized {
                            return Ok(plain(
                                http::StatusCode::UNAUTHORIZED,
                                "the editor's token is required",
                            ));
                        }
                        let response = tower_service::Service::call(&mut service, request).await;
                        Ok(response.unwrap_or_else(|never| match never {}))
                    }
                });
            let connection = hyper::server::conn::http1::Builder::new()
                .serve_connection(hyper_util::rt::TokioIo::new(stream), handle);
            tokio::select! {
                _ = connection => {}
                _ = stop.cancelled() => {}
            }
        });
    }
}

type Body = http_body_util::combinators::BoxBody<Bytes, std::convert::Infallible>;

fn plain(status: http::StatusCode, text: &'static str) -> http::Response<Body> {
    let mut response = http::Response::new(Full::new(Bytes::from(text)).boxed());
    *response.status_mut() = status;
    response
}
