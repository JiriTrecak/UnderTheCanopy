# Decisions

Decisions that shape the code. Newest last. Each entry says what was decided and why; reverse one by adding a new entry that supersedes it.

## D1. Rust and wgpu, shaders written once (2026-10-14)

The game is Rust on wgpu with WGSL shaders, driving Metal on macOS, DX12 on Windows and Vulkan as fallback. No per-backend shaders or code paths. No engine framework (no Bevy): our own architecture on a few libraries (wgpu, winit, an audio library, taffy, cosmic-text). Why: full control, predictable performance, one renderer everywhere.

## D2. WebGPU feature baseline (2026-10-14)

Devices are created with WebGPU's default limits and only features WebGPU also offers (plus BC texture compression when the cooker needs it). Native-only features are allowed only as optional fast paths with a fallback, and each one is agreed with Jiri first. Why: the editors will embed the same renderer through WASM and WebGPU in phase 2.

## D3. Simulation at 40 Hz on its own thread (2026-10-14)

The simulation ticks every 25 ms on its own thread; the renderer runs at the display rate and interpolates between snapshots. Why: the frozen server and lockstep input delay are built on 25 ms ticks, finer ticks keep commands responsive, gameplay is tuned at this rate, and the cost in Rust is small. The tick length is one constant. Content durations (`*Ticks` fields) are authored in fixed 25 ms units (`AUTHORED_TICK_MS`) and converted to simulation ticks on load, so a different tick rate would not change the data. Parallel work inside a tick merges results in a fixed order.

## D4. Determinism from day one (2026-10-14)

Simulation math uses deterministic functions (`libm` or fixed point), never platform math libraries; iteration orders are fixed; randomness comes only from the simulation's seeded generator. CI runs replay-hash tests on macOS and Windows, and hashes must match. Why: lockstep multiplayer and replays depend on it, and adding it later means rewriting the simulation.

## D5. Lua 5.4 through mlua for scenario scripting (2026-10-14)

Map and campaign scripts run on vendored Lua 5.4 via `mlua` with no standard libraries opened, exactly like the old sandbox: scripts reach only the `mission` API, so platform maths and Lua's own randomness are unreachable, and the unfixed string-hash seed only affects table iteration, which scripts cannot observe. Why: today's scripts target Lua 5.3 (fengari) and run on 5.4 nearly unchanged; Luau would break integer and bitwise semantics. Differences scripts can notice are listed in [simulation](simulation.md#lua).

## D6. Platform layer owns every path (2026-10-14)

No code depends on the working directory. Game data is found at `Contents/Resources/data` in the macOS bundle, `data/` beside the Windows executable, or `--data <folder>` in development; user files live in the OS user-data folder (`com.jt.canopy`). Why: packaging for any platform then needs no code changes.

## D7. Packaging and CI from M0 (2026-10-14)

CI builds, lints, tests and packages on macOS (Apple Silicon) and Windows on every push: an unsigned `.app` and an NSIS installer through `cargo-packager`. Checkouts skip Git LFS. macOS minimum version 13. Why: portability problems surface immediately; signing and installers are added later without code changes; LFS bandwidth is billed.

## D8. Apple Silicon only (2026-10-14)

macOS builds target arm64; no Intel support. The reference (weakest) machine is a base M1 with a 7-core GPU. Why: Intel integrated GPUs would set a far lower budget for little benefit.

## D9. Data mirrored, formats unchanged during phase 1 (2026-10-14)

`content/`, `assets/` and `art/` are mirrored from the old repository with `just sync-data` and keep their formats exactly. `canopy-content` is a strict reader of those formats. Why: the editors stay in the old repository until phase 2, so its output must work here unchanged. Identical binaries in `art/` and `assets/library/` cost nothing extra in LFS (content-addressed).

## D10. Match server and protocol frozen (2026-10-14)

The TypeScript match server stays in the old repository and stays deployed. `canopy-net` implements the client side of its protocol exactly. The server embeds `content/game.json` (races, starting heroes) and action-shape validation, so new races or heroes need the server redeployed with the new content, and new action types need a server change. Why: it works, and porting it adds nothing to the game.

## D11. Profiling built in (2026-10-14)

`tracing` spans everywhere; the `profile` feature streams them and frame marks to Tracy (`just profile`). The `profiling` Cargo profile is release with full symbols. Why: performance claims come from measurements on the reference machine.

## D12. Content validation scope in phase 1 (2026-10-14)

`canopy-content` rejects anything structurally wrong (unknown fields, wrong types, unknown enum values) and enforces every cross-reference and invariant the simulation relies on, matching the old loader's rules. Pure authoring limits (for example a maximum speed of 40) are not duplicated while the old editors still validate the data; they move to Rust with the editors in phase 2. `just parity` compares the Rust reader with the TypeScript loader on the same files. Why: full constraint parity is editor work; the game needs correct reading and sound invariants.

## D13. Sampled multiplayer hash (2026-10-14)

Desync detection keeps the old design: every 8 ticks a cheap fingerprint of the id counters, jobs, stocks, outcome and a rotating sample of at most 32 actors, in the existing `hash` message. A full-state hash is not computed. Why: it is enough to catch desyncs, and hashing everything every few ticks costs too much.

## D14. Fix bugs found in the old simulation (2026-10-14)

"Behaves the same" covers intended gameplay, not defects. Clear bugs found while rebuilding are fixed and noted in the commit and, if players would notice, in the release notes. Changes to how the game plays still go to Jiri first.

## D15. Movement and pathfinding keep the old rules for now (2026-10-14)

Movement, pathfinding, collision and traffic are rebuilt with the same rules and behaviour as the old game, implemented efficiently. A proper redesign (flow fields and more) comes later as its own piece of work.

## D16. New save format (2026-10-14)

No compatibility with the old game's saves; nobody depends on them. The Rust save format is designed fresh.

## D17. Verification by play (2026-10-14)

No automated behaviour comparison against the old game. Correctness comes from scenario and determinism tests, and from Jiri playing the game once it is close.

## D18. One unsafe exception: the Lua instruction hook (2026-10-14)

`crates/sim/src/mission/hook.rs` uses about 15 lines of `unsafe` to install a raw Lua count hook that counts instructions and inspects the running function's registers for strings over 64 KiB, as the old sandbox did. mlua's safe hook cannot see registers and is several times slower per instruction. The rest of the workspace keeps `unsafe_code = "deny"`; any further exception needs a decision here.
