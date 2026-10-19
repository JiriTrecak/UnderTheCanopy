---
name: canopy-maps
description: The map package format of Under the Canopy (.utcm): what each file holds, terrain grids, scenery placement, starts, camps, missions, previews, and how maps are found, identified and tested. Use when reading, generating, converting or editing maps.
---

# Map packages

A map is a package (skill **canopy-packages**): a folder named `<id>.utcm`.
- **Shipped maps:** `base/maps/<group>/` (groups: `skirmish`, `campaign`, `showcase`).
- **Player maps:** the user folder's `maps/` (own) and `downloads/` (received).

Authority:
- `crates/content/src/map/package.rs` (the container);
- `map/mod.rs` (the in-memory `UtcMap`);
- `map/authoring.rs` (terrain);
- `map/check.rs` and `map/validate.rs` (rules).

```
echo-isles.utcm/
├─ package.json     id (the map id), kind "map", name (shown in menus), description, version
├─ map.json         everything but terrain samples and scenery
├─ scenery.json     {"objects": [...]}: placed scenery, in order
├─ terrain/         one .grid file per terrain grid
├─ mission.lua      the mission script (missions only)
└─ preview.webp     the menus' picture (cooked into the pack as maps/<id>/preview.webp)
```

## map.json

The main fields:
- `size`: cells per side, one of 256, 480, 512, 640, 800, 960, 1024, 2048 (the renderer draws up to 1024).
- `playable` (optional): `{x, z, width, height}` in cells, the part play happens in. Around it is a border of scenery nobody enters (unwalkable, unbuildable); the camera, minimap and preview frame it. Starts stand at least 8 cells inside it. Warcraft III imports set it from the map's margins (decision D56).
- `biome`, `sandbox`, `initialExploration`.
- `playerStarts`: `[{player, x, z, setup, mainFort}]`, where `setup` is a race starting setup id such as `setup.ants`.
- `entities`: pre-placed units and buildings: `{id, definition, position: {x, y}, rotation, owner: "none" | "player.N", initialState?, appearance?: {asset, scale}}`.
- `camps`: neutral camps: `{id, members, home, aggroRange, leash, aggression, lootPool}`.
- `stamps`: scenery placed with walk surfaces.
- `landscape.environment`: `{hour, weather}`.
- `mission`: campaign metadata, objectives and regions; the script is in `mission.lua`.
- `terrain`: `{textureGrid, waterProfiles, paint: [{material, variants}], overlay?}`. The optional `overlay` (`{asset, x, z, width, height}`, metres) lays one painted texture asset, premultiplied alpha, over the ground in that rectangle (D63).

**The order of `terrain.paint` matters.** Later layers paint over earlier ones, and cliffs come after non-cliffs. Each layer has `terrain/paint-<n>-weights.grid`, plus `paint-<n>-variants.grid` when `variants` is true. `material` is a terrain-material asset id.

## Terrain grids

The terrain covers the playable square plus a 16-cell halo on every side:
- `span = size + 32` cells;
- `vertices = span + 1` per side;
- index `z × side + x`, starting at world coordinate -16.

| File | Samples | Values |
|---|---|---|
| `heights.grid` | f32 per vertex | -16 to 24 |
| `water-heights.grid` | f32 per vertex | the water surface; -17 means dry |
| `water-flow.grid` | 4 bytes per cell | x, z (128 still), foam, profile (0 = the biome's water) |
| `grass.grid`, `rock.grid` | f32 per vertex | 0 to 1 |
| `shade.grid` (optional) | f32 per vertex | 0 lit to 1 black: the frame shade (D63) |
| `blocked.grid` (optional) | u8 per cell | nonzero: closed, never walked or built on (D71). Paint it from the same mask as the dark frame: what looks impassable must be |
| `paint-<n>-weights.grid` | f32 per vertex | 0 to 1 |
| `paint-<n>-variants.grid` | u8 per texture cell | atlas variant |

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
