# The Reforged lab

Warcraft III: Reforged's 912 HD effects are PopcornFX 2.5 baked files (`.pkb`). We decode and import them as our effect graphs to study and to measure the gap (D113). **Everything here is Blizzard's: it stays in `target/wc3-lab` (ignored by Git) and the session scratchpad, never committed, cooked into the shipped pack, uploaded or published.**

## Layout

- `target/wc3-lab/` is a copy-on-write clone of the repository's `base/` (`cp -c -R base target/wc3-lab/base`), with symlinks `Cargo.toml` → `../../Cargo.toml` and `config` → `../../config` (the editor wants a checkout), and `target/cooked` → `../../cooked-wc3lab`.
- The FX textures are image assets `asset.effects.wc3.<group>.<name>` under `target/wc3-lab/base/assets/effects/wc3/` (452 of them, licence field "Local reference only").
- Imported effects are `target/wc3-lab/base/effects/wc3/<path-of-the-pkb>.json`, ids `effect.wc3.<same>`; hand-built references are in `effects/wc3lab/` (Holy Light, decoded by hand).

## Extracting and importing

```sh
# Files from the game's CASC storage (the casc-reader from the old repository's tools):
"../Settlers 3 Web/.asset-work/tools/casc-reader/casc-extract" '/Applications/Warcraft III/.build.info' --list | grep '\.pkb$'
"../Settlers 3 Web/.asset-work/tools/casc-reader/casc-extract" '/Applications/Warcraft III/.build.info' 'war3.w3mod:_hd.w3mod:<path>' <out>

python3 -I tools/effects/pkb_dump.py FILE.pkb            # readable tree: layers, curves, renderers, decompiled scripts
PY="$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)"   # a Python with numpy
"$PY" -I tools/effects/pkb_flow.py OUT FILE.pkb ... --report REPORT.json --models SEQUENCES.json   # 912 effects in about 45 s
"$PY" -I tools/effects/pkb_flow.py --convert EFFECT.json ...   # any effect file still in primitives, in place
```

**Effects are stored in the authoring nodes (D115)**, so the conversion runs in two halves. `pkb_import.py` (used by `pkb_flow.py`, also runnable alone for debugging) transcribes the scripts into compiler primitives as below; `pkb_flow.py` then runs each transcribed layer on the CPU (96 particles at the scripts' 1/60 s step; the position lags the velocity one step at birth, so fits start from the second) and fits the nodes to what the particles do: Life from their lives, Shape from where they are born (point, disc, ring, sphere, box or cylinder), Launch from their first velocity (outward, up, axis, cone or random), Physics from a least-squares fit of `a = g − k·v` (Orbit when they turn steadily about the emitter, Turbulence when neither explains the motion), Size, Colour (with per-particle brightness), Spin and Flipbook over life, Emit from when its condition fired (once early: birth; late: death; repeatedly: an interval; for some particles only: a chance), Shape `follow` for layers standing at their parent or the emitter, Beam and Path for layers reading the target; Life and the end with the effect become the Spawner's `life` and `ends`. A generate whose amount follows the spawner's life (blood, footprints, debris: `curve(life) × rand × dt`) becomes a `rateCurve` on the Emit or the stream; a trigger on the effect stopping becomes Emit `when: end`; a spread of hues becomes Colour over life `hue`; a colour multiplied by the team colour becomes its `team` tint (fitted as if the team were white); points inside an ellipsoid become the ellipsoid shape; an invisible layer that stands still and only streams events into one layer (PopcornFX's spray helper: blood, footprints, debris) folds into that layer's Spawner `spray` (395 lanes fewer). The fitting simulation keeps the effect running (its end is read from the graph). Its report marks a layer `approximate` with what it guessed. Over all 912: 4,757 layers clean, 148 approximate, every one compiling.

The transcription each layer's scripts statement by statement: fields a script updates become the particle's attributes (`setAttribute` / `attribute`), each read seeing the latest value, conditions become `compare` / `select`, and `scene.dt` is our fixed 1/60 step. Event routes become events: spawners that only generate fold into their parent's Emit event (instant ones into death or birth events, rate ones born with their parent into interval events, `generate` by distance into distance events), `trigger(…)` kicks into death or conditional step events, and layers born of events listen with `onEvent`; a child's emitter is where its event happened. Spatial-layer lookups (Reforged keys them by the inserting particle) read the `parent`, `scene.intersect` the ground plane, `self.kill` is `kill`. `Game.TargetPosition` is the `target` end, `Game.TeamColor` the team colour; a generate's payload elements 2 and 5 are the child's `order` and `share` in its batch (a beam's points); a distortion renderer becomes a `distort` billboard. Layers nothing kicks belong to the effect's watcher (it fires `__e_Trigger` when the effect stops), and the watcher lives from the start. Fields set at spawn and read later are stored as attributes (never recomputed), widened to their declared vector width. Over all 912: of 4,906 layers about 4,850 whole, 60 partial (decoder constants it cannot place, taken as 0, and a few vector `any` tests), 1 unreadable file, every built one compiling. It reports `ok`, `partial` (what it dropped) or `skipped` per layer. Notes on the format:
- One PopcornFX unit is 3.125 m; their z is up (converted to our y up). **Shape descriptors are y up already** (offsets are lifts such as `(0, 0.3, 0)`, flat boxes `(x, 0, z)`): their offset, box size and ellipsoid scale are taken as they are, never swapped. Only three-component values swap axes: four components are colours and HSV shifts, read as they are (a swapped one turns gold to pink).
- **An effect plays as long as its model does.** In Reforged each `.pkb` runs inside the `.mdx` beside it: it emits through the model's Birth and Stand sequences and stops at Death (its particles finish). `--models` takes those sequences (extract each `.mdx` at the `.pkb`'s path and read its `SEQS` chunk: name, start and end in ms, a non-looping flag) and sets the effect's length to Birth + Stand, looping when Stand loops. Without it, effects stop far too early (Starfall's star after 0.2 s instead of 3.3 s). Layers' own spawner durations still apply (Flame Strike's pillar spawns for 2 s).
- **Some art is still classic.** A model with no Reforged effect (Flame Strike's burning, `FlameStrikeDamageTarget`) plays its classic `PRE2` particle emitter in Reforged too: decode it (speed, variation, latitude, gravity, lifespan, rate, area, filter 1 additive / 0 blend, rows × columns, head intervals, three segment colours stored red first, alphas, scalings as half-widths, the middle time) and build it as a graph; one Warcraft unit is 5/128 m.
- Billboard `Size` is a **radius** (×2 for our full width); `Axis` lengths are scaled by 3.125.
- The dump writes enums as `PlaneAligned (5)`, `AlphaBlend (1)`: compare the name, not the whole string.
- **Bytecode `4a`** (two operands) moves a value between a float4 and an orientation (base type 36) unchanged; before it was decoded, the decoder stopped at it and 86 effects lost the rest of a script (Starfall's falling star among them). `swzf6_2124(un07?(h))` is a quaternion about their up axis from the half-angle `h`: our `rotate` about y by −2h (the axis swap mirrors the turn).
- **Payload elements** are tagged by type in the high byte (`0x1xx` a float3 position, `0x102` interpolated over the step; `0x2xx` a float4, an orientation or colour; `0` a number): a kick's position is its float3 element.
- **A layer that inserts a point into a spatial layer** stands at that point (its readers follow it as their parent), and **a root event stream fires at the times its sampler lists** (`f10`, 32-bit floats: Starfall's impact at 0.98 s, when the star lands); layers born of it wait that long.
- Decoded opcodes: `fn18?` saturate, `fn17?` and `un16?` fract, `un01?` inverse square root, `un33?` ≠ 0, `alu0c` <, `alu0d` ≤, `alu0e` >, `alu0f` ≥, `alu09` or, `alu04` a − b. `swzXX_HHHH`: 3-bit selectors from the lowest bits, 0–3 a component, 4 zero, 5 one (`swzd9_116d` is (1, 1, 1, x), `swz46_0024` (0, 0, x)).
- Temporaries (`t1 = …`) are transcribed in order as shared values; never paste them into each other's text (the expressions multiply).

## Cooking and looking

```sh
cargo run -q --release -p canopy-cook -- --data "$PWD/target/wc3-lab" --out target/cooked-wc3lab --effect-budget 400
```

`just tidy` (and so `just check`) deletes every `target/cooked-*`, the lab's pack included: cook it again (about two minutes) when the lab says "no cooked asset pack".

`--effect-budget <MiB>` lifts the resident effect-image budget for the lab only (the 452 textures need about 230 MB; the shipped budget is 64 MB).

- **In the editor:** `./target/release/canopy --data "$PWD/target/wc3-lab"`, Editor → Effects → `wc3`; each effect plays by itself on its Stage and previews on its Graph (every node previewing).
- **Headless captures:** `CANOPY_CAPTURE_EFFECT=effects/wc3/<file>.json CANOPY_CAPTURE_EFFECT_ID=effect.wc3.<file> CANOPY_CAPTURE_TICKS=4,10,18,30,48 ./target/release/canopy capture-screens --data "$PWD/target/wc3-lab" --only effect-graph --out <dir>` shoots the graph and the stage at those ticks.
- **A lone clip:** `./target/release/canopy shot --data "$PWD/target/wc3-lab" --map <map> --camera x,z,zoom --hour 12 --effect <id>,0 --sequence <frames>,<step> --out f.png` (pick a dark map; frost washes additive effects out).

## Warcraft's spells as abilities

`target/wc3-lab/wc3_spells.py` (in the lab, never committed) writes Resurrection, Tranquility, Starfall and Flame Strike as lab abilities (`abilities/wc3/`) with Warcraft III's own numbers and their Reforged effects bound as its ability skins bind them, plus a caster (`unit.wc3spells.caster`) that binds all four. Run it again after the lab is re-cloned. Read the numbers from the game's tables (`units/abilitydata.slk`, `units/abilityskin.txt`, `units/unitweapons.slk` for cast points, and the strings under `_locales/enus.w3mod:units/`) with `casc-extract`. Conversions: a tile (128 units) is 5 cells (areas as Warcraft's: abilities may reach 4,096 cells); a second is 40 ticks. Open one in the lab editor (Abilities → `wc3` → its Stage, Cast); self-cast spells stage the scene they act on (corpses, allies or enemies).

**Ragdoll Hill** (`maps/showcase/ragdoll-hill.utcm`, lab only): a two-tier hill made in the block editor (summit 9 m, terrace 6 m, ground 3 m; a two-step ramp north, a sheer 6 m cliff east), two Spell Testers on the summit and still creep camps at the east, north and south edges, for watching spell kills throw ragdolls over edges and down ramps: `./target/release/canopy --data "$PWD/target/wc3-lab" --map ragdoll-hill`. It is not rebuilt by `wc3_spells.py`: a re-cloned lab loses it.

Headless: `CANOPY_CAPTURE_ABILITY=ability.wc3.<name> CANOPY_CAPTURE_TICKS=30,60,… ./target/release/canopy capture-screens --data "$PWD/target/wc3-lab" --only effect-graph --out <dir>` shoots its Stage at those simulation ticks.

## Worth studying

Richest by layers and nodes: building deaths (`sharedfx-*buildingdeath-large`, 20–27 layers), `iceexplosion-iceexplosioncinematic`, `cleansingfire-cleansingfirecaster`, `consecration`, `righteousfury`, `lightningshield`, `massteleport-massteleportto`. By kind: fire (`flamestrike-*`, `rainoffire-rainoffiretarget`, `breathoffire-*`, `immolation*`, `incinerate-*`, `volcano-*`, `phoenix*`), holy (`holybolt`, `divineshield`, `resurrect`, `levelup`), sky (`starfall-*`, `blizzard-*`, `tranquility`), gates (`buildingfx-waygate`, `darkportal-*`, `massteleport-*`), novas (`warstomp`, `thunderclap`, `frostnova`), missiles (`abilities-weapons-*`).

Without reference footage we cannot confirm a one-to-one match; Jiri records it, or Claude drives the game with his yes (D113 decision 5).
