//! Every type stored in a pack. Changing any of them changes the format: bump
//! [`FORMAT_VERSION`].

use bytemuck::{Pod, Zeroable};
use serde::{Deserialize, Serialize};

/// First bytes of every pack.
pub const MAGIC: [u8; 8] = *b"CANOPYPK";
/// Version of the pack layout and of every metadata type.
pub const FORMAT_VERSION: u32 = 3;
/// Alignment of blobs and of every array inside one.
pub const ALIGN: usize = 16;
/// Rate at which clips are baked, in frames per second.
pub const CLIP_FRAME_RATE: f32 = 30.0;

/// The fixed header at the start of a pack.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct Header {
    pub magic: [u8; 8],
    pub version: u32,
    pub reserved: u32,
    pub index_offset: u64,
    pub index_len: u64,
    /// Identifies the cooked inputs: equal digests mean an identical pack.
    pub inputs_digest: [u8; 32],
}

/// The fixed start of every blob.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Pod, Zeroable)]
pub struct BlobHeader {
    /// [`EntryKind`] as a number, checked against the index.
    pub kind: u32,
    pub meta_len: u32,
    /// Offset of the data region from the start of the blob.
    pub data_offset: u32,
    pub data_len: u32,
}

/// A named entry of the pack index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IndexEntry {
    /// The source path relative to the data folder (`assets/library/<id>/geometry.glb`),
    /// with a `#suffix` for parts cooked out of one source, or `@name` for tables.
    pub name: String,
    pub kind: EntryKind,
    /// Absolute offset of the blob in the pack.
    pub offset: u64,
    pub len: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[repr(u32)]
pub enum EntryKind {
    Model = 1,
    Texture = 2,
    Sound = 3,
    Data = 4,
    Scenery = 5,
    /// The runtime effect table ([`crate::EffectTable`]).
    Effects = 6,
}

impl EntryKind {
    pub fn from_u32(value: u32) -> Option<Self> {
        Some(match value {
            1 => Self::Model,
            2 => Self::Texture,
            3 => Self::Sound,
            4 => Self::Data,
            5 => Self::Scenery,
            6 => Self::Effects,
            _ => return None,
        })
    }
}

/// A byte range of a blob's data region.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Span {
    pub offset: u32,
    pub len: u32,
}

impl Span {
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// --- Models ------------------------------------------------------------------------

/// The main vertex stream, 16 bytes.
///
/// - `position`: x, y, z as unorm16 inside the model's quantisation box
///   ([`ModelMeta::position_min`] + q / 65535 · [`ModelMeta::position_extent`]); w is the
///   wind bend weight (unorm16, 0 at the base, 1 at the top).
/// - `normal`: octahedral, snorm16 ×2.
/// - `uv`: unorm16 ×2 inside [`ModelMeta::uv_min`] + q / 65535 · [`ModelMeta::uv_extent`].
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Pod, Zeroable)]
pub struct Vertex {
    pub position: [u16; 4],
    pub normal: [i16; 2],
    pub uv: [u16; 2],
}

/// Skin influences: four joint indices into [`Skeleton::joints`] and unorm8 weights that
/// sum to exactly 255.
#[repr(C)]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Pod, Zeroable)]
pub struct SkinVertex {
    pub joints: [u8; 4],
    pub weights: [u8; 4],
}

/// A 3×4 affine matrix, row-major: rows are (m00 m01 m02 tx), (m10 …), (m20 …).
pub type Matrix3x4 = [f32; 12];

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndexFormat {
    U16,
    U32,
}

impl IndexFormat {
    pub fn size(self) -> usize {
        match self {
            Self::U16 => 2,
            Self::U32 => 4,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Aabb {
    pub min: [f32; 3],
    pub max: [f32; 3],
}

/// A cooked model: one glTF scene with the authored transform, pivot and soil offset
/// folded into its vertices, so placing it needs only the instance transform.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelMeta {
    /// The published asset record the geometry belongs to.
    pub asset: String,
    /// Bind-pose bounds of every vertex.
    pub rest_bounds: Aabb,
    /// Conservative bounds over every animation frame and wind sway: use for culling.
    pub bounds: Aabb,
    pub position_min: [f32; 3],
    pub position_extent: [f32; 3],
    pub uv_min: [f32; 2],
    pub uv_extent: [f32; 2],
    pub vertex_count: u32,
    /// [`Vertex`] × `vertex_count`.
    pub vertices: Span,
    /// Vertex colour (`COLOR_0`) as unorm8 RGBA, when present; what it means is in
    /// [`Material::vertex_color`].
    pub colors: Option<Span>,
    /// Per-leaf pivot data (`_LEAF`) as unorm16 RGBA: offset = (rgb · 2 − 1) · 4, a > 0
    /// marks leaf vertices.
    pub leaves: Option<Span>,
    /// [`SkinVertex`] × `vertex_count` for animated models.
    pub skin: Option<Span>,
    pub index_format: IndexFormat,
    /// Indices local to each section (add [`Section::base_vertex`]).
    pub indices: Span,
    /// One draw per section, in material order.
    pub sections: Vec<Section>,
    pub depth: DepthStream,
    /// Coarser index lists over the same vertices, finest first (level 0, the full mesh,
    /// is `sections` and `depth`).
    pub lods: Vec<Lod>,
    pub materials: Vec<Material>,
    pub skeleton: Option<Skeleton>,
    pub clips: Vec<Clip>,
    /// Gameplay animation states (semantic → clip).
    pub states: Vec<AnimationState>,
    pub sockets: Vec<Socket>,
    pub wind: Option<Wind>,
    /// Height of the foliage crown in model metres, for crown normals.
    pub plant_height: Option<f32>,
    pub ground: GroundContact,
    /// Metres added to y to put the soil line at the origin (already folded in).
    pub ground_offset: f32,
    /// Giant overhanging foliage whose crown dissolves around units behind it.
    pub canopy: bool,
    pub prototype: Prototype,
}

/// A draw range of the main streams.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Section {
    pub material: u32,
    pub first_index: u32,
    pub index_count: u32,
    pub base_vertex: u32,
    pub vertex_count: u32,
}

/// A level of detail: index ranges over the full mesh's vertices that deviate from its
/// rest pose by at most `error`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Lod {
    /// Largest deviation from the full mesh, model units.
    pub error: f32,
    /// `[first_index, index_count]` in the main indices per section (same base vertex).
    pub sections: Vec<[u32; 2]>,
    /// `[first_index, index_count]` in the depth stream's indices.
    pub depth: [u32; 2],
}

/// Positions (and skin) of every shadow-casting opaque section, welded across UV and
/// normal seams, for depth-only passes in one draw. Sections that are alpha-tested are not
/// in it: draw them from the main streams with their material ([`DepthStream::masked`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DepthStream {
    pub vertex_count: u32,
    /// `[u16; 4]` per vertex, quantised like [`Vertex::position`].
    pub positions: Span,
    /// [`SkinVertex`] per vertex for animated models.
    pub skin: Option<Span>,
    pub index_format: IndexFormat,
    pub index_count: u32,
    pub indices: Span,
    /// Sections that cast shadows but are alpha-tested.
    pub masked: Vec<u32>,
}

/// How a surface is lit; the renderer's material models (renderer §4.2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MaterialModel {
    Unit,
    Building,
    Foliage,
    Prop,
    /// Unlit (`KHR_materials_unlit`).
    Effect,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AlphaMode {
    Opaque,
    /// Alpha-tested at [`Material::alpha_cutoff`].
    Mask,
    Blend,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum VertexColor {
    /// No vertex colour.
    None,
    /// Multiplies the albedo.
    Tint,
    /// `r` is ambient occlusion on indirect light.
    Occlusion,
}

/// Every material parameter as a number or flag; no names are interpreted at runtime.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Material {
    pub name: String,
    pub model: MaterialModel,
    /// Linear RGBA factor.
    pub base_color: [f32; 4],
    pub emissive: [f32; 3],
    pub roughness: f32,
    pub metalness: f32,
    /// `KHR_materials_specular`: factor and linear colour.
    pub specular: f32,
    pub specular_color: [f32; 3],
    pub alpha: AlphaMode,
    pub alpha_cutoff: f32,
    pub double_sided: bool,
    pub casts_shadow: bool,
    pub vertex_color: VertexColor,
    /// No specular and no ground macro (hand-painted texture).
    pub painted: bool,
    /// Leaves flutter with the `_LEAF` stream and back faces flip normals.
    pub foliage: bool,
    /// Wrap-lighting factor for thin foliage.
    pub backside_lighting: f32,
    /// How much crown normals replace mesh normals on foliage, 0–1.
    pub crown_normal: f32,
    pub snow_surface: bool,
    pub team_color: Option<TeamColor>,
    pub normal_scale: f32,
    pub occlusion_strength: f32,
    /// Emission that lives only while its model works (D65).
    pub active: Option<ActiveGlow>,
    pub textures: MaterialTextures,
}

/// A material whose emission is off while its model rests and comes alive while it
/// works: scaled by the instance's activity, pulsing and flowing along the texture.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ActiveGlow {
    /// Pulses per second.
    pub pulse: f32,
    /// Texture lengths per second the emission streams along V.
    pub flow: f32,
}

/// Player colour on a material: `albedo' = mix(albedo, sqrt(max(albedo.rgb) / source_max) ·
/// player colour, mask)`. Without a mask texture the whole material takes the colour.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TeamColor {
    pub source_max: f32,
    /// Linear colour used when no player colour applies.
    pub default: [f32; 3],
}

/// Pack entry names of the textures a material samples. All use `TEXCOORD_0`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialTextures {
    pub base_color: Option<String>,
    /// R8, lossless: the player-colour mask.
    pub team_mask: Option<String>,
    pub normal: Option<String>,
    /// glTF layout: g = roughness, b = metalness.
    pub metallic_roughness: Option<String>,
    pub occlusion: Option<String>,
    pub emissive: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Joint {
    pub name: String,
    pub parent: Option<u32>,
}

/// The joint palette of an animated model. Rigidly animated nodes are joints too, so
/// every animated model skins the same way.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Skeleton {
    pub joints: Vec<Joint>,
}

/// A clip baked at [`CLIP_FRAME_RATE`]: frame `f` is time `min(f / rate, duration)`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Clip {
    pub name: String,
    pub duration: f32,
    pub frame_count: u32,
    /// Skinning matrices in model space ([`Matrix3x4`]), frame-major:
    /// `frame · joints + joint`. Apply to the stored (bind-pose) vertices.
    pub matrices: Span,
    /// Socket transforms in model space, frame-major: `frame · sockets + socket`.
    pub sockets: Span,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationState {
    pub semantic: String,
    pub clip: u32,
    pub looping: bool,
    /// Movement speed at which the clip plays at rate 1, when authored.
    pub nominal_speed: Option<f32>,
    pub events: Vec<AnimationEvent>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AnimationEvent {
    pub name: String,
    /// Fraction of the clip, 0–1.
    pub time: f32,
}

/// A named attachment point; its transform includes the authored offset.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Socket {
    pub name: String,
    pub node: String,
    /// Transform in model space at rest.
    pub rest: Matrix3x4,
}

/// Sway: `bend = position.w` (see [`Vertex`]);
/// `phase = t · speed · 2π + anchor.x · 0.37 + anchor.z · 0.29`;
/// `offset = (sin(phase), 0, 0.55 · cos(0.73 · phase)) · bend · amplitude`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Wind {
    /// Metres at the top of the model.
    pub amplitude: f32,
    /// Cycles per second.
    pub speed: f32,
    /// Authored capability (`capabilities.wind`), carried for the renderer's tuning.
    pub mode: Option<WindMode>,
    pub strength: f32,
    pub stiffness: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindMode {
    Tree,
    Foliage,
    Grass,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroundContact {
    /// The pivot sits on the terrain.
    Pivot,
    /// The lowest vertex sits on the terrain (folded into the vertices).
    Terrain,
    /// Floats at the water level.
    Water,
    /// Placed freely (effects, projectiles).
    Free,
}

/// Placement data of a model as a prototype.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Prototype {
    /// Authored body: radius, height and formation spacing in metres.
    pub dimensions: Option<[f32; 3]>,
    pub blocker: Option<Blocker>,
    pub vegetation_clearance: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlockerShape {
    Box,
    Ellipse,
}

/// Ground-plane footprint in model metres.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Blocker {
    pub shape: BlockerShape,
    pub width: f32,
    pub depth: f32,
    pub x: f32,
    pub z: f32,
    /// Radians.
    pub yaw: f32,
}

// --- Textures ----------------------------------------------------------------------

/// Pixel formats; each maps to the WebGPU format of the same name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureFormat {
    Bc7RgbaUnorm,
    Bc7RgbaUnormSrgb,
    R8Unorm,
    Rgba8Unorm,
    Rgba8UnormSrgb,
}

impl TextureFormat {
    pub fn is_compressed(self) -> bool {
        matches!(self, Self::Bc7RgbaUnorm | Self::Bc7RgbaUnormSrgb)
    }

    pub fn is_srgb(self) -> bool {
        matches!(self, Self::Bc7RgbaUnormSrgb | Self::Rgba8UnormSrgb)
    }

    /// The uncompressed format the CPU fallback decodes to on devices without BC.
    pub fn uncompressed(self) -> Self {
        match self {
            Self::Bc7RgbaUnorm => Self::Rgba8Unorm,
            Self::Bc7RgbaUnormSrgb => Self::Rgba8UnormSrgb,
            other => other,
        }
    }

    /// Bytes of one mip level of `width` × `height` (× `depth`) texels.
    pub fn level_size(self, width: u32, height: u32, depth: u32) -> usize {
        let (w, h, d) = (width as usize, height as usize, depth as usize);
        match self {
            Self::Bc7RgbaUnorm | Self::Bc7RgbaUnormSrgb => w.div_ceil(4) * h.div_ceil(4) * 16 * d,
            Self::R8Unorm => w * h * d,
            Self::Rgba8Unorm | Self::Rgba8UnormSrgb => w * h * d * 4,
        }
    }

    /// Bytes per row of blocks (or texels) for uploads.
    pub fn bytes_per_row(self, width: u32) -> u32 {
        match self {
            Self::Bc7RgbaUnorm | Self::Bc7RgbaUnormSrgb => width.div_ceil(4) * 16,
            Self::R8Unorm => width,
            Self::Rgba8Unorm | Self::Rgba8UnormSrgb => width * 4,
        }
    }

    /// Rows of blocks (or texels) of a level.
    pub fn rows(self, height: u32) -> u32 {
        if self.is_compressed() {
            height.div_ceil(4)
        } else {
            height
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureDimension {
    D2,
    D3,
}

/// What the alpha channel holds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum TextureAlpha {
    /// Always 1.
    Opaque,
    /// Alpha-tested at the cutoff; mips preserve coverage.
    Cutout(f32),
    /// Straight (not premultiplied) translucency.
    Blend,
    /// An unrelated data channel (roughness, height…).
    Data,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TextureMeta {
    pub format: TextureFormat,
    pub dimension: TextureDimension,
    pub width: u32,
    pub height: u32,
    /// 1 for 2D textures.
    pub depth: u32,
    /// The source image size; differs from `width` × `height` when the cook resampled
    /// to a multiple of the block size. Lay out UI images with this.
    pub source_width: u32,
    pub source_height: u32,
    pub alpha: TextureAlpha,
    /// Mip levels from the largest; level `i` is `max(1, size >> i)` texels.
    pub mips: Vec<Span>,
}

// --- Sounds and data ---------------------------------------------------------------

/// Signed 16-bit PCM, interleaved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SoundMeta {
    pub sample_rate: u32,
    pub channels: u16,
    pub frames: u32,
    pub samples: Span,
}

/// Opaque bytes the cooker passes through.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DataMeta {
    pub bytes: Span,
}

// --- Scenery -----------------------------------------------------------------------

/// `@scenery`: every placeable scenery prototype with its model and per-prototype data.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SceneryTable {
    pub prototypes: Vec<SceneryPrototype>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SceneryPrototype {
    pub id: String,
    /// Model entry name.
    pub model: String,
    pub light: Option<PointLight>,
    pub blockers: Vec<Blocker>,
    pub hidden: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PointLight {
    pub position: [f32; 3],
    /// Linear RGB.
    pub color: [f32; 3],
    pub intensity: f32,
    pub range: f32,
}

/// Entry name of the scenery table.
pub const SCENERY_ENTRY: &str = "@scenery";
