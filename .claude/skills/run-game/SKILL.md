---
name: run-game
description: Builds Under the Canopy with the latest changes and starts it for Jiri to play, optionally straight into a match (/run-game frozen-dream beetles,ants). Use when Jiri says /run-game, "run the game", "let me play" or "start the game".
---

# Run the game

Starts the game with everything that's in the working tree now, in a place Jiri can see and
use.

## Arguments

Free text after the command picks what to start:
- **Nothing:** the main menu.
- **A map id** (`frozen-dream`, …, the folder names under
  `base/maps/*/<id>.utcm`): a skirmish on that map, Jiri against computer players.
- **Races** (`beetles,ants`): the first is Jiri's race, the second the computer's.
- **"watch" or "spectate":** computer players only, Jiri watching (`--spectate` in place of
  `--map`).

So `/run-game frozen-dream beetles,ants` runs:
`./target/release/canopy --data "$PWD" --map frozen-dream --races beetles,ants`.

## Steps

1. **Build** with skill **build-game** (the release binary, then the cook). If it fails, don't
   start the game: report the error.
2. **Start it** from the repository root, so Jiri sees and controls it, and so it keeps running
   after this turn:
   - In the Claude desktop app, prefer the Terminal panel tools (`run_in_terminal`, a new tab
     named "Game") with the command below.
   - Otherwise run it with Bash `run_in_background`.
   - Never run the game in the foreground: that blocks the session until it quits.
   ```sh
   ./target/release/canopy --data "$PWD" [--map <id> | --spectate <id>] [--races <a,b>]
   ```
   A game that is already running can stay open: each copy reads its own pack. Say that the
   older window shows the older build.
3. **Confirm it started:** after a few seconds, read its output for `match started` (or
   `game data loaded` for the menu), and report any `ERROR` or panic line.
4. **Report** in one line what started, and the command, so Jiri can rerun it himself.

## Notes

- The log prints frame and simulation timings every two seconds; they aren't errors.
  "late_frames" climbing while the tick stands still means the match is paused.
- A gap of a minute or more in the log while the tick keeps advancing means the window was in
  the background.
