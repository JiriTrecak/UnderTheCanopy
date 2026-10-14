//! The map's committed terrain as a render terrain (old `authoredTerrain`).
//!
//! Layers, in compositing order: the biome's six built-in layers (soil as the base,
//! grass, dirt, waterbed, stones, rock), then the map's painted catalogue materials
//! (atlas grounds in paint order, then cliffs). Vertex weights follow the old rules:
//!
//! - every non-cliff paint erases what lies under it by its weight (grass, dirt, stones,
//!   rock, waterbed and the other atlas grounds), cliffs only compete with cliffs;
//! - grass fades out within 0.2 above the water, the waterbed starts 0.1 above the
//!   water and is full 0.1 below, rock follows the rock coverage or a slope over 0.7;
//! - atlas cells get a corner code from the weights at their four corners, and the
//!   lowest material of a cell the atlas grounds cover completely draws a full tile.

use canopy_content::{
    assets::landscape::{AssetKind, LandscapeAsset, Projection, Shading},
    map::{MAP_HALO, UtcMap},
};
use canopy_render::{
    environment::{BiomeConfig, Hex, TREE_COVER_RADIUS},
    terrain::{LayerKind, Surroundings, TerrainDesc, TerrainLayer, TextureGrid, WaterLook},
};
use std::collections::BTreeMap;

/// Built-in layer channels.
const SOIL: usize = 0;
const GRASS: usize = 1;
const DIRT: usize = 2;
const WATERBED: usize = 3;
const STONES: usize = 4;
const ROCK: usize = 5;
const FIXED: usize = 6;

/// Pack entry of a terrain material's packed tile (`data.bin`, `data_2.bin`, …).
pub fn tile_entry(asset: &str, index: u32) -> String {
    if index <= 1 {
        format!("assets/library/{asset}/data.bin")
    } else {
        format!("assets/library/{asset}/data_{index}.bin")
    }
}

fn round_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// What a painted material becomes.
struct Painted<'a> {
    asset: &'a LandscapeAsset,
    cliff: bool,
}

/// Builds the render terrain. `objects` are the map's visible scenery objects with their
/// catalogue kind, for ground cover.
pub fn build(
    map: &UtcMap,
    landscape: &[LandscapeAsset],
    biome: &BiomeConfig,
) -> Result<TerrainDesc, String> {
    let _span = tracing::info_span!("scene_terrain").entered();
    let data = &map.authoring.terrain;
    let decoded = data.decode().map_err(|e| e.to_string())?;
    let span = decoded.span;
    let w = decoded.vertices as usize;
    let n = w * w;
    let origin = -(MAP_HALO as f32);
    let catalogue: BTreeMap<&str, &LandscapeAsset> =
        landscape.iter().map(|a| (a.id.as_str(), a)).collect();

    // Built-in layers, and which painted materials are one of them.
    let fixed = biome.terrain.ordered();
    let fixed_channel = |material: &str| {
        fixed
            .iter()
            .position(|l| l.albedo == material)
            .filter(|&c| c != SOIL)
    };

    // Painted catalogue materials: non-cliffs in paint order, then cliffs.
    let mut painted: Vec<Painted> = Vec::new();
    let mut channel_of: Vec<Option<usize>> = Vec::with_capacity(decoded.paint.len());
    for paint in &decoded.paint {
        if let Some(c) = fixed_channel(&paint.material) {
            channel_of.push(Some(c));
            continue;
        }
        match catalogue
            .get(paint.material.as_str())
            .filter(|a| a.terrain.is_some())
        {
            Some(asset) => {
                let cliff = matches!(
                    asset.terrain.as_ref().and_then(|t| t.projection.as_ref()),
                    Some(Projection::Cliff { .. })
                );
                painted.push(Painted { asset, cliff });
                channel_of.push(None);
            }
            None => {
                tracing::warn!(material = %paint.material, "painted material has no ground texture; left out");
                channel_of.push(Some(usize::MAX));
            }
        }
    }
    let mut order: Vec<usize> = (0..painted.len()).collect();
    order.sort_by_key(|&i| painted[i].cliff);
    // Paint index → custom slot (index into `order`).
    let mut custom_slot = vec![usize::MAX; decoded.paint.len()];
    {
        let mut k = 0;
        for (p, channel) in channel_of.iter().enumerate() {
            if channel.is_none() {
                custom_slot[p] = order.iter().position(|&o| o == k).expect("ordered");
                k += 1;
            }
        }
    }
    let customs: Vec<&Painted> = order.iter().map(|&i| &painted[i]).collect();

    // Grass coverage plus ground cover around foliage and trees.
    let mut grass_cover = decoded.grass.clone();
    let cover = biome.ground_cover;
    let kinds: BTreeMap<&str, AssetKind> =
        landscape.iter().map(|a| (a.id.as_str(), a.kind)).collect();
    for object in map.authoring.objects.iter().filter(|o| o.visible) {
        let tree = match kinds.get(object.asset.as_str()) {
            Some(AssetKind::Tree) => true,
            Some(AssetKind::Foliage) => false,
            _ => continue,
        };
        let scale = object.scale as f32;
        let radius = if tree {
            TREE_COVER_RADIUS
        } else {
            cover.radius
        } * scale;
        if radius <= 0.0 {
            continue;
        }
        let (ox, oz) = (object.x as f32, object.z as f32);
        let x0 = ((ox - radius - origin).floor().max(0.0)) as usize;
        let x1 = ((ox + radius - origin).ceil().max(0.0) as usize).min(w - 1);
        let z0 = ((oz - radius - origin).floor().max(0.0)) as usize;
        let z1 = ((oz + radius - origin).ceil().max(0.0) as usize).min(w - 1);
        for z in z0..=z1 {
            for x in x0..=x1 {
                let i = z * w + x;
                if decoded.heights[i] < decoded.water_heights[i] + 0.15 {
                    continue;
                }
                let d =
                    ((origin + x as f32 - ox).powi(2) + (origin + z as f32 - oz).powi(2)).sqrt();
                if d < radius {
                    let weight = cover.strength * (1.0 - d / radius);
                    grass_cover[i] = grass_cover[i].max(weight);
                }
            }
        }
    }

    // Vertex weights.
    let mut masks = vec![vec![0u8; n]; FIXED + customs.len()];
    let height = |x: i64, z: i64| {
        decoded.heights[(z.clamp(0, w as i64 - 1) as usize) * w + x.clamp(0, w as i64 - 1) as usize]
    };
    let mut custom = vec![0f32; customs.len()];
    for i in 0..n {
        let (x, z) = ((i % w) as i64, (i / w) as i64);
        let mut grass = grass_cover[i];
        let mut rock = decoded.rock[i];
        let (mut road, mut stones, mut bed) = (0.0f32, 0.0f32, 0.0f32);
        custom.iter_mut().for_each(|c| *c = 0.0);
        for (p, paint) in decoded.paint.iter().enumerate() {
            let weight = paint.weights[i];
            if weight <= 0.0 {
                continue;
            }
            let channel = channel_of[p];
            if channel == Some(usize::MAX) {
                continue;
            }
            let slot = custom_slot[p];
            let cliff = slot != usize::MAX && customs[slot].cliff;
            if !cliff {
                let own = |c: usize| if channel == Some(c) { weight } else { 0.0 };
                grass = grass * (1.0 - weight) + own(GRASS);
                road = road * (1.0 - weight) + own(DIRT);
                stones = stones * (1.0 - weight) + own(STONES);
                rock = rock * (1.0 - weight) + own(ROCK);
                bed = bed * (1.0 - weight) + own(WATERBED);
            }
            for (k, value) in custom.iter_mut().enumerate() {
                if customs[k].cliff != cliff {
                    continue;
                }
                let target = if k == slot { 255.0 * weight } else { 0.0 };
                *value = (*value * (1.0 - weight) + target).round();
            }
        }
        let ground = decoded.heights[i];
        let water = decoded.water_heights[i];
        let slope = ((height(x + 1, z) - height(x - 1, z)) * 0.5)
            .hypot((height(x, z + 1) - height(x, z - 1)) * 0.5);
        masks[SOIL][i] = 255;
        masks[GRASS][i] = round_u8(grass * ((ground - water) / 0.2).clamp(0.0, 1.0));
        masks[DIRT][i] = round_u8(road);
        masks[WATERBED][i] = round_u8(((water - ground + 0.1) * 5.0).max(bed).clamp(0.0, 1.0));
        masks[STONES][i] = round_u8(stones);
        masks[ROCK][i] = round_u8(rock.max((slope - 0.7).clamp(0.0, 1.0)));
        for (k, value) in custom.iter().enumerate() {
            masks[FIXED + k][i] = value.clamp(0.0, 255.0) as u8;
        }
    }

    // Atlas corner codes per texture cell.
    let layout = decoded.texture;
    let cells = [layout.cells[0] as usize, layout.cells[1] as usize];
    let bilinear = |mask: &[u8], mx: f64, mz: f64| -> f32 {
        let fx = mx.clamp(0.0, (w - 1) as f64);
        let fz = mz.clamp(0.0, (w - 1) as f64);
        let (x0, z0) = (fx.floor() as usize, fz.floor() as usize);
        let (x1, z1) = ((x0 + 1).min(w - 1), (z0 + 1).min(w - 1));
        let (tx, tz) = ((fx - x0 as f64) as f32, (fz - z0 as f64) as f32);
        let at = |x: usize, z: usize| f32::from(mask[z * w + x]);
        let top = at(x0, z0) + (at(x1, z0) - at(x0, z0)) * tx;
        let bottom = at(x0, z1) + (at(x1, z1) - at(x0, z1)) * tx;
        top + (bottom - top) * tz
    };
    let atlas_layers: Vec<usize> = (0..customs.len()).filter(|&k| !customs[k].cliff).collect();
    let mut codes = vec![vec![0u8; cells[0] * cells[1]]; customs.len()];
    let mask_origin = f64::from(origin);
    for cz in 0..cells[1] {
        for cx in 0..cells[0] {
            let cell = cz * cells[0] + cx;
            let mut totals = [0f32; 4];
            for &k in &atlas_layers {
                let mut bits = 0u8;
                for (c, total) in totals.iter_mut().enumerate() {
                    let mx = layout.start[0] - mask_origin + (cx + c % 2) as f64 * layout.cell_size;
                    let mz = layout.start[1] - mask_origin + (cz + c / 2) as f64 * layout.cell_size;
                    let v = bilinear(&masks[FIXED + k], mx, mz);
                    *total += v;
                    if v >= 127.5 {
                        bits |= 1 << c;
                    }
                }
                codes[k][cell] = bits;
            }
            if totals.iter().all(|&t| t >= 254.0)
                && let Some(&k) = atlas_layers.iter().find(|&&k| codes[k][cell] != 0)
            {
                codes[k][cell] |= 16;
            }
        }
    }

    let mut layers = Vec::with_capacity(FIXED + customs.len());
    for (c, layer) in fixed.iter().enumerate() {
        let kind = if c == SOIL {
            LayerKind::Base {
                tiling: layer.tiling,
            }
        } else {
            LayerKind::Tiled {
                tiling: layer.tiling,
                blend: layer.blend,
                edge: layer.edge,
                breakup: layer.breakup,
            }
        };
        layers.push(TerrainLayer {
            kind,
            albedo: tile_entry(&layer.albedo, 1),
            normal: tile_entry(&layer.normal, 1),
            occlusion: None,
            tint: layer.tint.linear().to_array(),
            painted: false,
            mask: std::mem::take(&mut masks[c]),
            cells: Vec::new(),
        });
    }
    let variants: Vec<Option<Vec<u8>>> = decoded.paint.iter().map(|p| p.variants.clone()).collect();
    for (k, painted) in customs.iter().enumerate() {
        let ground = painted.asset.terrain.as_ref().expect("ground texture");
        let runtime = ground
            .runtime
            .as_ref()
            .ok_or_else(|| format!("{} has no packed runtime tiles", painted.asset.id))?;
        let id = painted.asset.id.as_str();
        let atlas = &ground.atlas;
        let kind = match &ground.projection {
            Some(Projection::Cliff {
                vertical_world_size,
                start_slope,
                full_slope,
            }) => LayerKind::Cliff {
                start_slope: *start_slope as f32,
                full_slope: *full_slope as f32,
                vertical_size: *vertical_world_size as f32,
            },
            None => {
                let mut corners = [0u8; 16];
                for (mask, tile) in atlas.corners.iter().flatten().enumerate().take(16) {
                    corners[mask] = tile.unwrap_or(0) as u8;
                }
                LayerKind::Atlas {
                    columns: atlas.columns,
                    rows: atlas.rows,
                    corners,
                }
            }
        };
        let cells_data = if painted.cliff {
            Vec::new()
        } else {
            let paint_index = (0..decoded.paint.len())
                .find(|&p| custom_slot[p] == k)
                .expect("paint of a custom layer");
            let full = &atlas.full_tiles;
            codes[k]
                .iter()
                .enumerate()
                .map(|(cell, &code)| {
                    let variant = variants[paint_index]
                        .as_ref()
                        .map_or(0, |v| usize::from(v[cell]));
                    let tile = if full.is_empty() {
                        0
                    } else {
                        full[variant % full.len()]
                    };
                    [code, tile as u8]
                })
                .collect()
        };
        let reflectance = ground.reflectance as f32;
        layers.push(TerrainLayer {
            kind,
            albedo: tile_entry(id, runtime.albedo_roughness.index),
            normal: tile_entry(id, runtime.normal_opacity.index),
            occlusion: Some(tile_entry(id, runtime.occlusion_metalness.index)),
            tint: [reflectance; 3],
            painted: ground.shading == Some(Shading::Painted),
            mask: std::mem::take(&mut masks[FIXED + k]),
            cells: cells_data,
        });
    }

    let water = catalogue
        .get(biome.water.as_str())
        .and_then(|a| a.water.as_ref())
        .ok_or_else(|| {
            format!(
                "biome water {} is not a catalogue water profile",
                biome.water
            )
        })?;
    let color = |hex: &str| {
        let text = format!("\"{hex}\"");
        serde_json::from_str::<Hex>(&text)
            .map(|h| h.linear().to_array())
            .map_err(|e| e.to_string())
    };
    let s = &biome.surroundings;
    Ok(TerrainDesc {
        span,
        origin,
        heights: decoded.heights,
        water: decoded.water_heights,
        layers,
        grid: TextureGrid {
            start: [layout.start[0] as f32, layout.start[1] as f32],
            cell_size: layout.cell_size as f32,
            cells: layout.cells,
        },
        water_look: WaterLook {
            shallow: color(&water.shallow_color)?,
            deep: color(&water.deep_color)?,
            clarity: water.clarity as f32,
        },
        surroundings: Some(Surroundings {
            floor_texture: format!("assets/library/{}/albedo.png", s.floor_texture),
            floor_color: s.floor_color.linear().to_array(),
            haze_color: s.haze_color.linear().to_array(),
            haze: [s.haze_start, s.haze_end],
        }),
    })
}
