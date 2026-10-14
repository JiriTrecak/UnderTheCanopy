//! The committed height and water fields, sampled between vertices (old `HeightField`,
//! `src/shared/map/height.ts`, and `cellWaterAt` in `terrainData.ts`).

use canopy_content::map::{GridError, MAP_HALO, TerrainData};

/// World position of vertex 0 on both axes.
const ORIGIN: f64 = -(MAP_HALO as f64);

/// Ground and water heights at every vertex, in world units. A map without watercourses
/// (every saved map) takes its water from the committed per-vertex water heights.
#[derive(Debug, Clone)]
pub struct HeightField {
    /// Cells per side including the halo.
    span: u32,
    /// Vertices per side (`span + 1`).
    vertices: u32,
    ground: Vec<f32>,
    water: Vec<f32>,
}

impl HeightField {
    /// Decodes only the grids the simulation reads.
    pub fn decode(terrain: &TerrainData) -> Result<Self, GridError> {
        Ok(Self {
            span: terrain.span(),
            vertices: terrain.vertices(),
            ground: terrain.heights()?,
            water: terrain.water_heights()?,
        })
    }

    /// A field from raw vertex grids (`vertices²` samples each, row index z).
    pub fn from_samples(size: u32, ground: Vec<f32>, water: Vec<f32>) -> Self {
        let span = size + 2 * MAP_HALO;
        let vertices = span + 1;
        let count = (vertices as usize).pow(2);
        assert!(
            ground.len() == count && water.len() == count,
            "height field needs {count} samples"
        );
        Self {
            span,
            vertices,
            ground,
            water,
        }
    }

    /// Ground height at a world position, bilinear between vertices. Outside the halo the
    /// ground is at 0, as the old `sampleHeight` answers.
    pub fn sample(&self, x: f64, z: f64) -> f64 {
        let span = f64::from(self.span);
        let fx = x - ORIGIN;
        let fz = z - ORIGIN;
        if !(fx >= 0.0 && fz >= 0.0 && fx <= span && fz <= span) {
            return 0.0;
        }
        let last = self.span as usize;
        let x0 = (fx.floor() as usize).min(last);
        let z0 = (fz.floor() as usize).min(last);
        let x1 = (x0 + 1).min(last);
        let z1 = (z0 + 1).min(last);
        let tx = fx - x0 as f64;
        let tz = fz - z0 as f64;
        let w = self.vertices as usize;
        let at = |x: usize, z: usize| f64::from(self.ground[z * w + x]);
        let (a, b, c, d) = (at(x0, z0), at(x1, z0), at(x0, z1), at(x1, z1));
        a * (1.0 - tx) * (1.0 - tz) + b * tx * (1.0 - tz) + c * (1.0 - tx) * tz + d * tx * tz
    }

    /// Water surface at a world position, bilinear between vertices; positions outside the
    /// halo take the nearest edge.
    pub fn water_at(&self, x: f64, z: f64) -> f64 {
        let span = f64::from(self.span);
        let gx = (x - ORIGIN).clamp(0.0, span);
        let gz = (z - ORIGIN).clamp(0.0, span);
        let last = self.span as usize;
        let ix = gx.floor() as usize;
        let iz = gz.floor() as usize;
        let jx = (ix + 1).min(last);
        let jz = (iz + 1).min(last);
        let u = gx - ix as f64;
        let v = gz - iz as f64;
        let w = self.vertices as usize;
        let h = |x: usize, z: usize| f64::from(self.water[z * w + x]);
        (h(ix, iz) * (1.0 - u) + h(jx, iz) * u) * (1.0 - v)
            + (h(ix, jz) * (1.0 - u) + h(jx, jz) * u) * v
    }

    /// Whether the ground lies below the water surface.
    pub fn wet(&self, x: f64, z: f64) -> bool {
        self.sample(x, z) < self.water_at(x, z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A 256-cell field whose ground rises one unit per vertex along x, with water at 2.5.
    fn ramp() -> HeightField {
        let vertices = 256 + 2 * MAP_HALO as usize + 1;
        let ground = (0..vertices * vertices)
            .map(|i| (i % vertices) as f32)
            .collect();
        HeightField::from_samples(256, ground, vec![2.5; vertices * vertices])
    }

    #[test]
    fn samples_interpolate_between_vertices() {
        let field = ramp();
        // World x = -16 is vertex 0.
        assert_eq!(field.sample(-16.0, 0.0), 0.0);
        assert_eq!(field.sample(0.0, 0.0), 16.0);
        assert_eq!(field.sample(0.25, 3.0), 16.25);
        assert_eq!(field.sample(272.0, 272.0), 288.0);
        assert!(field.wet(-15.0, 0.0));
        assert!(!field.wet(-13.0, 0.0));
    }

    #[test]
    fn outside_the_halo_ground_is_zero_and_water_clamps() {
        let field = ramp();
        assert_eq!(field.sample(-16.5, 0.0), 0.0);
        assert_eq!(field.sample(0.0, 272.5), 0.0);
        assert_eq!(field.sample(f64::NAN, 0.0), 0.0);
        assert_eq!(field.water_at(-100.0, 5000.0), 2.5);
    }
}
