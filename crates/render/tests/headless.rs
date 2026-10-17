//! Renders a tiny test-owned world offscreen and checks the picture's basic invariants.
//! Skipped (passes) on machines without a GPU adapter, and on software adapters (WARP,
//! llvmpipe on CI runners), whose results say nothing about real GPUs.

use canopy_assets::{
    Aabb, AlphaMode, BlobBuilder, DepthStream, EntryKind, GroundContact, IndexFormat, Material,
    MaterialModel, MaterialTextures, ModelMeta, PackWriter, Prototype, Section, TextureAlpha,
    TextureDimension, TextureFormat, TextureMeta, Vertex, VertexColor,
};
use canopy_render::{
    Environment, Gpu, Renderer, World,
    camera::{CameraView, RtsCamera},
    capture::{CAPTURE_FORMAT, Capture},
    device::DeviceOptions,
    environment::{BiomeConfig, LightingConfig},
    statics::StaticInstance,
    terrain::{LayerKind, TerrainDesc, TerrainLayer, TextureGrid},
    water::{
        DEFAULT_WAVES, WaterDesc, WaterProfile,
        ripples::{Mover, WaterField},
    },
};
use glam::{Mat4, Vec3};
use std::path::PathBuf;

const LIGHTING: &str = r#"{
  "daylight": { "test": {
    "day": { "ambient": { "rgb": [255, 255, 255], "multiplier": 1 },
             "sky": { "rgb": [200, 220, 255], "multiplier": 1 },
             "sun": { "rgb": [255, 250, 240], "multiplier": 3 } },
    "night": { "ambient": { "rgb": [40, 40, 60], "multiplier": 1 },
               "sky": { "rgb": [20, 20, 40], "multiplier": 1 },
               "sun": { "rgb": [60, 60, 90], "multiplier": 1 } } } },
  "phases": { "dawnStart": 5.8, "dawnEnd": 6.2, "duskStart": 17.8, "duskEnd": 18.2 },
  "bounceStrength": 0.15,
  "groundProbe": { "up": [0.7, 0.7, 0.7], "side": [0.5, 0.5, 0.5], "down": [0.3, 0.3, 0.3] },
  "environment": { "intensity": 0.75, "irradiance": [[0.5, 0.5, 0.5], [0, 0, 0], [0, 0, 0], [0, 0, 0],
    [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0], [0, 0, 0]] },
  "shadow": { "resolution": 1024, "below": 6, "above": 12, "padding": 2, "step": 4 }
}"#;

const BIOME: &str = r##"{
  "name": "Test", "daylight": "test",
  "light": { "sunTint": "#ffffff", "sunStrength": 1, "sunDirection": 0, "sunHeight": 60,
    "ambientTint": "#ffffff", "ambientStrength": 0.5, "skyTint": "#ffffff", "bounceTint": "#ffffff",
    "fillStrength": 1, "shadowSoftness": 2 },
  "canopy": { "height": 140, "scale": 240, "coverage": 0.5, "softness": 0.2, "strength": 0.2,
    "sway": 1, "speed": 0.3, "cloudShadow": 0, "seed": 1 },
  "grade": { "exposure": 1, "contrast": 1, "saturation": 1, "highlightShoulder": 0,
    "shadowTint": "#ffffff", "highlightTint": "#ffffff", "splitStrength": 0, "shadowLift": 0,
    "vignette": 0, "daytimeLutStrength": 0 },
  "bloom": { "strength": 0, "threshold": 1, "knee": 0.1, "radius": 1 },
  "contact": { "strength": 0, "radius": 1, "bias": 0.1 },
  "atmosphere": { "enabled": false, "color": "#ffffff", "density": 0, "shaftDensity": 0,
    "baseHeight": 0, "heightFalloff": 4, "sunStrength": 1, "noiseScale": 0.1,
    "noiseStrength": 0.5, "driftSpeed": 0.3 },
  "terrain": {
    "soil": { "albedo": "a", "normal": "n", "tiling": 1, "blend": -1 },
    "grass": { "albedo": "a", "normal": "n", "tiling": 1, "blend": 0.5 },
    "dirt": { "albedo": "a", "normal": "n", "tiling": 1, "blend": 0.5 },
    "waterbed": { "albedo": "a", "normal": "n", "tiling": 1, "blend": -1 },
    "stones": { "albedo": "a", "normal": "n", "tiling": 1, "blend": 0.5 },
    "rock": { "albedo": "a", "normal": "n", "tiling": 1, "blend": -1 } },
  "water": "w",
  "groundCover": { "radius": 1.3, "strength": 0.8 },
  "surroundings": { "floorTexture": "tile/albedo", "floorColor": "#808080", "hazeColor": "#c0c0c0",
    "hazeStart": 170, "hazeEnd": 850 }
}"##;

fn texture_blob(format: TextureFormat, rgba: [u8; 4]) -> Vec<u8> {
    let mut b = BlobBuilder::new();
    let mut mips = Vec::new();
    for size in [4u32, 2, 1] {
        let texels: Vec<u8> = (0..size * size).flat_map(|_| rgba).collect();
        mips.push(b.push(&texels));
    }
    b.finish(
        EntryKind::Texture,
        &TextureMeta {
            format,
            dimension: TextureDimension::D2,
            width: 4,
            height: 4,
            depth: 1,
            source_width: 4,
            source_height: 4,
            alpha: TextureAlpha::Opaque,
            mips,
        },
    )
}

/// A 2 m cube around its origin, with a bright red emissive material.
fn cube_blob() -> Vec<u8> {
    let mut b = BlobBuilder::new();
    let mut vertices = Vec::new();
    let mut indices: Vec<u16> = Vec::new();
    // Six faces, four vertices each; positions quantised in the [-1, 1]×[0, 2]×[-1, 1] box.
    let faces: [([f32; 3], [f32; 3], [f32; 3]); 6] = [
        ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]),
        ([-1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0, 1.0, 0.0]),
        ([0.0, 1.0, 0.0], [0.0, 0.0, 1.0], [1.0, 0.0, 0.0]),
        ([0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        ([0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        ([0.0, 0.0, -1.0], [0.0, 1.0, 0.0], [1.0, 0.0, 0.0]),
    ];
    for (n, u, v) in faces {
        let base = vertices.len() as u16;
        for (su, sv) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
            let p: [f32; 3] = std::array::from_fn(|i| n[i] + u[i] * su + v[i] * sv);
            let q = |c: f32| ((c + 1.0) / 2.0 * 65535.0).round() as u16;
            let l1 = n.iter().map(|c: &f32| c.abs()).sum::<f32>();
            vertices.push(Vertex {
                position: [q(p[0]), q(p[1]), q(p[2]), 0],
                normal: [(n[0] / l1 * 32767.0) as i16, (n[1] / l1 * 32767.0) as i16],
                uv: [0, 0],
            });
        }
        // The z = −1 face of the octahedral map needs the folded encoding; the cube
        // only needs plausible normals, so −z keeps (0, 0) (→ +z) for simplicity.
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
    let vertex_span = b.push(&vertices);
    let index_span = b.push(&indices);
    let depth_positions: Vec<[u16; 4]> = vertices.iter().map(|v| v.position).collect();
    let depth_span = b.push(&depth_positions);
    let depth_indices = b.push(&indices);
    let bounds = Aabb {
        min: [-1.0; 3],
        max: [1.0; 3],
    };
    let material = Material {
        name: "red".into(),
        model: MaterialModel::Prop,
        base_color: [0.2, 0.0, 0.0, 1.0],
        emissive: [4.0, 0.0, 0.0],
        roughness: 1.0,
        metalness: 0.0,
        specular: 0.0,
        specular_color: [1.0; 3],
        alpha: AlphaMode::Opaque,
        alpha_cutoff: 0.5,
        double_sided: true,
        casts_shadow: true,
        vertex_color: VertexColor::None,
        painted: false,
        foliage: false,
        backside_lighting: 0.0,
        crown_normal: 0.0,
        snow_surface: false,
        team_color: None,
        normal_scale: 1.0,
        occlusion_strength: 1.0,
        textures: MaterialTextures::default(),
    };
    let meta = ModelMeta {
        asset: "test.cube".into(),
        rest_bounds: bounds,
        bounds,
        position_min: [-1.0, -1.0, -1.0],
        position_extent: [2.0, 2.0, 2.0],
        uv_min: [0.0; 2],
        uv_extent: [1.0; 2],
        vertex_count: vertices.len() as u32,
        vertices: vertex_span,
        colors: None,
        leaves: None,
        skin: None,
        index_format: IndexFormat::U16,
        indices: index_span,
        sections: vec![Section {
            material: 0,
            first_index: 0,
            index_count: indices.len() as u32,
            base_vertex: 0,
            vertex_count: vertices.len() as u32,
        }],
        depth: DepthStream {
            vertex_count: vertices.len() as u32,
            positions: depth_span,
            skin: None,
            index_format: IndexFormat::U16,
            index_count: indices.len() as u32,
            indices: depth_indices,
            masked: Vec::new(),
        },
        lods: Vec::new(),
        materials: vec![material],
        skeleton: None,
        clips: Vec::new(),
        states: Vec::new(),
        sockets: Vec::new(),
        wind: None,
        plant_height: None,
        ground: GroundContact::Pivot,
        ground_offset: 0.0,
        canopy: false,
        prototype: Prototype::default(),
    };
    b.finish(EntryKind::Model, &meta)
}

fn write_pack() -> PathBuf {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    let path = std::env::temp_dir().join(format!(
        "canopy-render-test-{}-{}.pack",
        std::process::id(),
        NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
    ));
    let file = std::fs::File::create(&path).unwrap();
    let mut writer = PackWriter::new(std::io::BufWriter::new(file)).unwrap();
    for (name, blob) in [
        (
            "tile/albedo",
            texture_blob(TextureFormat::Rgba8UnormSrgb, [90, 140, 60, 240]),
        ),
        (
            "tile/normal",
            texture_blob(TextureFormat::Rgba8Unorm, [128, 128, 255, 128]),
        ),
    ] {
        let at = writer.write_blob(&blob).unwrap();
        writer.add_entry(name.into(), EntryKind::Texture, at);
    }
    let at = writer.write_blob(&cube_blob()).unwrap();
    writer.add_entry("cube".into(), EntryKind::Model, at);
    writer.finish([0; 32]).unwrap();
    path
}

fn terrain() -> TerrainDesc {
    let span = 32u32;
    let n = ((span + 1) * (span + 1)) as usize;
    let heights = (0..n)
        .map(|i| {
            let (x, z) = ((i % 33) as f32 - 16.0, (i / 33) as f32 - 16.0);
            (2.0 - (x * x + z * z).sqrt() * 0.2).max(0.0)
        })
        .collect();
    TerrainDesc {
        span,
        origin: -16.0,
        heights,
        water: vec![-17.0; n],
        layers: vec![TerrainLayer {
            kind: LayerKind::Base { tiling: 1.0 },
            albedo: "tile/albedo".into(),
            normal: "tile/normal".into(),
            occlusion: None,
            tint: [1.0; 3],
            painted: false,
            mask: vec![255; n],
            cells: Vec::new(),
        }],
        grid: TextureGrid {
            start: [-16.5, -16.5],
            cell_size: 4.0,
            cells: [9, 9],
        },
        surroundings: None,
    }
}

#[test]
fn a_tiny_world_renders_without_holes() {
    let Some(gpu) = Gpu::headless(DeviceOptions::default()).unwrap() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    if gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu {
        eprintln!("software adapter {}: skipped", gpu.name());
        return;
    }
    let lighting: LightingConfig = serde_json::from_str(LIGHTING).unwrap();
    let biome: BiomeConfig = serde_json::from_str(BIOME).unwrap();
    biome.validate(&lighting).unwrap();
    let environment = Environment::new(&lighting, &biome, 12.0, None);
    let (width, height) = (256, 144);
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, [width, height], environment);
    let pack_path = write_pack();
    let pack = canopy_assets::Pack::load(&pack_path).unwrap();
    let world = World {
        terrain: Some(terrain()),
        statics: vec![StaticInstance {
            model: "cube".into(),
            transform: Mat4::from_translation(Vec3::new(0.0, 2.0, 0.0)),
            owner_color: None,
        }],
        ..Default::default()
    };
    renderer.load_world(&gpu, &pack, &world).unwrap();
    let mut camera = RtsCamera::new([0.0, 0.0], None);
    camera.zoom_range = (0.05, 2.0);
    camera.set_zoom(0.15);
    let view: CameraView = camera.view(
        width as f32 / height as f32,
        &canopy_render::camera::FlatGround,
    );
    let capture = Capture::new(&gpu.device, width, height);
    let report = renderer.render(
        &gpu,
        &view,
        1.0,
        &capture.view,
        Some(&|encoder| capture.copy(encoder)),
    );
    let pixels = capture.read(&gpu.device).unwrap();
    std::fs::remove_file(&pack_path).ok();
    if let Ok(out) = std::env::var("CANOPY_TEST_SHOT") {
        image::save_buffer(out, &pixels, width, height, image::ExtendedColorType::Rgba8).unwrap();
    }

    assert_eq!(
        report.passes,
        [
            "cull", "skin", "shadow", "forward", "post", "world_ui", "ui"
        ]
    );
    assert!(report.draws >= 4, "{report:?}");
    assert!(report.terrain_chunks == 1);
    let texels: Vec<[u8; 4]> = pixels
        .chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect();
    // Every pixel is written and opaque; nothing is black (NaNs and holes come out black).
    assert!(texels.iter().all(|t| t[3] == 255));
    let black = texels
        .iter()
        .filter(|t| t[0] < 4 && t[1] < 4 && t[2] < 4)
        .count();
    assert_eq!(black, 0, "{black} black pixels");
    // The emissive cube covers the middle of the view; lit ground surrounds it.
    let red = texels.iter().filter(|t| t[0] > 200 && t[1] < 80).count();
    let green = texels.iter().filter(|t| t[1] > t[0] && t[1] > t[2]).count();
    assert!(red > 200, "cube coverage {red}");
    assert!(green > 1000, "ground coverage {green}");
    // The cube stands in the middle: the centre column crosses it.
    let column = (0..height).map(|y| texels[(y * width + width / 2) as usize]);
    assert!(column.filter(|t| t[0] > 200 && t[1] < 80).count() > 10);

    // A building foundation on the mound's flank, its base above the falling ground:
    // the pad and bank draw (and cast a shadow) without holes.
    renderer.set_foundations(
        &gpu,
        &[canopy_render::terrain::foundations::Foundation {
            center: [6.0, 4.0],
            half: [3.0, 2.0],
            base: 1.2,
        }],
    );
    let with = renderer.render(
        &gpu,
        &view,
        1.0,
        &capture.view,
        Some(&|encoder| capture.copy(encoder)),
    );
    let pixels = capture.read(&gpu.device).unwrap();
    assert_eq!(with.foundations, 1);
    assert_eq!(with.draws, report.draws + 2, "{with:?}");
    let black = pixels
        .chunks_exact(4)
        .filter(|t| t[0] < 4 && t[1] < 4 && t[2] < 4)
        .count();
    assert_eq!(black, 0, "{black} black pixels with a foundation");
}

/// A basin west of the mound: bed −1 at the far west rising to the shore, water at 0.
fn lake() -> TerrainDesc {
    let mut t = terrain();
    for (i, h) in t.heights.iter_mut().enumerate() {
        let x = (i % 33) as f32 - 16.0;
        if x < -2.0 {
            *h = (-1.0 + (x + 16.0) * 0.08).min(*h);
        }
    }
    t.water = vec![0.0; t.heights.len()];
    t
}

fn gpu() -> Option<Gpu> {
    let gpu = Gpu::headless(DeviceOptions::default()).unwrap()?;
    if gpu.adapter.get_info().device_type == wgpu::DeviceType::Cpu {
        eprintln!("software adapter {}: skipped", gpu.name());
        return None;
    }
    Some(gpu)
}

#[test]
fn a_tiny_lake_renders_with_wakes_and_every_post_effect() {
    let Some(gpu) = gpu() else {
        eprintln!("no GPU: skipped");
        return;
    };
    let lighting: LightingConfig = serde_json::from_str(LIGHTING).unwrap();
    let mut biome: BiomeConfig = serde_json::from_str(BIOME).unwrap();
    biome.contact.strength = 0.4;
    biome.bloom.strength = 0.3;
    biome.atmosphere.enabled = true;
    biome.atmosphere.shaft_density = 0.002;
    biome.atmosphere.density = 0.0002;
    biome.validate(&lighting).unwrap();
    let environment = Environment::new(&lighting, &biome, 12.0, None);
    let (width, height) = (256, 144);
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, [width, height], environment);
    let pack_path = write_pack();
    let pack = canopy_assets::Pack::load(&pack_path).unwrap();
    let terrain = lake();
    let profile = WaterProfile {
        shallow: [0.024, 0.107, 0.117],
        deep: [0.008, 0.021, 0.032],
        clarity: 0.85,
        ripple_scale: 0.15,
        ripple_strength: 0.055,
        foam_strength: 0.02,
        reflection_strength: 0.5,
        caustic_strength: 0.18,
        cloud_strength: 0.03,
        flow_speed: 1.0,
        waves: DEFAULT_WAVES,
    };
    let world = World {
        water: Some(WaterDesc::still(profile, terrain.span)),
        terrain: Some(terrain.clone()),
        ..Default::default()
    };
    renderer.load_world(&gpu, &pack, &world).unwrap();
    let mut camera = RtsCamera::new([-6.0, 0.0], None);
    camera.zoom_range = (0.05, 2.0);
    camera.set_zoom(0.15);
    let view: CameraView = camera.view(width as f32 / height as f32, &terrain);
    // Bodies wading west to east through the lake for two seconds.
    for f in 0..120 {
        let t = f as f32 / 60.0;
        let movers: Vec<Mover> = (0..4)
            .map(|i| Mover {
                id: i,
                x: -12.0 + t * 2.0,
                z: i as f32 * 2.0 - 3.0,
                surface: 0,
                elevation: 0.0,
            })
            .collect();
        assert!(terrain.water(movers[0].x, 0.0) > terrain.walk(movers[0].x, 0.0, 0));
        renderer
            .ripples_mut()
            .update(&movers, &terrain, t, [-6.0, 0.0]);
    }
    assert!(renderer.ripples_mut().len() >= 8);

    let capture = Capture::new(&gpu.device, width, height);
    let shoot = |renderer: &mut Renderer| {
        let report = renderer.render(
            &gpu,
            &view,
            2.0,
            &capture.view,
            Some(&|encoder| capture.copy(encoder)),
        );
        let pixels: Vec<[u8; 4]> = capture
            .read(&gpu.device)
            .unwrap()
            .chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect();
        (report, pixels)
    };
    let (report, pixels) = shoot(&mut renderer);
    if let Ok(out) = std::env::var("CANOPY_TEST_LAKE") {
        let bytes: Vec<u8> = pixels.iter().flatten().copied().collect();
        image::save_buffer(out, &bytes, width, height, image::ExtendedColorType::Rgba8).unwrap();
    }
    assert_eq!(
        report.passes,
        [
            "cull", "skin", "shadow", "ripples", "caustics", "forward", "contact", "bloom", "haze",
            "post", "world_ui", "ui"
        ]
    );
    assert!(
        report.water.cells > 0 && report.water.rings > 0,
        "{report:?}"
    );
    let black = |p: &[[u8; 4]]| {
        p.iter()
            .filter(|t| t[0] < 4 && t[1] < 4 && t[2] < 4)
            .count()
    };
    assert_eq!(black(&pixels), 0);
    assert!(pixels.iter().all(|t| t[3] == 255));
    // The west half is water: darker and bluer than the dry ground in the east.
    let mean = |x0: u32, x1: u32| {
        let mut sum = [0f32; 3];
        let mut n = 0.0;
        for y in height / 3..height * 2 / 3 {
            for x in x0..x1 {
                let t = pixels[(y * width + x) as usize];
                for c in 0..3 {
                    sum[c] += f32::from(t[c]);
                }
                n += 1.0;
            }
        }
        sum.map(|v| v / n)
    };
    let water = mean(4, width / 5);
    let ground = mean(width * 4 / 5, width - 4);
    assert!(
        water[2] / water[0].max(1.0) > ground[2] / ground[0].max(1.0),
        "water {water:?} ground {ground:?}"
    );
    assert!(water[1] < ground[1], "water {water:?} ground {ground:?}");

    // Half render scale: the same picture at the output size, no holes.
    renderer.set_quality(
        &gpu,
        canopy_render::Quality {
            render_scale: 0.5,
            ..Default::default()
        },
    );
    assert_eq!(renderer.scene_size(), [width / 2, height / 2]);
    let (_, half) = shoot(&mut renderer);
    assert_eq!(black(&half), 0);
    let difference: f32 = pixels
        .iter()
        .zip(&half)
        .map(|(a, b)| {
            (0..3)
                .map(|c| (f32::from(a[c]) - f32::from(b[c])).abs())
                .sum::<f32>()
        })
        .sum::<f32>()
        / (pixels.len() * 3) as f32;
    assert!(difference < 12.0, "mean difference {difference}");
    std::fs::remove_file(&pack_path).ok();
}

/// The game UI pass draws the HUD's quads over the finished frame and blends them in
/// sRGB space through the capture's non-sRGB view, as the old browser HUD did (D33).
#[test]
fn the_ui_pass_composites_the_hud_in_srgb_space() {
    let Some(gpu) = gpu() else {
        return;
    };
    let lighting: LightingConfig = serde_json::from_str(LIGHTING).unwrap();
    let biome: BiomeConfig = serde_json::from_str(BIOME).unwrap();
    let environment = Environment::new(&lighting, &biome, 12.0, None);
    let (width, height) = (64, 32);
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, [width, height], environment);
    let capture = Capture::new(&gpu.device, width, height);
    let view: CameraView =
        RtsCamera::new([0.0, 0.0], None).view(2.0, &canopy_render::camera::FlatGround);
    let shoot = |renderer: &mut Renderer| {
        renderer.render(&gpu, &view, 0.0, &capture.view, Some(&|e| capture.copy(e)));
        capture.read(&gpu.device).unwrap()
    };
    let background = shoot(&mut renderer);
    let mut list = canopy_ui::DrawList {
        viewport: [width, height],
        ..Default::default()
    };
    let quad = |x: f32, alpha: u8| {
        canopy_ui::Quad::new(
            canopy_ui::QuadKind::Shape,
            canopy_ui::Rect::new(x, 0.0, 32.0, 32.0),
            canopy_ui::Color::rgba(255, 0, 0, alpha),
        )
    };
    list.quads = vec![quad(0.0, 255), quad(32.0, 128)];
    list.batches = vec![canopy_ui::Batch {
        texture: None,
        scissor: None,
        quads: 0..2,
    }];
    list.atlas.size = 64;
    let ui = renderer.ui_mut(&gpu);
    ui.submit(&gpu, &list);
    ui.set_target(Some(capture.ui_view.clone()));
    let hud = shoot(&mut renderer);
    let at = |pixels: &[u8], x: usize, y: usize| {
        let i = (y * width as usize + x) * 4;
        [pixels[i], pixels[i + 1], pixels[i + 2]]
    };
    assert_eq!(at(&hud, 16, 16), [255, 0, 0]);
    let under = at(&background, 48, 16);
    let over = at(&hud, 48, 16);
    let mix = |c: u8, d: u8| {
        (f32::from(c) * (1.0 - 128.0 / 255.0) + f32::from(d) * 128.0 / 255.0).round()
    };
    let expected = [mix(under[0], 255), mix(under[1], 0), mix(under[2], 0)];
    for i in 0..3 {
        assert!(
            (f32::from(over[i]) - expected[i]).abs() <= 2.0,
            "{over:?} vs {expected:?} over {under:?}"
        );
    }
    // Without a new HUD frame the pass draws nothing.
    assert_eq!(at(&shoot(&mut renderer), 16, 16), at(&background, 16, 16));
}

/// Effects, the fog of war, the world UI and the HUD images on the tiny world: each
/// changes the picture where it should.
#[test]
fn effects_fog_world_ui_and_hud_images_render() {
    use canopy_render::{
        PortraitSubject,
        actors::ActorInstance,
        effects::{Blend, EffectDraw, EffectFrame, Emitter, Geometry, RibbonQuad, kind},
        fog::FogCells,
        world_ui::{OverlayVertex, WorldUiFrame, overlay},
    };
    let Some(gpu) = gpu() else { return };
    let lighting: LightingConfig = serde_json::from_str(LIGHTING).unwrap();
    let biome: BiomeConfig = serde_json::from_str(BIOME).unwrap();
    let environment = Environment::new(&lighting, &biome, 12.0, None);
    let (width, height) = (256, 144);
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, [width, height], environment);
    let pack_path = write_pack();
    let pack = canopy_assets::Pack::load(&pack_path).unwrap();
    let world = World {
        terrain: Some(terrain()),
        actor_models: vec!["cube".into()],
        ..Default::default()
    };
    renderer.load_world(&gpu, &pack, &world).unwrap();
    renderer.bake_minimap(&gpu, 32, [0.0, 0.0, 32.0], 64);
    renderer.set_portrait(
        &gpu,
        Some(PortraitSubject {
            instance: ActorInstance::rest(0, Mat4::IDENTITY),
            building: false,
        }),
        64,
    );
    let mut camera = RtsCamera::new([0.0, 0.0], None);
    camera.zoom_range = (0.05, 2.0);
    camera.set_zoom(0.15);
    let view = camera.view(
        width as f32 / height as f32,
        &canopy_render::camera::FlatGround,
    );
    let capture = Capture::new(&gpu.device, width, height);
    let shoot = |renderer: &mut Renderer| {
        let report = renderer.render(
            &gpu,
            &view,
            1.0,
            &capture.view,
            Some(&|encoder| capture.copy(encoder)),
        );
        (report, capture.read(&gpu.device).unwrap())
    };
    let luminance = |pixels: &[u8], x0: u32, y0: u32, x1: u32, y1: u32| {
        let mut sum = 0.0;
        for y in y0..y1 {
            for x in x0..x1 {
                let i = ((y * width + x) * 4) as usize;
                sum += f64::from(pixels[i]) + f64::from(pixels[i + 1]) + f64::from(pixels[i + 2]);
            }
        }
        sum / f64::from((x1 - x0) * (y1 - y0) * 3)
    };
    let (report, plain) = shoot(&mut renderer);
    assert!(report.passes.contains(&"minimap") && report.passes.contains(&"portrait"));
    let centre = (
        width / 2 - 10,
        height / 2 - 10,
        width / 2 + 10,
        height / 2 + 10,
    );
    let before = luminance(&plain, centre.0, centre.1, centre.2, centre.3);

    // A white soft sphere over the middle and a ribbon brighten it.
    let mut effects = EffectFrame {
        emitters: vec![Emitter {
            position: [0.0, 3.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            color: [1.0, 1.0, 1.0, 1.0],
            accent: [1.0, 1.0, 1.0, 2.0],
            shape: [0.5, 0.0, 1.0, 1.0],
            motion: [1.0, 0.0, 0.0, 0.0],
            ints: [kind::BILLBOARD, 0, 1, 0],
            ..Emitter::default()
        }],
        parts: vec![0],
        draws: vec![EffectDraw {
            geometry: Geometry::Quad,
            blend: Blend::Additive,
            texture: None,
            first: 0,
            count: 1,
        }],
        ribbons: vec![RibbonQuad::segment(
            [-6.0, 3.0, 0.0],
            1.0,
            [6.0, 3.0, 0.0],
            1.0,
            1.0,
        )],
        lights: Vec::new(),
    };
    renderer.set_effects(&mut effects);
    let (report, lit) = shoot(&mut renderer);
    assert!(report.effects.draws >= 2, "{:?}", report.effects);
    let after = luminance(&lit, centre.0, centre.1, centre.2, centre.3);
    assert!(after > before + 20.0, "effects {before} → {after}");
    renderer.set_effects(&mut EffectFrame::default());

    // A red overlay quad lands where it says, over everything.
    let mut ui = WorldUiFrame::default();
    let v = |x: f32, y: f32| OverlayVertex {
        position: [x, y],
        color: [1.0, 0.0, 0.0, 1.0],
        kind: [overlay::SOLID, 0, 0, 0],
        ..Default::default()
    };
    ui.quad([v(10.0, 10.0), v(30.0, 10.0), v(30.0, 30.0), v(10.0, 30.0)]);
    renderer.set_world_ui(&mut ui);
    let (_, marked) = shoot(&mut renderer);
    let i = ((20 * width + 20) * 4) as usize;
    assert!(
        marked[i] > 240 && marked[i + 1] < 20,
        "{:?}",
        &marked[i..i + 4]
    );
    renderer.set_world_ui(&mut WorldUiFrame::default());

    // Unexplored everywhere: the ground goes black; in sight everywhere it is as before
    // where the map is (the map covers x, z ≥ 0 here).
    let cells = vec![0u8; 64 * 64];
    renderer.set_fog(
        &gpu,
        Some(FogCells {
            size: 64,
            cells: &cells,
            changed: None,
        }),
    );
    let (_, dark) = shoot(&mut renderer);
    let black = |p: &[u8]| {
        p.chunks_exact(4)
            .filter(|t| t[0] < 6 && t[1] < 6 && t[2] < 6)
            .count()
    };
    assert!(
        black(&dark) > (width * height / 3) as usize,
        "{}",
        black(&dark)
    );
    let seen = vec![2u8; 64 * 64];
    renderer.set_fog(
        &gpu,
        Some(FogCells {
            size: 64,
            cells: &seen,
            changed: None,
        }),
    );
    let (_, bright) = shoot(&mut renderer);
    assert!(black(&bright) < black(&dark));
    renderer.set_fog(&gpu, None);

    // The HUD images.
    let read = |texture: &wgpu::Texture| {
        let image = Capture::new(&gpu.device, 64, 64);
        let mut encoder = gpu.device.create_command_encoder(&Default::default());
        image.copy_from(&mut encoder, texture);
        gpu.queue.submit([encoder.finish()]);
        image.read(&gpu.device).unwrap()
    };
    let minimap = read(renderer.minimap_texture().unwrap());
    let lit = minimap
        .chunks_exact(4)
        .filter(|t| t[0] > 20 || t[1] > 20 || t[2] > 20)
        .count();
    assert!(lit > 1000, "minimap {lit}");
    let portrait = read(renderer.portrait_texture().unwrap());
    let covered = portrait.chunks_exact(4).filter(|t| t[3] > 200).count();
    assert!(covered > 200, "portrait coverage {covered}");
    assert_eq!(portrait[3], 0, "the portrait's corner is transparent");
    std::fs::remove_file(&pack_path).ok();
}
