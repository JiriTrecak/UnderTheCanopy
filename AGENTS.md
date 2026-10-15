# Under the Canopy

A Warcraft III–style RTS on the forest floor, being rebuilt natively in Rust. Read [the plan](docs/plan.md) first: phase 1 rebuilds the existing game with identical gameplay and redesigned, maximally optimised internals.

## Ground rules

- **Behaviour comes from the old game, code does not.** The old TypeScript repository (`../Settlers 3 Web`, `settlers-web`) is the specification for what the game does and how it looks. Read or run it to answer questions; never translate its code line by line.
- **Ask before deciding anything visible or felt**: lighting, water, fog, colour grading, camera and input feel, performance-versus-quality trade-offs. Bring options with a recommendation, and side-by-side captures for visuals. Record agreed and purely technical decisions in [docs/decisions.md](docs/decisions.md).
- **Stay inside phase 1.** Editors, the match server, Steam, signing and distribution are phase 2 ([plan](docs/plan.md)).
- **Data is mirrored, not edited here.** `content/`, `assets/` and `art/` come from the old repository through `just sync-data` and keep their formats exactly. Don't reorganise or extend them.
- **Content is input.** No code names a shipped content id. Tests define their own units, abilities and effects.
- **Test what matters.** Broad scenario tests, replay-hash determinism tests, generic content validation, and unit tests for real algorithms. No tests of incidental numbers or duplicates.

## Release notes

- **Note every player-visible change** you complete in `releases/log.json` with `just release-note --kind added|improved|fixed|knownIssues --text "Concise player-facing change"`. It is the one shared log: keep other agents' notes, don't start other changelogs, don't invent changes or test results. Internal-only work needs no note.
- **Never prepare or publish a release without the user's intent to release**; ordinary work only maintains Unreleased. Routine releases are patches; `0.X.0` milestones need an agreed major upgrade and `--milestone`. Read [docs/releases.md](docs/releases.md) before release, publishing or updater work, and never show, commit or upload the updater's signing key.

## Engineering rules

- **Determinism** ([D4](docs/decisions.md)): simulation code uses deterministic math only, fixed iteration order, and the simulation's seeded random generator. No `HashMap` iteration, platform `f32::sin`, wall clocks or thread timing in simulation results.
- **Portability** ([D1](docs/decisions.md), [D6](docs/decisions.md)): nothing macOS-only. All file locations go through `game/src/paths.rs`. CI must pass on macOS and Windows.
- **WebGPU baseline** ([D2](docs/decisions.md)): no wgpu features or limits beyond WebGPU's without an agreed exception.
- **Measure**: add `tracing` spans to anything that costs time; check performance with `just profile` and Tracy, on the reference machine (base M1) when it matters.
- **Dependencies** ([structure](docs/structure.md)): crates never depend on `game/`; `canopy-render` never sees simulation types.
- `just check` (format, clippy with warnings denied, tests) passes before every commit.

## Visual previews

Use neutral zinc for model previews and review benches: background `#18181b`, ground and panels `#27272a`, controls and borders `#52525b`. Avoid green or olive stages. Verify units at normal game zoom from the overhead camera and from directly above, in every player colour including white and dark.
