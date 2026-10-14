//! Loading the game's content from a data folder.

use crate::{
    abilities::{self, AbilityLibrary},
    assets::{self, Manifest},
    effects::{self, VisualEffect},
    error::{Error, Errors, parse, read_json},
    game::{Actions, Definition, GameSource, RenderAsset, Rules},
    registry,
};
use std::{collections::BTreeMap, path::Path};

/// Files the game reads, relative to the data folder.
pub const GAME_FILE: &str = "content/game.json";
pub const ABILITIES_FILE: &str = "content/abilities/published.json";
pub const EFFECTS_FILE: &str = "content/effects/published.json";
pub const MANIFEST_FILE: &str = "assets/manifest.json";

/// All gameplay content, resolved and validated. Maps are loaded separately.
#[derive(Debug, Clone)]
pub struct Content {
    /// Sorted by id.
    pub definitions: Vec<Definition>,
    pub rules: Rules,
    pub actions: Actions,
    /// Render assets of every published record, sorted by id.
    pub render_assets: Vec<RenderAsset>,
    pub manifest: Manifest,
    pub abilities: AbilityLibrary,
    pub effects: Vec<VisualEffect>,
    definition_index: BTreeMap<String, usize>,
    asset_index: BTreeMap<String, usize>,
}

impl Content {
    /// Reads, resolves and validates everything under `data` (the folder holding `content/`
    /// and `assets/`). Returns every problem found, not just the first.
    pub fn load(data: &Path) -> Result<Self, Errors> {
        let mut errors = Vec::new();
        let content = Self::read(data, &mut errors);
        match content {
            Some(content) if errors.is_empty() => {
                registry::validate(&content, &mut errors);
                if errors.is_empty() {
                    Ok(content)
                } else {
                    Err(Errors(errors))
                }
            }
            _ => Err(Errors(errors)),
        }
    }

    fn read(data: &Path, errors: &mut Vec<Error>) -> Option<Self> {
        fn load<T: serde::de::DeserializeOwned>(
            data: &Path,
            file: &str,
            errors: &mut Vec<Error>,
        ) -> Option<T> {
            read_json(data, file)
                .and_then(|value| parse(value, file, ""))
                .map_err(|error| errors.push(error))
                .ok()
        }
        fn part<T: serde::de::DeserializeOwned>(
            value: &serde_json::Value,
            location: &str,
            errors: &mut Vec<Error>,
        ) -> Option<T> {
            parse(value.clone(), GAME_FILE, location)
                .map_err(|error| errors.push(error))
                .ok()
        }
        let game: Option<GameSource> = load(data, GAME_FILE, errors);
        let manifest: Option<Manifest> = assets::load(data, errors);
        let abilities = abilities::load(data, errors);
        let effects = effects::load(data, errors);
        let game = game?;
        let rules: Option<Rules> = part(&game.rules, "rules", errors);
        let actions: Option<Actions> = part(&game.actions, "actions", errors);
        let definitions = registry::resolve_definitions(&game, errors);

        let manifest = manifest?;
        let mut render_assets: Vec<RenderAsset> = assets::render_assets(&manifest);
        render_assets.sort_by(|a, b| a.id.cmp(&b.id));
        for pair in render_assets.windows(2) {
            if pair[0].id == pair[1].id {
                errors.push(Error::new(
                    MANIFEST_FILE,
                    "render",
                    format!("duplicate asset {}", pair[0].id),
                ));
            }
        }
        let definition_index = definitions
            .iter()
            .enumerate()
            .map(|(i, d)| (d.id.to_string(), i))
            .collect();
        let asset_index = render_assets
            .iter()
            .enumerate()
            .map(|(i, a)| (a.id.to_string(), i))
            .collect();
        Some(Self {
            definitions,
            rules: rules?,
            actions: actions?,
            render_assets,
            manifest,
            abilities: abilities?,
            effects: effects?,
            definition_index,
            asset_index,
        })
    }

    pub fn definition(&self, id: &str) -> Option<&Definition> {
        self.definition_index.get(id).map(|&i| &self.definitions[i])
    }

    pub fn asset(&self, id: &str) -> Option<&RenderAsset> {
        self.asset_index.get(id).map(|&i| &self.render_assets[i])
    }
}
