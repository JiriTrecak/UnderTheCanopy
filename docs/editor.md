# The editor (phase 2)

Design decisions for the native editor, agreed with Jiri milestone by milestone ([plan](plan.md), phase 2). Each milestone starts with its open questions; this page records what is settled.

## M1. The editor shell (in discussion)

**Settled (2026-10-16):**

- **Inside the game app.** The editor is its own crate (`crates/editor`) that the game opens as a mode: a main-menu "Editor" entry and `canopy edit`. It does nothing until opened: no threads, no loading, no effect on start-up or frames for players.
- **UI: egui with egui_dock** for the editor chrome; the game renderer draws the 3D viewport into a texture the editor shows. The game's HUD keeps its own toolkit.
- **Look: a tool look as close to Tailwind's zinc palette as egui allows** (neutral zinc surfaces, 1-pixel borders, compact controls, rounded corners, subtle hover and focus states). The old spell studio's UI is the reference to match; the other old editors are not.
- **Navigation: a Slack-style rail.** A slim icon rail on the left switches between editors (maps, units and heroes, abilities, effects, items and buildings, races and rules, assets, later campaigns) and the assistant. Each editor keeps its own tabs and state; cross-links (a hero's "add ability → new") switch editors and come back.
- **MCP: in Rust (`rmcp`), hosted by the editor.** When the editor opens it can start its MCP server (a setting, or a toolbar toggle), over Streamable HTTP on loopback (`127.0.0.1`), with Origin and Host checks and a per-install token; it stops with the editor. `canopy mcp` runs the same server headless (no window) for agent-only work. MCP may write only inside the packages open for editing; the base package only in developer mode.
- **One command set.** Every edit is a typed command; the editor's own controls, MCP clients and the built-in assistant all use it, so all of it is undoable and scriptable.
- **The assistant** (later milestone): one assistant with access to every editor and threads per topic. It calls the Claude API directly (needs an API key; not covered by a Claude Code subscription); the shell leaves room for its panel.

**Open:** which package is being edited (base in developer mode, players' maps and mods), the editing model (undo, dirty state, saving files in place with their key order), schemas from the Rust types (`schemars`), viewport foundations (shared camera and picking, live reload, GPU updates), the test loop (play the open package in-process).
