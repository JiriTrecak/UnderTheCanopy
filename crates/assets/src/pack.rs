//! Reading packs and typed, zero-copy views of their entries.

use crate::{
    AnimationState, BlobHeader, DataMeta, EntryKind, Error, FORMAT_VERSION, Header, IndexEntry,
    IndexFormat, MAGIC, Matrix3x4, ModelMeta, SceneryTable, SkinVertex, SoundMeta, Span,
    TextureMeta, Vertex,
};
use bytemuck::Pod;
use serde::de::DeserializeOwned;
use std::{
    collections::HashMap,
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

/// Bytes with 16-byte alignment, so arrays inside can be cast in place.
#[derive(Debug, Clone, Default)]
struct AlignedBytes {
    words: Vec<u128>,
    len: usize,
}

impl AlignedBytes {
    fn read(reader: &mut impl Read, len: usize) -> std::io::Result<Self> {
        let mut words = vec![0u128; len.div_ceil(16)];
        reader.read_exact(&mut bytemuck::cast_slice_mut::<u128, u8>(&mut words)[..len])?;
        Ok(Self { words, len })
    }

    fn bytes(&self) -> &[u8] {
        &bytemuck::cast_slice(&self.words)[..self.len]
    }
}

enum Source {
    Memory(AlignedBytes),
    File(PathBuf),
}

/// An opened pack: the index, and either every byte ([`Pack::load`]) or the file to read
/// entries from ([`Pack::open`]).
pub struct Pack {
    source: Source,
    header: Header,
    entries: Vec<IndexEntry>,
    by_name: HashMap<String, usize>,
}

impl Pack {
    /// Reads the whole pack with one read.
    pub fn load(path: &Path) -> Result<Self, Error> {
        let _span = tracing::info_span!("load_pack").entered();
        let io = |source| Error::Io {
            path: path.to_owned(),
            source,
        };
        let mut file = File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len() as usize;
        let bytes = AlignedBytes::read(&mut file, len).map_err(io)?;
        let header = read_header(bytes.bytes())?;
        let index = slice(
            bytes.bytes(),
            header.index_offset,
            header.index_len,
            "the index",
        )?;
        let entries = parse_index(index, bytes.len as u64)?;
        Ok(Self::new(Source::Memory(bytes), header, entries))
    }

    /// Reads only the header and index; entries are read from the file when asked for.
    pub fn open(path: &Path) -> Result<Self, Error> {
        let io = |source| Error::Io {
            path: path.to_owned(),
            source,
        };
        let mut file = File::open(path).map_err(io)?;
        let len = file.metadata().map_err(io)?.len();
        let mut head = [0u8; std::mem::size_of::<Header>()];
        file.read_exact(&mut head).map_err(io)?;
        let header = read_header(&head)?;
        if header.index_offset.saturating_add(header.index_len) > len {
            return Err(Error::corrupt("index past the end of the file"));
        }
        file.seek(SeekFrom::Start(header.index_offset))
            .map_err(io)?;
        let mut index = vec![0; header.index_len as usize];
        file.read_exact(&mut index).map_err(io)?;
        let entries = parse_index(&index, len)?;
        Ok(Self::new(Source::File(path.to_owned()), header, entries))
    }

    /// The header of a pack file without reading anything else, or `None` when the file
    /// is missing or not a pack of this format.
    pub fn peek_header(path: &Path) -> Option<Header> {
        let mut head = [0u8; std::mem::size_of::<Header>()];
        File::open(path).ok()?.read_exact(&mut head).ok()?;
        read_header(&head).ok()
    }

    fn new(source: Source, header: Header, entries: Vec<IndexEntry>) -> Self {
        let by_name = entries
            .iter()
            .enumerate()
            .map(|(i, e)| (e.name.clone(), i))
            .collect();
        Self {
            source,
            header,
            entries,
            by_name,
        }
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    /// Every entry, sorted by name.
    pub fn entries(&self) -> &[IndexEntry] {
        &self.entries
    }

    pub fn entry(&self, name: &str) -> Option<&IndexEntry> {
        self.by_name.get(name).map(|&i| &self.entries[i])
    }

    /// The blob of the entry named `name`.
    pub fn get(&self, name: &str) -> Result<Blob<'_>, Error> {
        let entry = self
            .entry(name)
            .ok_or_else(|| Error::Missing(name.to_owned()))?;
        self.blob(entry)
    }

    pub fn blob(&self, entry: &IndexEntry) -> Result<Blob<'_>, Error> {
        let bytes = match &self.source {
            Source::Memory(bytes) => {
                BlobBytes::Borrowed(slice(bytes.bytes(), entry.offset, entry.len, &entry.name)?)
            }
            Source::File(path) => {
                let io = |source| Error::Io {
                    path: path.clone(),
                    source,
                };
                let mut file = File::open(path).map_err(io)?;
                file.seek(SeekFrom::Start(entry.offset)).map_err(io)?;
                BlobBytes::Owned(AlignedBytes::read(&mut file, entry.len as usize).map_err(io)?)
            }
        };
        Blob::new(&entry.name, entry.kind, bytes)
    }

    pub fn model(&self, name: &str) -> Result<Model<'_>, Error> {
        self.get(name)?.into_model()
    }

    pub fn texture(&self, name: &str) -> Result<Texture<'_>, Error> {
        self.get(name)?.into_texture()
    }

    pub fn sound(&self, name: &str) -> Result<Sound<'_>, Error> {
        self.get(name)?.into_sound()
    }

    pub fn scenery(&self) -> Result<SceneryTable, Error> {
        self.get(crate::SCENERY_ENTRY)?.scenery()
    }

    /// The runtime effect table (`@effects`).
    pub fn effects(&self) -> Result<crate::EffectTable, Error> {
        self.get(crate::EFFECTS_ENTRY)?.effects()
    }
}

fn read_header(bytes: &[u8]) -> Result<Header, Error> {
    let head = bytes
        .get(..std::mem::size_of::<Header>())
        .ok_or_else(|| Error::corrupt("shorter than the header"))?;
    let header: Header = bytemuck::pod_read_unaligned(head);
    if header.magic != MAGIC {
        return Err(Error::Magic);
    }
    if header.version != FORMAT_VERSION {
        return Err(Error::Version {
            found: header.version,
            expected: FORMAT_VERSION,
        });
    }
    Ok(header)
}

fn parse_index(bytes: &[u8], file_len: u64) -> Result<Vec<IndexEntry>, Error> {
    let entries: Vec<IndexEntry> =
        postcard::from_bytes(bytes).map_err(|e| Error::corrupt(format!("index: {e}")))?;
    for entry in &entries {
        if entry.offset.saturating_add(entry.len) > file_len
            || !entry.offset.is_multiple_of(crate::ALIGN as u64)
        {
            return Err(Error::corrupt(format!("entry {} out of range", entry.name)));
        }
    }
    Ok(entries)
}

fn slice<'a>(bytes: &'a [u8], offset: u64, len: u64, what: &str) -> Result<&'a [u8], Error> {
    let start = usize::try_from(offset).map_err(|_| Error::corrupt(what.to_owned()))?;
    let end = start.saturating_add(len as usize);
    bytes
        .get(start..end)
        .ok_or_else(|| Error::corrupt(format!("{what} out of range")))
}

enum BlobBytes<'a> {
    Borrowed(&'a [u8]),
    Owned(AlignedBytes),
}

/// One entry's bytes: blob header, metadata and data region.
pub struct Blob<'a> {
    name: String,
    kind: EntryKind,
    bytes: BlobBytes<'a>,
    header: BlobHeader,
}

impl<'a> Blob<'a> {
    fn new(name: &str, kind: EntryKind, bytes: BlobBytes<'a>) -> Result<Self, Error> {
        let raw = match &bytes {
            BlobBytes::Borrowed(b) => *b,
            BlobBytes::Owned(b) => b.bytes(),
        };
        let header: BlobHeader = bytemuck::pod_read_unaligned(
            raw.get(..std::mem::size_of::<BlobHeader>())
                .ok_or_else(|| Error::corrupt(format!("{name}: no blob header")))?,
        );
        let meta_end = std::mem::size_of::<BlobHeader>() + header.meta_len as usize;
        let data_end = header.data_offset as usize + header.data_len as usize;
        if EntryKind::from_u32(header.kind) != Some(kind)
            || meta_end > header.data_offset as usize
            || data_end > raw.len()
            || !(header.data_offset as usize).is_multiple_of(crate::ALIGN)
        {
            return Err(Error::corrupt(format!("{name}: bad blob header")));
        }
        Ok(Self {
            name: name.to_owned(),
            kind,
            bytes,
            header,
        })
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn kind(&self) -> EntryKind {
        self.kind
    }

    fn raw(&self) -> &[u8] {
        match &self.bytes {
            BlobBytes::Borrowed(b) => b,
            BlobBytes::Owned(b) => b.bytes(),
        }
    }

    /// The postcard-encoded metadata.
    pub fn meta<M: DeserializeOwned>(&self) -> Result<M, Error> {
        let start = std::mem::size_of::<BlobHeader>();
        let end = start + self.header.meta_len as usize;
        postcard::from_bytes(&self.raw()[start..end])
            .map_err(|e| Error::corrupt(format!("{}: metadata: {e}", self.name)))
    }

    /// The data region.
    pub fn data(&self) -> &[u8] {
        let start = self.header.data_offset as usize;
        &self.raw()[start..start + self.header.data_len as usize]
    }

    fn expect(&self, kind: EntryKind) -> Result<(), Error> {
        if self.kind == kind {
            Ok(())
        } else {
            Err(Error::Kind {
                name: self.name.clone(),
                found: self.kind,
                expected: kind,
            })
        }
    }

    pub fn into_model(self) -> Result<Model<'a>, Error> {
        self.expect(EntryKind::Model)?;
        let meta: ModelMeta = self.meta()?;
        let model = Model { meta, blob: self };
        model.validate()?;
        Ok(model)
    }

    pub fn into_texture(self) -> Result<Texture<'a>, Error> {
        self.expect(EntryKind::Texture)?;
        let meta: TextureMeta = self.meta()?;
        let mut size = (meta.width, meta.height, meta.depth);
        for (level, span) in meta.mips.iter().enumerate() {
            check::<u8>(self.data(), *span, &self.name)?;
            if span.len as usize != meta.format.level_size(size.0, size.1, size.2) {
                return Err(Error::corrupt(format!("{}: mip {level} size", self.name)));
            }
            size = next_level(size, meta.dimension);
        }
        Ok(Texture { meta, blob: self })
    }

    pub fn into_sound(self) -> Result<Sound<'a>, Error> {
        self.expect(EntryKind::Sound)?;
        let meta: SoundMeta = self.meta()?;
        check::<i16>(self.data(), meta.samples, &self.name)?;
        Ok(Sound { meta, blob: self })
    }

    /// The bytes of a [`EntryKind::Data`] entry.
    pub fn bytes(&self) -> Result<&[u8], Error> {
        self.expect(EntryKind::Data)?;
        let meta: DataMeta = self.meta()?;
        check::<u8>(self.data(), meta.bytes, &self.name)?;
        Ok(range(self.data(), meta.bytes))
    }

    pub fn scenery(&self) -> Result<SceneryTable, Error> {
        self.expect(EntryKind::Scenery)?;
        self.meta()
    }

    pub fn effects(&self) -> Result<crate::EffectTable, Error> {
        self.expect(EntryKind::Effects)?;
        self.meta()
    }
}

fn range(data: &[u8], span: Span) -> &[u8] {
    &data[span.offset as usize..(span.offset + span.len) as usize]
}

fn check<T: Pod>(data: &[u8], span: Span, name: &str) -> Result<(), Error> {
    let end = span.offset as usize + span.len as usize;
    let bytes = data
        .get(span.offset as usize..end)
        .ok_or_else(|| Error::corrupt(format!("{name}: array out of range")))?;
    bytemuck::try_cast_slice::<u8, T>(bytes)
        .map(|_| ())
        .map_err(|e| Error::corrupt(format!("{name}: array {e}")))
}

/// Casts a validated span.
fn cast<T: Pod>(data: &[u8], span: Span) -> &[T] {
    bytemuck::cast_slice(range(data, span))
}

pub(crate) fn next_level(
    (w, h, d): (u32, u32, u32),
    dimension: crate::TextureDimension,
) -> (u32, u32, u32) {
    let d = match dimension {
        crate::TextureDimension::D2 => d,
        crate::TextureDimension::D3 => (d / 2).max(1),
    };
    ((w / 2).max(1), (h / 2).max(1), d)
}

/// Index data of a model or depth stream.
#[derive(Debug, Clone, Copy)]
pub enum Indices<'a> {
    U16(&'a [u16]),
    U32(&'a [u32]),
}

impl Indices<'_> {
    pub fn len(&self) -> usize {
        match self {
            Self::U16(i) => i.len(),
            Self::U32(i) => i.len(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    pub fn get(&self, i: usize) -> u32 {
        match self {
            Self::U16(v) => u32::from(v[i]),
            Self::U32(v) => v[i],
        }
    }

    pub fn bytes(&self) -> &[u8] {
        match self {
            Self::U16(v) => bytemuck::cast_slice(v),
            Self::U32(v) => bytemuck::cast_slice(v),
        }
    }
}

/// A model entry with typed views of its streams.
pub struct Model<'a> {
    pub meta: ModelMeta,
    blob: Blob<'a>,
}

impl Model<'_> {
    fn validate(&self) -> Result<(), Error> {
        let (m, data, name) = (&self.meta, self.blob.data(), self.blob.name());
        let n = m.vertex_count as usize;
        let sized = |span: Span, stride: usize, count: usize, what: &str| {
            if span.len as usize == stride * count {
                Ok(())
            } else {
                Err(Error::corrupt(format!("{name}: {what} length")))
            }
        };
        check::<Vertex>(data, m.vertices, name)?;
        sized(m.vertices, 16, n, "vertices")?;
        for span in [m.colors, m.skin].into_iter().flatten() {
            check::<[u8; 4]>(data, span, name)?;
        }
        if let Some(span) = m.colors {
            sized(span, 4, n, "colours")?;
        }
        if let Some(span) = m.skin {
            sized(span, 8, n, "skin")?;
        }
        if let Some(span) = m.leaves {
            check::<[u16; 4]>(data, span, name)?;
            sized(span, 8, n, "leaves")?;
        }
        check_indices(data, m.index_format, m.indices, name)?;
        let index_count = m.indices.len as usize / m.index_format.size();
        for s in &m.sections {
            if (s.first_index + s.index_count) as usize > index_count
                || (s.base_vertex + s.vertex_count) as usize > n
                || s.material as usize >= m.materials.len()
            {
                return Err(Error::corrupt(format!("{name}: section out of range")));
            }
        }
        let d = &m.depth;
        check::<[u16; 4]>(data, d.positions, name)?;
        sized(d.positions, 8, d.vertex_count as usize, "depth positions")?;
        if let Some(span) = d.skin {
            check::<SkinVertex>(data, span, name)?;
            sized(span, 8, d.vertex_count as usize, "depth skin")?;
        }
        check_indices(data, d.index_format, d.indices, name)?;
        let depth_count = d.indices.len as usize / d.index_format.size();
        if d.index_count as usize > depth_count {
            return Err(Error::corrupt(format!("{name}: depth indices length")));
        }
        let within = |[first, count]: [u32; 2], total: usize| (first + count) as usize <= total;
        for lod in &m.lods {
            if lod.sections.len() != m.sections.len()
                || !lod.sections.iter().all(|&r| within(r, index_count))
                || !within(lod.depth, depth_count)
            {
                return Err(Error::corrupt(format!(
                    "{name}: level of detail out of range"
                )));
            }
        }
        let joints = m.skeleton.as_ref().map_or(0, |s| s.joints.len());
        for clip in &m.clips {
            let frames = clip.frame_count as usize;
            check::<Matrix3x4>(data, clip.matrices, name)?;
            sized(clip.matrices, 48, frames * joints, "clip matrices")?;
            check::<Matrix3x4>(data, clip.sockets, name)?;
            sized(clip.sockets, 48, frames * m.sockets.len(), "clip sockets")?;
        }
        if m.states.iter().any(|s| s.clip as usize >= m.clips.len()) {
            return Err(Error::corrupt(format!("{name}: state without clip")));
        }
        Ok(())
    }

    fn data(&self) -> &[u8] {
        self.blob.data()
    }

    pub fn name(&self) -> &str {
        self.blob.name()
    }

    pub fn vertices(&self) -> &[Vertex] {
        cast(self.data(), self.meta.vertices)
    }

    pub fn colors(&self) -> Option<&[[u8; 4]]> {
        self.meta.colors.map(|s| cast(self.data(), s))
    }

    pub fn leaves(&self) -> Option<&[[u16; 4]]> {
        self.meta.leaves.map(|s| cast(self.data(), s))
    }

    pub fn skin(&self) -> Option<&[SkinVertex]> {
        self.meta.skin.map(|s| cast(self.data(), s))
    }

    pub fn indices(&self) -> Indices<'_> {
        indices(self.data(), self.meta.index_format, self.meta.indices)
    }

    pub fn depth_positions(&self) -> &[[u16; 4]] {
        cast(self.data(), self.meta.depth.positions)
    }

    pub fn depth_skin(&self) -> Option<&[SkinVertex]> {
        self.meta.depth.skin.map(|s| cast(self.data(), s))
    }

    pub fn depth_indices(&self) -> Indices<'_> {
        indices(
            self.data(),
            self.meta.depth.index_format,
            self.meta.depth.indices,
        )
    }

    /// Skinning matrices of clip `clip`, frame-major.
    pub fn clip_matrices(&self, clip: usize) -> &[Matrix3x4] {
        cast(self.data(), self.meta.clips[clip].matrices)
    }

    /// Socket transforms of clip `clip`, frame-major.
    pub fn clip_sockets(&self, clip: usize) -> &[Matrix3x4] {
        cast(self.data(), self.meta.clips[clip].sockets)
    }

    pub fn state(&self, semantic: &str) -> Option<&AnimationState> {
        self.meta.states.iter().find(|s| s.semantic == semantic)
    }

    /// Dequantised model-space position of a stored position.
    pub fn position(&self, q: [u16; 4]) -> [f32; 3] {
        let m = &self.meta;
        std::array::from_fn(|i| {
            m.position_min[i] + f32::from(q[i]) / 65535.0 * m.position_extent[i]
        })
    }

    /// Dequantised texture coordinate of a stored one.
    pub fn uv(&self, q: [u16; 2]) -> [f32; 2] {
        let m = &self.meta;
        std::array::from_fn(|i| m.uv_min[i] + f32::from(q[i]) / 65535.0 * m.uv_extent[i])
    }
}

fn check_indices(data: &[u8], format: IndexFormat, span: Span, name: &str) -> Result<(), Error> {
    match format {
        IndexFormat::U16 => check::<u16>(data, span, name),
        IndexFormat::U32 => check::<u32>(data, span, name),
    }
}

fn indices(data: &[u8], format: IndexFormat, span: Span) -> Indices<'_> {
    match format {
        IndexFormat::U16 => Indices::U16(cast(data, span)),
        IndexFormat::U32 => Indices::U32(cast(data, span)),
    }
}

/// A texture entry: its mip levels, ready to upload.
pub struct Texture<'a> {
    pub meta: TextureMeta,
    blob: Blob<'a>,
}

impl Texture<'_> {
    pub fn name(&self) -> &str {
        self.blob.name()
    }

    /// The bytes of mip `level`, laid out as the format's rows of blocks or texels.
    pub fn level(&self, level: usize) -> &[u8] {
        range(self.blob.data(), self.meta.mips[level])
    }

    /// Width, height and depth of mip `level`.
    pub fn level_extent(&self, level: usize) -> (u32, u32, u32) {
        let mut size = (self.meta.width, self.meta.height, self.meta.depth);
        for _ in 0..level {
            size = next_level(size, self.meta.dimension);
        }
        size
    }

    /// Mip `level` as uncompressed RGBA8 (or the stored texels when uncompressed): the
    /// fallback for devices without BC texture compression.
    pub fn decode_level(&self, level: usize) -> Vec<u8> {
        let (w, h, d) = self.level_extent(level);
        crate::texture::decode(self.meta.format, w, h, d, self.level(level))
    }
}

/// A sound entry.
pub struct Sound<'a> {
    pub meta: SoundMeta,
    blob: Blob<'a>,
}

impl Sound<'_> {
    pub fn samples(&self) -> &[i16] {
        cast(self.blob.data(), self.meta.samples)
    }
}
