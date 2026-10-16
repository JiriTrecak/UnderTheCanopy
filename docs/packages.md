# Content packages (phase 2, M0)

Agreed with Jiri, 2026-10-16 (D53). This is the data layer every later editor, test and agent builds on: one structure for the base game, campaigns, maps and, later, mods. It replaces `content/`, `assets/` and `art/assets/` and their registries. The game plays identically after the migration; the old TypeScript editors stop working, and that is accepted.

## Packages

Everything is a package: a folder for authoring and git, or (with map transfer, M5) a zip of the same tree for sharing. Every package has the same tree; kinds differ only in their manifest and extra files.

| Kind | Form | Notes |
|---|---|---|
| `base` | `base/` in the repository; read-only inside the app | The whole shipped game. |
| `campaign` | `<name>.utcc` | Campaign file, missions (`.utcm`), campaign-wide overrides and art. Shipped campaigns live inside base under `campaigns/`. |
| `map` | `<name>.utcm` | A map; may carry overrides and new definitions for itself. |
| `mod` | later | Same tree, no extra files. |

`package.json` (every package):

```json
{
  "format": 1,
  "id": "echo-isles",
  "kind": "map",
  "name": "Echo Isles",
  "version": "2.2.0",
  "author": "…",
  "description": "…",
  "dependencies": { "base": ">=0.4" },
  "license": "…"
}
```

## Load order and layering

- Packages load in order: base, then mods in the player's order, then the campaign, then the map, always last. A package always loads after the packages it depends on.
- **The same id in a later package overrides** the earlier one; there are no namespaces (RimWorld's model). New ids that two packages happen to share resolve the same way, later wins. The editor warns when an element shadows an id from another package.
- **An override merges**: the later file holds only the fields it changes. Objects merge field by field; **lists always replace** whole; `null` removes a field.
- **`{ "id": "…", "removed": true }`** removes a definition entirely (a map that bans a unit).
- **`"base": "<id>"`** makes a new definition start as a copy of another (as Warcraft III's custom units and StarCraft II's `parent`). Chains are allowed; cycles are errors.
- The loader resolves all layers into the one `Content` the simulation already uses; the simulation does not know about packages. Errors name the package and file responsible. The loader keeps which layer set each field, for the editor (overridden fields highlighted, reset to base).
- The ordered list of packages (id and hash each) plus the simulation ABI is the content identity of a match, a save and a replay.

## Elements, folders and ids

- **One element per file**, or one folder with a manifest for elements made of several files: assets (a folder containing `asset.json`), maps (`.utcm`), campaigns (`.utcc`).
- **The top-level folder gives the type** (`units/`, `abilities/`, …). Everything beneath it is free organisation, loaded recursively, with no meaning to the game: `abilities/heroes/marshal/rally.json`. The editor's tree is the directory tree; saving writes an element back where it came from, moving it in the editor moves the file. An empty folder keeps a `.folder` marker so git retains it.
- **The id is inside the file**, never derived from the path, so moving or renaming files breaks nothing. File names follow the id by convention and the editor keeps them in step. Two elements with one id in the same package is an error.
- Ids keep their current form (`unit.ants.settler`) except for assets (see Assets).

## Schemas, validation, versions

- The Rust types are the only definition of every format (strict: unknown fields are errors, and every error names its file). JSON Schemas generated from them for the editor's forms, MCP tools and agents, and the authoring limits (ranges, D12) as schema annotations the loader enforces, are **not done yet**: they need the `schemars` crate and come with the editor shell (M1), where forms and MCP tools consume them.
- Until then the agent skills (below) are the format reference; a test parses their examples against the Rust types.
- Each package records its format version. Old packages are upgraded by migrations in Rust, so players' maps survive our format changes.

## The base package tree

```
base/
├─ package.json
├─ rules/
│  ├─ game.json            unit and building caps, supply, construction, repair
│  ├─ combat.json          armour and damage types, damage table, armour formula, hero stun
│  ├─ experience.json      experience tables, neutral and summon multipliers
│  └─ commands.json        command card layout, categories, navigation (today's "actions")
├─ races/                  name, description, roster, construction, starting setup
├─ behaviors/              behaviour sets
├─ units/                  e.g. ants/warrior.json, ants/heroes/marshal.json, neutral/forest/…
├─ buildings/
├─ items/                  e.g. resources/amber.json, artifacts/legendary/…
├─ resources/              harvestable resources (trees)
├─ research/
├─ loot-pools/
├─ abilities/              one file each: definition and presentation together
├─ effects/
├─ scenery/                placeable scenery: category, model, blocker, what it yields
├─ landscape/
│  ├─ biomes/
│  └─ water/
├─ assets/                 one folder per asset (see Assets)
├─ campaigns/              <name>.utcc
└─ maps/                   e.g. skirmish/echo-isles.utcm
```

Moves from today's data:

- `content/game.json` splits into one file per definition (`units/`, `buildings/`, `items/`, `resources/`), `behaviors/`, `rules/commands.json`, `races/`, `research/`, `loot-pools/`, `campaigns/` and the three `rules/` files. The global `startingSetup` is the Ants start and moves into `races/ants.json` (Beetles already carry theirs). `supplyIcon` moves to presentation and `defaultRace` to the menus; the `id` leftover goes.
- **AI is code, not data.** `rules.ai` and the races' `ai` sections leave the data: think rates, budgets and personality become constants in the AI code. The AI should read the game data and react to the opponent rather than follow fixed lists; custom races later bring their own AI as a Lua script.
- Abilities: `content/abilities/<id>/{definition,presentation}.json` become one file each; `published.json` and `folders.json` go (folders are real directories).
- Effects: `content/effects/<id>/effect.json` become one file each; `published.json` goes.
- `config/biomes/` moves to `landscape/biomes/`. The 3 water-profile "assets" (JSON only) move to `landscape/water/`. The 22 landscape recipes (the old editor's generators) are removed: the new map editor will not use them.
- `content/neutral-camps.json` (unread; camps live in maps) and `content/import/warcraft/` (tool settings) leave the package.
- `config/ui`, `players.json`, lighting, water rendering and glow stay in `config/`: engine settings, not moddable content.

## Assets

Today five registries describe 317 assets: `art/assets/<id>/` (the source), a file copy in `assets/library/`, and `published.json` (byte-identical to the sources), `manifest.json`, `catalogue.json` and `models.json`, all generated from the sources by the old studio. After M0 one folder per asset is the only copy:

```
assets/units/ants/amber-adept/
├─ asset.json
├─ geometry.glb
├─ albedo.png
├─ team_mask.png
└─ source.blend            working source: kept in git, never shipped
```

- **One id per asset**, the id in its `asset.json`. Definitions reference assets directly (`"model": …`, `"icon": …`). The secondary ids that assets declare today (render entries such as `asset.ants.amber-adept`, icon entries such as `icon.ants.amber-adept`, scenery prototypes such as `mega-oak-trunk`) go away; a model's portrait becomes its own icon asset.
- **`asset.json` describes only its files**: kind, name, tags, the files and their roles, transform, materials, team-colour mask, animations with their events, sockets, ground contact, blocker and clearance, wind, canopy, licence note.
- **Unit presentation moves to the unit definition**: scale, health-bar height, projectile and its socket, cast timing (today in the model's render entry, beside the unit's own `modelScale`).
- **Placeable scenery becomes the `scenery/` definition kind** (category, model, blocker, harvesting), moved as it is; the map editor milestone redesigns it.
- `revision`, `status`, file hashes and `exportSettings` (old studio bookkeeping) go; git keeps history and the cooker computes hashes. The cooker reads asset folders directly and generates every index the game needs. Working files (`.blend`, recipes) are never packaged.
- Image-generation styles and profiles (`art/styles`, `art/profiles`) are tool inputs and move out of the package.

## Map package (`.utcm`)

Today Echo Isles is one 32 MB JSON file: terrain grids 31.6 MB as base64 text (ground painting 19.5 MB; heights, water heights, water flow, grass and rock 2.4 MB each), 2,052 placed scenery objects 480 KB, 106 units and buildings, 19 camps, 2 starts. After M0:

```
echo-isles.utcm/
├─ package.json
├─ map.json          size, biome, description, starts, units and buildings, camps, environment
├─ scenery.json      placed trees and props
├─ terrain/          one compressed binary file per grid
│  ├─ heights.bin  water-heights.bin  water-flow.bin  grass.bin  rock.bin
│  └─ paint/<layer>.bin
├─ mission.lua       (optional)
├─ preview.webp
└─ units/ abilities/ …   (optional) this map's overrides and additions
```

The terrain data stays what it is today; only the container changes. Map data will change often as maps gain features: a new feature is a new file, and maps without it get a default.

## Where packages live

- The base package ships inside the app, read-only. Developer mode (`--data <repository>`) edits the repository's.
- The user folder (the OS data directory, D6) holds `maps/` (the player's own, created at start) and `downloads/` (received); `mods/` later. The menus list them after the shipped maps; a map whose id is already taken is left out with a warning.
- Packages are folders today. Reading a package from a zip of the same tree comes with multiplayer map transfer (M5), where maps start to travel.

## The match server

The server stops embedding `content/game.json`; it validates the shape of seat choices and leaves content to the content identity every client sends before a match starts. Map transfer is a later milestone.

## Migration details

Settled from a full read of today's data and every consumer (2026-10-16). Each keeps gameplay identical unless it says otherwise.

**Loading.** Per collection: layer the packages, resolve `base`, then resolve behaviour sets exactly as today (they merge one level deep, field by field, with conflict errors; keep that), then parse into the Rust types (defaults apply only here), sort, validate. Errors name the element's own file. A `null` in the bottom layer stays a value (command overrides use `null` to clear a category); only a patch's `null` removes a field. `Content` is resolved per match, because a map may carry overrides; menus use the packages without a map.

**Ordering.** Definitions are already sorted by id. Abilities are numbered by their position in today's published file; the loader sorts them by id instead. Measured: every per-tick fingerprint on both maps, AI matches and crowd battles, stays identical; only saved ability numbers change.

**Races and the default race.** `rules/game.json` keeps `defaultRace`: the simulation uses it for seats without a race (AI matches, tests, campaign starts) and for map checks that need a fort (`playable`, resource clearance). The global starting setup becomes `races/ants.json`'s own (keeping id `setup.ants`, which maps name); the "global setup" fallbacks become "the default race's setup".

**AI.** Before the migration, a separate step makes the AI choose its army and spells from the race's data (what its buildings train, what its heroes learn) instead of the composition and skill-preference lists, measured with AI duels; then the baseline is recorded again. AI timing and limits become code constants with today's values. `revision`, `limits.spellCandidates` and `heroStunDurationPermille` are unused and go.

**Commands.** `rules/commands.json` holds navigation, categories and actions. The nine `build:<building>` overrides move into their buildings (`command: {priority, category, hidden, hotkey}`), so removing a building removes its override.

**Abilities and effects.** `published.json` is a plain copy of the sources (checked: identical values for all 24 abilities and 49 effects). An ability file holds its presentation inline under `presentation` (icon, animations, effect bindings); presentation ids go. The per-element `schemaVersion` fields and the library `abi` go; the package format replaces them. Folders follow owners: `abilities/heroes/marshal/rally.json`, `abilities/ants/rootweaver/root-bind.json`, `effects/weapons/arrow.json`.

**Assets.** References change from render ids to asset ids (definitions, research, armour and supply icons, commands, map appearances, `config/glow.json`, one HUD literal). Of the 277 render entries 73 are referenced by nothing and go, with the 24 dead placeholder ids and the unused geometry of legacy models. Where each render field goes:

| Render field | New home |
|---|---|
| `file`, `image` | the asset's files, by role |
| `scale`, `character` (animation profile), `healthHeight` | the model's `asset.json` (`display`): the same for every use of a model today. A definition may override `healthHeight` (the placeholder model stands in for many units). |
| `projectile`, `projectileSocket`, `impactEffect`, `castContact`, `cameraAnchor` | the unit definition's `presentation` |
| `harvestAnimation` | implied: the model's own clips |
| `sceneryAsset` | the `scenery/` definition |
| `dimensions`, `sockets`, `stackHeight`, `stackColumns`, `carryAsset` | deleted (duplicates of the model's capabilities, or unused) |

The worker's folder holds the carried wood, amber and root models: they become three item assets. The 56 unit and building portraits become icon assets of their own. Definitions name their model `model` (today `asset`). Pack entries are named by asset id and file role instead of `assets/library/…` paths, including the UI theme's images and the renderer's few fixed textures.

**Scenery.** The 73 scenery prototypes become `scenery/` definitions with ids `scenery.<name>`: name, category, model, blockers, deck, light, hidden in the editor, and harvesting (which resource it becomes, as the landscape catalogue says today). Map objects and biome forests reference scenery ids. Terrain materials stay assets (textures plus their projection and atlas settings); water profiles move to `landscape/`.

**Maps.** Terrain grid files use the extension `.grid` (`.bin` is stored in Git LFS): a small header (magic, element type, width, height, filters), then the values little-endian, byte-shuffled and deflate-compressed, bit-exact. Echo Isles' terrain shrinks from 31.6 MB of base64 to about 0.6 MB. Paint layers are listed in order in `map.json` (order decides blending), one weights grid and one variants grid each. The version wrappers, the duplicate terrain size and retired fields go. Scenery objects keep their order (the simulation numbers resources by it). Map previews move into their packages.

**Identity.** A package's hash is SHA-256 over its files, sorted by path, each prefixed by its path and length. Match, save and lobby identity use the ordered package hashes (the map's included) instead of today's five files and the map file.

**The match server** stops reading `game.json`: it no longer lists races and heroes or fills them into seats; the client takes them from its own content and the simulation fills seats left empty with the default race and its default hero. Clients older than the release that ships this cannot use the new server.

## Migration and acceptance

- A one-time converter turns today's data into `base/` and deletes the old folders and readers.
- **The game plays identically**: seeded AI-versus-AI matches on both maps produce the same replay hashes before and after, and captures of both maps and of every unit render the same.
- Saves made before the migration no longer load (their content identity is gone); tell players in the release notes.

## Agent skills

Skills in `.claude/skills/` describe the formats for agents: `canopy-packages` (packages and layering), `canopy-definitions` (units, buildings, items, research, races, rules), `canopy-abilities` (abilities and effects, from the spell studio's authoring reference), `canopy-assets` (asset folders, scenery, landscape), `canopy-maps` (map packages). Every example tagged with a kind (```` ```json unit ````) is parsed by `crates/content/tests/skills.rs`, so a skill cannot drift from the code.
