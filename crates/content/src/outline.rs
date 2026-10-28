//! A campaign step's outline (D110): `outlines/<step id>.json` in its campaign package.
//!
//! The design of a step before (and beside) the real thing, after Blizzard's mission
//! briefs: what the step is for, who is on stage, the threads it opens and carries; for a
//! mission its kinds, pitch, pillars, objectives, feel, story beats and a sketch of the
//! map; for a cinematic its shots. A story's pages are the story's own, in
//! `campaign.json`. Outlines are working documents: plain words, never text keys, and a
//! release leaves them out.

use serde::{Deserialize, Serialize};
use serde_with::skip_serializing_none;

/// `outlines/<step id>.json`.
#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Outline {
    /// What this step is for in the campaign.
    pub purpose: Option<String>,
    /// Free notes and open questions.
    pub notes: Option<String>,
    /// Who is on stage: characters by their unit or hero definition.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cast: Vec<Cast>,
    /// The story threads it opens, carries and closes.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threads: Vec<Thread>,
    /// How tense it is, 1 (calm) to 5 (the peak), for the read-through.
    pub tension: Option<u8>,
    /// A mission's kinds (`defend`, `timed`…; [`MISSION_KINDS`]).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<String>,
    /// A mission's pitch: a line or two.
    pub pitch: Option<String>,
    /// A mission's three pillars: what it plays, the character it grows, the story it tells.
    pub pillars: Option<Pillars>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub objectives: Vec<Objective>,
    /// About how long it plays, in minutes.
    pub minutes: Option<u32>,
    /// A mission's story beats, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub beats: Vec<Beat>,
    /// A mission's map, sketched.
    pub sketch: Option<Sketch>,
    /// A cinematic's (or a mission's) place.
    pub location: Option<String>,
    /// How a cinematic is made: `scene` (in-engine), `film` or `card`.
    pub made_as: Option<String>,
    /// A cinematic's shots, in order.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shots: Vec<Shot>,
}

/// The kinds a mission may be (D110); more may come.
pub const MISSION_KINDS: [&str; 10] = [
    "hero-walk",
    "base",
    "defend",
    "timed",
    "escort",
    "dungeon",
    "stealth",
    "race",
    "hold-points",
    "split-forces",
];

/// How a cast member stands in a step.
#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Presence {
    /// On stage.
    #[default]
    On,
    /// Named but not seen.
    Mentioned,
    /// First playable here.
    Joins,
    /// Leaves the story here.
    Leaves,
    /// Not decided.
    Open,
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Cast {
    /// A unit or hero definition: its portrait and name.
    pub who: String,
    #[serde(default)]
    pub presence: Presence,
    pub note: Option<String>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum ThreadMark {
    #[default]
    Opens,
    Carries,
    Closes,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Thread {
    #[serde(default)]
    pub mark: ThreadMark,
    pub text: String,
}

#[skip_serializing_none]
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Pillars {
    pub play: Option<String>,
    pub character: Option<String>,
    pub story: Option<String>,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum ObjectiveKind {
    #[default]
    Main,
    Optional,
    Secret,
    EasterEgg,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Objective {
    #[serde(default)]
    pub kind: ObjectiveKind,
    pub text: String,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum Phase {
    /// The opening scene.
    Before,
    #[default]
    During,
    /// The closing scene.
    After,
}

#[derive(
    Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema,
)]
#[serde(rename_all = "camelCase")]
pub enum BeatMode {
    /// Letterboxed, the battle held.
    Scene,
    /// Spoken in play, with a portrait.
    #[default]
    Line,
}

/// A story beat: who says what, when.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Beat {
    #[serde(default)]
    pub phase: Phase,
    /// A unit or hero definition; none for narration or a note.
    pub speaker: Option<String>,
    /// What sets it off: "Start", "Wave 3", "Found the crack"…
    pub trigger: Option<String>,
    #[serde(default)]
    pub mode: BeatMode,
    pub text: String,
}

/// A cinematic's shot.
#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shot {
    /// `wide`, `medium`, `close`, `two-shot`, `over`…
    pub framing: Option<String>,
    pub action: String,
    pub speaker: Option<String>,
    pub line: Option<String>,
    pub seconds: Option<f32>,
}

/// A map sketched on the tabletop: shapes and stamps in map cells, on a square of `size`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Sketch {
    /// Cells per side (a map size).
    pub size: u32,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub shapes: Vec<Shape>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stamps: Vec<Stamp>,
}

impl Default for Sketch {
    fn default() -> Self {
        Self {
            size: 256,
            shapes: Vec::new(),
            stamps: Vec::new(),
        }
    }
}

/// What a stroke draws.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Brush {
    /// A plain line.
    Pen,
    /// The rim of high ground (an area).
    High,
    /// Trees (an area).
    Forest,
    /// Water (an area).
    Water,
    /// The main road, with an arrow.
    Road,
    /// A side path, dashed.
    Path,
    /// Where enemies come from, dashed, with an arrow.
    Attack,
}

impl Brush {
    pub const ALL: [Brush; 7] = [
        Brush::Pen,
        Brush::High,
        Brush::Forest,
        Brush::Water,
        Brush::Road,
        Brush::Path,
        Brush::Attack,
    ];

    /// Drawn as a filled area rather than a line.
    pub fn area(self) -> bool {
        matches!(self, Brush::High | Brush::Forest | Brush::Water)
    }
}

/// A stroke: `points` in map cells `[x, z]`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Shape {
    pub brush: Brush,
    pub points: Vec<[f32; 2]>,
}

/// What a stamp marks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase")]
pub enum Mark {
    Start,
    Amber,
    /// A camp; `count` its size.
    Camp,
    /// A hero; `who` its definition.
    Hero,
    /// A main objective; `label` its number.
    Objective,
    /// An optional objective; `label` its letter.
    Optional,
    Secret,
    /// A story beat; `label` its number.
    Beat,
    Shop,
    Spring,
    /// Words on the map; `label`.
    Text,
}

impl Mark {
    pub const ALL: [Mark; 11] = [
        Mark::Start,
        Mark::Amber,
        Mark::Camp,
        Mark::Hero,
        Mark::Objective,
        Mark::Optional,
        Mark::Secret,
        Mark::Beat,
        Mark::Shop,
        Mark::Spring,
        Mark::Text,
    ];
}

#[skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Stamp {
    pub mark: Mark,
    pub x: f32,
    pub z: f32,
    pub label: Option<String>,
    pub count: Option<u32>,
    pub who: Option<String>,
}

/// The path of a step's outline within its campaign package.
pub fn path(step: &str) -> String {
    format!("{}/{step}.json", crate::campaign::OUTLINES_DIR)
}

/// Checks an outline's numbers and kinds; the words are free.
pub fn problems(outline: &Outline) -> Vec<String> {
    let mut found = Vec::new();
    if let Some(t) = outline.tension
        && !(1..=5).contains(&t)
    {
        found.push("tension is 1 to 5".to_string());
    }
    for kind in &outline.kinds {
        if !MISSION_KINDS.contains(&kind.as_str()) {
            found.push(format!("unknown mission kind {kind}"));
        }
    }
    if let Some(sketch) = &outline.sketch
        && !(32..=2048).contains(&sketch.size)
    {
        found.push("a sketch is 32 to 2048 cells".to_string());
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn an_outline_reads_its_parts_and_refuses_unknown_fields() {
        let outline: Outline = serde_json::from_value(json!({
            "purpose": "Hold the yard.",
            "kinds": ["defend", "timed"],
            "cast": [{"who": "unit.a", "presence": "joins"}],
            "beats": [{"phase": "before", "speaker": "unit.a", "mode": "scene", "text": "Here."}],
            "sketch": {"size": 256, "shapes": [{"brush": "road", "points": [[1, 2], [3, 4]]}],
                       "stamps": [{"mark": "camp", "x": 10, "z": 20, "count": 2}]},
            "tension": 4
        }))
        .unwrap();
        assert!(problems(&outline).is_empty());
        assert_eq!(outline.cast[0].presence, Presence::Joins);
        assert_eq!(outline.sketch.as_ref().unwrap().stamps[0].mark, Mark::Camp);
        assert!(serde_json::from_value::<Outline>(json!({"pitch": "x", "map": "y"})).is_err());
        let bad: Outline =
            serde_json::from_value(json!({"kinds": ["siege"], "tension": 9})).unwrap();
        assert_eq!(problems(&bad).len(), 2);
    }
}
