//! Loading a map for viewing: content, the cooked pack, the biome's configuration and
//! the scene the renderer draws.

use crate::paths::Paths;
use anyhow::{Context, Result, anyhow};
use canopy_assets::Pack;
use canopy_content::{Content, map};
use canopy_render::{Environment, environment::DaytimeLut};
use canopy_scene::{Look, MapScene};
use std::sync::Arc;

pub struct Loaded {
    pub scene: MapScene,
    pub environment: Environment,
    pub pack: Pack,
}

/// Loads map `id` with its biome's look at `hour` (the map's own when `None`).
pub fn load(paths: &Paths, id: &str, hour: Option<f32>) -> Result<Loaded> {
    let _span = tracing::info_span!("load_map", map = id).entered();
    let started = std::time::Instant::now();
    let content = Content::load(&paths.data).map_err(|e| anyhow!("content: {e}"))?;
    let library = map::library(&paths.data).map_err(|e| anyhow!("maps: {e}"))?;
    let file = library.iter().find(|m| m.id == id).with_context(|| {
        let ids: Vec<&str> = library.iter().map(|m| m.id.as_str()).collect();
        format!("no map {id} (maps: {})", ids.join(", "))
    })?;
    let map = file.load().map_err(|e| anyhow!("{}: {e}", file.file))?;
    let pack_path =
        canopy_assets::find_pack(&paths.data).context("no cooked asset pack (run `just cook`)")?;
    let pack = Pack::open(&pack_path).with_context(|| pack_path.display().to_string())?;
    let look = Look::load(&paths.data, map.biome())?;
    let scene = canopy_scene::build(Arc::new(content), &map, &pack, &look)?;
    let hour = hour.unwrap_or(scene.hour);
    let grade = &look.biome.grade;
    let lut_texture = match (&grade.daytime_lut, grade.daytime_lut_strength > 0.0) {
        (Some(entry), true) => Some(pack.texture(entry).with_context(|| entry.clone())?),
        _ => None,
    };
    let lut_texels = lut_texture.as_ref().map(|t| t.decode_level(0));
    let lut = match (&lut_texture, &lut_texels) {
        (Some(t), Some(rgba)) => Some(DaytimeLut {
            size: t.meta.width,
            rgba,
            strength: grade.daytime_lut_strength,
        }),
        _ => None,
    };
    let environment = Environment::new(&look.lighting, &look.biome, hour, lut.as_ref());
    tracing::info!(
        map = id,
        biome = map.biome(),
        statics = scene.world.statics.len(),
        ms = started.elapsed().as_millis() as u64,
        "map loaded"
    );
    Ok(Loaded {
        scene,
        environment,
        pack,
    })
}
