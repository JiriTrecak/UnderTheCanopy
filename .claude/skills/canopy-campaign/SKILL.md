---
name: canopy-campaign
description: How Under the Canopy's campaigns are built and continued: the campaign package and its steps, story and art, mission maps authored by scripts through the editor's MCP, mission players and waves, neutral places and secrets, and the headless playthroughs that prove a mission is winnable. Use when designing, authoring, testing or polishing a campaign mission, story page or campaign step.
---

# Campaigns

Read first:
- **docs/campaign.md:** the design (D85), formats, the runtime, the campaign AI and the scene API.
- **docs/story.md:** the story, the characters, the twelve-step outline per race, how missions are built, and each mission's design with "Built" notes.

Load skill **canopy-scenes** for cutscenes; **canopy-maps** for the map format; **canopy-definitions**, **canopy-assets** and **canopy-icons** for new units, props and portraits.

Design rule (AGENTS.md): a new mission starts as a design in docs/story.md, agreed with Jiri, before any map is built. Names must be memorable (Arthas, Sylvanas); "Myrrh" was rejected.

## Where things are

| What | Where |
|---|---|
| The ant campaign | `base/campaigns/vanguard.utcc/` (`campaign.json`, `package.json`, `missions/<id>.utcm`) |
| Its art | `base/assets/interface/campaign/vanguard/<name>/` (background, chapter cards, story pages, mission loading art) |
| Map generators | `tools/maps/<mission>.py` (and `_dress.py`, `_units.py` splits); script sources `tools/maps/<mission>.lua` where kept |
| Autoplay | each mission's `autoplay.lua`; `crates/sim/src/autoplay/`; runner `crates/sim/examples/autoplay.rs` (`just autoplay`, `just campaign-test`) |
| Probes | `crates/sim/examples/mission_walk.rs`, `crates/sim/examples/mission_playtest.rs` |
| Campaign AI | `crates/sim/src/ai/campaign/` |
| Mission API (Lua) | `crates/sim/src/mission/` (`api.rs`, `api/scene.rs`, `cinema.rs`) |
| The biome | `base/landscape/biomes/underleaf.json`, scenery `base/scenery/underleaf/`, giants `base/scenery/giants/` |
| Editor campaign tools | `crates/editor/src/campaign/`; MCP `campaign_steps`, `add_campaign_step`, `set_campaign_step`, `set_mission_tech`, `mission_tech_tree`, `set_mission_players` |

## campaign.json

A linear list of steps:
- `chapter`: title, art;
- `story`: title, pages with art and text;
- `mission`: map, title, chapterLine, art, story (the loading screen), heroLevelCap, tech `{new, blocked}`;
- `cinematic`: a placeholder card for now.

Each step has a stable `id`, and progress records the furthest step by id. Tech is cumulative: each mission lists only what becomes `new` (or `blocked`); everything not yet new is locked. The Tech view (editor, U) shows the race's tree.

Rules of the flow:
- Straight through: a won mission loads the next step at once.
- The campaign screen shows only the steps reached.
- Heroes carry their level, abilities and items to the next mission.

## Story and art

Story pages are 2–3 short pages per step, in the voice of docs/story.md. Each page has a painted illustration whose lower third is calm, so text can sit over it.

Generating art:
1. Write a request (`slug`, `size` "2048x1152", `n`, `quality` "high", `prompt`). Reuse the style tail of an existing prompt in the old repository's `.asset-work/concepts/buildings/story-*.prompt.json`: Warcraft III Reforged key art, a forest floor at insect scale, substance glowing blue to violet and never green, lanterns amber, no text.
2. Run `tools/buildings/concept.sh <request.json>` (gpt-image-2 through the old repository's provider; never print its key). Several requests can run in parallel with `xargs -P 2`.
3. Convert with `sh tools/campaign/art.sh vanguard <slug>:<name> …`, which writes `asset.interface.campaign.vanguard.<name>` as 1920-wide webp.
4. Look at every image before using it.

Aurelith in game is crimson and gold, with team colour on her gown. Player 1 is blue (`config/players.json`), so in the campaign her gown is blue, close to the royal-blue mantle of the story art.

## Mission maps

Missions live in the campaign package. A new map is blocked out by a Python generator talking to the editor's MCP:
1. Start the server: `CANOPY_MCP_PORT=<free port> just mcp` (headless). Several checkouts each use their own port, and `tools/maps/mcp.py` reads the same variable. Stop it when done.
2. Run the generator. It shapes blocks (`map_blocks`), places scenery and entities, writes the `mission` section (players, regions, objectives, cameras) and copies the Lua.

**Once a map has shipped, the map package is the only source of truth.** Edit it through the editor, its MCP tools, or targeted edits of map.json, scenery.json and mission.lua (2-space JSON, keys in place). Don't mirror changes back into the generator, and never re-run a generator over a shipped map: it would undo everyone's later edits. The generators in `tools/maps/` remain as a record of how each map was blocked out. (The first three missions were built before this rule, so their generators partly mirror later edits; don't rely on that.)

A mission map has:
- **A clear main road** and its objective.
- **Side places**, each worth the detour: caches, a camp with a drop, a spring, a merchant, a side quest.
- **One or two secrets**, hidden by the map's shape: a gap in a tree line, a ledge, a tunnel behind rubble.
- **One silly easter egg** (the tree frog, the sleeping Alate, the snoring engineer).
- **A frame of giant forest pieces** that sells the scale, while the playable ground stays clean.

Neutral features (D87): `building.neutral.amber-cache`, `supply-crate`, `seed-pod`, `rubble` (opens a way), `sap-spring`, `dew-spring`, `merchant`. Place them with `initialState.inventory`, `lootPool` or `wares`. Items can lie on the ground as entities whose definition is the item.

## Mission players

`mission.players[]`: `{player, race, team: enemy|ally, vision, color?, ai}`.
- **ai.behaviour:** `passive` (holds its ground), `base` (mans and rebuilds), `waves` (attack lines: `after`, `every`, units, `grow`, `from: trained` or arriving at a region, `target: base | {region}`), or the held-back `skirmish`.
- **ai options:** `dormant` (wakes with `ai_wake`), `rebuild`. The mission's AI players get free resources.
- **color:** a player-colour name or `#rrggbb`; the feral brood is `#8a7f96`. Player 1 is blue and slot 2 red by default.
- **Script control:** `ai_waves(p, on)`, `ai_attack_now`, `ai_target`, `ai_behaviour`, `ai_wake`.

## Proving a mission

Every change to a mission ends with its autoplay plan winning headless (docs/campaign.md "Autoplay", D93). Each mission map carries `autoplay.lua` beside `mission.lua`: a small plan that steers player 1 to victory with a player's commands (its API is in docs/campaign.md). Add only that file when you write one; it is outside the map's identity.
- **`just autoplay <mission>`** plays one mission as its campaign starts it and prints the plan's steps, objectives, refused commands, time and outcome. `--talk` adds the mission's lines, `--seed N`, `--plan <file>` tries another plan on the map without touching it, `--verify` replays the commands alone. `AUTOPLAY_COMMANDS=1` prints every command, `AUTOPLAY_WATCH=queen` a unit's order, route and tunnel trip every 10 s.
- **`just campaign-test`** plays every shipped plan with `--verify` (about 30 s) and fails unless all win. Run it after changing a mission, its waves or the simulation. Check new plans on several seeds (`--seed 1` to `8`): base missions run on the skirmish AI's economy and vary.
- **Writing a plan.** Missions without a base: `hero{}` with its learn order, heal and shield, then `go`, `attack`, `pickup()` through the side places in order. Base missions: `economy(true)`, then `mass(n)` before every assault and `push`/`attack` on the targets; `keep("queen", x, z)` holds her out of an assault the mission would be lost with. Visit the side places that make the difficulty work and click the easter egg once. The Queen (radius 3) does not fit narrow ways (Founding's sap spring, nook, mushrooms and waterfall cache; the Glass Road glade and the old tunnel's mouth): don't route her there.
- **As of 2026-10-22**: The Hollow Brood wins in about 10 minutes (every objective, the caches, frog, webs, burrow, wreck, survivors); Founding in 17–18 minutes on every seed tried (Alate clicked); The Glass Road on 10 of 12 seeds in 22–35 minutes (outposts, engineer, the gate with 30, the Queen home during the assault). Its losses are the economy collapsing under the waves or the Queen falling at the camp; the old tunnel cannot carry an army (a tunnel trip to an unreachable mouth waits forever).
- `mission_walk` and `mission_playtest` (`crates/sim/examples`) remain for probes: `--reach "from:x,y;name:x,y"` (ground reachability only), `--human` (an idle colony against the waves), `PLAYTEST_AI=1` (the skirmish AI's decisions).
- Don't run `ai_duels`.

## Checking a mission

`just check` runs the mission check over every shipped mission (`crates/sim/tests/mission_shipped.rs`, about 1.5 s; docs/campaign.md "Checking a mission"; `canopy_sim::mission::check`). It catches the pitfalls below without playing, by the simulation's own navigation:
- **Stranded units:** placed units and scripted spawns that cannot start a route where they stand, or reach under 300 cells. The finding names the nearest ground the body fits on.
- **Wave regions** whose centre lies in a building or on closed ground.
- **Routes:** player 1 into every region (its smallest walker), each wave line from its sources to its target (its largest unit), with the real pathfinder, and the narrowest width of each. A region player 1's large units (radius 3) cannot enter is flagged, not failed.
- **Corridor spawns:** a scripted neutral building where the corridor is under 13 cells.
- **A dry run of every scene,** skipped with every scripted placement spawned: no script error, cinematic mode off at its end, nobody left where it cannot step.

Read the routes, widths and flags with `cargo test -p canopy-sim --test mission_shipped -- --nocapture`. Fix a finding in the map package (map.json, scenery.json, mission.lua, or the blocks for terrain), then prove the mission again.

## Pitfalls that already cost a day

- **Scenery blockers on a road** (a corpse prop, a crate) can cut it for the pathfinder even when `--reach` says the ground connects. Debris units walk past has no blocker.
- **Neutral buildings are not attack-move targets.** A crate spawned on a narrow causeway walls it shut.
- **A region whose centre lies inside a building's footprint** gives waves no route. Keep wave regions on open ground.
- **Units placed on blocked cells never move**, and scenes then wait on them. After placing anything, walk it.
- **Scene effect and light keys** take letters, digits and `_` only: `condenser-hum` crashed the closing and the win never fired.
- **Cinematic mode holds the battle:** only the units the scene directs act. A scene's walk is given up after twice its straight time plus three seconds, so a blocked walk doesn't hang the scene but the actor stays where it stood. Frame shots on where actors really stand (see canopy-scenes).
- **The skirmish AI is a weak stand-in for a player.** If it stalls, read its decisions (`PLAYTEST_AI=1`) before blaming the mission.

## Finishing

- Add a `just release-note` for every player-visible change.
- Add a "Built" or "As built" note to the mission in docs/story.md.
- `just check` passes; read its exit status before committing. Commit only your own files.
- Delete scratch maps, raw image batches and render series.
