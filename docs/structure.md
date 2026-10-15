# Repository structure

```
UnderTheCanopy/
├─ Cargo.toml          Rust workspace
├─ justfile            every command: just play | profile | cook | check | test | package | server
├─ AGENTS.md           rules for anyone changing this repository
├─ crates/             libraries, named canopy-*; added when a milestone needs them
│  ├─ content/         data model, loading, validation (M1)
│  ├─ sim/             deterministic simulation; depends only on content (M2)
│  ├─ assets/          runtime asset format and loader (M3)
│  ├─ cook/            published assets → runtime assets, library and CLI (M3)
│  ├─ render/          wgpu renderer; knows nothing about the simulation (M3)
│  ├─ scene/           simulation snapshots → render world, interpolation (M3)
│  ├─ ui/              HUD toolkit: taffy layout, cosmic-text (M4)
│  ├─ audio/           playback, voice limiting, effect sounds, music (M4)
│  ├─ net/             client for the existing match server protocol (M5)
│  ├─ release/         release log, update feed, package signatures (D43)
│  └─ release-tool/    `just release-*`: notes, prepare, build, publish (docs/releases.md)
├─ game/               the executable: window, input, platform paths, screens, updater, packaging
├─ content/            gameplay data (see below)
├─ assets/             published asset outputs, registry, manifest and maps
├─ art/                editable masters and exported outputs, Git LFS
├─ config/             presentation settings owned here: lighting, biomes, player colours (D30)
├─ releases/           the release log, gallery media and the release host's identifiers (D43)
├─ server/             the match server, TypeScript on node: lobby and lockstep relay (docs/server.md)
├─ docs/               plan, decisions, design; reference/ describes the old game
└─ tools/              repository scripts
```

Phase 2 adds `editors/{map,assets,spells,shared}` (one Tauri app each).

## Dependency rules

- Crates never depend on `game/`.
- `canopy-sim` depends only on `canopy-content` (and deterministic math and Lua). No I/O, no clocks, no threads it doesn't own deterministically.
- `canopy-render` never sees simulation types; `canopy-scene` is the only bridge.
- Content is always passed in. No code names a shipped content id (`unit.ants.warrior`, a race, a map); tests use their own fixtures.

## Data

| Folder | What | Authored by |
|---|---|---|
| `content/` | `game.json` (definitions, rules, behaviour sets, actions), ability and effect folders, neutral camps | the old editors, through the links |
| `assets/library/` | published asset outputs (glb, png, wav, recipe and water definitions) | the old editors, through the links |
| `assets/manifest.json`, `assets/authoring/` | runtime manifest, publication registry, model and catalogue indexes | the old editors, through the links |
| `assets/maps/` | `.utcmap` maps and their previews | the old editors, through the links |
| `art/` | masters (`.blend`, source images) and exported outputs per asset | the old editors, through the links |

`config/` has no editor: it is edited by hand here (lighting, biome looks, player colours; D30), read at start and shipped in packages.

This repository is the only copy of the data (D47). The old repository's `content/`, `assets/` and `art/` are symbolic links to these folders, so its editors and scripts read and write here; review their changes with `git status` in this repository. `content/import/warcraft/` holds the map editor's Warcraft importer settings. Files the editors generate (the map catalogue, original `.map` copies, reconstructed pixels) are ignored. `just cook` turns `assets/` into the runtime pack `target/cooked/canopy.pack` ([D26](decisions.md)); cooked output is never committed.

## Tests

Unit tests inline; integration tests in each crate's `tests/`. Scenario tests are data files beside the simulation crate, with golden replay hashes. Fixtures are test-owned, never shipped content.
