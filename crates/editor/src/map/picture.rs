//! A map seen from straight above as a picture, one or more pixels per cell: ground and
//! paint shaded by height and slope, water by depth, cliffs dark, trees, amber, camps and
//! starts marked. What MCP's `map_picture` shows an agent laying out a map.

use canopy_content::map::{MAP_HALO, UtcMap};
use canopy_content::placement::Owner;

/// Player colours for the start markers (1 to 8).
const PLAYERS: [[u8; 3]; 8] = [
    [220, 40, 40],
    [40, 90, 230],
    [30, 200, 170],
    [140, 50, 200],
    [240, 220, 40],
    [240, 140, 30],
    [40, 200, 40],
    [240, 110, 190],
];

/// A material's colour from its id (the Warcraft tile names and plain words).
fn material(id: &str) -> [f32; 3] {
    let id = id.to_ascii_lowercase();
    let has = |words: &[&str]| words.iter().any(|w| id.contains(w));
    if has(&["cliff"]) {
        [92.0, 86.0, 78.0]
    } else if has(&["lgrd", "dark"]) {
        [58.0, 96.0, 44.0]
    } else if has(&["grs", "grass"]) {
        [92.0, 138.0, 58.0]
    } else if has(&["rok", "rock", "stone"]) {
        [128.0, 124.0, 116.0]
    } else if has(&["san", "sand"]) {
        [196.0, 178.0, 124.0]
    } else if has(&["dro", "rough"]) {
        [118.0, 96.0, 70.0]
    } else if has(&["drg", "grassy"]) {
        [112.0, 118.0, 66.0]
    } else if has(&["drt", "dirt", "soil"]) {
        [140.0, 110.0, 74.0]
    } else if has(&["snow", "ice"]) {
        [226.0, 232.0, 238.0]
    } else if has(&["leaf", "autumn"]) {
        [170.0, 110.0, 50.0]
    } else {
        [150.0, 130.0, 100.0]
    }
}

/// The map as an RGB picture, `scale` pixels per cell.
pub fn picture(map: &UtcMap, scale: u32) -> Result<image::RgbImage, String> {
    let terrain = map.authoring.terrain.decode().map_err(|e| e.to_string())?;
    let size = map.size;
    let side = terrain.vertices as usize;
    let halo = MAP_HALO as usize;
    let at = |grid: &[f32], x: usize, z: usize| grid[(z + halo) * side + x + halo];
    let colours: Vec<[f32; 3]> = terrain
        .paint
        .iter()
        .map(|p| material(&p.material))
        .collect();
    let scale = scale.max(1);
    let mut out = image::RgbImage::new(size * scale, size * scale);
    let soil = [140.0, 110.0, 74.0];
    let grass = [92.0, 138.0, 58.0];
    let rock = [128.0, 124.0, 116.0];
    let mix = |a: [f32; 3], b: [f32; 3], t: f32| {
        let t = t.clamp(0.0, 1.0);
        [
            a[0] + (b[0] - a[0]) * t,
            a[1] + (b[1] - a[1]) * t,
            a[2] + (b[2] - a[2]) * t,
        ]
    };
    for z in 0..size as usize {
        for x in 0..size as usize {
            let h = at(&terrain.heights, x, z);
            let mut c = mix(soil, grass, at(&terrain.grass, x, z));
            c = mix(c, rock, at(&terrain.rock, x, z));
            for (layer, colour) in terrain.paint.iter().zip(&colours) {
                c = mix(c, *colour, at(&layer.weights, x, z));
            }
            // Slope light from the north-west, height brightening, cliffs dark.
            let (hx, hz) = (
                at(&terrain.heights, x + 1, z),
                at(&terrain.heights, x, z + 1),
            );
            let step = (hx - h).abs().max((hz - h).abs());
            let light = (1.0 - ((hx - h) + (hz - h)) * 0.35).clamp(0.55, 1.35);
            let lift = (0.82 + (h + 4.0) * 0.022).clamp(0.6, 1.25);
            let mut k = light * lift;
            if step > crate::map::tools::CLIFF_STEP {
                k *= 0.55;
            }
            c = [c[0] * k, c[1] * k, c[2] * k];
            let water = at(&terrain.water_heights, x, z);
            if water > h + 0.05 {
                let depth = ((water - h) / 4.0).clamp(0.0, 1.0);
                c = mix([86.0, 150.0, 170.0], [24.0, 62.0, 112.0], depth);
            }
            fill(
                &mut out,
                scale,
                x as i64,
                z as i64,
                1,
                c.map(|v| v.clamp(0.0, 255.0) as u8),
            );
        }
    }
    for o in &map.authoring.objects {
        let colour =
            if o.asset.contains("pine") || o.asset.contains("tree") || o.asset.contains("oak") {
                [26, 64, 30]
            } else {
                [70, 70, 60]
            };
        fill(
            &mut out,
            scale,
            o.x.floor() as i64,
            o.z.floor() as i64,
            1,
            colour,
        );
    }
    let member: std::collections::HashSet<&str> = map
        .camps
        .iter()
        .flat_map(|c| c.members.iter().map(String::as_str))
        .collect();
    for e in &map.entities {
        let (x, z) = (e.position.x.floor() as i64, e.position.y.floor() as i64);
        let colour = if e.definition.as_str().contains("amber") {
            [255, 176, 30]
        } else if member.contains(e.id.as_str()) {
            [235, 60, 60]
        } else if e.owner == Owner::Nobody {
            [235, 235, 235]
        } else {
            [200, 200, 255]
        };
        fill(&mut out, scale, x - 1, z - 1, 3, colour);
    }
    for camp in &map.camps {
        ring(
            &mut out,
            scale,
            camp.home.x,
            camp.home.y,
            camp.aggro_range as f64,
            [235, 60, 60],
        );
    }
    for s in &map.player_starts {
        let colour = PLAYERS[(s.player as usize).saturating_sub(1) % 8];
        let (x, z) = (s.x.floor() as i64, s.z.floor() as i64);
        fill(&mut out, scale, x - 4, z - 4, 9, colour);
        ring(&mut out, scale, s.x, s.z, 14.0, colour);
    }
    Ok(out)
}

/// The picture as PNG bytes.
pub fn png(map: &UtcMap, scale: u32) -> Result<Vec<u8>, String> {
    let picture = picture(map, scale)?;
    let mut bytes = Vec::new();
    picture
        .write_to(
            &mut std::io::Cursor::new(&mut bytes),
            image::ImageFormat::Png,
        )
        .map_err(|e| e.to_string())?;
    Ok(bytes)
}

fn fill(out: &mut image::RgbImage, scale: u32, x: i64, z: i64, cells: i64, colour: [u8; 3]) {
    let s = scale as i64;
    let (w, h) = (out.width() as i64, out.height() as i64);
    for py in (z * s).max(0)..((z + cells) * s).min(h) {
        for px in (x * s).max(0)..((x + cells) * s).min(w) {
            out.put_pixel(px as u32, py as u32, image::Rgb(colour));
        }
    }
}

fn ring(out: &mut image::RgbImage, scale: u32, x: f64, z: f64, radius: f64, colour: [u8; 3]) {
    let s = scale as f64;
    let n = (radius * s * 6.0).max(24.0) as usize;
    for i in 0..n {
        let a = i as f64 / n as f64 * std::f64::consts::TAU;
        let (px, py) = ((x + a.cos() * radius) * s, (z + a.sin() * radius) * s);
        if px >= 0.0 && py >= 0.0 && (px as u32) < out.width() && (py as u32) < out.height() {
            out.put_pixel(px as u32, py as u32, image::Rgb(colour));
        }
    }
}
