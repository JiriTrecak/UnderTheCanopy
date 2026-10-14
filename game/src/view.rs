//! The spectator camera over a loaded map (no simulation yet).

use crate::options::CameraSpec;
use canopy_render::{
    RtsCamera,
    camera::{self, CameraView},
    terrain::TerrainDesc,
};
use canopy_scene::MapScene;

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
    let mut camera = RtsCamera::new(focus, Some(scene.size));
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
