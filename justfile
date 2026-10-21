# Under the Canopy. Run `just` to list recipes.

set windows-shell := ["pwsh", "-NoLogo", "-Command"]

package_format := if os() == "macos" { "app" } else { "nsis" }

default:
    @just --list

# Run the game against the repository's data (cooks first; a no-op when up to date)
play *args: cook
    cargo run -p canopy -- --data "{{justfile_directory()}}" {{args}}

# Open the editor (docs/editor.md)
edit *args: cook
    cargo run -p canopy -- edit --data "{{justfile_directory()}}" {{args}}

# The editor's MCP server without a window, for agents (docs/editor.md)
mcp *args:
    cargo run -p canopy -- mcp --data "{{justfile_directory()}}" {{args}}

# Watch a replay (`just replay <file.utcr>`), or check one headless with `--verify`: plays it to the end, checks its fingerprints and outcome, times seeks (docs/ui.md, Replays)
replay *args: cook
    cargo run -q --release -p canopy -- replay --data "{{justfile_directory()}}" {{args}}

# Render one view offscreen to a PNG: `just shot --map <id> --camera <x,z,zoom[,yaw]> --out <png>` (see docs/renderer.md, "Reference captures")
shot *args: cook
    cargo run -q --release -p canopy -- shot --data "{{justfile_directory()}}" {{args}}

# GPU benchmark over fixed scenes at 1080p and 2560×1600 (per-pass submit-to-idle, draws, triangles, CPU): `just bench-render [--label before] [--scenes crowd] [--preset medium]` (docs/renderer.md §9)
bench-render *args: cook
    cargo run -q --release -p canopy -- bench-render --data "{{justfile_directory()}}" {{args}}

# Run an optimised build with Tracy profiling (connect the Tracy app)
profile *args: cook
    cargo run -p canopy --profile profiling --features profile -- --data "{{justfile_directory()}}" --no-vsync {{args}}

# Simulation cost of a real match per tick and per step: `just bench-sim` (a crowd battle of 50 a player on frozen-dream), or e.g. `just bench-sim --map frozen-dream --ai 2 --ticks 4000`
bench-sim *args="--map frozen-dream --crowd 50 --ticks 1600":
    cargo run -q --release -p canopy-sim --example match_bench -- --data "{{justfile_directory()}}" {{args}}

# Cook published assets into target/cooked/canopy.pack; `just cook --inspect <asset id>` shows one, `--stats` the whole pack
cook *args:
    cargo run -q -p canopy-cook -- --data "{{justfile_directory()}}" --out "{{justfile_directory()}}/target/cooked" {{args}}

# The gate before every commit (no CI, D50): formatting, lints, tests, the match server
check: tidy
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    if [ -d server/node_modules ]; then cd server && npm run -s check && npm test --silent; fi
    cargo test --workspace

test *args:
    cargo test --workspace {{args}}

# Per-agent cook folders, scratch targets and the compiler's incremental caches always go;
# a whole profile once it passes 40 GB of stale builds. Everything regenerates on the next
# build.
# Purge old builds (AGENTS.md, Disk); `just check` runs it first
[unix]
tidy:
    #!/bin/sh
    cd "{{justfile_directory()}}/target" 2>/dev/null || exit 0
    rm -rf cooked-* ci tmp debug/incremental release/incremental
    for profile in debug release; do
        if [ -d "$profile" ] && [ "$(du -sk "$profile" | cut -f1)" -gt 41943040 ]; then
            echo "tidy: $profile is over 40 GB of builds; cleaning it"
            rm -rf "$profile/deps" "$profile/build" "$profile/.fingerprint" "$profile/examples"
        fi
    done

[windows]
tidy:
    $t = Join-Path "{{justfile_directory()}}" "target"; if (Test-Path $t) { Get-ChildItem $t -Directory | Where-Object { $_.Name -like "cooked-*" -or $_.Name -in @("ci", "tmp") } | Remove-Item -Recurse -Force; foreach ($p in "debug", "release") { $d = Join-Path $t $p; Remove-Item (Join-Path $d "incremental") -Recurse -Force -ErrorAction SilentlyContinue; if ((Test-Path $d) -and ((Get-ChildItem $d -Recurse -File | Measure-Object Length -Sum).Sum -gt 40GB)) { "tidy: $p is over 40 GB of builds; cleaning it"; foreach ($s in "deps", "build", ".fingerprint", "examples") { Remove-Item (Join-Path $d $s) -Recurse -Force -ErrorAction SilentlyContinue } } } }

# Build the platform package (.app on macOS, NSIS installer on Windows)
package:
    cargo packager -p canopy --release --formats {{package_format}}

# Run the match server locally on 127.0.0.1:8787 (docs/server.md)
server:
    cd server && npm ci --prefer-offline --no-audit --no-fund && npm start

# Deploy server/ to the match host; `just server-deploy rollback` returns to the old server
server-deploy *args:
    server/deploy/deploy.sh {{args}}

# Which engine features the shipped data uses
content-report:
    cargo run -q -p canopy-content --example report -- "{{justfile_directory()}}"

# Releases (D43, docs/releases.md). Arguments pass through unchanged, so quoted notes and
# titles stay whole; on Windows run `cargo run -q -p canopy-release-tool -- <command> …`.

# Note a player-visible change: `just release-note --kind added|improved|fixed|knownIssues --text "…"`
[positional-arguments]
[unix]
release-note *args:
    cargo run -q -p canopy-release-tool -- note "$@"

# Validate the release log and its gallery images (`--allow-lfs-pointers` where LFS is not checked out)
[positional-arguments]
[unix]
release-check *args:
    cargo run -q -p canopy-release-tool -- check "$@"

# Freeze Unreleased into a version and set the app version: `just release-prepare 0.3.1 --title "…" [--milestone]`
[positional-arguments]
[unix]
release-prepare *args:
    cargo run -q -p canopy-release-tool -- prepare "$@"

# Package, sign, verify and stage the prepared version for this platform under build/releases/
[positional-arguments]
[unix]
release-build *args:
    cargo run -q -p canopy-release-tool -- build "$@"

# Verify a staged release and the AWS account; `--apply` uploads it and advances the feed
[positional-arguments]
[unix]
release-publish *args:
    cargo run -q -p canopy-release-tool -- publish "$@"
