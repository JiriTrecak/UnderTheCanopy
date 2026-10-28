//! Campaign packages (`.utcc`, docs/campaign.md "Formats", D85): `campaign.json` (the
//! race, the campaign screen's art and one linear list of steps) and the mission maps in
//! `missions/`.
//!
//! Steps may be drafts (D110): outlined in `outlines/<step id>.json` ([`crate::outline`])
//! but never played or shipped. One step in all the campaigns is the early access line;
//! a release leaves out what follows it ([`release`]).
//!
//! The order of missions lives here and nowhere else. A mission step names its map, its
//! presentation (loading art, chapter line, title, story), the hero level cap and the tech
//! it unlocks or blocks; [`Campaign::locked`] turns the steps into what player 1 may not
//! build, train or research in a mission.

use crate::{
    Content,
    error::{Error, Errors, parse},
    game::{Kind, ProductionMode},
    id::Id,
    map::{self, MapFile},
    package::{Package, PackageKind},
};
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::{
    collections::BTreeSet,
    path::{Path, PathBuf},
};

/// The campaign document in a campaign package.
pub const CAMPAIGN_FILE: &str = "campaign.json";
/// The folder of a campaign package holding its maps.
pub const MISSIONS_DIR: &str = "missions";
/// Where shipped campaigns live, relative to the data folder.
pub const CAMPAIGNS_FOLDER: &str = "base/campaigns";
/// The folder of a campaign package holding its steps' outlines (D110).
pub const OUTLINES_DIR: &str = "outlines";
/// A mission's highest hero level cap.
pub const MAX_LEVEL_CAP: u32 = 50;

fn is_false(value: &bool) -> bool {
    !*value
}

/// `campaign.json`.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Campaign {
    /// The race the player plays (`races/`).
    pub race: String,
    /// The campaign screen's background image.
    pub background: Option<Id>,
    /// The card art of the campaign selection.
    pub card: Option<Id>,
    /// Where it plays among the campaigns, lowest first (then by id).
    pub order: Option<u32>,
    /// Only outlined: never played and never shipped.
    #[serde(default, skip_serializing_if = "is_false")]
    pub draft: bool,
    /// The campaign, step by step; played straight through.
    pub steps: Vec<Step>,
}

/// One step of a campaign.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Step {
    /// A title card.
    Chapter(Chapter),
    /// Full-screen pages of art and text.
    Story(Story),
    /// A playable map.
    Mission(MissionStep),
    /// A cinematic; for now a placeholder card with a note of what it will show.
    Cinematic(Cinematic),
    /// Where early access ends: one in all the campaigns. Players get the end card here,
    /// and a release leaves out everything after it.
    EarlyAccess(EarlyAccess),
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Chapter {
    pub id: Id,
    pub title: String,
    pub art: Option<Id>,
    pub music: Option<Id>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub draft: bool,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Story {
    pub id: Id,
    pub title: String,
    #[serde(default)]
    pub pages: Vec<Page>,
    pub music: Option<Id>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub draft: bool,
}

/// A story page: one image and some text.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Page {
    pub art: Option<Id>,
    pub text: String,
    /// A voice reading the page: a dialogue sound event (D97).
    pub narration: Option<Id>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct MissionStep {
    pub id: Id,
    /// A map id among the package's `missions/`; a draft may have none yet.
    pub map: Option<Id>,
    pub title: String,
    /// The small line above the title (`Prologue · Mission One`).
    pub chapter_line: Option<String>,
    /// The loading screen's illustration.
    pub art: Option<Id>,
    /// Paragraphs of the loading screen, separated by blank lines.
    pub story: Option<String>,
    /// Carried and placed heroes reach at most this level.
    pub hero_level_cap: Option<u32>,
    #[serde(default)]
    pub tech: Tech,
    pub music: Option<Id>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub draft: bool,
}

/// What a mission changes in the campaign race's tech.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Tech {
    /// Unlocked from this mission on, for the rest of the campaign.
    #[serde(default)]
    pub new: Vec<Id>,
    /// Taken away in this mission only.
    #[serde(default)]
    pub blocked: Vec<Id>,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cinematic {
    pub id: Id,
    pub title: String,
    /// What it will show.
    pub note: Option<String>,
    pub music: Option<Id>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub draft: bool,
}

/// The early access line: its card's words are the game's own.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EarlyAccess {
    pub id: Id,
}

impl Step {
    pub fn id(&self) -> &Id {
        match self {
            Self::Chapter(s) => &s.id,
            Self::Story(s) => &s.id,
            Self::Mission(s) => &s.id,
            Self::Cinematic(s) => &s.id,
            Self::EarlyAccess(s) => &s.id,
        }
    }

    /// Its title, a text key or words; the early access line has none of its own.
    pub fn title(&self) -> &str {
        match self {
            Self::Chapter(s) => &s.title,
            Self::Story(s) => &s.title,
            Self::Mission(s) => &s.title,
            Self::Cinematic(s) => &s.title,
            Self::EarlyAccess(_) => "",
        }
    }

    /// Only outlined: never played or shipped.
    pub fn draft(&self) -> bool {
        match self {
            Self::Chapter(s) => s.draft,
            Self::Story(s) => s.draft,
            Self::Mission(s) => s.draft,
            Self::Cinematic(s) => s.draft,
            Self::EarlyAccess(_) => false,
        }
    }

    pub fn music(&self) -> Option<&Id> {
        match self {
            Self::Chapter(s) => s.music.as_ref(),
            Self::Story(s) => s.music.as_ref(),
            Self::Mission(s) => s.music.as_ref(),
            Self::Cinematic(s) => s.music.as_ref(),
            Self::EarlyAccess(_) => None,
        }
    }

    /// `chapter`, `story`, `mission`, `cinematic` or `earlyAccess`.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Chapter(_) => "chapter",
            Self::Story(_) => "story",
            Self::Mission(_) => "mission",
            Self::Cinematic(_) => "cinematic",
            Self::EarlyAccess(_) => "earlyAccess",
        }
    }

    pub fn mission(&self) -> Option<&MissionStep> {
        match self {
            Self::Mission(m) => Some(m),
            _ => None,
        }
    }
}

impl MissionStep {
    /// Its map's id; a draft may have none yet.
    pub fn map_id(&self) -> Option<&str> {
        self.map.as_ref().map(Id::as_str)
    }
}

/// The paragraphs of a story text: blocks separated by blank lines, trimmed.
pub fn paragraphs(text: &str) -> Vec<String> {
    text.replace("\r\n", "\n")
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect()
}

impl Campaign {
    /// The campaign as it plays: without its draft steps.
    pub fn playable(&self) -> Campaign {
        let mut campaign = self.clone();
        campaign.steps.retain(|s| !s.draft());
        campaign
    }

    /// The index of the early access line, if this campaign holds it.
    pub fn early_access(&self) -> Option<usize> {
        self.steps
            .iter()
            .position(|s| matches!(s, Step::EarlyAccess(_)))
    }

    /// The index of the step with `id`.
    pub fn position(&self, id: &str) -> Option<usize> {
        self.steps.iter().position(|s| s.id().as_str() == id)
    }

    /// The tech everything up to step `index` has unlocked: every mission's `new` up to and
    /// including it.
    pub fn unlocked(&self, index: usize) -> BTreeSet<Id> {
        self.steps
            .iter()
            .take(index + 1)
            .filter_map(Step::mission)
            .flat_map(|m| m.tech.new.iter().cloned())
            .collect()
    }

    /// What player 1 may not build, train or research in the mission at step `index`: the
    /// race's tech no mission so far unlocked, plus this mission's blocked tech.
    pub fn locked(&self, content: &Content, index: usize) -> BTreeSet<Id> {
        let unlocked = self.unlocked(index);
        let mut locked: BTreeSet<Id> = race_tech(content, &self.race)
            .into_iter()
            .filter(|id| !unlocked.contains(id))
            .collect();
        if let Some(m) = self.steps.get(index).and_then(Step::mission) {
            locked.extend(m.tech.blocked.iter().cloned());
        }
        locked
    }
}

/// A race's tech: its roster (units and buildings), what its buildings train and research
/// and the buildings they upgrade into. What a mission may unlock or block.
pub fn race_tech(content: &Content, race: &str) -> BTreeSet<Id> {
    let Some(race) = content.rules.races.get(race) else {
        return BTreeSet::new();
    };
    let mut tech: BTreeSet<Id> = race.roster.iter().cloned().collect();
    for id in &race.roster {
        let Some(d) = content.definition(id.as_str()) else {
            continue;
        };
        if let Some(p) = &d.behaviors.production
            && p.mode == ProductionMode::Queued
        {
            tech.extend(p.outputs.iter().cloned());
        }
        if let Some(r) = &d.behaviors.research {
            tech.extend(r.outputs.iter().cloned());
        }
        if let Some(u) = &d.upgrade {
            tech.insert(u.target.clone());
        }
    }
    tech
}

/// A campaign package: its manifest, its campaign and its mission maps.
#[derive(Debug, Clone)]
pub struct CampaignPackage {
    pub package: Package,
    pub campaign: Campaign,
    /// The maps in `missions/`, sorted by id.
    pub missions: Vec<MapFile>,
}

impl CampaignPackage {
    pub fn id(&self) -> &str {
        self.package.id()
    }

    pub fn name(&self) -> &str {
        &self.package.manifest.name
    }

    /// The mission map with `id`.
    pub fn mission_map(&self, id: &str) -> Option<&MapFile> {
        self.missions.iter().find(|m| m.id == id)
    }

    /// The asset ids the package's own `assets/` holds.
    pub fn asset_ids(&self) -> BTreeSet<String> {
        let mut errors = Vec::new();
        crate::assets::package::index(
            std::slice::from_ref(&self.package),
            crate::assets::Files::Skip,
            &mut errors,
        )
        .assets
        .into_iter()
        .map(|a| a.id.to_string())
        .collect()
    }

    /// The campaign's own sound events (its `sounds/`), which its steps may name beside the
    /// base's (D97).
    pub fn sounds(&self) -> crate::sounds::Sounds {
        let mut errors = Vec::new();
        let elements = crate::load::elements(
            &crate::collections::SOUNDS,
            std::slice::from_ref(&self.package),
            &mut Default::default(),
            &mut errors,
        );
        crate::sounds::Sounds::new(
            elements
                .into_iter()
                .filter_map(|e| e.parse(&mut errors))
                .collect(),
        )
    }

    /// Whether the package changes game data (units, abilities, rules…), so its missions
    /// play on content layered over the base.
    pub fn has_content(&self) -> bool {
        has_content(&self.package)
    }
}

/// Whether a package holds game data beyond its own manifest, art and maps.
pub fn has_content(package: &Package) -> bool {
    use crate::collections::*;
    [
        UNITS, BUILDINGS, ITEMS, RESOURCES, BEHAVIORS, RACES, RESEARCH, LOOT_POOLS, ABILITIES,
        EFFECTS, SOUNDS, RULES,
    ]
    .iter()
    .any(|c| package.is_dir(c.dir))
        // Its own text (a mission's dialogue lines, D96) plays with it too.
        || package.is_dir(&crate::text::folder(crate::text::SOURCE_LANGUAGE))
}

/// Opens the campaign package at `dir`: its manifest, `campaign.json` and its maps. The
/// campaign is checked against content with [`validate`].
pub fn open(dir: &Path) -> Result<CampaignPackage, Errors> {
    let one = |e: Error| Errors(vec![e]);
    let package = Package::open(dir).map_err(one)?;
    if package.manifest.kind != PackageKind::Campaign {
        return Err(one(Error::new(
            &package.label(crate::package::MANIFEST),
            "kind",
            "not a campaign package",
        )));
    }
    let file = package.label(CAMPAIGN_FILE);
    let text = package
        .read_text(CAMPAIGN_FILE)
        .map_err(|e| one(Error::new(&file, "", e.to_string())))?;
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| one(Error::new(&file, "", e.to_string())))?;
    let campaign: Campaign = parse(value, &file, "").map_err(one)?;
    let missions = map::folder(&dir.join(MISSIONS_DIR), "campaign")?;
    Ok(CampaignPackage {
        package,
        campaign,
        missions,
    })
}

/// The campaign packages below `dir` (`<id>.utcc` folders), sorted by id.
pub fn folder(dir: &Path) -> Result<Vec<CampaignPackage>, Errors> {
    let mut found = Vec::new();
    let mut errors = Vec::new();
    if dir.is_dir() {
        let mut paths: Vec<PathBuf> = std::fs::read_dir(dir)
            .map_err(|e| {
                Errors(vec![Error::new(
                    &dir.display().to_string(),
                    "",
                    e.to_string(),
                )])
            })?
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.is_dir()
                    && p.extension()
                        .is_some_and(|e| e == crate::package::CAMPAIGN_EXTENSION)
            })
            .collect();
        paths.sort();
        for path in paths {
            match open(&path) {
                Ok(campaign) => found.push(campaign),
                Err(e) => errors.extend(e.0),
            }
        }
    }
    found.sort_by(|a: &CampaignPackage, b| a.id().cmp(b.id()));
    for pair in found.windows(2) {
        if pair[0].id() == pair[1].id() {
            errors.push(Error::new(
                &pair[1].package.label(""),
                "id",
                "another campaign has this id",
            ));
        }
    }
    let lines: Vec<&CampaignPackage> = found
        .iter()
        .filter(|c| c.campaign.early_access().is_some())
        .collect();
    for extra in lines.iter().skip(1) {
        errors.push(Error::new(
            &extra.package.label(CAMPAIGN_FILE),
            "steps",
            format!(
                "the early access line is already in {}; the game has one",
                lines[0].id()
            ),
        ));
    }
    if errors.is_empty() {
        Ok(found)
    } else {
        Err(Errors(errors))
    }
}

/// The campaigns in the order they play: by `order`, then by id.
pub fn in_play_order<T: std::borrow::Borrow<CampaignPackage>>(campaigns: &mut [T]) {
    campaigns.sort_by(|a, b| {
        let (a, b) = (a.borrow(), b.borrow());
        (a.campaign.order.unwrap_or(u32::MAX), a.id())
            .cmp(&(b.campaign.order.unwrap_or(u32::MAX), b.id()))
    });
}

/// The shipped campaigns (`base/campaigns/` in the data folder).
pub fn library(data: &Path) -> Result<Vec<CampaignPackage>, Errors> {
    folder(&data.join(CAMPAIGNS_FOLDER))
}

/// Checks a campaign against `content` (the base, or the base with the campaign layered):
/// unique step ids, mission maps among the package's `missions/`, tech of the campaign's
/// race, and art and sounds that exist (in `content` or the package's own `assets/`).
pub fn validate(campaign: &CampaignPackage, content: &Content) -> Vec<Error> {
    let file = campaign.package.label(CAMPAIGN_FILE);
    let c = &campaign.campaign;
    let mut errors = Vec::new();
    let mut fail = |location: String, message: String| {
        errors.push(Error::new(&file, location, message));
    };
    let own_assets = campaign.asset_ids();
    let asset_exists = |id: &Id| {
        own_assets.contains(id.as_str())
            || content
                .manifest
                .records
                .iter()
                .any(|r| r.id.as_str() == id.as_str())
            || content.asset(id.as_str()).is_some()
    };
    let check_asset = |fail: &mut dyn FnMut(String, String), at: String, id: &Option<Id>| {
        if let Some(id) = id
            && !asset_exists(id)
        {
            fail(at, format!("unknown asset {id}"));
        }
    };
    // Fields hold library keys (D96): the base text with the campaign's own over it.
    let mut library = content.text.clone();
    library.layer(std::slice::from_ref(&campaign.package), &mut Vec::new());
    let own_sounds = campaign.sounds();
    let check_sound = |fail: &mut dyn FnMut(String, String),
                       at: String,
                       id: Option<&Id>,
                       category: crate::sounds::SoundCategory| {
        let Some(id) = id else { return };
        match own_sounds
            .get(id.as_str())
            .or_else(|| content.sounds.get(id.as_str()))
        {
            None => fail(at, format!("unknown sound {id}")),
            Some(e) if e.category != category => {
                fail(at, format!("{id} is no {category:?} sound").to_lowercase())
            }
            Some(_) => {}
        }
    };
    let text = |fail: &mut dyn FnMut(String, String), at: String, value: &str, most: usize| {
        let value = match library.get(value) {
            Some(said) => said,
            None if crate::text::looks_like_key(value) => {
                return fail(at, format!("names {value}, which no text file holds"));
            }
            None => value,
        };
        let length = value.chars().count();
        if value.trim().is_empty() || length > most {
            fail(at, format!("must be 1 to {most} characters"));
        }
    };
    if !content.rules.races.contains_key(&c.race) {
        fail("race".into(), format!("unknown race {}", c.race));
    }
    check_asset(&mut fail, "background".into(), &c.background);
    check_asset(&mut fail, "card".into(), &c.card);
    if c.steps.is_empty() {
        fail("steps".into(), "a campaign needs at least one step".into());
    }
    let lines = c
        .steps
        .iter()
        .filter(|s| matches!(s, Step::EarlyAccess(_)))
        .count();
    if lines > 1 {
        fail(
            "steps".into(),
            "the early access line may stand only once".into(),
        );
    }
    if lines > 0 && c.draft {
        fail(
            "draft".into(),
            "a draft campaign cannot hold the early access line".into(),
        );
    }
    // Outlines (D110): one per step, named by its id (unsaved ones too, through the
    // package's overlay).
    let mut names: Vec<String> = campaign
        .package
        .entries(OUTLINES_DIR)
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, dir)| !dir)
        .map(|(name, _)| name)
        .collect();
    names.sort();
    for name in names {
        let Some(step) = name.strip_suffix(".json") else {
            continue;
        };
        let path = crate::outline::path(step);
        let label = campaign.package.label(&path);
        if c.position(step).is_none() {
            errors.push(Error::new(&label, "", format!("no step {step} to outline")));
        }
        let read = campaign
            .package
            .read_text(&path)
            .map_err(|e| e.to_string())
            .and_then(|t| {
                serde_json::from_str::<crate::outline::Outline>(&t).map_err(|e| e.to_string())
            });
        match read {
            Ok(outline) => errors.extend(
                crate::outline::problems(&outline)
                    .into_iter()
                    .map(|p| Error::new(&label, "", p)),
            ),
            Err(e) => errors.push(Error::new(&label, "", e)),
        }
    }
    let mut fail = |location: String, message: String| {
        errors.push(Error::new(&file, location, message));
    };
    let tech = race_tech(content, &c.race);
    let mut ids = BTreeSet::new();
    for (i, step) in c.steps.iter().enumerate() {
        let at = format!("steps[{i}]");
        if !ids.insert(step.id().as_str()) {
            fail(
                format!("{at}.id"),
                format!("step id {} is used twice", step.id()),
            );
        }
        if !matches!(step, Step::EarlyAccess(_)) {
            text(&mut fail, format!("{at}.title"), step.title(), 120);
        }
        check_sound(
            &mut fail,
            format!("{at}.music"),
            step.music(),
            crate::sounds::SoundCategory::Music,
        );
        match step {
            Step::Chapter(s) => check_asset(&mut fail, format!("{at}.art"), &s.art),
            Step::EarlyAccess(_) => {}
            Step::Story(s) => {
                if (s.pages.is_empty() && !s.draft) || s.pages.len() > 32 {
                    fail(format!("{at}.pages"), "a story has 1 to 32 pages".into());
                }
                for (p, page) in s.pages.iter().enumerate() {
                    let at = format!("{at}.pages[{p}]");
                    if !(s.draft && page.text.trim().is_empty()) {
                        text(&mut fail, format!("{at}.text"), &page.text, 4000);
                    }
                    check_asset(&mut fail, format!("{at}.art"), &page.art);
                    check_sound(
                        &mut fail,
                        format!("{at}.narration"),
                        page.narration.as_ref(),
                        crate::sounds::SoundCategory::Dialogue,
                    );
                }
            }
            Step::Cinematic(s) => {
                if let Some(note) = &s.note {
                    text(&mut fail, format!("{at}.note"), note, 1000);
                }
            }
            Step::Mission(m) => {
                match &m.map {
                    Some(map) if campaign.mission_map(map.as_str()).is_none() => fail(
                        format!("{at}.map"),
                        format!("no map {map} in {MISSIONS_DIR}/"),
                    ),
                    None if !m.draft => fail(
                        format!("{at}.map"),
                        "a mission needs its map; only a draft may have none yet".into(),
                    ),
                    _ => {}
                }
                check_asset(&mut fail, format!("{at}.art"), &m.art);
                if let Some(line) = &m.chapter_line {
                    text(&mut fail, format!("{at}.chapterLine"), line, 120);
                }
                if let Some(story) = &m.story {
                    text(&mut fail, format!("{at}.story"), story, 4000);
                }
                if let Some(cap) = m.hero_level_cap
                    && !(1..=MAX_LEVEL_CAP).contains(&cap)
                {
                    fail(
                        format!("{at}.heroLevelCap"),
                        format!("must be 1 to {MAX_LEVEL_CAP}"),
                    );
                }
                for (list, ids) in [("new", &m.tech.new), ("blocked", &m.tech.blocked)] {
                    let mut seen = BTreeSet::new();
                    for id in ids {
                        let location = format!("{at}.tech.{list}");
                        if !seen.insert(id) {
                            fail(location.clone(), format!("{id} is listed twice"));
                        }
                        let known = content.definition(id.as_str()).is_some_and(|d| {
                            matches!(d.kind, Kind::Unit | Kind::Building | Kind::Item)
                        }) || content.rules.research.contains_key(id);
                        if !known {
                            fail(location, format!("unknown tech {id}"));
                        } else if !tech.contains(id) {
                            fail(location, format!("{id} is not tech of the {} race", c.race));
                        }
                    }
                }
                if let Some(both) = m.tech.new.iter().find(|id| m.tech.blocked.contains(id)) {
                    fail(
                        format!("{at}.tech"),
                        format!("{both} is both new and blocked"),
                    );
                }
            }
        }
    }
    errors
}

/// What a release leaves out of the staged campaigns ([`release`]).
#[derive(Debug, Default, PartialEq)]
pub struct Trimmed {
    /// Campaigns removed whole: drafts and those after the early access line's.
    pub campaigns: Vec<String>,
    /// `campaign/step` of every step removed: drafts and steps after the line.
    pub steps: Vec<String>,
    /// `campaign/map` of every mission map removed with its steps.
    pub maps: Vec<String>,
    /// Every asset id what was removed names (its art), sorted: the cook leaves out of a
    /// release's pack those no shipped document still names.
    pub assets: Vec<String>,
}

/// Trims the campaign packages below `dir` (a staged copy of `base/campaigns`) to what a
/// release ships (D110): no draft campaigns, no campaigns after the one holding the early
/// access line, no draft steps, no steps after the line, no mission maps and text only
/// removed steps name, and no outlines. The line itself stays, so the game shows its card.
pub fn release(dir: &Path) -> Result<Trimmed, Errors> {
    let io = |path: &Path, e: std::io::Error| {
        Errors(vec![Error::new(
            &path.display().to_string(),
            "",
            e.to_string(),
        )])
    };
    let mut campaigns = folder(dir)?;
    in_play_order(&mut campaigns);
    let line = campaigns
        .iter()
        .position(|c| c.campaign.early_access().is_some());
    let mut trimmed = Trimmed::default();
    let mut assets = BTreeSet::new();
    for (index, package) in campaigns.iter().enumerate() {
        let root = package.package.root.clone();
        let id = package.id().to_string();
        if package.campaign.draft || line.is_some_and(|line| index > line) {
            named_assets_below(&root, &mut assets);
            std::fs::remove_dir_all(&root).map_err(|e| io(&root, e))?;
            trimmed.campaigns.push(id);
            continue;
        }
        let cut = package
            .campaign
            .early_access()
            .map_or(usize::MAX, |at| at + 1);
        let keep: Vec<bool> = package
            .campaign
            .steps
            .iter()
            .enumerate()
            .map(|(i, s)| i < cut && !s.draft())
            .collect();
        let removed: Vec<&Step> = package
            .campaign
            .steps
            .iter()
            .zip(&keep)
            .filter(|(_, keep)| !**keep)
            .map(|(s, _)| s)
            .collect();
        trimmed
            .steps
            .extend(removed.iter().map(|s| format!("{id}/{}", s.id())));
        // campaign.json without the removed steps, the rest as written.
        let file = root.join(CAMPAIGN_FILE);
        let text = std::fs::read_to_string(&file).map_err(|e| io(&file, e))?;
        let mut value: serde_json::Value = serde_json::from_str(&text).map_err(|e| {
            Errors(vec![Error::new(
                &file.display().to_string(),
                "",
                e.to_string(),
            )])
        })?;
        if let Some(steps) = value
            .get_mut("steps")
            .and_then(serde_json::Value::as_array_mut)
        {
            for (step, keep) in steps.iter().zip(&keep) {
                if !keep {
                    named_assets(step, &mut assets);
                }
            }
            let mut flags = keep.iter();
            steps.retain(|_| *flags.next().unwrap_or(&false));
        }
        write_json(&file, &value).map_err(|e| io(&file, e))?;
        // Maps no kept step plays.
        let played: BTreeSet<&str> = package
            .campaign
            .steps
            .iter()
            .zip(&keep)
            .filter(|(_, keep)| **keep)
            .filter_map(|(s, _)| s.mission().and_then(|m| m.map.as_ref()))
            .map(Id::as_str)
            .collect();
        for map in &package.missions {
            if !played.contains(map.id.as_str()) {
                let path = &map.path;
                if path.is_dir() {
                    named_assets_below(path, &mut assets);
                    std::fs::remove_dir_all(path).map_err(|e| io(path, e))?;
                    trimmed.maps.push(format!("{id}/{}", map.id));
                }
            }
        }
        // The removed steps' words: `campaign.<campaign>.<step>.*` in every language.
        let prefixes: Vec<String> = removed
            .iter()
            .map(|s| format!("campaign.{id}.{}.", s.id()))
            .collect();
        let text_dir = root.join("text");
        if text_dir.is_dir() && !prefixes.is_empty() {
            for language in std::fs::read_dir(&text_dir)
                .map_err(|e| io(&text_dir, e))?
                .flatten()
            {
                let Ok(files) = std::fs::read_dir(language.path()) else {
                    continue;
                };
                for file in files.flatten().map(|f| f.path()) {
                    if file.extension().is_none_or(|e| e != "json") {
                        continue;
                    }
                    let text = std::fs::read_to_string(&file).map_err(|e| io(&file, e))?;
                    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&text) else {
                        continue;
                    };
                    if let Some(words) = value.as_object_mut() {
                        let before = words.len();
                        words
                            .retain(|key, _| !prefixes.iter().any(|p| key.starts_with(p.as_str())));
                        if words.len() != before {
                            write_json(&file, &value).map_err(|e| io(&file, e))?;
                        }
                    }
                }
            }
        }
        let outlines = root.join(OUTLINES_DIR);
        if outlines.is_dir() {
            std::fs::remove_dir_all(&outlines).map_err(|e| io(&outlines, e))?;
        }
    }
    trimmed.assets = assets.into_iter().collect();
    Ok(trimmed)
}

/// Collects every string in `value` that is an asset id.
pub fn named_assets(value: &serde_json::Value, into: &mut BTreeSet<String>) {
    match value {
        serde_json::Value::String(s) if s.starts_with("asset.") => {
            into.insert(s.clone());
        }
        serde_json::Value::Array(items) => items.iter().for_each(|v| named_assets(v, into)),
        serde_json::Value::Object(fields) => fields.values().for_each(|v| named_assets(v, into)),
        _ => {}
    }
}

/// Collects the asset ids the JSON files below `dir` name, their own asset folders' too.
fn named_assets_below(dir: &Path, into: &mut BTreeSet<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for path in entries.flatten().map(|e| e.path()) {
        if path.is_dir() {
            named_assets_below(&path, into);
        } else if path.extension().is_some_and(|e| e == "json")
            && let Ok(text) = std::fs::read_to_string(&path)
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&text)
        {
            named_assets(&value, into);
        }
    }
}

fn write_json(path: &Path, value: &serde_json::Value) -> std::io::Result<()> {
    let mut text = serde_json::to_string_pretty(value).map_err(std::io::Error::other)?;
    text.push('\n');
    std::fs::write(path, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn steps_read_by_kind_and_refuse_unknown_fields() {
        let campaign: Campaign = serde_json::from_value(json!({
            "race": "ants",
            "steps": [
                {"kind": "chapter", "id": "prologue", "title": "Prologue"},
                {"kind": "story", "id": "hollow", "title": "The Hollow",
                 "pages": [{"text": "Once."}]},
                {"kind": "mission", "id": "brood", "map": "brood", "title": "Brood",
                 "heroLevelCap": 3, "tech": {"new": ["unit.a"], "blocked": []}},
                {"kind": "mission", "id": "second", "map": "second", "title": "Second",
                 "tech": {"new": ["unit.b"], "blocked": ["unit.a"]}},
                {"kind": "cinematic", "id": "hums", "title": "Hums", "note": "Later."}
            ]
        }))
        .unwrap();
        assert_eq!(campaign.steps.len(), 5);
        assert_eq!(campaign.position("brood"), Some(2));
        assert_eq!(campaign.steps[4].kind(), "cinematic");
        let unlocked: Vec<_> = campaign.unlocked(3).into_iter().collect();
        assert_eq!(unlocked.len(), 2);
        assert!(
            serde_json::from_value::<Campaign>(json!({
                "race": "ants",
                "steps": [{"kind": "chapter", "id": "a", "title": "A", "nextMission": "b"}]
            }))
            .is_err()
        );
        assert_eq!(
            paragraphs("One.\n\n  Two,\nstill two.\n\n\n"),
            vec!["One.".to_string(), "Two,\nstill two.".to_string()]
        );
    }
}
