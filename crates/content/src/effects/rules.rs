//! Cross-field rules of the effect format.

use super::{ColourKey, CurveKey, Pulse, Shape, VisualEffect, VisualLayer};
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
            if let Some(rides) = &layer.rides {
                let ridden = effect.layers.iter().find(|l| &l.id == rides);
                if !ridden.is_some_and(|l| matches!(l.shape, Shape::Missile | Shape::Rain)) {
                    fail(
                        &location,
                        "Rides must name a missile or rain layer of the same effect",
                    );
                }
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
            layer.model.is_none() || is(&[Missile, Rain])
        },
        "Models require mesh, missile or rain layers; mesh layers require a model",
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
                Burst, Billboard, Streaks, Rain, Missile, Wavefront, Particles, Ribbon, Beam,
            ]),
        "This layer does not accept textures",
    );
    require(
        (shape == Ribbon) == layer.ribbon.is_some(),
        "Ribbon layers require ribbon settings; other shapes do not",
    );
    require(
        layer.rides.is_none() || is(&[Particles, Ribbon]),
        "Only particle and ribbon layers ride other layers",
    );
    require(
        layer.ramp.is_none() || layer.texture.is_some(),
        "Ramps colour a texture",
    );
    require(
        layer
            .forks
            .is_none_or(|f| shape == Beam && layer.texture.is_some() && f <= 8),
        "Forks belong to textured beams, at most eight",
    );
    require(
        layer.soft.is_none_or(|s| (0.0..=10.0).contains(&s)),
        "Softness must be between zero and ten metres",
    );
    require(
        (layer.opacity_over_life.is_none() && layer.scale_over_life.is_none())
            || !is(&[Sound, Particles, Ribbon]),
        "Particles and ribbons take their curves from their emitter or ribbon settings",
    );
    let unit =
        |keys: &Option<Vec<CurveKey>>| keys.as_ref().is_none_or(|k| values_within(k, 0.0, 1.0));
    let colour_keys = [
        layer.ramp.as_ref(),
        layer
            .emitter
            .as_ref()
            .and_then(|e| e.colour_over_life.as_ref()),
        layer
            .ribbon
            .as_ref()
            .and_then(|r| r.colour_over_life.as_ref()),
    ];
    require(
        colour_keys.iter().flatten().all(|k| colour_keys_ok(k)),
        "Colour keys need one to eight increasing times in 0–1 and #rrggbb colours",
    );
    let mut curves: Vec<&Option<Vec<CurveKey>>> =
        vec![&layer.opacity_over_life, &layer.scale_over_life];
    if let Some(e) = &layer.emitter {
        curves.extend([
            &e.opacity_over_life,
            &e.size_over_life,
            &e.additive_over_life,
            &e.erosion_over_life,
            &e.heat_over_life,
        ]);
    }
    if let Some(r) = &layer.ribbon {
        curves.extend([
            &r.opacity_over_life,
            &r.width_over_life,
            &r.additive_over_life,
        ]);
    }
    require(
        curves.iter().all(|c| c.as_ref().is_none_or(|k| keys_ok(k))),
        "Curves need one to eight keys at increasing times in 0–1",
    );
    let unit_curves = [
        unit(&layer.opacity_over_life),
        layer.emitter.as_ref().is_none_or(|e| {
            unit(&e.opacity_over_life) && unit(&e.additive_over_life) && unit(&e.erosion_over_life)
        }),
        layer
            .ribbon
            .as_ref()
            .is_none_or(|r| unit(&r.opacity_over_life) && unit(&r.additive_over_life)),
    ];
    require(
        unit_curves.iter().all(|ok| *ok),
        "Opacity, additive and erosion curves stay between zero and one",
    );
    if let Some(e) = &layer.emitter {
        let fraction = |v: Option<f64>| v.is_none_or(|v| (0.0..=1.0).contains(&v));
        require(
            fraction(e.inherit_velocity)
                && fraction(e.spawn_inner)
                && fraction(e.erosion_softness)
                && fraction(e.collide.map(|c| c.friction)),
            "Inherited velocity, inner spawn, erosion softness and friction are between zero and one",
        );
        require(
            e.cone_inner_degrees
                .is_none_or(|c| c >= 0.0 && c <= e.cone_degrees),
            "The inner cone must lie within the cone",
        );
        require(
            e.turbulence
                .is_none_or(|t| t.strength >= 0.0 && t.scale > 0.0 && t.speed >= 0.0),
            "Turbulence needs a positive scale and no negative strength or speed",
        );
        require(
            e.size_over_life
                .as_ref()
                .is_none_or(|k| k.iter().all(|k| k.value >= 0.0)),
            "Sizes cannot be negative",
        );
    }
    if let Some(r) = &layer.ribbon {
        require(
            r.width > 0.0 && r.tile.is_none_or(|t| t > 0.0),
            "Ribbon width and tile must be positive",
        );
    }
    require(
        !is(&[Billboard, Streaks]) || layer.texture.is_some(),
        "This layer requires a texture",
    );
    require(
        shape != Rain || layer.texture.is_some() || layer.model.is_some(),
        "Rain requires a texture or a model",
    );
    require(
        layer.core.is_none_or(|c| (0.0..=1.0).contains(&c)),
        "Core must be between zero and one",
    );
    require(
        layer
            .emitter
            .as_ref()
            .and_then(|e| e.stretch)
            .is_none_or(|s| (0.0..=1.0).contains(&s)),
        "Stretch must be between zero and one second",
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
        layer.ribbon.as_ref().map(|r| r.duration_ticks),
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

/// One to eight keys at increasing times within 0–1.
fn keys_ok(keys: &[CurveKey]) -> bool {
    times_ok(keys.iter().map(|k| k.at))
}

fn colour_keys_ok(keys: &[ColourKey]) -> bool {
    times_ok(keys.iter().map(|k| k.at)) && keys.iter().all(|k| is_colour(&k.colour))
}

fn times_ok(times: impl ExactSizeIterator<Item = f64>) -> bool {
    let n = times.len();
    let mut last = -1.0;
    (1..=8).contains(&n)
        && times.into_iter().all(|t| {
            let ok = (0.0..=1.0).contains(&t) && t > last;
            last = t;
            ok
        })
}

fn values_within(keys: &[CurveKey], low: f64, high: f64) -> bool {
    keys.iter().all(|k| (low..=high).contains(&k.value))
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
                "Models require mesh, missile or rain layers; mesh layers require a model",
            ),
            (
                json!({"shape": "rain"}),
                "Rain requires a texture or a model",
            ),
            (json!({"core": 1.5}), "Core must be between zero and one"),
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
                json!({"forks": 2}),
                "Forks belong to textured beams, at most eight",
            ),
            (
                json!({"shape": "ribbon"}),
                "Ribbon layers require ribbon settings; other shapes do not",
            ),
            (
                json!({"rides": "drop"}),
                "Only particle and ribbon layers ride other layers",
            ),
            (
                json!({"ramp": [{"at": 0, "colour": "#000000"}]}),
                "Ramps colour a texture",
            ),
            (
                json!({"opacityOverLife": [{"at": 0.5, "value": 1}, {"at": 0.2, "value": 0}]}),
                "Curves need one to eight keys at increasing times in 0–1",
            ),
            (
                json!({"opacityOverLife": [{"at": 0, "value": 2}]}),
                "Opacity, additive and erosion curves stay between zero and one",
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
    fn riders_need_a_travelling_layer() {
        let effect: VisualEffect = serde_json::from_value(json!({"id": "effect.test.fall", "name": "Fall",
            "durationTicks": 10, "layers": [
                {"id": "glow", "shape": "glow", "colour": "#ffffff", "accent": "#ffffff", "durationTicks": 10, "count": 1, "size": 1, "height": 0},
                {"id": "fire", "shape": "ribbon", "rides": "glow", "colour": "#ffffff", "accent": "#ffffff", "durationTicks": 10,
                 "count": 1, "size": 1, "height": 0, "ribbon": {"durationTicks": 8, "width": 1}}]}))
        .unwrap();
        let mut errors = Vec::new();
        check(&[effect], &std::collections::BTreeMap::new(), &mut errors);
        let messages: Vec<_> = errors.iter().map(|e| e.message.as_str()).collect();
        assert_eq!(
            messages,
            ["Rides must name a missile or rain layer of the same effect"]
        );
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
