---
name: canopy-maps
description: The map package format of Under the Canopy (.utcm): what each file holds, terrain grids, scenery placement, starts, camps, missions, previews, and how maps are found, identified and tested. Use when reading, generating, converting or editing maps.
---

# Map packages

A map is a package (skill **canopy-packages**): a folder named `<id>.utcm`.
- **Shipped maps:** `base/maps/<group>/` (groups: `skirmish`, `campaign`, `showcase`). A campaign's missions live in its package instead: `base/campaigns/<id>.utcc/missions/<map>.utcm` (docs/campaign.md).
- **Player maps:** the user folder's `maps/` (own) and `downloads/` (received).

Authority:
- `crates/content/src/map/package.rs` (the container);
- `map/mod.rs` (the in-memory `UtcMap`);
- `map/authoring.rs` (terrain);
- `map/check.rs` and `map/validate.rs` (rules).

```
frozen-dream.utcm/
├─ package.json     id (the map id), kind "map", name (shown in menus), description, version
├─ map.json         everything but the terrain and scenery
├─ scenery.json     {"objects": [...]}: placed scenery, in order
├─ terrain/         the blocks, and the grids they make (.grid files)
├─ mission.lua      the mission script (missions only)
└─ preview.webp     the menus' picture (cooked into the pack as maps/<id>/preview.webp)
```

## map.json

The main fields:
- `size`: cells per side, one of 256, 480, 512, 640, 800, 960, 1024, 2048 (the renderer draws up to 1024).
- `playable` (optional): `{x, z, width, height}` in cells, the part play happens in. Around it is a border of scenery nobody enters (unwalkable, unbuildable); the camera, minimap and preview frame it. Starts stand at least 8 cells inside it. The block editor outlines it and sets its border.
- `biome`, `sandbox`, `initialExploration`.
- `playerStarts`: `[{player, x, z, setup, mainFort}]`, where `setup` is a race starting setup id such as `setup.ants`.
- `entities`: pre-placed units, buildings and items: `{id, definition, position: {x, y}, rotation, owner: "none" | "player.N", initialState?, appearance?: {asset, scale}}`.
  - An item placed with owner `none` lies on the ground for a hero to pick up (a hidden cache, a dead scout's satchel).
  - Neutral features (skill **canopy-definitions**): a breakable crate or rubble drops `initialState.inventory` (`{"item.moon-dew": 1}`) or a roll of `initialState.lootPool`, else its definition's loot; a shop sells `initialState.wares` in place of its definition's. Buildings turn in quarter turns (`rotation` 0, 90, 180, 270): rubble 12 × 4 closes a gap either way.
- `camps`: neutral camps: `{id, members, home, aggroRange, leash, aggression, lootPool}`.
- `stamps`: scenery placed with walk surfaces.
- `landscape.environment`: `{hour, weather}`.
- `mission`: `{objectives, regions, players, cameras}`; the script is in `mission.lua`. `cameras` are the saved cameras its scenes cut and blend to (`{id, x, y, height?, distance, pitch, yaw, fov, roll?}`, placed in the map editor; docs/campaign.md "Scenes" has them and the scene API). Everything about the campaign (its order, the loading art and story, the player's race, the hero level cap, the tech) is the campaign's `campaign.json` (docs/campaign.md "Formats"), so a mission map plays on its own too. A mission needs no player starts: a player with a start gets its race's base as in a skirmish, a player without one begins with only what `entities` places for it. The mission script decides the outcome (`mission.win()`, `mission.lose()`); losing every building ends nothing by itself.

map.json has no `terrain` section (D82): a map is made of blocks, and its ground, cliffs and dressing are generated from them by its biome at load (D80). A map in the old format (painted ground layers) is refused.

## Terrain grids

A map is made of **blocks** (D80, `crates/content/src/map/blocks.rs`): `blocks.grid` is what it is authored as, and the block editor (or MCP `map_blocks`) writes it together with the grids it makes (`Blocks::apply`), which the simulation reads and which are stored so a map plays the same forever. Never write heights without their blocks.

The terrain covers the playable square plus a 16-cell halo on every side:
- `span = size + 32` cells;
- `vertices = span + 1` per side;
- index `z × side + x`, starting at world coordinate -16.

| File | Samples | Values |
|---|---|---|
| `blocks.grid` | 4 bytes per 4 × 4-cell column | level, kind, cut and ramp direction, face style (`blocks.rs`) |
| `block-paint.grid` (optional) | u8 per cell | 0 keeps the column's kind, else kind + 1 |
| `heights.grid` | f32 per vertex | made from the blocks |
| `water-heights.grid` | f32 per vertex | made from the blocks; -17 means dry |
| `water-flow.grid` | 4 bytes per cell | x, z (128 still), foam, profile (0 = the biome's water) |
| `blocked.grid` (optional) | u8 per cell | made from the blocks: nonzero is closed (forest floor, rock), never walked or built on |

A `.grid` file is:
- a 16-byte header: `UTCG`, format 1, sample type, codec, filter, width, height;
- then the samples, little-endian, byte-shuffled, zlib-compressed.

Samples come back bit-exact (`crates/content/src/map/grid.rs`).

## Scenery

`scenery.json` objects: `{id, asset, x, z, elevation, yaw (radians), scale, heightMode, visible, locked}`. `asset` is a scenery id (`scenery.woodland-pine-a`, skill **canopy-assets**).

Order matters. The simulation turns each harvestable piece standing on playable ground into a resource, numbered in object order. Only the first one on a cell counts. The base game's trees are not harvestable (D62): they are scenery whose blockers close the ground.

**Substance Deposits** (`building.neutral.corrupted-root`) are entities, like amber mines. A race's Substance Well is built exactly on one (`placementOn`), so place each on flat, open, dry ground every army can walk to, with its 8-cell footprint on the building lattice (edges at -0.5 + 4k) and its 3-cell clearance free of trees and other buildings.

## Reading and writing in code

```rust
let map = canopy_content::map::package::load(path)?;          // reads and checks; a UtcMap
canopy_content::map::package::write(&map, "my-map", "1.0.0", out)?;
let summary = canopy_content::map::package::summary(path)?;   // menus: without the terrain
let files = canopy_content::map::library(data)?;              // the base package's maps
```

In memory a map is one `UtcMap`. Its JSON form (`UtcMap::parse`, grids as base64) is what tests build fixtures with.

## Identity

A map's identity is its package digest (its `.json`, `.lua` and `.grid` files). Saves record it, and multiplayer clients compare it before a match starts. Any change to those files makes a different map. Re-saving with identical content gives identical bytes.

## Testing a map

```bash
cargo test -q -p canopy-content --test map_shipped                 # every shipped map loads and passes the rules
just shot --map <id> --camera <x>,<z>,<zoom> --out target/shots/m.png
cargo run --release -p canopy-sim --example ai_duels -- --data .   # AI duels on every map
```
