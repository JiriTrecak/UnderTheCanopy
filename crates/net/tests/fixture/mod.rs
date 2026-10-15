//! Test-owned content and a two-player map for driving `canopy_sim::World` from the match
//! server's commits, and this repository's match server (`server/`) run locally. No shipped
//! content.
#![allow(dead_code)]

use canopy_content::{
    Content,
    map::UtcMap,
    placement::Owner,
    testing::{DataFolder, image, model},
};
use canopy_net::{Event, Net, ServerAddress, Session};
use canopy_sim::{Action, MatchSetup, World, action::CellPoint};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

pub const SIZE: u32 = 256;
const HALO: i32 = 16;

fn definition(id: &str, kind: &str, extra: Value) -> Value {
    let mut definition = json!({
        "id": id, "kind": kind, "name": id, "description": "",
        "asset": "asset.model", "icon": "asset.icon",
    });
    for (key, value) in extra.as_object().expect("object") {
        definition[key] = value.clone();
    }
    definition
}

fn game() -> Value {
    let body = json!({"maxHp": 100, "armor": 0, "armorType": "armor.plain"});
    let meta = json!({"name": "Command", "description": "", "icon": "asset.icon", "priority": 0});
    let actions: serde_json::Map<String, Value> = [
        "move",
        "attack",
        "stop",
        "hold",
        "patrol",
        "follow",
        "garrison",
        "unload",
        "build",
        "produce",
        "cancel",
        "rally",
        "pause",
        "upgrade",
        "cancelUpgrade",
        "research",
        "cancelResearch",
    ]
    .into_iter()
    .map(|name| (name.to_string(), meta.clone()))
    .collect();
    let walker = json!({
        "body": body, "supplyCost": 1,
        "dimensions": {"radius": 0.4, "height": 1.5, "formationSpacing": 1.0},
        "behaviors": {"playerControl": {}, "movement": {"speed": 4}},
    });
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {"armor.plain": {"name": "Plain", "icon": "asset.icon"}},
            "maxUnits": 400, "maxSupply": 400, "supplyIcon": "asset.icon", "maxBuildings": 100,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {}, "lootPools": {}, "armorCoefficient": 0.06,
            "damageTypes": {"damage.plain": {"name": "Plain", "appliesArmor": true}, "spell": {"name": "Spell", "appliesArmor": false}},
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [1], "heroRewards": [1], "neutralLevelCap": 2,
                "neutralMultipliersPermille": [500], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": [0],
            },
            "damageMultipliers": {"damage.plain": {"armor.plain": 1000}, "spell": {"armor.plain": 1000}},
            "startingSetup": {
                "id": "setup.test", "fort": "building.hall", "inventory": {},
                "units": [
                    {"definition": "unit.walker", "offset": {"x": 0, "y": 4}},
                    {"definition": "unit.walker", "offset": {"x": 2, "y": 4}},
                ],
            },
            "ai": {
                "revision": 1, "decisionTicks": 1, "economyTicks": 1, "strategyTicks": 1,
                "operationTicks": 1, "reactionTicks": 1, "orderIntervalTicks": 1,
                "workers": {"minimum": 1, "target": 2, "maximum": 3, "reserve": 0},
                "army": {"minimum": 0, "maximum": 1, "homeGuard": 0, "engagePermille": 0, "campPermille": 0, "retreatHealthPermille": 0, "pursuitRadius": 1},
                "limits": {"commandsPerBeat": 1, "placementCandidates": 1, "spellCandidates": 1},
                "composition": [], "skillPreference": [],
            },
        },
        "definitions": [
            definition("building.hall", "building", json!({
                "body": body, "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5},
            })),
            definition("unit.walker", "unit", walker),
        ],
    })
}

pub fn content() -> Arc<Content> {
    static CONTENT: OnceLock<Arc<Content>> = OnceLock::new();
    CONTENT
        .get_or_init(|| {
            let folder = DataFolder::new(game(), vec![image("asset.icon"), model("asset.model")]);
            Arc::new(
                folder
                    .load("net-lockstep")
                    .unwrap_or_else(|errors| panic!("{errors}")),
            )
        })
        .clone()
}

/// Standard base64 with padding.
fn base64(bytes: &[u8]) -> String {
    const SYMBOLS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = u32::from(chunk[0]) << 16
            | u32::from(*chunk.get(1).unwrap_or(&0)) << 8
            | u32::from(*chunk.get(2).unwrap_or(&0));
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(char::from(SYMBOLS[(word >> (18 - 6 * i) & 63) as usize]));
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn grid(value: f32) -> String {
    let vertices = SIZE as i32 + 2 * HALO + 1;
    let bytes: Vec<u8> = (0..vertices * vertices)
        .flat_map(|_| value.to_le_bytes())
        .collect();
    base64(&bytes)
}

/// A flat four-start skirmish map (two-player matches leave starts 3 and 4 empty).
pub fn map() -> &'static UtcMap {
    static MAP: OnceLock<UtcMap> = OnceLock::new();
    MAP.get_or_init(build_map)
}

fn build_map() -> UtcMap {
    let span = (SIZE + 2 * HALO as u32) as usize;
    let flow: Vec<u8> = (0..span * span).flat_map(|_| [128, 128, 0, 0]).collect();
    let terrain = json!({
        "version": 1, "size": SIZE,
        "heights": grid(0.0), "waterHeights": grid(-1.0),
        "waterFlow": base64(&flow), "waterProfiles": [],
        "grass": grid(0.0), "rock": grid(0.0), "paint": [],
    });
    let document = json!({
        "v": 3, "size": SIZE, "name": "Lockstep fixture", "stamps": [],
        "initialExploration": "explored",
        "playerStarts": [
            {"player": 1, "x": 17.5, "z": 17.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
            {"player": 2, "x": 49.5, "z": 49.5, "setup": "setup.test", "mainFort": "start.player.2/main-fort"},
            {"player": 3, "x": 17.5, "z": 49.5, "setup": "setup.test", "mainFort": "start.player.3/main-fort"},
            {"player": 4, "x": 49.5, "z": 17.5, "setup": "setup.test", "mainFort": "start.player.4/main-fort"},
        ],
        "entities": [], "camps": [],
        "authoring": {"version": 1, "terrain": terrain, "objects": []},
    });
    UtcMap::from_value(document, "lockstep.utcmap").unwrap_or_else(|errors| panic!("{errors}"))
}

/// The world for a match the server set up. The server's slots name its own races and
/// heroes (from the content it embeds); this fixture has none, so they are dropped. Seed,
/// players, kinds and names stay as the server froze them.
pub fn world(setup: &MatchSetup) -> World {
    let mut setup = setup.clone();
    for slot in &mut setup.slots {
        slot.race = None;
        slot.hero = None;
    }
    World::new(content(), map(), setup).unwrap_or_else(|e| panic!("{e}"))
}

/// A move of every unit `player` owns to `(x, y)`.
pub fn march(world: &World, player: u8, x: u32, y: u32) -> Action {
    let actors = world
        .state()
        .entities
        .iter()
        .filter(|e| e.owner == Owner::Player(player) && e.unit.is_some())
        .map(|e| e.id.0)
        .collect();
    Action::Move {
        actors,
        destination: CellPoint {
            x,
            y,
            surface: None,
        },
        attack_move: None,
        append: None,
    }
}

/// Unit positions of `player`, to see commands take effect.
pub fn positions(world: &World, player: u8) -> Vec<(i32, i32)> {
    world
        .state()
        .entities
        .iter()
        .filter(|e| e.owner == Owner::Player(player) && e.unit.is_some())
        .map(|e| (e.position.x, e.position.y))
        .collect()
}

/// Polls `session` until `found` picks an event, collecting everything seen.
pub fn wait_for<T>(
    session: &mut Session,
    seen: &mut Vec<Event>,
    what: &str,
    mut found: impl FnMut(&Event) -> Option<T>,
) -> T {
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        for event in session.poll() {
            let hit = found(&event);
            seen.push(event);
            if let Some(hit) = hit {
                return hit;
            }
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for {what}: {seen:#?}"
        );
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// The old repository, if it is checked out beside this one with its dependencies: its
/// TypeScript client still plays against our server in `old_client.rs` and `old_lockstep.rs`.
pub fn old_repository() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::env::var_os("CANOPY_OLD_REPO")
        .map(PathBuf::from)
        .into_iter()
        .chain([
            root.join("../Settlers 3 Web"),
            root.join("../../Settlers 3 Web"),
        ])
        .find(|candidate| candidate.join("src/net/lobby.ts").is_file() && tsx(candidate).is_file())
}

pub fn tsx(repository: &Path) -> PathBuf {
    repository.join("node_modules/tsx/package.json")
}

/// This repository's match server (`server/`, docs/server.md), from `crates/net` or `game/`.
pub fn server_folder() -> PathBuf {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR"));
    [manifest.join("../../server"), manifest.join("../server")]
        .into_iter()
        .find(|folder| folder.join("src/index.ts").is_file())
        .unwrap_or_else(|| manifest.join("../../server"))
}

/// The match server, run locally on a free port; killed on drop.
pub struct MatchServer {
    child: Child,
    pub address: ServerAddress,
}

impl MatchServer {
    /// Starts the server, or `None` (with a note) when node or the server's
    /// `node_modules` (`npm ci` in `server/`) are not available.
    pub fn start() -> Option<Self> {
        let folder = server_folder();
        if !tsx(&folder).is_file() {
            eprintln!("skipped: run `npm ci` in server/ to test against the match server");
            return None;
        }
        if Command::new("node").arg("--version").output().is_err() {
            eprintln!("skipped: node is not available");
            return None;
        }
        for _attempt in 0..3 {
            let port = TcpListener::bind("127.0.0.1:0")
                .and_then(|l| l.local_addr())
                .expect("free port")
                .port();
            // `node --import tsx` keeps the server in this one process (the tsx command
            // runs it in a child that a kill would orphan).
            let mut child = Command::new("node")
                .args(["--import", "tsx", "src/index.ts"])
                .current_dir(&folder)
                .env("PORT", port.to_string())
                .env("BIND", "127.0.0.1")
                .stdout(Stdio::piped())
                .stderr(Stdio::inherit())
                .spawn()
                .expect("start the match server");
            // The server prints `matchhost <version> <bind>:<port>` once listening.
            let stdout = child.stdout.take().expect("stdout");
            let (tx, rx) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let _ = tx.send(line);
                }
            });
            let listening = rx
                .recv_timeout(Duration::from_secs(30))
                .is_ok_and(|line| line.starts_with("matchhost"));
            let address = ServerAddress::parse(&format!("http://127.0.0.1:{port}")).unwrap();
            let healthy = listening
                && Net::new(address.clone())
                    .unwrap()
                    .health()
                    .wait()
                    .is_ok_and(|h| h.ok);
            if healthy {
                return Some(Self { child, address });
            }
            let _ = child.kill();
            let _ = child.wait();
        }
        panic!("the match server did not start");
    }

    pub fn net(&self) -> Net {
        Net::new(self.address.clone()).expect("network thread")
    }
}

impl Drop for MatchServer {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}
