# Under the Canopy. Run `just` to list recipes.

set windows-shell := ["pwsh", "-NoLogo", "-Command"]

package_format := if os() == "macos" { "app" } else { "nsis" }

default:
    @just --list

# Run the game against the repository's data (cooks first; a no-op when up to date)
play *args: cook
    cargo run -p canopy -- --data "{{justfile_directory()}}" {{args}}

# Render one view offscreen to a PNG: `just shot --map <id> --camera <x,z,zoom[,yaw]> --out <png>` (see docs/renderer.md, "Reference captures")
shot *args: cook
    cargo run -q --release -p canopy -- shot --data "{{justfile_directory()}}" {{args}}

# GPU benchmark over fixed scenes at 1080p and 2560×1600 (per-pass submit-to-idle, draws, triangles, CPU): `just bench-render [--label before] [--scenes crowd] [--preset medium]` (docs/renderer.md §9)
bench-render *args: cook
    cargo run -q --release -p canopy -- bench-render --data "{{justfile_directory()}}" {{args}}

# Run an optimised build with Tracy profiling (connect the Tracy app)
profile *args: cook
    cargo run -p canopy --profile profiling --features profile -- --data "{{justfile_directory()}}" --no-vsync {{args}}

# Simulation cost of a real match per tick and per step: `just bench-sim` (8 × 50 crowd battle on echo-isles), or e.g. `just bench-sim --map turtle-rock --ai 4 --ticks 4000`
bench-sim *args="--map echo-isles --crowd 50 --ticks 1600":
    cargo run -q --release -p canopy-sim --example match_bench -- --data "{{justfile_directory()}}" {{args}}

# Cook published assets into target/cooked/canopy.pack; `just cook --inspect <asset id>` shows one, `--stats` the whole pack
cook *args:
    cargo run -q -p canopy-cook -- --data "{{justfile_directory()}}" --out "{{justfile_directory()}}/target/cooked" {{args}}

# Everything CI checks: formatting, lints, tests
check:
    cargo fmt --all --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace

test *args:
    cargo test --workspace {{args}}

# Build the platform package (.app on macOS, NSIS installer on Windows)
package:
    cargo packager -p canopy --release --formats {{package_format}}

# Mirror content/, assets/ and art/ from the old TypeScript repository
sync-data old="../Settlers 3 Web":
    tools/sync-data.sh "{{old}}"

# Compare every Rust reader with the old TypeScript game on the old repository's data
parity old="../Settlers 3 Web": (parity-content old) (parity-assets old) (parity-maps old)

# Content (game.json, abilities, effects) against the old TypeScript loader
parity-content old="../Settlers 3 Web":
    mkdir -p target/parity
    cd "{{old}}" && node_modules/.bin/tsx "{{justfile_directory()}}/tools/parity/dump-ts.mts" "$PWD" "{{justfile_directory()}}/target/parity/content.json"
    cargo run -q -p canopy-content --example parity -- "{{old}}" target/parity/content.json

# Published asset data against the old TypeScript game
parity-assets old="../Settlers 3 Web":
    mkdir -p target/parity
    cd "{{old}}" && node_modules/.bin/tsx "{{justfile_directory()}}/tools/parity/assets-ts.mts" "$PWD" "{{justfile_directory()}}/target/parity/assets.json"
    cargo run -q -p canopy-content --example assets_parity -- "{{old}}" target/parity/assets.json

# Every map against the old TypeScript parser
parity-maps old="../Settlers 3 Web":
    mkdir -p target/parity
    cd "{{old}}" && node_modules/.bin/tsx "{{justfile_directory()}}/tools/parity/maps-ts.mts" "$PWD" "{{justfile_directory()}}/target/parity/maps.json"
    cargo run -q --release -p canopy-content --example map_parity -- "{{old}}" target/parity/maps.json

# Which engine features the shipped data uses
content-report:
    cargo run -q -p canopy-content --example report -- "{{justfile_directory()}}"
