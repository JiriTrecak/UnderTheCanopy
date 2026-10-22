//! A map's preview for the map picker (`preview.webp` in its package), the old game's
//! illustrated atlas (`scripts/maps/atlas.ts` there): elevation shading, water by depth,
//! ground paint, woodland darkening the ground around trees and over a map's closed frame,
//! and symbols for amber deposits, camps and player starts.

use canopy_content::map::{MAP_HALO, UtcMap};

/// Preview pixels a side.
pub const SIZE: u32 = 512;

type Rgb = [f32; 3];

fn hex(v: u32) -> Rgb {
    [
        ((v >> 16) & 255) as f32,
        ((v >> 8) & 255) as f32,
        (v & 255) as f32,
    ]
}

fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
    ]
}

/// The biome's map colours: ground, grass, forest (the old biome table's `minimap`).
fn palette(biome: &str) -> (Rgb, Rgb, Rgb, bool) {
    match biome {
        b if b.contains("frozen") || b.contains("frost") => {
            (hex(0xced4d8), hex(0xa8b7ad), hex(0x4f6869), true)
        }
        _ => (hex(0xad956d), hex(0x829151), hex(0x344c2a), false),
    }
}

/// A paint material's colour on the atlas, by the ground it shows.
fn paint(material: &str, grass: Rgb, soil: Rgb, winter: bool) -> Option<Rgb> {
    let m = material.to_ascii_lowercase();
    let has = |w: &[&str]| w.iter().any(|s| m.contains(s));
    Some(if has(&["cliff"]) {
        return None;
    } else if has(&["grass", "moss"]) {
        grass
    } else if has(&["rock", "paving"]) {
        hex(if winter { 0xa4b0bc } else { 0x929080 })
    } else if has(&["pebble", "stones", "rubble"]) {
        hex(if winter { 0xbed0db } else { 0xa79a78 })
    } else if has(&["riverbed"]) {
        hex(0x65948d)
    } else if has(&["dirt"]) {
        hex(if winter { 0xadc6d8 } else { 0xb99a69 })
    } else {
        soil
    })
}

/// The player colours (the game's ownership palette).
const PLAYERS: [u32; 8] = [
    0x338dff, 0xff3038, 0x35de53, 0xf2cf35, 0xed842a, 0x9656cf, 0x36cbd0, 0xffffff,
];

/// The preview of `map`; `trees` tells which scenery assets are trees.
pub fn render(map: &UtcMap, trees: &dyn Fn(&str) -> bool) -> Result<image::RgbaImage, String> {
    let terrain = map.authoring.terrain.decode().map_err(|e| e.to_string())?;
    let (soil, grass, forest, winter) = palette(map.biome());
    let side = terrain.vertices as usize;
    let halo = MAP_HALO as f32;
    let vertex = |x: f32, z: f32| {
        let ix = (x + halo).round().clamp(0.0, side as f32 - 1.0) as usize;
        let iz = (z + halo).round().clamp(0.0, side as f32 - 1.0) as usize;
        iz * side + ix
    };
    let sample = |grid: &[f32], x: f32, z: f32| {
        let (fx, fz) = (
            (x + halo).clamp(0.0, side as f32 - 1.001),
            (z + halo).clamp(0.0, side as f32 - 1.001),
        );
        let (x0, z0) = (fx.floor() as usize, fz.floor() as usize);
        let (tx, tz) = (fx - x0 as f32, fz - z0 as f32);
        let at = |x: usize, z: usize| grid[z * side + x];
        let top = at(x0, z0) * (1.0 - tx) + at(x0 + 1, z0) * tx;
        let bottom = at(x0, z0 + 1) * (1.0 - tx) + at(x0 + 1, z0 + 1) * tx;
        top * (1.0 - tz) + bottom * tz
    };
    // Woodland: the ground darkens within three cells of a tree (old `forestCoverage`).
    let mut woods = vec![0f32; side * side];
    for o in map
        .authoring
        .objects
        .iter()
        .filter(|o| o.visible && trees(&o.asset))
    {
        let radius = 3.0 * o.scale as f32;
        let (ox, oz) = (o.x as f32, o.z as f32);
        for z in (oz - radius).floor() as i32..=(oz + radius).ceil() as i32 {
            for x in (ox - radius).floor() as i32..=(ox + radius).ceil() as i32 {
                let d = ((x as f32 - ox).powi(2) + (z as f32 - oz).powi(2)).sqrt();
                if d >= radius {
                    continue;
                }
                let i = vertex(x as f32, z as f32);
                woods[i] = woods[i].max((0.8 * (1.0 - d / radius) * 2.0).min(1.0));
            }
        }
    }
    // A map's closed frame (terrain/blocked.grid) reads as woodland too.
    if let Some(blocked) = &terrain.blocked {
        let span = terrain.span as usize;
        for z in 0..span {
            for x in 0..span {
                if blocked[z * span + x] {
                    for (dx, dz) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        woods[(z + dz) * side + x + dx] = 1.0;
                    }
                }
            }
        }
    }
    let colours: Vec<Option<Rgb>> = terrain
        .paint
        .iter()
        .map(|p| paint(&p.material, grass, soil, winter))
        .collect();
    // The playable area's square, as the minimap frames it.
    let area = map.playable_area();
    let side = area.width.max(area.height) as f32;
    let (ox, oz) = (
        area.x as f32 + area.width as f32 / 2.0 - side / 2.0,
        area.z as f32 + area.height as f32 / 2.0 - side / 2.0,
    );
    let step = side / SIZE as f32;
    let mut out = image::RgbaImage::new(SIZE, SIZE);
    for py in 0..SIZE {
        for px in 0..SIZE {
            let (wx, wz) = (ox + (px as f32 + 0.5) * step, oz + (py as f32 + 0.5) * step);
            let h = sample(&terrain.heights, wx, wz);
            let water = sample(&terrain.water_heights, wx, wz);
            let i = vertex(wx, wz);
            let grain = ((px.wrapping_add(1).wrapping_mul(73_856_093)
                ^ py.wrapping_add(1).wrapping_mul(19_349_663))
                % 17) as f32
                / 16.0
                - 0.5;
            let mut c = mix(soil, grass, terrain.grass[i]);
            for (layer, colour) in terrain.paint.iter().zip(&colours) {
                if let Some(colour) = colour {
                    c = mix(c, *colour, layer.weights[i]);
                }
            }
            c = mix(c, forest, (woods[i] * 1.15).min(0.96));
            let slope = sample(&terrain.heights, wx - step, wz - step)
                - sample(&terrain.heights, wx + step, wz + step);
            let shade = (1.0 + slope * 0.045).clamp(0.68, 1.22);
            if h < water + 0.04 {
                let depth = ((water - h) / 3.0).clamp(0.0, 1.0);
                c = if winter {
                    mix(hex(0x9cbdd2), hex(0x456d92), depth)
                } else {
                    mix(hex(0x80b7ac), hex(0x285b74), depth)
                };
            } else {
                c = c.map(|v| v * shade + grain * 9.0);
            }
            out.put_pixel(
                px,
                py,
                image::Rgba([
                    c[0].clamp(0.0, 255.0) as u8,
                    c[1].clamp(0.0, 255.0) as u8,
                    c[2].clamp(0.0, 255.0) as u8,
                    255,
                ]),
            );
        }
    }
    symbols(map, &mut out, [ox, oz, side]);
    Ok(out)
}

/// Amber deposits (adjacent mines as one), camps and starts.
fn symbols(map: &UtcMap, out: &mut image::RgbaImage, area: [f32; 3]) {
    let scale = SIZE as f32 / area[2];
    let (ox, oz) = (area[0], area[1]);
    let mut mines: Vec<(f32, f32)> = map
        .entities
        .iter()
        .filter(|e| e.definition.as_str().contains("amber-mine"))
        .map(|e| (e.position.x as f32, e.position.y as f32))
        .collect();
    mines.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
    let mut seen = vec![false; mines.len()];
    for i in 0..mines.len() {
        if seen[i] {
            continue;
        }
        seen[i] = true;
        let mut cluster = vec![i];
        let mut cursor = 0;
        while cursor < cluster.len() {
            let p = mines[cluster[cursor]];
            for j in 0..mines.len() {
                if !seen[j] && (p.0 - mines[j].0).powi(2) + (p.1 - mines[j].1).powi(2) <= 144.0 {
                    seen[j] = true;
                    cluster.push(j);
                }
            }
            cursor += 1;
        }
        let n = cluster.len() as f32;
        let (x, y) = (
            (cluster.iter().map(|&k| mines[k].0).sum::<f32>() / n - ox) * scale,
            (cluster.iter().map(|&k| mines[k].1).sum::<f32>() / n - oz) * scale,
        );
        disc(out, x, y, 8.0, [0x14, 0x1b, 0x17]);
        disc(out, x, y - 1.0, 7.2, [0xff, 0xe0, 0x8b]);
        disc(out, x, y - 1.0, 6.0, [0xef, 0xb9, 0x38]);
        disc(out, x - 0.5, y - 2.5, 2.2, [0xff, 0xf0, 0xa5]);
    }
    for camp in &map.camps {
        let (x, y) = (
            (camp.home.x as f32 - ox) * scale,
            (camp.home.y as f32 - oz) * scale,
        );
        triangle(out, x, y, 10.0, [0x16, 0x20, 0x1a]);
        triangle(out, x, y, 8.0, [0xe3, 0xd6, 0x98]);
        triangle(out, x + 0.5, y + 3.0, 3.0, [0x51, 0x3d, 0x27]);
    }
    for s in &map.player_starts {
        let (x, y) = ((s.x as f32 - ox) * scale, (s.z as f32 - oz) * scale);
        let colour = PLAYERS[(s.player as usize).saturating_sub(1) % 8];
        let colour = [(colour >> 16) as u8, (colour >> 8) as u8, colour as u8];
        disc(out, x, y, 13.0, [0x11, 0x1b, 0x15]);
        blend_disc(out, x, y, 12.0, [0x11, 0x22, 0x1d], 0.79);
        for t in -14..=14 {
            let t = t as f32 * 0.5;
            disc(out, x + t, y + t, 2.5, colour);
            disc(out, x + t, y - t, 2.5, colour);
        }
        let label = format!("P{}", s.player);
        let below = if y > SIZE as f32 - 32.0 { -26.0 } else { 18.0 };
        text(out, x, y + below, &label);
    }
}

fn put(out: &mut image::RgbaImage, x: i32, y: i32, c: [u8; 3]) {
    if x >= 0 && y >= 0 && (x as u32) < out.width() && (y as u32) < out.height() {
        out.put_pixel(x as u32, y as u32, image::Rgba([c[0], c[1], c[2], 255]));
    }
}

fn disc(out: &mut image::RgbaImage, cx: f32, cy: f32, r: f32, c: [u8; 3]) {
    for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
        for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
            if (x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2) <= r * r {
                put(out, x, y, c);
            }
        }
    }
}

fn blend_disc(out: &mut image::RgbaImage, cx: f32, cy: f32, r: f32, c: [u8; 3], a: f32) {
    for y in (cy - r).floor() as i32..=(cy + r).ceil() as i32 {
        for x in (cx - r).floor() as i32..=(cx + r).ceil() as i32 {
            if (x as f32 + 0.5 - cx).powi(2) + (y as f32 + 0.5 - cy).powi(2) <= r * r
                && x >= 0
                && y >= 0
                && (x as u32) < out.width()
                && (y as u32) < out.height()
            {
                let p = out.get_pixel(x as u32, y as u32).0;
                let m = |i: usize| (p[i] as f32 * (1.0 - a) + c[i] as f32 * a) as u8;
                out.put_pixel(x as u32, y as u32, image::Rgba([m(0), m(1), m(2), 255]));
            }
        }
    }
}

/// An upward triangle of half-width `r` centred on (`cx`, `cy`).
fn triangle(out: &mut image::RgbaImage, cx: f32, cy: f32, r: f32, c: [u8; 3]) {
    let (top, bottom) = (cy - r, cy + r * 0.75);
    for y in top.floor() as i32..=bottom.ceil() as i32 {
        let t = ((y as f32 + 0.5 - top) / (bottom - top)).clamp(0.0, 1.0);
        let half = r * t;
        for x in (cx - half).floor() as i32..=(cx + half).ceil() as i32 {
            if (x as f32 + 0.5 - cx).abs() <= half {
                put(out, x, y, c);
            }
        }
    }
}

/// `P` and the digits in a 5 × 7 pixel font.
fn glyph(c: char) -> [u8; 7] {
    match c {
        'P' => [
            0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000,
        ],
        '1' => [
            0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110,
        ],
        '2' => [
            0b01110, 0b10001, 0b00001, 0b00110, 0b01000, 0b10000, 0b11111,
        ],
        '3' => [
            0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110,
        ],
        '4' => [
            0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010,
        ],
        '5' => [
            0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110,
        ],
        '6' => [
            0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110,
        ],
        '7' => [
            0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000,
        ],
        '8' => [
            0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110,
        ],
        _ => [0; 7],
    }
}

/// Light text with a dark outline, 2 pixels a font pixel, centred on `cx`, top at `top`.
fn text(out: &mut image::RgbaImage, cx: f32, top: f32, label: &str) {
    let width = label.chars().count() as i32 * 12 - 2;
    let x0 = cx as i32 - width / 2;
    let draw = |out: &mut image::RgbaImage, grow: i32, colour: [u8; 3]| {
        for (k, ch) in label.chars().enumerate() {
            for (row, bits) in glyph(ch).iter().enumerate() {
                for col in 0..5 {
                    if bits & (1 << (4 - col)) == 0 {
                        continue;
                    }
                    let (x, y) = (x0 + k as i32 * 12 + col * 2, top as i32 + row as i32 * 2);
                    for dy in -grow..2 + grow {
                        for dx in -grow..2 + grow {
                            put(out, x + dx, y + dy, colour);
                        }
                    }
                }
            }
        }
    };
    draw(out, 1, [0x17, 0x21, 0x19]);
    draw(out, 0, [0xff, 0xf2, 0xd0]);
}

/// The preview as WebP (lossless), for `preview.webp`.
pub fn webp(image: &image::RgbaImage) -> Result<Vec<u8>, String> {
    let rgb = image::DynamicImage::ImageRgba8(image.clone()).to_rgb8();
    let mut bytes = Vec::new();
    rgb.write_to(
        &mut std::io::Cursor::new(&mut bytes),
        image::ImageFormat::WebP,
    )
    .map_err(|e| e.to_string())?;
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_content::map::grid::Grid;
    use serde_json::json;

    #[test]
    fn a_preview_marks_the_starts_in_their_colours() {
        let mut map: UtcMap = serde_json::from_value(json!({
            "v": canopy_content::map::UTCMAP_VERSION,
            "name": "Test",
            "size": 256,
            "playerStarts": [
                {"player": 1, "x": 61.5, "z": 61.5, "setup": "setup.test", "mainFort": "start.player.1/main-fort"},
                {"player": 2, "x": 193.5, "z": 193.5, "setup": "setup.test", "mainFort": "start.player.2/main-fort"}
            ],
            "entities": [],
            "camps": [],
            "authoring": {"version": 1, "objects": [], "terrain": {
                "version": 1, "size": 256, "heights": "", "waterHeights": "", "waterFlow": "",
                "waterProfiles": [], "grass": "", "rock": "", "paint": []
            }}
        }))
        .expect("a test map");
        let side = map.authoring.terrain.vertices() as usize;
        let floats = |v: f32| Grid((0..side * side).flat_map(|_| v.to_le_bytes()).collect());
        map.authoring.terrain.heights = floats(1.0);
        map.authoring.terrain.water_heights = floats(-17.0);
        map.authoring.terrain.grass = floats(1.0);
        map.authoring.terrain.rock = floats(0.0);
        let span = map.authoring.terrain.span() as usize;
        map.authoring.terrain.water_flow = Grid([128u8, 128, 0, 0].repeat(span * span));
        let image = render(&map, &|_| false).unwrap();
        assert_eq!(image.dimensions(), (SIZE, SIZE));
        // The starts' crosses meet at their centres: blue for player 1, red for 2.
        let at = |x: f64, z: f64| image.get_pixel((x * 2.0) as u32, (z * 2.0) as u32).0;
        assert_eq!(&at(61.5, 61.5)[..3], &[0x33, 0x8d, 0xff]);
        assert_eq!(&at(193.5, 193.5)[..3], &[0xff, 0x30, 0x38]);
        // Elsewhere: the biome's grass.
        let grass = at(128.0, 30.0);
        assert!(grass[1] > grass[0] && grass[1] > grass[2], "{grass:?}");
        assert!(!webp(&image).unwrap().is_empty());
    }
}
