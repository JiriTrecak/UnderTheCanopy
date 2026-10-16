# The editor (phase 2)

Design decisions for the native editor, agreed with Jiri milestone by milestone ([plan](plan.md), phase 2). Each milestone starts with its open questions; this page records what is settled.

## M1. The editor shell (settled; building)

**Settled (2026-10-16):**

- **Inside the game app.** The editor is its own crate (`crates/editor`) that the game opens as a mode: a main-menu "Editor" entry and `canopy edit`. It does nothing until opened: no threads, no loading, no effect on start-up or frames for players.
- **UI: egui with egui_dock** for the editor chrome; the game renderer draws the 3D viewport into a texture the editor shows. The game's HUD keeps its own toolkit.
- **Look: a tool look as close to Tailwind's zinc palette as egui allows** (neutral zinc surfaces, 1-pixel borders, compact controls, rounded corners, subtle hover and focus states). The old spell studio's UI is the reference to match; the other old editors are not.
- **Navigation: a Slack-style rail.** A slim icon rail on the left switches between editors (maps, units and heroes, abilities, effects, items and buildings, races and rules, assets, later campaigns). Each editor keeps its own tabs and state; cross-links (a hero's "add ability → new") switch editors and come back.
- **MCP: in Rust (`rmcp`), hosted by the editor.** When the editor opens it can start its MCP server (a setting, or a toolbar toggle), over Streamable HTTP on loopback (`127.0.0.1`), with Origin and Host checks and a per-install token; it stops with the editor. `canopy mcp` runs the same server headless (no window) for agent-only work. MCP may write only inside the repository's base package.
- **It edits the repository, nothing else.** The editor works on the base package of the repository checkout the game runs from (`--data`, `just edit`). A game without one (every installed copy) shows the Editor entry disabled, and `canopy edit` refuses. Players' maps and mods are not edited for now (phase 2's players' content milestone).
- **One command set.** Every edit is a typed command; the editor's own controls, MCP clients and the built-in assistant all use it, so all of it is undoable and scriptable.
- **The assistant** (later milestone): a sidebar on the right that opens from the toolbar over any editor, as in the old spell studio; one assistant with access to every editor and threads per topic. It calls the Claude API directly (needs an API key; not covered by a Claude Code subscription); the shell leaves room for its panel.

**Settled (2026-10-16): the editing model.**

- **One undo history across all editors.** Commands group into steps (a transaction: "new ability + bind it to the Marshal" is one step, across files). Undo and redo bring back the context of the step: they switch to its editor, open or focus its tab and select what it changed.
- **Edits live in memory; Cmd+S saves everything.** Changed documents show an unsaved mark; Cmd+S (and MCP's `save`) writes every changed file. Leaving the editor (Exit, closing the window) with unsaved changes asks first.
- **Files changed on disk reload.** A watcher (`notify`) follows the base package: a document without unsaved edits reloads quietly; one with unsaved edits shows the conflict and asks which side to keep.
- **One file format, written in place.** Documents keep their key order (serde_json's `preserve_order`) and are written as 2-space JSON with a trailing newline, the format nearly every document already has; a test keeps every document in it, so a save's diff is only what changed.
- **The loader checks after every step.** The full load (about 90 ms) runs after each step and keeps a problems list that links to the element and field. Invalid states are allowed while working; saving with problems warns.
- **Renames carry their references.** Renaming an id rewrites every reference in the package in the same step; deleting something still referenced is refused, with where it is used.

**Settled (2026-10-16): schemas, viewport, testing.**

- **Schemas from the Rust types** (`schemars`): the content crate's serde types produce JSON Schema. It drives the inspector's forms, MCP's tool definitions and the skills' checks. Fields that name another element say so in the types (a unit id, an effect), so forms show a searchable picker and renames find every reference.
- **Generic forms, custom panels where they pay.** Every element gets a form from its schema; hand-built panels sit on top where a form is not enough (the ability timeline and encounter stage, the map tools).
- **One viewport widget.** The game renderer draws a scene into a texture sized to the panel; camera (orbit, top-down, the game's), picking and an overlay for gizmos are shared by every editor that shows a scene. One renderer serves whichever viewport shows. Each step updates the running preview in place. Previews stand on a zinc stage, with a switch to a biome's terrain.
- **Test in game runs what is open, unsaved edits included**, in-process: the game's data built from the editor's documents, a skirmish on the open map (or the last one), Esc back to the editor as it was, with the match tools (speed, reveal) and testing cheats. Editors may need their own loops beyond this (the map editor, the ability stage): decided with them.

**Build order:** (1) documents and commands: key order kept, the global undo with its context, Cmd+S, asking before leaving, the file-format check; (2) the loader's check after every step and the problems list; (3) the file watcher; (4) schemas, generic forms with reference pickers, renames; (5) MCP over the commands; (6) the viewport; (7) Test in game.

**Built (2026-10-16): the shell, read-only.** `crates/editor`, opened from the main menu's Editor entry, `canopy edit` or `just edit`. The rail switches seven editors; each lists its collections from the base package with the loader's own walk, opens documents as egui_dock tabs (read-only JSON for now) and shows the open one in its inspector. Undo, save, MCP, Test in game and the assistant are there as disabled controls. The theme is `crates/editor/src/theme.rs`; icons are drawn strokes (`icons.rs`), no icon font. It uses the game's interface font (Outfit). `canopy capture-screens --only editor` renders it offscreen in a few states.

**Built (2026-10-16): step 1, documents and commands** (`crates/editor/src/document.rs`). Edits are `Command`s (set, insert, remove, create, delete) on JSON pointers, grouped into steps; one history across editors, and undo or redo switches to the step's editor and tab. Removing a field and undoing puts it back in its place, so files keep their key order (D55). Cmd+S writes every changed document; the tab, the library row and the header mark unsaved ones; Exit and closing the window ask first. For now the inspector edits an element's name and the tab edits its JSON (Apply); forms come with step 4. A test keeps every base document in the file format.

**Built (2026-10-16): step 2, the check after every step** (`checker.rs`). A package can carry an overlay of documents that stand in for its files while it loads (`Package::with_overlay`); the editor lays its unsaved documents over the base package and runs the full load on a worker thread after each step (about 45 ms). The status bar shows the outcome; its list links each problem to its document. Saving with problems asks first.

**Built (2026-10-16): step 3, files changed on disk** (`watch.rs`, `notify`). A document without unsaved edits follows its file (its undo steps are dropped, as they were made against a version that is gone); one with unsaved edits becomes a conflict, and the editor asks which version to keep. Libraries are read again and the check reruns when files change; a file that is not valid JSON is reported and left alone.

**Built (2026-10-16): step 4, schemas, forms, references** (`content/src/schema.rs`, `forms.rs`, `references.rs`, `tabs.rs`, `inspector.rs`). Every content type derives `schemars::JsonSchema` (hand-serialized types have hand-written schemas; ids are `format: canopy-id`); `schema::of_document` shapes them like the files. A tab shows its document as a form (or its JSON): fields in schema order, enums and tagged variants as menus, ids with a searchable picker, add and remove for optional fields, list entries and map keys, numbers that fold a drag into one undo step; documents without a schema (behaviour sets, biomes, maps for now) get a form from their values. Undo marks the field it changed. The inspector lists where the element's id is used; Rename id rewrites every use (values and keys, in place) in one step; Delete is refused while anything uses it; the library's + creates an element from its schema's required fields.

## M3. The map editor (decided 2026-10-16)

**Settled (Jiri, ahead of the build):**

- **Brushes that change the terrain directly**, as in Warcraft III's editor: raise, lower, flatten, smooth, cliffs, paint. No editable landform objects for now. Procedural or stored objects ("paint a layer called island and get a good island") come later, once everything can be edited by hand.
- **The finish line:** with these manual tools we can build Echo Isles, and M3 ends with a good, solid, playable 1v1 map authored in the editor with everything a map needs: creep camps, forests, resources.
- **No symmetry tools** for now.
- **Start locations** place the player's main hall location and its amber resources, properly oriented. Nothing more by rules yet.
- **No new ground cover** (moving grass, decals, foliage) for now.
- **Mission scripting is Lua.**

## M4. The asset editor (direction 2026-10-16)

**Settled (Jiri):** the asset editor is a tool for us, mostly for agents: what makes new assets quick to create and to author well into the game. Its scope is mine to decide from the asset format (not overdone). **Players never bring their own models, icons or other assets**: player packages use the game's pool only. Built right after M3.
