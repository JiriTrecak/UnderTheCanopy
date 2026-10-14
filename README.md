# Under the Canopy

A real-time strategy game on the forest floor, in Rust.

```bash
just play       # run the game (cooks assets first)
just cook       # cook assets into target/cooked/canopy.pack
just check      # format, lint, test
just package    # build the .app (macOS) or installer (Windows)
```

Needs Rust (pinned in `rust-toolchain.toml`), [just](https://github.com/casey/just), Git LFS for `art/` and `assets/`, and `cargo install cargo-packager` for packaging.

Start with [the plan](docs/plan.md), [the structure](docs/structure.md) and [the decisions](docs/decisions.md).
