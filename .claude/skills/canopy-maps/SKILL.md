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
- `size`: playable cells per side, one of 256, 512, 640, 1024, 2048.
- `biome`, `sandbox`, `initialExploration`.
- `playerStarts`: `[{player, x, z, setup, mainFort}]`, where `setup` is a race starting setup id such as `setup.ants`.
- `entities`: pre-placed units and buildings: `{id, definition, position: {x, y}, rotation, owner: "none" | "player.N", initialState?, appearance?: {asset, scale}}`.
- `camps`: neutral camps: `{id, members, home, aggroRange, leash, aggression, lootPool}`.
- `stamps`: scenery placed with walk surfaces.
- `landscape.environment`: `{hour, weather}`.
- `mission`: campaign metadata, objectives and regions; the script is in `mission.lua`.
- `terrain`: `{textureGrid, waterProfiles, paint: [{material, variants}]}`.

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
| `paint-<n>-weights.grid` | f32 per vertex | 0 to 1 |
| `paint-<n>-variants.grid` | u8 per texture cell | atlas variant |

A `.grid` file is:
- a 16-byte header: `UTCG`, format 1, sample type, codec, filter, width, height;
- then the samples, little-endian, byte-shuffled, zlib-compressed.

Samples come back bit-exact (`crates/content/src/map/grid.rs`).

## Scenery

`scenery.json` objects: `{id, asset, x, z, elevation, yaw (radians), scale, heightMode, visible, locked}`. `asset` is a scenery id (`scenery.woodland-pine-a`, skill **canopy-assets**).

Order matters. The simulation turns each harvestable piece standing on playable ground into a resource, numbered in object order. Only the first one on a cell counts.

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
