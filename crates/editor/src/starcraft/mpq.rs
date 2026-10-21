//! A reader for MPQ archives, the container of Warcraft III and StarCraft II maps
//! (formats 0 to 3: the header, the encrypted hash and block tables, sectored files
//! compressed with zlib, bzip2 or PKWARE implode). Only what map files use; everything is
//! bounds-checked, as maps come from anywhere.

use std::io::Read;

/// The files an archive may hold in all, decoded.
const MAX_FILE: usize = 64 << 20;

/// The decryption table every MPQ uses.
fn crypt_table() -> &'static [u32; 0x500] {
    static TABLE: std::sync::OnceLock<[u32; 0x500]> = std::sync::OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table = [0u32; 0x500];
        let mut seed: u32 = 0x0010_0001;
        for index1 in 0..0x100 {
            let mut index2 = index1;
            for _ in 0..5 {
                seed = (seed.wrapping_mul(125).wrapping_add(3)) % 0x002A_AAAB;
                let high = (seed & 0xFFFF) << 16;
                seed = (seed.wrapping_mul(125).wrapping_add(3)) % 0x002A_AAAB;
                let low = seed & 0xFFFF;
                table[index2] = high | low;
                index2 += 0x100;
            }
        }
        table
    })
}

/// The hash of `name` of kind `kind` (0 table offset, 1 and 2 the name checks, 3 the
/// file key); names compare case-insensitively with `/` as `\`.
fn hash(name: &str, kind: u32) -> u32 {
    let table = crypt_table();
    let (mut seed1, mut seed2): (u32, u32) = (0x7FED_7FED, 0xEEEE_EEEE);
    for c in name.bytes() {
        let c = match c.to_ascii_uppercase() {
            b'/' => b'\\',
            c => c,
        } as u32;
        seed1 = table[(kind * 0x100 + c) as usize] ^ seed1.wrapping_add(seed2);
        seed2 = c
            .wrapping_add(seed1)
            .wrapping_add(seed2)
            .wrapping_add(seed2 << 5)
            .wrapping_add(3);
    }
    seed1
}

fn decrypt(words: &mut [u32], mut key: u32) {
    let table = crypt_table();
    let mut seed: u32 = 0xEEEE_EEEE;
    for word in words {
        seed = seed.wrapping_add(table[(0x400 + (key & 0xFF)) as usize]);
        let value = *word ^ key.wrapping_add(seed);
        key = ((!key << 0x15).wrapping_add(0x1111_1111)) | (key >> 0x0B);
        seed = value
            .wrapping_add(seed)
            .wrapping_add(seed << 5)
            .wrapping_add(3);
        *word = value;
    }
}

fn words(bytes: &[u8]) -> Vec<u32> {
    bytes
        .chunks_exact(4)
        .map(|c| u32::from_le_bytes([c[0], c[1], c[2], c[3]]))
        .collect()
}

fn bytes_of(words: &[u32]) -> Vec<u8> {
    words.iter().flat_map(|w| w.to_le_bytes()).collect()
}

#[derive(Debug, Clone, Copy)]
struct Block {
    offset: u32,
    packed: u32,
    size: u32,
    flags: u32,
}

const EXISTS: u32 = 0x8000_0000;
const IMPLODED: u32 = 0x0000_0100;
const COMPRESSED: u32 = 0x0000_0200;
const ENCRYPTED: u32 = 0x0001_0000;
const FIX_KEY: u32 = 0x0002_0000;
const SINGLE_UNIT: u32 = 0x0100_0000;
const SECTOR_CRC: u32 = 0x0400_0000;

/// An opened archive.
pub struct Archive<'a> {
    data: &'a [u8],
    /// Where the archive starts in `data` (maps may carry a 512-byte header first).
    base: usize,
    sector: usize,
    /// (name A, name B, block index) of each hash slot, in table order.
    hashes: Vec<(u32, u32, u32)>,
    blocks: Vec<Block>,
}

impl<'a> Archive<'a> {
    pub fn open(data: &'a [u8]) -> Result<Self, String> {
        if data.len() > 256 << 20 {
            return Err("the map is larger than 256 MB".into());
        }
        // StarCraft II maps may begin with a user data header pointing at the archive.
        let mut skip = 0usize;
        if data.get(..4) == Some(b"MPQ\x1B") {
            skip = data
                .get(8..12)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]) as usize)
                .ok_or("truncated MPQ user data")?;
        }
        let base = (skip..data.len().saturating_sub(32))
            .step_by(512)
            .find(|&i| &data[i..i + 4] == b"MPQ\x1A")
            .ok_or("not an MPQ archive (no header)")?;
        let u32_at = |at: usize| -> Result<u32, String> {
            data.get(at..at + 4)
                .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
                .ok_or_else(|| "truncated MPQ header".to_string())
        };
        let shift = data.get(base + 14).copied().ok_or("truncated MPQ header")?;
        if shift > 16 {
            return Err("unsupported MPQ sector size".into());
        }
        let sector = 512usize << shift;
        let (mut hash_at, mut block_at) =
            (u32_at(base + 16)? as usize, u32_at(base + 20)? as usize);
        // Format 1 and later: the tables' offsets carry 16 more high bits.
        let format = data.get(base + 12).copied().unwrap_or(0);
        if format >= 1 {
            let u16_at = |at: usize| {
                data.get(at..at + 2)
                    .map(|b| u16::from_le_bytes([b[0], b[1]]) as usize)
                    .unwrap_or(0)
            };
            hash_at |= u16_at(base + 40) << 32;
            block_at |= u16_at(base + 42) << 32;
        }
        let (hash_count, block_count) = (u32_at(base + 24)? as usize, u32_at(base + 28)? as usize);
        if hash_count == 0 || hash_count > 1 << 20 || block_count > 1 << 20 {
            return Err("invalid MPQ table sizes".into());
        }
        let table = |at: usize, count: usize, key: &str| -> Result<Vec<u32>, String> {
            let start = base + at;
            let end = start + count * 16;
            let raw = data
                .get(start..end)
                .ok_or("an MPQ table lies outside the file")?;
            let mut w = words(raw);
            decrypt(&mut w, hash(key, 3));
            Ok(w)
        };
        let h = table(hash_at, hash_count, "(hash table)")?;
        let b = table(block_at, block_count, "(block table)")?;
        let hashes = h.chunks_exact(4).map(|e| (e[0], e[1], e[3])).collect();
        let blocks = b
            .chunks_exact(4)
            .map(|e| Block {
                offset: e[0],
                packed: e[1],
                size: e[2],
                flags: e[3],
            })
            .collect();
        Ok(Self {
            data,
            base,
            sector,
            hashes,
            blocks,
        })
    }

    fn block(&self, name: &str) -> Option<Block> {
        let count = self.hashes.len();
        let (start, a, b) = (hash(name, 0) as usize % count, hash(name, 1), hash(name, 2));
        for step in 0..count {
            let (na, nb, index) = self.hashes[(start + step) % count];
            if index == 0xFFFF_FFFF {
                return None;
            }
            if na == a && nb == b && index != 0xFFFF_FFFE {
                return self
                    .blocks
                    .get(index as usize)
                    .copied()
                    .filter(|b| b.flags & EXISTS != 0);
            }
        }
        None
    }

    /// Whether the archive holds `name`.
    pub fn has(&self, name: &str) -> bool {
        self.block(name).is_some()
    }

    /// The file `name`, decoded.
    pub fn read(&self, name: &str) -> Result<Vec<u8>, String> {
        let block = self
            .block(name)
            .ok_or_else(|| format!("the map has no {name}"))?;
        let size = block.size as usize;
        if size > MAX_FILE {
            return Err(format!("{name} is larger than {} MB", MAX_FILE >> 20));
        }
        let start = self.base + block.offset as usize;
        let raw = self
            .data
            .get(start..start + block.packed as usize)
            .ok_or_else(|| format!("{name} lies outside the file"))?;
        let key = if block.flags & ENCRYPTED != 0 {
            let base_name = name.rsplit(['\\', '/']).next().unwrap_or(name);
            let mut key = hash(base_name, 3);
            if block.flags & FIX_KEY != 0 {
                key = (key.wrapping_add(block.offset)) ^ block.size;
            }
            Some(key)
        } else {
            None
        };
        let packed = block.flags & (COMPRESSED | IMPLODED) != 0;
        if block.flags & SINGLE_UNIT != 0 {
            let mut bytes = raw.to_vec();
            if let Some(key) = key {
                decrypt_bytes(&mut bytes, key);
            }
            return if packed && raw.len() < size {
                decompress(&bytes, size, block.flags)
            } else {
                Ok(bytes)
            };
        }
        if !packed {
            // Uncompressed sectors, each encrypted with its own key.
            let mut out = Vec::with_capacity(size);
            for (i, chunk) in raw.chunks(self.sector).enumerate() {
                let mut chunk = chunk.to_vec();
                if let Some(key) = key {
                    decrypt_bytes(&mut chunk, key.wrapping_add(i as u32));
                }
                out.extend_from_slice(&chunk);
            }
            out.truncate(size);
            return Ok(out);
        }
        let sectors = size.div_ceil(self.sector);
        let entries = sectors + 1 + usize::from(block.flags & SECTOR_CRC != 0);
        let table = raw
            .get(..entries * 4)
            .ok_or_else(|| format!("{name}: truncated sector table"))?;
        let mut offsets = words(table);
        if let Some(key) = key {
            decrypt(&mut offsets, key.wrapping_sub(1));
        }
        let mut out = Vec::with_capacity(size);
        for i in 0..sectors {
            let (from, to) = (offsets[i] as usize, offsets[i + 1] as usize);
            let mut chunk = raw
                .get(from..to)
                .ok_or_else(|| format!("{name}: a sector lies outside the file"))?
                .to_vec();
            if let Some(key) = key {
                decrypt_bytes(&mut chunk, key.wrapping_add(i as u32));
            }
            let expected = self.sector.min(size - i * self.sector);
            if chunk.len() < expected {
                out.extend(decompress(&chunk, expected, block.flags)?);
            } else {
                out.extend_from_slice(&chunk[..expected]);
            }
        }
        Ok(out)
    }
}

fn decrypt_bytes(bytes: &mut [u8], key: u32) {
    let whole = bytes.len() / 4 * 4;
    let mut w = words(&bytes[..whole]);
    decrypt(&mut w, key);
    bytes[..whole].copy_from_slice(&bytes_of(&w));
}

/// One compressed sector: a byte naming the method (zlib 0x02, bzip2 0x10), or PKWARE
/// implode for imploded files.
fn decompress(chunk: &[u8], size: usize, flags: u32) -> Result<Vec<u8>, String> {
    if flags & IMPLODED != 0 {
        return super::explode::explode(chunk, size);
    }
    let (&method, rest) = chunk.split_first().ok_or("an empty compressed sector")?;
    match method {
        0x02 => {
            let mut out = Vec::with_capacity(size);
            flate2::read::ZlibDecoder::new(rest)
                .take(size as u64)
                .read_to_end(&mut out)
                .map_err(|e| format!("zlib: {e}"))?;
            Ok(out)
        }
        0x10 => {
            let mut out = Vec::with_capacity(size);
            bzip2::read::BzDecoder::new(rest)
                .take(size as u64)
                .read_to_end(&mut out)
                .map_err(|e| format!("bzip2: {e}"))?;
            Ok(out)
        }
        0x08 => super::explode::explode(rest, size),
        other => Err(format!(
            "unsupported MPQ compression {other:#04x} (only zlib, bzip2 and implode)"
        )),
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use std::io::Write;

    /// A small archive holding `files` (zlib-compressed in 512-byte sectors, the second
    /// one encrypted with a fixed key), as Warcraft III writes them.
    pub fn archive(files: &[(&str, &[u8])]) -> Vec<u8> {
        let sector = 512usize;
        let mut body = Vec::new();
        let mut blocks = Vec::new();
        let header = 32usize;
        for (k, (name, content)) in files.iter().enumerate() {
            let offset = header + body.len();
            let sectors = content.len().div_ceil(sector);
            let mut data: Vec<Vec<u8>> = content
                .chunks(sector)
                .map(|c| {
                    let mut z =
                        flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
                    z.write_all(c).unwrap();
                    let mut packed = vec![0x02];
                    packed.extend(z.finish().unwrap());
                    if packed.len() >= c.len() {
                        c.to_vec()
                    } else {
                        packed
                    }
                })
                .collect();
            let mut offsets = vec![((sectors + 1) * 4) as u32];
            for d in &data {
                offsets.push(offsets.last().unwrap() + d.len() as u32);
            }
            let mut flags = EXISTS | COMPRESSED;
            if k == 1 {
                flags |= ENCRYPTED | FIX_KEY;
                let key = (hash(name, 3).wrapping_add(offset as u32)) ^ content.len() as u32;
                encrypt(&mut offsets, key.wrapping_sub(1));
                for (i, d) in data.iter_mut().enumerate() {
                    let whole = d.len() / 4 * 4;
                    let mut w = words(&d[..whole]);
                    encrypt(&mut w, key.wrapping_add(i as u32));
                    d[..whole].copy_from_slice(&bytes_of(&w));
                }
            }
            let start = body.len();
            body.extend(bytes_of(&offsets));
            for d in &data {
                body.extend(d);
            }
            blocks.push([
                offset as u32,
                (body.len() - start) as u32,
                content.len() as u32,
                flags,
            ]);
        }
        let hash_count = 16usize;
        let mut hashes = vec![[0xFFFF_FFFFu32; 4]; hash_count];
        for (i, (name, _)) in files.iter().enumerate() {
            let mut slot = hash(name, 0) as usize % hash_count;
            while hashes[slot][3] != 0xFFFF_FFFF {
                slot = (slot + 1) % hash_count;
            }
            hashes[slot] = [hash(name, 1), hash(name, 2), 0, i as u32];
        }
        let mut hash_words: Vec<u32> = hashes.concat();
        let mut block_words: Vec<u32> = blocks.concat();
        encrypt(&mut hash_words, hash("(hash table)", 3));
        encrypt(&mut block_words, hash("(block table)", 3));
        let hash_at = header + body.len();
        let block_at = hash_at + hash_count * 16;
        let mut out = Vec::new();
        out.extend(b"MPQ\x1A");
        out.extend((header as u32).to_le_bytes());
        out.extend(((block_at + blocks.len() * 16) as u32).to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend(0u16.to_le_bytes());
        out.extend((hash_at as u32).to_le_bytes());
        out.extend((block_at as u32).to_le_bytes());
        out.extend((hash_count as u32).to_le_bytes());
        out.extend((blocks.len() as u32).to_le_bytes());
        out.extend(body);
        out.extend(bytes_of(&hash_words));
        out.extend(bytes_of(&block_words));
        // A 512-byte map header comes first in .w3x files.
        let mut map = vec![0u8; 512];
        map[..4].copy_from_slice(b"HM3W");
        map.extend(out);
        map
    }

    fn encrypt(words: &mut [u32], mut key: u32) {
        let table = crypt_table();
        let mut seed: u32 = 0xEEEE_EEEE;
        for word in words {
            seed = seed.wrapping_add(table[(0x400 + (key & 0xFF)) as usize]);
            let plain = *word;
            *word = plain ^ key.wrapping_add(seed);
            key = ((!key << 0x15).wrapping_add(0x1111_1111)) | (key >> 0x0B);
            seed = plain
                .wrapping_add(seed)
                .wrapping_add(seed << 5)
                .wrapping_add(3);
        }
    }

    #[test]
    fn the_hash_matches_the_known_table_keys() {
        // Well-known keys every MPQ uses.
        assert_eq!(hash("(hash table)", 3), 0xC3AF_3770);
        assert_eq!(hash("(block table)", 3), 0xEC83_B3A3);
    }

    #[test]
    fn files_come_back_through_tables_sectors_zlib_and_encryption() {
        let big: Vec<u8> = (0..2000u32).flat_map(|i| (i % 251).to_le_bytes()).collect();
        let small = b"W3E! a small file".to_vec();
        let bytes = archive(&[("war3map.w3e", &small), ("war3mapUnits.doo", &big)]);
        let archive = Archive::open(&bytes).unwrap();
        assert_eq!(archive.read("war3map.w3e").unwrap(), small);
        assert_eq!(archive.read("WAR3MAPUNITS.DOO").unwrap(), big);
        assert!(!archive.has("war3map.j"));
        assert!(Archive::open(b"not an archive").is_err());
    }
}
