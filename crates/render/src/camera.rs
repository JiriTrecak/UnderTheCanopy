//! The RTS camera: the old game's play camera as a pure controller
//! (`src/render/camera/camera.ts` in the old repository).
//!
//! World axes: y up, +x east (screen right), +z south (screen down); one navigation cell
//! is one world unit and cell centres are integers.
//!
//! The rig looks at a focus point on the ground from `distance` away, pitched
//! [`GAME_PITCH`] below the horizon. Yaw 0 puts the eye due south of the focus, looking
//! north. The vertical field of view is 34.318° at 16:9 and widens on narrower windows so
//! the horizontal coverage stays the same. `zoom` is `distance / GAME_DISTANCE`: 1 is the
//! game's default, [`GAME_ZOOM`] the range a player may use.
//!
//! The controller only changes state through explicit calls (pan, zoom, look-at); every
//! frame [`RtsCamera::view`] derives the matrices, following the terrain like the old
//! game (no smoothing). Projection is reverse-Z: depth 1 at the near plane, 0 at the far.

use glam::{Mat4, Vec3, Vec4};

/// Pitch below the horizon, degrees.
pub const GAME_PITCH_DEGREES: f32 = 56.0;
/// The aspect the framing is authored for.
pub const GAME_ASPECT: f32 = 16.0 / 9.0;
/// World units across the focus plane at the default zoom and 16:9 (32 building cells).
pub const GAME_VIEW_WIDTH: f32 = 128.0;
/// Eye height kept above the ground under the eye, and above the water surface.
pub const MIN_TERRAIN_CLEARANCE: f32 = 4.0;
pub const MIN_WATER_CLEARANCE: f32 = 12.0;
/// Keyboard and edge-scroll panning speed, world units per second (independent of zoom).
pub const PAN_SPEED: f32 = 28.0;
/// Width of the screen border that scrolls, logical pixels.
pub const EDGE_SCROLL_PIXELS: f32 = 20.0;
/// Wheel zoom: log-distance per pixel of wheel travel, the largest pending change, and
/// the time constant (ms) the pending change is applied with.
pub const ZOOM_PER_PIXEL: f32 = 0.0004;
pub const ZOOM_PENDING_MAX: f32 = 0.3;
pub const ZOOM_TIME_CONSTANT_MS: f32 = 140.0;
/// Longest frame step the input integrates, ms.
pub const MAX_STEP_MS: f32 = 50.0;

/// Vertical field of view at 16:9, degrees: the old `70 / hypot(1, 16/9)`.
pub fn game_fov_degrees() -> f32 {
    70.0 / (1.0f32 + GAME_ASPECT * GAME_ASPECT).sqrt()
}

/// Eye-to-focus distance at zoom 1 (116.59 units).
pub fn game_distance() -> f32 {
    let half = (game_fov_degrees().to_radians() / 2.0).tan();
    GAME_VIEW_WIDTH / (2.0 * half * GAME_ASPECT)
}

/// The zoom range a player may use: 60 units close (0.5146) to 40 building cells across
/// (1.25).
pub fn game_zoom() -> (f32, f32) {
    (60.0 / game_distance(), 1.25)
}

/// Ground and water heights the camera follows, world units.
pub trait Ground {
    fn height(&self, x: f32, z: f32) -> f32;
    /// Water surface at a point, if wet.
    fn water(&self, x: f32, z: f32) -> Option<f32>;
}

/// Flat ground at 0 without water.
pub struct FlatGround;

impl Ground for FlatGround {
    fn height(&self, _: f32, _: f32) -> f32 {
        0.0
    }
    fn water(&self, _: f32, _: f32) -> Option<f32> {
        None
    }
}

/// Matrices and parameters of one view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CameraView {
    pub view: Mat4,
    pub projection: Mat4,
    pub view_projection: Mat4,
    pub eye: Vec3,
    pub target: Vec3,
    /// Vertical field of view, radians.
    pub fov_y: f32,
    pub aspect: f32,
    pub near: f32,
    pub far: f32,
}

impl CameraView {
    /// A view looking from `eye` at `target` with a reverse-Z perspective.
    pub fn look_at(eye: Vec3, target: Vec3, fov_y: f32, aspect: f32, near: f32, far: f32) -> Self {
        let view = Mat4::look_at_rh(eye, target, Vec3::Y);
        // Swapping near and far maps the near plane to depth 1 and the far plane to 0.
        let projection = Mat4::perspective_rh(fov_y, aspect, far, near);
        Self {
            view,
            projection,
            view_projection: projection * view,
            eye,
            target,
            fov_y,
            aspect,
            near,
            far,
        }
    }

    /// The six frustum planes as `(n, d)` with `n·p + d ≥ 0` inside; normals are
    /// normalised. Order: left, right, bottom, top, then depth 0 and depth 1 (far and near
    /// with reverse-Z).
    pub fn frustum(&self) -> [Vec4; 6] {
        frustum_planes(self.view_projection)
    }
}

/// Frustum planes of a reverse-Z (or standard 0..1) view-projection matrix, inside where
/// `plane · (p, 1) ≥ 0`.
pub fn frustum_planes(m: Mat4) -> [Vec4; 6] {
    let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
    // Clip space: -w ≤ x ≤ w, -w ≤ y ≤ w, 0 ≤ z ≤ w.
    [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2].map(|p| p / p.truncate().length())
}

/// The camera's state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RtsCamera {
    /// Focus on the ground plane (x, z).
    pub focus: [f32; 2],
    /// Eye-to-focus distance.
    pub distance: f32,
    /// Radians; 0 looks north. The game keeps it at 0.
    pub yaw: f32,
    /// Radians below the horizon.
    pub pitch: f32,
    /// Distance limits as zoom factors.
    pub zoom_range: (f32, f32),
    /// Playable cells per side: the view's ground footprint stays inside `[0, size − 1]`.
    pub map_size: Option<u32>,
    /// Wheel zoom still to apply, log-distance.
    pending_zoom: f32,
}

impl RtsCamera {
    /// The game's camera at zoom 1 looking at `focus`.
    pub fn new(focus: [f32; 2], map_size: Option<u32>) -> Self {
        Self {
            focus,
            distance: game_distance(),
            yaw: 0.0,
            pitch: GAME_PITCH_DEGREES.to_radians(),
            zoom_range: game_zoom(),
            map_size,
            pending_zoom: 0.0,
        }
    }

    pub fn zoom(&self) -> f32 {
        self.distance / game_distance()
    }

    /// Sets the zoom (clamped to the range).
    pub fn set_zoom(&mut self, zoom: f32) {
        let (min, max) = self.zoom_range;
        self.distance = zoom.clamp(min, max) * game_distance();
        self.pending_zoom = 0.0;
    }

    pub fn look_at(&mut self, x: f32, z: f32) {
        self.focus = [x, z];
    }

    /// Moves the focus along the screen axes: `right` east and `up` north at yaw 0.
    pub fn pan(&mut self, right: f32, up: f32) {
        let (s, c) = self.yaw.sin_cos();
        self.focus[0] += right * c - up * s;
        self.focus[1] += -right * s - up * c;
    }

    /// Keyboard or edge-scroll panning for `dt_ms`: `dir` is (right, up), normalised when
    /// diagonal.
    pub fn pan_for(&mut self, dir: [f32; 2], dt_ms: f32) {
        let length = (dir[0] * dir[0] + dir[1] * dir[1]).sqrt();
        if length == 0.0 {
            return;
        }
        let step = PAN_SPEED * dt_ms.min(MAX_STEP_MS) / 1000.0 / length.max(1.0);
        self.pan(dir[0] * step, dir[1] * step);
    }

    /// Adds wheel travel in pixels (positive zooms out). A reversal drops what is pending.
    pub fn wheel(&mut self, pixels: f32) {
        let change = pixels * ZOOM_PER_PIXEL;
        if change * self.pending_zoom < 0.0 {
            self.pending_zoom = 0.0;
        }
        self.pending_zoom = (self.pending_zoom + change).clamp(-ZOOM_PENDING_MAX, ZOOM_PENDING_MAX);
    }

    /// Applies part of the pending zoom for a frame of `dt_ms`.
    pub fn update(&mut self, dt_ms: f32) {
        if self.pending_zoom == 0.0 {
            return;
        }
        let change =
            self.pending_zoom * (1.0 - (-dt_ms.min(MAX_STEP_MS) / ZOOM_TIME_CONSTANT_MS).exp());
        self.pending_zoom -= change;
        let (min, max) = self.zoom_range;
        let wanted = self.distance * change.exp();
        self.distance = wanted.clamp(min * game_distance(), max * game_distance());
        if self.distance != wanted || self.pending_zoom.abs() < 1e-5 {
            self.pending_zoom = 0.0;
        }
    }

    /// Vertical field of view for a window of `aspect`, radians.
    pub fn fov_y(&self, aspect: f32) -> f32 {
        let half = (game_fov_degrees().to_radians() / 2.0).tan();
        let widened = 2.0 * (half * (GAME_ASPECT / aspect).max(1.0)).atan();
        let limit = (75.0f32)
            .min(self.pitch.to_degrees() * 2.0 - 10.0)
            .to_radians();
        widened.min(limit)
    }

    /// Eye offset from the focus.
    fn offset(&self) -> Vec3 {
        let (sy, cy) = self.yaw.sin_cos();
        let (sp, cp) = self.pitch.sin_cos();
        Vec3::new(sy * cp, sp, cy * cp) * self.distance
    }

    /// Keeps the view's ground footprint on the map: shrinks the distance when the
    /// footprint is larger than the map, then moves the focus (the old `clamp`).
    pub fn clamp(&mut self, aspect: f32, floor: f32) {
        let Some(size) = self.map_size else {
            return;
        };
        let bound = size as f32 - 1.0;
        for _ in 0..4 {
            let Some([min_x, max_x, min_z, max_z]) = self.footprint(aspect, floor) else {
                return;
            };
            let (w, h) = (max_x - min_x, max_z - min_z);
            if w > bound || h > bound {
                let fit = (bound / w).min(bound / h);
                self.distance *= fit * 0.999;
                continue;
            }
            let dx = if min_x < 0.0 {
                -min_x
            } else if max_x > bound {
                bound - max_x
            } else {
                0.0
            };
            let dz = if min_z < 0.0 {
                -min_z
            } else if max_z > bound {
                bound - max_z
            } else {
                0.0
            };
            if dx == 0.0 && dz == 0.0 {
                return;
            }
            self.focus[0] += dx;
            self.focus[1] += dz;
        }
    }

    /// Where the four corner rays of the view meet the plane `y = floor`, as
    /// `[min x, max x, min z, max z]`; `None` when a ray misses it.
    pub fn footprint(&self, aspect: f32, floor: f32) -> Option<[f32; 4]> {
        let target = Vec3::new(self.focus[0], floor, self.focus[1]);
        let view = CameraView::look_at(
            target + self.offset(),
            target,
            self.fov_y(aspect),
            aspect,
            1.0,
            1000.0,
        );
        let inverse = view.view_projection.inverse();
        let mut bounds = [f32::MAX, f32::MIN, f32::MAX, f32::MIN];
        for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
            let far = inverse.project_point3(Vec3::new(x, y, 0.0));
            let dir = far - view.eye;
            if dir.y >= -1e-6 {
                return None;
            }
            let t = (floor - view.eye.y) / dir.y;
            let p = view.eye + dir * t;
            bounds = [
                bounds[0].min(p.x),
                bounds[1].max(p.x),
                bounds[2].min(p.z),
                bounds[3].max(p.z),
            ];
        }
        Some(bounds)
    }

    /// The view for a window of `aspect`, following `ground` like the old game: the focus
    /// sits on the ground (never below 0), the eye keeps clear of the ground under it and
    /// of the water.
    pub fn view(&self, aspect: f32, ground: &dyn Ground) -> CameraView {
        let [tx, tz] = self.focus;
        let offset = self.offset();
        let mut target_y = ground.height(tx, tz).max(0.0);
        let eye_xz = (tx + offset.x, tz + offset.z);
        let under_eye = ground.height(eye_xz.0, eye_xz.1).max(0.0);
        target_y = target_y.max(under_eye + MIN_TERRAIN_CLEARANCE - offset.y);
        let mut target = Vec3::new(tx, target_y, tz);
        let mut eye = target + offset;
        if let Some(water) = ground.water(eye.x, eye.z) {
            let lift = (water + MIN_WATER_CLEARANCE - eye.y).max(0.0);
            target.y += lift;
            eye.y += lift;
        }
        CameraView::look_at(
            eye,
            target,
            self.fov_y(aspect),
            aspect,
            1.0,
            2.0 * self.distance + 32.0,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn close(a: f32, b: f32, eps: f32) -> bool {
        (a - b).abs() <= eps
    }

    #[test]
    fn framing_matches_the_old_constants() {
        assert!(close(game_fov_degrees(), 34.31829, 1e-4));
        assert!(close(game_distance(), 116.59146, 1e-3));
        let (min, max) = game_zoom();
        assert!(close(min, 0.51462, 1e-4));
        assert_eq!(max, 1.25);
        let camera = RtsCamera::new([100.0, 100.0], None);
        let view = camera.view(16.0 / 9.0, &FlatGround);
        // 96.66 up and 65.20 south of the focus.
        assert!(close(view.eye.y, 96.659, 1e-2));
        assert!(close(view.eye.z - 100.0, 65.197, 1e-2));
        assert!(close(view.far, 2.0 * game_distance() + 32.0, 1e-3));
        // 128 units across the focus plane.
        let [min_x, max_x, ..] = camera.footprint(16.0 / 9.0, 0.0).unwrap();
        assert!(max_x - min_x > 128.0);
    }

    #[test]
    fn narrow_windows_widen_the_vertical_field() {
        let camera = RtsCamera::new([0.0, 0.0], None);
        assert!(close(
            camera.fov_y(16.0 / 9.0).to_degrees(),
            game_fov_degrees(),
            1e-4
        ));
        assert!(camera.fov_y(4.0 / 3.0) > camera.fov_y(16.0 / 9.0));
        assert_eq!(camera.fov_y(3.0), camera.fov_y(16.0 / 9.0));
    }

    #[test]
    fn reverse_z_puts_the_near_plane_at_one() {
        let view =
            CameraView::look_at(Vec3::new(0.0, 10.0, 10.0), Vec3::ZERO, 1.0, 1.5, 1.0, 100.0);
        let dir = (view.target - view.eye).normalize();
        let near = view.view_projection.project_point3(view.eye + dir * 1.0);
        let far = view.view_projection.project_point3(view.eye + dir * 100.0);
        assert!(close(near.z, 1.0, 1e-4));
        assert!(close(far.z, 0.0, 1e-4));
        for plane in view.frustum() {
            assert!(plane.dot(view.target.extend(1.0)) > 0.0);
        }
        assert!(view.frustum()[5].dot((view.eye - dir * 2.0).extend(1.0)) < 0.0);
        assert!(view.frustum()[4].dot((view.eye + dir * 200.0).extend(1.0)) < 0.0);
    }

    #[test]
    fn the_footprint_stays_on_the_map() {
        let mut camera = RtsCamera::new([2.0, 2.0], Some(256));
        camera.clamp(16.0 / 9.0, 0.0);
        let [min_x, max_x, min_z, max_z] = camera.footprint(16.0 / 9.0, 0.0).unwrap();
        assert!(min_x >= -1e-3 && min_z >= -1e-3);
        assert!(max_x <= 255.001 && max_z <= 255.001);
        // A map smaller than the view shrinks the distance.
        let mut small = RtsCamera::new([30.0, 30.0], Some(64));
        small.clamp(16.0 / 9.0, 0.0);
        assert!(small.distance < game_distance());
        let [min_x, max_x, ..] = small.footprint(16.0 / 9.0, 0.0).unwrap();
        assert!(max_x - min_x <= 63.0);
    }

    struct Hill;
    impl Ground for Hill {
        fn height(&self, _: f32, z: f32) -> f32 {
            if z > 150.0 { 120.0 } else { 5.0 }
        }
        fn water(&self, _: f32, _: f32) -> Option<f32> {
            None
        }
    }

    #[test]
    fn the_rig_follows_the_ground() {
        let camera = RtsCamera::new([100.0, 100.0], None);
        let view = camera.view(16.0 / 9.0, &Hill);
        // The eye is over the hill (z ≈ 165): the rig rises to keep 4 units above it.
        assert!(close(view.eye.y, 124.0, 1e-3));
        assert!(close(view.eye.y - view.target.y, 96.659, 1e-2));
    }

    #[test]
    fn panning_and_zoom_follow_the_old_rates() {
        let mut camera = RtsCamera::new([100.0, 100.0], None);
        camera.pan_for([0.0, 1.0], 1000.0);
        // Steps are capped at 50 ms: 28 · 0.05 = 1.4 units north.
        assert!(close(camera.focus[1], 98.6, 1e-4));
        camera.pan_for([1.0, 1.0], 50.0);
        assert!(close(camera.focus[0], 100.0 + 1.4 / 2f32.sqrt(), 1e-4));
        // A 100 px notch zooms out by about 4% once fully applied.
        let before = camera.distance;
        camera.wheel(100.0);
        for _ in 0..200 {
            camera.update(16.0);
        }
        assert!(close(camera.distance / before, (0.04f32).exp(), 1e-3));
        // Zoom stops at the range.
        camera.wheel(10_000.0);
        for _ in 0..200 {
            camera.update(16.0);
        }
        assert!(close(camera.zoom(), 1.25, 1e-4));
        camera.set_zoom(0.1);
        assert!(close(camera.zoom(), game_zoom().0, 1e-5));
    }
}
