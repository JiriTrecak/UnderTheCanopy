# Combat feedback and performance captures

This pass adds visual impact feedback, layered spell authoring, silent ant articulation and tools for identifying rendering or simulation stalls. It does not add audio.

## Confirmed impact feedback

A short directional chip/spark burst accompanies an observed loss of HP. Attacking without dealing damage does not emit a hit. The renderer uses only observed entities, so hidden attackers are not revealed. The shared pool holds at most 64 bursts / 512 particles and removes bursts when their target leaves observation or their short lifetime expires. These particles have no collision or gameplay effect.

## Talking ants

Worker, Warrior, Archer, Marshal, Hunter and Bombardier have separate mandible bones. Text timing opens and closes the jaws, with punctuation and spaces creating pauses. Speech runs after the body animation mixer and does not replace locomotion, attacks or casting. Death closes the jaws. Each clone controls its own bones.

Use the optional final `actorTag` in `mission.say` to name the speaker. Heartwood Vault's lines now identify Marshal and Scout explicitly. Existing 2D portrait images remain static; the animation is on the actual 3D unit. The character studio has a **Speaking** checkbox for inspection. See [mission scripting](/development/mission-scripting) and the canonical character source and recipe resources in `art/assets/<id>/`.

## Capture a slowdown

1. Open **Performance** (Ctrl+F3) while the slowdown is visible. Keep the same map, camera, viewport and graphics settings when comparing changes.
2. Choose **Capture 10 seconds**. Play through the problematic situation. Wait for recording to finish; **Last capture** then becomes available.
3. Use **Report** to download JSON containing the live report and finished capture. **Set baseline** stores that capture for comparison with the next one. **Download trace** exports measured CPU spans and sampled simulation, AI, frame-interval and GPU counters as trace-event JSON.
4. Repeat after changing one variable. Compare GPU frame/atmosphere, CPU scopes and frame intervals separately. Negative baseline percentages mean less time.

The overlay reports the fraction of presented frame intervals over the 120 Hz budget, mean, p95 and p99 latency, a recent-frame graph, and counters for render calls, geometry, effects and simulation work. Captures retain up to 4,096 samples per scope and the 40 worst frames exceeding the 8.33 ms budget. GPU frame, scene, atmosphere and portrait queries rotate without nesting and only run while profiling is enabled. Unsupported GPU timer extensions leave GPU timings unavailable rather than estimating them from CPU submission time.

CPU scopes overlap and must not be added together. GPU timings arrive asynchronously. A spike record contains the latest sampled scopes, not a causal per-frame trace. The downloadable trace records CPU scopes at their actual start/duration, while simulation/AI/GPU counters are timestamped when delivered. These counters are not execution spans; GPU results in particular arrive late. Trace storage is capped at 64,000 events, with any dropped events reported in the file metadata. Starting another capture replaces the previous trace. Counters in the summary report are the last frame, not capture averages. Close other GPU-heavy tools before comparisons, and repeat runs before making a performance claim.

## Changes made from profiling

- The spell studio freezes its covered map renderer and stops its own render loop when the document is hidden.
- Scenery lights cache unchanged declarations and camera selection instead of rebuilding a JSON signature every frame. Terrain changes explicitly invalidate their sampling.
- Spell particles share geometry/masks, instance draws and cached terrain samples. Global budgets bound expensive visual combinations.
- The fog marcher uses a deterministic 32 KiB 3D noise lattice instead of recomputing eight hashes for each interpolated noise lookup. Fog resolution and march-step counts were retained; noise detail changes slightly because the texture repeats.
- Observation maintains overlapping per-sensor coverage and updates only changed sight edges. Trees publish immutable projections refreshed by harvest/regrowth receipts. Resource gathering counts and work jobs are indexed once per update; units remain fully projected. Explicit tooling edits, replacement and restore rebuild derived data. Fog memory retains the last witnessed state rather than leaking offscreen resource changes.
- Visibility footprints use an LRU cache bounded by both entry count and 4 MiB of cell IDs. Existing terrain occlusion rules remain intact.
- Worker candidate searches share body blockers and a bounded proof of a closed starting pocket. A boxed-in worker no longer repeats a map search for hundreds of tree candidates.
- A* reuses typed heap storage, retains integer costs and stable ties, and caches static legal edges separately from moving bodies. Building/resource changes invalidate neighboring edges. The subsequent sector pass rebuilds affected terrain clusters and their boundary links instead of flooding the complete cell grid.
- Combat planning shares target terrain checks and uses spatial buckets for occupancy. Reservations remain per actor and no cache survives movement.
- AI intent is tracked per actor, independent of squad membership. A new reinforcement or stalled unit receives an order without resetting every progressing member's path. Continuing high-priority orders still reserve the actor for that decision beat.
- Simulation telemetry separates worker assignment, queued orders, inventory planning, combat planning, idle movement and actual movement. Paused cinematics clear previous tick timings rather than displaying stale work.
- Profiling uses fixed-size timing rings instead of shifting arrays. GPU timer polling avoids GL state queries when disabled and idle. Reference-stage measurements clear the previous mode's samples after warm-up.

## Measurement and limits

The target is 8.33 ms for a complete 120 Hz frame, with headroom for slower machines. Average improvements do not establish that target: measure p95/p99, worst ticks, loading events and command latency. Simulation ticks, render CPU work, asynchronous GPU timing and display intervals are distinct scopes.

Use `npm run bench:sim -- --map echo-isles --ticks 12000 --output /tmp/utc-simulation.json`. Navigation tracing and checkpoint/resume options isolate expensive searches. The [sector](spatial-sectors.md) and [worker](simulation-worker.md) guides explain their boundaries. Keep results in ignored `tmp/` and record the map revision, camera, resolution and graphics settings for browser measurements.

Verify the effect editor and profiling controls in the actual browser as well as automated tests. Whole-frame performance remains hardware- and scene-dependent; no historical benchmark establishes current 120 FPS performance.
