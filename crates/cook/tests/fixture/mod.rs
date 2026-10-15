//! Test-owned data folders: tiny glTF binaries, PNGs and WAVs built in code, with the
//! manifest and indexes that publish them.

#![allow(dead_code)]

use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A glTF document under construction; `glb` writes the binary container.
pub struct Gltf {
    pub json: Value,
    bin: Vec<u8>,
}

impl Gltf {
    pub fn new() -> Self {
        Self {
            json: json!({
                "asset": {"version": "2.0", "generator": "canopy fixture"},
                "scene": 0,
                "scenes": [{"nodes": []}],
                "nodes": [], "meshes": [], "materials": [], "accessors": [], "bufferViews": [],
                "buffers": [{"byteLength": 0}],
            }),
            bin: Vec::new(),
        }
    }

    fn array(&mut self, key: &str) -> &mut Vec<Value> {
        self.json[key].as_array_mut().expect("array")
    }

    fn view(&mut self, bytes: &[u8]) -> usize {
        while !self.bin.len().is_multiple_of(4) {
            self.bin.push(0);
        }
        let offset = self.bin.len();
        self.bin.extend_from_slice(bytes);
        let views = self.array("bufferViews");
        views.push(json!({"buffer": 0, "byteOffset": offset, "byteLength": bytes.len()}));
        views.len() - 1
    }

    fn accessor(
        &mut self,
        bytes: &[u8],
        component: u32,
        kind: &str,
        count: usize,
        extra: Value,
    ) -> usize {
        let view = self.view(bytes);
        let mut accessor =
            json!({"bufferView": view, "componentType": component, "type": kind, "count": count});
        if let Value::Object(extra) = extra {
            for (k, v) in extra {
                accessor[k] = v;
            }
        }
        let accessors = self.array("accessors");
        accessors.push(accessor);
        accessors.len() - 1
    }

    pub fn floats(&mut self, values: &[[f32; 3]]) -> usize {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        let mut min = [f32::MAX; 3];
        let mut max = [f32::MIN; 3];
        for v in values {
            for k in 0..3 {
                min[k] = min[k].min(v[k]);
                max[k] = max[k].max(v[k]);
            }
        }
        self.accessor(
            &bytes,
            5126,
            "VEC3",
            values.len(),
            json!({"min": min, "max": max}),
        )
    }

    pub fn vec2(&mut self, values: &[[f32; 2]]) -> usize {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        self.accessor(&bytes, 5126, "VEC2", values.len(), json!({}))
    }

    pub fn vec4(&mut self, values: &[[f32; 4]]) -> usize {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        self.accessor(&bytes, 5126, "VEC4", values.len(), json!({}))
    }

    pub fn scalars(&mut self, values: &[f32]) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|f| f.to_le_bytes()).collect();
        let min = values.iter().copied().fold(f32::MAX, f32::min);
        let max = values.iter().copied().fold(f32::MIN, f32::max);
        self.accessor(
            &bytes,
            5126,
            "SCALAR",
            values.len(),
            json!({"min": [min], "max": [max]}),
        )
    }

    pub fn joints(&mut self, values: &[[u8; 4]]) -> usize {
        let bytes: Vec<u8> = values.iter().flatten().copied().collect();
        self.accessor(&bytes, 5121, "VEC4", values.len(), json!({}))
    }

    pub fn matrices(&mut self, values: &[[f32; 16]]) -> usize {
        let bytes: Vec<u8> = values
            .iter()
            .flatten()
            .flat_map(|f| f.to_le_bytes())
            .collect();
        self.accessor(&bytes, 5126, "MAT4", values.len(), json!({}))
    }

    pub fn indices(&mut self, values: &[u16]) -> usize {
        let bytes: Vec<u8> = values.iter().flat_map(|i| i.to_le_bytes()).collect();
        self.accessor(&bytes, 5123, "SCALAR", values.len(), json!({}))
    }

    /// Embeds a PNG and returns its texture index.
    pub fn texture(&mut self, png: &[u8]) -> usize {
        let view = self.view(png);
        let images = self
            .json
            .as_object_mut()
            .unwrap()
            .entry("images")
            .or_insert(json!([]));
        images
            .as_array_mut()
            .unwrap()
            .push(json!({"bufferView": view, "mimeType": "image/png"}));
        let image = images.as_array().unwrap().len() - 1;
        let textures = self
            .json
            .as_object_mut()
            .unwrap()
            .entry("textures")
            .or_insert(json!([]));
        textures
            .as_array_mut()
            .unwrap()
            .push(json!({"source": image}));
        textures.as_array().unwrap().len() - 1
    }

    pub fn material(&mut self, material: Value) -> usize {
        let materials = self.array("materials");
        materials.push(material);
        materials.len() - 1
    }

    pub fn mesh(&mut self, primitives: Vec<Value>) -> usize {
        let meshes = self.array("meshes");
        meshes.push(json!({"primitives": primitives}));
        meshes.len() - 1
    }

    pub fn node(&mut self, node: Value) -> usize {
        let nodes = self.array("nodes");
        nodes.push(node);
        nodes.len() - 1
    }

    pub fn root(&mut self, node: usize) {
        self.json["scenes"][0]["nodes"]
            .as_array_mut()
            .unwrap()
            .push(json!(node));
    }

    pub fn glb(&self) -> Vec<u8> {
        let mut json = self.json.clone();
        json["buffers"][0]["byteLength"] = json!(self.bin.len());
        let mut text = serde_json::to_vec(&json).unwrap();
        while !text.len().is_multiple_of(4) {
            text.push(b' ');
        }
        let mut bin = self.bin.clone();
        while !bin.len().is_multiple_of(4) {
            bin.push(0);
        }
        let total = 12 + 8 + text.len() + 8 + bin.len();
        let mut out = Vec::with_capacity(total);
        out.extend_from_slice(b"glTF");
        out.extend_from_slice(&2u32.to_le_bytes());
        out.extend_from_slice(&(total as u32).to_le_bytes());
        out.extend_from_slice(&(text.len() as u32).to_le_bytes());
        out.extend_from_slice(b"JSON");
        out.extend_from_slice(&text);
        out.extend_from_slice(&(bin.len() as u32).to_le_bytes());
        out.extend_from_slice(b"BIN\0");
        out.extend_from_slice(&bin);
        out
    }
}

/// An RGBA PNG whose pixel (x, y) is `pixel(x, y)`.
pub fn png(width: u32, height: u32, pixel: impl Fn(u32, u32) -> [u8; 4]) -> Vec<u8> {
    let mut image = image::RgbaImage::new(width, height);
    for (x, y, p) in image.enumerate_pixels_mut() {
        *p = image::Rgba(pixel(x, y));
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image.write_to(&mut out, image::ImageFormat::Png).unwrap();
    out.into_inner()
}

/// Alpha of the team texture: a pattern no lossy codec keeps exactly.
pub fn team_alpha(x: u32, y: u32) -> u8 {
    ((x * 37 + y * 101) % 256) as u8
}

/// A static building: a team-coloured quad on a node scaled ×2 (y from −1 to 1) and an
/// alpha-tested leaf triangle.
pub fn building() -> Vec<u8> {
    let mut g = Gltf::new();
    let team_png = png(8, 8, |x, y| {
        [200, (x * 30) as u8, (y * 30) as u8, team_alpha(x, y)]
    });
    let team_texture = g.texture(&team_png);
    let leaf_png = png(4, 4, |x, _| [40, 160, 40, if x < 2 { 255 } else { 0 }]);
    let leaf_texture = g.texture(&leaf_png);
    let team = g.material(json!({
        "name": "Team",
        "pbrMetallicRoughness": {"baseColorTexture": {"index": team_texture}, "metallicFactor": 0.0},
        "extras": {"teamColorMask": "baseColorAlpha", "teamColorSourceMax": 0.5, "teamColorDefault": [0.1, 0.2, 0.6]},
    }));
    let leaf = g.material(json!({
        "name": "Leaf",
        "alphaMode": "MASK",
        "alphaCutoff": 0.4,
        "doubleSided": true,
        "pbrMetallicRoughness": {"baseColorTexture": {"index": leaf_texture}},
        "extras": {"foliage": true, "noShadow": false},
    }));
    let quad_positions = g.floats(&[
        [-0.5, -0.5, 0.0],
        [0.5, -0.5, 0.0],
        [0.5, 0.5, 0.0],
        [-0.5, 0.5, 0.0],
    ]);
    let quad_normals = g.floats(&[[0.0, 0.0, 1.0]; 4]);
    let quad_uvs = g.vec2(&[[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]]);
    let quad_indices = g.indices(&[0, 1, 2, 0, 2, 3]);
    let leaf_positions = g.floats(&[[0.0, 0.5, 0.0], [0.2, 0.7, 0.0], [0.0, 0.9, 0.1]]);
    let leaf_normals = g.floats(&[[1.0, 0.0, 0.0]; 3]);
    let leaf_uvs = g.vec2(&[[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]]);
    let mesh = g.mesh(vec![
        json!({"attributes": {"POSITION": quad_positions, "NORMAL": quad_normals, "TEXCOORD_0": quad_uvs}, "indices": quad_indices, "material": team}),
        json!({"attributes": {"POSITION": leaf_positions, "NORMAL": leaf_normals, "TEXCOORD_0": leaf_uvs}, "material": leaf}),
    ]);
    let child = g.node(json!({"name": "body", "mesh": mesh}));
    let root = g.node(json!({"name": "root", "scale": [2.0, 2.0, 2.0], "children": [child]}));
    g.root(root);
    g.glb()
}

/// A skinned strip: joint `root` at the origin, joint `arm` at (0, 1, 0) with a socket
/// node one metre further. Clip `bend` turns `arm` 90° about +Z over one second.
pub fn creature() -> Vec<u8> {
    let mut g = Gltf::new();
    let skin_material = g.material(
        json!({"name": "Skin", "pbrMetallicRoughness": {"baseColorFactor": [0.8, 0.5, 0.3, 1.0]}}),
    );
    let positions = g.floats(&[[0.0, 0.0, 0.0], [0.0, 2.0, 0.0], [1.0, 2.0, 0.0]]);
    let normals = g.floats(&[[0.0, 0.0, 1.0]; 3]);
    let joints = g.joints(&[[0, 0, 0, 0], [1, 0, 0, 0], [1, 0, 0, 0]]);
    let weights = g.vec4(&[[1.0, 0.0, 0.0, 0.0]; 3]);
    let mesh = g.mesh(vec![json!({
        "attributes": {"POSITION": positions, "NORMAL": normals, "JOINTS_0": joints, "WEIGHTS_0": weights},
        "material": skin_material,
    })]);
    let identity: [f32; 16] = [
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ];
    let mut arm_inverse = identity;
    arm_inverse[13] = -1.0;
    let ibms = g.matrices(&[identity, arm_inverse]);
    let socket = g.node(json!({"name": "socket_tip", "translation": [0.0, 1.0, 0.0]}));
    let arm = g.node(json!({"name": "arm", "translation": [0.0, 1.0, 0.0], "children": [socket]}));
    let root_joint = g.node(json!({"name": "root", "children": [arm]}));
    let body = g.node(json!({"name": "body", "mesh": mesh, "skin": 0}));
    g.json["skins"] = json!([{"joints": [root_joint, arm], "inverseBindMatrices": ibms}]);
    let times = g.scalars(&[0.0, 1.0]);
    let half = std::f32::consts::FRAC_1_SQRT_2;
    let rotations = g.vec4(&[[0.0, 0.0, 0.0, 1.0], [0.0, 0.0, half, half]]);
    g.json["animations"] = json!([{
        "name": "bend",
        "samplers": [{"input": times, "output": rotations, "interpolation": "LINEAR"}],
        "channels": [{"sampler": 0, "target": {"node": arm, "path": "rotation"}}],
    }]);
    g.root(root_joint);
    g.root(body);
    g.glb()
}

/// A mono 16-bit WAV.
pub fn wav(samples: &[i16]) -> Vec<u8> {
    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: 22_050,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let mut out = std::io::Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut out, spec).unwrap();
    for &s in samples {
        writer.write_sample(s).unwrap();
    }
    writer.finalize().unwrap();
    out.into_inner()
}

/// A fresh empty folder under the system temp directory.
pub fn temp_dir(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("canopy-cook-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

pub const BUILDING: &str = "assets/library/asset.fixture.building/geometry.glb";
pub const CREATURE: &str = "assets/library/asset.fixture.creature/geometry.glb";
pub const ICON: &str = "assets/library/asset.fixture.icon/image.png";
pub const SOUND: &str = "assets/library/asset.fixture.sound/audio.wav";
pub const SPARK: &str = "assets/library/asset.fixture.spark/image.png";

fn output(data: &Path, role: &str, path: &str) -> Value {
    let bytes = std::fs::metadata(data.join(path)).unwrap().len();
    json!({"role": role, "path": path, "sha256": "0".repeat(64), "bytes": bytes})
}

/// Writes a data folder with a building, a creature, an icon and a sound, published the
/// way the asset studio publishes them. `customise` may edit the model index first.
pub fn data_folder(name: &str, customise: impl FnOnce(&mut Value)) -> PathBuf {
    let data = temp_dir(name);
    let write = |path: &str, bytes: &[u8]| {
        let file = data.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, bytes).unwrap();
    };
    write(BUILDING, &building());
    write(CREATURE, &creature());
    write(
        ICON,
        &png(16, 16, |x, y| [(x * 16) as u8, (y * 16) as u8, 90, 255]),
    );
    write(SOUND, &wav(&[0, 1000, -1000, 32767, -32768]));
    write(
        SPARK,
        &png(8, 8, |x, y| [255, 200, 90, ((x + y) * 16) as u8]),
    );
    let record = |id: &str, kind: &str, profile: &str, outputs: Vec<Value>, scenery: Value| {
        json!({"id": id, "name": id, "kind": kind, "tags": [], "revision": 1, "profile": profile,
               "outputs": outputs, "render": [], "scenery": scenery})
    };
    let manifest = json!({"version": 1, "records": [
        record("asset.fixture.building", "model", "building", vec![output(&data, "model", BUILDING)],
            json!([{"id": "fixture-hut", "name": "Hut", "category": "landmark", "type": "prop",
                    "file": "library/asset.fixture.building/geometry.glb",
                    "light": {"x": 0.0, "y": 2.0, "z": 0.0, "color": "#ffffff", "intensity": 2.0, "range": 6.0},
                    "blocker": {"width": 2.0, "depth": 1.0}}])),
        record("asset.fixture.creature", "model", "base", vec![output(&data, "model", CREATURE)], json!([])),
        record("asset.fixture.icon", "icon", "icon", vec![output(&data, "image", ICON)], json!([])),
        record("asset.fixture.sound", "audio", "audio", vec![output(&data, "data", SOUND)], json!([])),
        record("asset.fixture.spark", "icon", "icon", vec![output(&data, "image", SPARK)], json!([])),
    ]});
    let transform = |forward: &str| json!({"scale": 1.0, "pivot": [0.0, 0.0, 0.0], "up": "Y", "forward": forward});
    let mut models = json!([
        {"id": "asset.fixture.building", "transform": transform("-Z"), "groundContact": "terrain",
         "capabilities": {"teamColor": {"mode": "mask", "slots": ["Team"]}, "groundContact": {"mode": "terrain"}},
         "geometry": [BUILDING], "scenery": ["fixture-hut"]},
        {"id": "asset.fixture.creature", "transform": transform("+Z"), "groundContact": "pivot",
         "capabilities": {
             "groundContact": {"mode": "pivot"},
             "animations": [{"semantic": "attack", "clip": "bend", "loop": false, "events": [{"name": "hit", "time": 0.5}]}],
             "sockets": [{"name": "tip", "node": "socket_tip", "offset": [0.0, 0.0, 0.0]}]},
         "geometry": [CREATURE], "scenery": []},
    ]);
    customise(&mut models);
    write(
        "assets/manifest.json",
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    );
    write(
        "assets/authoring/models.json",
        &serde_json::to_vec_pretty(&models).unwrap(),
    );
    write("assets/authoring/catalogue.json", b"[]");
    write("assets/maps/previews/index.json", b"{}");
    let layer = |id: &str, shape: &str| {
        json!({"id": id, "shape": shape, "colour": "#ffffff", "accent": "#000000",
               "durationTicks": 40, "count": 1, "size": 1.0, "height": 0.0})
    };
    let mut sigil = layer("sigil", "billboard");
    sigil["texture"] = json!({"asset": "asset.fixture.spark", "role": "image", "index": 1});
    let mut hut = layer("hut", "mesh");
    hut["model"] = json!({"asset": "asset.fixture.building", "role": "geometry", "index": 1,
                          "rotation": {"x": 0.0, "y": 90.0, "z": 0.0}});
    let effects = json!({"schemaVersion": 1, "effects": [
        {"schemaVersion": 1, "id": "effect.fixture.sigil", "name": "Sigil", "durationTicks": 40,
         "layers": [sigil, hut]},
    ]});
    write(
        "content/effects/published.json",
        &serde_json::to_vec_pretty(&effects).unwrap(),
    );
    data
}
