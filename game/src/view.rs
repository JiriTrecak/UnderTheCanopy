//! The spectator camera over a loaded map.

use crate::options::CameraSpec;
use canopy_render::{
    RtsCamera,
    camera::{self, CameraView},
    terrain::TerrainDesc,
};
use canopy_scene::MapScene;
use glam::Vec3;

/// The spectator may zoom out further than a player (the game stops at 1.25).
pub const SPECTATOR_MAX_ZOOM: f32 = 6.0;

/// Lowest water surface, or 0: the plane the camera keeps its footprint on (old
/// `clamp`, `min(0, waterLevel)`).
pub fn clamp_floor(terrain: &TerrainDesc) -> f32 {
    terrain
        .water
        .iter()
        .zip(&terrain.heights)
        .filter(|(w, h)| w > h)
        .map(|(w, _)| *w)
        .fold(0.0, f32::min)
}

/// The camera a view starts with: the requested pose, or the first player's start at
/// the game's default zoom.
pub fn initial(spec: Option<CameraSpec>, scene: &MapScene) -> RtsCamera {
    let focus = spec
        .map(|s| [s.x, s.z])
        .or_else(|| scene.starts.first().copied())
        .unwrap_or([scene.size as f32 / 2.0; 2]);
    let mut camera = RtsCamera::new(focus, Some(scene.playable));
    camera.zoom_range = (camera::game_zoom().0, SPECTATOR_MAX_ZOOM);
    if let Some(spec) = spec {
        camera.set_zoom(spec.zoom);
        camera.yaw = spec.yaw.to_radians();
    }
    camera
}

/// Clamps the camera like the old game every frame, then derives the view.
pub fn frame(camera: &mut RtsCamera, aspect: f32, scene: &MapScene) -> CameraView {
    let terrain = scene.terrain();
    camera.clamp(aspect, clamp_floor(terrain));
    camera.view(aspect, terrain)
}

/// Middle-drag panning (old `panPersp`, "grab-pan from the center ray"): how far the
/// focus moves for a drag of `(dx, dy)` pixels on a `size` screen. The ground under the
/// screen centre follows the pointer: one pixel right and one pixel down are measured
/// where the centre ray and its neighbours meet the plane through the view's target, so
/// the drag feels the same wherever it starts on the screen.
pub fn grab_pan(view: &CameraView, size: [f32; 2], dx: f32, dy: f32) -> [f32; 2] {
    let [w, h] = [size[0].max(1.0), size[1].max(1.0)];
    let inverse = view.view_projection.inverse();
    let plane = view.target.y;
    let ground = |x: f32, y: f32| -> Option<Vec3> {
        // Reverse-Z: depth 0 is the far plane.
        let far = inverse.project_point3(Vec3::new(x, y, 0.0));
        let dir = far - view.eye;
        if dir.y.abs() < 1e-8 {
            return None;
        }
        Some(view.eye + dir * ((plane - view.eye.y) / dir.y))
    };
    let (Some(c), Some(r), Some(d)) = (
        ground(0.0, 0.0),
        ground(2.0 / w, 0.0),
        ground(0.0, -2.0 / h),
    ) else {
        return [0.0, 0.0];
    };
    let (right, down) = (r - c, d - c);
    [-(dx * right.x + dy * down.x), -(dx * right.z + dy * down.z)]
}

#[cfg(test)]
mod tests {
    use super::*;
    use canopy_render::camera::FlatGround;

    /// The pixel of a ground point.
    fn pixel(view: &CameraView, size: [f32; 2], p: Vec3) -> [f32; 2] {
        let ndc = view.view_projection.project_point3(p);
        [(ndc.x + 1.0) / 2.0 * size[0], (1.0 - ndc.y) / 2.0 * size[1]]
    }

    #[test]
    fn a_middle_drag_carries_the_ground_under_the_screen_centre() {
        let size = [1600.0, 900.0];
        for yaw in [0.0f32, 0.7] {
            let mut camera = RtsCamera::new([200.0, 200.0], None);
            camera.yaw = yaw;
            let before = camera.view(size[0] / size[1], &FlatGround);
            let centre = before.target;
            let (dx, dy) = (12.0, -7.0);
            let [mx, mz] = grab_pan(&before, size, dx, dy);
            camera.focus[0] += mx;
            camera.focus[1] += mz;
            let after = camera.view(size[0] / size[1], &FlatGround);
            let [x, y] = pixel(&after, size, centre);
            assert!((x - (800.0 + dx)).abs() < 0.05, "{x}");
            assert!((y - (450.0 + dy)).abs() < 0.05, "{y}");
        }
        // The same drag moves the focus less than the old uniform far-edge scale did.
        let camera = RtsCamera::new([200.0, 200.0], None);
        let view = camera.view(size[0] / size[1], &FlatGround);
        let [x0, x1, ..] = camera.footprint(size[0] / size[1], 0.0).unwrap();
        let far_edge = (x1 - x0) / size[0];
        let [mx, _] = grab_pan(&view, size, 1.0, 0.0);
        assert!(mx < 0.0 && -mx < far_edge);
    }
}
