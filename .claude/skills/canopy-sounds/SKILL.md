---
name: canopy-sounds
description: Under the Canopy's sounds (D97): sound events (`sounds/`), what plays them (unit and building bindings, unit voices, race alerts and music, the game's menu and interface sounds, biome ambience, map and campaign music, dialogue voice by line), and authoring them with ElevenLabs (tools/sound/elevenlabs.py: effects, loops, music, designed voices, spoken lines; takes, audition, pick). Use when adding, generating, binding or reviewing any sound or music.
---

# Sounds

Read **canopy-packages** and **canopy-assets** first. The decision is D97 in [decisions](../../../docs/decisions.md); the engine is [docs/audio.md](../../../docs/audio.md). The Rust types are the authority: `crates/content/src/sounds.rs`.

## Sound events

Content never names a clip; it names a **sound event**, one file `sounds/<free folders>/<name>.json`:

```json sound
{
  "id": "sound.example.spear-strike",
  "note": "A light spear thrust landing on chitin: a dry wooden knock with a short crack.",
  "category": "world",
  "clips": ["asset.audio.spells.frost-impact"],
  "volume": [0.8, 1.0],
  "pitch": [0.94, 1.06],
  "distance": {"reference": 12, "max": 64},
  "cooldown": 0,
  "limit": 4
}
```

- `category`: `world` (placed: blows, steps, buildings, deaths), `voice` (a unit answering its player), `interface` (clicks, spoken alerts), `ambience` (placed loops of a place), `music` (playlist tracks, streamed), `dialogue` (a line's or a page's voice, streamed).
- `clips`: audio assets; each start picks one, never the previous one again when there are several. Variations are what keeps a hundred strikes from sounding like a machine gun: give blows and steps three or more.
- `volume` and `pitch`: a number or a `[min, max]` range picked per start. `loop`, `fade` (seconds a loop fades), `distance` (world and ambience only; the category's default otherwise), `cooldown` (seconds between starts anywhere), `limit` (copies at once).

## What plays them

| Where | Field | Plays |
|---|---|---|
| A unit or building (`presentation.sounds`) | `[{sound, state \| event \| on}]` | a loop while in a D65 state (`work`, `move`, …), once on a clip event (`hit`, `step`, `strike`), or once `on` a moment: `spawn`, `complete`, `death`, `impact` (its blow or missile lands, heard at the target) |
| A unit (`presentation.voice`) | `{select, order, attack, annoyed, ready}` | voice events, to its commanding player only, one at a time |
| A race (`sounds`) | `{alerts: {<text key>: sound}, music: [sounds]}` | a spoken alert when a message with that key reaches its player (`message.research-completed`, `alert.under-attack`); skirmish music |
| `rules/sounds.json` | `{menu, victory, defeat, interface}` | menu music, the end stings, interface sounds by name (`click`, `refused`, `objective`) |
| A biome (`ambience`) | `{day, night}` | ambience loops by the hour |
| A map (`map.json` → `music`) | `[sounds]` | its playlist, over the race's |
| A campaign step | `music`, a page's `narration` | a music event; a dialogue event |
| A dialogue line | — | the event `sound.<line key>` when it exists (`sound.line.founding.here-we-start-again-and`) |

## Authoring with ElevenLabs

Generation is authoring only (D97). The key is `ELEVENLABS_API_KEY` or `~/.config/under-the-canopy/elevenlabs-key` (mode 0600). **Never print, copy, log or commit it**; the tool redacts it from errors.

```bash
python3 tools/sound/elevenlabs.py check            # the key is there (--online: it works)
python3 tools/sound/elevenlabs.py dry tools/sound/specs/<spec>.json [name ...]   # requests, no call
python3 tools/sound/elevenlabs.py request tools/sound/specs/<spec>.json <scratch> [name ...]
python3 tools/sound/elevenlabs.py list <scratch>
python3 tools/sound/elevenlabs.py pick tools/sound/specs/<spec>.json <scratch> <name>=<take> ...
python3 tools/sound/elevenlabs.py voice <scratch>/<name> <preview>
```

- **Specs** (`tools/sound/specs/<topic>.json`, `{"items": [...]}`), one item per sound: `name`, `kind` (`effect`, `music`, `voice`, `line`), `prompt`, `seconds`, `loop`, `takes`, `dir` (asset folder), `id`, `title`, and `event` (the sound event `pick` creates or adds the clip to). The tool's docstring has every field.
- **Effects** come back as MP3 and are stored as 16-bit mono WAV (they stay PCM in the pack); **music and lines** stay MP3 and stream. Every pick writes `generation.json` (the request, the takes, the one picked) beside `audio.*`.
- **Voices**: a `voice` item designs previews from a description; listen, then keep one under its speaker key (`speaker.aurelith`) with `voice`, which records the voice id in `tools/sound/voices.json`. A `line` item speaks a text library line in its speaker's kept voice; eleven_v3 takes audio tags in `tags` (`[whispers]`, `[determined]`).
- **Audition**: send takes to Jiri with SendUserFile, or play them with `afplay`. Ask Jiri before a sound's character is settled (AGENTS.md: anything felt).
- Delete the scratch takes once the picks are in.
- **Through the editor's MCP** the same steps are tools: `sound_events` (the catalog), `generate_sound` (one item, as a spec item; takes in `target/sound-takes/<name>/`), `pick_sound` and `keep_voice`. They run the script on a thread of their own, so the editor stays responsive while a take is made.
- **Credits**: every request spends ElevenLabs credits. Generate what Jiri asked for, two takes at most unless he wants more, and never in a loop.

## Prompts that work

- Effects: say the material, the action, the length and the room: "a dry wooden spear thrust hitting hard chitin, short crack, no reverb, 0.6 seconds". Ask for no music, no voices. Loops: `loop: true` and a steady texture ("a soft forge bellows breathing in and out, warm crackle, seamless").
- Music: instruments, mood, tempo, structure, and what to avoid ("no vocals, no drums for the first 30 seconds"). Warm, enchanted forest-floor orchestral for the first campaign (memory: campaign tone).
- Voices: age, timbre, accent, pace, character ("an old ant queen, warm alto, unhurried, regal but kind").

## Check

```bash
cargo test -q -p canopy-content --test shipped_data   # every sound event and reference
cargo test -q -p canopy-content --test skills          # these examples parse
```

Listen in the game: `/run-game`, at normal zoom, with a battle; note player-visible sound changes with `just release-note`.
