//! Small f64 affine maths for baking. Transcendental functions come from `libm`, so
//! cooks are identical on every platform.

// Matrix code reads best with explicit row and column indices.
#![allow(clippy::needless_range_loop)]

use std::ops::Mul;

pub type Vec3 = [f64; 3];
pub type Quat = [f64; 4];

/// A column-major 4×4 matrix (glTF layout): `m[col][row]`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Mat4(pub [[f64; 4]; 4]);

impl Mat4 {
    pub const IDENTITY: Self = Self([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ]);

    pub fn from_cols_f32(m: [[f32; 4]; 4]) -> Self {
        Self(m.map(|c| c.map(f64::from)))
    }

    pub fn translation(t: Vec3) -> Self {
        let mut m = Self::IDENTITY;
        m.0[3] = [t[0], t[1], t[2], 1.0];
        m
    }

    pub fn scale(s: Vec3) -> Self {
        let mut m = Self::IDENTITY;
        m.0[0][0] = s[0];
        m.0[1][1] = s[1];
        m.0[2][2] = s[2];
        m
    }

    /// Rotation about +Y by `angle` radians.
    pub fn rotation_y(angle: f64) -> Self {
        let (s, c) = (libm::sin(angle), libm::cos(angle));
        let mut m = Self::IDENTITY;
        m.0[0] = [c, 0.0, -s, 0.0];
        m.0[2] = [s, 0.0, c, 0.0];
        m
    }

    pub fn from_trs(t: Vec3, r: Quat, s: Vec3) -> Self {
        let [x, y, z, w] = r;
        let (x2, y2, z2) = (x + x, y + y, z + z);
        let (xx, xy, xz) = (x * x2, x * y2, x * z2);
        let (yy, yz, zz) = (y * y2, y * z2, z * z2);
        let (wx, wy, wz) = (w * x2, w * y2, w * z2);
        Self([
            [
                (1.0 - (yy + zz)) * s[0],
                (xy + wz) * s[0],
                (xz - wy) * s[0],
                0.0,
            ],
            [
                (xy - wz) * s[1],
                (1.0 - (xx + zz)) * s[1],
                (yz + wx) * s[1],
                0.0,
            ],
            [
                (xz + wy) * s[2],
                (yz - wx) * s[2],
                (1.0 - (xx + yy)) * s[2],
                0.0,
            ],
            [t[0], t[1], t[2], 1.0],
        ])
    }

    pub fn transform_point(&self, p: Vec3) -> Vec3 {
        let m = &self.0;
        std::array::from_fn(|r| m[0][r] * p[0] + m[1][r] * p[1] + m[2][r] * p[2] + m[3][r])
    }

    pub fn transform_vector(&self, v: Vec3) -> Vec3 {
        let m = &self.0;
        std::array::from_fn(|r| m[0][r] * v[0] + m[1][r] * v[1] + m[2][r] * v[2])
    }

    /// The matrix that transforms normals: the inverse transpose of the linear part.
    pub fn normal_matrix(&self) -> Self {
        let inv = self.inverse().0;
        let mut out = Self::IDENTITY;
        for c in 0..3 {
            for r in 0..3 {
                out.0[c][r] = inv[r][c];
            }
        }
        out
    }

    /// Transforms a normal with a [`Mat4::normal_matrix`].
    pub fn transform_normal(&self, n: Vec3) -> Vec3 {
        normalize(self.transform_vector(n))
    }

    pub fn determinant3(&self) -> f64 {
        let m = &self.0;
        m[0][0] * (m[1][1] * m[2][2] - m[2][1] * m[1][2])
            - m[1][0] * (m[0][1] * m[2][2] - m[2][1] * m[0][2])
            + m[2][0] * (m[0][1] * m[1][2] - m[1][1] * m[0][2])
    }

    /// Inverse of an affine matrix (the last row is 0 0 0 1).
    pub fn inverse(&self) -> Self {
        let m = &self.0;
        let det = self.determinant3();
        let inv_det = if det.abs() < 1e-300 { 0.0 } else { 1.0 / det };
        let mut r = [[0.0; 4]; 4];
        r[0][0] = (m[1][1] * m[2][2] - m[2][1] * m[1][2]) * inv_det;
        r[0][1] = (m[2][1] * m[0][2] - m[0][1] * m[2][2]) * inv_det;
        r[0][2] = (m[0][1] * m[1][2] - m[1][1] * m[0][2]) * inv_det;
        r[1][0] = (m[2][0] * m[1][2] - m[1][0] * m[2][2]) * inv_det;
        r[1][1] = (m[0][0] * m[2][2] - m[2][0] * m[0][2]) * inv_det;
        r[1][2] = (m[1][0] * m[0][2] - m[0][0] * m[1][2]) * inv_det;
        r[2][0] = (m[1][0] * m[2][1] - m[2][0] * m[1][1]) * inv_det;
        r[2][1] = (m[2][0] * m[0][1] - m[0][0] * m[2][1]) * inv_det;
        r[2][2] = (m[0][0] * m[1][1] - m[1][0] * m[0][1]) * inv_det;
        let t = m[3];
        for row in 0..3 {
            r[3][row] = -(r[0][row] * t[0] + r[1][row] * t[1] + r[2][row] * t[2]);
        }
        r[3][3] = 1.0;
        Self(r)
    }

    /// Row-major 3×4 for the pack.
    pub fn to_3x4(self) -> [f32; 12] {
        let m = &self.0;
        let mut out = [0.0f32; 12];
        for row in 0..3 {
            for col in 0..4 {
                out[row * 4 + col] = m[col][row] as f32;
            }
        }
        out
    }
}

impl Mul for Mat4 {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        let (a, b) = (&self.0, &rhs.0);
        Self(std::array::from_fn(|col| {
            std::array::from_fn(|row| (0..4).map(|k| a[k][row] * b[col][k]).sum())
        }))
    }
}

pub fn normalize(v: Vec3) -> Vec3 {
    let len = libm::sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]);
    if len > 1e-20 {
        v.map(|c| c / len)
    } else {
        [0.0, 1.0, 0.0]
    }
}

pub fn lerp3(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t)
}

pub fn quat_normalize(q: Quat) -> Quat {
    let len = libm::sqrt(q.iter().map(|c| c * c).sum());
    if len > 1e-20 {
        q.map(|c| c / len)
    } else {
        [0.0, 0.0, 0.0, 1.0]
    }
}

/// Spherical interpolation along the shorter arc.
pub fn slerp(a: Quat, b: Quat, t: f64) -> Quat {
    let mut dot: f64 = (0..4).map(|i| a[i] * b[i]).sum();
    let b = if dot < 0.0 {
        dot = -dot;
        b.map(|c| -c)
    } else {
        b
    };
    if dot > 0.9995 {
        return quat_normalize(std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t));
    }
    let theta = libm::acos(dot.min(1.0));
    let sin = libm::sin(theta);
    let wa = libm::sin((1.0 - t) * theta) / sin;
    let wb = libm::sin(t * theta) / sin;
    quat_normalize(std::array::from_fn(|i| a[i] * wa + b[i] * wb))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: &Mat4, b: &Mat4) -> bool {
        a.0.iter()
            .flatten()
            .zip(b.0.iter().flatten())
            .all(|(x, y)| (x - y).abs() < 1e-9)
    }

    #[test]
    fn inverse_undoes_affine_transforms() {
        let m = Mat4::from_trs(
            [1.0, -2.0, 3.0],
            quat_normalize([0.2, 0.5, -0.1, 0.8]),
            [2.0, 0.5, 1.5],
        );
        assert!(close(&(m * m.inverse()), &Mat4::IDENTITY));
    }

    #[test]
    fn slerp_hits_the_ends_and_stays_unit() {
        let a = [0.0, 0.0, 0.0, 1.0];
        let b = quat_normalize([0.0, 1.0, 0.0, 1.0]);
        assert!(
            slerp(a, b, 0.0)
                .iter()
                .zip(a)
                .all(|(x, y)| (x - y).abs() < 1e-12)
        );
        assert!(
            slerp(a, b, 1.0)
                .iter()
                .zip(b)
                .all(|(x, y)| (x - y).abs() < 1e-12)
        );
        let mid = slerp(a, b, 0.5);
        assert!((mid.iter().map(|c| c * c).sum::<f64>() - 1.0).abs() < 1e-12);
    }
}
