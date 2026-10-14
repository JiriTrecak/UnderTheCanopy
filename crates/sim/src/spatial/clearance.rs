//! Positive proof that a long ground sweep is clear (old `TerrainClearance.provesClear`,
//! `src/sim/game/terrainClearance.ts`): if no closed cell and no steep edge entered in the
//! sweep's direction lies in the swept envelope, every ray of the exact sweep passes. An
//! unproven sweep falls back to the exact rays, so answers never change.

use crate::{spatial::geometry::Fixed, terrain::SteepEdges};

#[derive(Debug, Clone)]
pub struct Clearance {
    size: usize,
    words: usize,
    /// Cells closed to ground movement (terrain or static occupancy), one bit each.
    closed: Vec<u32>,
}

impl Clearance {
    pub fn new(size: u32, walkable: impl Fn(u32) -> bool) -> Self {
        let size = size as usize;
        let words = size.div_ceil(32);
        let mut this = Self {
            size,
            words,
            closed: vec![0; words * size],
        };
        for cell in 0..(size * size) as u32 {
            this.refresh(cell, walkable(cell));
        }
        this
    }

    pub fn refresh(&mut self, cell: u32, walkable: bool) {
        let (x, y) = (cell as usize % self.size, cell as usize / self.size);
        let word = y * self.words + x / 32;
        let bit = 1u32 << (x % 32);
        if walkable {
            self.closed[word] &= !bit;
        } else {
            self.closed[word] |= bit;
        }
    }

    /// Whether the square sweep of `radius` from `from` to `to` provably meets nothing.
    pub fn proves_clear(&self, steep: &SteepEdges, from: Fixed, to: Fixed, radius: i32) -> bool {
        let (dx, dy) = (i64::from(to.x - from.x), i64::from(to.y - from.y));
        let (sx, sy) = (dx.signum() as i32, dy.signum() as i32);
        let horizontal = steep.plane(sx, 0);
        let vertical = steep.plane(0, sy);
        let diagonal = steep.plane(sx, sy);
        let r = i64::from(radius);
        let left = i64::from(from.x.min(to.x)) - r;
        let right = i64::from(from.x.max(to.x)) + r;
        let top = i64::from(from.y.min(to.y)) - r;
        let bottom = i64::from(from.y.max(to.y)) + r;
        let edge = self.size as i64 * 1000 - 500;
        if left < -500 || top < -500 || right >= edge || bottom >= edge {
            return false;
        }
        let y0 = (top + 500).div_euclid(1000);
        let y1 = (bottom + 500).div_euclid(1000);
        let (fx, fy, fdx, fdy, fr) = (
            f64::from(from.x),
            f64::from(from.y),
            dx as f64,
            dy as f64,
            r as f64,
        );
        for y in y0..=y1 {
            let (mut enter, mut leave) = (0.0f64, 1.0f64);
            if dy != 0 {
                let a = ((y * 1000 - 500 - r) as f64 - fy) / fdy;
                let b = ((y * 1000 + 500 + r) as f64 - fy) / fdy;
                enter = a.min(b).max(0.0);
                leave = a.max(b).min(1.0);
            }
            // Both corner-touching cells, with a margin against division roundoff.
            let a = fx + fdx * enter;
            let b = fx + fdx * leave;
            let x0 = (((a.min(b) - fr + 500.0 - 1e-7) / 1000.0).floor() as i64).max(0);
            let x1 = (((a.max(b) + fr + 500.0 + 1e-7) / 1000.0).floor() as i64)
                .min(self.size as i64 - 1);
            if x1 < x0 {
                continue;
            }
            let (first, last) = ((x0 >> 5) as usize, (x1 >> 5) as usize);
            let row = y as usize * self.words;
            for word in first..=last {
                let low = if word == first {
                    u32::MAX << (x0 & 31)
                } else {
                    u32::MAX
                };
                let high = if word == last {
                    u32::MAX >> (31 - (x1 & 31))
                } else {
                    u32::MAX
                };
                let index = row + word;
                if (self.closed[index] | horizontal[index] | vertical[index] | diagonal[index])
                    & low
                    & high
                    != 0
                {
                    return false;
                }
            }
        }
        true
    }
}
