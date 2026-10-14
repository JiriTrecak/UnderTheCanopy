# Architecture and repository layout

The deterministic simulation owns gameplay. Presentation reads observation snapshots; rendering and UI do not mutate authoritative state. The simulation runs in a worker, while the main thread handles input, camera, HUD and Three.js submission. See [worker transport](expansion/simulation-worker.md) for scheduling and snapshot details.

## Source boundaries

- `content/`, `src/content/`: gameplay definitions, schemas, validation and registry.
- `src/sim/`: world state, economy, combat, AI, observation, pathfinding and deterministic commands. No DOM or renderer dependencies.
- `src/session/`, `src/net/`, `server/`: match lifecycle, local/network channels and authoritative turn delivery. Module notes in `src/net/` describe the protocol.
- `src/presentation/`, `src/ui/`: observation-derived command availability, selection, HUD and settings.
- `src/render/`: terrain, models, materials, fog, lighting and visual effects.
- `src/shared/authoring/`, `src/editor/`: reusable asset/layer schemas, procedural compilation and map authoring.
- `tooling/asset-studio/`, `tooling/src/`: asset publication service and separate tools UI.
- `mcp/`: editor/authoring commands for agents.

Use strict TypeScript, typed command unions and explicit lifecycle ownership. Keep deterministic RNG in the simulation; visual randomness must not alter gameplay. Content fingerprints intentionally reject incompatible saves and network peers.

## Content storage

- `assets/maps/`: authored map source and generated map catalog.
- `art/assets/`: canonical working asset definitions and role-named files.
- `art/assets/<id>/`: canonical role-named geometry, editable Blender sources, texture masters, recipes and build manifests. `.asset-work/build/` is disposable tool staging.
- `art/styles/`, `art/profiles/`: approved style inputs and validation/export profiles.
- `assets/authoring/published.json`: released asset definitions.
- `assets/library/`: published runtime resources, with `assets/manifest.json` and generated URL bindings.

The runtime does not scan arbitrary art folders. Use the [publication workflow](asset-pipeline/publication.md) to change assets.

## Local and generated files

`tmp/`, `.asset-work/`, build output and wiki output are ignored. `.asset-work/` can contain credentials and publication journals: it is not a general cleanup target. The ignored `original/` directory is a local legacy install, not part of the shipped game. Removing tracked files does not erase Git history; repository history is not rewritten during normal cleanup.
