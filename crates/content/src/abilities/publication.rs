//! Every resource a presentation plays must be published: the icon image and the
//! textures, sounds and models of each enabled layer of its bound effects.

use super::{AbilityLibrary, Presentation};
use crate::{
    effects::{VisualEffect, VisualLayer},
    error::{Error, parse, read_json},
};
use serde::Deserialize;
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
};

const FILE: &str = crate::ABILITIES_FILE;

/// The part of `assets/manifest.json` this check reads. Deliberately lenient (no
/// `deny_unknown_fields`): the assets module owns and strictly checks that format.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishedRecords {
    records: Vec<PublishedRecord>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishedRecord {
    id: String,
    #[serde(default)]
    outputs: Vec<PublishedOutput>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PublishedOutput {
    path: String,
}

/// A published output of an asset, by role and one-based index: `image`, `audio` or
/// `geometry`, stored as `<role>.<ext>` for index 1 and `<role>_<index>.<ext>` after that.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct ResourceRef<'a> {
    pub asset: &'a str,
    pub role: &'static str,
    pub index: u32,
}

/// Reads the effect library and asset manifest (whose own problems their loaders report)
/// and checks every presentation's resources.
pub(crate) fn check_resources(data: &Path, library: &AbilityLibrary, errors: &mut Vec<Error>) {
    let Ok(effects) = crate::effects::read(data) else {
        return;
    };
    let published = read_json(data, crate::MANIFEST_FILE)
        .and_then(|value| parse::<PublishedRecords>(value, crate::MANIFEST_FILE, ""))
        .ok();
    let outputs: Option<BTreeMap<&str, Vec<&str>>> = published.as_ref().map(|published| {
        published
            .records
            .iter()
            .map(|record| {
                let files = record
                    .outputs
                    .iter()
                    .map(|o| o.path.rsplit('/').next().unwrap_or_default())
                    .collect();
                (record.id.as_str(), files)
            })
            .collect()
    });
    for presentation in &library.presentations {
        let at = presentation.id.as_str();
        let refs = match presentation_resources(presentation, &effects.effects) {
            Ok(refs) => refs,
            Err(unknown) => {
                errors.push(Error::new(
                    FILE,
                    at,
                    format!("Unknown visual effect {unknown}"),
                ));
                continue;
            }
        };
        let Some(outputs) = &outputs else {
            continue;
        };
        for resource in refs.into_iter().collect::<BTreeSet<_>>() {
            let name = match resource.index {
                1 => format!("{}.", resource.role),
                index => format!("{}_{index}.", resource.role),
            };
            let found = outputs
                .get(resource.asset)
                .is_some_and(|files| files.iter().any(|f| f.starts_with(&name)));
            if !found {
                errors.push(Error::new(
                    FILE,
                    at,
                    format!(
                        "Missing published effect {role} {asset}/{role}:{index}",
                        role = resource.role,
                        asset = resource.asset,
                        index = resource.index
                    ),
                ));
            }
        }
    }
}

/// The icon and every resource of the enabled layers of the bound effects, or the first
/// unknown effect id.
pub(crate) fn presentation_resources<'a>(
    presentation: &'a Presentation,
    effects: &'a [VisualEffect],
) -> Result<Vec<ResourceRef<'a>>, &'a str> {
    let mut refs: Vec<ResourceRef> = presentation
        .icon
        .iter()
        .map(|icon| ResourceRef {
            asset: icon.as_str(),
            role: "image",
            index: 1,
        })
        .collect();
    for binding in &presentation.effects {
        let effect = effects
            .iter()
            .find(|e| e.id == binding.effect)
            .ok_or(binding.effect.as_str())?;
        for layer in effect.layers.iter().filter(|l| l.enabled) {
            refs.extend(layer_resources(layer));
        }
    }
    Ok(refs)
}

fn layer_resources(layer: &VisualLayer) -> impl Iterator<Item = ResourceRef<'_>> {
    let texture = layer.texture.as_ref().map(|t| ResourceRef {
        asset: t.asset.as_str(),
        role: "image",
        index: t.index,
    });
    let sound = layer.sound.as_ref().map(|s| ResourceRef {
        asset: s.asset.as_str(),
        role: "audio",
        index: s.index,
    });
    let model = layer.model.as_ref().map(|m| ResourceRef {
        asset: m.asset.as_str(),
        role: "geometry",
        index: m.index,
    });
    texture.into_iter().chain(sound).chain(model)
}
