# Audio

`canopy-audio` (`crates/audio`) plays every sound of the game: the sound layers of ability effects, sounds the game triggers, interface sounds and music. Decisions: [D29](decisions.md).

## What the old game does

The behaviour reference is `src/render/audio/effectSound.ts`, `src/render/abilities/effectPlayer.ts`, `abilityEffects.ts` and `src/shared/settings/audio.ts` in the old repository.

- **Only effect sounds.** The only sounds are `shape: "sound"` layers of ability effects (two shipped clips, used by four effects). No music, no interface, unit, combat or building sounds.
- **One setting**: "Spell effect volume", 0–1, default 0.65, a linear gain.
- **Listener**: the camera's focus point on the ground; panning along the camera's right vector. Distance is measured on the ground, so zoom does not change what is heard.
- **Attenuation**: full volume up to the layer's `referenceDistance`, then `(1 − (d − ref)/(max − ref))²` to silence at `maxDistance`; at or beyond `maxDistance` the voice stops.
- **Pan**: `clamp(lateral / max(ref, d), −1, 1)` through a Web Audio stereo panner (equal power).
- **Gain**: `volume × effectVolume × attenuation × fadeIn × fadeOut`, glided with a 12 ms time constant (pan 20 ms).
- **Timeline**: every frame the sound is *sampled* from the game tick: offset `= age × pitch`, rate `= pitch × game speed`; a voice more than 0.12 s off its timeline restarts at the right offset. One-shots never replay after their sample ends; `sound.loop` loops the sample; a looping parent effect repeats one-shots each period.
- **Pause** (or game speed 0) stops effect sounds; a hidden tab suspends audio.
- **24 voices** in total, first come, first served.
- **Cue lifecycle** (shared with the visual layers): see [Effect sounds](#effect-sounds).

## Library: kira

kira 0.12 on its cpal backend (`default-features = false, features = ["cpal"]`).

| | kira | cpal + own mixer |
|---|---|---|
| Platforms | macOS (CoreAudio), Windows (WASAPI) through cpal | same |
| Audio thread | allocation-free by design: fixed-capacity arenas, lock-free command queues, finished sounds dropped on the main thread (verified by our `no_alloc` test) | ours to write and verify |
| Device loss, default device changes | handled (stream restarted) | ours to write |
| Resampling, playback rate, tweens, tracks | built in | ours to write |
| Streaming compressed music later | built in (symphonia) | ours to write |
| Headless testing | mock backend runs the full mixer without a device | ours to write |
| Control over the model | we compute gain and pan ourselves on the main thread, so the old model is exact | full |

kira's own spatial tracks are not used: their model differs from the old game's, and computing gain and pan per voice on the main thread costs nothing. kira's pan law (`sqrt`) differs from Web Audio's (`cos`/`sin`): mono sounds are stored at −3 dB and the pan is passed as `sin(p·π/2)`, which reproduces the old channel gains exactly for every pan (tested).

## Architecture

```
EffectSounds (cues from views) ─┐
Sources (play / play_with)      ├─ requests ─▶ Selector (budgets) ─▶ Output (kira)
Music (playlist)                ┘   offset, rate, gain, pan          granted voices
```

Each frame `Audio::update` asks every source what it wants to be heard: the offset into the sound, the rate, the gain (volume, fades, distance) and the pan. The selector grants voices; the output reconciles kira's playing sounds with the granted requests: new voices start at their offset with a 12 ms fade-in, kept voices glide to the new gain, pan and rate over 20 ms and seek back to their timeline when they drift more than 0.12 s, lost voices fade out over 30 ms. Sources without a voice are *virtual*: their timeline continues, so a voice won later starts at the right offset.

Two clocks: effect and world sounds follow game time (`render_tick`, scaled by game speed, silent while paused); interface sounds and music follow the wall clock.

Sounds come from the pack as 16-bit PCM ([D26](decisions.md)) and are converted once to kira's stereo frames (`SoundLibrary::from_pack`). Effect layers find their clip through the manifest: asset record `asset`, output `audio.<ext>` for index 1 and `audio_<index>.<ext>` after that, exactly as the old game resolved them; the pack entry is that output's path.

## Wiring it into the game

```rust
// Loading (after the pack and content):
let mut audio = canopy_audio::Audio::new(settings.audio);   // silent if there is no device
audio.load(&pack)?;
audio.bind(&content);
// Every simulation view the game builds for the player (once per tick):
audio.observe(&view);
// Every rendered frame:
audio.update(&canopy_audio::Frame {
    real_dt,                      // wall-clock seconds since the last frame
    render_tick,                  // the renderer's fractional tick
    speed,                        // 0 while paused (menu pause too)
    listener: Listener::new(focus_x, focus_y, right_x, right_y), // cells, camera right on the ground
});
// Window hidden or minimised:
audio.set_background(hidden);
// New match or loaded game:
audio.clear_game();
```

`AudioSettings` is serde-ready (camelCase, missing fields take defaults) for the game's settings file; `apply_settings` takes effect within a frame. `Audio::stats` feeds the debug overlay. Game code plays its own sounds with `play(sound, At::ground(x, y))`, `play_interface(sound)` and `play_with(sound, at, PlayOptions { … })`; loops return a `Handle` for `stop` and `set_at`. Sound ids come from data (`Audio::sound(<pack entry name>)` or `SoundLibrary::find_published`), never from names in code.

## Effect sounds

`EffectSounds::bind` compiles every enabled `sound` layer of every effect bound in an ability's presentation into a recipe. `observe(&View)` feeds it:

- **Ability events** the viewer may see (`view.spells.events`), each once (by event id). `ability_event` takes single events for callers that read `World::drain_events` and filter them by viewer themselves.
- **Statuses** on live units in the view: a status binding (`lifetime: status`) without a cue starts one (also when the status was applied before the player saw it); status cues end when their status is gone or the unit dies. Finite layers on an aura status pulse every 5 seconds (old: 200 ticks).
- **Deliveries in flight** first seen after their launch event start their `projectile` sound once.
- **Heroes returning** through an ability keep their `revivalStarted` cues until the return completes or is cancelled.

Cue rules, as in the old effect player: a cue starts `startTick` after its event at the event's point (the caster's origin for caster-anchored layers) plus the layer offset; with `follow` it tracks its anchored unit. It ends after the layer's `durationTicks` (the event's duration with `durationFrom: delivery`); finite bindings of sustained or looping layers end after one effect period (or the delivery's duration); a `cancelled` event ends the cast's cues. Pitch is the playback rate and scales with game speed; fades and attenuation shape the gain; nothing plays while paused.

Deliberate differences, all presentation only:

- Projectile sounds start once per cast. The old renderer re-emitted the `projectile` event for every delivery without a missile layer each frame, which would have stacked copies of such a sound.
- Status cues are keyed by the unit holding the status (and the status), not by the cue's anchor: a caster-anchored `statusApplied` layer no longer restarts every frame, and re-applying a status does not restart its loop. (Views do not carry the status's cast id.)
- Sound layers ignore model sockets (`attachment`): their position is the anchor's ground point, which is what is heard at RTS distances.

## Voice limiting

Requests compete within their **category**; the most audible win (gain × priority), subject to the category's budget and a per-sound cap. A playing voice keeps its place unless a newcomer is 25% louder, so near-equal sources do not flap. Inaudible requests (below −60 dB) and requests on a muted bus never take a voice.

| Category | Bus | Clock | Voices | Per sound | Retrigger merge | Late start |
|---|---|---|---|---|---|---|
| Effect (ability effect layers) | effects | game | 24 (old cap) | 6 | none | any time (old behaviour) |
| World (sounds the game plays) | effects | game | 16 | 4 | 40 ms | until 0.15 s in |
| Interface | interface | wall | 8 | 2 | 30 ms | until 0.1 s in |
| Music | music | wall | 2 (crossfades) | 2 | none | any time |

- **Per-sound cap**: 400 units striking at once play the 4 nearest copies of the hit sound, not 400.
- **Retrigger merge**: a world or interface one-shot of the same sound started within the window of another is dropped; simultaneous copies only add loudness and phasing.
- **Late start**: a world one-shot that never got a voice in its first 0.15 s is dropped instead of starting mid-transient.

Budgets are `Limits` and can be tuned with measurements; bus capacities in kira are twice the budgets so fading voices never block new ones.

## Buses and settings

| Setting | Default | Notes |
|---|---|---|
| `master` | 1.0 | main track |
| `effects` | 0.65 | the old "Spell effect volume" and its default; effect and world sounds |
| `interface` | 0.7 | new |
| `music` | 0.5 | new |
| `muteInBackground` | true | the old game suspended audio in a hidden tab |

Volumes are linear amplitudes multiplied together (master × bus × sound), as the old effect volume was.

## Music

`Playlist { tracks, shuffle, repeat, crossfade, volume }` plays tracks in order or shuffled (each round a new order, never the same track twice in a row) with equal-power crossfades between tracks and when a new playlist takes over; `stop_music(fade)` fades everything out. No music ships yet. Tracks are library clips (PCM in the pack); see deferred items.

## Tests

Without an audio device (CI): attenuation and pan maths, the pan law against Web Audio's, settings, the selector (crowds, budgets, hold, mute, late starts), sources (clocks, pause, retriggers, fades, stops), music (crossfade power, playlist end and repeat, shuffle), effect cue rules against the old player (one-shots, delays, anchors, offsets, follow, speed, fades, distance, status loops, aura pulses, cancel, looping effects, delivery durations, projectiles, returns, missing clips), the headless pipeline on kira's mock backend (pack loading, crowd voice counts, pause, bus and background mute, music, stops), a real simulation casting a spell whose release and status play test-owned sound layers, and a check that kira's renderer never allocates while sounds start, play, move, fade and stop.

Manual: `cargo run -p canopy-audio --example play_pack` plays every sound of the cooked pack centred, swept left to right and moving out of range (`--volume`, `--headless`).

## Deferred

- **Music streaming**: real music needs compressed audio (Ogg Vorbis or Opus) in the pack and kira's streaming sounds; a three-minute stereo track as PCM is 30 MB on disk and 60 MB as mixer frames. Decide the format with the cooker when music arrives ([D26](decisions.md) already says to revisit then).
- **Interface, unit, combat and building sounds**: categories, buses and APIs exist, but there is no content or binding format for them (which button, order or hit plays which sound). Needs data, which is a phase 2 format question, or a small data file agreed with Jiri.
- **Settings screen and persistence** in `game/` (M4 settings), and calling `set_background` from window events.
- **A device that appears after launch**: without a device at start the game runs silent until restarted. After start, kira restarts the stream when the device is lost; on macOS it does not follow a change of the default device while the old one remains (kira avoids querying CoreAudio during playback).
- **Budgets measured** on the reference machine in a full battle; `audio_update` and `audio_observe` have tracing spans.
