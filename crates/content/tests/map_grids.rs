//! Decoding the committed terrain grids.

mod map_fixture;

use canopy_content::map::{MAP_HALO, WaterFlow, decode_base64, decode_bytes, decode_floats};
use map_fixture::{SIZE, base64, floats, map, parse, set, vertex_count};
use serde_json::json;

#[test]
fn base64_decodes_like_atob() {
    assert_eq!(decode_base64("AQID").unwrap(), [1, 2, 3]);
    assert_eq!(decode_base64("AQI=").unwrap(), [1, 2]);
    assert_eq!(decode_base64("AQ==").unwrap(), [1]);
    // Padding is optional, whitespace ignored, unused trailing bits dropped.
    assert_eq!(decode_base64("AQI").unwrap(), [1, 2]);
    assert_eq!(decode_base64(" AQ\nID ").unwrap(), [1, 2, 3]);
    assert_eq!(decode_base64("AR==").unwrap(), [1]);
    assert_eq!(decode_base64("").unwrap(), [] as [u8; 0]);
    for invalid in ["A", "AQIDB", "AQ-D", "A===", "AQ=D", "Zm9v\u{e9}"] {
        assert!(decode_base64(invalid).is_err(), "{invalid:?}");
    }
}

#[test]
fn byte_grids_need_canonical_dimensions() {
    assert_eq!(decode_bytes("AQID", 3).unwrap(), [1, 2, 3]);
    assert_eq!(decode_bytes("AQI=", 2).unwrap(), [1, 2]);
    // Right length for the count, but whitespace makes it decode short.
    assert!(decode_bytes("AQ I", 3).is_err());
    assert!(decode_bytes("AQID", 2).is_err());
    assert!(decode_bytes("AQ", 1).is_err());
    let values = [0.5_f32, -16.0, 24.0, f32::MIN_POSITIVE];
    assert_eq!(decode_floats(&floats(values), 4).unwrap(), values);
    assert!(decode_floats(&floats(values), 3).is_err());
}

#[test]
fn terrain_decodes_to_typed_buffers() {
    let n = vertex_count(SIZE);
    let span = (SIZE + 2 * MAP_HALO) as usize;
    let heights: Vec<f32> = (0..n).map(|i| (i % 41) as f32 - 16.0).collect();
    let mut flow = vec![0u8; span * span * 4];
    flow[4..8].copy_from_slice(&[1, 2, 3, 1]);
    let mut document = map();
    set(
        &mut document,
        "/authoring/terrain/heights",
        json!(floats(heights.iter().copied())),
    );
    set(
        &mut document,
        "/authoring/terrain/waterFlow",
        json!(base64(&flow)),
    );
    set(
        &mut document,
        "/authoring/terrain/waterProfiles",
        json!(["water.test"]),
    );
    set(
        &mut document,
        "/authoring/terrain/paint",
        json!([{"material": "ground.test", "weights": floats(std::iter::repeat_n(0.25, n)), "variants": base64(&vec![7; 72 * 72])}]),
    );
    let parsed = parse(document).unwrap_or_else(|e| panic!("{e}"));
    let terrain = &parsed.authoring.terrain;
    assert_eq!((terrain.span(), terrain.vertices()), (288, 289));
    let decoded = terrain.decode().unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(decoded.heights, heights);
    assert_eq!(decoded.water_heights, vec![-1.0; n]);
    assert_eq!(decoded.grass.len(), n);
    assert_eq!(decoded.water_flow.len(), span * span);
    assert_eq!(
        decoded.water_flow[1],
        WaterFlow {
            x: 1,
            z: 2,
            foam: 3,
            profile: 1
        }
    );
    assert_eq!(decoded.paint[0].weights, vec![0.25; n]);
    assert_eq!(
        decoded.paint[0].variants.as_deref(),
        Some(&vec![7; 72 * 72][..])
    );
}

#[test]
fn texture_layout_follows_the_grid() {
    let parsed = parse(map()).unwrap();
    // Without a texture grid: the legacy four-unit raster over the halo-inclusive span.
    let legacy = parsed.authoring.terrain.texture_layout();
    assert_eq!(
        (legacy.cell_size, legacy.cells, legacy.count),
        (4.0, [72, 72], 72 * 72)
    );
    assert_eq!(legacy.start, [-16.5, -16.5]);
    let mut terrain = parsed.authoring.terrain.clone();
    terrain.texture_grid = Some(canopy_content::map::TextureGrid {
        cell_size: 5.0,
        origin: [2.0, -0.5],
    });
    let layout = terrain.texture_layout();
    assert_eq!(layout.start, [-18.0, -20.5]);
    assert_eq!(layout.cells, [58, 59]);
    assert_eq!(layout.count, 58 * 59);
}

#[test]
fn grid_values_are_checked() {
    let n = vertex_count(SIZE);
    let span = (SIZE + 2 * MAP_HALO) as usize;
    let mut too_high = vec![0.0_f32; n];
    too_high[5] = 24.5;
    let mut unknown_profile = vec![128u8; span * span * 4];
    unknown_profile[3] = 1;
    for (pointer, value) in [
        ("/authoring/terrain/heights", floats(too_high)),
        ("/authoring/terrain/heights", floats([f32::NAN].repeat(n))),
        ("/authoring/terrain/grass", floats([1.5].repeat(n))),
        ("/authoring/terrain/waterHeights", floats([-17.5].repeat(n))),
        ("/authoring/terrain/rock", floats([0.0].repeat(n - 1))),
        ("/authoring/terrain/waterFlow", base64(&unknown_profile)),
    ] {
        let mut document = map();
        set(&mut document, pointer, json!(value));
        assert!(parse(document).is_err(), "{pointer}");
    }
    let mut document = map();
    set(
        &mut document,
        "/authoring/terrain/paint",
        json!([{"material": "m", "weights": floats([0.0].repeat(n)), "variants": base64(&[0; 10])}]),
    );
    assert!(
        parse(document)
            .unwrap_err()
            .to_string()
            .contains("paint.0.variants")
    );
}
