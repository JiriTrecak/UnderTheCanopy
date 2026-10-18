//! The forest beyond the map border (old `ForestSurroundingsLayer`): giant trunks in
//! rings around the map with forked limbs, giant mushrooms and fallen logs near the
//! border, and an overhead sheet of broad leaves. Render only: nothing is added to the
//! map, pathing, picking or fog of war. The ground floor around the map is drawn with the
//! terrain.
//!
//! Placement reproduces the old generator exactly (its seeded LCG, anchor rings, forks
//! and leaf grid), so the same biome and map size give the same forest. Like the old
//! layer, the tall parts of trunks beyond the border dissolve and the limbs and leaves
//! disappear as the camera pitches down to RTS views (`view_fade`); everything hazes
//! with distance towards the biome's haze colour.

use crate::{camera::CameraView, device::GpuFeatures, targets, terrain::TerrainDesc, texture};
use anyhow::{Context, Result};
use bytemuck::{Pod, Zeroable};
use glam::{EulerRot, Mat4, Quat, Vec3};
use wgpu::util::DeviceExt;

/// One ring of trunks: distance beyond the border, spacing along it and trunk height.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TrunkRing {
    pub distance: f32,
    pub spacing: f32,
    pub height: f32,
}

/// What the scene resolves for the forest.
#[derive(Debug, Clone, PartialEq)]
pub struct ForestDesc {
    /// Playable cells per side.
    pub map_size: u32,
    /// Pack model entries.
    pub trunk: String,
    pub mushroom: Option<String>,
    pub log: Option<String>,
    /// Pack texture of the limbs' bark.
    pub limb_texture: Option<String>,
    pub rings: Vec<TrunkRing>,
    /// Linear leaf colours.
    pub leaf_colors: Vec<[f32; 3]>,
    pub leaf_spacing: f32,
    pub leaf_length: f32,
    /// The biome canopy's seed and height (the old layer shared them).
    pub seed: u32,
    pub canopy_height: f32,
}

impl ForestDesc {
    pub fn validate(&self) -> Result<(), String> {
        if self.rings.is_empty() || self.leaf_colors.is_empty() {
            return Err("the forest needs trunk rings and leaf colours".into());
        }
        if self
            .rings
            .iter()
            .any(|r| !(r.distance >= 0.0 && r.spacing > 0.0 && r.height > 0.0))
        {
            return Err("trunk rings need a distance, a positive spacing and height".into());
        }
        if !(self.leaf_spacing > 0.0 && self.leaf_length > 0.0) {
            return Err("leaf spacing and length must be positive".into());
        }
        Ok(())
    }
}

/// The old `forestRandom`: a 32-bit LCG returning values in [0, 1).
#[derive(Debug, Clone)]
pub struct ForestRandom(u32);

impl ForestRandom {
    pub fn new(seed: u32) -> Self {
        Self(seed)
    }

    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        f64::from(self.0) / 4_294_967_296.0
    }
}

/// A trunk position (old `forestAnchors`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Anchor {
    pub x: f64,
    pub z: f64,
    pub height: f64,
    pub yaw: f64,
    pub radius: f64,
    pub ring: usize,
}

/// Trunk positions on square rings around a `size` map: the clearance is the same at
/// corners and along edges, and the count grows with the perimeter.
pub fn anchors(size: u32, seed: u32, rings: &[TrunkRing]) -> Vec<Anchor> {
    let mut random = ForestRandom::new(seed);
    let size = f64::from(size);
    let mut out = Vec::new();
    for (index, ring) in rings.iter().enumerate() {
        let distance = f64::from(ring.distance);
        let span = size + distance * 2.0;
        let count = (span / f64::from(ring.spacing)).ceil().min(32.0) as usize;
        for side in 0..4 {
            for i in 0..count {
                let along =
                    -distance + span * (i as f64 + 0.2 + random.next() * 0.6) / count as f64;
                let outside = distance + random.next() * 18.0;
                let (x, z) = match side {
                    0 => (along, -outside),
                    1 => (size + outside, along),
                    2 => (size - along, size + outside),
                    _ => (-outside, size - along),
                };
                let height = f64::from(ring.height) * (0.85 + random.next() * 0.3);
                let yaw = random.next() * std::f64::consts::TAU;
                let radius = 13.0 + random.next() * 7.0;
                out.push(Anchor {
                    x,
                    z,
                    height,
                    yaw,
                    radius,
                    ring: index,
                });
            }
        }
    }
    out
}

/// A point on three.js's centripetal Catmull-Rom curve through `points` (open), at `t`.
fn catmull_rom(points: &[Vec3], t: f32) -> Vec3 {
    let l = points.len();
    let p = (l - 1) as f32 * t;
    let mut int = p.floor() as usize;
    let mut weight = p - int as f32;
    if weight == 0.0 && int == l - 1 {
        int = l - 2;
        weight = 1.0;
    }
    let p0 = if int > 0 {
        points[int - 1]
    } else {
        points[0] - points[1] + points[0]
    };
    let p1 = points[int];
    let p2 = points[int + 1];
    let p3 = if int + 2 < l {
        points[int + 2]
    } else {
        points[l - 1] - points[l - 2] + points[l - 1]
    };
    let mut dt1 = p1.distance_squared(p2).powf(0.25);
    let mut dt0 = p0.distance_squared(p1).powf(0.25);
    let mut dt2 = p2.distance_squared(p3).powf(0.25);
    if dt1 < 1e-4 {
        dt1 = 1.0;
    }
    if dt0 < 1e-4 {
        dt0 = dt1;
    }
    if dt2 < 1e-4 {
        dt2 = dt1;
    }
    let axis = |x0: f32, x1: f32, x2: f32, x3: f32| {
        let t1 = ((x1 - x0) / dt0 - (x2 - x0) / (dt0 + dt1) + (x2 - x1) / dt1) * dt1;
        let t2 = ((x2 - x1) / dt1 - (x3 - x1) / (dt1 + dt2) + (x3 - x2) / dt2) * dt1;
        let (c0, c1) = (x1, t1);
        let c2 = -3.0 * x1 + 3.0 * x2 - 2.0 * t1 - t2;
        let c3 = 2.0 * x1 - 2.0 * x2 + t1 + t2;
        c0 + c1 * weight + c2 * weight * weight + c3 * weight * weight * weight
    };
    Vec3::new(
        axis(p0.x, p1.x, p2.x, p3.x),
        axis(p0.y, p1.y, p2.y, p3.y),
        axis(p0.z, p1.z, p2.z, p3.z),
    )
}

/// A generated mesh: position, normal, uv per vertex (and an optional grey shade).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Mesh {
    pub vertices: Vec<MeshVertex>,
    pub indices: Vec<u32>,
}

#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub shade: f32,
    pub normal: [f32; 3],
    pub pad: f32,
    pub uv: [f32; 2],
    pub pad2: [f32; 2],
}

impl Mesh {
    /// Area-weighted vertex normals (three's `computeVertexNormals`).
    fn compute_normals(&mut self) {
        let mut normals = vec![Vec3::ZERO; self.vertices.len()];
        for tri in self.indices.chunks_exact(3) {
            let [a, b, c] = [tri[0], tri[1], tri[2]].map(|i| i as usize);
            let p = |i: usize| Vec3::from_array(self.vertices[i].position);
            let n = (p(c) - p(b)).cross(p(a) - p(b));
            normals[a] += n;
            normals[b] += n;
            normals[c] += n;
        }
        for (v, n) in self.vertices.iter_mut().zip(normals) {
            // Degenerate corners (the leaf's tips) face up.
            v.normal = n.try_normalize().unwrap_or(Vec3::Y).to_array();
        }
    }

    fn push(&mut self, position: Vec3, uv: [f32; 2], shade: f32) {
        self.vertices.push(MeshVertex {
            position: position.to_array(),
            shade,
            normal: [0.0; 3],
            pad: 0.0,
            uv,
            pad2: [0.0; 2],
        });
    }

    fn append(&mut self, other: &Mesh) {
        let base = self.vertices.len() as u32;
        self.vertices.extend_from_slice(&other.vertices);
        self.indices.extend(other.indices.iter().map(|i| i + base));
    }
}

/// A folded broad leaf with a raised midrib and a gently lobed outline, one unit long
/// (old `canopyLeaf`).
pub fn leaf_mesh() -> Mesh {
    let mut mesh = Mesh::default();
    let rows = 13;
    for i in 0..rows {
        let t = i as f32 / (rows - 1) as f32;
        let pi = std::f32::consts::PI;
        let w = (t * pi).sin().powf(0.7) * (0.31 + 0.035 * (t * pi * 10.0).cos());
        for j in -1i32..=1 {
            let jf = j as f32;
            mesh.push(
                Vec3::new(jf * w, (t * pi).sin() * 0.09 - jf * jf * w * 0.22, t - 0.5),
                [jf * 0.5 + 0.5, t],
                1.0 - jf.abs() * 0.045,
            );
        }
    }
    for i in 0..rows - 1 {
        for j in 0..2 {
            let a = (i * 3 + j) as u32;
            let b = a + 3;
            mesh.indices
                .extend_from_slice(&[a, b, a + 1, b, b + 1, a + 1]);
        }
    }
    mesh.compute_normals();
    mesh
}

/// A tapering tube along `points` (old `limbGeometry`).
fn limb_mesh(points: &[Vec3], radius: f32) -> Mesh {
    let mut mesh = Mesh::default();
    let n = points.len();
    for i in 0..n {
        let tangent = (points[(i + 1).min(n - 1)] - points[i.saturating_sub(1)]).normalize();
        let u = Vec3::Z.cross(tangent).normalize();
        let v = tangent.cross(u).normalize();
        let r = radius * (1.0 - i as f32 / n as f32).powf(1.2);
        for j in 0..=8 {
            let t = j as f32 / 8.0 * std::f32::consts::TAU;
            mesh.push(
                points[i] + u * t.cos() * r + v * t.sin() * r,
                [j as f32 / 8.0, i as f32 * 1.6],
                1.0,
            );
        }
    }
    for i in 0..n - 1 {
        for j in 0..8 {
            let a = (i * 9 + j) as u32;
            let b = a + 9;
            mesh.indices
                .extend_from_slice(&[a, a + 1, b, b, a + 1, b + 1]);
        }
    }
    mesh.compute_normals();
    mesh
}

/// A leaf instance: transform rows and colour.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Pod, Zeroable)]
pub struct Instance {
    pub rows: [[f32; 4]; 3],
    /// Linear colour (leaves) or white.
    pub color: [f32; 4],
}

impl Instance {
    fn new(m: Mat4, color: [f32; 3]) -> Self {
        let r = m.transpose();
        Self {
            rows: [
                r.x_axis.to_array(),
                r.y_axis.to_array(),
                r.z_axis.to_array(),
            ],
            color: [color[0], color[1], color[2], 1.0],
        }
    }
}

/// Everything the forest places, in the old generator's order.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Forest {
    pub trunks: Vec<Mat4>,
    pub mushrooms: Vec<Mat4>,
    pub logs: Vec<Mat4>,
    /// All limbs merged.
    pub limbs: Mesh,
    pub leaves: Vec<Instance>,
}

fn euler_xyz(x: f64, y: f64, z: f64) -> Quat {
    Quat::from_euler(EulerRot::XYZ, x as f32, y as f32, z as f32)
}

/// Generates the forest of `desc` around `terrain` for a trunk model whose rest bounds
/// are `trunk_bounds` (min, max).
pub fn generate(desc: &ForestDesc, terrain: &TerrainDesc, trunk_bounds: (Vec3, Vec3)) -> Forest {
    let _span = tracing::info_span!("forest_generate").entered();
    let size = desc.map_size;
    let sizef = f64::from(size);
    let mut random = ForestRandom::new(desc.seed.wrapping_add(size));
    let anchors = anchors(size, desc.seed, &desc.rings);
    let extent = trunk_bounds.1 - trunk_bounds.0;
    let trunk_radius = f64::from(extent.x.max(extent.z) * 0.5);
    let mut forest = Forest::default();
    let mut crown_points = Vec::new();
    let pi = std::f64::consts::PI;
    for a in &anchors {
        let clamp = |v: f64| v.min(sizef - 1.0).max(0.0) as f32;
        let y = f64::from(terrain.sample(clamp(a.x), clamp(a.z))) - 6.0;
        forest.trunks.push(Mat4::from_scale_rotation_translation(
            Vec3::new(
                (a.radius / trunk_radius) as f32,
                (a.height * 0.69 / f64::from(extent.y)) as f32,
                (a.radius / trunk_radius) as f32,
            ),
            euler_xyz(0.0, a.yaw, 0.0),
            Vec3::new(a.x as f32, y as f32, a.z as f32),
        ));
        // Forks above the clear bole; every limb starts inside the trunk.
        for limb in 0..5 {
            let angle = a.yaw + f64::from(limb) * pi * 0.5 + (random.next() - 0.5) * 0.45;
            let reach = if limb == 4 {
                8.0
            } else {
                40.0 + random.next() * 38.0
            };
            let base = if limb == 4 {
                0.63
            } else {
                0.57 + random.next() * 0.07
            };
            let (c, s) = (angle.cos(), angle.sin());
            let top = if limb == 4 { 1.06 } else { 0.94 };
            let p = |along: f64, height: f64| {
                Vec3::new(
                    (a.x + c * reach * along) as f32,
                    (y + a.height * height) as f32,
                    (a.z + s * reach * along) as f32,
                )
            };
            let points = [p(0.0, base), p(0.22, 0.75), p(0.6, 0.86), p(1.0, top)];
            crown_points.push(points[3]);
            crown_points.push(points[2]);
            let curve: Vec<Vec3> = (0..=9)
                .map(|d| catmull_rom(&points, d as f32 / 9.0))
                .collect();
            let radius = a.radius * if limb == 4 { 0.48 } else { 0.24 };
            forest.limbs.append(&limb_mesh(&curve, radius as f32));
        }
        if desc.mushroom.is_some() && a.ring < 2 && random.next() < 0.45 {
            let scale = 2.0 + random.next() * 2.0;
            forest.mushrooms.push(Mat4::from_scale_rotation_translation(
                Vec3::splat(scale as f32),
                euler_xyz(0.0, a.yaw, 0.0),
                Vec3::new(a.x as f32, y as f32, a.z as f32),
            ));
        }
        if desc.log.is_some() && a.ring == 0 && random.next() < 0.25 {
            forest.logs.push(Mat4::from_scale_rotation_translation(
                Vec3::new(1.9, 1.6, 1.9),
                euler_xyz(0.0, a.yaw, 0.08),
                Vec3::new(a.x as f32, (y + 4.0) as f32, a.z as f32),
            ));
        }
    }
    // A bounded grid gives the overhead sheet a fixed budget on any map size.
    let colors = &desc.leaf_colors;
    let color = |random: &mut ForestRandom| colors[(random.next() * colors.len() as f64) as usize];
    let margin = 340.0;
    let span = sizef + margin * 2.0;
    let step = f64::from(desc.leaf_spacing).max(span / 85.0);
    let height = f64::from(desc.canopy_height);
    let leaf_length = f64::from(desc.leaf_length);
    let mut z = -margin;
    while z < sizef + margin {
        let mut x = -margin;
        while x < sizef + margin {
            let px = x + random.next() * step;
            let pz = z + random.next() * step;
            // Three broad overlapping leaves per crown, with differing tilt and shade.
            for _ in 0..3 {
                let lx = px + (random.next() - 0.5) * step;
                let ly = height
                    + (px * 0.018).sin() * 12.0
                    + (pz * 0.025).cos() * 10.0
                    + (random.next() - 0.5) * 7.0;
                let lz = pz + (random.next() - 0.5) * step;
                let rx = (random.next() - 0.5) * 0.65;
                let ry = random.next() * pi * 2.0;
                let rz = (random.next() - 0.5) * 0.7;
                let length = leaf_length * (0.8 + random.next() * 0.8);
                let m = Mat4::from_scale_rotation_translation(
                    Vec3::splat(length as f32),
                    euler_xyz(rx, ry, rz),
                    Vec3::new(lx as f32, ly as f32, lz as f32),
                );
                let c = color(&mut random);
                forest.leaves.push(Instance::new(m, c));
            }
            x += step;
        }
        z += step;
    }
    // Every fork carries its own cluster of foliage at its branch height.
    for point in crown_points {
        for _ in 0..22 {
            let angle = random.next() * pi * 2.0;
            let r = random.next().sqrt() * 22.0;
            let p = Vec3::new(
                (f64::from(point.x) + angle.cos() * r) as f32,
                (f64::from(point.y) - 3.0 + random.next() * 9.0) as f32,
                (f64::from(point.z) + angle.sin() * r) as f32,
            );
            let rx = (random.next() - 0.5) * 0.7;
            let ry = random.next() * pi * 2.0;
            let rz = (random.next() - 0.5) * 0.55;
            let length = leaf_length * (0.6 + random.next() * 0.7);
            let m = Mat4::from_scale_rotation_translation(
                Vec3::splat(length as f32),
                euler_xyz(rx, ry, rz),
                p,
            );
            let c = color(&mut random);
            forest.leaves.push(Instance::new(m, c));
        }
    }
    forest
}

/// How visible the overhead parts are for a camera: 0 at RTS pitches, 1 when looking
/// near the horizon (old `viewFade`, from the view direction's height).
pub fn view_fade(camera: &CameraView) -> f32 {
    let direction = (camera.target - camera.eye).normalize_or_zero();
    ((direction.y + 0.65) / 0.3).clamp(0.0, 1.0)
}

// --- GPU -------------------------------------------------------------------------------

const KIND_MODEL: u32 = 0;
const KIND_LIMB: u32 = 1;
const KIND_LEAF: u32 = 2;

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct ForestFrame {
    // map size, view fade, day, 0
    params: [f32; 4],
    // rgb haze colour, a 0; haze start, end
    haze: [f32; 4],
    range: [f32; 4],
    // rgb sun tint
    sun: [f32; 4],
}

#[repr(C)]
#[derive(Debug, Clone, Copy, Pod, Zeroable)]
struct PartUniform {
    // xyz quantisation origin; w: kind
    position_min: [f32; 4],
    // xyz quantisation extent; w: flags (1: normal map)
    position_extent: [f32; 4],
    uv: [f32; 4],
    // base colour factor
    base_color: [f32; 4],
    // roughness, metalness, normal scale, painted
    material: [f32; 4],
}

/// A model's instances, culled against the camera every frame into its storage buffer.
struct Culled {
    all: Vec<Instance>,
    /// Centre and radius per instance.
    spheres: Vec<glam::Vec4>,
    storage: wgpu::Buffer,
    visible: u32,
}

struct Part {
    /// Index into the culled instance lists; `None` draws `instances` as uploaded.
    cull: Option<usize>,
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    index_format: wgpu::IndexFormat,
    index_count: u32,
    instances: u32,
    group: wgpu::BindGroup,
    pipeline: usize,
}

/// The forest's GPU resources: one draw per model section, one for the limbs and one
/// for the leaves.
pub struct ForestGpu {
    parts: Vec<Part>,
    culled: Vec<Culled>,
    scratch: Vec<Instance>,
    pipelines: Vec<wgpu::RenderPipeline>,
    frame: wgpu::Buffer,
    haze: ([f32; 3], [f32; 2]),
    map_size: f32,
    fade: f32,
    pub bytes: u64,
    pub stats: ForestStats,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ForestStats {
    pub trunks: u32,
    pub props: u32,
    pub leaves: u32,
    pub limb_triangles: u32,
}

impl ForestGpu {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        features: GpuFeatures,
        pack: &canopy_assets::Pack,
        desc: &ForestDesc,
        terrain: &TerrainDesc,
        haze: ([f32; 3], [f32; 2]),
        frame_layout: &wgpu::BindGroupLayout,
    ) -> Result<Self> {
        let _span = tracing::info_span!("forest_upload").entered();
        desc.validate().map_err(anyhow::Error::msg)?;
        let trunk_model = pack
            .model(&desc.trunk)
            .with_context(|| desc.trunk.clone())?;
        let b = &trunk_model.meta.rest_bounds;
        let forest = generate(
            desc,
            terrain,
            (Vec3::from_array(b.min), Vec3::from_array(b.max)),
        );
        let frag = wgpu::ShaderStages::FRAGMENT;
        let vf = wgpu::ShaderStages::VERTEX | wgpu::ShaderStages::FRAGMENT;
        let tex = |binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: frag,
            ty: wgpu::BindingType::Texture {
                sample_type: wgpu::TextureSampleType::Float { filterable: true },
                view_dimension: wgpu::TextureViewDimension::D2,
                multisampled: false,
            },
            count: None,
        };
        let buffer_entry = |binding, ty| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: vf,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("forest"),
            entries: &[
                buffer_entry(0, wgpu::BufferBindingType::Uniform),
                buffer_entry(1, wgpu::BufferBindingType::Uniform),
                buffer_entry(2, wgpu::BufferBindingType::Storage { read_only: true }),
                tex(3),
                tex(4),
                tex(5),
                wgpu::BindGroupLayoutEntry {
                    binding: 6,
                    visibility: frag,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let frame = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("forest frame"),
            size: std::mem::size_of::<ForestFrame>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("forest"),
            address_mode_u: wgpu::AddressMode::Repeat,
            address_mode_v: wgpu::AddressMode::Repeat,
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            mipmap_filter: wgpu::MipmapFilterMode::Linear,
            anisotropy_clamp: 4,
            ..Default::default()
        });
        let white = texture::solid(
            device,
            queue,
            "white",
            wgpu::TextureFormat::Rgba8UnormSrgb,
            [255; 4],
        )
        .create_view(&Default::default());
        let flat = texture::solid(
            device,
            queue,
            "flat normal",
            wgpu::TextureFormat::Rgba8Unorm,
            [128, 128, 255, 255],
        )
        .create_view(&Default::default());
        let rough = texture::solid(
            device,
            queue,
            "rough",
            wgpu::TextureFormat::Rgba8Unorm,
            [255, 255, 0, 255],
        )
        .create_view(&Default::default());
        let texture_bytes = std::cell::Cell::new(0u64);
        let upload = |name: &Option<String>| -> Result<Option<wgpu::TextureView>> {
            match name {
                Some(n) => {
                    let t = pack.texture(n).with_context(|| n.clone())?;
                    texture_bytes.set(
                        texture_bytes.get()
                            + t.meta.mips.iter().map(|m| u64::from(m.len)).sum::<u64>(),
                    );
                    Ok(Some(
                        texture::upload(device, queue, features, &t)
                            .create_view(&Default::default()),
                    ))
                }
                None => Ok(None),
            }
        };
        let mut bytes = 0u64;
        let storage_of = |instances: &[Instance]| {
            device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("forest instances"),
                contents: bytemuck::cast_slice(instances),
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            })
        };
        let make_group = |part: &PartUniform,
                          storage: &wgpu::Buffer,
                          views: [&wgpu::TextureView; 3]|
         -> (wgpu::BindGroup, u64) {
            let uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("forest part"),
                contents: bytemuck::bytes_of(part),
                usage: wgpu::BufferUsages::UNIFORM,
            });
            let group = device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("forest part"),
                layout: &layout,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: frame.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: uniform.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: storage.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(views[0]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::TextureView(views[1]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(views[2]),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                ],
            });
            (group, 0)
        };

        // Pipelines: quantised pack models, generated meshes (limbs, leaves).
        let module = crate::shader::module(
            device,
            "surroundings",
            crate::shader::LIT,
            include_str!("../shaders/surroundings.wgsl"),
        );
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("forest"),
            bind_group_layouts: &[Some(frame_layout), Some(&layout)],
            immediate_size: 0,
        });
        let model_attributes =
            wgpu::vertex_attr_array![0 => Unorm16x4, 1 => Snorm16x2, 2 => Unorm16x2];
        let mesh_attributes =
            wgpu::vertex_attr_array![0 => Float32x4, 1 => Float32x4, 2 => Float32x4];
        let pipeline =
            |entry: &str, attributes: &[wgpu::VertexAttribute], stride: u64, cull: bool| {
                device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("forest"),
                    layout: Some(&pipeline_layout),
                    vertex: wgpu::VertexState {
                        module: &module,
                        entry_point: Some(entry),
                        compilation_options: Default::default(),
                        buffers: &[Some(wgpu::VertexBufferLayout {
                            array_stride: stride,
                            step_mode: wgpu::VertexStepMode::Vertex,
                            attributes,
                        })],
                    },
                    primitive: wgpu::PrimitiveState {
                        cull_mode: cull.then_some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: Some(targets::scene_depth(true)),
                    multisample: targets::scene_multisample(false),
                    fragment: Some(wgpu::FragmentState {
                        module: &module,
                        entry_point: Some("fs_main"),
                        compilation_options: Default::default(),
                        targets: &targets::forward_targets(None, true),
                    }),
                    multiview_mask: None,
                    cache: None,
                })
            };
        let pipelines = vec![
            pipeline("vs_model", &model_attributes, 16, true),
            pipeline("vs_mesh", &mesh_attributes, 48, true),
            pipeline("vs_mesh", &mesh_attributes, 48, false),
        ];

        let mut parts = Vec::new();
        let mut culled = Vec::new();
        let models = [
            (Some(&desc.trunk), &forest.trunks),
            (desc.mushroom.as_ref(), &forest.mushrooms),
            (desc.log.as_ref(), &forest.logs),
        ];
        for (name, transforms) in models {
            let (Some(name), false) = (name, transforms.is_empty()) else {
                continue;
            };
            let model = pack.model(name).with_context(|| name.clone())?;
            let meta = &model.meta;
            let instances: Vec<Instance> = transforms
                .iter()
                .map(|&m| Instance::new(m, [1.0; 3]))
                .collect();
            // Culling spheres: the bounds' centre and radius, transformed.
            let (lo, hi) = (
                Vec3::from_array(meta.bounds.min),
                Vec3::from_array(meta.bounds.max),
            );
            let spheres = transforms
                .iter()
                .map(|m| {
                    let scale = m
                        .x_axis
                        .length()
                        .max(m.y_axis.length())
                        .max(m.z_axis.length());
                    m.transform_point3((lo + hi) * 0.5)
                        .extend((hi - lo).length() * 0.5 * scale)
                })
                .collect();
            let storage = storage_of(&instances);
            bytes += storage.size();
            let cull = culled.len();
            culled.push(Culled {
                all: instances,
                spheres,
                storage: storage.clone(),
                visible: 0,
            });
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("forest model"),
                contents: bytemuck::cast_slice(model.vertices()),
                usage: wgpu::BufferUsages::VERTEX,
            });
            bytes += vertices.size() + model.indices().bytes().len() as u64;
            for section in &meta.sections {
                let material = &meta.materials[section.material as usize];
                let t = &material.textures;
                let base = upload(&t.base_color)?;
                let normal = upload(&t.normal)?;
                let mr = upload(&t.metallic_roughness)?;
                let part = PartUniform {
                    position_min: [
                        meta.position_min[0],
                        meta.position_min[1],
                        meta.position_min[2],
                        KIND_MODEL as f32,
                    ],
                    position_extent: [
                        meta.position_extent[0],
                        meta.position_extent[1],
                        meta.position_extent[2],
                        if normal.is_some() { 1.0 } else { 0.0 },
                    ],
                    uv: [
                        meta.uv_min[0],
                        meta.uv_min[1],
                        meta.uv_extent[0],
                        meta.uv_extent[1],
                    ],
                    base_color: material.base_color,
                    material: [
                        material.roughness,
                        material.metalness,
                        material.normal_scale,
                        if material.painted { 1.0 } else { 0.0 },
                    ],
                };
                let (group, size) = make_group(
                    &part,
                    &storage,
                    [
                        base.as_ref().unwrap_or(&white),
                        normal.as_ref().unwrap_or(&flat),
                        mr.as_ref().unwrap_or(&rough),
                    ],
                );
                bytes += size;
                // Section-local indices into a slice of the model's index buffer.
                let index_size = match meta.index_format {
                    canopy_assets::IndexFormat::U16 => 2,
                    canopy_assets::IndexFormat::U32 => 4,
                };
                let start = section.first_index as usize * index_size;
                let end = start + section.index_count as usize * index_size;
                let mut index_bytes = model.indices().bytes()[start..end].to_vec();
                // Rebase to the section's first vertex (the vertex buffer holds them all).
                match meta.index_format {
                    canopy_assets::IndexFormat::U16 => {
                        let wide: Vec<u32> = bytemuck::cast_slice::<u8, u16>(&index_bytes)
                            .iter()
                            .map(|&i| u32::from(i) + section.base_vertex)
                            .collect();
                        index_bytes = bytemuck::cast_slice(&wide).to_vec();
                    }
                    canopy_assets::IndexFormat::U32 => {
                        let wide: Vec<u32> = bytemuck::cast_slice::<u8, u32>(&index_bytes)
                            .iter()
                            .map(|&i| i + section.base_vertex)
                            .collect();
                        index_bytes = bytemuck::cast_slice(&wide).to_vec();
                    }
                }
                let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("forest model indices"),
                    contents: &index_bytes,
                    usage: wgpu::BufferUsages::INDEX,
                });
                parts.push(Part {
                    vertices: vertices.clone(),
                    indices,
                    index_format: wgpu::IndexFormat::Uint32,
                    index_count: section.index_count,
                    instances: 0,
                    cull: Some(cull),
                    group,
                    pipeline: 0,
                });
            }
        }
        let mesh_part = |mesh: &Mesh,
                         instances: &[Instance],
                         kind: u32,
                         base: &wgpu::TextureView,
                         pipeline: usize| {
            let part = PartUniform {
                position_min: [0.0, 0.0, 0.0, kind as f32],
                position_extent: [1.0, 1.0, 1.0, 0.0],
                uv: [0.0, 0.0, 1.0, 1.0],
                base_color: [1.0; 4],
                material: [if kind == KIND_LEAF { 0.94 } else { 1.0 }, 0.0, 1.0, 0.0],
            };
            let storage = storage_of(instances);
            let (group, size) = make_group(&part, &storage, [base, &flat, &rough]);
            let size = size + storage.size();
            let vertices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("forest mesh"),
                contents: bytemuck::cast_slice(&mesh.vertices),
                usage: wgpu::BufferUsages::VERTEX,
            });
            let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("forest mesh indices"),
                contents: bytemuck::cast_slice(&mesh.indices),
                usage: wgpu::BufferUsages::INDEX,
            });
            let bytes = size + vertices.size() + indices.size();
            (
                Part {
                    vertices,
                    indices,
                    index_format: wgpu::IndexFormat::Uint32,
                    index_count: mesh.indices.len() as u32,
                    instances: instances.len() as u32,
                    cull: None,
                    group,
                    pipeline,
                },
                bytes,
            )
        };
        let bark = upload(&desc.limb_texture)?;
        let identity = [Instance::new(Mat4::IDENTITY, [1.0; 3])];
        if !forest.limbs.indices.is_empty() {
            let (part, size) = mesh_part(
                &forest.limbs,
                &identity,
                KIND_LIMB,
                bark.as_ref().unwrap_or(&white),
                1,
            );
            bytes += size;
            parts.push(part);
        }
        if !forest.leaves.is_empty() {
            let (part, size) = mesh_part(&leaf_mesh(), &forest.leaves, KIND_LEAF, &white, 2);
            bytes += size;
            parts.push(part);
        }
        bytes += texture_bytes.get();
        let stats = ForestStats {
            trunks: forest.trunks.len() as u32,
            props: (forest.mushrooms.len() + forest.logs.len()) as u32,
            leaves: forest.leaves.len() as u32,
            limb_triangles: forest.limbs.indices.len() as u32 / 3,
        };
        tracing::info!(?stats, "forest");
        Ok(Self {
            parts,
            culled,
            scratch: Vec::new(),
            pipelines,
            frame,
            haze,
            map_size: size_of_map(desc),
            fade: 0.0,
            bytes,
            stats,
        })
    }

    /// This frame's view fade, haze and sun.
    pub fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        camera: &CameraView,
        sun_color: Vec3,
        sun_intensity: f32,
    ) {
        self.fade = view_fade(camera);
        let planes = camera.frustum();
        for c in &mut self.culled {
            self.scratch.clear();
            for (instance, sphere) in c.all.iter().zip(&c.spheres) {
                if planes
                    .iter()
                    .all(|p| p.truncate().dot(sphere.truncate()) + p.w >= -sphere.w)
                {
                    self.scratch.push(*instance);
                }
            }
            c.visible = self.scratch.len() as u32;
            if !self.scratch.is_empty() {
                queue.write_buffer(&c.storage, 0, bytemuck::cast_slice(&self.scratch));
            }
        }
        let tint = sun_color * sun_intensity.min(1.3);
        let (haze, range) = self.haze;
        let frame = ForestFrame {
            params: [
                self.map_size,
                self.fade,
                sun_intensity.clamp(0.07, 1.0),
                0.0,
            ],
            haze: [haze[0], haze[1], haze[2], 0.0],
            range: [range[0], range[1], 0.0, 0.0],
            sun: [tint.x, tint.y, tint.z, 0.0],
        };
        queue.write_buffer(&self.frame, 0, bytemuck::bytes_of(&frame));
    }

    /// The background behind the forest: the biome's, turning towards the dimmed haze
    /// as the camera looks towards the horizon (old layer's background).
    pub fn background(&self, base: Vec3, sun_intensity: f32) -> Vec3 {
        let haze = Vec3::from_array(self.haze.0) * sun_intensity.clamp(0.07, 1.0) * 0.68;
        base.lerp(haze, self.fade)
    }

    pub fn draw<'a>(&'a self, pass: &mut wgpu::RenderPass<'a>, frame: &'a wgpu::BindGroup) -> u32 {
        let mut draws = 0;
        pass.set_bind_group(0, frame, &[]);
        for part in &self.parts {
            // Limbs and leaves only show as the camera looks towards the horizon.
            if part.pipeline > 0 && self.fade <= 0.001 {
                continue;
            }
            let instances = part.cull.map_or(part.instances, |c| self.culled[c].visible);
            if instances == 0 {
                continue;
            }
            pass.set_pipeline(&self.pipelines[part.pipeline]);
            pass.set_bind_group(1, &part.group, &[]);
            pass.set_vertex_buffer(0, part.vertices.slice(..));
            pass.set_index_buffer(part.indices.slice(..), part.index_format);
            pass.draw_indexed(0..part.index_count, 0, 0..instances);
            draws += 1;
        }
        draws
    }
}

fn size_of_map(desc: &ForestDesc) -> f32 {
    desc.map_size as f32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::{LayerKind, TerrainLayer, TextureGrid};

    fn rings() -> Vec<TrunkRing> {
        vec![
            TrunkRing {
                distance: 48.0,
                spacing: 92.0,
                height: 155.0,
            },
            TrunkRing {
                distance: 145.0,
                spacing: 110.0,
                height: 195.0,
            },
            TrunkRing {
                distance: 270.0,
                spacing: 120.0,
                height: 240.0,
            },
        ]
    }

    #[test]
    fn the_random_sequence_is_the_old_lcg() {
        let mut r = ForestRandom::new(1);
        // state = 1 · 1664525 + 1013904223
        assert_eq!(r.next(), 1_015_568_748.0 / 4_294_967_296.0);
        let mut r = ForestRandom::new(u32::MAX);
        let v = r.next();
        assert!((0.0..1.0).contains(&v));
    }

    #[test]
    fn anchors_surround_the_map_outside_its_border() {
        let size = 640;
        let a = anchors(size, 731, &rings());
        // ⌈(640 + 96) / 92⌉ = 8, ⌈930 / 110⌉ = 9, ⌈1180 / 120⌉ = 10 per side.
        assert_eq!(a.len(), 4 * (8 + 9 + 10));
        for anchor in &a {
            let ring = rings()[anchor.ring];
            let outside = (-anchor.x)
                .max(anchor.x - 640.0)
                .max(-anchor.z)
                .max(anchor.z - 640.0);
            assert!(
                outside >= f64::from(ring.distance) - 1e-9
                    && outside <= f64::from(ring.distance) + 18.0,
                "{anchor:?}"
            );
            assert!((13.0..20.0).contains(&anchor.radius));
        }
        // Deterministic.
        assert_eq!(a, anchors(size, 731, &rings()));
        assert_ne!(a, anchors(size, 732, &rings()));
    }

    #[test]
    fn curves_pass_through_their_points_and_leaves_are_closed_meshes() {
        let points = [
            Vec3::ZERO,
            Vec3::new(2.0, 5.0, 0.0),
            Vec3::new(6.0, 8.0, 1.0),
            Vec3::new(10.0, 9.0, 3.0),
        ];
        for (i, p) in points.iter().enumerate() {
            let q = catmull_rom(&points, i as f32 / 3.0);
            assert!(q.distance(*p) < 1e-4, "{i}: {q} vs {p}");
        }
        let leaf = leaf_mesh();
        assert_eq!(leaf.vertices.len(), 39);
        assert_eq!(leaf.indices.len(), 12 * 2 * 6);
        assert!(
            leaf.vertices
                .iter()
                .all(|v| (Vec3::from_array(v.normal).length() - 1.0).abs() < 1e-4)
        );
        let limb = limb_mesh(&points, 2.0);
        assert_eq!(limb.vertices.len(), 4 * 9);
        assert_eq!(limb.indices.len(), 3 * 8 * 6);
    }

    #[test]
    fn the_forest_is_placed_around_the_terrain() {
        let span = 64u32;
        let w = (span + 1) as usize;
        let terrain = TerrainDesc {
            span,
            origin: -16.0,
            heights: vec![2.0; w * w],
            water: vec![-17.0; w * w],
            layers: vec![TerrainLayer {
                kind: LayerKind::Base { tiling: 1.0 },
                albedo: String::new(),
                normal: String::new(),
                occlusion: None,
                tint: [1.0; 3],
                painted: false,
                mask: vec![255; w * w],
                cells: Vec::new(),
            }],
            grid: TextureGrid {
                start: [-16.5, -16.5],
                cell_size: 4.0,
                cells: [17, 17],
            },
            surroundings: None,
            shade: None,
        };
        let desc = ForestDesc {
            map_size: 32,
            trunk: "trunk".into(),
            mushroom: Some("mushroom".into()),
            log: Some("log".into()),
            limb_texture: None,
            rings: rings(),
            leaf_colors: vec![[0.1, 0.3, 0.1], [0.2, 0.4, 0.1]],
            leaf_spacing: 16.0,
            leaf_length: 24.0,
            seed: 731,
            canopy_height: 140.0,
        };
        assert!(desc.validate().is_ok());
        let forest = generate(
            &desc,
            &terrain,
            (Vec3::new(-20.0, -1.1, -20.0), Vec3::new(20.0, 70.0, 20.0)),
        );
        let n = anchors(32, 731, &rings()).len();
        assert_eq!(forest.trunks.len(), n);
        // Every trunk stands 6 units into the ground at the nearest map edge.
        assert!(
            forest
                .trunks
                .iter()
                .all(|m| (m.w_axis.y - (2.0 - 6.0)).abs() < 1e-5)
        );
        // Five limbs per trunk, 10 rings of 9 vertices each.
        assert_eq!(forest.limbs.vertices.len(), n * 5 * 90);
        // Mushrooms on the inner two rings at most, logs on the first.
        assert!(forest.mushrooms.len() <= n && forest.logs.len() <= n / 3 + 1);
        // The leaf grid (85 steps over map + margins) plus 22 leaves per fork.
        let grid = (712.0f64 / 16.0).ceil() as usize;
        assert_eq!(forest.leaves.len(), grid * grid * 3 + n * 10 * 22);
        assert_eq!(
            forest,
            generate(
                &desc,
                &terrain,
                (Vec3::new(-20.0, -1.1, -20.0), Vec3::new(20.0, 70.0, 20.0))
            )
        );
        let mut bad = desc.clone();
        bad.rings[0].spacing = 0.0;
        assert!(bad.validate().is_err());
    }
}
