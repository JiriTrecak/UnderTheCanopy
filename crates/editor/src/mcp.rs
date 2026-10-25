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

/// The port the server listens on by default.
pub const PORT: u16 = 7431;

/// The port to serve on: `CANOPY_MCP_PORT` when set (several checkouts on one machine
/// each run their own server), otherwise [`PORT`].
pub fn port() -> u16 {
    std::env::var("CANOPY_MCP_PORT")
        .ok()
        .and_then(|p| p.parse().ok())
        .unwrap_or(PORT)
}

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
    Encounter(EncounterArgs),
    ImportStarcraft(ImportStarcraftArgs),
    /// The campaign tools (`crate::campaign::tools`).
    Campaign(crate::campaign::tools::Call),
    /// Every sound event of the checked content.
    SoundEvents,
    /// Sound authoring with ElevenLabs (`crate::sound`), answered when it finishes.
    Sound(crate::sound::Call),
    /// Shows a document in its editor, as a click in its library does.
    Open {
        document: String,
    },
    /// Runs a Combat Lab scenario headless (`canopy_sim::lab::Run`).
    LabRun(LabRunArgs),
    /// The repository's skills: the list, or one skill's text.
    Skill {
        name: Option<String>,
    },
    /// A file of the repository's docs or skills, read-only.
    Doc {
        path: String,
    },
}

/// A Combat Lab scenario run to its end (or `ticks`), with the lab's tuning switches.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct LabRunArgs {
    /// The scenario: its id (`tower-melee`) or its document (`@labs/tower-melee.lab.json`).
    /// Unsaved edits play.
    pub scenario: String,
    /// Ticks to run (40 a second); the scenario's whole duration when absent.
    pub ticks: Option<u32>,
    /// The melee approach: spots (the game's), direct or ring.
    pub approach: Option<String>,
    /// Fast melee units lead a fleeing target (lab only).
    pub lead: Option<bool>,
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
pub struct ImportStarcraftArgs {
    /// The StarCraft II map file (.SC2Map or a Battle.net cache .s2ma); omit to list the
    /// melee maps found on this machine.
    pub source: Option<String>,
    /// The new map's id: lowercase words joined by hyphens (`oxide`).
    pub id: Option<String>,
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
    /// Cells per side: a multiple of 32 from 128 to 1024. 512 by default.
    pub size: Option<u32>,
    /// One of the biomes: frozen-dream (the default) or underleaf.
    pub biome: Option<String>,
    /// The folder under `maps/`; `skirmish` by default.
    pub folder: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct MapArgs {
    /// The map: its id (`frozen-dream`) or its package path (`maps/skirmish/frozen-dream.utcm`).
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
    /// What the block editor's tools do (D80): `flat` (starts the map again as level-1
    /// ground), raise, lower, level, paint, face,
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

#[derive(Debug, Deserialize, JsonSchema)]
struct SkillArgs {
    /// The skill's name (`canopy-editor`); absent lists every skill with what it covers.
    name: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
struct DocArgs {
    /// A path from the repository's root: `AGENTS.md`, `docs/<name>.md`, or a file of a
    /// skill (`.claude/skills/<name>/<file>`).
    path: String,
}

#[derive(Clone)]
struct Handler {
    calls: mpsc::Sender<Call>,
}

impl Handler {
    async fn ask(&self, tool: Tool) -> Result<CallToolResult, ErrorData> {
        let cook = matches!(tool, Tool::Cook | Tool::Sound(_));
        let (reply, answer) = tokio::sync::oneshot::channel();
        if self.calls.send(Call { tool, reply }).is_err() {
            return Ok(failed("The editor is closed."));
        }
        // A cook may compile the cooker first; a take of music can take minutes.
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
        description = "Creates a map package made of flat blocks (level 1), with players 1 and 2 in opposite corners. Shape it with map_blocks and furnish it with map_place; its biome generates its look."
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
        description = "Edits a map's blocks (D80) as the block editor's tools do, one undo step: raise, lower, level, paint a kind, add or remove ramps and diagonal cuts over an area. Heights, cliffs, water and closed ground follow from the blocks; the world is generated from them."
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
        description = "Imports a StarCraft II melee map as a new block map (cliff levels, ramps, chasms as deep water, its playable area with a forest border, starts, amber nodes for mineral fields, substance deposits for geysers); one of its cells is two here. Without a source, lists the melee maps found on this machine (the game's Maps folder and the Battle.net cache). Written at once with its preview; returns what it did and the problems the check finds."
    )]
    async fn import_starcraft(
        &self,
        Parameters(args): Parameters<ImportStarcraftArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::ImportStarcraft(args)).await
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

    // --- Campaigns (docs/campaign.md, crate::campaign::tools) ------------------------

    #[tool(
        description = "Creates a campaign package campaigns/<id>.utcc: its package.json and a campaign.json for a race with its first chapter. Add steps with add_campaign_step."
    )]
    async fn create_campaign(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::CreateCampaign>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Create(args))).await
    }

    #[tool(
        description = "A campaign's steps in order: index, kind, id, title, a mission's map and what its tech makes new and blocks, and the campaign check's problems for it."
    )]
    async fn campaign_steps(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::CampaignArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Steps(args))).await
    }

    #[tool(
        description = "Adds a chapter, story, mission or cinematic step to a campaign, one undo step. A new mission gets its own map package of flat blocks in the campaign's missions/ unless it names one there."
    )]
    async fn add_campaign_step(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::AddStep>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Add(args))).await
    }

    #[tool(
        description = "Every sound event of the package (D97): id, category, clips, loop and its note. What units, buildings, races, rules, biomes, maps and campaigns name."
    )]
    async fn sound_events(&self) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::SoundEvents).await
    }

    #[tool(
        description = "Generates takes of a sound with ElevenLabs (D97; skill canopy-sounds): an effect or loop, music, a designed voice's previews, or a text library line spoken in a kept voice. Takes land in target/sound-takes/<name>/; audition them, then pick_sound (keep_voice for a voice). Costs ElevenLabs credits: generate only what Jiri asked for."
    )]
    async fn generate_sound(
        &self,
        Parameters(args): Parameters<crate::sound::GenerateSound>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Sound(crate::sound::Call::Generate(Box::new(args))))
            .await
    }

    #[tool(
        description = "Picks a take of a generated sound into its asset folder (audio, asset.json, generation.json) and creates or extends its sound event. Effects are stored as 16-bit WAV, music and lines as MP3 that streams."
    )]
    async fn pick_sound(
        &self,
        Parameters(args): Parameters<crate::sound::PickSound>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Sound(crate::sound::Call::Pick(args))).await
    }

    #[tool(
        description = "Keeps one preview of a designed voice as its speaker's voice (tools/sound/voices.json), for lines to be spoken in."
    )]
    async fn keep_voice(
        &self,
        Parameters(args): Parameters<crate::sound::KeepVoice>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Sound(crate::sound::Call::Keep(args))).await
    }

    #[tool(description = "Moves a campaign's step to another place in its list.")]
    async fn move_campaign_step(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::MoveStep>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Move(args))).await
    }

    #[tool(
        description = "Removes a step from a campaign (a mission's map stays in missions/, listed as not in the campaign)."
    )]
    async fn remove_campaign_step(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::StepArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Remove(args))).await
    }

    #[tool(
        description = "Sets fields of a campaign step (title, art, chapterLine, story, heroLevelCap, pages, note, music, map, id …) as one undo step; null removes a field."
    )]
    async fn set_campaign_step(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::SetStep>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Set(args))).await
    }

    #[tool(
        description = "Sets what a mission's tech makes new or blocks (units, buildings, research of the campaign's race), as the Tech view's clicks do, one undo step. Returns the mission's tech and the contradictions it leaves (something allowed whose building is blocked or locked)."
    )]
    async fn set_mission_tech(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::SetTech>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Tech(args))).await
    }

    #[tool(
        description = "The campaign race's tech tree in one mission, as the Tech view draws it: every building (by hall tier), unit and research with its state (locked, available, new, blocked), whether it can be reached, and the contradictions."
    )]
    async fn mission_tech_tree(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::StepArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Tree(args))).await
    }

    #[tool(
        description = "Sets a mission map's computer players and their campaign AI (behaviour, waves, skirmish limits), replacing its list, one undo step; the map gets its mission section when it has none. Returns what the format check says of them."
    )]
    async fn set_mission_players(
        &self,
        Parameters(args): Parameters<crate::campaign::tools::MissionPlayers>,
    ) -> Result<CallToolResult, ErrorData> {
        use crate::campaign::tools::Call;
        self.ask(Tool::Campaign(Call::Players(args))).await
    }

    #[tool(description = "Removes starts, entities and trees around a point, as one undo step.")]
    async fn map_remove(
        &self,
        Parameters(args): Parameters<MapRemoveArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::MapRemove(args)).await
    }

    #[tool(
        description = "Shows a document to the user in its editor (a unit, an ability, a map, a campaign, a lab scenario …), as a click in its library does: its tab opens and comes to the front."
    )]
    async fn open(
        &self,
        Parameters(args): Parameters<DocumentArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Open {
            document: args.document,
        })
        .await
    }

    #[tool(
        description = "Runs a Combat Lab scenario (labs/*.lab.json) with the real simulation and returns its measures per group: alive, arrivals, unit-seconds held up and the longest hold-up, course flips, damage taken, the first swing and the most in reach at once, the worst overlap of two bodies, and the step's cost. The lab's tuning switches (approach, lead) apply to this run only; matches always play the game's."
    )]
    async fn lab_run(
        &self,
        Parameters(args): Parameters<LabRunArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::LabRun(args)).await
    }

    #[tool(
        description = "The repository's skills: without a name, every skill with what it covers; with one, its SKILL.md and the other files in its folder. canopy-editor describes the editor's workbenches and these tools; the canopy-* skills describe the content formats."
    )]
    async fn read_skill(
        &self,
        Parameters(args): Parameters<SkillArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Skill { name: args.name }).await
    }

    #[tool(
        description = "Reads a file of the repository's documentation, read-only: AGENTS.md, docs/*.md (the plan, decisions, editor, campaign, story …) or a file of a skill's folder."
    )]
    async fn read_doc(
        &self,
        Parameters(args): Parameters<DocArgs>,
    ) -> Result<CallToolResult, ErrorData> {
        self.ask(Tool::Doc { path: args.path }).await
    }
}

/// Every tool's definition (name, description, input schema), as MCP lists them; the
/// assistant offers the same tools to Claude.
pub fn definitions() -> Vec<rmcp::model::Tool> {
    Handler::tool_router().list_all()
}

/// The tool call `name` with `args`, as the MCP handlers above build it (the assistant's
/// dispatch: one set of tools, one set of answers).
pub fn parse(name: &str, args: Value) -> Result<Tool, String> {
    use crate::campaign::tools::Call as Campaign;
    use crate::sound::Call as Sound;
    fn arg<T: serde::de::DeserializeOwned>(args: Value) -> Result<T, String> {
        let args = if args.is_null() {
            Value::Object(Default::default())
        } else {
            args
        };
        serde_json::from_value(args).map_err(|e| format!("invalid arguments: {e}"))
    }
    Ok(match name {
        "status" => Tool::Status,
        "list_elements" => Tool::List {
            editor: arg::<ListArgs>(args)?.editor,
        },
        "read_document" => Tool::Read {
            document: arg::<DocumentArgs>(args)?.document,
        },
        "document_schema" => Tool::Schema {
            document: arg::<DocumentArgs>(args)?.document,
        },
        "edit" => {
            let a: EditArgs = arg(args)?;
            Tool::Edit {
                label: a.label,
                commands: a.commands,
            }
        }
        "undo" => Tool::Undo,
        "redo" => Tool::Redo,
        "save" => Tool::Save {
            force: arg::<SaveArgs>(args)?.force,
        },
        "problems" => Tool::Problems,
        "find_uses" => Tool::Uses {
            id: arg::<UsesArgs>(args)?.id,
        },
        "rename_id" => {
            let a: RenameArgs = arg(args)?;
            Tool::Rename {
                document: a.document,
                new: a.new,
            }
        }
        "create_map" => Tool::MapCreate(arg(args)?),
        "map_info" => Tool::MapInfo {
            map: arg::<MapArgs>(args)?.map,
        },
        "map_picture" => {
            let a: MapPictureArgs = arg(args)?;
            Tool::MapPicture {
                map: a.map,
                scale: a.scale,
            }
        }
        "map_blocks" => Tool::MapBlocks(arg(args)?),
        "map_place" => Tool::MapPlace(arg(args)?),
        "map_remove" => Tool::MapRemove(arg(args)?),
        "unused_assets" => Tool::UnusedAssets,
        "import_asset" => Tool::Import(arg(args)?),
        "cook" => Tool::Cook,
        "map_preview" => Tool::MapPreview {
            map: arg::<MapArgs>(args)?.map,
        },
        "import_starcraft" => Tool::ImportStarcraft(arg(args)?),
        "encounter" => Tool::Encounter(arg(args)?),
        "create_campaign" => Tool::Campaign(Campaign::Create(arg(args)?)),
        "campaign_steps" => Tool::Campaign(Campaign::Steps(arg(args)?)),
        "add_campaign_step" => Tool::Campaign(Campaign::Add(arg(args)?)),
        "move_campaign_step" => Tool::Campaign(Campaign::Move(arg(args)?)),
        "remove_campaign_step" => Tool::Campaign(Campaign::Remove(arg(args)?)),
        "set_campaign_step" => Tool::Campaign(Campaign::Set(arg(args)?)),
        "set_mission_tech" => Tool::Campaign(Campaign::Tech(arg(args)?)),
        "mission_tech_tree" => Tool::Campaign(Campaign::Tree(arg(args)?)),
        "set_mission_players" => Tool::Campaign(Campaign::Players(arg(args)?)),
        "sound_events" => Tool::SoundEvents,
        "generate_sound" => Tool::Sound(Sound::Generate(Box::new(arg(args)?))),
        "pick_sound" => Tool::Sound(Sound::Pick(arg(args)?)),
        "keep_voice" => Tool::Sound(Sound::Keep(arg(args)?)),
        "open" => Tool::Open {
            document: arg::<DocumentArgs>(args)?.document,
        },
        "lab_run" => Tool::LabRun(arg(args)?),
        "read_skill" => Tool::Skill {
            name: arg::<SkillArgs>(args)?.name,
        },
        "read_doc" => Tool::Doc {
            path: arg::<DocArgs>(args)?.path,
        },
        other => return Err(format!("No tool {other}.")),
    })
}

/// Tools that write files at once or spend money: the assistant asks before running them.
pub fn needs_approval(name: &str, args: &Value) -> bool {
    match name {
        "save" | "cook" | "import_asset" | "map_preview" | "generate_sound" | "pick_sound"
        | "keep_voice" => true,
        // Listing the machine's maps writes nothing; importing one does.
        "import_starcraft" => args.get("source").is_some_and(|s| !s.is_null()),
        _ => false,
    }
}

#[tool_handler(
    name = "Under the Canopy editor",
    instructions = "Edits the game's base package (units, buildings, items, abilities, effects, races, rules, assets, maps) through the open editor. Find documents with list_elements, read them with read_document and their shape with document_schema, change them with edit (one undo step per call), check with problems, then save. Maps have their own tools: create_map, map_blocks (the block editor, D80), map_place, map_remove, map_info and map_picture. Campaigns too: create_campaign, campaign_steps, add_campaign_step, move_campaign_step, remove_campaign_step, set_campaign_step, set_mission_tech, mission_tech_tree and set_mission_players (docs/campaign.md). Sounds: sound_events, generate_sound, pick_sound and keep_voice (D97, skill canopy-sounds). The Combat Lab: lab_run (scenarios are documents under @labs/). open shows a document to the user. read_skill lists and reads the repository's skills (canopy-editor covers the editor and these tools; the canopy-* skills the formats); read_doc reads docs/*.md."
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
