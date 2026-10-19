---
name: build-game
description: Builds Under the Canopy with the latest changes — the release game binary and the cooked asset pack — and reports whether it is ready to run. Use when Jiri says /build-game, "build the game", or before running or capturing the game after changes.
---

# Build the game

Builds what `./target/release/canopy` runs: the optimised game binary and the cooked asset pack
(`target/cooked/canopy.pack`). It doesn't run checks or commit anything. `just check` is the
gate before commits (AGENTS.md); this is only the build.

## Steps

From the repository root:

1. **The binary:**
   ```sh
   cargo build --release -p canopy
   ```
   On a compile error, stop and report it with the first error's message and location. Don't
   start fixing unrelated code unless asked.
2. **The assets:**
   ```sh
   cargo run --release -p canopy-cook -- --data "$PWD" --out target/cooked
   ```
   The cook redoes only changed assets, usually in seconds; a format change recooks everything
   (a minute or two). It refuses broken art, such as a floating model or a clip that carries a
   unit off its position (skill **canopy-assets**): report those errors as they are, naming the
   asset.
3. **Report** in one or two lines: built, or what failed. If the cook printed warnings, say how
   many, without listing them.

## Notes

- A game that is already running keeps working while you rebuild: it holds the pack it opened.
  It shows the new build only after it is restarted (skill **run-game** does that).
- Debug builds (`just play`) are slow; always build release for playing.
- Follow the Disk rules in AGENTS.md: don't leave extra `target/cooked-*` folders behind.
