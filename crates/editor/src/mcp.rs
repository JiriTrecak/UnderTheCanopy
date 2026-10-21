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
    MapBlocks(MapBlocksArgs),
    MapPlace(MapPlaceArgs),
    MapRemove(MapRemoveArgs),
    UnusedAssets,
    Import(ImportArgs),
    Cook,
    MapPreview {
        map: String,
    },
    ImportWarcraft(ImportWarcraftArgs),
    Encounter(EncounterArgs),
}

/// An ability's encounter, headless (the ability stage's take run to its end).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct EncounterArgs {
    /// The ability's id; its unsaved edits play.
    pub ability: String,
    /// The scene: enemy-single, enemy-group, ally-single, ally-group, fallen-allies,
    /// neutral-camp, empty or self. The ability's own suggestion when absent.
    pub preset: Option<String>,
    /// The unit that casts (a unit definition id); a unit that binds the ability when
    /// absent.
    pub caster: Option<String>,
    /// The unit every target is.
    pub target: Option<String>,
    /// How many targets, 0 to 8.
    pub targets: Option<u32>,
    /// ally, enemy, self or neutral (a camp).
    pub relation: Option<String>,
    /// Cells from the caster to the first target (1 to 30), and between the targets
    /// around it (1 to 8).
    pub distance: Option<f64>,
    pub spacing: Option<f64>,
    pub rank: Option<u32>,
    /// Health at the start, percent.
    pub caster_health: Option<u32>,
    pub target_health: Option<u32>,
    /// The caster's mana at the start (of 1000).
    pub mana: Option<u32>,
    /// Units keep their weapons, move and fight.
    pub combat: Option<bool>,
    /// The targets start as corpses.
    pub fallen: Option<bool>,
    /// Aim at this unit (an entity id from a previous answer's units) instead of the
    /// first target.
    pub unit: Option<u32>,
    /// Aim at this cell `[x, y]` instead of the first target (point abilities).
    pub point: Option<[u32; 2]>,
    /// Stop the caster at this tick (an interruption).
    pub interrupt_at: Option<u32>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ImportWarcraftArgs {
    /// The Warcraft III map file (.w3x or .w3m).
    pub source: String,
    /// The new map's id: lowercase words joined by hyphens (`twisted-meadows`).
    pub id: String,
    /// The folder under maps/; `skirmish` by default.
    pub folder: Option<String>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ImportArgs {
    /// The file to copy in: a glTF binary (.glb) for a model, a PNG for an icon or texture.
    pub source: String,
    /// The new asset's id, starting with `asset.` (`asset.models.units.example`).
    pub id: String,
    pub name: Option<String>,
    /// .glb: unit, creature, building, tree, foliage, prop or bridge. .png: icon,
    /// interface or texture.
    pub kind: String,
    /// The new folder below assets/ (`units/example`).
    pub folder: String,
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
    /// Cells per side: 256, 480, 512, 640, 800, 960 or 1024. 512 by default.
    pub size: Option<u32>,
    /// vibrant-forest (the default), deep-forest, autumn-forest, frozen-forest or frost-hollow.
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
pub struct MapBlocksArgs {
    /// The map: its id or its package path.
    pub map: String,
    /// What the block editor's tools do (D80): `flat` (blocks out the map as level-1
    /// ground), `blockout` (blocks from its heights), raise, lower, level, paint, face,
    /// unface, ramp, unramp, cut, uncut, cells, uncells, fill, round, unround.
    /// - raise, lower, level, paint, face, unface: the blocks of the box `from`–`to`, or
    ///   with `points`, those within `radius` blocks of that line.
    /// - ramp: from its top (`from`) to its foot (`to`), `radius` blocks to each side
    ///   (default 1); it falls one level that way. A diagonal ramp's teeth are cut.
    /// - round, unround: cuts (or uncuts) the plateaus' turning corners at 45°.
    /// - cells, uncells: kinds painted cell by cell within `radius` cells of `points`;
    ///   fill: the cells inside the outline `points`, painted `kind` (`none` clears).
    pub op: String,
    /// The area's corners in world cells (x, z); `to` defaults to `from`. Every block
    /// (4 × 4 cells) the rectangle touches is changed.
    pub from: Option<[f32; 2]>,
    pub to: Option<[f32; 2]>,
    /// A line or outline in world cells (x, z).
    pub points: Option<Vec<[f32; 2]>>,
    /// raise, lower, level, paint, face, round: the blocks inside the outline `points`
    /// instead of along it.
    pub fill: Option<bool>,
    /// Brush radius: blocks for block ops, cells for cells.
    pub radius: Option<f32>,
    /// level: the level (0–7; flat ground is 1, one level is 3 m); ramp: its top level.
    pub level: Option<u8>,
    /// paint, cells, fill: ground, wall (forest floor), rock, shallow, deep, ice (or none).
    pub kind: Option<String>,
    /// face: the biome's face style (1 onwards; 0 is the biome's cliff).
    pub style: Option<u8>,
    /// cut: the corner of the block at `to` nearest this world point is cut away.
    pub corner: Option<[f32; 2]>,
    /// Makes this and later edits in the mirror too, as the editor's Symmetry does:
    /// off, point (half a turn round the middle), x (left–right) or z (top–bottom).
    pub symmetry: Option<String>,
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
        let cook = matches!(tool, Tool::Cook);
        let (reply, answer) = tokio::sync::oneshot::channel();
        if self.calls.send(Call { tool, reply }).is_err() {
            return Ok(failed("The editor is closed."));
        }
        // A cook may compile the cooker first.
        let wait = if cook { 900 } else { 60 };
        match tokio::time::timeout(Duration::from_secs(wait), answer).await {
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
        description = "Creates a map package: flat dry ground at height 2 in grass, every ground material as a paint layer, and players 1 and 2 in opposite corners. Shape it in blocks with map_blocks (op flat first) and furnish it with map_place."
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
        description = "Edits a map's blocks (D80) as the block editor's tools do, one undo step: start from flat ground or block out an existing map, then raise, lower, level, paint a kind, add or remove ramps and diagonal cuts over an area. Heights, cliffs, water and closed ground follow from the blocks; the world is generated from them."
    )]
    async fn map_blocks(
        &self,
        Parameters(args): Parameters<MapBlocksArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapBlocks(args)).await
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

    #[tool(
        description = "Assets nothing names: no definition, ability, effect, scenery, biome, map, config file or code. Delete one with edit (delete its asset.json): its folder goes on save."
    )]
    async fn unused_assets(&self) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::UnusedAssets).await
    }

    #[tool(
        description = "Copies a glTF binary or a PNG in as a new asset folder with its asset.json (a model's clips and socket_* nodes filled in). Written at once; cook afterwards."
    )]
    async fn import_asset(
        &self,
        Parameters(args): Parameters<ImportArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Import(args)).await
    }

    #[tool(
        description = "Cooks the changed assets into the pack (as `just cook`), then the editor's viewports show them. Waits for the cook."
    )]
    async fn cook(&self) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Cook).await
    }

    #[tool(
        description = "Writes the map's preview for the map picker (preview.webp in its package, the illustrated atlas with amber, camps and starts) from the map as the editor holds it, and returns it as a PNG. Cook afterwards for the game to show it."
    )]
    async fn map_preview(
        &self,
        Parameters(args): Parameters<MapArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapPreview { map: args.map }).await
    }

    #[tool(
        description = "Imports a Warcraft III melee map as a new map package: terrain, cliffs, ramps and water at 5 cells a tile; its ground tiles substituted with the nearest ground this game has (in a biome picked by its tileset); trees as the biome's trees; gold mines as amber sites; creep groups as camps of the same strength; start locations as starts. Written at once with its preview; returns what it did and the problems the check finds."
    )]
    async fn import_warcraft(
        &self,
        Parameters(args): Parameters<ImportWarcraftArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::ImportWarcraft(args)).await
    }

    #[tool(
        description = "Casts an ability on test targets with the real simulation, as the editor's ability stage does, and runs the take to its end: free and ready at once, on a flat sandbox. Returns the settings used, every ability event (tick, event, amount, target, status), what the simulation refused, the cast's timeline (prepare, channel, recover, impacts, statuses) and each unit's health and mana at the end. Unsaved edits play (waits for the check)."
    )]
    async fn encounter(
        &self,
        Parameters(args): Parameters<EncounterArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Encounter(args)).await
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
    instructions = "Edits the game's base package (units, buildings, items, abilities, effects, races, rules, assets, maps) through the open editor. Find documents with list_elements, read them with read_document and their shape with document_schema, change them with edit (one undo step per call), check with problems, then save. Maps have their own tools: create_map, map_blocks (the block editor, D80), map_place, map_remove, map_info and map_picture. The canopy-* skills in the repository describe the formats."
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
