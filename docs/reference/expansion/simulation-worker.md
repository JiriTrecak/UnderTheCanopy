# Simulation worker

The match runs in a dedicated Web Worker. The main thread owns input, camera, HUD, Three.js presentation and rendering submission. It reads the newest completed presentation snapshot; it never advances or waits for the authoritative simulation inside a render frame.

## Ownership and scheduling

`SimulationRuntime` owns `World`, AI, navigation, visibility, economy, combat, campaign Lua, local `Room` and lockstep mailboxes. Its clock advances in fixed 25 ms simulation ticks. The worker scheduler processes at most one tick per callback, then yields so incoming commands can be handled before the next tick. Accelerated local matches use the same tick rules, with bounded catch-up. Remote matches stay at normal speed and never discard committed turns.

The main thread forwards network messages to the worker and sends the worker's turn confirmations and hashes through the existing channel. Confirmations are based on simulated time, not elapsed wall time. A slow or suspended peer therefore cannot promise an arbitrarily long sequence of empty turns and push subsequent input far into the future.

Rendering and simulation have independent clocks. Existing transform smoothing settles onto the latest authoritative position. The presentation clock advances at most one unconfirmed tick; it does not extrapolate units through obstacles or invent combat outcomes during a prolonged stall. Camera controls, selection and local command markers remain responsive independently of route computation.

Commands retain FIFO transport and lockstep ordering. New movement orders supersede earlier orders when applied by the simulation. Pathfinding currently remains synchronous **inside the worker**: a long search can still delay combat and new orders in that worker. It cannot directly block the main-thread render loop. Separate, versioned pathfinding jobs would be a further architectural step.

## Snapshot transport

The worker publishes at most 40 ordinary presentation snapshots per second, independent of match acceleration. Initialization, restore and explicit view changes can publish immediately. Only one packet may be awaiting acknowledgment. If the main thread falls behind, the worker keeps simulating and coalesces unpublished state; it does not queue a snapshot for every missed tick.

The stream sends entity additions, changes and removals. Unchanged resource projections retain their identity in the decoder and are not repeatedly copied across the worker boundary. Fog sends an initial full buffer, then changed cell indices/values, choosing another full buffer when that is smaller. Fog buffers are transferred, rather than cloned. Static deck metadata only travels when its source changes.

Every packet has a sequence number. Missing deltas fail explicitly; restore establishes a fresh baseline. The sender only encodes a delta when it can publish it, so coalescing cannot create references to a snapshot the receiver never saw.

Visual/debug revelation and the player's selection/command observation remain separate projections. Revealing another player's position does not grant command authority or change AI knowledge. AI observation remains entirely in the worker.

## Asynchronous operations

- **Placement preview:** at most one validation query is in flight. Pointer movement replaces the pending query; stale responses cannot repaint an older cursor location. The simulation validates the actual build command again when it executes.
- **Save:** the worker captures world state and command mailboxes together between ticks. UI control groups are attached on the main thread.
- **Load:** validation constructs and restores a world off the main thread. Invalid imported saves leave the current paused match intact. The presentation waits for the fresh snapshot baseline before reporting restoration complete.
- **Campaign continuation:** surviving company state is captured in the worker before loading the next chapter.
- **Lifecycle:** stopping a session terminates its worker and rejects pending requests. Initialization failures reach the loading screen; runtime failures leave rendering available and report the stopped simulation.

## Telemetry

Debug profiling distinguishes worker simulation, snapshot projection, encoding, main-thread decoding, snapshot delivery and command-to-applied-tick latency. Snapshot age shows how long the displayed state has been waiting for a replacement.

When profiling is enabled, simulation and AI timing samples are collected per actual tick/decision and batched into presentation packets. The batch is capped at 4,096 samples; dropped samples are explicitly counted. A slow consumer cannot create an unbounded profiler queue. Projection and encoding samples are per published snapshot, which is a different cadence from simulation ticks.

Command latency includes main-to-worker delivery, command pipeline delay, simulation execution and acknowledgment delivery. It measures application of the command, not the time at which a unit first turns or moves on screen. Snapshot delivery includes message scheduling and decoding by the platform before the main callback, not network latency.

## Verification

The regression tests execute the production worker entry in real Node worker threads, with an adapter for the browser message port. They cover autonomous ticking without a renderer, bounded snapshots when acknowledgments are withheld, restore during an in-flight packet, a deliberately stalled worker with a responsive parent loop, latest movement order, immediate local feedback and two-worker lockstep hashes. Existing latency/jitter tests exercise the extracted runtime and command pipeline.

The Node adapter is test tooling, not a second simulation implementation. Browser gameplay and production builds must also be checked because Node does not reproduce browser scheduling, GPU load or module loading.

Run the repeatable isolation benchmark with:

```sh
node --import tsx scripts/bench/worker-isolation.ts --ticks 12000 --output /tmp/worker-isolation.json
```

It runs Echo Isles (or `--map <id>`) through the production worker protocol, at 4× speed, with an 8 ms parent heartbeat and no renderer. It reports worker tick cost, publication costs, main-thread message handling, heartbeat intervals, transport counts and a final deterministic checksum. The heartbeat is evidence of event-loop isolation, **not a browser FPS measurement**. Compare it with the [sector benchmark](/development/expansion/spatial-sectors) using the same map, seed, slots and tick count.

### Remaining limits

The renderer, GPU workload, HUD and scene updates still have their own frame budget. Worker isolation does not establish 120 FPS on slower machines. A long worker tick can still delay authoritative movement and combat; it simply no longer occupies the rendering thread. The next independent controls are publication frequency, per-publication projection/encoding cost, worker catch-up scheduling, and eventually separately scheduled navigation jobs with versioned results.
