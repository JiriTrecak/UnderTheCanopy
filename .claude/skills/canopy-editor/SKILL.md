---
name: canopy-editor
description: How Under the Canopy's editor works and how to drive every workbench of it through its tools — the MCP server for agents and the editor's own assistant — documents and undo steps, the check, saving, the text library, maps in blocks, campaigns, the ability stage, the Combat Lab and assets, with the tools' arguments, recipes and pitfalls. Use before editing content through the editor's MCP tools or as the editor's assistant.
---

# The editor and its tools

The editor (`crates/editor`, docs/editor.md) is a mode of the game for authoring the base package. Every change is a typed command grouped into undo steps in one history across all its editors. Its controls, the MCP server and the built-in assistant all use the same commands, so a change from any of them:
- shows at once;
- undoes like the user's own;
- stays in memory until it is saved.

The content formats are in the other skills. Read the matching one before changing that kind of content:
- **canopy-definitions** for units, buildings, items, races and rules;
- **canopy-abilities** for abilities and effects;
- **canopy-maps** for maps, and **canopy-campaign** for missions;
- **canopy-assets** for art, **canopy-icons** for icons, **canopy-sounds** for sounds.

## Starting it

- **The editor:** `just edit` opens it in a window, or use the main menu's Editor entry.
  - Its header's **MCP** chip starts the server, and can make it start with the editor.
  - The robot icon opens the **assistant**.
- **Headless:** `CANOPY_MCP_PORT=<free port> just mcp` serves the same tools without a window. The port is 7431 by default. Several checkouts each use their own port.
  - It prints the `claude mcp add …` command, which carries the token (the user folder's `editor-mcp-token`). Keep the token private.
  - Its edits live in its memory: **save before stopping it**, or they are gone.

## The workbenches (the rail)

| Editor | What it holds | Its own view |
|---|---|---|
| Maps | `maps/<group>/<id>.utcm`; campaigns' mission maps under each campaign | The block editor (D80): blocks, the generated world, split, pathing, a mission's Tech graph, the map panel (meta, size, visuals, players) |
| Campaigns | `campaigns/<id>.utcc` | The steps list (chapters, stories, missions, cinematics) and its panel |
| Units | `units/**` (heroes under `heroes/`) | Overview: stat card, a hero's levels, bound abilities with hotkeys, trained by |
| Abilities | `abilities/**` | Overview (ranks, the cast to scale, targeting, bound by) and **Stage** (the real simulation casting it) |
| Effects | `effects/**` | Overview: layers on a timeline |
| Buildings | `buildings/**`, `items/**`, `loot-pools/**` | Overview: trains, researches, cost, footprint; an item's stats |
| Races | `races/`, `research/`, `resources/`, `behaviors/`, `rules/` | A race's roster and starting setup |
| Assets | `assets/**/asset.json`, `sounds/`, `scenery/`, `landscape/biomes/`, `landscape/water/` | An asset's files, clips and users; Import…, Cook, unused assets |
| Lab | `labs/*.lab.json` beside the package (documents under `@labs/`) | The Combat Lab: a grid, units, orders, the run, tuning, measures and debug layers |

Every document also has **Form** (from its schema) and **JSON** (its text) in the tab bar.

## Documents and commands

- **Paths** go from the package root, as `list_elements` gives them: `units/ants/warrior.json`, `maps/skirmish/frozen-dream.utcm/map.json`, `campaigns/vanguard.utcc/campaign.json`. Lab scenarios are `@labs/<id>.lab.json`.
- **`edit`** takes `label` and `commands`, applied in order as **one undo step**: all of them happen or none does. Each command:
  - `{"op":"set","document":…,"pointer":"/body/maxHp","value":204}` adds or replaces a field or an array entry.
  - `{"op":"insert","document":…,"pointer":"/behaviorSets","index":1,"value":…}` inserts into the **array** at `pointer`. `index` is required; the array's length appends.
  - `{"op":"remove","document":…,"pointer":…}` removes a field or an array entry.
  - `{"op":"create","document":…,"value":{…}}` makes a new document. `{"op":"delete","document":…}` deletes one, and its file goes on save.
  - Terrain grids have `grid` and `patch` commands. Use the map tools instead.
- **Pointers** are JSON pointers: `~1` stands for `/` and `~0` for `~`. A text key such as `unit.ants.warrior.name` needs no escaping.
- **`problems`** waits for the loader's check of the latest edits, which loads the whole package in about 50–90 ms. Call it after every change, and fix what you broke.
- **`save`** writes every changed document in the file format: 2-space JSON, key order kept, a trailing newline.
  - It is refused while the check finds problems, unless `force` is set.
  - Never save unless the user asked.
- **`undo` and `redo`** take back or repeat the last step, whoever made it.
- **`find_uses`** answers before you remove an id. **`rename_id`** rewrites every use, text library keys included, in one step. It refuses an id that already exists.
- **`read_document`** includes unsaved edits. **`document_schema`** gives a document's shape: fields with `format: canopy-id` name another element, and `null` means the kind has no schema, such as text files.
- **`open`** shows a document to the user in its editor.

## The text library (D96)

- **Names and descriptions** live in `text/en/<area>.json`, not in the definitions. The areas are units, buildings, items, abilities, research and races.
  - The keys are `<id>.name` and `<id>.description` (`unit.ants.warrior.name`).
  - A definition with a `name` field is a problem: "text lives in the text library".
- **Campaign text** (D96, skill canopy-campaign) is also by key.
  - Steps' titles are keys such as `campaign.vanguard.prologue.title`, in the campaign's `text/en/campaign.json`.
  - Mission lines are `line.<mission>.<words>`, in the mission map's `text/en/mission.json`.
  - `campaign_steps` returns each title's key and its words (`titleText`).

## Maps (blocks, D80)

- **`create_map`** takes `{id, name, size, biome, folder}`. A map is a multiple of 32 cells from 128 to 1024.
- **`map_blocks`** edits the blocks over a box (`from` and `to`), along a line (`points` with `radius`), or inside an outline (`points` with `fill`). Coordinates are **world cells `[x, z]`**, and a block is 4 × 4 cells.
  - **Shaping:** `raise`, `lower`, `level` (with `level`), `paint` (`kind`), `face` and `unface` (`style`).
  - **Ramps:** `ramp` and `unramp` go from the top (`from`) to the foot (`to`).
  - **Cuts:** `cut` and `uncut` work on the block at `to`; `corner` picks which corner.
  - **Corners:** `round` and `unround`.
  - **Cells:** `cells`, `uncells` and `fill` paint kinds cell by cell.
  - **`flat`** starts the map again.
  - **Symmetry:** `symmetry` (`point`, `x`, `z` or `off`) applies to this edit and later ones.
- **`map_place`** places an `amber` site, a `camp` (`tier`: easy, medium or hard) or the next player's `start`, as the editor's clicks do. **`map_remove`** removes what lies within a radius of a point.
- **Looking at a map:**
  - `map_info` gives the layout and the check's problems for this map;
  - `map_picture` shows it from straight above;
  - `map_preview` writes the map picker's picture, and asks first.
- **Other placements** (units, buildings, scenery, items, cameras, a mission's regions) are edits of `map.json` and `scenery.json`. Read the map first, and follow skill canopy-maps.

## Campaigns (D85)

- **`create_campaign`** takes `{id, name, race}`. **`campaign_steps`** lists the steps, each mission's map and tech, its problems, and the `missionMaps`.
- **Editing steps:**
  - `add_campaign_step`: `{campaign, kind, title, id?, at?, map?}`. A new mission gets its own flat map in `missions/`.
  - `move_campaign_step`: `{campaign, step, to}`.
  - `remove_campaign_step`: `{campaign, step}`.
  - `set_campaign_step`: `{campaign, step, fields: {…}}`, where `null` removes a field.
  - A `step` is its id or its index.
- **Tech:**
  - `set_mission_tech`: `{campaign, step, set: {"unit.x": "new" | "blocked" | "locked" | "available"}}`.
  - `mission_tech_tree`: `{campaign, step}` gives the Tech view as data.
- **Players:** `set_mission_players` takes `{map, players}` (the mission map's id or package path) or `{campaign, step, players}`. It replaces the mission's computer players.
- **Before a mission is done,** prove it with skill canopy-campaign (autoplay, the mission check). That needs a shell; the editor cannot run it.

## Trying things

- **`encounter`** casts an ability with the real simulation, on a flat sandbox, as the Stage does.
  - Arguments: `ability`, plus `preset` (enemy-single, enemy-group, ally-single, ally-group, fallen-allies, neutral-camp, empty, self) or individual settings.
  - It returns the events, what the simulation refused, the timeline, and each unit's health and mana at the end.
- **`lab_run`** runs a Combat Lab scenario to its end, or for `ticks`, with `approach` (spots, direct or ring) and `lead`.
  - It returns the measures per group: arrivals, held up, flips, damage, the first swing, the most in reach, and the overlap.
  - Scenarios are documents: change one with `edit` on `@labs/<id>.lab.json`.
  - Its format is `canopy_content::lab`: size, duration, closed spans, units with groups, and orders by tick.
- **Test in game,** and Play on a campaign's step, are the user's buttons. Suggest them when a change wants playing.

## Assets and sounds

- **`import_asset`** copies a `.glb` or `.png` in as a new asset folder, written at once. **`cook`** brings changed assets into the pack. Both ask first.
- **`unused_assets`** lists assets nothing names. Delete one with `edit` (`delete` its `asset.json`): the folder goes on save.
- **Sounds:**
  - `sound_events` lists the sound events.
  - `generate_sound`, `pick_sound` and `keep_voice` use ElevenLabs and cost credits: generate only what was asked for (skill canopy-sounds).

## Reading the repository

- **`read_skill`** lists the skills; `read_skill {name}` gives a skill's text and its files.
- **`read_doc {path}`** reads `AGENTS.md`, `docs/*.md` or a skill's file.
  - Agreed decisions in `docs/decisions.md` (D-numbers) are binding.
  - Decide nothing that players would see or feel (looks, balance, camera) without the user: bring options.

## The assistant

The sidebar on the right of every editor: Claude (Anthropic, the default) or OpenAI's GPT, chosen in its Settings and called with the user's key for that provider, working through these same tools (docs/editor.md, The assistant).
- **Context:** each message carries what the user is looking at, in an `<editor-context>` block (the editor, the open document, unsaved documents, problems).
- **Approval:** `save`, `cook`, `import_asset`, `import_starcraft` (with a source), `map_preview` and the sound tools wait for the user's Allow. Everything else runs as undo steps.
- **Threads** are kept in the user folder's `assistant/threads/`, in one form either provider can carry on. The provider, the model for each, the effort and each provider's key (`ANTHROPIC_API_KEY` / `OPENAI_API_KEY`, or one entered there) are in the Settings.

## Recipes

- **Tweak a unit:**
  1. `read_document` it.
  2. `edit` the field, with a clear label.
  3. `problems`.
  4. Say what changed.
  5. Run `encounter` or `lab_run` if fighting changes.
- **A new unit:** follow canopy-definitions and canopy-icons.
  1. `create` its definition.
  2. Set its text in `text/en/units.json`.
  3. Add it to its race's `roster` and to what trains it.
  4. `problems`.
- **A map:**
  1. `create_map`.
  2. `map_blocks` for the plateaus and ramps.
  3. `map_place` the starts, amber and camps.
  4. `map_info` and `map_picture` to check it.
  5. `problems`: the starts must be playable and every site reachable.
- **A mission:**
  1. `add_campaign_step` with `kind: mission`.
  2. Shape its map.
  3. `set_mission_tech`.
  4. `set_mission_players`.
  5. Write the script, following canopy-campaign.
  6. `problems`.
- **Movement:** `lab_run` the scenario that shows it, with each `approach`, and compare `stuckTicks`, `flips` and `mostInReach`.

## Pitfalls

- **Large answers:** `list_elements` with no editor lists everything, which is large. Name the editor (maps, campaigns, units, abilities, effects, buildings, races, assets, lab).
- **Map geometry:** placements snap as the simulation requires. Buildings, starts and amber go on the 4-cell building lattice, units on cell centres. A placement where it may not stand is refused, with the reason.
- **Failed steps:** a step that fails changes nothing (`edit` is all or nothing). Read the error, fix the command, and send it again.
- **Lost edits:** edits are lost if a headless server stops unsaved.
- **Files written at once:** imports and map previews write files immediately, outside the undo history.
