//! Sound authoring through the editor's MCP (D97): requests takes from ElevenLabs, picks
//! one into an audio asset with its sound event, keeps a designed voice. The work is
//! `tools/sound/elevenlabs.py`'s (the same steps the skill canopy-sounds runs by hand), on
//! a thread of its own, since a take can take a minute. Nothing here sees the API key.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// Where takes and their specs wait to be auditioned and picked (kept by `just tidy`).
const TAKES: &str = "target/sound-takes";
const SCRIPT: &str = "tools/sound/elevenlabs.py";

/// One sound to generate: an item of the script's spec (its docstring has every field).
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "camelCase")]
pub struct GenerateSound {
    /// Unique name; the takes go to target/sound-takes/<name>.
    pub name: String,
    /// effect (one-shots and loops), music, voice (designs a voice from a description) or
    /// line (speaks a text library line in a kept voice).
    pub kind: String,
    /// effect, music: what it should sound like.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prompt: Option<String>,
    /// voice: who speaks (age, timbre, accent, pace, character).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    /// voice: a sample line (100 to 1000 characters).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// effect 0.5–30, music 3–600.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seconds: Option<f64>,
    /// effect: a seamless loop.
    #[serde(rename = "loop", skip_serializing_if = "Option::is_none")]
    pub looping: Option<bool>,
    /// effect: how literally the prompt is followed, 0–1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub influence: Option<f64>,
    /// music: no vocals (default true).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instrumental: Option<bool>,
    /// How many takes (default 2; music and voice 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub takes: Option<u32>,
    /// line: the text library key it speaks; voice and line: the speaker key
    /// (`speaker.aurelith`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub speaker: Option<String>,
    /// line: eleven_v3 audio tags before the text (`[whispers]`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tags: Option<String>,
    /// The asset folder a pick writes (`base/assets/audio/<…>`), its id
    /// (`asset.audio.<…>`) and name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dir: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// The sound event a pick creates or adds the clip to: {id, category, volume, pitch,
    /// loop, distance, cooldown, limit, fade, note}. A line's is `sound.<line key>`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub event: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct PickSound {
    /// The name it was generated under.
    pub name: String,
    /// The take's number (`take-2.mp3` is 2).
    pub take: String,
}

#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct KeepVoice {
    /// The voice item's name.
    pub name: String,
    /// The preview kept (`take-1-2.mp3` is 1-2).
    pub preview: String,
}

#[derive(Debug, Clone)]
pub enum Call {
    Generate(Box<GenerateSound>),
    Pick(PickSound),
    Keep(KeepVoice),
}

fn spec_path(data: &Path, name: &str) -> PathBuf {
    data.join(TAKES).join(format!("{name}.spec.json"))
}

fn script(data: &Path, args: &[&str]) -> Result<String, String> {
    let output = std::process::Command::new("python3")
        .arg(data.join(SCRIPT))
        .args(args)
        .current_dir(data)
        .output()
        .map_err(|e| format!("cannot run {SCRIPT}: {e}"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    if output.status.success() {
        Ok(text.trim().to_string())
    } else {
        Err(text.trim().to_string())
    }
}

/// Runs one call to its end (on a thread of its own).
pub fn run(data: &Path, call: Call) -> Result<Value, String> {
    let takes = data.join(TAKES);
    std::fs::create_dir_all(&takes).map_err(|e| e.to_string())?;
    match call {
        Call::Generate(item) => {
            let spec = spec_path(data, &item.name);
            let doc = json!({"items": [item]});
            std::fs::write(
                &spec,
                serde_json::to_string_pretty(&doc).unwrap_or_default(),
            )
            .map_err(|e| e.to_string())?;
            let spec = spec.to_string_lossy().to_string();
            let folder = takes.to_string_lossy().to_string();
            let log = script(data, &["request", &spec, &folder, &item.name])?;
            let mut files: Vec<String> = std::fs::read_dir(takes.join(&item.name))
                .map(|d| {
                    d.flatten()
                        .map(|e| e.path())
                        .filter(|p| p.extension().is_some_and(|x| x == "mp3"))
                        .map(|p| p.to_string_lossy().to_string())
                        .collect()
                })
                .unwrap_or_default();
            files.sort();
            Ok(json!({"log": log, "takes": files,
                "next": "Listen to the takes (afplay, or send them to Jiri), then pick_sound."}))
        }
        Call::Pick(pick) => {
            let spec = spec_path(data, &pick.name);
            if !spec.is_file() {
                return Err(format!("{} was not generated here", pick.name));
            }
            let choice = format!("{}={}", pick.name, pick.take);
            let log = script(
                data,
                &[
                    "pick",
                    &spec.to_string_lossy(),
                    &takes.to_string_lossy(),
                    &choice,
                ],
            )?;
            Ok(json!({"log": log,
                "next": "Cook (cook tool) and check (problems) to hear it in the game."}))
        }
        Call::Keep(keep) => {
            let folder = takes.join(&keep.name);
            let log = script(data, &["voice", &folder.to_string_lossy(), &keep.preview])?;
            Ok(json!({"log": log}))
        }
    }
}

/// Every sound event of the content: the catalog the assist chooses from.
pub fn events(content: &canopy_content::Content) -> Value {
    Value::Array(
        content
            .sounds
            .iter()
            .map(|e| {
                json!({"id": e.id, "category": e.category, "clips": e.clips, "note": e.note,
                    "loop": e.looping})
            })
            .collect(),
    )
}
