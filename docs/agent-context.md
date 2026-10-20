# Agent context and code index

Refreshed 2026-10-19 against `96448f1` (`Release 0.4.0: The beetles`), on `main`.
This is a shared navigation aid for Codex and Claude, not a replacement for the
design documents or an assertion that pending reviews have been approved.
Recheck HEAD and the working tree before using this snapshot for a new task.

## Start here

1. Read [the plan](plan.md) first, as [AGENTS.md](../AGENTS.md) requires.
2. Read the relevant recent entries in [decisions](decisions.md), including
   amendments within older entries. Later decisions can supersede phase 1 parity.
3. Use [structure](structure.md), this index, and the subsystem document to find
   the implementation. The Rust types define content formats.
4. Inspect `git status --short` and the affected files before editing: Codex and
   Claude share this project. Preserve other work and shared release notes.

[Status](status.md) is primarily the October 14–15 rebuild report. Its descriptions
of remaining work, tree harvesting, old editors and phase 2 are historical;
consult the plan, later decisions, current code and [release log](../releases/log.json).
[Reference](reference/README.md) describes the old TypeScript game, whose code
paths refer to `../Settlers 3 Web`. It remains a behaviour and visual reference;
its code is not a template to translate.

## Current baseline

- The game and editor are native Rust. The workspace is **0.4.0**, Rust **1.97.1**,
  edition 2024. The match server remains TypeScript/Node in `server/` (D48);
  its protocol is frozen (D10). Python and shell scripts remain authoring tools.
- Phase 1 M0–M5 is recorded as done. Phase 2 package data, editor shell, data,
  map and asset editors are built. Some implementations still await Jiri's review.
- The editor is a game mode (`just edit`), currently for a repository checkout's
  base package. It has shared undo, schema forms, references, disk watching,
  unsaved Test in game, an ability encounter stage, map brushes, Warcraft map
  import, asset import/cooking, and MCP. See [editor](editor.md).
- Content lives in `base/`, not the former `content/game.json` or top-level asset
  registries. Maps are `.utcm` package directories; campaigns use `.utcc`.
  Objects merge across package layers, lists replace, null removes; inheritance
  and removals are supported. See [packages](packages.md), D53.
- **Amber and Substance** are the shipped economy (D62). Amber is finite;
  Substance deposits are endless. Wells occupy deposits and now pump without
  workers. The visible name is Substance; some internal ids still say `root`.
  Trees are no longer harvested by the base game; generic engine felling remains.
- Beetles are the second complete playable race beside ants. Their production
  uses a Substance power network (D66), extended by connected nodes. See
  [beetles](beetles.md), D65–D70, and the actual definitions for current numbers.
- The art target is stylized, crisp armies on a quiet realistic forest floor at
  insect scale (D61). Detail belongs in the non-playable frame; armies must read
  first. Forest magic is blue-violet; fire is orange. Buildings have clean bases,
  without modelled dirt skirts. See [art direction](art-direction.md) and D65.
- Autumn Echo Isles and Look-dev: Fall demonstrate the newer environment work.
  `terrain/blocked.grid` makes closed ground impassable and unbuildable (D71).
- Replays and in-game feedback are built (D59–D60). A feature being built does
  not establish that a remote service has been deployed or configured.

## Runtime and code ownership

- **Entry and platform:** [main.rs](../game/src/main.rs) dispatches play, edit,
  headless MCP, captures and replay verification;
  [app.rs](../game/src/app.rs) owns the window/event loop;
  [options.rs](../game/src/options.rs) defines CLI arguments;
  [paths.rs](../game/src/paths.rs) owns file locations.
- **Game flow:** [screens](../game/src/screens/) owns menus, loading, multiplayer,
  in-match UI, feedback and replay screens. [play](../game/src/play/) owns camera
  interaction, picking, selection, orders, keys and HUD bindings. Read
  [play.md](play.md) and [ui.md](ui.md).
- **Simulation host:** [game/src/sim.rs](../game/src/sim.rs) drives 25 ms ticks on
  its thread, through `canopy_net::Session`, and publishes scene snapshots for
  interpolation. Local and network play share the session abstraction.
- **Simulation:** [World](../crates/sim/src/world.rs) coordinates the fixed tick
  order; [state](../crates/sim/src/state.rs),
  [view](../crates/sim/src/view.rs) and
  [fingerprint](../crates/sim/src/fingerprint.rs) cover saved state, player
  knowledge and determinism. Read [simulation.md](simulation.md).
  Movement/pathing: `movement/`, `navigation/`, `spatial/`, `terrain/`.
  Combat/spells: `combat/`, `abilities/`, `garrisons.rs`, `heroes.rs`, `items/`.
  Economy: `economy/`, especially `income.rs`, `network.rs`, `work.rs`,
  `harvest.rs`, `sites.rs`, `production.rs`. AI: `ai/`, especially `economy.rs`,
  `placement.rs`, `geography.rs`, `operations.rs`, `tactics.rs`, `powers.rs`.
  Missions: `mission/` (Lua). These paths are relative to `crates/sim/src/`.
- **Presentation bridge:** [canopy-scene](../crates/scene/src/lib.rs) converts
  simulation views to renderer inputs: `live.rs`, `interpolate.rs`, `models.rs`,
  `animation.rs`, `state_effects.rs`, `effects/`, `damage.rs`, `terrain.rs`.
  It is the bridge; the renderer never depends on simulation types.
- **Rendering:** [canopy-render](../crates/render/src/lib.rs), especially
  `renderer.rs`, `graph.rs`, `actors.rs`, `statics.rs`, `terrain/`, `water/`,
  `effects.rs`, `shadow.rs`, `fog.rs`, `post.rs`, `surroundings.rs`, `motes.rs`,
  `quality.rs`. Read [renderer.md](renderer.md) together with D61–D64 and D69;
  early renderer design sections predate the new direction.
- **Content:** [canopy-content](../crates/content/src/lib.rs), especially
  `package/`, `load.rs`, `registry.rs`, `game.rs`, `abilities/`, `effects/`,
  `assets/`, `map/`, `schema.rs`. Schemas come from Rust types.
- **Art pipeline:** [canopy-cook](../crates/cook/src/lib.rs) cooks authoring files;
  [canopy-assets](../crates/assets/src/lib.rs) defines/reads the runtime pack.
  `base/assets/` contains asset folders and working sources; `art/` contains
  style/reference inputs. `tools/buildings/`, `tools/units/`, `tools/props/`,
  `tools/icons/`, `tools/terrain/`, `tools/content/beetles/` are authoring tools.
- **UI/audio:** [canopy-ui](../crates/ui/src/lib.rs) is the game's own HUD toolkit
  (taffy and cosmic-text), driven by `config/ui/`; it is distinct from the
  editor/debug egui UI. [canopy-audio](../crates/audio/src/lib.rs) handles playback;
  see [audio.md](audio.md).
- **Editor:** [canopy-editor](../crates/editor/src/lib.rs): `document.rs` is the
  command/undo core; `checker.rs`, `watch.rs`, `forms.rs`, `references.rs`,
  `map/`, `warcraft/`, `assets.rs`, `stage.rs`, `encounter.rs` implement the tools.
  [mcp.rs](../crates/editor/src/mcp.rs) exposes the same editing commands.
  `just mcp` serves headless; [tools/maps/mcp.py](../tools/maps/mcp.py) is an
  existing client. Use the documented loopback/token access, never print tokens.
- **Multiplayer:** [canopy-net](../crates/net/src/lib.rs): `wire.rs`, `session.rs`,
  `lockstep.rs`, `link.rs`, `net.rs`, `server.rs`; [server.md](server.md) describes
  the separate Node match host.
- **Replays/releases:** [game/src/replay](../game/src/replay/) owns recording,
  format, library, playback, tally and verification;
  [game/src/update](../game/src/update/) handles updates.
  `crates/release/` defines logs/feeds/signatures, `crates/release-tool/` performs
  release operations. Read [releases.md](releases.md) before release/updater work.

## Shared skills and working rules

The repository's skills live under **`.claude/skills/`**. They describe this Rust
project and apply to Codex too; a globally installed old-editor skill is not
evidence of the current format or workflow. Read the matching repository skill
before content work:

- [canopy-packages](../.claude/skills/canopy-packages/SKILL.md): package structure,
  layers and identity; prerequisite to the other content skills.
- [canopy-definitions](../.claude/skills/canopy-definitions/SKILL.md): units,
  buildings, races, items, research, rules and networks.
- [canopy-abilities](../.claude/skills/canopy-abilities/SKILL.md): ability programs,
  bindings, effects and checked examples.
- [canopy-assets](../.claude/skills/canopy-assets/SKILL.md): assets, scenery,
  landscape and runtime publication.
- [canopy-maps](../.claude/skills/canopy-maps/SKILL.md): map packages and validation.
- [canopy-buildings](../.claude/skills/canopy-buildings/SKILL.md): concepts, Tripo
  base models, Blender finishing/animation, team colour, states and effects.
- [build-game](../.claude/skills/build-game/SKILL.md),
  [run-game](../.claude/skills/run-game/SKILL.md),
  [release-new-version](../.claude/skills/release-new-version/SKILL.md): the
  requested build, launch and release workflows. Reading this index authorizes
  none of those actions by itself.

Jiri decides visible/felt changes and quality trade-offs; propose options with a
recommendation and side-by-side captures for visuals. New phase 2 milestones
need settled design before code. Preserve the distinction between agreed choices
and entries labelled **review**. Technical decisions go in `decisions.md`.

Project-specific Claude memory reviewed on 2026-10-19 also records that Jiri
prefers proposals explained in prose and questions in plain text, and wants the
look established before optimisation during look-dev. Treat these as remembered
preferences, subordinate to current instructions; continue measuring costs and
leave shipped quality trade-offs to Jiri. Claude's private memory was not edited.

## Validation and commands

- `just check`: tidy, format check, clippy with warnings denied, workspace tests;
  also server check/tests when `server/node_modules` exists. Required before
  every commit. No CI currently (D50).
- `just cook`: canonical `target/cooked/canopy.pack` and staged base documents.
  `just play`, `just edit`, `just shot`, `just replay` use cooked data.
- `just bench-sim`, `just bench-render`, `just profile`: measurement entry points.
  Base M1 is the target, not a measurement inferred from the M4 Max reports.
- `crates/sim/tests/`: scenario and determinism coverage, including save/rewind,
  movement, combat, vision, economy, income, networks and AI. Fixtures are
  test-owned. `crates/sim/examples/ai_duels.rs` exercises real matchups.
- `crates/content/tests/`: generic shipped-content/map/asset checks and skill
  examples; renderer/UI/net integration tests live in their respective crates.
- Keep deterministic math, seeded randomness and fixed order in simulation.
  No shipped content ids in engine logic. Crates never depend on `game/`.
  Renderer capabilities stay within WebGPU baseline without an agreed exception.
- Player-visible work gets a `just release-note` entry in the single shared
  `releases/log.json`; internal documentation does not. Release preparation or
  publication requires release intent. Never expose signing keys.
- Keep only needed scratch files. `just tidy` removes disposable build caches;
  it runs at the start of `just check`, which matters when builds share a checkout.

## Open work to recheck before starting

The plan defers player content distribution/user scripts, the campaign editor,
editor assistant, Steam, Windows validation, a Rust server and retiring the old
repository. Map landform/symmetry tools remain deferred. Recent decision entries
retain visible/felt reviews; balance, map asymmetry and late-tier ability usage
remain open in D68–D71. Check current code and newer decisions before treating
any historical TODO as an outstanding task.

This refresh inspected source, documentation, skills metadata and project memory.
It did not build, run, benchmark, test, publish or verify remote deployments.
