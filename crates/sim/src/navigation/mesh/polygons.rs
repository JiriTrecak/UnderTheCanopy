//! Tile-local convex regions of at most six vertices, merged from the triangles in a fixed
//! order (old `convexPolygons`, `src/shared/navigation/groundMeshPolygons.ts`).

use super::build::TileGeometry;
use std::collections::BTreeMap;

/// A point in cells: `(x, z)`.
pub type P = (f64, f64);

pub fn cross(a: P, b: P, c: P) -> f64 {
    (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0)
}

/// Convex polygons of a tile, in cells.
pub fn convex_polygons(tile: &TileGeometry) -> Vec<Vec<P>> {
    let vertices: Vec<P> = tile
        .vertices
        .iter()
        .map(|&(x, z)| (f64::from(x) / 1000.0, f64::from(z) / 1000.0))
        .collect();
    // Tile vertices are unique by coordinate, so each is its own canonical key.
    let width = vertices.len() as u64;
    let mut polygons: Vec<Option<Vec<u32>>> =
        tile.triangles.iter().map(|t| Some(t.to_vec())).collect();
    let mut versions = vec![0u32; polygons.len()];
    let merge = |a: &[u32], ai: usize, b: &[u32], bi: usize| -> Option<Vec<u32>> {
        if a.len() + b.len() - 2 > 6 {
            return None;
        }
        let mut points = Vec::with_capacity(6);
        for k in 1..=a.len() {
            points.push(a[(ai + k) % a.len()]);
        }
        for k in 2..b.len() {
            points.push(b[(bi + k) % b.len()]);
        }
        for i in 0..points.len() {
            for j in 0..i {
                if vertices[points[i] as usize] == vertices[points[j] as usize] {
                    return None;
                }
            }
        }
        let n = points.len();
        for i in 0..n {
            let (p, q, r) = (
                vertices[points[i] as usize],
                vertices[points[(i + 1) % n] as usize],
                vertices[points[(i + 2) % n] as usize],
            );
            if cross(p, q, r) > 1e-8 {
                return None;
            }
        }
        Some(points)
    };
    let mut changed = true;
    while changed {
        changed = false;
        // Directed edge → (polygon, edge index, polygon version when recorded).
        let mut edges: BTreeMap<u64, (usize, usize, u32)> = BTreeMap::new();
        for id in 0..polygons.len() {
            let Some(points) = polygons[id].clone() else {
                continue;
            };
            for edge in 0..points.len() {
                let a = u64::from(points[edge]);
                let b = u64::from(points[(edge + 1) % points.len()]);
                if let Some(&(other, other_edge, version)) = edges.get(&(b * width + a))
                    && versions[other] == version
                    && let Some(other_points) = polygons[other].as_ref()
                    && let Some(combined) = merge(other_points, other_edge, &points, edge)
                {
                    polygons[other] = Some(combined);
                    versions[other] += 1;
                    polygons[id] = None;
                    changed = true;
                    break;
                }
                edges.insert(a * width + b, (id, edge, versions[id]));
            }
        }
    }
    polygons
        .into_iter()
        .flatten()
        .map(|points| points.iter().map(|&i| vertices[i as usize]).collect())
        .collect()
}
