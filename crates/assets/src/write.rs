//! Building blobs and writing packs. The cooker is the only writer; it lives here so the
//! format has one owner.

use crate::{ALIGN, BlobHeader, EntryKind, FORMAT_VERSION, Header, IndexEntry, MAGIC, Span};
use bytemuck::Pod;
use serde::Serialize;
use std::io::{self, Seek, SeekFrom, Write};

/// Collects the arrays of one blob, then seals them with their metadata.
#[derive(Debug, Default)]
pub struct BlobBuilder {
    data: Vec<u8>,
}

impl BlobBuilder {
    pub fn new() -> Self {
        Self::default()
    }

    /// Appends `items` at the next aligned offset and returns where they are.
    pub fn push<T: Pod>(&mut self, items: &[T]) -> Span {
        self.push_bytes(bytemuck::cast_slice(items))
    }

    pub fn push_bytes(&mut self, bytes: &[u8]) -> Span {
        pad(&mut self.data);
        let offset = u32::try_from(self.data.len()).expect("blob data over 4 GiB");
        self.data.extend_from_slice(bytes);
        Span {
            offset,
            len: u32::try_from(bytes.len()).expect("array over 4 GiB"),
        }
    }

    /// The finished blob: header, postcard-encoded `meta`, data region.
    pub fn finish<M: Serialize>(mut self, kind: EntryKind, meta: &M) -> Vec<u8> {
        let meta = postcard::to_stdvec(meta).expect("metadata always serialises");
        pad(&mut self.data);
        let header_len = std::mem::size_of::<BlobHeader>();
        let data_offset = (header_len + meta.len()).next_multiple_of(ALIGN);
        let header = BlobHeader {
            kind: kind as u32,
            meta_len: meta.len() as u32,
            data_offset: data_offset as u32,
            data_len: self.data.len() as u32,
        };
        let mut blob = Vec::with_capacity(data_offset + self.data.len());
        blob.extend_from_slice(bytemuck::bytes_of(&header));
        blob.extend_from_slice(&meta);
        blob.resize(data_offset, 0);
        blob.extend_from_slice(&self.data);
        blob
    }
}

fn pad(bytes: &mut Vec<u8>) {
    bytes.resize(bytes.len().next_multiple_of(ALIGN), 0);
}

/// Where a written blob lives, for entries that share it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlobLocation {
    pub offset: u64,
    pub len: u64,
}

/// Streams blobs to a file and finishes with the index. Entries may share a blob.
pub struct PackWriter<W: Write + Seek> {
    out: W,
    position: u64,
    entries: Vec<IndexEntry>,
}

impl<W: Write + Seek> PackWriter<W> {
    pub fn new(mut out: W) -> io::Result<Self> {
        out.write_all(&[0; std::mem::size_of::<Header>()])?;
        Ok(Self {
            out,
            position: std::mem::size_of::<Header>() as u64,
            entries: Vec::new(),
        })
    }

    pub fn write_blob(&mut self, blob: &[u8]) -> io::Result<BlobLocation> {
        let location = BlobLocation {
            offset: self.position,
            len: blob.len() as u64,
        };
        self.out.write_all(blob)?;
        self.position += blob.len() as u64;
        let padding = self.position.next_multiple_of(ALIGN as u64) - self.position;
        self.out.write_all(&[0; ALIGN][..padding as usize])?;
        self.position += padding;
        Ok(location)
    }

    pub fn add_entry(&mut self, name: String, kind: EntryKind, blob: BlobLocation) {
        self.entries.push(IndexEntry {
            name,
            kind,
            offset: blob.offset,
            len: blob.len,
        });
    }

    /// Writes the index (sorted by name) and the header; returns the output.
    pub fn finish(mut self, inputs_digest: [u8; 32]) -> io::Result<W> {
        self.entries.sort_by(|a, b| a.name.cmp(&b.name));
        if let Some(pair) = self.entries.windows(2).find(|p| p[0].name == p[1].name) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("duplicate pack entry {}", pair[0].name),
            ));
        }
        let index = postcard::to_stdvec(&self.entries).expect("index always serialises");
        let header = Header {
            magic: MAGIC,
            version: FORMAT_VERSION,
            reserved: 0,
            index_offset: self.position,
            index_len: index.len() as u64,
            inputs_digest,
        };
        self.out.write_all(&index)?;
        self.out.seek(SeekFrom::Start(0))?;
        self.out.write_all(bytemuck::bytes_of(&header))?;
        self.out.flush()?;
        Ok(self.out)
    }
}
