//! Turning (old `src/sim/game/facing.ts`). Rotation is world yaw in millidegrees, +Z
//! forward; the shortest turn wins with a stable clockwise tie.

use crate::math;

/// Degrees per tick at `turn_rate` degrees per second (40 Hz).
pub fn turn_step(turn_rate: u32) -> f64 {
    f64::from(turn_rate) * 25.0 / 1000.0
}

/// Heading in degrees from a point (cells) towards a target (cells); `current` when they
/// coincide.
pub fn heading(from: (f64, f64), target: (f64, f64), current: f64) -> f64 {
    let (dx, dy) = (target.0 - from.0, target.1 - from.1);
    if dx * dx + dy * dy < 1e-10 {
        current
    } else {
        (math::atan2(dx, dy) * 180.0 / std::f64::consts::PI + 360.0) % 360.0
    }
}

/// The signed shortest turn from `from` to `to`, in degrees (−180, 180].
pub fn turn_difference(from: f64, to: f64) -> f64 {
    let d = ((to - from) % 360.0 + 360.0) % 360.0;
    if d > 180.0 { d - 360.0 } else { d }
}

/// Turns `rotation` (millidegrees) towards `target` by at most `step` degrees. Returns
/// whether the unit now faces it closely enough to move.
pub fn turn_toward(rotation: &mut i32, from: (f64, f64), target: (f64, f64), step: f64) -> bool {
    let current = f64::from(*rotation) / 1000.0;
    let desired = heading(from, target, current);
    let difference = turn_difference(current, desired);
    let turned = ((current + difference.clamp(-step, step) + 360.0) % 360.0) * 1000.0;
    *rotation = crate::terrain::round_half_up(turned) as i32;
    difference.abs() <= step + 1.0
}

/// Whether a unit at `from` with `rotation` faces `target` within a degree.
pub fn facing(rotation: i32, from: (f64, f64), target: (f64, f64)) -> bool {
    let current = f64::from(rotation) / 1000.0;
    turn_difference(current, heading(from, target, current)).abs() <= 1.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn turns_the_short_way_at_the_turn_rate() {
        // Facing +Z (0°), target to the east (+X): 90°, turned 18° per tick at 720°/s.
        let mut rotation = 0;
        assert!(!turn_toward(
            &mut rotation,
            (0.0, 0.0),
            (5.0, 0.0),
            turn_step(720)
        ));
        assert_eq!(rotation, 18_000);
        for _ in 0..3 {
            turn_toward(&mut rotation, (0.0, 0.0), (5.0, 0.0), 18.0);
        }
        assert!(turn_toward(&mut rotation, (0.0, 0.0), (5.0, 0.0), 18.0));
        assert_eq!(rotation, 90_000);
        // West from 0° turns counter-clockwise.
        let mut rotation = 0;
        turn_toward(&mut rotation, (0.0, 0.0), (-5.0, 0.0), 18.0);
        assert_eq!(rotation, 342_000);
        assert!(facing(90_000, (0.0, 0.0), (5.0, 0.05)));
        assert!(!facing(90_000, (0.0, 0.0), (5.0, 0.2)));
    }
}
