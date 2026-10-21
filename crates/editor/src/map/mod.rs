//! The map editor (docs/editor.md, M3): an open map is a [`Session`] over the map
//! package's documents and grids in the store. The viewport draws an editor [`Scene`]
//! built from the map (terrain, water, the forest around it, trees and props, mines,
//! creeps and start halls); brushes change the grids directly, and a stroke becomes one
//! undo step of grid patches when it ends.

pub mod picture;
pub mod preview;
pub mod tools;
pub mod view;

use canopy_assets::Pack;
use canopy_content::{Content, map::UtcMap, placement::Owner};
use canopy_render::{World, actors::ActorInstance, statics::StaticInstance};
use canopy_scene::Look;
use canopy_sim::terrain::field::HeightField;
use glam::{Mat4, Quat, Vec3};
use std::collections::BTreeMap;
use std::sync::Arc;

/// What the viewport draws of a map.
pub struct Scene {
    pub world: World,
    /// Mines, creeps, start halls (drawn as models; `world.actor_models` lists them).
    pub actors: Vec<ActorInstance>,
    pub biome: String,
    pub hour: f32,
}

/// The player colours of `look`, linear.
fn colors(look: &Look) -> Vec<[f32; 3]> {
    look.players
        .colors
        .iter()
        .map(|c| c.linear().to_array())
        .collect()
}

/// Builds the editor's scene of `map` as `view` shows it: its blocks (D80, when it has
/// them), the world generated from them (or its terrain, water and scenery), or where
/// ground units can walk, cell by cell.
pub fn build(
    map: &UtcMap,
    content: &Content,
    pack: &Pack,
    look: &Look,
    view: crate::world::View,
) -> Result<Scene, String> {
    use crate::world::View;
    let blockout = match view {
        View::Blocks | View::Split => {
            canopy_content::map::blocks::Blocks::of(&map.authoring.terrain)
                .and_then(Result::ok)
                .map(|b| crate::world::mesh::mesh(&b))
        }
        View::Generated => None,
        View::Pathing => {
            let terrain = canopy_sim::terrain::Terrain::compile(map, content)
                .map_err(|e| format!("{e:?}"))?;
            Some(crate::world::mesh::pathing(&terrain))
        }
    };
    // Otherwise the world generated from the blocks, as the game builds it (D80).
    let generated = blockout
        .is_none()
        .then(|| canopy_scene::generate::generate(map, &look.biome))
        .flatten();
    let ground_map = generated.as_ref().map_or(map, |g| &g.map);
    let (terrain, water, forest) = if blockout.is_some() {
        (None, None, None)
    } else {
        let (terrain, water) =
            canopy_scene::terrain::build(ground_map, &content.landscape, &look.biome)?;
        let forest = look
            .biome
            .surroundings
            .forest
            .as_ref()
            .map(|f| canopy_scene::forest(f, &look.biome, map.size, pack))
            .transpose()
            .map_err(|e| e.to_string())?;
        (Some(terrain), Some(water), forest)
    };
    let field = HeightField::decode(&map.authoring.terrain).map_err(|e| format!("{e:?}"))?;
    let ground = |x: f64, z: f64| field.sample(x, z);
    let scenery: BTreeMap<String, canopy_assets::SceneryPrototype> = pack
        .scenery()
        .map_err(|e| e.to_string())?
        .prototypes
        .into_iter()
        .map(|p| (p.id.clone(), p))
        .collect();
    // Scenery is named by its id or its model asset's (the catalogue keys both).
    let by_model: BTreeMap<String, String> = content
        .landscape
        .iter()
        .filter_map(|a| Some((a.id.to_string(), a.scenery.as_ref()?.to_string())))
        .collect();
    let mut statics = Vec::new();
    let generated_objects = generated
        .as_ref()
        .map(|g| g.objects.as_slice())
        .unwrap_or(&[]);
    for object in map
        .authoring
        .objects
        .iter()
        .chain(generated_objects)
        .filter(|o| o.visible && blockout.is_none())
    {
        let id = by_model.get(&object.asset).unwrap_or(&object.asset);
        let Some(prototype) = scenery.get(id) else {
            continue;
        };
        let y = match object.height_mode {
            canopy_content::map::HeightMode::Absolute => object.elevation,
            _ => ground(object.x, object.z) + object.elevation,
        };
        statics.push(StaticInstance {
            model: prototype.model.clone(),
            transform: Mat4::from_scale_rotation_translation(
                Vec3::splat(object.scale as f32),
                Quat::from_rotation_y(object.yaw as f32),
                Vec3::new(object.x as f32, y as f32, object.z as f32),
            ),
            owner_color: None,
        });
    }
    let colors = colors(look);
    let mut models: Vec<String> = Vec::new();
    let mut model_index = |file: &str| -> u32 {
        match models.iter().position(|m| m == file) {
            Some(i) => i as u32,
            None => {
                models.push(file.to_string());
                (models.len() - 1) as u32
            }
        }
    };
    let mut actors = Vec::new();
    let mut place = |file: &str, scale: f64, x: f64, z: f64, degrees: f64, owner: Option<usize>| {
        let mut actor = ActorInstance::rest(
            model_index(file),
            Mat4::from_scale_rotation_translation(
                Vec3::splat(scale as f32),
                Quat::from_rotation_y((degrees as f32).to_radians()),
                Vec3::new(x as f32, ground(x, z) as f32, z as f32),
            ),
        );
        actor.owner_color = owner.and_then(|o| colors.get(o).copied());
        actors.push(actor);
    };
    for entity in &map.entities {
        let Some(definition) = content.definition(entity.definition.as_str()) else {
            continue;
        };
        let asset = entity
            .appearance
            .as_ref()
            .and_then(|a| a.asset.as_ref())
            .unwrap_or(&definition.model);
        let Some(render) = content.render_of(definition, asset.as_str()) else {
            continue;
        };
        let Some(file) = &render.file else {
            continue;
        };
        // As the game draws it: the look's scale, the placement's and the definition's.
        let scale = render.scale.unwrap_or(1.0)
            * entity
                .appearance
                .as_ref()
                .and_then(|a| a.scale)
                .unwrap_or(1.0)
            * definition.model_scale.unwrap_or(1.0);
        let owner = match entity.owner {
            Owner::Player(slot) => Some(slot as usize),
            Owner::Nobody => None,
        };
        place(
            file,
            scale,
            entity.position.x,
            entity.position.y,
            entity.rotation,
            owner,
        );
    }
    // Each start shows the hall its setup places.
    for start in &map.player_starts {
        let setup = content
            .rules
            .races
            .values()
            .filter_map(|r| r.starting_setup.as_ref())
            .find(|s| s.id.as_str() == start.setup)
            .unwrap_or(&content.rules.starting_setup);
        let (Some(look), Some(fort)) = (
            content.look(setup.fort.as_str()),
            content.definition(setup.fort.as_str()),
        ) else {
            continue;
        };
        let Some(file) = &look.file else {
            continue;
        };
        let degrees = f64::from(start.rotation.unwrap_or(0));
        let owner = Some(usize::from(start.player.saturating_sub(1)));
        let scale = look.scale.unwrap_or(1.0) * fort.model_scale.unwrap_or(1.0);
        place(file, scale, start.x, start.z, degrees, owner);
    }
    let hour = map
        .landscape
        .as_ref()
        .map_or(12.0, |l| l.environment.hour as f32);
    Ok(Scene {
        world: World {
            terrain,
            statics,
            glows: look.glows.by_model(content).map_err(|e| e.to_string())?,
            point_lights: Vec::new(),
            actor_models: models,
            water,
            water_textures: look.water.textures(),
            forest,
            blockout,
            dressing: generated.map(|g| g.dressing),
        },
        actors,
        biome: map.biome().to_string(),
        hour,
    })
}

/// An orbiting camera over the map: what it looks at, how far and from which side.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Camera {
    pub focus: [f32; 2],
    pub distance: f32,
    pub yaw: f32,
    pub pitch: f32,
}

impl Camera {
    pub fn over(size: u32) -> Self {
        Self {
            focus: [size as f32 / 2.0, size as f32 / 2.0],
            distance: size as f32 * 0.55,
            yaw: 0.0,
            pitch: 0.95,
        }
    }

    /// Eye and target in the world, the target on the ground below the focus.
    pub fn eye_target(&self, ground: f32) -> (Vec3, Vec3) {
        let target = Vec3::new(self.focus[0], ground, self.focus[1]);
        let d = self.distance;
        let eye = target
            + d * Vec3::new(
                self.yaw.sin() * self.pitch.cos(),
                self.pitch.sin(),
                self.yaw.cos() * self.pitch.cos(),
            );
        (eye, target)
    }
}

/// One open map.
pub struct Session {
    /// The map package's folder in the base package (`maps/skirmish/frozen-dream.utcm`).
    pub root: String,
    /// The map as last loaded from the store, and the store generation it matches.
    pub map: Option<UtcMap>,
    pub loaded_at: Option<u64>,
    pub scene: Option<Arc<Scene>>,
    /// Bumped whenever `scene` is rebuilt (the stage reloads the world).
    pub version: u64,
    /// The terrain changed in `map` since `scene` was built (a stroke in progress).
    pub stale: bool,
    pub last_build: Option<std::time::Instant>,
    pub error: Option<String>,
    pub camera: Camera,
    pub tools: tools::Tools,
    /// The last step came from this session's own stroke: its map already shows it.
    pub own: bool,
    /// The block editor's state (D80, D81).
    pub blocks: crate::world::State,
    /// The split view's generated world, its version, and since when it waits to follow
    /// the blocks (rebuilt [`GENERATE_AFTER`] after the last change, not while a drag
    /// is under way).
    pub scene_generated: Option<Arc<Scene>>,
    pub generated_version: u64,
    pub generated_due: Option<std::time::Instant>,
}

/// How long the split view's generated world waits after the last change before it is
/// made again: making it costs far more than the blocks.
pub const GENERATE_AFTER: std::time::Duration = std::time::Duration::from_secs(2);

impl Session {
    pub fn new(root: &str) -> Self {
        Self {
            root: root.to_string(),
            map: None,
            loaded_at: None,
            scene: None,
            version: 0,
            stale: false,
            last_build: None,
            error: None,
            camera: Camera::over(640),
            tools: tools::Tools::default(),
            own: false,
            blocks: Default::default(),
            scene_generated: None,
            generated_version: 0,
            generated_due: None,
        }
    }

    /// The document of a map package's file (`terrain/heights.grid`, `map.json`).
    pub fn path(&self, file: &str) -> String {
        format!("{}/{file}", self.root)
    }
}

/// The map package folder of `document` (a map's `map.json`), if it is one.
pub fn root_of(document: &str) -> Option<&str> {
    let root = document.strip_suffix("/map.json")?;
    root.ends_with(".utcm").then_some(root)
}

/// The commands that create a new map package `maps/<group>/<id>.utcm`: its manifest, a
/// map of `size` in `biome` with players 1 and 2 in opposite corners (a map has two
/// starts at least; the Start tool places them properly), made of flat blocks (D80).
pub fn new_map(
    content: &Content,
    base_version: &str,
    group: &str,
    id: &str,
    name: &str,
    size: u32,
    biome: &str,
) -> Result<Vec<crate::document::Command>, String> {
    use canopy_content::map::MAP_SIZES;
    if !MAP_SIZES.contains(&size) || size > 1024 {
        return Err(format!(
            "a map is 256, 480, 512, 640, 800, 960 or 1024 cells, not {size}"
        ));
    }
    let setup = content
        .rules
        .races
        .values()
        .filter_map(|r| r.starting_setup.as_ref())
        .next()
        .map_or(content.rules.starting_setup.id.to_string(), |s| {
            s.id.to_string()
        });
    new_map_of(&setup, base_version, group, id, name, size, biome)
}

/// [`new_map`] with its starting setup given.
fn new_map_of(
    setup: &str,
    base_version: &str,
    group: &str,
    id: &str,
    name: &str,
    size: u32,
    biome: &str,
) -> Result<Vec<crate::document::Command>, String> {
    use crate::document::{Command, Sample};
    use canopy_content::map::blocks::{BLOCK, Blocks};
    use serde_json::json;
    canopy_content::Id::new(id)?;
    let root = format!("maps/{group}/{id}.utcm");
    let corner = |player: u32, x: f64| {
        let (x, z) = tools::start_position(x, x);
        json!({"mainFort": format!("start.player.{player}/main-fort"), "player": player, "setup": setup, "x": x, "z": z})
    };
    let starts = vec![corner(1, 48.0), corner(2, f64::from(size) - 48.0)];
    let terrain = Blocks::flat(size).terrain(size);
    let (span, vertices) = (terrain.span(), terrain.vertices());
    let grid = |file: &str, element: Sample, side: u32, bytes: &[u8]| Command::Grid {
        document: format!("{root}/terrain/{file}.grid"),
        element,
        width: side,
        height: side,
        bytes: bytes.to_vec(),
    };
    let mut commands = vec![
        Command::Create {
            document: format!("{root}/package.json"),
            value: json!({
                "format": 1,
                "id": id,
                "kind": "map",
                "name": name,
                "version": "1.0.0",
                "dependencies": {"base": format!(">={base_version}")},
            }),
        },
        Command::Create {
            document: format!("{root}/map.json"),
            value: json!({
                "biome": biome,
                "camps": [],
                "entities": [],
                "playerStarts": starts,
                "size": size,
            }),
        },
        Command::Create {
            document: format!("{root}/scenery.json"),
            value: json!({"objects": []}),
        },
        grid("heights", Sample::F32, vertices, &terrain.heights.0),
        grid(
            "water-heights",
            Sample::F32,
            vertices,
            &terrain.water_heights.0,
        ),
        grid("water-flow", Sample::Rgba8, span, &terrain.water_flow.0),
    ];
    if let Some(blocks) = &terrain.blocks {
        commands.push(grid("blocks", Sample::Rgba8, span / BLOCK, &blocks.0));
    }
    if let Some(blocked) = &terrain.blocked {
        commands.push(grid("blocked", Sample::U8, span, &blocked.0));
    }
    Ok(commands)
}

/// Writes the map package at `source` with `overlay` laid over it (its unsaved documents
/// and grids, paths relative to the package) to the folder `target`.
pub fn write_overlaid(
    source: &std::path::Path,
    overlay: &canopy_content::package::Overlay,
    target: &std::path::Path,
) -> std::io::Result<()> {
    fn walk(dir: &std::path::Path, prefix: &str, out: &mut Vec<String>) -> std::io::Result<()> {
        if !dir.is_dir() {
            return Ok(());
        }
        for entry in std::fs::read_dir(dir)? {
            let entry = entry?;
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = if prefix.is_empty() {
                name.clone()
            } else {
                format!("{prefix}/{name}")
            };
            if entry.file_type()?.is_dir() {
                walk(&entry.path(), &path, out)?;
            } else {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(source, "", &mut files)?;
    for (path, bytes) in &overlay.0 {
        if bytes.is_some() && !files.contains(path) {
            files.push(path.clone());
        }
    }
    for path in files {
        let bytes = match overlay.0.get(&path) {
            Some(Some(bytes)) => bytes.clone(),
            Some(None) => continue,
            None => std::fs::read(source.join(&path))?,
        };
        let file = target.join(&path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(file, bytes)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::document::{Context, Store};
    use crate::library::Kind;
    use canopy_content::package::Package;
    use std::sync::Arc;

    #[test]
    fn a_new_map_loads_before_and_after_it_is_saved() {
        let root = std::env::temp_dir().join(format!("canopy-new-map-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"format": 1, "id": "base", "kind": "base", "name": "Test", "version": "1.0.0"}"#,
        )
        .unwrap();
        let mut store = Store::new(&root);
        let commands = new_map_of(
            "setup.test",
            "1.0.0",
            "skirmish",
            "flat",
            "Flat",
            256,
            "frozen-dream",
        )
        .unwrap();
        let map_root = "maps/skirmish/flat.utcm";
        let context = Context {
            editor: Kind::Maps,
            document: format!("{map_root}/map.json"),
            pointer: None,
        };
        store.run("Create map", context, commands).unwrap();
        let check = |package: Package| {
            let map = canopy_content::map::package::load_package(&package).unwrap();
            assert_eq!(map.size, 256);
            assert!(map.authoring.terrain.blocks.is_some());
            let terrain = map.authoring.terrain.decode().unwrap();
            let first = terrain.heights[0];
            assert!(terrain.heights.iter().all(|h| *h == first));
        };
        let overlay = Arc::new(store.overlay().below(map_root));
        check(Package::open_overlaid(&root.join(map_root), Some(overlay.clone())).unwrap());
        // The copy Test in game plays.
        let copy = root.join("copy/flat.utcm");
        write_overlaid(&root.join(map_root), &overlay, &copy).unwrap();
        check(Package::open(&copy).unwrap());
        store.save().unwrap();
        check(Package::open(&root.join(map_root)).unwrap());
        let _ = std::fs::remove_dir_all(&root);
    }
}
