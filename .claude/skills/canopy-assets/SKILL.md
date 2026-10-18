---
name: canopy-assets
description: How Under the Canopy stores art (models, textures, icons, portraits, sounds, ground textures) as asset folders, how definitions and maps reference them, placeable scenery, landscape files, and how assets reach the cooked pack. Use when adding, replacing, reviewing or referencing any art.
---

# Assets

Read **canopy-packages** first. Assets live in a package's `assets/` folder:
- **one folder per asset**, anywhere below `assets/` (free organisation);
- each holds an `asset.json` and its files.

The id inside `asset.json` is the asset's only id. Everything references assets by it:
- definitions: `model`, `icon`;
- spells: `presentation.icon`;
- effects: `{asset, role, index}`;
- maps: placement `appearance.asset`;
- biomes, rules icons, `config/glow.json`.

Authority: `crates/content/src/assets/package.rs` (formats and the registries built from them) and `assets/models.rs` (capabilities).

## asset.json

```json asset
{
  "id": "asset.models.units.example-spearman",
  "name": "Example Spearman",
  "kind": "unit",
  "tags": ["example"],
  "race": "ants",
  "resources": [
    {"role": "geometry", "index": 1, "format": "glb"},
    {"role": "albedo", "index": 1, "format": "png"},
    {"role": "team_mask", "index": 1, "format": "png"},
    {"role": "source", "index": 1, "format": "blend"}
  ],
  "transform": {"scale": 1, "pivot": [0, 0, 0], "up": "Y", "forward": "+Z"},
  "capabilities": {
    "teamColor": {"mode": "mask", "slots": ["TC_TeamColor"], "mask": {"role": "team_mask", "index": 1}},
    "animations": [
      {"semantic": "idle", "clip": "idle", "loop": true, "events": []},
      {"semantic": "attack", "clip": "attack", "loop": false, "events": [{"name": "hit", "time": 0.5}]}
    ],
    "sockets": [{"name": "projectile", "node": "socket_projectile", "offset": [0, 0, 0]}],
    "groundContact": {"mode": "pivot"}
  },
  "display": {"scale": 2, "character": "base", "healthHeight": 2.2},
  "license": "Original artwork for Under the Canopy."
}
```

**Files** are named `<role>.<format>`, or `<role>_<n>.<format>` for the n-th file of a role. Every listed file must exist.
- Runtime roles: `geometry`, `albedo`, `normal`, `roughness`, `metalness`, `occlusion`, `team_mask`, `emissive`, `image`, `audio`, `data`.
- `source`, `recipe`, `build`, `generation` and `reference` are working files. They are kept in git but never cooked or shipped.

**Kinds:** `unit`, `creature`, `building`, `tree`, `foliage`, `prop`, `bridge`, `terrain-material`, `icon`, `interface`, `texture`, `audio`.
- An icon (or a portrait) is its own asset of kind `icon` with one `image` file. Model assets don't carry portraits.

**Model fields:**
- `transform` is applied when cooking.
- `display` is how the model is shown everywhere: runtime scale, animation profile (`character`), health bar height. A definition may override the health bar height and profile in its own `presentation`.
- `capabilities`:
  - `teamColor`: which material slots take the player colour, through a mask or a whole slot;
  - `animations`: the semantic → clip map, with events such as the `hit`/`release` contact time as a share of the clip; a gait (`walk`, `run`, `charge`, `carry_walk`, `carry_run`) may declare `nominalSpeed`, the ground speed in the model's own metres per second at which its clip plays at rate 1 without sliding feet. The clip's rate is then the unit's measured ground speed ÷ (`nominalSpeed` × the scale it is drawn at), clamped to 0.4–3×. Without it, gaits play at the engine's 1.5×, as the ants do;
  - `sockets`: model nodes for missiles, cargo and effects;
  - `groundContact`: `pivot` or `terrain`;
  - `wind`, `canopy`, `blocker`, `vegetationClearance`, `dimensions`.
- `shading: "building"` gives a structure the building material; the default is prop.
- **Buildings at work** (D65): a building model may declare the states `idle` (its rest pose: a still clip, nothing moves), `work` (loops while it works) and `spawn` (once per trained unit). Moving parts are rigid nodes, not skins; event `time` is seconds into the clip. A material's glTF extra `activeGlow: {pulse, flow}` makes its emission dark at rest and alive while the building works. Built by `tools/buildings/<building>.py` from a segmented Tripo model; review on the `beetle-bench` map.

**Ground textures** (`terrain-material`) carry `terrain`: tile size, atlas, channel roles and packed runtime tiles.

## Pack entries

`just cook` builds `target/cooked/canopy.pack` from the asset folders. Each runtime file becomes the entry `<asset id>/<file name>`, for example `asset.icons.command-attack/image.png`, wherever its folder sits. Code and UI config that name a file directly use that form: `config/ui/theme.json` images, biome textures. `just cook --inspect <asset id>` shows what an asset cooked to.

The cooker also stages the base package's documents into `target/cooked/base` for packaging.

## Placeable scenery

`scenery/<free folders>/<name>.json` is a piece maps place (objects in `scenery.json`) and biomes use (forest trunks). It names its model and its placement rules. A piece with `harvesting` (`{"definition": <resource>}`) becomes that resource where it stands on playable ground. In the base game trees are no resource (D62): a tree's trunk closes the ground with a `blocker` instead, so tree walls stay solid.

```json scenery
{
  "id": "scenery.example-pine",
  "name": "Example pine",
  "category": "foliage",
  "type": "prop",
  "model": "asset.models.environment.woodland-pine-a",
  "blocker": {"width": 3.3, "depth": 3.3, "shape": "ellipse"}
}
```

Optional fields:
- `blocker`/`blockers`: ground collision in the model's metres;
- `deck`: a walkable bridge deck;
- `light`;
- `editorHidden`.

## Landscape

- `landscape/biomes/<id>.json` sets each biome's look: lighting, fixed ground layers, water, forest pieces. The renderer reads the base package's biomes. Its optional `motes` adds dust in the canopy's light and falling leaves; `atmosphere.overheadStrength` and `shaftElevation` shape the light shafts per camera (D64).
- `landscape/water/<name>.json` is a water profile (a biome's water, or one a map's water flow names). An optional `glint` (0 none) adds sun glitter on the ripples.

```json water
{
  "id": "water.example-pond",
  "name": "Example pond",
  "water": {
    "shallowColor": "#2b5c60", "deepColor": "#102832", "clarity": 0.85, "rippleScale": 0.15,
    "rippleStrength": 0.055, "foamStrength": 0.02, "reflectionStrength": 0.5, "causticStrength": 0.18,
    "cloudStrength": 0.03, "flowSpeed": 1
  }
}
```

## Reviewing art

AGENTS.md has the unit readability rules: check at game zoom, from above, and in every player colour.

```bash
just shot --map turtle-rock --gallery --camera 310,322,0.7 --out target/shots/units.png
```

`--gallery` places one of every unit and building around the middle of the map's starts.
