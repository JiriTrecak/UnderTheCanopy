//! Positional sound: the listener and the old game's distance and panning model.
//!
//! Ground coordinates are cells (the simulation's x and y). The listener is the camera's
//! focus point on the ground, not the camera itself, so zooming does not change what is
//! heard; panning follows the camera's right vector.

/// Where the player hears from.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Listener {
    /// The camera's focus point on the ground, in cells.
    pub x: f32,
    pub y: f32,
    /// The camera's right vector on the ground plane (unit length).
    pub right_x: f32,
    pub right_y: f32,
}

impl Listener {
    /// A listener at `(x, y)` whose right is `(right_x, right_y)` (normalised here).
    pub fn new(x: f32, y: f32, right_x: f32, right_y: f32) -> Self {
        let length = (right_x * right_x + right_y * right_y).sqrt();
        let (right_x, right_y) = if length > 1e-6 {
            (right_x / length, right_y / length)
        } else {
            (1.0, 0.0)
        };
        Self {
            x,
            y,
            right_x,
            right_y,
        }
    }
}

impl Default for Listener {
    fn default() -> Self {
        Self::new(0.0, 0.0, 1.0, 0.0)
    }
}

/// How a positional sound fades with distance (an effect sound layer's
/// `referenceDistance` and `maxDistance`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Distances {
    /// Full volume up to this distance, in cells.
    pub reference: f32,
    /// Silent from this distance on; greater than `reference`.
    pub max: f32,
}

impl Distances {
    pub fn new(reference: f32, max: f32) -> Self {
        let reference = reference.max(0.01);
        Self {
            reference,
            max: max.max(reference + 0.01),
        }
    }
}

impl Default for Distances {
    /// The distances most shipped effect sounds use.
    fn default() -> Self {
        Self::new(12.0, 80.0)
    }
}

/// Where a sound plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum At {
    /// Not positional: interface sounds and music, centred at full volume.
    Listener,
    /// On the ground at a point in cells, heard with distance attenuation and panning.
    Ground { x: f32, y: f32 },
}

impl At {
    pub fn ground(x: f32, y: f32) -> Self {
        Self::Ground { x, y }
    }
}

/// What the listener hears of a sound: its gain from distance and its stereo position.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Heard {
    pub gain: f32,
    /// -1 is hard left, 1 hard right, with an equal-power law.
    pub pan: f32,
}

/// The old game's attenuation: full volume to the reference distance, then a quadratic
/// fall to silence at the maximum distance.
pub fn attenuation(distance: f32, distances: Distances) -> f32 {
    if distance <= distances.reference {
        return 1.0;
    }
    let linear =
        (1.0 - (distance - distances.reference) / (distances.max - distances.reference)).max(0.0);
    linear * linear
}

/// What `listener` hears of a sound at `at`, or `None` when it is out of range (at or
/// beyond the maximum distance; the old game stopped such voices). Distance is measured
/// on the ground. Panning is the offset along the camera's right vector divided by the
/// distance, but never by less than the reference distance, so sounds close to the focus
/// stay near the centre.
pub fn hear(listener: &Listener, at: At, distances: Distances) -> Option<Heard> {
    let At::Ground { x, y } = at else {
        return Some(Heard {
            gain: 1.0,
            pan: 0.0,
        });
    };
    let (dx, dy) = (x - listener.x, y - listener.y);
    let distance = (dx * dx + dy * dy).sqrt();
    if !distance.is_finite() || distance >= distances.max {
        return None;
    }
    let lateral = dx * listener.right_x + dy * listener.right_y;
    Some(Heard {
        gain: attenuation(distance, distances),
        pan: (lateral / distances.reference.max(distance)).clamp(-1.0, 1.0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const D: Distances = Distances {
        reference: 10.0,
        max: 50.0,
    };

    #[test]
    fn attenuation_is_flat_then_quadratic_to_silence() {
        assert_eq!(attenuation(0.0, D), 1.0);
        assert_eq!(attenuation(10.0, D), 1.0);
        // Half way between reference and max: (1 - 0.5)^2.
        assert!((attenuation(30.0, D) - 0.25).abs() < 1e-6);
        assert_eq!(attenuation(50.0, D), 0.0);
        assert_eq!(attenuation(500.0, D), 0.0);
        let mut previous = 1.0;
        for step in 0..100 {
            let gain = attenuation(step as f32, D);
            assert!(gain <= previous);
            previous = gain;
        }
    }

    #[test]
    fn panning_follows_the_camera_right_vector() {
        let listener = Listener::new(100.0, 100.0, 1.0, 0.0);
        let right = hear(&listener, At::ground(130.0, 100.0), D).unwrap();
        assert_eq!(right.pan, 1.0);
        let left = hear(&listener, At::ground(70.0, 100.0), D).unwrap();
        assert_eq!(left.pan, -1.0);
        // Straight ahead or behind: centred.
        assert_eq!(
            hear(&listener, At::ground(100.0, 140.0), D).unwrap().pan,
            0.0
        );
        // Close to the focus, the reference distance bounds the pan.
        let near = hear(&listener, At::ground(105.0, 100.0), D).unwrap();
        assert!((near.pan - 0.5).abs() < 1e-6);
        // A camera turned half way round swaps the sides.
        let turned = Listener::new(100.0, 100.0, -2.0, 0.0);
        assert_eq!(turned.right_x, -1.0);
        assert_eq!(
            hear(&turned, At::ground(130.0, 100.0), D).unwrap().pan,
            -1.0
        );
    }

    #[test]
    fn out_of_range_and_flat_sounds() {
        let listener = Listener::default();
        assert!(hear(&listener, At::ground(50.0, 0.0), D).is_none());
        assert!(hear(&listener, At::ground(f32::NAN, 0.0), D).is_none());
        assert_eq!(
            hear(&listener, At::Listener, D),
            Some(Heard {
                gain: 1.0,
                pan: 0.0
            })
        );
    }
}
