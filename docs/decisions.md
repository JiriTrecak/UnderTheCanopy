# Decisions

Decisions that shape the code. Newest last. Each entry says what was decided and why; reverse one by adding a new entry that supersedes it.

## D1. Rust and wgpu, shaders written once (2026-10-14)

The game is Rust on wgpu with WGSL shaders, driving Metal on macOS, DX12 on Windows and Vulkan as fallback. No per-backend shaders or code paths. No engine framework (no Bevy): our own architecture on a few libraries (wgpu, winit, an audio library, taffy, cosmic-text). Why: full control, predictable performance, one renderer everywhere.

## D2. WebGPU feature baseline (2026-10-14)

Devices are created with WebGPU's default limits and only features WebGPU also offers (plus BC texture compression when the cooker needs it). Native-only features are allowed only as optional fast paths with a fallback, and each one is agreed with Jiri first. Why: the editors will embed the same renderer through WASM and WebGPU in phase 2.

## D3. Simulation at 40 Hz on its own thread (2026-10-14)

The simulation ticks every 25 ms on its own thread; the renderer runs at the display rate and interpolates between snapshots. Why: the frozen server and lockstep input delay are built on 25 ms ticks, finer ticks keep commands responsive, gameplay is tuned at this rate, and the cost in Rust is small. The tick length is one constant; content is authored in seconds and converted on load. Parallel work inside a tick merges results in a fixed order.

## D4. Determinism from day one (2026-10-14)

Simulation math uses deterministic functions (`libm` or fixed point), never platform math libraries; iteration orders are fixed; randomness comes only from the simulation's seeded generator. CI runs replay-hash tests on macOS and Windows, and hashes must match. Why: lockstep multiplayer and replays depend on it, and adding it later means rewriting the simulation.

## D5. Lua 5.4 through mlua for scenario scripting (2026-10-14)

Map and campaign scripts run on vendored Lua 5.4 via `mlua`, with a fixed string-hash seed, `math` and `random` routed to the simulation's deterministic functions, and `os`/`io` removed. Why: today's scripts target Lua 5.3 (fengari) and run on 5.4 nearly unchanged; Luau would break integer and bitwise semantics.

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
