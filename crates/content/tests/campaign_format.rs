//! Campaign packages (`.utcc`): `campaign.json` is checked against the content and the
//! package's own maps, and its steps add up to each mission's tech. Test-owned content and
//! campaigns; the shipped campaigns are only required to pass every rule.

use canopy_content::{Content, campaign, map};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    sync::OnceLock,
};

fn definition(id: &str, kind: &str, extra: Value) -> Value {
    let mut definition = json!({
        "id": id, "kind": kind, "name": id, "description": "",
        "asset": "asset.model", "icon": "asset.icon",
    });
    for (key, value) in extra.as_object().expect("object") {
        definition[key] = value.clone();
    }
    definition
}

fn game() -> Value {
    let body = json!({"maxHp": 100, "armor": 0, "armorType": "armor.plain"});
    let unit = json!({"body": body, "supplyCost": 1, "dimensions": {"radius": 0.4, "height": 1, "formationSpacing": 1}, "behaviors": {"movement": {"speed": 1}}});
    let hall = json!({"body": body, "footprint": {"width": 4, "depth": 4}, "entrance": {"x": 0.5, "y": 2.5}, "behaviors": {"storage": {"capacity": 50, "accepts": ["item.coin"]}}});
    let meta = json!({"name": "Command", "description": "", "icon": "asset.icon", "priority": 0});
    let race = |roster: Value, setup: Value| json!({"name": "Race", "description": "", "roster": roster, "startingSetup": setup});
    let setup = |id: &str, fort: &str, worker: &str| {
        json!({
            "id": id, "fort": fort, "inventory": {"item.coin": 10},
            "units": [{"definition": worker, "offset": {"x": 0, "y": 4}}],
        })
    };
    let actions: serde_json::Map<String, Value> = [
        "move",
        "attack",
        "stop",
        "hold",
        "patrol",
        "follow",
        "garrison",
        "unload",
        "build",
        "produce",
        "cancel",
        "rally",
        "pause",
        "upgrade",
        "cancelUpgrade",
        "research",
        "cancelResearch",
    ]
    .into_iter()
    .map(|name| (name.to_string(), meta.clone()))
    .collect();
    json!({
        "behaviorSets": [],
        "actions": {"navigation": {"back": meta}, "categories": {}, "actions": actions, "overrides": {}},
        "rules": {
            "id": "rules.test",
            "armorTypes": {"armor.plain": {"name": "Plain", "icon": "asset.icon"}},
            "maxUnits": 100, "maxSupply": 100, "supplyIcon": "asset.icon", "maxBuildings": 100,
            "constructionHpPermille": 100, "constructionRefundPermille": 500, "repairTicks": 10,
            "research": {}, "lootPools": {},
            "armorCoefficient": 0.06,
            "damageTypes": {"damage.plain": {"name": "Plain", "appliesArmor": true}},
            "heroStunDurationPermille": 500,
            "experience": {
                "unitRewards": [1], "heroRewards": [1], "neutralLevelCap": 2,
                "neutralMultipliersPermille": [500], "summonMultiplierPermille": 500,
                "globalFallback": false, "soloHeroTierBonusPermille": [0],
            },
            "damageMultipliers": {"damage.plain": {"armor.plain": 1000}},
            "defaultRace": "red",
            "races": {
                "red": race(
                    json!(["building.red.hall", "unit.red.worker", "unit.red.soldier", "unit.red.flier"]),
                    setup("setup.red", "building.red.hall", "unit.red.worker"),
                ),
                "blue": race(
                    json!(["building.blue.hall", "unit.blue.soldier"]),
                    setup("setup.blue", "building.blue.hall", "unit.blue.soldier"),
                ),
            },
            "startingSetup": {
                "id": "setup.test", "fort": "building.red.hall", "inventory": {"item.coin": 10},
                "units": [{"definition": "unit.red.worker", "offset": {"x": 0, "y": 4}}],
            },
        },
        "definitions": [
            definition("building.red.hall", "building", hall.clone()),
            definition("building.blue.hall", "building", hall),
            definition("item.coin", "item", json!({"stackLimit": 100, "currency": true})),
            definition("unit.red.worker", "unit", unit.clone()),
            definition("unit.red.soldier", "unit", unit.clone()),
            definition("unit.red.flier", "unit", unit.clone()),
            definition("unit.blue.soldier", "unit", unit),
        ],
    })
}

fn content() -> &'static Content {
    static CONTENT: OnceLock<Content> = OnceLock::new();
    CONTENT.get_or_init(|| {
        let records = vec![
            canopy_content::testing::image("asset.icon"),
            canopy_content::testing::model("asset.model"),
        ];
        canopy_content::testing::DataFolder::new(game(), records)
            .load("campaign-format")
            .unwrap_or_else(|errors| panic!("{errors}"))
    })
}

/// Writes a campaign package with `campaign` and empty map packages named `maps`.
fn package(name: &str, campaign: Value, maps: &[&str]) -> PathBuf {
    let root = Path::new(env!("CARGO_TARGET_TMPDIR"))
        .join("campaign-format")
        .join(name)
        .join(format!("{name}.utcc"));
    let _ = std::fs::remove_dir_all(&root);
    let write = |path: &Path, value: Value| {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, value.to_string()).unwrap();
    };
    write(
        &root.join("package.json"),
        json!({"format": 1, "id": name, "kind": "campaign", "name": "Test campaign", "version": "1.0.0"}),
    );
    write(&root.join("campaign.json"), campaign);
    for id in maps {
        write(
            &root.join(format!("missions/{id}.utcm/package.json")),
            json!({"format": 1, "id": id, "kind": "map", "name": id, "version": "1.0.0"}),
        );
    }
    root
}

fn steps() -> Value {
    json!([
        {"kind": "chapter", "id": "prologue", "title": "Prologue", "art": "asset.icon"},
        {"kind": "story", "id": "tale", "title": "A Tale",
         "pages": [{"art": "asset.icon", "text": "Once."}, {"text": "Twice."}]},
        {"kind": "mission", "id": "first", "map": "first-map", "title": "First",
         "chapterLine": "Prologue · Mission One", "story": "One.\n\nTwo.", "heroLevelCap": 3,
         "tech": {"new": ["building.red.hall", "unit.red.worker", "unit.red.flier"], "blocked": []}},
        {"kind": "cinematic", "id": "scene", "title": "Scene", "note": "What it will show."},
        {"kind": "mission", "id": "second", "map": "second-map", "title": "Second",
         "tech": {"new": ["unit.red.soldier"], "blocked": ["unit.red.flier"]}}
    ])
}

fn messages(campaign: &campaign::CampaignPackage) -> Vec<String> {
    campaign::validate(campaign, content())
        .into_iter()
        .map(|e| format!("{} {}", e.location, e.message))
        .collect()
}

#[test]
fn a_campaign_reads_its_steps_and_maps() {
    let root = package(
        "valid",
        json!({"race": "red", "card": "asset.icon", "background": "asset.icon", "steps": steps()}),
        &["first-map", "second-map"],
    );
    let found = campaign::open(&root).unwrap_or_else(|e| panic!("{e}"));
    assert_eq!(messages(&found), Vec::<String>::new());
    assert_eq!(found.missions.len(), 2);
    assert_eq!(found.campaign.steps.len(), 5);
    assert!(!found.has_content());
    let listed = campaign::folder(root.parent().unwrap()).unwrap_or_else(|e| panic!("{e}"));
    assert!(listed.iter().any(|c| c.id() == "valid"));
}

#[test]
fn tech_adds_up_down_the_steps() {
    let found = campaign::open(&package(
        "tech",
        json!({"race": "red", "steps": steps()}),
        &["first-map", "second-map"],
    ))
    .unwrap();
    let c = &found.campaign;
    let ids = |set: std::collections::BTreeSet<canopy_content::Id>| {
        set.into_iter().map(|i| i.to_string()).collect::<Vec<_>>()
    };
    // The first mission: what it unlocks is open, the rest of the race is locked.
    assert_eq!(ids(c.locked(content(), 2)), vec!["unit.red.soldier"]);
    // The second: the soldier opens, the flier is blocked for this mission only.
    assert_eq!(ids(c.locked(content(), 4)), vec!["unit.red.flier"]);
    // Another race's units are never the campaign's tech.
    assert!(
        !campaign::race_tech(content(), "red")
            .iter()
            .any(|i| i.as_str() == "unit.blue.soldier")
    );
}

#[test]
fn broken_campaigns_say_what_is_wrong() {
    let broken = |name: &str, change: &dyn Fn(&mut Value), expected: &str| {
        let mut document = json!({"race": "red", "steps": steps()});
        change(&mut document);
        let found = campaign::open(&package(name, document, &["first-map", "second-map"]))
            .unwrap_or_else(|e| panic!("{e}"));
        let found = messages(&found);
        assert!(
            found.iter().any(|m| m.contains(expected)),
            "{name}: expected {expected:?}, got {found:?}"
        );
    };
    broken(
        "race",
        &|d| d["race"] = json!("green"),
        "unknown race green",
    );
    broken(
        "twice",
        &|d| d["steps"][3]["id"] = json!("first"),
        "step id first is used twice",
    );
    broken(
        "map",
        &|d| d["steps"][2]["map"] = json!("elsewhere"),
        "no map elsewhere",
    );
    broken(
        "foreign",
        &|d| d["steps"][4]["tech"]["new"] = json!(["unit.blue.soldier"]),
        "not tech of the red race",
    );
    broken(
        "unknown-tech",
        &|d| d["steps"][4]["tech"]["new"] = json!(["unit.red.ghost"]),
        "unknown tech unit.red.ghost",
    );
    broken(
        "both",
        &|d| d["steps"][4]["tech"]["blocked"] = json!(["unit.red.soldier"]),
        "both new and blocked",
    );
    broken(
        "art",
        &|d| d["steps"][1]["pages"][0]["art"] = json!("asset.nowhere"),
        "unknown asset asset.nowhere",
    );
    broken(
        "cap",
        &|d| d["steps"][2]["heroLevelCap"] = json!(0),
        "heroLevelCap",
    );
    broken("empty", &|d| d["steps"] = json!([]), "at least one step");
    // Unknown fields are refused when the package opens.
    let mut extra = json!({"race": "red", "steps": steps()});
    extra["steps"][2]["nextMission"] = json!("second");
    assert!(campaign::open(&package("extra", extra, &["first-map"])).is_err());
}

#[test]
fn shipped_campaigns_are_valid() {
    let data = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let content = Content::load(data).unwrap_or_else(|errors| panic!("{errors}"));
    let campaigns = campaign::library(data).unwrap_or_else(|errors| panic!("{errors}"));
    for c in &campaigns {
        let errors = campaign::validate(c, &content);
        assert!(errors.is_empty(), "{}: {errors:#?}", c.id());
        for file in &c.missions {
            let loaded = file.load().unwrap_or_else(|errors| panic!("{errors}"));
            assert!(loaded.mission.is_some(), "{}: not a mission map", file.file);
            let errors = map::validate(&loaded, &content);
            assert!(errors.is_empty(), "{}: {errors:#?}", file.file);
        }
    }
}

/// Every text key a shipped mission names, in its objectives and its script, is in the base,
/// campaign or mission text (D96).
#[test]
fn shipped_missions_name_only_known_text() {
    let data = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    let content = Content::load(data).unwrap_or_else(|errors| panic!("{errors}"));
    let campaigns = campaign::library(data).unwrap_or_else(|errors| panic!("{errors}"));
    for c in &campaigns {
        for file in &c.missions {
            let mission =
                canopy_content::package::Package::open(&file.path).expect("mission package");
            let mut library = content.text.clone();
            let mut errors = Vec::new();
            library.layer(&[c.package.clone(), mission], &mut errors);
            assert!(errors.is_empty(), "{}: {errors:#?}", file.file);
            let loaded = file.load().unwrap_or_else(|errors| panic!("{errors}"));
            let mut named: Vec<String> = Vec::new();
            for o in loaded
                .mission
                .iter()
                .flat_map(|m| m.objectives.iter().flatten())
            {
                named.extend([o.title.clone(), o.description.clone()]);
            }
            if let Ok(script) = std::fs::read_to_string(file.path.join("mission.lua")) {
                named.extend(
                    script
                        .split('"')
                        .skip(1)
                        .step_by(2)
                        .filter(|s| {
                            ["line.", "objective.", "campaign."]
                                .iter()
                                .any(|p| s.starts_with(p))
                        })
                        .map(str::to_string),
                );
            }
            for key in named {
                assert!(
                    library.get(&key).is_some() || !canopy_content::text::looks_like_key(&key),
                    "{}: names {key}, which no text file holds",
                    file.file
                );
            }
        }
    }
}
