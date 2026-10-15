//! Renders a tiny test-owned skinned model offscreen: two stacked blocks on two joints and
//! one clip that slides the upper block sideways. The picture must follow the pose the
//! instance asks for, through the palette pass and the instanced draw. Skipped (passes)
//! without a GPU adapter and on software adapters.

use canopy_assets::{
    Aabb, AlphaMode, BlobBuilder, Clip, DepthStream, EntryKind, GroundContact, IndexFormat, Joint,
    Material, MaterialModel, MaterialTextures, ModelMeta, PackWriter, Prototype, Section, Skeleton,
    SkinVertex, Vertex, VertexColor,
};
use canopy_render::{
    Environment, Gpu, Renderer, World,
    actors::{ActorInstance, Pose},
    camera::{FlatGround, RtsCamera},
    capture::{CAPTURE_FORMAT, Capture},
    device::DeviceOptions,
    environment::{BiomeConfig, LightingConfig},
};
use glam::{Mat4, Vec3};

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

/// Box of the quantised positions: x, z in [-3, 3], y in [0, 2].
const BOX_MIN: [f32; 3] = [-3.0, 0.0, -3.0];
const BOX_EXTENT: [f32; 3] = [6.0, 2.0, 6.0];

fn quantise(p: [f32; 3]) -> [u16; 4] {
    let q = |i: usize| (((p[i] - BOX_MIN[i]) / BOX_EXTENT[i]) * 65535.0).round() as u16;
    [q(0), q(1), q(2), 0]
}

/// A 1 m block of joint `joint` spanning y0..y0+1, centred on x = z = 0.
fn block(
    vertices: &mut Vec<Vertex>,
    skin: &mut Vec<SkinVertex>,
    indices: &mut Vec<u16>,
    y0: f32,
    joint: u8,
) {
    let corners = |x: f32, y: f32, z: f32| [x * 0.5, y0 + y, z * 0.5];
    // Six faces as quads; normals point up (plausible enough for an emissive block).
    let faces = [
        [
            corners(-1.0, 0.0, 1.0),
            corners(1.0, 0.0, 1.0),
            corners(1.0, 1.0, 1.0),
            corners(-1.0, 1.0, 1.0),
        ],
        [
            corners(1.0, 0.0, -1.0),
            corners(-1.0, 0.0, -1.0),
            corners(-1.0, 1.0, -1.0),
            corners(1.0, 1.0, -1.0),
        ],
        [
            corners(1.0, 0.0, 1.0),
            corners(1.0, 0.0, -1.0),
            corners(1.0, 1.0, -1.0),
            corners(1.0, 1.0, 1.0),
        ],
        [
            corners(-1.0, 0.0, -1.0),
            corners(-1.0, 0.0, 1.0),
            corners(-1.0, 1.0, 1.0),
            corners(-1.0, 1.0, -1.0),
        ],
        [
            corners(-1.0, 1.0, 1.0),
            corners(1.0, 1.0, 1.0),
            corners(1.0, 1.0, -1.0),
            corners(-1.0, 1.0, -1.0),
        ],
        [
            corners(-1.0, 0.0, -1.0),
            corners(1.0, 0.0, -1.0),
            corners(1.0, 0.0, 1.0),
            corners(-1.0, 0.0, 1.0),
        ],
    ];
    for face in faces {
        let base = vertices.len() as u16;
        for p in face {
            vertices.push(Vertex {
                position: quantise(p),
                normal: [0, 32767],
                uv: [0, 0],
            });
            skin.push(SkinVertex {
                joints: [joint, 0, 0, 0],
                weights: [255, 0, 0, 0],
            });
        }
        indices.extend_from_slice(&[base, base + 1, base + 2, base, base + 2, base + 3]);
    }
}

fn translation(x: f32) -> [f32; 12] {
    [1.0, 0.0, 0.0, x, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0]
}

/// Two blocks (joints 0 and 1); clip "slide" moves joint 1 by +2 m over two frames.
fn model_blob() -> Vec<u8> {
    let mut b = BlobBuilder::new();
    let (mut vertices, mut skin, mut indices) = (Vec::new(), Vec::new(), Vec::new());
    block(&mut vertices, &mut skin, &mut indices, 0.0, 0);
    block(&mut vertices, &mut skin, &mut indices, 1.0, 1);
    let vertex_span = b.push(&vertices);
    let skin_span = b.push(&skin);
    let index_span = b.push(&indices);
    let depth_positions: Vec<[u16; 4]> = vertices.iter().map(|v| v.position).collect();
    let depth_span = b.push(&depth_positions);
    let depth_skin = b.push(&skin);
    let depth_indices = b.push(&indices);
    let matrices: Vec<[f32; 12]> = vec![
        translation(0.0),
        translation(0.0),
        translation(0.0),
        translation(2.0),
    ];
    let clip_span = b.push(&matrices);
    let bounds = Aabb {
        min: [-3.0, 0.0, -3.0],
        max: [3.0, 2.0, 3.0],
    };
    let material = Material {
        name: "red".into(),
        model: MaterialModel::Unit,
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
        asset: "test.blocks".into(),
        rest_bounds: Aabb {
            min: [-0.5, 0.0, -0.5],
            max: [0.5, 2.0, 0.5],
        },
        bounds,
        position_min: BOX_MIN,
        position_extent: BOX_EXTENT,
        uv_min: [0.0; 2],
        uv_extent: [1.0; 2],
        vertex_count: vertices.len() as u32,
        vertices: vertex_span,
        colors: None,
        leaves: None,
        skin: Some(skin_span),
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
            skin: Some(depth_skin),
            index_format: IndexFormat::U16,
            index_count: indices.len() as u32,
            indices: depth_indices,
            masked: Vec::new(),
        },
        lods: Vec::new(),
        materials: vec![material],
        skeleton: Some(Skeleton {
            joints: vec![
                Joint {
                    name: "base".into(),
                    parent: None,
                },
                Joint {
                    name: "top".into(),
                    parent: Some(0),
                },
            ],
        }),
        clips: vec![Clip {
            name: "slide".into(),
            duration: 1.0 / 30.0,
            frame_count: 2,
            matrices: clip_span,
            sockets: Default::default(),
        }],
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

/// Red pixels' mean column in the top and bottom halves of the picture.
fn red_columns(pixels: &[u8], width: u32, height: u32) -> (f32, f32, usize) {
    let mut sums = [(0.0f32, 0usize); 2];
    for (i, c) in pixels.chunks_exact(4).enumerate() {
        if c[0] > 200 && c[1] < 80 {
            let (x, y) = (i as u32 % width, i as u32 / width);
            let half = usize::from(y >= height / 2);
            sums[half].0 += x as f32;
            sums[half].1 += 1;
        }
    }
    let mean = |(s, n): (f32, usize)| if n == 0 { f32::NAN } else { s / n as f32 };
    (mean(sums[0]), mean(sums[1]), sums[0].1 + sums[1].1)
}

#[test]
fn a_skinned_model_follows_its_pose() {
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
    let environment = Environment::new(&lighting, &biome, 12.0, None);
    let (width, height) = (256, 256);
    let mut renderer = Renderer::new(&gpu, CAPTURE_FORMAT, [width, height], environment);
    let path = std::env::temp_dir().join(format!("canopy-skin-test-{}.pack", std::process::id()));
    {
        let file = std::fs::File::create(&path).unwrap();
        let mut writer = PackWriter::new(std::io::BufWriter::new(file)).unwrap();
        let at = writer.write_blob(&model_blob()).unwrap();
        writer.add_entry("blocks".into(), EntryKind::Model, at);
        writer.finish([0; 32]).unwrap();
    }
    let pack = canopy_assets::Pack::load(&path).unwrap();
    let world = World {
        actor_models: vec!["blocks".into(), "absent".into()],
        ..Default::default()
    };
    renderer.load_world(&gpu, &pack, &world).unwrap();
    std::fs::remove_file(&path).ok();
    assert!(renderer.actors().unwrap().is_ready(0));
    assert!(!renderer.actors().unwrap().is_ready(1));

    // Looking straight down would hide the slide's direction; the play camera's pitch keeps
    // x as screen x, the top block nearer the top of the picture.
    let mut camera = RtsCamera::new([0.0, 0.0], None);
    camera.zoom_range = (0.02, 2.0);
    camera.set_zoom(0.05);
    let view = camera.view(width as f32 / height as f32, &FlatGround);
    let capture = Capture::new(&gpu.device, width, height);
    let mut shoot = |time: f32| {
        let mut instance = ActorInstance::rest(0, Mat4::from_translation(Vec3::ZERO));
        instance.poses[0] = Pose {
            clip: 0,
            time,
            weight: 1.0,
        };
        instance.pose_count = 1;
        // A second instance of a model that failed to load draws nothing.
        let absent = ActorInstance::rest(1, Mat4::IDENTITY);
        renderer.set_actors(&[instance, absent]);
        let report = renderer.render(&gpu, &view, 0.0, &capture.view, Some(&|e| capture.copy(e)));
        let pixels = capture.read(&gpu.device).unwrap();
        (report, red_columns(&pixels, width, height))
    };
    let (report, (top_rest, bottom_rest, red_rest)) = shoot(0.0);
    assert_eq!(report.actors.visible, 1, "{:?}", report.actors);
    assert_eq!(report.actors.joints, 2);
    assert!(red_rest > 500, "rest coverage {red_rest}");
    // At rest both blocks stand on the same column.
    assert!(
        (top_rest - bottom_rest).abs() < 6.0,
        "{top_rest} {bottom_rest}"
    );
    let (_, (top_slid, bottom_slid, red_slid)) = shoot(1.0 / 30.0);
    assert!(red_slid > 500);
    // The lower block stays; the upper one moved right by about two block widths.
    assert!(
        (bottom_slid - bottom_rest).abs() < 6.0,
        "{bottom_slid} {bottom_rest}"
    );
    assert!(top_slid - top_rest > 20.0, "{top_slid} {top_rest}");
    // Half way through, frames mix linearly: the top block lies between.
    let (_, (top_half, _, _)) = shoot(0.5 / 30.0);
    assert!(
        top_half > top_rest + 8.0 && top_half < top_slid - 8.0,
        "{top_rest} {top_half} {top_slid}"
    );
}
