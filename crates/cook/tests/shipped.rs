//! The repository's own data, checked generically: no shipped id is named here.
//!
//! Planning reads only JSON, so it runs everywhere. Cooking needs the art, which CI does
//! not fetch (Git LFS), so it is ignored by default and skips itself on LFS pointers:
//! `cargo test -p canopy-cook --test shipped -- --ignored`.

#![allow(linker_messages)]

use canopy_assets::{EntryKind, Pack};
use canopy_content::assets;
use canopy_cook::{Options, PACK_FILE, cook, diag::Severity, plan};
use std::{collections::BTreeSet, path::PathBuf};

fn data() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn every_published_output_is_cooked_or_deliberately_skipped() {
    let data = data();
    let inputs = plan::Inputs::load(&data).unwrap();
    let plan = plan::plan(&data, &inputs);
    assert!(
        plan.unknown.is_empty(),
        "outputs of unknown type: {:?}",
        plan.unknown
    );
    let planned: BTreeSet<&str> = plan.jobs.iter().flat_map(|j| j.sources()).collect();
    let skipped: BTreeSet<&str> = plan.skipped.iter().map(|(p, _)| p.as_str()).collect();
    for path in assets::output_paths(&inputs.manifest) {
        assert!(
            planned.contains(path) != skipped.contains(path),
            "{path} must be either cooked or skipped"
        );
    }
    // What the game binds is always cooked.
    for asset in assets::render_assets(&inputs.manifest) {
        for file in [&asset.file, &asset.image, &asset.harvest_animation]
            .into_iter()
            .flatten()
        {
            assert!(
                planned.contains(file.as_str()),
                "{file} is bound but not cooked"
            );
        }
    }
    for scenery in assets::scenery(&inputs.manifest) {
        let file = format!("assets/{}", scenery.file);
        assert!(
            planned.contains(file.as_str()),
            "{file} is scenery but not cooked"
        );
    }
}

#[test]
#[ignore = "cooks every shipped asset (needs Git LFS files; about a minute)"]
fn the_shipped_data_cooks_cleanly() {
    let data = data();
    let out = std::env::temp_dir().join(format!("canopy-cook-shipped-{}", std::process::id()));
    let report = match cook(&Options {
        data: data.clone(),
        out: out.clone(),
        force: false,
        allow_lfs_pointers: false,
    }) {
        Err(error) if error.to_string().contains("Git LFS") => {
            eprintln!("skipped: {error}");
            return;
        }
        other => other.unwrap(),
    };
    let errors: Vec<_> = report
        .diagnostics
        .iter()
        .filter(|d| d.severity == Severity::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:#?}");

    let pack = Pack::load(&out.join(PACK_FILE)).unwrap();
    for entry in pack.entries() {
        let blob = pack.blob(entry).unwrap();
        match entry.kind {
            EntryKind::Model => {
                let model = blob.into_model().unwrap();
                for material in &model.meta.materials {
                    let t = &material.textures;
                    for name in [
                        &t.base_color,
                        &t.team_mask,
                        &t.normal,
                        &t.metallic_roughness,
                        &t.occlusion,
                        &t.emissive,
                    ]
                    .into_iter()
                    .flatten()
                    {
                        pack.texture(name).unwrap();
                    }
                }
            }
            EntryKind::Texture => drop(blob.into_texture().unwrap()),
            EntryKind::Sound => drop(blob.into_sound().unwrap()),
            EntryKind::Data => drop(blob.bytes().unwrap().len()),
            EntryKind::Scenery => {
                for prototype in blob.scenery().unwrap().prototypes {
                    assert_eq!(
                        pack.entry(&prototype.model).map(|e| e.kind),
                        Some(EntryKind::Model)
                    );
                }
            }
            EntryKind::Effects => {
                // Every converted layer samples cooked textures and draws cooked models.
                for layer in blob
                    .effects()
                    .unwrap()
                    .effects
                    .iter()
                    .flat_map(|e| &e.layers)
                {
                    if let Some(texture) = &layer.texture {
                        assert_eq!(
                            pack.entry(texture).map(|e| e.kind),
                            Some(EntryKind::Texture)
                        );
                    }
                    if let Some(model) = &layer.model {
                        assert_eq!(
                            pack.entry(&model.model).map(|e| e.kind),
                            Some(EntryKind::Model)
                        );
                    }
                }
            }
        }
    }

    // Every resource an effect or ability presentation plays resolves to a cooked entry.
    let content = canopy_content::Content::load(&data).unwrap();
    let file = |asset: &str, role: &str, index: u32| -> String {
        let prefix = if index == 1 {
            format!("{role}.")
        } else {
            format!("{role}_{index}.")
        };
        let record = content
            .manifest
            .records
            .iter()
            .find(|r| r.id.as_str() == asset)
            .unwrap();
        record
            .outputs
            .iter()
            .find(|o| o.path.rsplit('/').next().unwrap().starts_with(&prefix))
            .unwrap()
            .path
            .clone()
    };
    for layer in content.effects.iter().flat_map(|e| &e.layers) {
        let resources = [
            layer
                .texture
                .as_ref()
                .map(|t| file(t.asset.as_str(), "image", t.index)),
            layer
                .model
                .as_ref()
                .map(|m| file(m.asset.as_str(), "geometry", m.index)),
            layer
                .sound
                .as_ref()
                .map(|s| file(s.asset.as_str(), "audio", s.index)),
        ];
        for path in resources.into_iter().flatten() {
            assert!(
                pack.entry(&path).is_some(),
                "effect resource {path} is not cooked"
            );
        }
    }
    for asset in &content.render_assets {
        for path in [&asset.file, &asset.image].into_iter().flatten() {
            assert!(
                pack.entry(path).is_some(),
                "render binding {path} is not cooked"
            );
        }
    }
    std::fs::remove_dir_all(&out).ok();
}
