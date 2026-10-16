> **Reference only.** These pages describe the old TypeScript game in `settlers-web` (`../Settlers 3 Web`). They are the behaviour specification for the Rust rebuild. File paths and implementation notes refer to that repository; links to its code don't resolve here. Data paths (`content/game.json`, `assets/…`, `.utcmap`) are the old layout; today's content is in packages (`base/`, [packages](../packages.md)).

# Documentation

These pages describe the maintained game and tools. Gameplay values come from `content/game.json`; asset revisions come from the published registry; available maps come from `assets/maps/`. Do not duplicate their inventories in hand-maintained milestone reports.

## Work on the game

- [Architecture and repository layout](architecture.md)
- [Gameplay and controls](game/README.md), [economy](game/economy.md), [desktop packaging](game/desktop.md)
- [Declarative gameplay](declarations/README.md), [behaviors](declarations/behaviors.md), [examples](declarations/examples.md), [systems](declarations/systems.md)
- [Validation](declarations/validation.md), [opponent AI](ai/implementation.md)
- [Simulation worker](expansion/simulation-worker.md), [spatial sectors](expansion/spatial-sectors.md)

## Author worlds and assets

- [Map editor and procedural layers](editor.md)
- [Art direction](game/art.md), [asset publication](asset-pipeline/publication.md), [Asset Studio](asset-pipeline/studio.md)
- [World and campaign design](wiki/factions/story.md): working narrative cornerstones, faction arcs and open story decisions.
- [Weather](expansion/weather.md), [atmosphere](expansion/volumetric-atmosphere.md), [graphics settings](expansion/graphics-settings.md)
- [Spell effects](expansion/spell-effects.md), [unit cameras](expansion/unit-camera-modes.md), [team colors](declarations/team-color.md)

## Implementation contracts

- [Competitive gameplay foundation](expansion/competitive-foundation.md): agreed spatial, economy, hero, editor and map migration targets.
- [Declarative abilities](design/abilities.md): ability content contracts, publication and deterministic runtime.

## Proposed changes

- [Repository structure](design/repository-structure.md): proposed one-pass restructure into per-app folders (game, each editor as its own app, server), Rust crates and LFS-backed art.
- [Native renderer](design/native-renderer.md): proposed Rust + wgpu renderer replacing Three.js, based on an audit of the current renderer. The current render code remains the implementation until the migration lands.

## Documentation policy

Keep one current contract per system. Put implementation details beside the relevant source module when useful. Update the contract when behavior changes; remove superseded plans, copied audits, old map walkthroughs and temporary handoff notes. Git history provides change history.

Use repository-relative links. Store test captures, comparison screenshots and benchmark output in ignored `tmp/`. Keep editable Blender files, original texture/icon masters, generation provenance and approved style inputs in `art/`; they are inputs, not disposable evidence. Do not commit generated wiki/build output. Record a fresh measurement with its conditions when needed instead of presenting old test counts or FPS as current guarantees.

Run `npm run docs:check` for repository link validation and `npm run wiki:build` for generated-route validation.

The [wiki maintenance guide](wiki/development/index.md) explains generated player documentation. The wiki publishes current technical pages and links to the content-derived map and definition catalogs.
