# Repository structure

```
UnderTheCanopy/
├─ Cargo.toml          Rust workspace
├─ justfile            every command: just play | profile | cook | check | test | package | server
├─ AGENTS.md           rules for anyone changing this repository
├─ crates/             libraries, named canopy-*; added when a milestone needs them
│  ├─ content/         packages, data model, loading, validation (M1, D53)
│  ├─ sim/             deterministic simulation; depends only on content (M2)
│  ├─ assets/          runtime asset format and loader (M3)
│  ├─ cook/            asset folders → the runtime pack, library and CLI (M3)
│  ├─ render/          wgpu renderer; knows nothing about the simulation (M3)
│  ├─ scene/           simulation snapshots → render world, interpolation (M3)
│  ├─ ui/              HUD toolkit: taffy layout, cosmic-text (M4)
│  ├─ audio/           playback, voice limiting, effect sounds, music (M4)
│  ├─ editor/          the editor the game opens as a mode: egui shell, zinc theme (docs/editor.md)
│  ├─ net/             client for the existing match server protocol (M5)
│  ├─ release/         release log, update feed, package signatures (D43)
│  └─ release-tool/    `just release-*`: notes, prepare, build, publish (docs/releases.md)
├─ game/               the executable: window, input, platform paths, screens, updater, packaging
├─ base/               the base package: all game content, its art and its maps (see below)
├─ art/                image-generation styles and profiles (tool inputs)
├─ config/             engine settings owned here: lighting, water, glow, player colours, UI (D30)
├─ releases/           the release log, gallery media and the release host's identifiers (D43)
├─ server/             the match server, TypeScript on node: lobby and lockstep relay (docs/server.md)
├─ docs/               plan, decisions, design; reference/ describes the old game
├─ .claude/skills/     agent skills describing the content formats (canopy-*)
└─ tools/              repository scripts
```

Phase 2 adds one native editor (docs/plan.md, phase 2).

## Dependency rules

- Crates never depend on `game/`.
- `canopy-sim` depends only on `canopy-content` (and deterministic math and Lua). No I/O, no clocks, no threads it doesn't own deterministically.
- `canopy-render` never sees simulation types; `canopy-scene` is the only bridge.
- Content is always passed in. No code names a shipped content id (`unit.ants.warrior`, a race, a map); tests use their own fixtures.

## Data

All content is in packages (docs/packages.md, D53); the agent skills `canopy-packages`, `canopy-definitions`, `canopy-abilities`, `canopy-assets` and `canopy-maps` describe the formats.

| Folder | What |
|---|---|
| `base/` | the base package: `package.json`; rules, races, behaviours, research, loot pools; one file per unit, building, item, resource, ability and effect; `scenery/`; `landscape/` (biomes, water profiles); `assets/` (one folder per asset with `asset.json`, runtime files and working sources); `maps/<group>/<id>.utcm` and `campaigns/<id>.utcc` (nested packages) |
| `config/` | engine settings edited by hand (lighting, water rendering, glow, player colours, UI; D30), read at start and shipped |
| `art/` | inputs of the image-generation tooling (styles, profiles) |

Art files (glb, images, `.blend` sources) are in Git LFS; map grids (`.grid`) and every document stay in plain git. `just cook` turns the asset folders and map previews into the runtime pack `target/cooked/canopy.pack` ([D26](decisions.md)) and stages the base package's documents in `target/cooked/base` for packaging; cooked output is never committed. Players' maps live in the user folder (`maps/`, `downloads/`).

## Tests

Unit tests inline; integration tests in each crate's `tests/`. Scenario tests are data files beside the simulation crate, with golden replay hashes. Fixtures are test-owned, never shipped content.
