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
    MapCreate(MapCreateArgs),
    MapInfo {
        map: String,
    },
    MapPicture {
        map: String,
        scale: Option<u32>,
    },
    MapStroke(MapStrokeArgs),
    MapPlace(MapPlaceArgs),
    MapRemove(MapRemoveArgs),
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

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MapCreateArgs {
    /// The map's id: lowercase words joined by hyphens (`amber-falls`).
    pub id: String,
    /// Its name in the game's map list.
    pub name: Option<String>,
    /// Cells per side: 256, 512, 640 or 1024. 512 by default.
    pub size: Option<u32>,
    /// vibrant-forest (the default), deep-forest, autumn-forest or frozen-forest.
    pub biome: Option<String>,
    /// The folder under `maps/`; `skirmish` by default.
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MapArgs {
    /// The map: its id (`echo-isles`) or its package path (`maps/skirmish/echo-isles.utcm`).
    map: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MapPictureArgs {
    /// The map: its id or its package path.
    map: String,
    /// Pixels per cell, 1 to 4 (default: about 1024 pixels across).
    scale: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MapStrokeArgs {
    /// The map: its id or its package path.
    pub map: String,
    /// raise, lower, flatten, smooth, cliff, ramp, paint, water, trees or props. A ramp runs
    /// straight from the first point to the last (from the ground height there to the
    /// height at the last point, or `level`), twice the radius wide.
    pub tool: String,
    /// The stroke's line in world cells (x, z), from (0, 0) at the north-west corner to
    /// (size, size). One point dabs once.
    pub points: Vec<[f32; 2]>,
    /// Brush radius in cells (default 6).
    pub radius: Option<f32>,
    /// Brush strength 0 to 1 (default 0.5): how fast raise, lower, smooth and paint act.
    pub strength: Option<f32>,
    /// flatten and cliff: the height to level to (ground is -16 to 24; 2 is a new map's
    /// ground; one cliff level is 3). water: the surface height to flood to. Without it,
    /// flatten keeps the first point's height and cliff lifts one level.
    pub level: Option<f32>,
    /// The opposite action: lower for raise, a level down for cliff, drain for water,
    /// clear for trees.
    pub lower: Option<bool>,
    /// How many times the brush goes over the line (default 1).
    pub passes: Option<u32>,
    /// paint: soil, grass, rock or a paint material of the map (map_info lists them).
    pub ground: Option<String>,
    /// trees: the scenery to plant (map_info lists the trees); trees per 100 cells
    /// (default 6).
    pub tree: Option<String>,
    pub density: Option<f32>,
    /// props: the decoration to scatter (map_info lists the props); spaced wider than
    /// trees, at `density` per 100 cells.
    pub prop: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MapPlaceArgs {
    /// The map: its id or its package path.
    pub map: String,
    /// amber (a site of five crystals facing the nearest start), camp (creeps guarding
    /// a point), or start (the next player's start, its amber 26 cells towards the middle
    /// and a level pad).
    pub kind: String,
    pub x: f64,
    pub z: f64,
    /// camp: easy, medium or hard.
    pub tier: Option<String>,
    /// amber and start: amber per crystal (default 2500).
    pub amount: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct MapRemoveArgs {
    /// The map: its id or its package path.
    pub map: String,
    pub x: f64,
    pub z: f64,
    /// Cells around the point.
    pub radius: f64,
    /// Any of starts, entities (amber, camps: a camp goes whole) and trees (scenery
    /// objects). All of them when absent.
    pub what: Option<Vec<String>>,
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
            Ok(Ok(Ok(Value::Object(image)))) if image.contains_key("png") => {
                let data = image["png"].as_str().unwrap_or_default().to_string();
                Ok(CallToolResult::success(vec![ContentBlock::image(
                    data,
                    "image/png",
                )]))
            }
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

    #[tool(
        description = "Creates a map package: flat dry ground at height 2 in grass, every ground material as a paint layer, and players 1 and 2 in opposite corners. Shape it with map_stroke and furnish it with map_place."
    )]
    async fn create_map(
        &self,
        Parameters(args): Parameters<MapCreateArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapCreate(args)).await
    }

    #[tool(
        description = "A map's layout: size, biome, starts, amber sites, camps, tree count, paint materials and trees it can use, a coarse height and water grid, and the check's problems for it."
    )]
    async fn map_info(
        &self,
        Parameters(args): Parameters<MapArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapInfo { map: args.map }).await
    }

    #[tool(
        description = "The map from straight above as a PNG: ground shaded by height and slope, water blue by depth, cliffs dark, trees dark green, amber orange, camps red with their aggro ring, starts in player colours. North (z = 0) is up."
    )]
    async fn map_picture(
        &self,
        Parameters(args): Parameters<MapPictureArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapPicture {
            map: args.map,
            scale: args.scale,
        })
        .await
    }

    #[tool(
        description = "One brush stroke along a line as one undo step, as the editor's brushes do it: raise, lower, flatten, smooth, cliff (hard steps of one level), ramp (a straight walkable slope between two heights), paint, water, trees and props (decoration that blocks nothing)."
    )]
    async fn map_stroke(
        &self,
        Parameters(args): Parameters<MapStrokeArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapStroke(args)).await
    }

    #[tool(
        description = "Places what a click of the editor's tools places: an amber site, a creep camp or a player start (with its amber and a level pad), as one undo step."
    )]
    async fn map_place(
        &self,
        Parameters(args): Parameters<MapPlaceArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapPlace(args)).await
    }

    #[tool(description = "Removes starts, entities and trees around a point, as one undo step.")]
    async fn map_remove(
        &self,
        Parameters(args): Parameters<MapRemoveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapRemove(args)).await
    }
}

#[tool_handler(
    name = "Under the Canopy editor",
    instructions = "Edits the game's base package (units, buildings, items, abilities, effects, races, rules, assets, maps) through the open editor. Find documents with list_elements, read them with read_document and their shape with document_schema, change them with edit (one undo step per call), check with problems, then save. Maps have their own tools: create_map, map_stroke, map_place, map_remove, map_info and map_picture. The canopy-* skills in the repository describe the formats."
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
