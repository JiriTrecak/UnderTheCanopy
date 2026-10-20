//! Authored visual effects (`content/effects/published.json`, unchanged) converted to the
//! runtime effect table (`@effects`, [`canopy_assets::EffectTable`]): v1 layer shapes
//! become primitives and motions, ticks become seconds, colours linear RGB, degrees
//! radians, and texture and model references pack entry names (D24, Q11).
//!
//! Budgets of the old effect player are checked here, where a problem can be fixed in
//! the editor: effect image files at most 8 MB and 2048 px a side, 64 MB resident for all
//! of them together (`EFFECT_IMAGE_LIMITS`). Model triangles (`EFFECT_MODEL_LIMITS`) are checked by the
//! runtime from the cooked models.

use canopy_assets::{
    ColourCurve, CookedAttachment, CookedCueMotion, CookedEffect, CookedEmitter, CookedFlipbook,
    CookedImpact, CookedModel, CookedPulse, CookedRain, CookedRibbon, CookedTrail, Curve, Easing,
    EffectBlend, EffectLayer, EffectTable, FlipbookPlay, Motion, Primitive,
};
use canopy_content::{
    assets::Manifest,
    effects::{self as fx, Blend, Orientation, Shape, VisualEffect, VisualLayer},
    game::AUTHORED_TICK_MS,
};

/// Largest side of an effect image (old `EFFECT_IMAGE_LIMITS.maxDimension`).
pub const MAX_IMAGE_SIDE: u32 = 2048;
/// Largest effect image with its mips at 4 bytes a texel (old `maxBytes`).
pub const MAX_IMAGE_BYTES: u64 = 8 * 1024 * 1024;

/// Seconds of a duration in authored 25 ms ticks.
fn seconds(ticks: u32) -> f32 {
    (f64::from(ticks) * f64::from(AUTHORED_TICK_MS) / 1000.0) as f32
}

/// `#rrggbb` as linear RGB (white when malformed; the content reader checks the format).
pub fn linear(hex: &str) -> [f32; 3] {
    let digits = hex.trim_start_matches('#');
    std::array::from_fn(|i| {
        let c = digits
            .get(i * 2..i * 2 + 2)
            .and_then(|h| u8::from_str_radix(h, 16).ok())
            .unwrap_or(255);
        let c = f64::from(c) / 255.0;
        (if c <= 0.04045 {
            c / 12.92
        } else {
            libm::pow((c + 0.055) / 1.055, 2.4)
        }) as f32
    })
}

/// The output of `asset` with `role` and 1-based `index` (`<role>[_<index>].<ext>`).
pub fn output(manifest: &Manifest, asset: &str, role: &str, index: u32) -> Option<String> {
    let record = manifest.records.iter().find(|r| r.id.as_str() == asset)?;
    record
        .outputs
        .iter()
        .map(|o| o.path.as_str())
        .find(|path| {
            let file = path.rsplit('/').next().unwrap_or(path);
            let stem = file.split('.').next().unwrap_or(file);
            match index {
                1 => stem == role || stem == format!("{role}_1"),
                n => stem == format!("{role}_{n}"),
            }
        })
        .map(str::to_owned)
}

fn curve(keys: &Option<Vec<fx::CurveKey>>) -> Option<Curve> {
    keys.as_ref()
        .map(|k| k.iter().map(|k| (k.at as f32, k.value as f32)).collect())
}

fn colours(keys: &Option<Vec<fx::ColourKey>>) -> Option<ColourCurve> {
    keys.as_ref()
        .map(|k| k.iter().map(|k| (k.at as f32, linear(&k.colour))).collect())
}

/// The converted table, the image entries it samples and problems found.
pub struct Converted {
    pub table: EffectTable,
    pub images: Vec<String>,
    pub warnings: Vec<String>,
}

fn pulse(p: &fx::Pulse) -> CookedPulse {
    CookedPulse {
        period: seconds(p.period_ticks.max(1)),
        min: p.min as f32,
        max: p.max as f32,
        easing: match p.easing {
            fx::Easing::Sine => Easing::Sine,
            fx::Easing::Smoothstep => Easing::Smoothstep,
            fx::Easing::Bounce => Easing::Bounce,
        },
        phase: p.phase.unwrap_or(0.0) as f32,
    }
}

/// The primitive and motion of an authored shape (old `EffectPlayer.makeParticle`: any
/// textured layer facing the ground is a flat quad, untextured rings and glows lie flat
/// unless turned to the camera, other untextured parts are soft spheres).
pub fn classify(layer: &VisualLayer) -> (Primitive, Motion) {
    let textured = layer.texture.is_some();
    let ground = layer.orientation == Some(Orientation::Ground);
    let camera = layer.orientation == Some(Orientation::Camera);
    let flat = |untextured_flat: bool| {
        if (textured && ground) || (!textured && untextured_flat) {
            Primitive::Decal
        } else {
            Primitive::Particle
        }
    };
    match layer.shape {
        Shape::Billboard => (flat(false), Motion::Billboard),
        Shape::Burst => (flat(false), Motion::Burst),
        Shape::Streaks => (flat(false), Motion::Streaks),
        Shape::Particles => (flat(false), Motion::Emitter),
        // Untextured rings and glows lie flat unless explicitly turned to the camera (a
        // glow in the air reads as a soft orb, not a floating disc).
        Shape::Ring => (flat(!camera), Motion::Ring),
        Shape::Glow => (flat(!camera), Motion::Glow),
        Shape::Pillar => (Primitive::Particle, Motion::Pillar),
        Shape::Missile if layer.model.is_some() => (Primitive::Mesh, Motion::Missile),
        Shape::Missile => (flat(false), Motion::Missile),
        Shape::Wavefront => (flat(false), Motion::Wavefront),
        Shape::Mesh => (Primitive::Mesh, Motion::Model),
        Shape::Light => (Primitive::Light, Motion::Light),
        Shape::Sound => (Primitive::Sound, Motion::Sound),
        Shape::Beam => (Primitive::Particle, Motion::Beam),
        Shape::Rain => (Primitive::Particle, Motion::Rain),
        Shape::Ribbon => (Primitive::Particle, Motion::Ribbon),
    }
}

fn layer(
    effect: &VisualEffect,
    l: &VisualLayer,
    manifest: &Manifest,
    images: &mut Vec<String>,
    warnings: &mut Vec<String>,
) -> EffectLayer {
    let at = format!("{}/{}", effect.id.as_str(), l.id);
    let (primitive, motion) = classify(l);
    let texture = l.texture.as_ref().and_then(|t| {
        let found = output(manifest, t.asset.as_str(), "image", t.index);
        if found.is_none() {
            warnings.push(format!(
                "{at}: no published image {}/image:{} (drawn untextured)",
                t.asset.as_str(),
                t.index
            ));
        }
        found
    });
    if let Some(t) = &texture
        && !images.contains(t)
    {
        images.push(t.clone());
    }
    let model = l.model.as_ref().and_then(|m| {
        let found = output(manifest, m.asset.as_str(), "geometry", m.index);
        if found.is_none() {
            warnings.push(format!(
                "{at}: no published model {}/geometry:{}",
                m.asset.as_str(),
                m.index
            ));
        }
        found.map(|model| CookedModel {
            model,
            rotation: [
                (m.rotation.x as f32).to_radians(),
                (m.rotation.y as f32).to_radians(),
                (m.rotation.z as f32).to_radians(),
            ],
            animation: m
                .animation
                .as_ref()
                .map(|a| (a.clip.clone(), a.speed as f32, a.looping)),
        })
    });
    let v3 = |v: &fx::Vec3| [v.x as f32, v.y as f32, v.z as f32];
    EffectLayer {
        id: l.id.clone(),
        primitive,
        motion,
        start: seconds(l.start_tick),
        duration: seconds(l.duration_ticks.max(1)),
        sustain: l.sustain,
        at_source: l.anchor == fx::LayerAnchor::Source,
        follow: l.follow.unwrap_or(false),
        offset: l.offset.as_ref().map_or([0.0; 3], v3),
        color: linear(&l.colour),
        accent: linear(&l.accent),
        count: l.count,
        size: l.size as f32,
        height: l.height as f32,
        intensity: l.intensity.map(|v| v as f32),
        core: l.core.unwrap_or(0.0).clamp(0.0, 1.0) as f32,
        spread: l.spread.map(|v| v as f32),
        length: l.length.map(|v| v as f32),
        arc_height: l.arc_height.unwrap_or(0.0) as f32,
        blend: match l.blend {
            Some(Blend::Normal) => EffectBlend::Normal,
            _ => EffectBlend::Additive,
        },
        ground: l.orientation == Some(Orientation::Ground),
        disc: l.distribution == Some(fx::Distribution::Disc),
        flipbook: l
            .flipbook
            .as_ref()
            .filter(|_| texture.is_some())
            .map(|f| CookedFlipbook {
                columns: f.columns.max(1),
                rows: f.rows.max(1),
                first: f.first_frame,
                frames: f.frames.max(1),
                frame_time: seconds(f.frame_ticks.max(1)),
                play: match f.mode {
                    fx::FlipbookMode::Once => FlipbookPlay::Once,
                    fx::FlipbookMode::Loop => FlipbookPlay::Loop,
                    fx::FlipbookMode::PingPong => FlipbookPlay::PingPong,
                },
                random_start: f.random_start,
                over_life: f.over_life.unwrap_or(false),
                blend: f.blend.unwrap_or(false),
            }),
        texture,
        model,
        attachment: l.attachment.as_ref().map(|a| CookedAttachment {
            socket: a.socket.clone(),
            hide: a.fallback == fx::AttachmentFallback::Hide,
            inherit_rotation: a.inherit_rotation,
        }),
        emitter: l.emitter.as_ref().map(|e| {
            let d = [e.direction.x, e.direction.y, e.direction.z];
            let n = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
            CookedEmitter {
                burst: e.mode == fx::EmitterMode::Burst,
                lifetime: seconds(e.lifetime_ticks.max(1)),
                rate: e.rate as f32,
                speed: e.speed as f32,
                speed_variation: e.speed_variation as f32,
                direction: if n > 0.0 {
                    [(d[0] / n) as f32, (d[1] / n) as f32, (d[2] / n) as f32]
                } else {
                    [0.0, 1.0, 0.0]
                },
                cone: (e.cone_degrees as f32).to_radians(),
                gravity: e.gravity as f32,
                drag: e.drag as f32,
                spawn_radius: e.spawn_radius as f32,
                start_size: e.start_size as f32,
                end_size: e.end_size as f32,
                spin: e.spin as f32,
                fade_in: e.fade_in as f32,
                fade_out: e.fade_out as f32,
                stretch: e.stretch.unwrap_or(0.0).max(0.0) as f32,
                world: e.space == Some(fx::EmitterSpace::World),
                inherit: e.inherit_velocity.unwrap_or(0.0) as f32,
                cone_inner: (e.cone_inner_degrees.unwrap_or(0.0) as f32).to_radians(),
                spawn_inner: e.spawn_inner.unwrap_or(0.0) as f32,
                colour: colours(&e.colour_over_life),
                opacity: curve(&e.opacity_over_life),
                size: curve(&e.size_over_life),
                additive: curve(&e.additive_over_life),
                erosion: curve(&e.erosion_over_life),
                erosion_softness: e.erosion_softness.unwrap_or(0.1) as f32,
                heat: curve(&e.heat_over_life),
                turbulence: e
                    .turbulence
                    .map(|t| [t.strength as f32, t.scale as f32, t.speed as f32]),
                collide: e.collide.map(|c| c.friction as f32),
                orbit: (e.orbit.unwrap_or(0.0) as f32).to_radians(),
                pull: e.pull.unwrap_or(0.0) as f32,
            }
        }),
        cue_motion: l.motion.as_ref().map(|m| CookedCueMotion {
            rotation: m.rotation.as_ref().map(|r| {
                (
                    seconds(r.period_ticks.max(1)),
                    match r.direction {
                        fx::RotationDirection::Clockwise => -1.0,
                        fx::RotationDirection::Counterclockwise => 1.0,
                    },
                    (r.phase_degrees.unwrap_or(0.0) as f32).to_radians(),
                )
            }),
            scale: m.scale.as_ref().map(pulse),
            opacity: m.opacity.as_ref().map(pulse),
        }),
        trail: l.trail.as_ref().map(|t| CookedTrail {
            duration: seconds(t.duration_ticks.max(1)),
            max_points: t.max_points.max(2),
            width: t.width as f32,
            color: linear(&t.colour),
            opacity: t.opacity as f32,
            break_distance: t.break_distance as f32,
        }),
        rain: (l.shape == Shape::Rain).then(|| rain(l)),
        rides: l.rides.as_ref().and_then(|id| {
            let found = effect
                .layers
                .iter()
                .filter(|x| x.enabled)
                .position(|x| &x.id == id);
            if found.is_none() {
                warnings.push(format!("{at}: rides {id}, which is disabled"));
            }
            found.map(|i| i as u16)
        }),
        ribbon: l.ribbon.as_ref().map(|r| CookedRibbon {
            duration: seconds(r.duration_ticks.max(1)),
            width: r.width as f32,
            tile: r.tile.unwrap_or(0.0) as f32,
            scroll: r.scroll.unwrap_or(0.0) as f32,
            colour: colours(&r.colour_over_life),
            opacity: curve(&r.opacity_over_life),
            width_curve: curve(&r.width_over_life),
            additive: curve(&r.additive_over_life),
        }),
        ramp: colours(&l.ramp),
        opacity_curve: curve(&l.opacity_over_life),
        scale_curve: curve(&l.scale_over_life),
        soft: l.soft.map(|s| s as f32),
        forks: l.forks.unwrap_or(0),
    }
}

/// How a rain layer falls (old `fallDirection`: the fall leans `fallAngleDegrees` from
/// vertical towards `fallAzimuthDegrees`).
fn rain(l: &VisualLayer) -> CookedRain {
    let angle = l
        .fall_angle_degrees
        .unwrap_or(0.0)
        .clamp(0.0, 89.0)
        .to_radians();
    let azimuth = l.fall_azimuth_degrees.unwrap_or(0.0).to_radians();
    let back = [
        azimuth.cos() * angle.tan(),
        1.0,
        azimuth.sin() * angle.tan(),
    ];
    let n = (back[0] * back[0] + back[1] * back[1] + back[2] * back[2]).sqrt();
    CookedRain {
        speed: l.fall_speed.unwrap_or(1.0).max(0.05) as f32,
        launch_delay: (l.launch_delay_ms.unwrap_or(0.0).max(0.0) / 1000.0) as f32,
        back: back.map(|v| (v / n) as f32),
        impact: l.impact.as_ref().map(|i| CookedImpact {
            duration: seconds(i.duration_ticks.max(1)),
            count: i.count,
            size: i.size as f32,
            spread: i.spread as f32,
            height: i.height as f32,
            color: linear(&i.colour),
            accent: linear(&i.accent),
        }),
    }
}

/// Converts every effect (enabled layers only), sorted by id.
pub fn convert(effects: &[VisualEffect], manifest: &Manifest) -> Converted {
    let mut images = Vec::new();
    let mut warnings = Vec::new();
    let mut out: Vec<CookedEffect> = effects
        .iter()
        .map(|effect| CookedEffect {
            id: effect.id.to_string(),
            duration: seconds(effect.duration_ticks.max(1)),
            looping: effect.looping,
            layers: effect
                .layers
                .iter()
                .filter(|l| l.enabled)
                .map(|l| layer(effect, l, manifest, &mut images, &mut warnings))
                .collect(),
        })
        .collect();
    out.sort_by(|a, b| a.id.cmp(&b.id));
    images.sort();
    Converted {
        table: EffectTable { effects: out },
        images,
        warnings,
    }
}

/// Resident bytes of an effect image as the old player counted them (RGBA8 with mips,
/// `w · h · 4 · 4/3`).
pub fn resident_bytes(width: u32, height: u32) -> u64 {
    (u64::from(width) * u64::from(height) * 16).div_ceil(3)
}

/// The problem with an effect image file of `file_bytes` and this size, if any (old
/// `EFFECT_IMAGE_LIMITS`: at most 8 MB published, 2048 px a side).
pub fn image_problem(width: u32, height: u32, file_bytes: u64) -> Option<String> {
    if width == 0 || height == 0 || width > MAX_IMAGE_SIDE || height > MAX_IMAGE_SIDE {
        return Some(format!(
            "effect image of {width}×{height} exceeds {MAX_IMAGE_SIDE} px a side"
        ));
    }
    (file_bytes > MAX_IMAGE_BYTES)
        .then(|| format!("effect image file of {file_bytes} bytes exceeds {MAX_IMAGE_BYTES}"))
}

/// Resident bytes of every effect image together (old `residentBytes`).
pub const MAX_RESIDENT_BYTES: u64 = 64 * 1024 * 1024;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn effect(layers: serde_json::Value) -> VisualEffect {
        serde_json::from_value(json!({
            "id": "effect.test.spark",
            "name": "Spark",
            "durationTicks": 40,
            "loop": true,
            "layers": layers,
        }))
        .unwrap()
    }

    fn base(shape: &str) -> serde_json::Value {
        json!({
            "id": shape, "shape": shape, "colour": "#ff8000", "accent": "#ffffff",
            "durationTicks": 20, "count": 3, "size": 1.5, "height": 2.0, "startTick": 4,
        })
    }

    #[test]
    fn shapes_become_primitives_with_seconds_and_linear_colours() {
        let mut ground = base("billboard");
        ground["texture"] = json!({"asset": "asset.test.image", "role": "image", "index": 1});
        ground["orientation"] = json!("ground");
        let mut emitter = base("particles");
        emitter["emitter"] = json!({
            "mode": "continuous", "lifetimeTicks": 30, "rate": 5, "speed": 2,
            "speedVariation": 0.5, "direction": {"x": 0, "y": 2, "z": 0}, "coneDegrees": 90,
            "gravity": -1, "drag": 2, "spawnRadius": 0.5, "startSize": 0.2, "endSize": 0,
            "spin": 1, "fadeIn": 0.1, "fadeOut": 0.5
        });
        let mut rain = base("rain");
        rain["fallSpeed"] = json!(2.0);
        rain["fallAngleDegrees"] = json!(45.0);
        rain["texture"] = json!({"asset": "asset.test.image", "role": "image", "index": 1});
        let mut off = base("ring");
        off["id"] = json!("off");
        off["enabled"] = json!(false);
        let layers = json!([base("ring"), base("burst"), ground, emitter, rain, off]);
        let manifest: Manifest =
            serde_json::from_value(json!({"version": 1, "records": []})).unwrap();
        let converted = convert(&[effect(layers)], &manifest);
        let e = &converted.table.effects[0];
        assert!(e.looping && (e.duration - 1.0).abs() < 1e-6);
        // Disabled layers are left out.
        assert_eq!(e.layers.len(), 5);
        let ring = &e.layers[0];
        assert_eq!(
            (ring.primitive, ring.motion),
            (Primitive::Decal, Motion::Ring)
        );
        assert!((ring.start - 0.1).abs() < 1e-6 && (ring.duration - 0.5).abs() < 1e-6);
        assert_eq!(ring.color[0], 1.0);
        assert!((ring.color[1] - 0.2158605).abs() < 1e-5, "{:?}", ring.color);
        assert_eq!(ring.color[2], 0.0);
        // Untextured bursts are soft spheres; a missing image is reported.
        assert_eq!(e.layers[1].primitive, Primitive::Particle);
        assert_eq!(e.layers[2].primitive, Primitive::Decal);
        assert!(e.layers[2].texture.is_none());
        let emitter = e.layers[3].emitter.as_ref().unwrap();
        assert!(!emitter.burst && (emitter.lifetime - 0.75).abs() < 1e-6);
        assert_eq!(emitter.direction, [0.0, 1.0, 0.0]);
        assert!((emitter.cone - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
        let rain = &e.layers[4];
        assert_eq!(rain.motion, Motion::Rain);
        let fall = rain.rain.as_ref().unwrap();
        assert!((fall.speed - 2.0).abs() < 1e-6);
        assert!(
            (fall.back[0] - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-5,
            "{fall:?}"
        );
        // Two missing images (the decal and the rain).
        assert_eq!(converted.warnings.len(), 2, "{:?}", converted.warnings);
    }

    #[test]
    fn image_budgets_follow_the_old_limits() {
        assert_eq!(resident_bytes(3, 1), 16);
        assert!(image_problem(2048, 2048, 1 << 20).is_none());
        assert!(image_problem(2048, 2048, 9 << 20).is_some());
        assert!(image_problem(4096, 16, 1).is_some());
        assert!(image_problem(0, 16, 1).is_some());
    }
}
