//! Sound events (D97): what content names when something should be heard. An event is one
//! file, `sounds/<free folders>/<name>.json` with the id `sound.<scope>.<name>`: its clips
//! (audio assets, one picked each time), the ranges its volume and pitch vary in, how far
//! it carries, its category and how often it may start. Definitions, races, rules, biomes,
//! maps and campaigns name events, never clips, so a clip is polished or swapped without
//! touching what plays it. Everything here is presentation: nothing reaches the simulation.

use crate::error::Error;
use crate::id::Id;
use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;
use std::collections::BTreeMap;

/// One sound event.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SoundEvent {
    pub id: Id,
    /// What it should sound like, for authors and the generation tool; never shown.
    pub note: Option<String>,
    pub category: SoundCategory,
    /// Audio assets; each start picks one at random, never the one before when it has more.
    pub clips: Vec<Id>,
    /// Gain, 0–2, or a `[min, max]` range each start picks in (default 1).
    #[serde(default = "Range::one")]
    pub volume: Range,
    /// Playback rate, 0.25–4, or a range (default 1).
    #[serde(default = "Range::one")]
    pub pitch: Range,
    /// Loops until what plays it ends (a building's work, a stream, a track).
    #[serde(default, rename = "loop")]
    pub looping: bool,
    /// How far a world or ambience sound carries; the category's default without it.
    pub distance: Option<Distance>,
    /// Least seconds between two starts of this event anywhere (0: none).
    #[serde(default)]
    pub cooldown: f64,
    /// Most copies heard at once; the category's default without it.
    pub limit: Option<u32>,
    /// Seconds a loop fades in and out (default 0.25).
    pub fade: Option<f64>,
}

/// What kind of sound, which decides its bus, its clock, whether it is placed in the world
/// and its voice budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum SoundCategory {
    /// Placed in the world: steps, blows, buildings at work, deaths (game time).
    World,
    /// A unit answering its commanding player: not placed, one at a time (game time).
    Voice,
    /// The interface and a race's spoken alerts (wall clock).
    Interface,
    /// Loops of the place: a biome's air, streams and falls (game time).
    Ambience,
    /// Tracks of a playlist, streamed (wall clock).
    Music,
    /// Spoken story: a dialogue line's voice, a page's narration (wall clock, streamed).
    Dialogue,
}

impl SoundCategory {
    /// Whether its sounds have a place in the world (and fall off with distance).
    pub fn placed(self) -> bool {
        matches!(self, Self::World | Self::Ambience)
    }

    /// Whether its clips are long enough to stream rather than sit decoded in memory.
    pub fn streamed(self) -> bool {
        matches!(self, Self::Music | Self::Dialogue)
    }
}

/// A fixed value or a `[min, max]` range.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(untagged)]
pub enum Range {
    Fixed(f64),
    Between([f64; 2]),
}

impl Range {
    fn one() -> Self {
        Self::Fixed(1.0)
    }

    pub fn bounds(self) -> (f64, f64) {
        match self {
            Self::Fixed(v) => (v, v),
            Self::Between([a, b]) => (a, b),
        }
    }

    /// The value at `t` in 0..1 of the range.
    pub fn at(self, t: f64) -> f64 {
        let (a, b) = self.bounds();
        a + (b - a) * t
    }
}

/// Full volume up to `reference` cells from the listener, silent at `max` (the old game's
/// quadratic falloff, D29).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Distance {
    pub reference: f64,
    pub max: f64,
}

/// A sound a unit or building makes: looping while it is in `state` (the states of D65's
/// effects), once whenever its clip crosses the animation `event`, or once at a moment
/// `on`. Exactly one of the three. It sounds from the model's origin.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SoundBinding {
    pub sound: Id,
    pub state: Option<String>,
    pub event: Option<String>,
    pub on: Option<Moment>,
}

/// Moments of a unit's or building's life a sound can mark.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Moment {
    /// It appears: trained, summoned, revived, or a building placed as a site.
    Spawn,
    /// A building's construction or upgrade is finished.
    Complete,
    /// It dies, or a building falls.
    Death,
    /// Its blow or missile lands on its target (heard at the target).
    Impact,
}

/// How a unit answers its commanding player, as Warcraft III's units do: each names a
/// voice event whose clips are its lines.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Voice {
    /// Selected.
    pub select: Option<Id>,
    /// Ordered to move, gather, build or follow.
    pub order: Option<Id>,
    /// Ordered to attack.
    pub attack: Option<Id>,
    /// Selected again and again in quick succession.
    pub annoyed: Option<Id>,
    /// Trained or revived.
    pub ready: Option<Id>,
}

impl Voice {
    pub fn events(&self) -> impl Iterator<Item = (&'static str, &Id)> {
        [
            ("select", &self.select),
            ("order", &self.order),
            ("attack", &self.attack),
            ("annoyed", &self.annoyed),
            ("ready", &self.ready),
        ]
        .into_iter()
        .filter_map(|(name, id)| id.as_ref().map(|id| (name, id)))
    }
}

/// A race's sounds: its spoken alerts and its music in skirmish.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RaceSounds {
    /// What a player of the race hears when a message reaches them, by the message's text
    /// key (`message.research-completed`, `alert.under-attack`).
    #[serde(default)]
    pub alerts: BTreeMap<String, Id>,
    /// Music events, shuffled, for the race's player in a skirmish.
    #[serde(default)]
    pub music: Vec<Id>,
}

/// `rules/sounds.json`: the game's own sounds.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SoundRules {
    /// The main menu's music, shuffled.
    #[serde(default)]
    pub menu: Vec<Id>,
    /// Stings when a match is won or lost.
    pub victory: Option<Id>,
    pub defeat: Option<Id>,
    /// Interface sounds by what they mark: `click`, `refused`, `objective`, `message`…
    #[serde(default)]
    pub interface: BTreeMap<String, Id>,
}

/// Default distances by category, cells (world sounds carry about a screen).
pub fn default_distance(category: SoundCategory) -> Distance {
    match category {
        SoundCategory::Ambience => Distance {
            reference: 8.0,
            max: 48.0,
        },
        _ => Distance {
            reference: 12.0,
            max: 64.0,
        },
    }
}

/// The library of sound events, sorted by id.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Sounds {
    events: Vec<SoundEvent>,
}

impl Sounds {
    pub fn new(mut events: Vec<SoundEvent>) -> Self {
        events.sort_by(|a, b| a.id.cmp(&b.id));
        Self { events }
    }

    pub fn get(&self, id: &str) -> Option<&SoundEvent> {
        self.events
            .binary_search_by(|e| e.id.as_str().cmp(id))
            .ok()
            .map(|i| &self.events[i])
    }

    pub fn iter(&self) -> impl Iterator<Item = &SoundEvent> {
        self.events.iter()
    }

    pub fn len(&self) -> usize {
        self.events.len()
    }

    pub fn is_empty(&self) -> bool {
        self.events.is_empty()
    }
}

/// The rules of one event on its own; `audio` says whether an asset id is an audio asset
/// with a clip.
pub(crate) fn check_event(event: &SoundEvent, audio: &dyn Fn(&str) -> bool) -> Vec<String> {
    let mut problems = Vec::new();
    let mut require = |ok: bool, message: String| {
        if !ok {
            problems.push(message);
        }
    };
    require(
        event.id.as_str().starts_with("sound."),
        "sound ids start with sound.".into(),
    );
    require(
        !event.clips.is_empty(),
        "a sound needs at least one clip".into(),
    );
    for clip in &event.clips {
        require(
            audio(clip.as_str()),
            format!("{clip} is no audio asset with a clip"),
        );
    }
    for (name, range, min, max) in [
        ("volume", event.volume, 0.0, 2.0),
        ("pitch", event.pitch, 0.25, 4.0),
    ] {
        let (a, b) = range.bounds();
        require(
            (min..=max).contains(&a) && (min..=max).contains(&b) && a <= b,
            format!("{name} must be {min} to {max}, low before high"),
        );
    }
    if let Some(d) = event.distance {
        require(
            event.category.placed(),
            "only world and ambience sounds have a distance".into(),
        );
        require(
            d.reference >= 0.0 && d.max > d.reference && d.max <= 512.0,
            "distance needs 0 ≤ reference < max ≤ 512".into(),
        );
    }
    require(
        (0.0..=600.0).contains(&event.cooldown),
        "cooldown must be 0 to 600 seconds".into(),
    );
    require(
        event.limit.is_none_or(|l| (1..=64).contains(&l)),
        "limit must be 1 to 64".into(),
    );
    require(
        event.fade.is_none_or(|f| (0.0..=30.0).contains(&f)),
        "fade must be 0 to 30 seconds".into(),
    );
    problems
}

/// Checks that `id` names an event of one of `categories`, reporting at `file`/`location`.
pub(crate) fn check_reference(
    sounds: &Sounds,
    id: &Id,
    categories: &[SoundCategory],
    (file, location): (&str, &str),
    errors: &mut Vec<Error>,
) {
    match sounds.get(id.as_str()) {
        None => errors.push(Error::new(file, location, format!("unknown sound {id}"))),
        Some(event) if !categories.contains(&event.category) => errors.push(Error::new(
            file,
            location,
            format!(
                "{id} is a {:?} sound; this wants {}",
                event.category,
                categories
                    .iter()
                    .map(|c| format!("{c:?}").to_lowercase())
                    .collect::<Vec<_>>()
                    .join(" or ")
            ),
        )),
        Some(_) => {}
    }
}

/// Checks every sound event and everything that names one: definitions' sounds and voices,
/// races' alerts and music, and `rules/sounds.json`.
pub(crate) fn validate(content: &crate::Content, errors: &mut Vec<Error>) {
    use crate::assets::{FileRole, landscape::AssetKind};
    use crate::game::{Kind, StateEffect};
    use SoundCategory::*;
    let audio = |id: &str| {
        content.assets.iter().any(|a| {
            a.id.as_str() == id
                && a.kind == AssetKind::Audio
                && a.resources.iter().any(|r| r.role == FileRole::Audio)
        })
    };
    for event in content.sounds.iter() {
        let file = content
            .files
            .get(event.id.as_str())
            .map_or("sounds", String::as_str);
        for problem in check_event(event, &audio) {
            errors.push(Error::new(file, event.id.as_str(), problem));
        }
        // A spoken line lasts as long as its voice, so a voice's clips say how long they are.
        if event.category == Dialogue {
            for clip in &event.clips {
                let known = content
                    .assets
                    .iter()
                    .find(|a| a.id == *clip.as_str())
                    .is_none_or(|a| a.seconds.is_some_and(|s| s > 0.0));
                if !known {
                    errors.push(Error::new(
                        file,
                        event.id.as_str(),
                        format!("dialogue clip {clip} needs its length in seconds (`seconds`)"),
                    ));
                }
            }
        }
    }
    let mut refer = |id: &Id, categories: &[SoundCategory], location: String| {
        let file = content.origin(&location).to_string();
        check_reference(&content.sounds, id, categories, (&file, &location), errors);
    };
    let mut wrong = Vec::new();
    for d in &content.definitions {
        let Some(p) = &d.presentation else { continue };
        let at = d.id.as_str();
        let states = match d.kind {
            Kind::Building => StateEffect::BUILDING_STATES,
            Kind::Unit => StateEffect::UNIT_STATES,
            _ => &[],
        };
        for (i, binding) in p.sounds.iter().flatten().enumerate() {
            let location = format!("{at}.presentation.sounds[{i}]");
            refer(&binding.sound, &[World, Ambience], location.clone());
            let ok = match (&binding.state, &binding.event, binding.on) {
                (Some(state), None, None) => states.contains(&state.as_str()),
                (None, Some(event), None) => !event.is_empty(),
                (None, None, Some(_)) => true,
                _ => false,
            };
            if !ok {
                wrong.push((
                    location,
                    format!(
                        "a sound binding names one of a state ({}), a clip event or a moment",
                        states.join(", ")
                    ),
                ));
            }
        }
        if let Some(voice) = &p.voice {
            if d.kind != Kind::Unit {
                wrong.push((
                    format!("{at}.presentation.voice"),
                    "only units answer".into(),
                ));
            }
            for (name, id) in voice.events() {
                refer(id, &[Voice], format!("{at}.presentation.voice.{name}"));
            }
        }
    }
    for (race_id, race) in &content.rules.races {
        let Some(sounds) = &race.sounds else { continue };
        for (key, id) in &sounds.alerts {
            refer(
                id,
                &[Voice, Interface],
                format!("rules.races.{race_id}.sounds.alerts.{key}"),
            );
            if crate::text::check_key(key).is_err() {
                wrong.push((
                    format!("rules.races.{race_id}.sounds.alerts"),
                    format!("{key} is no text key"),
                ));
            }
        }
        for (i, id) in sounds.music.iter().enumerate() {
            refer(
                id,
                &[Music],
                format!("rules.races.{race_id}.sounds.music[{i}]"),
            );
        }
    }
    let rules = &content.rules.sounds;
    for (i, id) in rules.menu.iter().enumerate() {
        refer(id, &[Music], format!("rules/sounds.menu[{i}]"));
    }
    for (name, id) in [("victory", &rules.victory), ("defeat", &rules.defeat)] {
        if let Some(id) = id {
            refer(id, &[Music, Interface], format!("rules/sounds.{name}"));
        }
    }
    if let Some(sound) = content
        .rules
        .level_up
        .as_ref()
        .and_then(|l| l.sound.as_ref())
    {
        refer(sound, &[World], "rules.levelUp.sound".into());
    }
    for (name, id) in &rules.interface {
        refer(id, &[Interface], format!("rules/sounds.interface.{name}"));
    }
    for (location, message) in wrong {
        let file = content.origin(&location).to_string();
        errors.push(Error::new(&file, location, message));
    }
}
