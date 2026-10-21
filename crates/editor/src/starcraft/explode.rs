//! PKWARE Data Compression Library "implode" decoding, which older MPQ archives use: a
//! port of zlib's `contrib/blast/blast.c` (Mark Adler).

const MAX_BITS: usize = 13;

struct Huffman {
    count: [u16; MAX_BITS + 1],
    symbol: Vec<u16>,
}

impl Huffman {
    /// From blast's compact form: each byte is (repeat - 1) << 4 | length.
    fn new(rep: &[u8]) -> Self {
        let mut lengths = Vec::new();
        for &b in rep {
            for _ in 0..=(b >> 4) {
                lengths.push((b & 15) as usize);
            }
        }
        let mut count = [0u16; MAX_BITS + 1];
        for &l in &lengths {
            count[l] += 1;
        }
        let mut offs = [0u16; MAX_BITS + 1];
        for len in 1..MAX_BITS {
            offs[len + 1] = offs[len] + count[len];
        }
        let mut symbol = vec![0u16; lengths.len()];
        for (s, &l) in lengths.iter().enumerate() {
            if l != 0 {
                symbol[offs[l] as usize] = s as u16;
                offs[l] += 1;
            }
        }
        Self { count, symbol }
    }
}

const LIT: [u8; 98] = [
    11, 124, 8, 7, 28, 7, 188, 13, 76, 4, 10, 8, 12, 10, 12, 10, 8, 23, 8, 9, 7, 6, 7, 8, 7, 6, 55,
    8, 23, 24, 12, 11, 7, 9, 11, 12, 6, 7, 22, 5, 7, 24, 6, 11, 9, 6, 7, 22, 7, 11, 38, 7, 9, 8,
    25, 11, 8, 11, 9, 12, 8, 12, 5, 38, 5, 38, 5, 11, 7, 5, 6, 21, 6, 10, 53, 8, 7, 24, 10, 27, 44,
    253, 253, 253, 252, 252, 252, 13, 12, 45, 12, 45, 12, 61, 12, 45, 44, 173,
];
const LEN: [u8; 6] = [2, 35, 36, 53, 38, 23];
const DIST: [u8; 7] = [2, 20, 53, 230, 247, 151, 248];
const BASE: [usize; 16] = [3, 2, 4, 5, 6, 7, 8, 9, 10, 12, 16, 24, 40, 72, 136, 264];
const EXTRA: [usize; 16] = [0, 0, 0, 0, 0, 0, 0, 0, 1, 2, 3, 4, 5, 6, 7, 8];

struct Bits<'a> {
    input: &'a [u8],
    at: usize,
    buffer: u32,
    count: usize,
}

impl Bits<'_> {
    fn bits(&mut self, need: usize) -> Result<usize, String> {
        while self.count < need {
            let byte = *self
                .input
                .get(self.at)
                .ok_or("implode: input ended early")?;
            self.at += 1;
            self.buffer |= u32::from(byte) << self.count;
            self.count += 8;
        }
        let value = self.buffer & ((1u32 << need) - 1);
        self.buffer >>= need;
        self.count -= need;
        Ok(value as usize)
    }

    fn decode(&mut self, h: &Huffman) -> Result<usize, String> {
        let (mut code, mut first, mut index) = (0usize, 0usize, 0usize);
        for len in 1..=MAX_BITS {
            code |= self.bits(1)? ^ 1;
            let count = h.count[len] as usize;
            if code < first + count {
                return Ok(h.symbol[index + code - first] as usize);
            }
            index += count;
            first = (first + count) << 1;
            code <<= 1;
        }
        Err("implode: invalid code".into())
    }
}

/// Decodes `input` into at most `size` bytes.
pub fn explode(input: &[u8], size: usize) -> Result<Vec<u8>, String> {
    let (lit, len, dist) = (Huffman::new(&LIT), Huffman::new(&LEN), Huffman::new(&DIST));
    let mut s = Bits {
        input,
        at: 0,
        buffer: 0,
        count: 0,
    };
    let coded = s.bits(8)?;
    let dict = s.bits(8)?;
    if coded > 1 || !(4..=6).contains(&dict) {
        return Err("implode: invalid header".into());
    }
    let mut out = Vec::with_capacity(size);
    while out.len() < size {
        if s.bits(1)? == 1 {
            let symbol = s.decode(&len)?;
            let length = BASE[symbol] + s.bits(EXTRA[symbol])?;
            if length == 519 {
                break;
            }
            let shift = if length == 2 { 2 } else { dict };
            let distance = (s.decode(&dist)? << shift) + s.bits(shift)? + 1;
            if distance > out.len() {
                return Err("implode: distance too far back".into());
            }
            for _ in 0..length {
                let byte = out[out.len() - distance];
                out.push(byte);
            }
        } else {
            let byte = if coded == 1 {
                s.decode(&lit)?
            } else {
                s.bits(8)?
            };
            out.push(byte as u8);
        }
    }
    out.truncate(size);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_blasts_reference_stream() {
        // blast.c's own test: "AIAIAIAIAIAIA".
        let input = [0x00, 0x04, 0x82, 0x24, 0x25, 0x8f, 0x80, 0x7f];
        assert_eq!(explode(&input, 64).unwrap(), b"AIAIAIAIAIAIA");
    }
}
