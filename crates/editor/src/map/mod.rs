//! The map editor (docs/editor.md, M3): an open map is a [`Session`] over the map
//! package's documents and grids in the store. The viewport draws an editor [`Scene`]
//! built from the map (terrain, water, the forest around it, trees and props, mines,
//! creeps and start halls); brushes change the grids directly, and a stroke becomes one
//! undo step of grid patches when it ends.

pub mod tools;
pub mod view;

use canopy_assets::Pack;
use canopy_content::{
    Content,
    map::{UtcMap, grid::Element},
    placement::Owner,
};
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
    pub field: HeightField,
    pub size: u32,
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

/// Builds the editor's scene of `map`.
pub fn build(map: &UtcMap, content: &Content, pack: &Pack, look: &Look) -> Result<Scene, String> {
    let (terrain, water) = canopy_scene::terrain::build(map, &content.landscape, &look.biome)?;
    let forest = look
        .biome
        .surroundings
        .forest
        .as_ref()
        .map(|f| canopy_scene::forest(f, &look.biome, map.size, pack))
        .transpose()
        .map_err(|e| e.to_string())?;
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
    for object in map.authoring.objects.iter().filter(|o| o.visible) {
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
            terrain: Some(terrain),
            statics,
            glows: look.glows.by_model(content).map_err(|e| e.to_string())?,
            point_lights: Vec::new(),
            actor_models: models,
            water: Some(water),
            water_textures: look.water.textures(),
            forest,
        },
        actors,
        field,
        size: map.size,
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
    /// The map package's folder in the base package (`maps/skirmish/echo-isles.utcm`).
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
}

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

/// A float grid's samples from its raw bytes.
pub fn floats(bytes: &[u8]) -> Vec<f32> {
    bytes
        .chunks_exact(4)
        .map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

/// Raw bytes of float samples.
pub fn bytes_of(values: &[f32]) -> Vec<u8> {
    values.iter().flat_map(|v| v.to_le_bytes()).collect()
}

/// Whether `element` is a float grid.
pub fn is_float(element: Element) -> bool {
    element == Element::F32
}
