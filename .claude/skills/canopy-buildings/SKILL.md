---
name: canopy-buildings
description: How to make a race's building from concept to a working, animated, team-coloured model in the game — sizing, state concepts with gpt-image, the Tripo base model and its segmentation, the Blender building script (team mask, active glow, moving parts, clips, sockets), state effects, and checking it in game. Use when creating or remaking any building's model, its animation states or its effects, alone or as one of several agents building in parallel.
---

# Buildings

Read **canopy-assets** and **canopy-definitions** first; this skill is the pipeline on top of them. The worked example is the beetles' Shell Forge:
- script `tools/buildings/shell_forge.py`;
- asset `base/assets/buildings/beetles-shell-forge/`;
- definition `base/buildings/beetles/shell-forge.json`;
- effects `base/effects/buildings/forge-*.json`;
- concepts `art/styles/forge-*.png`.

When in doubt, do what it does.

## Rules (Jiri's, recorded in D61 and D65)

- **The base model comes from Tripo.** Never model a building from primitives or "invent" one. Tripo makes the model from an approved concept, and Blender fixes it and animates it.
- **Motion means work.** A building at rest is completely still: no animation, no glow, no smoke. The `idle` clip is a still pose. The building comes alive only while it is busy: `work` while it trains, researches, upgrades or revives, and `spawn` once when a unit comes out. Magic globules, fire and steam move only then.
- **Forest magic glows blue to violet, never green.** Use about `#8a5cff`–`#b89cff`; the forge tank uses `(0.55, 0.32, 1.0)`. Fire stays orange (`#ff8a35`).
- **Stylised and model-ready, like Warcraft III: Reforged.** Use a few big readable forms with crisp silhouettes and simple materials. Avoid greebles, grime and small details. Strip anything a modeller would not need.
- **No dirt, no ground, no plinth of earth.** The building stands straight on the map's ground; the terrain and its textures make the ground, never the model. This starts at the images: every concept and every Tripo reference shows the building with nothing under or around its foot (no dirt patch, mound, rubble, grass, puddle or shadowed ground skirt), so Tripo never models one. Reject an image that has it and regenerate; don't plan to cut it out later. `kit.strip_dirt` exists only to rescue an old model.
- **Team colour must work.** The race's blue plates become the player colour. Check it in every player colour, including white and dark.
- **Ask before anything visible.** Send Jiri polished state concepts, starting with idle, and wait for approval before building. Show side-by-side captures of results.

## Size

Buildings have **no walkable perimeter**: the model fills its footprint to the edge (Jiri, 2026-10-18).

Footprints are multiples of 4 cells. Size each building against the race's main hall using Warcraft III's tile ratios:

| Building | Warcraft III | Ours (cells) |
|---|---|---|
| Main hall (Town Hall) | 6×6 tiles, including a 1-tile pathing buffer | 20 |
| Upgrade building (Blacksmith) | 5×5 tiles, no buffer | 16 |

For other kinds (barracks, farm, tower, altar):
1. Look up the Warcraft III footprint (`pathTex`).
2. Scale it to the 20-cell hall, then round to a multiple of 4.
3. Propose the size to Jiri before building.

The model is `WIDTH` = the footprint in metres across its widest side. Its height follows the Tripo proportions; the forge is 16 × 14 × 11 m, and the hall is 18.8 m wide and 13.5 m tall.

In `asset.json`, match the footprint:
- `blocker` `{shape: box, size: [f, f]}`;
- `vegetationClearance` ≈ f/2;
- `display.healthHeight` ≈ the model height + 0.6.

In the definition, the `entrance` is `{x: 0.5, y: f/2 + 0.5}`.

**Placement on maps:** footprint edges lie on the 4-cell lattice from −0.5, so the centre must be ≡ (−0.5 + f/2) mod 4:
- 12- and 20-cell footprints: centres at 1.5 mod 4;
- 16-cell footprints: centres at 3.5 mod 4.

See `snap()` in `tools/maps/lookdev_fall.py`. A misaligned building fails `map_shipped`.

## Pipeline

### 1. State concepts (gpt-image)

Write one request per state and run it with `tools/buildings/concept.sh <request.json>`. Requests are JSON: `{slug, size, n, quality, references, prompt}`.
- The tool uses the old repository's key store. Never print the key, put it in a file, or pass it in a command.
- References must be paths inside `../Settlers 3 Web`. Stage them in its `.asset-work/concepts/` (the beetle refs are in `deep-forest-2026-10-17/refs/`).
- Outputs go to `.asset-work/concepts/buildings/`.
- One 1024² high-quality image takes about a minute. Run up to 4 at once (`xargs -P 4`).

States to draw:
- `idle`: cold and dark, magic dim;
- `work`: fire, smoke, glow and the moving part caught mid-motion;
- `spawn` where the building makes units.

Use the race's sheet and the hall as references so the family stays consistent. End every prompt with the model-ready clause:

> CLEAN, SIMPLE AND MODEL-READY, LIKE A WARCRAFT III: REFORGED BUILDING: a few big, bold, readable forms with crisp silhouettes; clean smooth surfaces and simple materials; no small greebles, no noise, no grime. Everything in focus and sharp. The building stands directly on a plain flat neutral grey floor: NO dirt, earth, mound, rubble, grass, roots, puddles or ground patch under or around its foot; its own built base (stone, steel, chitin) ends cleanly at the floor.

Check every result for ground under the building before showing it: a dirt ring, mound or skirt fails the image.

After Jiri approves, copy the chosen frames to `art/styles/<building>-<state>.png` (LFS).

**Tripo reference.** Make one more image from the approved idle frame:
- the same building, idle;
- **without its moving parts** (arms, hammers, wheels, doors that swing), with clean floor where they stood;
- seen from the front-left at about 35° down, whole and centred;
- on a plain light grey background, soft studio light, no ground plane, and **no dirt or ground of any kind at its foot** (the model must come out of Tripo clean).

Moving parts are made separately (step 2) so they can be separate nodes.

### 2. Tripo base model and segments

Tripo runs in the built-in browser pane, which is signed in to tripo3d.ai.

1. Open `https://www.tripo3d.ai/workspace/generate`.
2. **Image to 3D.** The built-in browser cannot upload local files: localhost fetch, the clipboard and file pickers are blocked. Ask Jiri once to drop the Tripo reference into the image tab and start the generation, and give its path. Use the newest model, textured.
3. **Segment.** On the finished model, choose the Segment tool in the left toolbar. It opens `/workspace/segmentation/<id>`. Use Balanced (8–15 parts, 40 credits), then Save.
4. **Export** each model as GLB to the receiver:
   - start `python3 tools/buildings/tripo_receiver.py <scratchpad>/models`, unless something already listens on 8766 (`lsof -iTCP:8766 -sTCP:LISTEN`); that receiver saves into its own folder, so look there;
   - in the Tripo tab, set `localStorage.__lookdevExport` to the text of `tools/buildings/tripo_export.js` once;
   - then run `eval(localStorage.__lookdevExport); await window.__exportTo('<building>-segmented.glb')`;
   - the tab navigates to the receiver's answer; go back for the next export.
5. **Moving parts** are Tripo **text to 3D** models of the one part (for example "a brass forge hammer arm on a beam, stylised, clean, game asset"), exported the same way.

Tripo's prompt box is a React textarea. Set it through the page's value setter and an `input` event, or type into it. Generate is at the bottom of the left panel.

### 3. Find the parts

```bash
/Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/inspect_model.py -- <model.glb> <scratchpad>/out/<name> <width m> [frame]
```

This lists every `tripo_part_<n>` with its faces, centre and extent in metres at the game size. It also renders top, front and side views (1 m grid) and the overhead game angle.

Check the base first: a part that is earth or a skirt around the foot means the reference was not clean. Go back to step 1, and use `kit.strip_dirt(part, below)` only when regenerating is impossible.

Assign roles by position: furnace mouth, tank, chimney tops, anvil, door. Write them at the top of the script. Segment numbers are only stable for that one export.

### 4. The building script (`tools/buildings/<building>.py`)

Copy `shell_forge.py`'s structure and use `tools/buildings/kit.py`:
- `reset`;
- `socket(name, at)`;
- `clip(obj, name, keys, path, index)`;
- `cylinder`, `box`, `cube_uv`;
- `export(folder, name, asset)`.

Run it like this:

```bash
/Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/<building>.py -- <segmented.glb> [<part.glb> ...]
```

Keep Tripo exports out of the repository (scratchpad). The script writes `geometry.glb`, `source.blend` and `asset.json` into `base/assets/buildings/<race>-<building>/`. Delete any `source.blend1`.

The script does these things in order:
1. **Normalise.** Unparent, apply the transforms, centre the footprint, put the ground at z 0, and scale to `WIDTH` across. Blender is Z-up and the building faces −Y; the exporter turns it Y-up / +Z.
2. **Team mask.** Per part, the race-colour texels (blue: `b − max(r, g) > 0.04`, soft over 0.1) go into the base-colour texture's **alpha**:
   - skip parts with mask mean < 0.002;
   - set `alpha_mode = 'CHANNEL_PACKED'`;
   - name the material `TC_<part>` and set `mat['teamColorMask'] = 'baseColorAlpha'`;
   - list the names in `capabilities.teamColor.slots` with `mode: "mask"`.
3. **Active glow** (D65) on the parts that come alive. Build an emission texture from the part's own texture, weighted to its darkest texels (only the furnace interior burns, never its stone ring):
   - fire: `clip((0.1 − lum)/0.06)^1.5` × orange, emission 5, pulse 1.6;
   - magic: `clip((0.2 − lum)/0.12)` × violet, emission 5, pulse 0.9;
   - then set `mat['activeGlow'] = {'pulse': hz, 'flow': v_per_s}`. `flow` > 0 streams the glow along V (sap rising).

   It is dark at rest and eases in while the building works.
4. **Sockets** are empties named `socket_*` (`kit.socket`): chimney tops (+0.3·scale), furnace mouth, anvil top, tank, spawn point, and so on. Effects attach there.
5. **Moving parts** become their own mesh node:
   - transform the mesh so the node origin is the hinge;
   - set `rotation_mode = 'XYZ'`, because glTF imports use quaternions and Euler keys on them are silently ignored;
   - decimate to about 1,600 faces;
   - support it visibly (a post or bracket); nothing floats.

   Make the motion **read from the RTS camera**: swing across the view, never towards the camera, and make it big enough (the forge hammer is 4.2 m on a 16 m building).
6. **Clips** (`kit.clip`, 30 fps, rigid node animation only, never skins, so the building keeps the building material):
   - `idle`: the rest pose held still, `[(0, REST), (2, REST)]`;
   - `work`: a loop that starts and ends at REST;
   - optionally `spawn`, played once.
7. **Events** in `capabilities.animations[].events` are **seconds** into the clip (`frame / 30`), not shares of it, for example the hammer's `strike`.
8. **The asset:** `kind: building`, `shading: building`, `display.scale: 1`, `groundContact: pivot`, the blocker and clearance from **Size**, and a `license` naming Tripo, gpt-image-2 and the script.

Gotchas:
- Any mesh with a textured material needs UVs (`kit.cube_uv`), or the exporter writes −1 indices and the cook fails.
- `kit.export` exports `ACTIONS` with forced sampling. NLA-only export drops still clips.
- Offsets in metres (socket heights, post radius) should scale with `WIDTH`, so a resize is one number (see `S` in `shell_forge.py`).

### 5. Definition and effects

In `base/buildings/<race>/<building>.json`:
- `model`, `footprint`, `entrance`;
- `presentation.effects`: a list of `{effect, state: "work" | "spawn", socket}` for effects that loop while in that state, or `{effect, event, socket}` for one-shots on a clip event.

Effects live in `base/effects/buildings/<building>-*.json`. Start from the forge's:
- `forge-smoke` (loop, particles);
- `forge-fire` (flickering light);
- `forge-sparks` (burst plus flash on `strike`);
- `forge-magic` (pulsing violet light).

Up to 8 effect lights show at once, so a building should use at most 2.

### 6. Check it in game

```bash
cargo run -q --release -p canopy-cook -- --data "$PWD" --out target/cooked
cargo build -q --release -p canopy
./target/release/canopy shot --data "$PWD" --map beetle-bench --play \
  --input "select:<definition id fragment>;cmd:research:<research id>;ticks:60" \
  --camera <x>,<z>,0.9 --hour 12 --out <scratchpad>/<name>.png
```

- `select:` matches **definition ids** (`select:shell-forge`), not map entity ids. Check the output line: `selected [n]` must not be empty.
- `cmd:` takes a command id: `research:<research id>`, `produce:<unit id>` (train), `upgrade:<building id>`.
- `ticks:n` advances the simulation.

Capture:
- rest (no command) against work at several tick counts, to catch the moving part mid-motion and on an event;
- the building beside the hall, for size;
- other player colours: place the building with `owner: player.N`.

The play camera clamps zoom, so crop full-resolution captures for detail. Benches:
- `base/maps/showcase/beetle-bench.utcm` (`tools/maps/beetle_bench.py`): flat, sandbox, a hall stocked with 20k;
- `lookdev-fall` (`tools/maps/lookdev_fall.py`, the look-dev map, hall stocked with 9k/9k).

Sandbox maps spawn no start setup, so stock comes from the placed hall's `initialState.inventory`.

### 7. Land it

- Write a release note with `just release-note --kind improved --text "…"` (player-facing).
- Run `just check` before committing.
- Commit only your own building's files.

## Working in parallel

Several agents can each build one building at once if each owns only its files:
- `tools/buildings/<building>.py`;
- `base/assets/buildings/<race>-<building>/`;
- `base/buildings/<race>/<building>.json`;
- `base/effects/buildings/<building>-*.json`;
- `art/styles/<building>-*.png`.

Shared files belong to the lead session. Propose changes to them; don't edit them:
- `tools/buildings/kit.py`;
- the map scripts and maps (report the building's footprint and the position you want);
- engine code (`crates/`);
- `docs/decisions.md`;
- `releases/log.json` (only through `just release-note`, which merges).

Don't run the map scripts; they rewrite whole maps. One Tripo account is shared: generate, segment and export your own models only, and name exports `<race>-<building>-*.glb` so they don't collide in the receiver's folder. Ask Jiri for each image upload through the lead session, batched where possible.
