---
name: canopy-scenes
description: Warcraft III-style in-mission cutscenes in Under the Canopy: the scene functions of a mission's Lua, saved cameras, shot grammar, staging (moves, facing, kneel and sleep poses, lights and effects), the skip, and how to capture and review scene frames headless. Use when writing, staging, reframing or reviewing a mission's scenes.
---

# Mission scenes

Authority:
- **docs/campaign.md "Scenes":** the API table and the hold.
- **Decisions:** D88 (cinematics) and D89 (staging) in docs/decisions.md.
- **Code:** `crates/sim/src/mission/api/scene.rs` and `cinema.rs`; the picture in `game/src/play/shots.rs` and `screens/match_ui.rs`.

## How a scene runs

`mission.play("name")` starts `scenes.name()` in the mission's Lua.
- **The function is re-run from the top every tick**, skipping the statements already done. So branch only on `mission.get`/`set` variables, never on live queries that change between runs.
- **Escape skips:** walks become placements, waits pass, cameras cut, lines drop. Every scene must therefore end in a playable state: `cinematic(false)`, `camera_reset`, the objective set.
- **`cinematic(true)` holds the battle.** Only the cast acts: the units the scene moves, faces or animates. Everyone else freezes, and AI clocks stop.

## A scene, typically

```lua
function scenes.meeting()
  mission.cinematic(true)
  mission.camera_to("meet-wide", 0)            -- establishing
  mission.move("queen", 178, 238)
  mission.move("oleander", 168, 238)
  mission.wait_arrived("queen")                 -- gives up after 2x the walk + 3 s
  mission.face("queen", 168, 238)
  mission.face("oleander", 178, 238)
  mission.wait_facing()
  mission.camera_to("meet-over-queen", 0)       -- over her shoulder onto the speaker
  mission.speak("oleander", "I had your letter.", nil, "Oleander")
  mission.camera_to("meet-over-oleander", 0)
  mission.speak("queen", "Then tell me.", nil, "Aurelith")
  mission.animate("korrath", "kneel", true)     -- held until his next order
  mission.cinematic(false)
  mission.camera_reset(1.5)
end
```

## Saved cameras

`mission.cameras[]` in map.json: `{id, x, y, height, distance, pitch, yaw, fov, roll}`. Generators keep them as SHOTS (eye point, target point, fov, roll) and convert with their own formula; mirror any change both ways. The missions' scripts also have a helper (`shot_on` in The Hollow Brood, `close_on` in Founding) that keeps a saved camera's angle but aims it at where the speaker actually stands. Use it whenever a walk may stop short.

## Shot grammar

Every line is seen on the person saying it:
- an establishing shot when a place or group first appears;
- over-the-shoulder shots for exchanges, with the listener's shoulder at the frame edge and the speaker's face toward camera;
- clean singles for important lines;
- cut on line changes, and blend (2–5 s) only for travel.

Never:
- a back filling the frame;
- a giant prop, fern or stone across a face;
- a shot into black frame, or one that misses its speaker.

Staging:
- Turn characters to whom they speak (`face`).
- Put marks on reachable ground, and check them with `mission_playtest --reach`.
- Crowds: two angles, or move extras aside.

Poses and effects:
- `animate(unit, "kneel"|"sleep", true)` holds a pose until an order. Kneel exists for the ant marshal, warrior, archer and settler; sleep for the beetle worker and the winglancer.
- `effect_at(key, effect, x, y[, seconds])` and `light_at(key, x, y, colour, intensity, radius[, seconds][, height])`. Keys take letters, digits and `_` only.
- `fade_out(seconds, colour)` and `fade_in`.

Dialogue portraits are the speaker's live model. Humanoid rigs frame head and shoulders, by the head's length.

## Seeing it

```bash
just shot --play --map base/campaigns/vanguard.utcc/missions/<id>.utcm --scene <name> --out target/review/scenes/<id>/<name> --sheet
```

This plays `scenes.<name>` from its start at normal speed and writes:
- `00-establishing.png`: the scene's first shot, once it has faded in (or just before it cuts to its first line);
- `NN-<speaker>.png`: each line 0.75 s after it starts, with the letterbox and the speaker's portrait;
- `lines.txt`: each frame's tick, speaker, line and camera (`id`, `id+pan` for a `shot_on`/`camera_pan`, `a → b` mid-blend);
- `sheet.png` with `--sheet`: a labelled contact sheet.

Other forms:
- `--scene list` prints the mission's scenes; `--scene all` plays each into its own folder of `--out`, and `--sheet` adds one sheet of them all.
- `--compare <git rev>` also plays the scene from the map as it was at that revision (into `before/`) and writes `pair-NN.png`, before on the left. Show Jiri these pairs for a reframing.
- `--set <variable>=<value>` (repeatable) picks a branch, e.g. `--set tiv_ok=false`.

The opening (the scene `on_start` plays) plays as a new mission starts. Any other scene is staged mid-mission, as the script would reach it, and the map shows explored. The opening is skipped, then:
- script-activated units the scene names appear, and so do those the script spawns just before `play("<name>")`;
- units the trigger's condition needs gone (`not mission.alive("…")`, without an `or`) fall;
- the variables set just before `play("<name>")` are set. Variables the scene reads that nothing set default to a unit's position (variables the script fills from `mission.position`), else to `true`;
- objectives the scene ends are active;
- a dry run up to the first line learns the walks. Each walker starts 5 cells short of its mark, or on it when the way is blocked. A unit that is only turned, and stands far from what it turns to, is put 8 cells from it.

The tool prints what it staged. Mid-mission state the trigger reaches through earlier blocks is not staged (e.g. the webs' spiders in the patrol scene stay alive), and neither are helper conditions like `all_dead`. Read frames with that in mind.

`just shot --play --map … --tick <t> --out <png>` still renders one tick of a mission played from its start. `lines.txt` gives an opening's line ticks. `base/maps/showcase/cinematic-bench.utcm` is a bench of the Hollow Brood opening.

Review each line's frame yourself before showing Jiri.
