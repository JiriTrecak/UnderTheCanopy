//! Cross-field rules of the effect format.

use super::{Pulse, Shape, VisualEffect, VisualLayer};
use crate::error::Error;
use std::collections::BTreeSet;

/// Checks every effect; reports each problem once, attributed to the effect or layer.
/// `files` maps effect ids to the file that authored them; errors name it.
pub(crate) fn check(
    effects: &[VisualEffect],
    files: &std::collections::BTreeMap<String, String>,
    errors: &mut Vec<Error>,
) {
    let mut ids = BTreeSet::new();
    for effect in effects {
        let at = effect.id.as_str();
        let file = files.get(at).map_or("effects", String::as_str);
        let mut fail = |location: &str, message: &str| {
            errors.push(Error::new(file, location, message));
        };
        if !ids.insert(at) {
            fail(at, "Duplicate effect ID");
        }
        if !at.starts_with("effect.") {
            fail(at, "Effect IDs must start with effect.");
        }
        if effect.duration_ticks == 0 {
            fail(at, "Effect duration must be positive");
        }
        let mut layer_ids = BTreeSet::new();
        for layer in &effect.layers {
            if !layer_ids.insert(layer.id.as_str()) {
                fail(at, "Layer IDs must be unique");
            }
            let location = format!("{at}/layers/{}", layer.id);
            for problem in layer_problems(layer) {
                fail(&location, problem);
            }
        }
    }
}

/// The rules one layer breaks.
pub(crate) fn layer_problems(layer: &VisualLayer) -> Vec<&'static str> {
    use Shape::*;
    let shape = layer.shape;
    let is = |shapes: &[Shape]| shapes.contains(&shape);
    let mut problems = Vec::new();
    let mut require = |ok: bool, message: &'static str| {
        if !ok {
            problems.push(message);
        }
    };
    require(
        layer.trail.is_none() || is(&[Missile, Wavefront]),
        "Trails require a missile or wavefront delivery layer",
    );
    require(
        layer.flipbook.is_none() || layer.texture.is_some(),
        "Flipbooks require a texture",
    );
    require(
        if shape == Mesh {
            layer.model.is_some()
        } else {
            layer.model.is_none() || shape == Missile
        },
        "Models require mesh or missile layers; mesh layers require a model",
    );
    require(
        layer.model.is_none() || layer.texture.is_none(),
        "Model layers use their own textures",
    );
    require(
        layer.arc_height.is_none() || shape == Missile,
        "Arc height requires a missile",
    );
    require(
        layer.attachment.is_none() || !is(&[Rain, Missile, Beam, Wavefront]),
        "Attachments apply to actor-local layers; travel and rain layers use their delivery path",
    );
    require(
        (shape == Sound) == layer.sound.is_some(),
        "Sound layers require audio settings; other shapes do not",
    );
    require(
        (shape == Particles) == layer.emitter.is_some(),
        "Particle layers require emitter settings; other shapes do not",
    );
    require(
        layer.motion.is_none() || is(&[Ring, Glow, Billboard, Light, Mesh]),
        "Transform motion requires a layer shape",
    );
    let fall = [
        layer.fall_speed,
        layer.launch_delay_ms,
        layer.fall_angle_degrees,
        layer.fall_azimuth_degrees,
    ];
    require(
        shape == Rain || (fall.iter().all(Option::is_none) && layer.impact.is_none()),
        "Fall and impact settings require rain",
    );
    require(
        layer.texture.is_none()
            || is(&[
                Burst, Billboard, Streaks, Rain, Missile, Wavefront, Particles,
            ]),
        "This layer does not accept textures",
    );
    require(
        !is(&[Billboard, Streaks, Rain]) || layer.texture.is_some(),
        "This layer requires a texture",
    );
    if let Some(sound) = &layer.sound {
        require(
            sound.max_distance > sound.reference_distance,
            "Maximum distance must exceed reference distance",
        );
        require(sound.index >= 1, "Resource index must be positive");
    }
    if let Some(flipbook) = &layer.flipbook {
        require(
            flipbook.columns >= 1
                && flipbook.rows >= 1
                && flipbook.frames >= 1
                && flipbook.frame_ticks >= 1,
            "Flipbook grid, frames and frame time must be positive",
        );
        require(
            u64::from(flipbook.first_frame) + u64::from(flipbook.frames)
                <= u64::from(flipbook.columns) * u64::from(flipbook.rows),
            "Flipbook frames exceed the atlas grid",
        );
    }
    if let Some(motion) = &layer.motion {
        let pulses = motion.scale.iter().chain(&motion.opacity);
        if pulses.clone().any(|p: &Pulse| p.max < p.min) {
            problems.push("Pulse maximum must be at least its minimum");
        }
        if motion.opacity.as_ref().is_some_and(|p| p.max > 1.0) {
            problems.push("Opacity cannot exceed one");
        }
        let periods = motion
            .rotation
            .iter()
            .map(|r| r.period_ticks)
            .chain(pulses.map(|p| p.period_ticks));
        if periods.into_iter().any(|t| t == 0) {
            problems.push("Motion periods must be positive");
        }
    }
    let ticks = [
        Some(layer.duration_ticks),
        layer.emitter.as_ref().map(|e| e.lifetime_ticks),
        layer.impact.as_ref().map(|i| i.duration_ticks),
        layer.trail.as_ref().map(|t| t.duration_ticks),
    ];
    if ticks.into_iter().flatten().any(|t| t == 0) {
        problems.push("Durations must be positive");
    }
    let counts = [
        Some(layer.count),
        layer.impact.as_ref().map(|i| i.count),
        layer.model.as_ref().map(|m| m.index),
        layer.texture.as_ref().map(|t| t.index),
    ];
    if counts.into_iter().flatten().any(|n| n == 0) {
        problems.push("Counts and resource indexes must be positive");
    }
    if layer.trail.as_ref().is_some_and(|t| t.max_points < 2) {
        problems.push("Trails need at least two points");
    }
    if layer.emitter.as_ref().is_some_and(|e| e.rate < 1.0) {
        problems.push("Emitter rate must be at least one");
    }
    let colours = [Some(&layer.colour), Some(&layer.accent)]
        .into_iter()
        .chain([
            layer.impact.as_ref().map(|i| &i.colour),
            layer.impact.as_ref().map(|i| &i.accent),
            layer.trail.as_ref().map(|t| &t.colour),
        ])
        .flatten();
    if colours.into_iter().any(|c| !is_colour(c)) {
        problems.push("Colours must be #rrggbb");
    }
    problems
}

/// `#rrggbb`, either case.
fn is_colour(text: &str) -> bool {
    text.len() == 7 && text.starts_with('#') && text[1..].bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Value, json};

    fn layer(extra: Value) -> VisualLayer {
        let mut source = json!({"id": "l", "shape": "glow", "colour": "#ffAA00", "accent": "#000000",
            "durationTicks": 10, "count": 1, "size": 1, "height": 0});
        for (key, value) in extra.as_object().unwrap() {
            source[key] = value.clone();
        }
        serde_json::from_value(source).unwrap()
    }

    #[test]
    fn layer_defaults() {
        let parsed = layer(json!({}));
        assert!(parsed.enabled && !parsed.sustain);
        assert_eq!(parsed.start_tick, 0);
        assert_eq!(parsed.anchor, super::super::LayerAnchor::Target);
        assert!(layer_problems(&parsed).is_empty());
    }

    #[test]
    fn shape_rules() {
        let cases = [
            (
                json!({"shape": "billboard"}),
                "This layer requires a texture",
            ),
            (
                json!({"shape": "mesh"}),
                "Models require mesh or missile layers; mesh layers require a model",
            ),
            (
                json!({"shape": "sound"}),
                "Sound layers require audio settings; other shapes do not",
            ),
            (json!({"arcHeight": 2}), "Arc height requires a missile"),
            (
                json!({"fallSpeed": 2}),
                "Fall and impact settings require rain",
            ),
            (json!({"colour": "red"}), "Colours must be #rrggbb"),
            (
                json!({"motion": {"opacity": {"periodTicks": 10, "min": 0, "max": 2, "easing": "sine"}}}),
                "Opacity cannot exceed one",
            ),
            (
                json!({"shape": "particles", "texture": {"asset": "asset.test.spark", "role": "image", "index": 1},
                    "flipbook": {"columns": 2, "rows": 2, "firstFrame": 2, "frames": 3, "frameTicks": 1, "mode": "loop", "randomStart": false},
                    "emitter": {"mode": "burst", "lifetimeTicks": 10, "rate": 5, "speed": 1, "speedVariation": 0, "direction": {"x": 0, "y": 1, "z": 0},
                        "coneDegrees": 30, "gravity": 0, "drag": 0, "spawnRadius": 0, "startSize": 1, "endSize": 0, "spin": 0, "fadeIn": 0, "fadeOut": 0}}),
                "Flipbook frames exceed the atlas grid",
            ),
        ];
        for (extra, expected) in cases {
            let problems = layer_problems(&layer(extra.clone()));
            assert_eq!(problems, [expected], "{extra}");
        }
    }

    #[test]
    fn library_rules() {
        let effect = |id: &str| -> VisualEffect {
            serde_json::from_value(json!({"id": id, "name": " Glow ", "durationTicks": 10,
                "layers": [{"id": "a", "shape": "glow", "colour": "#ffffff", "accent": "#ffffff", "durationTicks": 10, "count": 1, "size": 1, "height": 0},
                           {"id": "a", "shape": "glow", "colour": "#ffffff", "accent": "#ffffff", "durationTicks": 10, "count": 1, "size": 1, "height": 0}]}))
            .unwrap()
        };
        let first = effect("effect.test.glow");
        assert_eq!(first.name, "Glow");
        assert!(!first.looping);
        let mut errors = Vec::new();
        check(
            &[first, effect("effect.test.glow"), effect("spark.test")],
            &std::collections::BTreeMap::new(),
            &mut errors,
        );
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            [
                "Layer IDs must be unique",
                "Duplicate effect ID",
                "Layer IDs must be unique",
                "Effect IDs must start with effect.",
                "Layer IDs must be unique"
            ]
        );
    }
}
