//! Cooking test-owned data folders end to end.

#![allow(linker_messages)]

mod fixture;

use canopy_assets::{AlphaMode, GroundContact, Pack, TextureFormat};
use canopy_cook::{Options, PACK_FILE, cook, diag::Severity};
use fixture::{BUILDING, CREATURE, ICON, SOUND, SPARK};
use std::path::Path;

fn options(data: &Path, out: &Path) -> Options {
    Options {
        data: data.to_owned(),
        out: out.to_owned(),
        force: false,
        allow_lfs_pointers: false,
        release: false,
        effect_budget: None,
    }
}

fn apply(m: &[f32; 12], p: [f32; 3]) -> [f32; 3] {
    std::array::from_fn(|r| {
        m[r * 4] * p[0] + m[r * 4 + 1] * p[1] + m[r * 4 + 2] * p[2] + m[r * 4 + 3]
    })
}

fn close(a: [f32; 3], b: [f32; 3]) -> bool {
    a.iter().zip(b).all(|(x, y)| (x - y).abs() < 1e-3)
}

#[test]
fn folds_transforms_and_keeps_team_masks_lossless() {
    let data = fixture::data_folder("static", |_| {});
    let report = cook(&options(&data, &data.join("out"))).unwrap();
    assert_eq!(report.errors(), 0, "{:?}", report.diagnostics);
    let pack = Pack::load(&data.join("out").join(PACK_FILE)).unwrap();

    let model = pack.model(BUILDING).unwrap();
    let m = &model.meta;
    assert_eq!(m.ground, GroundContact::Terrain);
    // The node scale doubles the quad (y −1..1); the soil offset lifts it onto y = 0.
    assert!((m.ground_offset - 1.0).abs() < 1e-6);
    assert!(m.rest_bounds.min[1].abs() < 1e-4, "{:?}", m.rest_bounds);
    assert!(
        (m.rest_bounds.max[1] - 2.8).abs() < 1e-3,
        "{:?}",
        m.rest_bounds
    );
    // Forward −Z turns the model half way: the quad's +x corner is now at −x.
    let positions: Vec<[f32; 3]> = model
        .vertices()
        .iter()
        .map(|v| model.position(v.position))
        .collect();
    assert!(
        positions.iter().any(|p| close(*p, [-1.0, 0.0, 0.0])),
        "{positions:?}"
    );
    assert_eq!(m.sections.len(), 2);
    assert_eq!(m.vertex_count, 7);

    let team = &m.materials[m.sections[0].material as usize];
    assert_eq!(team.name, "Team");
    let colour = team.team_color.as_ref().expect("team colour");
    assert_eq!(colour.source_max, 0.5);
    assert_eq!(team.alpha, AlphaMode::Opaque);
    let base = pack
        .texture(team.textures.base_color.as_deref().unwrap())
        .unwrap();
    assert_eq!(base.meta.format, TextureFormat::Bc7RgbaUnormSrgb);
    let mask = pack
        .texture(team.textures.team_mask.as_deref().unwrap())
        .unwrap();
    assert_eq!(mask.meta.format, TextureFormat::R8Unorm);
    let expected: Vec<u8> = (0..8)
        .flat_map(|y| (0..8).map(move |x| fixture::team_alpha(x, y)))
        .collect();
    assert_eq!(mask.level(0), expected.as_slice());
    assert_eq!(mask.meta.mips.len(), 4);

    let leaf = &m.materials[m.sections[1].material as usize];
    assert_eq!(leaf.alpha, AlphaMode::Mask);
    assert!(leaf.foliage && leaf.double_sided);
    assert_eq!(m.depth.masked, vec![1]);
    assert_eq!(m.depth.index_count, 6);
    assert_eq!(m.depth.vertex_count, 4);
    // The fallback path decodes BC7 to RGBA8.
    assert_eq!(base.decode_level(0).len(), 8 * 8 * 4);

    let scenery = pack.scenery().unwrap();
    let hut = &scenery.prototypes[0];
    assert_eq!(
        (hut.id.as_str(), hut.model.as_str()),
        ("scenery.fixture-hut", BUILDING)
    );
    assert_eq!(hut.blockers.len(), 1);
    assert_eq!(hut.light.as_ref().unwrap().color, [1.0; 3]);

    let icon = pack.texture(ICON).unwrap();
    assert_eq!((icon.meta.width, icon.meta.mips.len()), (16, 5));
    let sound = pack.sound(SOUND).unwrap();
    assert_eq!(sound.samples(), &[0, 1000, -1000, 32767, -32768]);
    std::fs::remove_dir_all(&data).ok();
}

#[test]
fn bakes_skinning_clips_states_and_sockets() {
    let data = fixture::data_folder("skinned", |_| {});
    cook(&options(&data, &data.join("out"))).unwrap();
    let pack = Pack::load(&data.join("out").join(PACK_FILE)).unwrap();
    let model = pack.model(CREATURE).unwrap();
    let m = &model.meta;
    let joints = m.skeleton.as_ref().unwrap().joints.len();
    assert_eq!(joints, 2);
    let clip = &m.clips[0];
    assert_eq!((clip.name.as_str(), clip.frame_count), ("bend", 31));
    let state = model.state("attack").unwrap();
    assert!(!state.looping);
    assert_eq!(state.events[0].name, "hit");
    assert!((state.events[0].time - 0.5).abs() < 1e-6);

    // The arm's tip (0, 2, 0) swings to (−1, 1, 0) in the last frame.
    let skin = model.skin().unwrap();
    let last = &model.clip_matrices(0)[(clip.frame_count as usize - 1) * joints..];
    let tip = model
        .vertices()
        .iter()
        .zip(skin)
        .map(|(v, s)| (model.position(v.position), s))
        .find(|(p, _)| close(*p, [0.0, 2.0, 0.0]))
        .expect("tip vertex");
    assert_eq!(tip.1.weights, [255, 0, 0, 0]);
    let posed = apply(&last[tip.1.joints[0] as usize], tip.0);
    assert!(close(posed, [-1.0, 1.0, 0.0]), "{posed:?}");
    // Frame 0 is the rest pose.
    let first = &model.clip_matrices(0)[..joints];
    assert!(close(
        apply(&first[tip.1.joints[0] as usize], tip.0),
        [0.0, 2.0, 0.0]
    ));

    // The socket follows the arm.
    assert_eq!(m.sockets.len(), 1);
    assert_eq!(m.sockets[0].name, "tip");
    let socket = model.clip_sockets(0)[clip.frame_count as usize - 1];
    assert!(close([socket[3], socket[7], socket[11]], [-1.0, 1.0, 0.0]));
    // Culling bounds hold every frame.
    assert!(m.bounds.min[0] <= -1.0 + 1e-3);
    std::fs::remove_dir_all(&data).ok();
}

#[test]
fn recooks_only_what_changed_and_is_deterministic() {
    let data = fixture::data_folder("incremental", |_| {});
    let out = data.join("out");
    let first = cook(&options(&data, &out)).unwrap();
    assert_eq!(first.cooked, first.jobs);
    let bytes = std::fs::read(out.join(PACK_FILE)).unwrap();

    let again = cook(&options(&data, &out)).unwrap();
    assert!(again.up_to_date);
    assert_eq!(again.cooked, 0);

    // A fresh cook elsewhere writes the same bytes.
    cook(&options(&data, &data.join("elsewhere"))).unwrap();
    assert_eq!(
        std::fs::read(data.join("elsewhere").join(PACK_FILE)).unwrap(),
        bytes
    );

    // Changing one image cooks one job.
    std::fs::write(
        data.join(fixture::source(ICON)),
        fixture::png(16, 16, |_, _| [1, 2, 3, 255]),
    )
    .unwrap();
    let changed = cook(&options(&data, &out)).unwrap();
    assert_eq!(changed.cooked, 1);
    assert_ne!(std::fs::read(out.join(PACK_FILE)).unwrap(), bytes);
    std::fs::remove_dir_all(&data).ok();
}

#[test]
fn lfs_pointers_fail_unless_allowed() {
    let data = fixture::data_folder("lfs", |_| {});
    std::fs::write(
        data.join(fixture::source(ICON)),
        "version https://git-lfs.github.com/spec/v1\noid sha256:00\nsize 1\n",
    )
    .unwrap();
    let error = cook(&options(&data, &data.join("out"))).unwrap_err();
    assert!(error.to_string().contains("git lfs pull"), "{error}");
    let report = cook(&Options {
        allow_lfs_pointers: true,
        release: false,
        ..options(&data, &data.join("out"))
    })
    .unwrap();
    assert!(report.diagnostics.iter().any(|d| d.message.contains("LFS")));
    let pack = Pack::load(&data.join("out").join(PACK_FILE)).unwrap();
    assert!(pack.entry(ICON).is_none());
    assert!(pack.entry(BUILDING).is_some());
    std::fs::remove_dir_all(&data).ok();
}

#[test]
fn reports_problems_in_the_art() {
    let data = fixture::data_folder("problems", |models| {
        let caps = &mut models[1]["capabilities"];
        caps["sockets"][0]["node"] = "socket_missing".into();
        caps["animations"][0]["clip"] = "missing".into();
        models[0]["capabilities"]["teamColor"]["slots"] = serde_json::json!(["Nothing"]);
    });
    let report = cook(&options(&data, &data.join("out"))).unwrap();
    let has = |severity: Severity, text: &str| {
        report
            .diagnostics
            .iter()
            .any(|d| d.severity == severity && d.message.contains(text))
    };
    assert!(has(Severity::Warning, "missing node"));
    assert!(has(Severity::Warning, "missing clip"));
    assert!(has(Severity::Error, "no material can take it"));
    assert!(!data.join("out").join(PACK_FILE).exists());
    std::fs::remove_dir_all(&data).ok();
}

#[test]
fn effects_become_runtime_primitives_bound_to_pack_entries() {
    let data = fixture::data_folder("effects", |_| {});
    let report = cook(&options(&data, &data.join("out"))).unwrap();
    assert_eq!(report.errors(), 0, "{:?}", report.diagnostics);
    let pack = Pack::load(&data.join("out").join(PACK_FILE)).unwrap();
    let table = pack.effects().unwrap();
    let effect = &table.effects[table.find("effect.fixture.sigil").unwrap()];
    assert!((effect.duration - 1.0).abs() < 1e-6);
    let sigil = &effect.layers[0];
    assert_eq!(sigil.primitive, canopy_assets::Primitive::Particle);
    assert_eq!(sigil.texture.as_deref(), Some(SPARK));
    // The image it samples is cooked.
    assert!(pack.texture(SPARK).is_ok());
    let hut = effect.layers[1].model.as_ref().unwrap();
    assert_eq!(hut.model, BUILDING);
    assert!((hut.rotation[1] - std::f32::consts::FRAC_PI_2).abs() < 1e-6);
}

#[test]
fn a_release_withholds_the_art_only_trimmed_steps_name() {
    let base = std::env::temp_dir().join(format!("canopy-cook-withheld-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&base);
    let write = |path: &str, value: serde_json::Value| {
        let file = base.join(path);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, value.to_string()).unwrap();
    };
    for id in ["asset.shared", "asset.later", "asset.scripted"] {
        write(
            &format!("assets/{id}/asset.json"),
            serde_json::json!({"id": id, "kind": "interface"}),
        );
    }
    let root = "campaigns/held.utcc";
    write(
        &format!("{root}/package.json"),
        serde_json::json!({"format": 1, "id": "held", "kind": "campaign", "name": "Held", "version": "1.0.0"}),
    );
    write(
        &format!("{root}/campaign.json"),
        serde_json::json!({"race": "red", "steps": [
            {"kind": "chapter", "id": "one", "title": "One", "art": "asset.shared"},
            {"kind": "earlyAccess", "id": "line"},
            {"kind": "chapter", "id": "two", "title": "Two", "art": "asset.shared"},
            {"kind": "story", "id": "tale", "title": "Tale",
             "pages": [{"art": "asset.later", "text": "Later."}, {"art": "asset.scripted", "text": "."}]},
            {"kind": "mission", "id": "far", "map": "far", "title": "Far"}
        ]}),
    );
    write(
        &format!("{root}/missions/far.utcm/package.json"),
        serde_json::json!({"format": 1, "id": "far", "kind": "map", "name": "Far", "version": "1.0.0"}),
    );
    std::fs::create_dir_all(base.join("maps/skirmish/near.utcm")).unwrap();
    std::fs::write(
        base.join("maps/skirmish/near.utcm/mission.lua"),
        "show(\"asset.scripted\")",
    )
    .unwrap();

    let out = base.with_extension("out");
    let withheld = canopy_cook::stage_documents(&base, &out).unwrap();
    // Named only past the line: left out, its staged asset too. Named by a kept step or a
    // shipped script: kept.
    assert_eq!(withheld.assets.iter().collect::<Vec<_>>(), ["asset.later"]);
    assert!(!out.join("assets/asset.later/asset.json").exists());
    assert!(out.join("assets/asset.shared/asset.json").exists());
    assert!(withheld.holds("asset.later/image.webp"));
    assert!(!withheld.holds("asset.shared/image.webp"));
    assert!(withheld.holds("maps/far/preview.webp"));
    assert!(!withheld.holds("maps/near/preview.webp"));
    std::fs::remove_dir_all(&base).ok();
    std::fs::remove_dir_all(&out).ok();
}
