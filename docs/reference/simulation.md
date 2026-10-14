# The old simulation: survey for M2

Read-only survey of the TypeScript simulation (`settlers-web`, 2026-10-14). Paths are relative to its `src/`. It describes what exists; the Rust design is in [the plan](../plan.md) and [decisions](../decisions.md).

**Read with D4 in mind:** the Rust simulation must be deterministic with itself on every platform, not bit-identical to the TypeScript one. Where this survey notes JS-specific numeric behaviour (`Math.round` half toward +∞, V8 `hypot`, object key order), it matters only for understanding behaviour, not for reproduction.

## Architecture

- `World` (`sim/world/world.ts`, 329): clock (25 ms), slots, Mulberry32 rng (saved and hashed, never drawn), pending command queue, applied log, one `PlayerAI` per AI slot, the `Game`.
- `Game` (`sim/game/game.ts`, 1018): orchestrator over `GameState`; creates `GameContext`, `Economy`, `BuildingUpgrades`, `Research`, `UnitOrders`, `Garrisons`, `CampLoot`, `Revival`, `Observation`, `Combat`, `Inventory`, `AbilityRuntime`, optional `Mission`.
- `GameContext` (`sim/game/context.ts`, 727): entity indexes, create/remove, movement integrator, `Spatial`.
- `Spatial` (`sim/game/spatial.ts`, 1019): terrain grids, static occupancy, pathfinding, collision sweeps.
- Session: `session/worker/runtime.ts` runs the sim in a Web Worker from lockstep commits, sends a hash every 8 ticks in multiplayer; `snapshots.ts` delta-encodes frames.

## Systems

| System | Main files (lines) | Notes |
|---|---|---|
| Commands | `game.ts:253-535`, `world.ts:117-133,297-328`, `shared/types/types.ts` | 28 action types: abilityAutocast, castAbility{unit\|point}, garrison, unload, research/cancelResearch, upgrade/cancelUpgrade, revive/cancelRevival, learnAbility, gather, construct, pickup, dropItem, useItem, move{actors≤160, destination, attackMove?, append?}, attack{force?}, stop, hold, patrol, follow, build{definition, position, rotation%90}, produce, cancel{queue?}, rally{destination\|null, target?}, pause, noop, ping. Validated in `World.enqueue` and `Game.command`; rejections become error facts |
| Orders | `unitOrders.ts`, `state.ts:20-39` | `unit.order`, `orderQueue` ≤16, replayed through `Game.command` |
| Movement | `context.ts:381-727`, `motion.ts`, `facing.ts` | Positions in integer 1/1000 cells. Budget/tick = `floor(speed*1000*moveSpeedPermille*chargePermille/4e7)`. Must face waypoint (turnRate, ±1°) before moving. Per step: terrain sweep, cell-reservation ray, swept-circle body test |
| Collision / traffic | `spatial.ts:53-58,927-994`, `separation.ts`, `trafficRequests.ts`, `trafficEscape.ts`, `localPath.ts`, `unitIndex.ts` | Swept circle-vs-circle allowing separating motion; bounded local detour; deadlock handling (Tarjan SCC, priority propagation, 120-tick yield after 200 stall ticks); idle-ally yield after 40 stalled ticks; separation every 20 ticks staggered by id; harvesting workers ghost through units |
| Pathfinding | `spatial.ts:235-400,758-836`, `navigation.ts`, `sectorNavigation.ts`, `groundNavigation.ts`, `shared/navigation/*`, `shared/map/walkSurfaces.ts` | 1) direct swept segment; 2) ≥16 cells with no blockers: navmesh (32-cell tiles, weighted A* ×1.2 + funnel), rasterised and repaired; 3) else 8-neighbour integer A* (1000/1414, octile, radix queue), ≥32 cells restricted to a 16×16-sector corridor. Destination-pocket probe, waypoint smoothing. Air: trivial grid. Bridges: `WalkSurfaces.path`. Per-body-radius navigation buffers |
| Combat | `combat.ts` (461), `attackApproach.ts`, `attackPositions.ts`, `targetIndex.ts`, `charge.ts`, `attackTiming.ts`, `damage.ts`, `stats.ts` | Acquisition every 8 ticks (`tick%8==id%8`), nearest by (distance, id); windup/recovery timeline; simultaneous damage (summed per target, applied in id order, then deaths/XP); armour `1/(1+k*armor)` |
| Projectiles | `missiles.ts`, `shellCombat.ts` | Homing missiles: flight = `ceil(dist/speed*40)` ticks, `shotClear` at impact. Shells: fixed point, splash + slow, victims' owners captured at launch |
| Abilities | `abilities/runtime.ts` (528) + castOrders, weaponCasts, worldEffects, summons, corpses, containment, splitForms, swarm, reactions, taunt, recipients, magnitudes, eligibility, validation | Data-driven effect-op interpreter; casters in id order; preparing → channeling → recovering; mana regen integer accumulator /40000; lifecycle reactions ≤128/tick |
| Statuses | `abilities/statuses.ts`, controlPolicy, damagePolicy, concealment, forms, effects | Auras rebuilt every tick; forms override definitions |
| Items | `itemEffects.ts`, `inventory.ts`, `loot.ts`, `campLoot.ts`, `dropPosition.ts` | Auras, actives, rescue, on-hit procs |
| Economy | `economy.ts` (1123), population, supply, goodsView, colony | Jobs deliver/construct/repair/harvest/plant (walk/wait/work/fall/return); harvest FIFO per source with `activeWorkers` cap; felling; regrowth |
| Construction | `game.ts:174-252,462-490`, economy construct jobs | Bill reserved up front; HP grows with progress |
| Production / research / upgrades | `economy.ts` startWorkplace/completeUnit, `research.ts`, `upgrades.ts` | Deployment around the building perimeter |
| Vision / fog | `observation.ts` (972), `visionMask.ts`, `shared/map/tacticalTerrain.ts`, `shared/spatial/sectors.ts` | Per-owner memory (cells 0/1/2, remembered scenery, observed deaths) is saved state. Height-filtered circles, not ray occlusion (visible if target ≤ sensor height + 200 cm). Incremental sensor spans |
| Camps | `combat.ts:73-85,147-162`, `campLoot.ts` | Leash/return, aggression, loot once when the last defender dies |
| Heroes / XP / revival | `progression.ts`, `revival.ts` | Fallen heroes stay as entities; XP split by id with remainder rules |
| Garrison | `garrisons.ts` | Single-slot lookouts; firing and sight elevation |
| Formation / idle | `formation.ts`, `idleMotion.ts` | Idle motion from a hash of id and tick |
| Regeneration | `regeneration.ts` | Integer accumulators |
| AI | `ai/playerAI.ts` (795), frame, economy, tactics, state, observedNavigation, briefing, placementSearch, orderContinuity; `abilities/ai.ts` | Briefing → geography → per-decision frame from own observation → observed navigation → decide: abilities, heroes, strategy (200 ticks), economy (80), reactions, operations (40). ≤4 commands/beat. Decisions every 10 ticks, staggered. Commands enqueued locally at tick+1 on every peer (not broadcast) |
| Scenario / Lua | `sim/scenario/mission.ts`, `inventory.ts`, `company.ts`; `shared/scenario/lua.ts`, `schema.ts` | See below |
| Victory | `game.ts:630-653`, `colony.ts` | Defeated with no colony buildings; ends at ≤1 team; missions decide via Lua |

## Tick order (`World.tick`, `Game.tick` at `game.ts:564-648`)

1. Clock; apply due commands sorted by (tick, player, seq).
2. Cinematic dialogue pauses the game tick (observe only).
3. Clear expired stuns → ability lifecycle (statuses, auras, weapon cast orders, channels, mana) → cast orders → ambient → taunts.
4. Item effects → regeneration.
5. Order advance → economy assignment.
6. Garrisons → inventory plan → combat plan → separation → idle motion → movement.
7. Cast orders (second pass) → item effects.
8. Combat resolve (projectiles first), deaths.
9. Ability resolve (reactions, persistent effects, deliveries, waves) → taunts.
10. Revival → economy advance → inventory → upgrades → research.
11. Mission tick → observation update → defeat/victory.
12. AI decisions (enqueued for tick+1).

## State model

One `state.entities` array of plain objects with optional component-like sub-objects (`unit`, `production`, `resource`, `item`, `construction`, `abilities`, `revival`, `research`, `progression`, `spellStatuses`…); not an ECS. Derived indexes rebuilt by `reindex()`. Monotonic id counters (`nextId`, `nextJob`, `nextQueue`, `nextFact`, `nextCast`, `nextMissile`, `nextShell`, `nextSpellVision`), never reused. Initial placements created in placement-id order. New entities are eligible from tick+1.

**Randomness:** gameplay xorshift32 (`game/loot.ts`) only for camp loot, critical/evasion and reaction chance; seeded from the match seed. No `Math.random`.

## Outputs

- `Game.view(owner)` → `SettlementView`: entities (sorted by id, with private control/production/job for the owner, remembered fogged scenery), fog cells and revision, facts, death cues (80 ticks), missiles/shells, corpses, fallen heroes, ~40 ability event kinds, deliveries, hero returns, goods, supply, population, research, mission, outcome, objectives.
- Save: JSON `LocalSave` v4 with world snapshot (tick, rng, slots, pending, full game state, per-owner knowledge, AI states). No replay files.
- Multiplayer hash: sampled FNV-1a fingerprint (counters, jobs, stocks, outcome, ≤32 rotating actors) every 8 ticks; the server answers `hashOk` or `desync`.

## Lua scripting

- fengari (Lua 5.3), fresh state per run, no standard libraries, 20,000-instruction limit, strings ≤64 KiB, scalar arguments and returns only, script ≤64,000 characters.
- `on_start` once, then `on_tick` every 4 ticks; ≤256 API calls per run; transactional (mutations queued, applied only on success, then `spatial.rebuild()`); any error permanently sets `mission.error`. A cinematic `say` pauses the game tick.
- API (`mission` table, 34 functions): `has_item`, `give_item`, `take_item`, `dialogue_busy`, `tick`, `count`, `stock`, `alive`, `position`, `in_region`, `arrived`, `facing_done`, `get`, `set` (≤128 vars), `damage`, `recover`, `spawn` (script-activated placements), `move`, `attack_move`, `follow`, `attack`, `transfer`, `transform`, `begin_scene`, `end_scene`, `camera`, `face`, `say`, `begin_objective`, `complete_objective`, `fail_objective`, `objective`, `win`, `lose`. Entities are addressed by placement id.

## Performance notes

- Hot: movement (per-tick occupied sets, blocker scans, multi-ray sweeps), `clearSegment`, combat target index rebuilt every pass, observation update for all owners every tick, heavy `structuredClone` in views.
- O(N)/O(N²): job lists, `hall()`, `colonySupply`, aura target scans, removal by splice, `canBuild` over all resources, definition lookups through forms on every call.
- Full `Spatial.rebuild()` (occupancy + all navmeshes) on tree regrowth, mission operations, revival and restore; lazy navmesh compile caused hitches, so meshes are prebuilt per radius.

## Determinism hazards in the old code

Transcendentals: `hypot` ~37, `sin`/`cos` ~23, `atan2` 3, `sqrt` ~8 in sim and map code; `atan2` in facing runs every moving unit every tick. ~176 `Math.round`. Iteration order matters in unit buckets (insertion-ordered sets), traffic components, static occupancy ("last writer wins"), JS object key order in stock records and AI tables. ~108 sorts, some without total order (rely on stable sort and input order). Derived caches must answer like a cold rebuild after restore.

## Map load

Terrain heights and forest placements come from the authoring pipeline at load (`shared/authoring/project.ts` → `compileMapScene`, float and trig heavy), not from stored data.

## Couplings to fix in Rust

- Owner literal `'player.1'` in missions and `scenario/company.ts`; placement prefix `"start."` selects starting gather workers (`economy.ts:330`).
- Damage type `'spell'` hard-coded (`abilities/damagePolicy.ts`, `statuses.ts`, `itemEffects.ts`, `mission.ts`); fallback `"normal"` (`combat.ts:314`) which content doesn't define.
- Defaults in code: turnRate 720, flightHeight 6, stall 200 / yield 120 ticks, approach retry 6/20, camp-AI sight 24, cinematic reveal 36.
- UI text "Build a Mound" (`supply.ts:45`).

## Tests worth mining

`tests/net/*-lockstep.test.ts` and the scenario-style suites in `tests/game`, `tests/abilities` describe behaviour to keep.
