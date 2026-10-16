//! Loading the game's content from packages (docs/packages.md, D53): the base package
//! (`base/` in the data folder), and later campaigns, mods and a map layered on top.
//!
//! Every collection is layered across the packages ([`crate::package::layer`]), then the
//! documents are assembled into the typed [`Content`] the simulation reads. Errors name
//! the element's own file.

use crate::{
    abilities::{self, AbilityLibrary},
    assets::{self, Manifest, landscape::LandscapeAsset},
    effects::{self, VisualEffect},
    error::{Error, Errors, parse},
    game::{Actions, Definition, RenderAsset, Rules},
    package::{Collection, Layers, Package, check_order},
    registry,
};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, path::Path};

/// The base package's folder in the data folder.
pub const BASE_DIR: &str = "base";

/// Collections of game data, by package folder.
pub mod collections {
    use crate::package::{Collection, Form};

    pub const UNITS: Collection = Collection::files("units");
    pub const BUILDINGS: Collection = Collection::files("buildings");
    pub const ITEMS: Collection = Collection::files("items");
    pub const RESOURCES: Collection = Collection::files("resources");
    pub const BEHAVIORS: Collection = Collection::files("behaviors");
    pub const RACES: Collection = Collection::files("races");
    pub const RESEARCH: Collection = Collection::files("research");
    pub const LOOT_POOLS: Collection = Collection::files("loot-pools");
    pub const ABILITIES: Collection = Collection::files("abilities");
    pub const EFFECTS: Collection = Collection::files("effects");
    /// One-off rule documents, by name.
    pub const RULES: Collection = Collection {
        dir: "rules",
        form: Form::Singletons(&["game", "combat", "experience", "commands"]),
    };
    pub const CAMPAIGNS: Collection = Collection {
        dir: "campaigns",
        form: Form::Packages(crate::package::CAMPAIGN_EXTENSION),
    };
}

use collections::*;

/// An element after layering: its id, the file that last authored it and its document.
pub(crate) struct Element {
    pub id: String,
    pub file: String,
    pub value: Value,
}

impl Element {
    /// Parses the document, reporting a mismatch against the element's file.
    pub fn parse<T: serde::de::DeserializeOwned>(self, errors: &mut Vec<Error>) -> Option<T> {
        parse(self.value, &self.file, "")
            .map_err(|e| errors.push(e))
            .ok()
    }
}

/// All gameplay content, resolved and validated. Maps are loaded separately.
#[derive(Debug, Clone)]
pub struct Content {
    /// Sorted by id.
    pub definitions: Vec<Definition>,
    pub rules: Rules,
    pub actions: Actions,
    /// How every asset and piece of scenery is drawn, sorted by id.
    pub render_assets: Vec<RenderAsset>,
    /// How each definition is drawn: its model's render asset with the definition's
    /// presentation applied, by definition index.
    pub looks: Vec<RenderAsset>,
    /// The asset records, built from the asset folders ([`assets::package`]).
    pub manifest: Manifest,
    /// Landscape catalogue: scenery, ground textures, water profiles and recipes the map
    /// scene is compiled from.
    pub landscape: Vec<LandscapeAsset>,
    /// Every asset and piece of scenery as authored.
    pub assets: Vec<assets::package::AssetDefinition>,
    pub scenery: Vec<assets::package::SceneryDefinition>,
    /// Sorted by id.
    pub abilities: AbilityLibrary,
    /// Sorted by id.
    pub effects: Vec<VisualEffect>,
    /// The file that last authored each element, by element id (definitions, abilities,
    /// effects, races, research, loot pools, behaviour sets) and `rules/<name>`.
    pub files: BTreeMap<String, String>,
    definition_index: BTreeMap<String, usize>,
    asset_index: BTreeMap<String, usize>,
}

/// Layers `collection` and returns its elements in id order, recording their files.
pub(crate) fn elements(
    collection: &Collection,
    packages: &[Package],
    files: &mut BTreeMap<String, String>,
    errors: &mut Vec<Error>,
) -> Vec<Element> {
    let layers = Layers::build(collection, packages, errors);
    layers
        .elements
        .into_values()
        .map(|layered| {
            let top = layered.top();
            let file = packages[top.package].label(&top.path);
            files.insert(layered.id.clone(), file.clone());
            Element {
                id: layered.id,
                file,
                value: layered.value,
            }
        })
        .collect()
}

/// A keyed rule map (`research`, `lootPools`, `races`): id → document without its id.
fn keyed(
    collection: &Collection,
    packages: &[Package],
    files: &mut BTreeMap<String, String>,
    errors: &mut Vec<Error>,
) -> Value {
    let mut map = Map::new();
    for element in elements(collection, packages, files, errors) {
        let mut value = element.value;
        if let Some(object) = value.as_object_mut() {
            object.remove("id");
        }
        map.insert(element.id, value);
    }
    Value::Object(map)
}

/// The documents of the game data before typing: definitions with their kind, behaviour
/// sets, and the rule and command documents assembled from their files.
struct Source {
    behavior_sets: Vec<Element>,
    definitions: Vec<Element>,
    rules: Value,
    actions: Value,
    abilities: Vec<Element>,
    effects: Vec<Element>,
}

fn read_source(
    packages: &[Package],
    files: &mut BTreeMap<String, String>,
    errors: &mut Vec<Error>,
) -> Source {
    let mut definitions = Vec::new();
    // Command card overrides live with their definition (`command`).
    let mut overrides = Map::new();
    for (collection, kind, verb) in [
        (UNITS, "unit", "produce"),
        (BUILDINGS, "building", "build"),
        (ITEMS, "item", "produce"),
        (RESOURCES, "resource", "produce"),
    ] {
        for mut element in elements(&collection, packages, files, errors) {
            if let Some(object) = element.value.as_object_mut() {
                if let Some(command) = object.remove("command") {
                    overrides.insert(format!("{verb}:{}", element.id), command);
                }
                object.insert("kind".into(), Value::String(kind.into()));
            }
            definitions.push(element);
        }
    }
    definitions.sort_by(|a, b| a.id.cmp(&b.id));

    let mut rule_files: BTreeMap<String, Value> = BTreeMap::new();
    let mut rule_paths = BTreeMap::new();
    for element in elements(&RULES, packages, &mut rule_paths, errors) {
        files.insert(format!("rules/{}", element.id), element.file);
        rule_files.insert(element.id, element.value);
    }
    let mut rules = Map::new();
    for name in ["game", "combat"] {
        if let Some(Value::Object(fields)) = rule_files.remove(name) {
            rules.extend(fields);
        }
    }
    if let Some(experience) = rule_files.remove("experience") {
        rules.insert("experience".into(), experience);
    }
    rules.insert("research".into(), keyed(&RESEARCH, packages, files, errors));
    rules.insert(
        "lootPools".into(),
        keyed(&LOOT_POOLS, packages, files, errors),
    );
    let races = keyed(&RACES, packages, files, errors);
    // The default race's starting setup is also the setup of seats and maps that name no
    // race (`Rules::starting_setup`).
    let default_race = rules
        .get("defaultRace")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if let Some(setup) = races.get(default_race).and_then(|r| r.get("startingSetup")) {
        rules.insert("startingSetup".into(), setup.clone());
    }
    rules.insert("races".into(), races);
    rules.insert("campaigns".into(), campaigns(packages, errors));

    let mut actions = match rule_files.remove("commands") {
        Some(Value::Object(fields)) => fields,
        _ => Map::new(),
    };
    actions.insert("overrides".into(), Value::Object(overrides));

    Source {
        behavior_sets: elements(&BEHAVIORS, packages, files, errors),
        definitions,
        rules: Value::Object(rules),
        actions: Value::Object(actions),
        abilities: elements(&ABILITIES, packages, files, errors),
        effects: elements(&EFFECTS, packages, files, errors),
    }
}

/// Campaigns of every package: id → `{name, race}`, from each campaign package's manifest
/// and `campaign.json`.
fn campaigns(packages: &[Package], errors: &mut Vec<Error>) -> Value {
    let mut map = Map::new();
    for package in packages {
        for found in CAMPAIGNS.read(package, errors) {
            let campaign = match Package::open(&package.root.join(&found.path)) {
                Ok(campaign) => campaign,
                Err(e) => {
                    errors.push(e);
                    continue;
                }
            };
            let file = campaign.label("campaign.json");
            let read = std::fs::read_to_string(campaign.root.join("campaign.json"))
                .map_err(|e| e.to_string())
                .and_then(|text| serde_json::from_str::<Value>(&text).map_err(|e| e.to_string()));
            match read {
                Ok(Value::Object(mut fields)) => {
                    fields.insert("name".into(), Value::String(campaign.manifest.name.clone()));
                    map.insert(campaign.id().to_string(), Value::Object(fields));
                }
                Ok(_) => errors.push(Error::new(&file, "", "expected an object")),
                Err(e) => errors.push(Error::new(&file, "", e)),
            }
        }
    }
    Value::Object(map)
}

/// A definition's look: `model` (its model's render asset, when there is one) with the
/// definition's presentation applied.
fn look(model: Option<&RenderAsset>, d: &Definition) -> RenderAsset {
    let mut look = model.cloned().unwrap_or_else(RenderAsset::empty);
    look.id = d.model.clone();
    if let Some(p) = &d.presentation {
        if p.character.is_some() {
            look.character = p.character.clone();
        }
        if p.health_height.is_some() {
            look.health_height = p.health_height;
        }
        look.projectile = p.projectile.clone();
        look.projectile_socket = p.projectile_socket.clone();
        look.impact_effect = p.impact_effect.clone();
        look.cast_contact = p.cast_contact;
        look.camera_anchor = p.camera_anchor.clone();
    }
    look
}

impl Content {
    /// Reads, resolves and validates the base package of `data` (the folder holding
    /// `base/`). Returns every problem found, not just the first.
    pub fn load(data: &Path) -> Result<Self, Errors> {
        Self::load_with(data, assets::Files::Check)
    }

    /// Loads a packaged game's data: like [`Content::load`], but asset files are not
    /// required on disk, because the cooked pack replaces them.
    pub fn load_packaged(data: &Path) -> Result<Self, Errors> {
        Self::load_with(data, assets::Files::Skip)
    }

    fn load_with(data: &Path, files: assets::Files) -> Result<Self, Errors> {
        let base = Package::open(&data.join(BASE_DIR)).map_err(|e| Errors(vec![e]))?;
        Self::from_packages(&[base], files)
    }

    /// Resolves `packages` in load order (the base first).
    pub fn from_packages(packages: &[Package], asset_files: assets::Files) -> Result<Self, Errors> {
        let mut errors = check_order(packages);
        let content = Self::read(packages, asset_files, &mut errors);
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

    fn read(
        packages: &[Package],
        asset_files: assets::Files,
        errors: &mut Vec<Error>,
    ) -> Option<Self> {
        let mut files = BTreeMap::new();
        let source = read_source(packages, &mut files, errors);
        let file = |name: &str| {
            files
                .get(name)
                .cloned()
                .unwrap_or_else(|| format!("{BASE_DIR}/{name}.json"))
        };
        let rules: Option<Rules> = parse(source.rules, &file("rules/game"), "")
            .map_err(|e| errors.push(e))
            .ok();
        let actions: Option<Actions> = parse(source.actions, &file("rules/commands"), "")
            .map_err(|e| errors.push(e))
            .ok();
        let definitions =
            registry::resolve_definitions(&source.behavior_sets, source.definitions, errors);
        let abilities = abilities::read(source.abilities, &files, errors);
        let effects = effects::read(source.effects, errors);
        let index = assets::package::index(packages, asset_files, errors);
        let manifest = index.manifest;
        let mut render_assets: Vec<RenderAsset> = assets::render_assets(&manifest);
        render_assets.sort_by(|a, b| a.id.cmp(&b.id));
        for pair in render_assets.windows(2) {
            if pair[0].id == pair[1].id {
                errors.push(Error::new(
                    "assets",
                    pair[0].id.as_str(),
                    "an asset and a piece of scenery share this id",
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
        let asset_of = |id: &str| {
            render_assets
                .binary_search_by(|a| a.id.as_str().cmp(id))
                .ok()
                .map(|i| &render_assets[i])
        };
        let looks = definitions
            .iter()
            .map(|d| look(asset_of(d.model.as_str()), d))
            .collect();
        Some(Self {
            definitions,
            rules: rules?,
            actions: actions?,
            looks,
            render_assets,
            manifest,
            landscape: index.catalogue,
            assets: index.assets,
            scenery: index.scenery,
            abilities: abilities?,
            effects: effects?,
            files,
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

    /// How definition `id` is drawn: its model with its presentation applied.
    pub fn look(&self, id: &str) -> Option<&RenderAsset> {
        self.definition_index.get(id).map(|&i| &self.looks[i])
    }

    /// How an entity of definition `d` shown as `asset` is drawn: the definition's look
    /// when it shows its own model, else that asset (a placement's appearance, a
    /// depletion stage, a magic form).
    pub fn render_of(&self, d: &Definition, asset: &str) -> Option<&RenderAsset> {
        if asset == d.model.as_str() {
            self.look(d.id.as_str())
        } else {
            self.asset(asset)
        }
    }

    /// The file that last authored element `id`; the game rules for anything else.
    pub fn file_of(&self, id: &str) -> &str {
        self.files
            .get(id)
            .or_else(|| self.files.get("rules/game"))
            .map_or(BASE_DIR, String::as_str)
    }

    /// The file a validation location points into: `definitions/<id>…`,
    /// `rules.races.<id>…`, `rules.research.<id>…`, `rules.lootPools.<id>…` or an element
    /// id; the game rules for anything else.
    pub fn origin(&self, location: &str) -> &str {
        let rest = [
            "definitions/",
            "rules.races.",
            "rules.research.",
            "rules.lootPools.",
        ]
        .iter()
        .find_map(|p| location.strip_prefix(p))
        .unwrap_or(location);
        // The longest known id the location starts with.
        let mut end = rest.len();
        loop {
            if let Some(file) = self.files.get(&rest[..end]) {
                return file;
            }
            match rest[..end].rfind(['.', '/', ' ', '[']) {
                Some(cut) if cut > 0 => end = cut,
                _ => return self.file_of(""),
            }
        }
    }
}
