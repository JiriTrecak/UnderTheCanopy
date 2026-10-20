# Under the Canopy

A Warcraft III–style RTS on the forest floor, being rebuilt natively in Rust. Read [the plan](docs/plan.md) first: phase 1 rebuilds the existing game with identical gameplay and redesigned, maximally optimised internals.

## Ground rules

- **Behaviour comes from the old game, code does not.** The old TypeScript repository (`../Settlers 3 Web`, `settlers-web`) is the specification for what the game does and how it looks. Read or run it to answer questions; never translate its code line by line.
- **Ask before deciding anything visible or felt**: lighting, water, fog, colour grading, camera and input feel, performance-versus-quality trade-offs. Bring options with a recommendation, and side-by-side captures for visuals. Record agreed and purely technical decisions in [docs/decisions.md](docs/decisions.md).
- **Phase 2 milestones start with a design discussion** ([plan](docs/plan.md)): no code for a milestone before Jiri has settled its design. The match server lives in `server/` ([docs/server.md](docs/server.md)); its protocol is frozen (D10), so leave it alone unless asked.
- **Content is in packages** ([docs/packages.md](docs/packages.md), D53): the base game is `base/` (rules, races, one file per unit, building, item, ability and effect, asset folders, scenery, landscape, maps as `.utcm` packages). Load the matching skill before changing content: `canopy-packages`, `canopy-definitions`, `canopy-abilities`, `canopy-assets`, `canopy-maps`; `canopy-buildings` for a building's model, states and effects; `canopy-icons` for any icon or portrait (every new unit, ability, research, building or item needs one that follows it). The old TypeScript editors no longer read this data; the native editor is phase 2 ([plan](docs/plan.md)).
- **Content is input.** No code names a shipped content id. Tests define their own units, abilities and effects.
- **Test what matters.** Broad scenario tests, replay-hash determinism tests, generic content validation, and unit tests for real algorithms. No tests of incidental numbers or duplicates.

## Release notes

- **Note every player-visible change** you complete in `releases/log.json` with `just release-note --kind added|improved|fixed|knownIssues --text "Concise player-facing change"`. It is the one shared log: keep other agents' notes, don't start other changelogs, don't invent changes or test results. Internal-only work needs no note.
- **Never prepare or publish a release without the user's intent to release**; ordinary work only maintains Unreleased. Jiri releases with `/release-new-version` (skill `release-new-version`, the checklist for docs/releases.md); `/build-game` and `/run-game` build the game and start it for him. Routine releases are patches; `0.X.0` milestones need an agreed major upgrade and `--milestone`. Read [docs/releases.md](docs/releases.md) before release, publishing or updater work, and never show, commit or upload the updater's signing key.

## Engineering rules

- **Determinism** ([D4](docs/decisions.md)): simulation code uses deterministic math only, fixed iteration order, and the simulation's seeded random generator. No `HashMap` iteration, platform `f32::sin`, wall clocks or thread timing in simulation results.
- **Portability** ([D1](docs/decisions.md), [D6](docs/decisions.md)): nothing macOS-only. All file locations go through `game/src/paths.rs`. There is no CI for now ([D50](docs/decisions.md)): `just check` on the Mac is the gate.
- **WebGPU baseline** ([D2](docs/decisions.md)): no wgpu features or limits beyond WebGPU's without an agreed exception.
- **Measure**: add `tracing` spans to anything that costs time; check performance with `just profile` and Tracy, on the reference machine (base M1) when it matters.
- **Dependencies** ([structure](docs/structure.md)): crates never depend on `game/`; `canopy-render` never sees simulation types.
- `just check` (format, clippy with warnings denied, tests) passes before every commit.

## Disk

Several agents build in this checkout, and Cargo never deletes old builds: `target/` once reached 433 GB. No old builds are kept.
- `just tidy` purges per-agent cook folders (`target/cooked-*`), `target/ci`, `target/tmp` and the incremental caches, and a whole profile once it passes 40 GB. `just check` runs it first.
- Cook into `target/cooked` only. If parallel work needs its own pack, cook into `target/cooked-<name>` and delete it as soon as the work lands; `just tidy` removes any left over.
- Your scratchpad holds only what you still need. Delete raw exports (Tripo GLBs, image batches), render series and data copies once their result is in the repository, and before you end a task.

## Visual previews

Use neutral zinc for model previews and review benches: background `#18181b`, ground and panels `#27272a`, controls and borders `#52525b`. Avoid green or olive stages. Verify units at normal game zoom from the overhead camera and from directly above, in every player colour including white and dark.
