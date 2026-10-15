# Plan

Agreed with Jiri, 2026-10-14.

## Phase 1: the game, rebuilt in Rust

Rebuild the existing game natively. **Gameplay behaves the same** (rules, numbers, AI, controls, the multiplayer protocol); **nothing is translated 1:1**. Every system is redesigned for the best performance we can reach, rendering above all. The old TypeScript game is the behaviour specification and the visual reference, not a source of code.

Phase 1 runs on Apple Silicon Macs with `just play` and is packaged by CI, but not distributed. The code stays portable: CI builds, tests and packages Windows on every commit.

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

## Data during phase 1

The editors stay in the old repository, so it remains the place where data is authored. This repository mirrors it with `just sync-data`, and **the formats stay identical**: no conversion, no reorganisation. New fields or a format redesign wait for phase 2, when the editors move here.

## The old repository

`settlers-web` (`../Settlers 3 Web`) stays runnable and is the reference for behaviour and looks. It keeps hosting the editors and the match server. Nothing is copied from its code; read or run it to answer "what does the game do here".

## Phase 2: deferred

| Deferred | Why it can wait |
|---|---|
| Map, asset and spell editors (and the in-game map editor entry) | They work in the old repository; their output copies over unchanged. Ported one at a time once the game is near 100%, onto the Rust core (Tauri host, WebGPU viewport). |
| Warcraft map importer | Part of the map editor. |
| Match server in Rust | It works and stays deployed; the protocol is frozen. |
| Content format redesign (one file per definition, generated TypeScript types) | Happens with the editors. |
| Steam integration | Purely additive. |
