# Simulation

Design of `canopy-sim` (milestone M2). Behaviour comes from the old game ([survey](reference/simulation.md)); the structure below is new. Decisions referenced: D3 (40 Hz), D4 (determinism), D5 (Lua), D13–D17.

## Shape

- `canopy-sim` is a library with no I/O, no clocks and no threads. The game runs it on its own thread; tests run it directly.
- `World::new(content, map, setup)` → `world.tick(commands)` → `world.view(owner)` / `world.hash()` / `world.save()`.
- Input is the frozen protocol's `Action` (same JSON shape as the TypeScript `actionSchema`, so `canopy-net` sends it unchanged), stamped with `(tick, player, seq)`.
- Output is a per-owner `View`: everything the renderer and HUD may know, already fog-filtered, plus the events of the tick. The renderer never reads simulation state directly.
- AI players run inside the simulation on every peer, deterministically, from their own player's knowledge (the entities its view lists, read through `Sight` without building a `View`); their commands are ordinary `Action`s queued for the next tick (`ai`, D27).

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
7. Combat resolution (sight sensors brought up to date after movement; shells, then missiles, then swings), deaths.
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

- A save is the serialized state plus pending commands; AI memories are part of the state (`State::ai`), their derived navigation is rebuilt on load (new format, D16).
- Desync detection (D13): every 8 ticks a cheap fingerprint of counters, jobs, stocks, outcome and a rotating sample of up to 32 actors.
- Tests run long AI-versus-AI matches and require identical fingerprints across runs, across save/load, and on macOS and Windows in CI.

## Performance

Budget: a tick must finish well inside 25 ms (D3) on the base M1 while the render thread keeps 60 fps; the worst case is eight players with about 50 units each fighting at once (plan).

**Benchmark.** `just bench-sim` runs the `match_bench` example of `canopy-sim`: the measured crowd battle (`canopy_sim::crowd`, the same scenario as `canopy shot --crowd 50`: eight players with 50 soldiers each in a ring on echo-isles, every soldier type of the default race, armies meeting from tick ~120 to a melee that leaves ~350 alive at tick 1600). It prints the whole tick (step, and the observer view) as mean, p50, p99 and max, the worst tick with its steps, every `tick: …` step span of `World::step` and the costliest inner spans, and a digest of every tick's fingerprint plus the final save: equal digests mean identical simulations (D36). Other scenarios: `just bench-sim --map turtle-rock --crowd 50`, `--ai 4 --ticks 12000` (computer players), `--no-spans` (no span subscriber overhead). The observer view and the live presenter are timed by `canopy shot --tick N` (`step + view + present`).

**Measured** (M4 Max, release, `just bench-sim`, ms per tick, best of three runs while other builds were running):

| | mean | p50 | p99 | max |
|---|---|---|---|---|
| before (2026-10-15, `35af425`) | 7.31 | 7.05 | 17.1 | 27.3 |
| after | 2.13 | 2.11 | 3.4 | 5.1 |

| step (mean / max) | before | after |
|---|---|---|
| abilities (lifecycle, auras, autocast) | 5.17 / 25.7 | 1.18 / 3.9 |
| — autocast | 3.97 / 24.3 | 0.37 / 2.9 |
| — auras | 1.05 / 7.6 | 0.71 / 1.1 |
| combat plan (attack approach) | 0.85 / 3.4 | 0.43 / 1.1 |
| combat resolve | 0.83 / 2.6 | 0.11 / 0.3 |
| ability resolve | 0.31 / 14.7 | 0.09 / 1.8 |
| movement, separation, idle | 0.14 / 4.0 | 0.12 / 0.5 |
| everything else | < 0.3 | < 0.3 |

turtle-rock with the same crowd: 7.69 → 1.92 mean, 32.6 → 5.2 max. AI matches are cheap (after: four AIs on turtle-rock, 0.37 ms mean and 1.9 ms max over 12,000 ticks). Outside the simulation, `canopy shot --crowd 50 --tick 801`: observer view 0.16 ms, presenter 1.13 → 0.18 ms per tick.

**What it was.**
- *Exact sight* (`Sight::visible`) walked every sensor in the target's sectors, looking each up twice by binary search before checking whether it even shared its sight with the asking team. In an eight-player crowd seven of eight sensors were someone else's. The sector index now stores each sensor's team mask and sector range, so other teams' sensors are skipped without a lookup (autocast, combat perception, projectile witnesses and events all ask this).
- *Autocast* asked that sight question for every unit in reach of every caster due this tick, and snapshotted (with full stats) each unit again per caster, and the caster again for every aim it checked. Sight answers and snapshots are now remembered per pass and forgotten after each cast (the only thing in the pass that changes the state).
- *Attack approach* sorted every candidate cell in range (~260) and, in a crowd, almost always tried all of them (99.6% end on a detour), each with a swept-body test. It now pops a heap until eight detours exist, proves most remaining directions blocked by the bodies touching the actor, and sorts only the candidates that pass.
- *Unit snapshots* (`actor::of`, `Stats::of`) computed control immunities up to five times, `actor::plain` built full stats for a level, and status checks cloned the whole source entity to swap its owner.
- *The presenter* deep-copied each model's metadata (clips, states, names) for every unit every tick; it now shares it.

**Where the time goes now** (crowd, after): auras 0.7 ms (each aura holder re-applies its one-tick statuses to every unit in radius through the ability program, with unit snapshots, and scans all ~2,500 entities, mostly trees, for candidates), autocast 0.4 ms, attack approach 0.3 ms, everything else under 0.15 ms each. The worst tick is the first autocast wave (ticks 1–8, ~3–4 ms autocast: every cast forgets what the pass learned).

**Base M1** (not measured; Firestorm cores run single-threaded code about 1.7× slower than the M4 Max): about 3.6 ms mean, 6 ms p99 and 9 ms at the opening autocast wave for the simulation, plus about 0.6 ms for view and presentation, well inside 25 ms. Dev builds (opt-level 2, no LTO) are slower still. Next candidates if needed: a candidate index for auras (entities with health by position), finer forgetting in autocast, and a lazily built corpse list.

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
