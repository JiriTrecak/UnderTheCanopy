# Under the Canopy. Run `just` to list recipes.

set windows-shell := ["pwsh", "-NoLogo", "-Command"]

package_format := if os() == "macos" { "app" } else { "nsis" }

default:
    @just --list

# Run the game against the repository's data
play *args:
    cargo run -p canopy -- --data "{{justfile_directory()}}" {{args}}

# Run an optimised build with Tracy profiling (connect the Tracy app)
profile *args:
    cargo run -p canopy --profile profiling --features profile -- --data "{{justfile_directory()}}" --no-vsync {{args}}

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
