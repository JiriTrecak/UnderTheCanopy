# Plan

Agreed with Jiri, 2026-10-14.

## Phase 1: the game, rebuilt in Rust

Rebuild the existing game natively. **Gameplay behaves the same** (rules, numbers, AI, controls, the multiplayer protocol); **nothing is translated 1:1**. Every system is redesigned for the best performance we can reach, rendering above all. The old TypeScript game is the behaviour specification and the visual reference, not a source of code.

Phase 1 runs on Apple Silicon Macs with `just play` and `just package`. The code stays portable, but nothing builds or tests Windows for now (no CI, D50).

### Targets

- Reliable 60 FPS. Worst case: 8 players with about 50 units each (~400 units). Fights are realistically smaller; an all-in brawl of every army is best effort.
- Reference machine: base M1 (7-core GPU). No Intel Macs.
- Build the fastest thing we can, then set budgets from measurements.

### Milestones

Status: M0, M1 and M2 done (2026-10-14); M3 in progress.

**M0. Repository.** Cargo workspace, `just`, CI on macOS and Windows (format, lint, test, package), a window on Metal and DX12, the platform layer (data and user folders, DPI, present modes), Tracy profiling. Data mirrored from the old repository; docs, plan and decision log.

**M1. Content.** `canopy-content` reads and validates all of today's data in its current formats (`content/game.json`, ability and effect folders, `assets/` publication registry and manifest, `.utcmap`). Produces a parity report: every definition kind, ability operation and effect primitive the data uses.

**M2. Simulation, headless.** `canopy-sim`: movement and pathing, combat, abilities and effects, items, heroes and experience, settlement and economy, construction, neutral camps, vision and fog, AI, victory, save state, and Lua scenario scripting. Deterministic from day one. Scenario tests from the old game's behaviour checklist with test-owned fixtures; replay-hash tests; a full AI-versus-AI match headless.

**M3. Watching a match.** `canopy-cook`, `canopy-assets`, `canopy-render`, `canopy-scene`: terrain, water (ripples carried over), animated units, buildings, scenery, effects, fog of war, selection and health visuals, following [the renderer design](renderer.md). Spectate an AI match. The renderer's open questions are decided with Jiri at the start of M3.

**M4. Playable game.** Camera, input, selection, orders, order queue, control groups, command feedback; HUD (command card, hero bar, tooltips, minimap); audio; main menu, skirmish setup, race and hero select, map picker, campaign screen and mission HUD, loading screen, game menu, match result; save and load; settings (graphics, audio, HUD, key bindings); egui debug overlay.

**M5. Multiplayer.** `canopy-net`: a client for the existing match server and its frozen protocol (lobby, rooms, chat, lockstep, latency, disconnects, observers), tested against recorded sessions.

Phase 1 is done when the parity checklist against the old game is complete.

### Working together

- Jiri decides everything that is seen or felt: lighting, water, fog, grading, camera and input feel, and every performance-versus-quality trade-off. Visual choices come with side-by-side captures and a recommendation.
- Technical decisions that change neither look nor behaviour are made by the implementer and recorded in [decisions](decisions.md).
- Each milestone starts with its open questions.

## Data

All content is in packages ([packages](packages.md), D53), the base game in `base/`. The old TypeScript editors no longer read it; until the native editor (phase 2) exists, content is edited as files, by hand or by agents with the `canopy-*` skills.

## The old repository

`settlers-web` (`../Settlers 3 Web`) stays runnable and is the reference for behaviour and looks. It no longer holds data, its editors no longer read this repository's content (D53), and the match server moved to `server/` (D48). Nothing is copied from its code; read or run it to answer "what does the game do here".

## Phase 2: one editor, and players' content

Agreed with Jiri, 2026-10-16. One native Rust editor shipped inside the game, used by us to author the game itself (directly and by agents through MCP) and given to players for their own maps and mods. The base game becomes the bottom layer of the same package format players use (as in StarCraft II and Factorio).

**Working rule: every milestone starts with a deep design discussion with Jiri, and no code is written for it until that discussion is settled.** The milestones below are candidates, not commitments; their scope changes as each is discussed. Old editors are not ported: each editor is designed again from the ground up. The spell editor is the exception, and its design carries over largely as it is.

| Candidate | Scope to discuss |
|---|---|
| **M0. Data layer** (done, D53) | Packages and layering (base, campaign, map; mods later), one file per definition, overrides and inheritance, schemas from the Rust types, the asset registry, the map format, user folders, content identity, migration of today's data. |
| **M1. Editor shell** (done, docs/editor.md) | One editor inside the game for the repository's base package: the rail, tabs, forms from schemas, one undo history, the loader's check, files followed on disk, MCP (and `canopy mcp`), the viewport on the game renderer, Test in game. |
| **M2. Data editors** (done, docs/editor.md) | Overviews for units and heroes, buildings, items, abilities and effects, races; links between them with Back and Forward; abilities tried in a test match (the encounter). |
| **M3. Map editor** (done, docs/editor.md) | Warcraft III-style brushes (raise, lower, flatten, smooth, cliff, ramp, paint, water, trees, props), amber sites, camps and starts by click, new maps, the check of playability and army reachability, Test in game with unsaved edits, MCP map tools; Amberleaf Crossing authored with them. Landform objects, symmetry tools and new ground cover wait (decided 2026-10-16). |
| **M4. Asset editor** (done, review; docs/editor.md) | An asset's overview (files, animations played in the preview, users), import of models and images (MCP too), cooking from the editor, unused assets and deleting an asset with its folder. |
| M5. Players' content | Custom maps in the game, packaged maps, map transfer in multiplayer, the Lua sandbox audit. |
| M6. Retire the old repository | |

AI stays code, not data: it should read the game data and react to what the opponent does rather than follow fixed unit lists, and custom races later bring their own AI as an included Lua script. Deferred beyond these: a campaign editor, the Warcraft map importer, the match server in Rust (it works as TypeScript in `server/`, D48), Steam integration.
