# Performance and match loading

A smooth RTS needs predictable frame times as well as a high average frame rate.
A half-second search is visible even when the surrounding frames are fast.
Measure simulation, presentation and the GPU separately, and keep commands and
combat authoritative while optimizing them.

Targets: 8.33 ms for a complete 120 Hz frame, and **3 ms p99** whole-match
simulation CPU per 25 ms window in four-player matches. Averages do not establish
either target: measure p95/p99, worst ticks, loading events and command latency.
Timing reports are machine-specific; record map, revision, camera, resolution and
graphics settings, and keep output in ignored `tmp/`. The profiler UI and capture
internals are documented in `src/debug/performance.md`.

## Browser capture

**Performance · Ctrl+F3** opens the shared profiler in matches and the map editor.
`gamePerformance` captures include `cpuBudget`: timestamped worker callback roots,
browser snapshot receive/send, and match input/HUD work before and after the
scene draw, unioned per thread into fixed 25 ms windows. GPU work and message
latency are never added to that total. `worst[].contributors` lists the worker
tick and overlapping roots; contributor rows may nest, so use window totals, not
their sum. `completeAccounting` is **false**: some UI callbacks, native message
dispatch, layout/paint and profiler overhead are not covered. Elapsed timing
includes OS preemption; a 25 ms aggregate percentile is not a per-tick percentile.

Capture a restored browser battle through the MCP hub:

```sh
node --import tsx scripts/bench/browser-budget.ts --map echo-isles \
  --load /path/to/browser-save.json --x 100 --z 128 --output /tmp/browser.json
```

`--mode details` adds diagnostic simulation subcategories; the default budget
mode disables them. For manual input checks, start **Quiet capture** in the debug
UI, exercise the controls immediately, then export with `--read-capture
--interaction "description"`; check `sourceCounts` to confirm input was recorded.
`--prepare-only` restores a battle without starting the ten-second capture. Do
not run benchmarks or interact with the browser during a quiet baseline.

## Headless benchmarks

```sh
npm run bench:sim -- --map echo-isles --ticks 12000 --output /tmp/sim.json
npm run bench:match -- --map echo-isles --output /tmp/match.json
npm run bench:match -- --multiplayer --map echo-isles --peers 2
```

`bench:sim` runs AI players with seed `731942`, builds the observer view every
tick, discards 200 warm-up samples, and reports mean/p95/p99/max, phase timings,
unit counts and a deterministic checksum. `--trace-navigation`, `--trace-routes`,
`--trace-sight` and `--trace-slow` add diagnostics; `--checkpoint <tick>` and
`--resume <file>` save and continue a world (restored runs have cold caches).
`bench:match` bundles the production runtime before measuring
`scripts/bench/match-budget.ts`; `--multiplayer` runs
`scripts/bench/multiplayer-match.ts`, one lockstep replica per peer. Profiling
flags (`--cpu-profile`, `--allocation-profile`, `--details`) add overhead and are
not budget evidence.

Other repeatable benches: `scripts/bench/worker-isolation.ts`
([simulation worker](/development/expansion/simulation-worker)),
`scripts/bench/sector-scaling.ts` ([spatial sectors](/development/expansion/spatial-sectors)),
`npm run bench:scenery-rays`, `npm run bench:scenery-visibility`, the browser
benches `bench:textures`, `bench:scenery-cells` and `bench:worker-decode`, and
`scripts/bench/spell-effects/`.

Stop browser matches and other benchmark/test processes before a comparison.
Laptop sleep or background throttling invalidates wall-clock results; rerun
rather than averaging the pause away. Headless results are CPU workloads, not
browser FPS predictions.

## Map editor

`editor_performance` exposes the same profiler through MCP: `get` reads build
stages, the last presentation, rolling samples and a completed capture;
`capture` records 10 seconds; `census` attributes the next frame's draws to scene
branches; `trace` returns Chrome trace JSON; `reset` and `enabled` control
sampling. The **Opening** tab and MCP `startup` report the initial load's map
compilation, renderer creation, terrain upload, first submission, asset waits and
final synchronization. Asset-group waits overlap; never sum them.

For a CPU generation run:

```sh
BENCH_RUNS=6 npm run bench:authoring -- assets/maps/skirmish/echo-isles.utcmap
```

The first run warms caches; the report includes individual timings, generation
stages, object counts and an output checksum. It measures CPU compilation, not
downloads, scene construction or first frame. `scripts/bench/editorEdits.ts` and
`scripts/bench/terrain-edits.ts` measure incremental edits on a map path.

## Before the match starts

The loading screen stays visible while the game:

1. Constructs the map-backed simulation and its spatial data.
2. Loads gameplay models, map scenery, texture dependencies, declared command
   icons and fonts.
3. Initializes textures and compiles material variants against the actual game
   lights and fog, then renders a small offscreen pass to upload shared geometry
   and skinning buffers.
4. Presents the battlefield, releases the input barrier and starts the clock.

File progress counts completed requests including dependencies; synchronous
stages use a named indeterminate indicator. Required asset failures stay on the
screen with a return-to-menu action. Cancelling invalidates the session, releases
listeners and disposes the renderer and late-arriving resources. Loading clients
withhold simulation confirmations until ready, so a room cannot run ahead of a
slow peer. Preloading prevents first-use downloads; it does not prevent pauses
from garbage collection, GPU scheduling or new runtime instances.

## Implementation boundaries

**Simulation indexes.** The context maintains structural indexes for units,
buildings, bodies and vision providers; systems still check life, ownership,
readiness and behavior at query time. Combat uses a spatial broad phase followed
by exact range, hostility and visibility tests, with unchanged distance/ID
tie-breaks. No attack is delayed to meet a wall-clock budget.

**Navigation.** Static components reject disconnected destinations; A* and the
ground mesh choose routes. Long routes keep their terrain corridor when a nearby
body blocks the next step: bounded local recovery and rejoin searches handle
traffic, then units wait or yield. There is no flow field. See
[spatial sectors](/development/expansion/spatial-sectors).

**Observations.** Unchanged neutral resources reuse immutable public projections;
fog memory keeps the previous projection when a tree changes out of sight. The
renderer never receives mutable simulation entities. Resource scenery preserves
array identity when stamps are unchanged.

**Rendering.** Scenery uses spatially partitioned instances for camera and shadow
culling with static matrices. Compatible static model surfaces merge into one
geometry; transparent, animated, skinned and differently textured or team-colored
materials stay separate. Drag previews update instance matrices directly.
Visible unfocused windows keep `requestAnimationFrame`; a worker timer takes over
when the document is hidden, sharing one elapsed-time clock.

## Regression expectations

Performance changes must preserve command responsiveness, collision and slope
rules, target choice, fog memory, recruitment, harvest timing and multiplayer
checksums. Run the focused tests and the full suite, and compare a fixed-seed
match checksum before and after any change claimed to be behavior-neutral.

## Map browsing and published previews

Skirmish, multiplayer and editor map pickers consume a lightweight map summary.
They never generate terrain, load models or run full playability validation while
browsing; full validation runs when saving and starting a match.

`npm run maps:previews` publishes 512×512 WebP strategic atlases and
`assets/maps/previews/index.json`, sampling the compiled terrain offline.
Unchanged inputs are skipped. `npm run maps:previews -- echo-isles` regenerates
one map, `--force` forces regeneration, and `npm run maps:check` fails on stale
or missing outputs. The game build runs the generator before bundling. Custom
artwork (`<map-id>.preview.png`, `.webp`, `.jpg`) next to a `.utcmap` is
center-cropped and encoded instead; commit source and output.
