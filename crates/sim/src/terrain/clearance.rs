//! Directed steep edges between neighbouring cells: the terrain half of the old
//! `TerrainClearance` (`src/sim/game/terrainClearance.ts`). Movement combines these
//! planes with its occupancy bits to prove long sweeps clear without ray tests.

use super::tactical::MAX_GROUND_STEP_CM;

/// One bit per cell per incoming direction, rows packed into 32-bit words.
#[derive(Debug, Clone)]
pub struct SteepEdges {
    size: usize,
    words_per_row: usize,
    /// Plane `(dy + 1) * 3 + dx + 1` marks cells entered from `(x − dx, y − dy)` across a
    /// step higher than [`MAX_GROUND_STEP_CM`].
    planes: [Vec<u32>; 9],
}

impl SteepEdges {
    pub fn new(size: u32, heights: &[i16]) -> Self {
        let size = size as usize;
        let words_per_row = size.div_ceil(32);
        let mut planes: [Vec<u32>; 9] = std::array::from_fn(|_| vec![0; words_per_row * size]);
        let step = MAX_GROUND_STEP_CM;
        for y in 0..size {
            for x in 0..size {
                let h = i32::from(heights[y * size + x]);
                let (word, bit) = (y * words_per_row + x / 32, 1u32 << (x % 32));
                for dy in -1i64..=1 {
                    for dx in -1i64..=1 {
                        let (px, py) = (x as i64 - dx, y as i64 - dy);
                        if px < 0 || py < 0 || px >= size as i64 || py >= size as i64 {
                            continue;
                        }
                        let other = i32::from(heights[py as usize * size + px as usize]);
                        if (h - other).abs() > step {
                            planes[((dy + 1) * 3 + dx + 1) as usize][word] |= bit;
                        }
                    }
                }
            }
        }
        Self {
            size,
            words_per_row,
            planes,
        }
    }

    pub fn words_per_row(&self) -> usize {
        self.words_per_row
    }

    /// The plane of cells entered by a step of `(dx, dy)` (each −1, 0 or 1).
    pub fn plane(&self, dx: i32, dy: i32) -> &[u32] {
        &self.planes[((dy + 1) * 3 + dx + 1) as usize]
    }

    /// Whether stepping from `(x − dx, y − dy)` into `(x, y)` crosses a steep edge.
    pub fn entered(&self, x: usize, y: usize, dx: i32, dy: i32) -> bool {
        debug_assert!(x < self.size && y < self.size);
        self.plane(dx, dy)[y * self.words_per_row + x / 32] & (1 << (x % 32)) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn steps_are_marked_in_the_direction_they_are_entered() {
        // A 40-cell map (two words per row) with a 1 m cliff at x ≥ 35.
        let size = 40;
        let heights: Vec<i16> = (0..size * size)
            .map(|i| if i % size >= 35 { 100 } else { 0 })
            .collect();
        let edges = SteepEdges::new(size as u32, &heights);
        assert_eq!(edges.words_per_row(), 2);
        assert!(edges.entered(35, 3, 1, 0));
        assert!(edges.entered(34, 3, -1, 0));
        assert!(edges.entered(35, 3, 1, 1));
        assert!(!edges.entered(35, 3, 0, 1));
        assert!(!edges.entered(36, 3, 1, 0));
        assert!(!edges.entered(10, 3, 1, 0));
    }
}
