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
    terrain::{LayerKind, TerrainDesc, TerrainLayer, TextureGrid, WaterLook},
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
    let path = std::env::temp_dir().join(format!("canopy-render-test-{}.pack", std::process::id()));
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
        water_look: WaterLook {
            shallow: [0.1, 0.3, 0.3],
            deep: [0.0, 0.1, 0.15],
            clarity: 0.85,
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
        glows: Default::default(),
        point_lights: Vec::new(),
        actor_models: Vec::new(),
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

    assert_eq!(report.passes, ["cull", "skin", "shadow", "forward", "post"]);
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
}
