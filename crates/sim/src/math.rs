//! Deterministic maths. Transcendental functions come from the pure-Rust `libm` so every
//! platform computes identical bits; the standard library versions are forbidden in this
//! crate (`clippy.toml`).

/// Milli-units per navigation cell: positions are stored in 1/1000 cells.
pub const MILLI: i64 = 1000;

pub fn sin(x: f64) -> f64 {
    libm::sin(x)
}

pub fn cos(x: f64) -> f64 {
    libm::cos(x)
}

pub fn atan2(y: f64, x: f64) -> f64 {
    libm::atan2(y, x)
}

pub fn hypot(x: f64, y: f64) -> f64 {
    libm::hypot(x, y)
}

/// Largest integer whose square is at most `n`.
pub fn isqrt(n: u64) -> u64 {
    n.isqrt()
}

/// Smallest integer whose square is at least `n`.
pub fn isqrt_ceil(n: u64) -> u64 {
    let root = n.isqrt();
    if root * root == n { root } else { root + 1 }
}

/// Integer length of `(dx, dy)`, rounded up.
pub fn length_ceil(dx: i64, dy: i64) -> u64 {
    isqrt_ceil(dx.unsigned_abs().pow(2) + dy.unsigned_abs().pow(2))
}

/// Floor division that rounds toward negative infinity for negative numerators.
pub fn div_floor(a: i64, b: i64) -> i64 {
    a.div_euclid(b) - if b < 0 && a.rem_euclid(b) != 0 { 1 } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn integer_roots_bracket_the_value() {
        for n in [
            0u64,
            1,
            2,
            3,
            4,
            15,
            16,
            17,
            1_000_000,
            999_999,
            u32::MAX as u64,
        ] {
            let (low, high) = (isqrt(n), isqrt_ceil(n));
            assert!(low * low <= n && (low + 1) * (low + 1) > n, "{n}");
            assert!(
                high * high >= n && (high == 0 || (high - 1) * (high - 1) < n),
                "{n}"
            );
        }
        assert_eq!(length_ceil(3, -4), 5);
        assert_eq!(length_ceil(1, 1), 2);
    }

    #[test]
    fn floor_division_rounds_down() {
        assert_eq!(div_floor(7, 2), 3);
        assert_eq!(div_floor(-7, 2), -4);
        assert_eq!(div_floor(-8, 2), -4);
        assert_eq!(div_floor(7, -2), -4);
    }

    /// Fixed vectors pin the libm results, so a dependency change that alters bits fails here
    /// rather than as a desync.
    #[test]
    fn transcendentals_are_pinned() {
        assert_eq!(sin(1.0).to_bits(), 0x3FEA_ED54_8F09_0CEE);
        assert_eq!(cos(1.0).to_bits(), 0x3FE1_4A28_0FB5_068C);
        assert_eq!(atan2(1.0, 2.0).to_bits(), 0x3FDD_AC67_0561_BB4F);
        assert_eq!(hypot(3.0, 4.0), 5.0);
    }
}
