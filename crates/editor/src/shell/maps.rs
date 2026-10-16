//! The shell's answers to MCP's map tools: the same placing, brushes and documents as the
//! map view, on the map's session, each as one undo step.

use super::*;
use crate::map::tools::{Ground, Tier, Tool as MapTool, Tools};
use crate::map::view::{self as map_view, Docs};
use crate::mcp::{MapCreateArgs, MapPlaceArgs, MapStrokeArgs, Tool};
use serde_json::json;

impl Shell {
    pub(super) fn answer_map(&mut self, tool: Tool) -> Result<Value, String> {
        match tool {
            Tool::MapCreate(args) => self.create_map(args),
            Tool::MapInfo { map } => self.map_info(&map),
            Tool::MapPicture { map, scale } => {
                let root = self.map_root(&map)?;
                let map = self.loaded_map(&root)?;
                let scale = scale.unwrap_or((1024 / map.size).max(1)).clamp(1, 4);
                let png = crate::map::picture::png(map, scale)?;
                use base64::Engine;
                Ok(json!({ "png": base64::engine::general_purpose::STANDARD.encode(png) }))
            }
            Tool::MapStroke(args) => self.map_stroke(args),
            Tool::MapPlace(args) => self.map_place(args),
            Tool::MapRemove(args) => {
                let root = self.map_root(&args.map)?;
                self.loaded_map(&root)?;
                let docs = self.map_docs(&root)?;
                let what: Vec<&str> = match &args.what {
                    Some(what) => what.iter().map(String::as_str).collect(),
                    None => vec!["starts", "entities", "trees"],
                };
                let session = self.maps.get(&root).expect("loaded");
                match map_view::remove_area(session, &docs, args.x, args.z, args.radius, &what) {
                    Some(edit) => self.run_map_edit(&root, edit, false),
                    None => Ok(json!({ "removed": 0 })),
                }
            }
            _ => Err("not a map tool".into()),
        }
    }

    /// The package path of the map an agent named by id or path.
    fn map_root(&self, name: &str) -> Result<String, String> {
        let name = name.trim_end_matches('/').trim_end_matches("/map.json");
        let workspace = self.workspaces.get(&Kind::Maps).ok_or("No maps library")?;
        let mut elements = Vec::new();
        workspace
            .library
            .nodes
            .iter()
            .for_each(|n| n.elements(&mut elements));
        elements
            .iter()
            .find(|e| e.id == name || e.path == name)
            .map(|e| e.path.clone())
            .ok_or_else(|| format!("No map {name}; list_elements with editor maps lists them"))
    }

    /// The map at `root`, loaded into its session.
    fn loaded_map(&mut self, root: &str) -> Result<&canopy_content::map::UtcMap, String> {
        self.load_map(root);
        let session = self.maps.get(root).ok_or("The map has no session")?;
        match (&session.map, &session.error) {
            (Some(map), _) => Ok(map),
            (None, Some(error)) => Err(error.clone()),
            (None, None) => Err("The map does not load".into()),
        }
    }

    fn map_docs(&mut self, root: &str) -> Result<Docs, String> {
        let store = self.store.as_mut().ok_or("No package")?;
        let mut get = |file: &str| {
            store
                .get(&format!("{root}/{file}"))
                .ok()
                .flatten()
                .cloned()
                .ok_or_else(|| format!("{root}/{file} does not read"))
        };
        Ok(Docs {
            map: get("map.json")?,
            scenery: get("scenery.json")?,
        })
    }

    fn content(&self) -> Result<Arc<canopy_content::Content>, String> {
        self.checker
            .as_ref()
            .and_then(|c| c.content().cloned())
            .ok_or_else(|| "The package has not passed a check yet; see problems".into())
    }

    /// Runs a map step; `own` when the session already shows it (strokes).
    fn run_map_edit(&mut self, root: &str, edit: Edit, own: bool) -> Result<Value, String> {
        let label = edit.label.clone();
        let commands = edit.commands.len();
        let store = self.store.as_mut().ok_or("No package")?;
        let result = store.run_merging(&edit.label, edit.context, edit.commands, None);
        if let Err(error) = result {
            // The session may hold a change the store refused: load it again.
            if let Some(session) = self.maps.get_mut(root) {
                session.own = false;
                session.loaded_at = None;
            }
            return Err(error.to_string());
        }
        if !own && let Some(session) = self.maps.get_mut(root) {
            session.own = false;
        }
        Ok(json!({ "done": label, "commands": commands }))
    }

    fn create_map(&mut self, args: MapCreateArgs) -> Result<Value, String> {
        let content = self.content()?;
        let folder = args.folder.unwrap_or_else(|| "skirmish".into());
        let folder = folder.trim_matches('/');
        let root = format!("maps/{folder}/{}.utcm", args.id);
        let store = self.store.as_mut().ok_or("No package")?;
        if matches!(store.get(&format!("{root}/map.json")), Ok(Some(_))) {
            return Err(format!("{root} already exists"));
        }
        let version = self
            .package
            .as_ref()
            .map(|p| p.manifest.version.to_string())
            .map_err(Clone::clone)?;
        let name = args.name.unwrap_or_else(|| args.id.clone());
        let biome = args.biome.unwrap_or_else(|| "vibrant-forest".into());
        let commands = crate::map::new_map(
            &content,
            &version,
            folder,
            &args.id,
            &name,
            args.size.unwrap_or(512),
            &biome,
        )?;
        let context = Context {
            editor: Kind::Maps,
            document: format!("{root}/map.json"),
            pointer: None,
        };
        store
            .run(&format!("Create map {}", args.id), context, commands)
            .map_err(|e| e.to_string())?;
        if let Ok(package) = self.package.clone() {
            self.refresh_libraries(&package);
        }
        Ok(json!({ "created": root }))
    }

    fn map_stroke(&mut self, args: MapStrokeArgs) -> Result<Value, String> {
        let content = self.content()?;
        let root = self.map_root(&args.map)?;
        let map = self.loaded_map(&root)?;
        let tool = match args.tool.to_lowercase().as_str() {
            "raise" => MapTool::Raise,
            "lower" => MapTool::Lower,
            "flatten" => MapTool::Flatten,
            "smooth" => MapTool::Smooth,
            "cliff" => MapTool::Cliff,
            "ramp" => MapTool::Ramp,
            "paint" => MapTool::Paint,
            "water" => MapTool::Water,
            "trees" => MapTool::Trees,
            other => {
                return Err(format!(
                    "No brush {other}: raise, lower, flatten, smooth, cliff, paint, water or trees"
                ));
            }
        };
        let ground = match args.ground.as_deref().unwrap_or("grass") {
            "soil" => Ground::Soil,
            "grass" => Ground::Grass,
            "rock" => Ground::Rock,
            material => Ground::Paint(
                map.authoring
                    .terrain
                    .paint
                    .iter()
                    .position(|p| {
                        p.material == material || p.material.ends_with(&format!(".{material}"))
                    })
                    .ok_or_else(|| format!("The map has no paint material {material}"))?,
            ),
        };
        let mut settings = Tools {
            tool,
            ground,
            ..Tools::default()
        };
        if let Some(radius) = args.radius {
            settings.radius = radius.clamp(0.5, 64.0);
        }
        if let Some(strength) = args.strength {
            settings.strength = strength.clamp(0.0, 1.0);
        }
        if let Some(density) = args.density {
            settings.density = density.clamp(0.5, 40.0);
        }
        if let Some(tree) = args.tree {
            settings.tree = if tree.starts_with("scenery.") {
                tree
            } else {
                format!("scenery.{tree}")
            };
            if !crate::map::tools::tree_kinds(&content).contains(&settings.tree) {
                return Err(format!(
                    "{} is not a tree the package has; map_info lists them",
                    settings.tree
                ));
            }
        }
        let docs = self.map_docs(&root)?;
        let session = self.maps.get_mut(&root).expect("loaded");
        let edit = map_view::stroke(
            session,
            &content,
            &docs,
            settings,
            &args.points,
            args.level,
            args.lower.unwrap_or(false),
            args.passes.unwrap_or(1).min(50),
        )?;
        match edit {
            Some(edit) => self.run_map_edit(&root, edit, true),
            None => Ok(json!({ "done": "nothing changed" })),
        }
    }

    fn map_place(&mut self, args: MapPlaceArgs) -> Result<Value, String> {
        let content = self.content()?;
        let root = self.map_root(&args.map)?;
        self.loaded_map(&root)?;
        let tool = match args.kind.to_lowercase().as_str() {
            "amber" => MapTool::Amber,
            "camp" => MapTool::Camp,
            "start" => MapTool::Start,
            other => return Err(format!("Nothing places {other}: amber, camp or start")),
        };
        let docs = self.map_docs(&root)?;
        let session = self.maps.get_mut(&root).expect("loaded");
        if let Some(tier) = &args.tier {
            session.tools.tier = Tier::ALL
                .into_iter()
                .find(|t| t.name() == tier.as_str())
                .ok_or_else(|| format!("No tier {tier}: easy, medium or hard"))?;
        }
        if let Some(amount) = args.amount {
            session.tools.amber = amount.clamp(100, 100_000);
        }
        let edit = map_view::place(session, &content, &docs, tool, args.x, args.z)?;
        match edit {
            Some(edit) => self.run_map_edit(&root, edit, false),
            None => Ok(json!({ "done": "nothing placed" })),
        }
    }

    fn map_info(&mut self, name: &str) -> Result<Value, String> {
        let root = self.map_root(name)?;
        let content = self.content().ok();
        let problems: Vec<Value> = self
            .checker
            .as_ref()
            .and_then(|c| c.report())
            .map(|r| {
                r.problems
                    .iter()
                    .filter(|p| {
                        p.file.starts_with(&root)
                            || p.document.as_deref().is_some_and(|d| d.starts_with(&root))
                    })
                    .map(|p| json!({ "location": p.location, "message": p.message }))
                    .collect()
            })
            .unwrap_or_default();
        let map = self.loaded_map(&root)?;
        let terrain = map.authoring.terrain.decode().map_err(|e| e.to_string())?;
        let side = terrain.vertices as usize;
        let halo = canopy_content::map::MAP_HALO as usize;
        let step = (map.size / 32).max(1) as usize;
        let rows: Vec<String> = (0..map.size as usize)
            .step_by(step)
            .map(|z| {
                (0..map.size as usize)
                    .step_by(step)
                    .map(|x| {
                        let i = (z + halo) * side + x + halo;
                        let (h, w) = (terrain.heights[i], terrain.water_heights[i]);
                        if w > h + 0.05 {
                            "  ~".to_string()
                        } else {
                            format!("{:>3}", h.round() as i32)
                        }
                    })
                    .collect::<String>()
            })
            .collect();
        // What ground units of player 1 cannot walk to.
        let mut unreachable = Vec::new();
        if let (Some(content), Some(first)) = (&content, map.player_starts.first())
            && let Ok(terrain) = canopy_sim::terrain::Terrain::compile(map, content)
        {
            let reached = canopy_sim::spawn::reachable_from(&terrain, first.x, first.z);
            let near = |x: f64, z: f64| canopy_sim::spawn::near(&terrain, &reached, x, z, 7);
            for s in &map.player_starts {
                if !near(s.x, s.z) {
                    unreachable.push(format!("start {}", s.player));
                }
            }
            for e in &map.entities {
                if !near(e.position.x, e.position.y) {
                    unreachable.push(e.id.clone());
                }
            }
        }
        let member: std::collections::HashSet<&str> = map
            .camps
            .iter()
            .flat_map(|c| c.members.iter().map(String::as_str))
            .collect();
        let amber: Vec<Value> = map
            .entities
            .iter()
            .filter(|e| !member.contains(e.id.as_str()))
            .map(|e| json!({ "id": e.id, "definition": e.definition.as_str(), "x": e.position.x, "z": e.position.y }))
            .collect();
        Ok(json!({
            "map": root,
            "name": map.name,
            "size": map.size,
            "biome": map.biome(),
            "starts": map.player_starts.iter().map(|s| json!({"player": s.player, "x": s.x, "z": s.z})).collect::<Vec<_>>(),
            "neutralEntities": amber,
            "camps": map.camps.iter().map(|c| json!({"id": c.id, "x": c.home.x, "z": c.home.y, "members": c.members.len(), "loot": c.loot_pool.as_ref().map(|l| l.as_str().to_string())})).collect::<Vec<_>>(),
            "trees": map.authoring.objects.len(),
            "paintMaterials": map.authoring.terrain.paint.iter().map(|p| p.material.clone()).collect::<Vec<_>>(),
            "treeKinds": content.as_deref().map(crate::map::tools::tree_kinds).unwrap_or_default(),
            "heights": {
                "every": step,
                "note": "rows run north (z = 0) to south, columns west to east; ~ is water",
                "rows": rows,
            },
            "unreachableFromPlayer1": unreachable,
            "problems": problems,
        }))
    }
}
