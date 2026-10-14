# Repository structure

```
UnderTheCanopy/
├─ Cargo.toml          Rust workspace
├─ justfile            every command: just play | profile | cook | check | test | package | sync-data
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
│  └─ net/             client for the existing match server protocol (M5)
├─ game/               the executable: window, input, platform paths, screens, packaging
├─ content/            gameplay data (mirrored, see below)
├─ assets/             published asset outputs, registry, manifest and maps (mirrored)
├─ art/                editable masters and exported outputs, Git LFS (mirrored)
├─ docs/               plan, decisions, design; reference/ describes the old game
└─ tools/              repository scripts
```

Phase 2 adds `editors/{map,assets,spells,shared}` (one Tauri app each) and possibly `server/`.

## Dependency rules

- Crates never depend on `game/`.
- `canopy-sim` depends only on `canopy-content` (and deterministic math and Lua). No I/O, no clocks, no threads it doesn't own deterministically.
- `canopy-render` never sees simulation types; `canopy-scene` is the only bridge.
- Content is always passed in. No code names a shipped content id (`unit.ants.warrior`, a race, a map); tests use their own fixtures.

## Data

| Folder | What | Source of truth during phase 1 |
|---|---|---|
| `content/` | `game.json` (definitions, rules, behaviour sets, actions), ability and effect folders, neutral camps | old repository, mirrored |
| `assets/library/` | published asset outputs (glb, png, wav, recipe and water definitions) | old repository, mirrored |
| `assets/manifest.json`, `assets/authoring/` | runtime manifest, publication registry, model and catalogue indexes | old repository, mirrored |
| `assets/maps/` | `.utcmap` maps and their previews | old repository, mirrored |
| `art/` | masters (`.blend`, source images) and exported outputs per asset | old repository, mirrored |

`just sync-data` copies exactly the files the old repository tracks (minus the Warcraft importer settings) and removes ones it no longer tracks; review the result with `git status`. `just cook` turns `assets/` into the runtime pack `target/cooked/canopy.pack` ([D26](decisions.md)); cooked output is never committed.

## Tests

Unit tests inline; integration tests in each crate's `tests/`. Scenario tests are data files beside the simulation crate, with golden replay hashes. Fixtures are test-owned, never shipped content.
