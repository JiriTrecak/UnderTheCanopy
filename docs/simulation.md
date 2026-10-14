# Simulation

Design of `canopy-sim` (milestone M2). Behaviour comes from the old game ([survey](reference/simulation.md)); the structure below is new. Decisions referenced: D3 (40 Hz), D4 (determinism), D5 (Lua), D13–D17.

## Shape

- `canopy-sim` is a library with no I/O, no clocks and no threads. The game runs it on its own thread; tests run it directly.
- `World::new(content, map, setup)` → `world.tick(commands)` → `world.view(owner)` / `world.hash()` / `world.save()`.
- Input is the frozen protocol's `Action` (same JSON shape as the TypeScript `actionSchema`, so `canopy-net` sends it unchanged), stamped with `(tick, player, seq)`.
- Output is a per-owner `View`: everything the renderer and HUD may know, already fog-filtered, plus the events of the tick. The renderer never reads simulation state directly.
- AI players run inside the simulation on every peer, deterministically, from their own `View`.

## Numbers

- **Simulation state holds integers.** Positions are milli-cells (`i32`, 1/1000 of a navigation cell; a 2048-cell map spans 2,048,000). Angles are millidegrees. Time is ticks. Permille for ratios. Heights are centimetres.
- **Content floats convert once at load** into these units (speeds, ranges, radii, regeneration). Authored `*Ticks` are 25 ms units (D3).
- **Floating-point arithmetic is allowed in intermediate maths** (`+ − × ÷ sqrt` are exact IEEE and identical on every platform; Rust never fuses them). **Transcendental functions are not**: `sin`, `cos`, `atan2`, `hypot`, `powf`, `exp`, `ln` come only from `canopy_sim::math`, which uses the pure-Rust `libm`. `crates/sim/clippy.toml` forbids the standard library versions.
- **No unordered containers.** `HashMap`/`HashSet` are forbidden in the crate (clippy); use `BTreeMap`, sorted `Vec`, or id-indexed tables. Every iteration order is defined.
- **Randomness** comes from one seeded generator in the state (xorshift32, as today), used only where gameplay rolls dice (loot, critical/evasion, reaction chance).

## State

- **Entities** live in one table sorted by id. Ids are `u32`, allocated monotonically and never reused, so "in id order" is the natural iteration. An id → slot index table gives O(1) lookup.
- Each entity has the common fields (definition, owner, position, rotation, hp) and optional parts (`unit`, `production`, `resource`, `construction`, `abilities`, `item`, …), mirroring the old model so behaviour ports cleanly. Hot data can move into dense side tables later if profiling says so.
- Definitions are resolved once into compact per-definition records (`DefinitionId` → stats in simulation units), so hot paths never look up strings.
- Other collections (jobs, missiles, shells, corpses, spell instances, facts, wallets, research, objectives, outcome) are plain sorted vectors or `BTreeMap`s.
- **Derived data** (spatial grids, navigation, unit buckets, vision coverage) is never saved. It is rebuilt from state on load and must give identical answers to the incrementally maintained version; tests check this.

## Tick

One function per system, called in a fixed order (from the old game, `Game.tick`):

1. Apply due commands, sorted by (tick, player, seq); rejected commands become error facts.
2. Ability lifecycle → cast orders → ambient effects → taunts.
3. Item effects → regeneration.
4. Order advance → economy assignment.
5. Garrisons → inventory → combat planning → separation → idle motion → movement.
6. Cast orders (second pass) → item effects.
7. Combat resolution (projectiles first), deaths.
8. Ability delivery → taunts.
9. Revival → economy → inventory → upgrades → research.
10. Mission script.
11. Observation (fog and knowledge) → defeat and victory.
12. AI decisions, enqueued for the next tick.

## Map

`World::new` compiles the `.utcmap`: decodes terrain grids, rounds heights to centimetres, derives walkability (ground at least 60 cm above water), bridge decks and scenery blockers, and projects harvestable scenery objects into resource placements (first object per cell wins). Then it creates entities from placements in placement-id order and adds each player's starting setup.

## Lua

Differences from the old Lua 5.3 (fengari) runtime that scripts can notice:
- Arithmetic on strings fails (`"10" + 1`), because there is no string library to coerce them.
- Integers are 64-bit (fengari wrapped at 2^31). Whole numbers reach scripts as integers, so `"t" .. mission.tick()` reads `t4`, not `t4.0`.
- `<const>`/`<close>` locals are valid; a `for` loop with step 0 is an error.
- Loops count up to about a third more instructions against the 20,000 limit.
- Some error messages are worded differently.
- `mission.tick()` starts at 0 (one lower than the old game).

Vendored Lua 5.4 through `mlua`, no standard libraries, a fresh state per run, an instruction limit (20,000) and string limit (64 KiB), scalar arguments only, ≤256 API calls per run, transactional effects (applied only when the run succeeds). `on_start` once, `on_tick` every 4 ticks. The 34-function `mission` API from the old game.

## Saves and hashing

- A save is the serialized state plus pending commands and AI state (new format, D16).
- Desync detection (D13): every 8 ticks a cheap fingerprint of counters, jobs, stocks, outcome and a rotating sample of up to 32 actors.
- Tests run long AI-versus-AI matches and require identical fingerprints across runs, across save/load, and on macOS and Windows in CI.

## Build order

| Step | Contents |
|---|---|
| M2.1 Foundation | crate, maths, ids, state, commands, world and tick skeleton, map compile, entity creation, view skeleton, fingerprint, saves, determinism tests |
| M2.2 Spatial and movement | occupancy, navigation (grid A*, sectors, navmesh), bridges, collision, traffic, formations, idle motion |
| M2.3 Vision | fog, knowledge, views |
| M2.4 Combat | targeting, attacks, projectiles and shells, damage, deaths, experience, heroes and revival, camps and loot, garrisons |
| M2.5 Economy | jobs, harvesting, felling and regrowth, construction, production, research, upgrades, supply, victory |
| M2.6 Items and abilities | inventory, item effects, the ability runtime, statuses |
| M2.7 Missions | Lua and the mission API |
| M2.8 AI | briefing, economy, tactics, abilities |

Each step lands with scenario tests on test-owned content. M2 is done when an AI-versus-AI match on every shipped map runs headless to an outcome with stable fingerprints.
