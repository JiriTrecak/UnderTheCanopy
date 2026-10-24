//! What the editor's live stages share (the ability stage, the combat lab): unique scene
//! versions for the one renderer that draws them all, and the zinc ground a sandbox world
//! stands on.

use canopy_assets::Pack;
use canopy_content::map::UtcMap;
use canopy_render::terrain::{LayerKind, TerrainDesc};
use canopy_scene::MapScene;

/// The zinc stage's ground, linear (its texture's average is tinted to this).
pub const GROUND: f32 = 0.05;

/// The next scene version: unique across stages, since one renderer draws them all and
/// tells their grounds apart by it.
pub fn next_version() -> u64 {
    static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
}

/// `map` drawn on the zinc stage: one neutral material (the biome's rock, tinted so it
/// averages zinc) without the forest around it.
pub fn zinc_ground(
    world: &canopy_sim::World,
    map: &UtcMap,
    pack: &Pack,
    look: &canopy_scene::Look,
) -> Result<MapScene, String> {
    let mut map = map.clone();
    let terrain = &mut map.authoring.terrain;
    let floats = |value: f32, like: &canopy_content::map::grid::Grid| {
        canopy_content::map::grid::Grid(
            std::iter::repeat_n(value.to_le_bytes(), like.0.len() / 4)
                .flatten()
                .collect(),
        )
    };
    terrain.rock = floats(1.0, &terrain.rock);
    terrain.grass = floats(0.0, &terrain.grass);
    let mut scene = canopy_scene::build(world, &map, pack, look).map_err(|e| e.to_string())?;
    scene.world.forest = None;
    if let Some(desc) = &mut scene.world.terrain {
        neutral(desc, pack);
    }
    Ok(scene)
}

/// Makes `desc` one material: the layer that covers the most ground becomes the base,
/// tinted so its texture averages a dark neutral grey.
fn neutral(desc: &mut TerrainDesc, pack: &Pack) {
    let main = desc
        .layers
        .iter()
        .enumerate()
        .skip(1)
        .filter(|(_, l)| matches!(l.kind, LayerKind::Tiled { .. }))
        .max_by_key(|(_, l)| l.mask.iter().map(|&w| u64::from(w)).sum::<u64>())
        .map(|(i, _)| i);
    if let Some(i) = main {
        let layer = desc.layers[i].clone();
        let base = &mut desc.layers[0];
        base.albedo = layer.albedo;
        base.normal = layer.normal;
        base.occlusion = layer.occlusion;
        if let LayerKind::Tiled { tiling, .. } = layer.kind {
            base.kind = LayerKind::Base { tiling };
        }
    }
    let base = &mut desc.layers[0];
    base.tint = average(pack, &base.albedo).map_or([0.35; 3], |mean| {
        [0, 1, 2].map(|c| GROUND / mean[c].max(0.005))
    });
    base.painted = true;
    desc.layers.truncate(1);
    desc.surroundings = None;
}

/// The average linear colour of a texture (its smallest level).
fn average(pack: &Pack, name: &str) -> Option<[f32; 3]> {
    let texture = pack.texture(name).ok()?;
    let level = texture.meta.mips.len().checked_sub(1)?;
    let texels = texture.decode_level(level);
    let linear = |c: u8| {
        let c = f32::from(c) / 255.0;
        if c <= 0.04045 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let n = (texels.len() / 4).max(1) as f32;
    let mut sum = [0.0f32; 3];
    for px in texels.chunks_exact(4) {
        for c in 0..3 {
            sum[c] += linear(px[c]);
        }
    }
    Some(sum.map(|s| s / n))
}
