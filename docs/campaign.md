# Campaigns

Agreed with Jiri, 2026-10-21 (D85). The mockups are the canvas [Campaign Mockups](https://claude.ai/artifact/CbzEBp3GQZB5wqcrxA6aBT). Built so far: the campaign runtime (the format, progress, tech, carried heroes, the screens and the straight-through flow; see "The runtime" below). The campaign AI is built too (below). Not yet: the cinematic player and the editor's Campaigns view.

Campaigns are linear, Warcraft III style, with heroes as the stars of the show. About twelve missions per race are planned. The research behind the design (how Warcraft III and StarCraft II build, script and present campaigns) is summarised at the end.

## The campaign package

A campaign is a `.utcc` package (D53) holding everything it needs:

```
vanguard.utcc/
  package.json       id, name, version, depends on base
  campaign.json      the campaign: identity, background, race, the steps
  missions/          its maps: missions, later cinematic scenes (each a normal .utcm)
  units/ abilities/ items/ …   campaign-only content and overrides of base, same files as base
  assets/            loading art, story art, the campaign's background, music, later voice
```

`campaign.json` holds the name, the race the player plays, the background image of the campaign screen (later perhaps an animated 3D scene), the card art for choosing a campaign, and **one linear list of steps**. The order of missions lives here and nowhere else: maps no longer name their campaign, their order or the next mission.

## Steps

| Step | In the game | Holds |
|---|---|---|
| **Chapter** | A title card | Title, art, music |
| **Story** | Full-screen pages of art and text, Continue | Pages (one image and some text each), optional narration and music |
| **Mission** | Its loading screen, then the playable map | The map in `missions/`; the loading art, chapter line, title, story text; heroes arriving and leaving; tech; hero level cap |
| **Cinematic** | For now a placeholder card ("cinematic here later"), then on | Title and a note of what it will show; later its scene and the cinematic player |

There is no branching, no side missions, no continuing into another campaign and no difficulty yet. A mission script may set campaign variables that later missions read, which changes what happens inside them but never the order.

## Playing a campaign

- The campaign plays straight through. Winning a mission saves the progress and loads the next step at once: a mission's loading screen, a cinematic playing, a story's pages, a chapter's card. Losing offers Retry or going back to the campaign screen.
- **The campaign screen** is where a campaign is started, resumed (Continue) and replayed. Over the campaign's background it lists only the steps already reached, grouped under their chapters, each with what fits it (Play, Watch, Read), and the next step highlighted. Nothing ahead of it shows: not its titles, not a count.
- **The mission loading screen** is designed anew: the mission's illustration full-bleed, the chapter line, title and a few paragraphs of story on it, one gold loading bar and nothing else (no tips, stage or file counts). When loading is done the bar gives way to "Click or press any key to begin" and the screen waits. Skirmish and multiplayer keep starting at once.
- **A mission map needs no player starts.** A player with a start gets its race's base and starting units as in a skirmish (player 1's race is the campaign's); a player without one begins with only the units and buildings the map places for it. In a mission, losing every building ends nothing by itself: the script decides with `mission.win()` / `mission.lose()`.

## What carries over

- **Heroes**, with their level, experience, learned abilities, items and permanent bonuses (the old game's company). Every hero of the player is saved when a mission is won; the map places each hero as usual and the campaign swaps in the carried hero's progress, so a map stays playable on its own (in the editor, or on a fresh start).
- **Campaign variables** that mission scripts set and read.

## Tech

What the player may build, train and research in a mission is the campaign's race's tech, step by step:

- **New**: unlocked from this mission on, for the rest of the campaign. Unlocks accumulate down the steps.
- **Blocked**: taken away in this mission only (a mission without fliers).
- **Available**: unlocked by an earlier mission. **Locked**: not unlocked yet.

A unit the player only uses for one mission is placed on the map for them (placed units need no tech), or allowed by the mission's script (`mission.allow` / `mission.forbid`, which also unlock things mid-mission).

## Campaign AI

The skirmish AI is far too strong for a campaign. Campaign opponents are a separate, simpler AI, set per computer player in each mission, running inside the simulation like the skirmish AI (deterministic, in replays and saves):

| Behaviour | What it does |
|---|---|
| **Passive** | Holds its ground; nothing is trained or rebuilt |
| **Base** | Keeps its defenders at full strength and rebuilds its buildings, never attacks |
| **Waves** | A base that also sends scripted attack waves |
| **Skirmish** | The skirmish AI, held back: the mission's tech, an army cap, a slower pace, an income handicap |

- **Waves** are data: the first at a time, then every so often, or only once a script starts them; a list of units that can grow every few waves and change later in the mission; the hero joins while alive. By default a wave is **trained at the AI's own buildings**, so destroying its production stops the pressure; a wave can instead **arrive at a region** (reinforcements, an army marching in from the map's edge). A wave goes to the player's main base, the nearest of the player's units, or a region or unit the script names.
- **Nothing costs these AIs resources**: waves come on their timer, defenders and buildings are rebuilt free. Only the Skirmish behaviour mines and pays, as the skirmish AI does.
- **Allies**: every computer player has a team, so any behaviour serves the player's side as well as the enemy's. An allied Base holds its ground, allied Waves march on the enemy (the nearest enemy base by default), a dormant ally arrives to the rescue. Allies share their vision with the player by default (set per player), and scripts can hand an ally's units to the player (`mission.transfer`) or have them follow a hero.
- **Dormant players**: a computer player can start a mission with nothing and be brought in by the script, arriving at a region with an army and then playing its behaviour: allies coming to the rescue, or a new enemy.
- **Scripts** pause and resume waves, send the next wave now, change the target, wake dormant players and switch a player's behaviour.
- The wave format leaves room for numbers per difficulty, for when difficulty comes.

### How the campaign AI plays (built)

The simulation's `ai::campaign` (`crates/sim/src/ai/campaign/`) plays every player a mission lists in `mission.players`, deterministically inside the simulation, right after the mission script each tick; its memory is saved with the state (`campaignAi`).

- **The match's players are the map's.** A mission whose map lists players seats player 1 (as the caller set it up) and exactly those players: enemies share one team, allies join player 1's; none needs a player start or a starting setup, their units and buildings are what the map places for them, and player 1 needs no start either (`canopy_sim::ai::campaign::mission_setup`, applied by `World::new`). Maps whose mission lists no players keep the setup they are given.
- **Posts.** Every unit and building the map places for a player (also script-spawned ones, and units handed to it) is a post: units guard where they stand, buildings stand where they stood. A defender walks back home when idle and more than 3 cells away, breaks off once drawn more than 18 cells away, and joins a neighbour fighting within 10 cells. A script's move of a defender moves its home there; units on other script orders (follow, attack, patrol) are left alone.
- **Base** (and Waves): a lost defender is trained again at the player's own building that trains it (its training time, nothing paid; no such building, no replacement); a fallen hero revives at the player's altar after the altar's revival time; with `rebuild` (the default), a destroyed building rises again where it stood 30 seconds later, as a site that raises itself over its construction time, once nothing stands on its footprint. Units the script transferred away are not replaced.
- **Waves.** Wave times count from when the player became active (the start, or its waking). A line without `after` waits for `ai_waves(player, true)`; `grow` adds its units after every `every` waves sent; `until` ends the line. A trained wave is spread over the buildings that train its units, one unit after another per building, and launches when the last is out; a unit no building can train is left out, and training at a building that falls is lost. A region wave appears around the region's centre and launches at once. With `hero`, the player's living heroes march with it. A wave attack-moves to its target and keeps attacking until its units are gone; its heroes then walk home. Targets: `base` is the nearest living enemy main fort (a player start's `mainFort`, else any enemy building of a race's fort line: its starting fort and what that upgrades into), else the nearest enemy building, and an enemy unit only when the enemy has no buildings; `nearest` is each unit's nearest enemy unit; enemies are players of the other team, never neutral camps. A wave aimed at a building heads for the side of its footprint facing the wave (a building's centre is no place a route can end), and one aimed at a region whose centre a building now covers for the open ground nearest it; a unit that has arrived (within 8 cells) with nothing to fight goes for the nearest enemy within 24 cells, its defenders or the building itself.
- **Skirmish**: the skirmish AI with the player's `skirmish` limits (`style`, `armyCap` capping its army, `pace` multiplying every decision interval, `income` multiplying gathered currencies). The mission's tech reaches it through `World::set_ai_tech(player, forbidden)`: in a campaign mission a Skirmish player of the campaign's race is held to the same locked tech as player 1 (in the saved limits, refused like player 1's, and told to the AI again after a restore; `World::limit_tech` does the same for any player). Forbidden units leave its recruits and no build, train or research command for them is issued. Switching a player to another behaviour stops the skirmish AI; switching to Skirmish starts it.
- **Vision.** An ally with `vision: false` keeps its sight to itself (vision treats it as a team of one); it stays an ally in everything else.
- **Script API** (`mission.*`; a player is its number `2` or `"player.2"`, and must be one the mission lists, else the script stops with an error): `ai_waves(player, on)` pauses or resumes its waves (resuming starts lines without `after`; a wave due while paused is sent on resume); `ai_attack_now(player)` sends the next wave of every running line now (if none runs, the first unfinished line's) and restarts their timers (Waves behaviour only); `ai_target(player, target)` sends its waves from now on to `"base"`, `"nearest"` or a region id; `ai_behaviour(player, behaviour)` switches it to `passive`, `base`, `waves` or `skirmish`; `ai_wake(player)` brings a dormant player in: its placements marked `activation: "script"` appear and its timers start; it returns false when the player is not dormant.

## The editor

- (Built: [editor](editor.md), "Campaigns".) **Campaigns** on the editor's rail: the library of campaigns; a campaign's steps as a list (chapters as headings, then Story, Mission and Cinematic rows, reordered by dragging, "+ Chapter / Story / Mission / Cinematic"); every step with **Play**, missions also with **Edit**, which opens the map in the Maps editor. The panel shows, with nothing selected, the campaign's settings (name, race, background, card art); with a step selected, its fields.
- **The Maps library** gets a **Campaigns** group: each campaign, under it its maps in step order. A map in a campaign's `missions/` that no step uses shows as "not in the campaign".
- **The Tech view** (U) of a campaign map in the Maps editor: the campaign race's whole tree drawn from the content (what workers build, what buildings train and research, requirements, research chains, building upgrades), laid out by hall tier with each building carrying its units and research. Each node shows its state in this mission and is clicked through Locked, New and Blocked; a blocked building marks everything that needs it unreachable; contradictions (research allowed whose building is blocked) show in red and in the check. A strip of the campaign's missions moves between them; "changes only" hides the rest. Positions are computed, never saved.
- **Players** in the map's panel: each computer player's race, team (ally or enemy), colour, shared vision and behaviour, and its waves as a list of rows of unit portraits with their timing, source and target.
- **Play from here** on any step, with the carried state fresh, as after the previous step, or edited by hand.
- The check covers campaigns: missing or broken maps, carried heroes missing from the next map, tech contradictions, variables read that nothing sets, missing art.

## Formats

The shapes every part builds against (2026-10-21). Field names are camelCase JSON; the Rust types in `canopy-content` define them exactly and generate their schemas.

**`campaign.json`** (in `campaigns/<id>.utcc/`):

```json
{
  "race": "ants",
  "background": "asset.interface.campaign.vanguard-background",
  "card": "asset.interface.campaign.ants",
  "steps": [
    {"kind": "chapter", "id": "prologue", "title": "campaign.vanguard.prologue.title"},
    {"kind": "story", "id": "the-hollow", "title": "campaign.vanguard.the-hollow.title",
     "pages": [{"art": "asset.interface.campaign.vanguard.story-hollow-1",
                "text": "campaign.vanguard.the-hollow.page-1"}]},
    {"kind": "mission", "id": "hollow-brood", "map": "hollow-brood",
     "title": "campaign.vanguard.hollow-brood.title",
     "chapterLine": "campaign.vanguard.hollow-brood.chapter-line",
     "art": "asset.interface.campaign.vanguard.hollow-brood",
     "story": "campaign.vanguard.hollow-brood.story", "heroLevelCap": 3,
     "tech": {"new": ["building.ants.fort", "unit.ants.settler"], "blocked": []}},
    {"kind": "cinematic", "id": "condenser-hums", "title": "campaign.vanguard.condenser-hums.title",
     "note": "campaign.vanguard.condenser-hums.note"}
  ]
}
```

- **Text** (D96): every word a player reads is a key into the text library, as in the base game. The campaign's `text/en/campaign.json` holds its steps' text, `campaign.<campaign>.<step>.<field>` (`title`, `chapter-line`, `story`, `note`, `page-<n>`), and its speakers' names, `speaker.<name>` (`speaker.aurelith`), shared by its missions. Each mission map's `text/en/mission.json` holds its objectives, `objective.<mission>.<id>.title` and `.description` (what the map's objectives name), and its script's lines, `line.<mission>.<words>` (`line.founding.here-we-start-again-and`). The game layers base, campaign and mission text, so a mission may also name base keys (`unit.ants.queen.name`). The editor's campaign view shows and edits the text, writing new keys as it goes. A field holding a dotted key no text file has is an error, and a test checks the keys every shipped mission's objectives and script name.
- Every step has an `id` unique in the campaign (progress, Play from here). Optional on every step: `music` (a music sound event, D97). Story pages may add `narration` (a dialogue sound event). Events may live in the base or in the campaign's own `sounds/`. Chapters may add `art`.
- A mission's `map` is a map id among the package's `missions/` maps. Its tech: everything listed in `new` by this mission or any before it, minus this mission's `blocked`; whatever no mission has listed is locked. A race's starting setup still places its units and buildings.
- **Carried heroes** need no list: when a mission is won, every hero of player 1 is saved by its definition (level, experience, learned abilities, items, permanent bonuses); when a mission starts, every hero of player 1 the map places whose definition has a saved state takes it on. `heroLevelCap` caps them.

**Progress** (the user folder, `campaigns/<campaign id>.json`): the furthest step reached, by its id (the next to play; ids survive steps being inserted), `complete` once every step is done, the saved heroes by definition, and the campaign variables (`{name: number | string | boolean}`):

```json
{
  "furthest": "condenser-hums",
  "heroes": {
    "unit.ants.queen": {"definition": "unit.ants.queen", "experience": 230,
                        "learned": {"swarm": 2, "royal-guard": 1},
                        "items": [{"item": "item.charm", "charges": 2}, null],
                        "bonuses": {"maxHp": 10, "damage": 0, "armor": 0, "maxMana": 0}}
  },
  "variables": {"rescued": 3, "ally": "moths"}
}
```

**A map's `mission` section** keeps objectives, regions and the script, and gains `players`; `campaign`, `order`, `nextMission`, `company`, `presentation`, `race`, `playerRaces` and `heroLevelCap` move to the campaign (the player's race is the campaign's):

```json
"players": [
  {"player": 2, "race": "ants", "team": "enemy", "vision": false, "color": "#8a7f96",
   "ai": {"behaviour": "waves", "dormant": false, "rebuild": true,
          "waves": [{"after": 120, "every": 90,
                     "units": [{"unit": "unit.ants.warrior", "count": 2}],
                     "grow": {"every": 3, "unit": "unit.ants.warrior", "count": 1},
                     "hero": true, "from": "trained", "target": "base"}]}},
  {"player": 3, "race": "ants", "team": "ally", "vision": true, "ai": {"behaviour": "base"}}
]
```

- `team` is `ally` (player 1's team) or `enemy`; `behaviour` one of `passive`, `base`, `waves`, `skirmish`; a wave's `from` is `"trained"` or `{"region": "<id>"}`, its `target` `"base"`, `"nearest"` or `{"region": "<id>"}`; `until` (seconds) ends a wave line, and a later line can take over. `skirmish` takes `{"style", "armyCap", "pace", "income"}`. Optional `color` is the player's team colour over its slot's, everywhere the slot's would show (team colour masks, the minimap, the world UI): a player colour by name (`red`, `blue`, `green`, `yellow`, `orange`, `purple`, `teal`, `white`, as `config/players.json` names them) or any `#rrggbb` (a feral brood's sickly violet-grey, a colour no player has); the editor's Players panel picks it (Slot, the eight swatches, or the picker). Players the map lists nowhere stay out of the match.

**Mission script additions** (Lua, the `mission` API): `give_stock(player, item, amount)` adds a currency (`item.amber`) to a player's stock, or takes it with a negative amount, never below nothing (`stock(player, item)` reads it); `campaign_get(name)`, `campaign_set(name, value)`; `allow(id)`, `forbid(id)` (tech, for the rest of the mission); `ai_waves(player, on)`, `ai_attack_now(player)`, `ai_target(player, region)`, `ai_behaviour(player, behaviour)`, `ai_wake(player)` (a dormant player arrives: its placements marked `activation: "script"` appear, then it plays its behaviour).

## The runtime

Built 2026-10-21 (`canopy-content` `campaign`, `canopy-sim` `campaign`, `game/src/campaign.rs`, `game/src/screens/campaign.rs`).

- **Loading.** Campaigns are the `.utcc` packages in `base/campaigns/`; a campaign that breaks a rule is left out with a warning. Its mission maps join the game's maps (the skirmish list leaves missions out). Its `assets/` cook into the same pack. A mission plays on the base content, or on base, campaign and map layered when the campaign or the map package carries game data of its own (units, abilities, rules…); the save and replay identity then names the campaign too.
- **Validation** (`canopy_content::campaign::validate`, and a test over every shipped campaign): the race exists; step ids are unique; a mission's map is among the package's `missions/`; tech ids are units, buildings, items or research of the campaign's race and never both new and blocked; every art asset exists (in the base or the package's own `assets/`), music and narration name music and dialogue sound events (the base's or its own `sounds/`); step texts name keys some text file holds; texts and the level cap (1–50) are in range. Unknown fields are errors. Story text and a mission's `story` are paragraphs separated by blank lines.
- **A race's tech** is its roster (units and buildings), what its roster buildings train (queued production), research and upgrade into. `Campaign::locked(content, step)` is that minus everything `new` up to the step, plus the step's `blocked`.
- **In the simulation** a mission starts with `World::new_campaign(content, map, setup, &CampaignStart)`: the locked tech for player 1, the carried heroes, the level cap and the variables. Everything then lives in the saved state (`state.campaign`, `state.tech`), so saves, restores and replays carry it (a campaign mission's replay starts from the save of its first tick). Locked tech is refused for building, training, research and building upgrades ("Not available in this mission"); the view's colony lists it and the command card leaves it out. `World::limit_tech(player, ids)` holds a computer player back the same way (the campaign AI's Skirmish behaviour).
- **Carried heroes.** When a mission of the next step is won, every hero of player 1 (fallen ones too, never summoned ones; one per definition) is saved over the earlier save of its definition. A placed hero of player 1 of a saved definition takes it on at the start: experience up to the mission's cap, items with their charges, permanent bonuses and its learned ranks, or, when its capped level no longer allows them, its initial ranks with the points to spend again; then full health and mana.
- **Variables.** `campaign_set` checks names and values as the mission variables are (at most 128, text of at most 2,000 characters); a run reads its own writes and a failed run changes nothing. A mission played on its own keeps no campaign variables; its `allow`/`forbid` act on player 1.
- **Flow.** Continue runs the next step; a story or card finished, or a mission won, saves the progress and runs the step after it (a won mission shows its victory for 2.5 seconds first). Play, Read and Watch on an earlier step replay it and return to the campaign screen without changing the progress. A lost mission shows its result with Retry and Return to campaign. A loaded save of a campaign mission belongs to the furthest step reached that plays its map. Escape skips a story or card.
- **Screens** (`config/ui/layouts`): `campaign` (the campaigns as cards), `campaign-steps` (the campaign screen), `card` (story pages revealed paragraph by paragraph with Continue and Skip; chapter and cinematic cards that move on after a few seconds or a click or key), and the `mission-view` of `loading`. `canopy capture-screens --only campaign` captures them.

## Checking a mission

Built 2026-10-22 (`canopy_sim::mission::check`; `crates/sim/tests/mission_shipped.rs` runs it over every shipped mission in `just check`, about 1.5 s). A static check, without playing, of what cost the campaign builds days. The mission starts headless as the campaign starts it, and the simulation's own navigation judges it (terrain, scenery blockers and buildings, body by body):

- **Stranded units.** Every placed unit and scripted spawn (`activation: script`) that walks must start a route where it stands (as `Spatial::route` starts one: its cell, a neighbouring centre, or the step a body wedged against a blocker takes) and reach at least 300 cells by the route search's own steps.
- **Wave regions.** A region a wave comes from or marches to has its centre on open ground: not in a building's footprint, not on closed ground.
- **Routes.** Player 1 (its start, else its largest starting walker) to every region: reached when its smallest walker can stand anywhere inside it (scripts ask `in_region`), with the real pathfinder (`Spatial::find_path`) to that cell. Each wave line from its sources (the region it arrives at, or the door of each building of its player that trains its units) to its target (a region, or player 1 for `base`/`nearest`), walked with its largest unit's body. Each route reports the narrowest width of its widest way (`2c − 1` cells for a clearance of `c`, steep steps never crossed) and where it lies. A region player 1's large units (radius 3, a hero twice a warrior's size) cannot enter is flagged, not failed: a secret behind a gap in the trees may be meant so.
- **Buildings as each side meets them.** Player 1 breaks the enemy's and the neutral buildings (gates, rubble, crates), so its routes and every unit's room are judged without them; waves are judged with neutral buildings (no attack-move target), and a way their own side's buildings close until they fall (a station's gate) is only noted (`walled`): the script must hold the waves until then.
- **Corridor spawns.** A scripted neutral building (a crate, rubble, a cache) spawns where the corridor is at least 13 cells wide.
- **A dry run of every scene.** For each function of the script's `scenes` table, the mission starts, `on_start` runs (a scene it plays is skipped), every scripted placement spawns (a scene's own `spawn` of one then passes), the objectives start afresh (a scene that ends one no earlier scene began here begins it first), `on_tick` does nothing, and the scene plays skipped. A variable the script names that nothing has set yet stands in as player 1's lead unit's x (names ending in `x`), its y (ending in `y`) or true: what triggers set before they play a scene. It must end without a script error, with cinematic mode off (unless it won or lost), and leave nobody where it cannot take a step.

Each finding is one sentence naming the map, the placement or region and the problem; a unit that cannot step names the nearest ground its body fits on. `cargo test -p canopy-sim --test mission_shipped -- --nocapture` prints every route with its width, the scripted buildings' corridors and the flags.

## Scenes

In-mission cinematics, as Warcraft III builds them with triggers (D88): the mission fades in on a hero already running along a road, the bars come in, the camera cuts and blends between saved cameras, characters turn to each other and speak in the lower bar with their portrait talking, and the player gets the game back. Everything a scene changes is simulation state (`MissionState::cinema`, `crates/sim/src/mission/cinema.rs`), so saves, replays and every peer show the same scene; the picture follows that state (the camera blend and the fade are computed from their start tick and length at the tick drawn).

**Saved cameras** (`mission.cameras` in `map.json`, `canopy_content::map::MapCamera`), placed in the map editor (Place → Special → Camera, [editor](editor.md)):

```json
"cameras": [
  {"id": "over-tiv", "x": 84, "y": 112, "height": 3, "distance": 28, "pitch": 11, "yaw": 128, "fov": 32, "roll": 1.5}
]
```

`x`, `y`: the point looked at (navigation cells); `height`: its lift above the ground (default 0); `distance`: eye to that point (2–400 world units; one per cell; the game's camera stands 117 away); `pitch`: angle of attack below the horizon (−30 to 89°; the game's is 56); `yaw`: rotation round the point (0 puts the eye due south looking north, as the game's camera; 90 east); `fov`: vertical field of view (10–100°; the game's is 34.3 at 16:9); `roll` (−90 to 90°, default 0). Ids are unique; the check reports cameras outside the map or out of range.

**A scene** is a function of the script's `scenes` table, started with `mission.play(name)`. It reads top to bottom like a Warcraft III trigger, waits included:

```lua
function on_start()
  mission.fade_out(0)                      -- black from the first frame
  mission.play("opening")
end

function scenes.opening()
  mission.cinematic(true)                  -- bars in, HUD and selection away, commands refused
  mission.camera_to("road", 0)             -- a cut
  mission.move("aurelith", 84, 112)        -- already running when the picture comes up
  mission.fade_in(2.5)
  mission.camera_to("follow", 6)           -- an eased six-second blend
  mission.wait_arrived("aurelith")
  mission.face("aurelith", 90, 106)
  mission.wait_facing()
  mission.speak("tiv", "line.hollow-brood.my-queen-the-hollow-s", nil, "speaker.tiv")
  mission.camera_to("over-tiv", 0)
  mission.animate("aurelith", "spell")
  mission.speak("aurelith", "line.hollow-brood.then-we-go-and-ask", nil, "speaker.aurelith")
  mission.cinematic(false)
  mission.camera_reset(1.5)                -- back to the game's camera over the last shot
end
```

How it runs: the sandbox has no coroutines and a fresh Lua state every run, so a scene cannot be suspended. Instead **every run calls the scene's function from its top**; each call it makes to the `mission` table that is not a query is a numbered *statement*. Statements done in earlier runs are passed over (they answer nothing), the next one is done, and a wait whose time has not come ends the run there; when the function returns, the scene is over. What is saved is the scene's name, how many statements are done and when the last one was. A scene therefore must reach the same statements in the same order every run: branch on variables it set (`mission.set`/`get`) rather than on things that change while it plays. Queries (`tick`, `alive`, `position`, `get`, `clicks`, `playing`, …) answer every run; a new read-only function of the API joins `QUERIES` in `api/scene.rs`. Statements passed over cost nothing of the run's call budget. `on_tick` keeps running beside a scene (`if mission.playing() then return end` keeps it quiet).

**The skip.** Escape skips a skippable scene (`mission.play(name)` is; `mission.play(name, false)` is not): the player's `skipScene` command reaches the simulation, which the next tick runs the rest of the scene at once. Walks still under way end where they lead (the unit is put there), waits pass, moves become placements, turns are made, lines and animations are dropped, cameras cut and fades jump to their end; everything else (spawns, variables, objectives, cinematic mode, `win`) happens as written. So a skipped scene leaves the world as if it had played, which is why a scene ends with what the player should find (cinematic mode off, the camera reset, the objective set). The picture covers the jump with a moment of black.

**Cinematic mode holds the battle** (D88, as Warcraft III's campaign triggers pause every unit and the computer players for a cutscene). While `cinematic(true)` is on, the scene owns the moment:
- **The cast acts, everyone else holds.** The units the script directs while the mode is on (`move`, `attack_move`, `attack`, `follow`, `face`, `animate`, and a skip's placements) are the *cast* (`Cinema::actors`, at most 64, saved). Every other unit and tower stands where it is: it acquires nothing, its swing in progress is undone, a cast in progress is interrupted, cast orders and autocast wait, it neither walks nor starts work. It keeps its orders, so an attack, an attack-move or a wave's march goes on when the mode ends. A run's statements count together: a unit ordered in the same run that turns the mode on joins the cast.
- **The cast fights only when told.** A cast unit acquires no targets of its own; `attack` and `attack_move` fight as ordered. Held units never strike back.
- **The computer players wait.** The skirmish AI decides nothing, and the campaign AI's clocks stop: wave lines, free training, rebuilds and revivals fall due as many ticks later as the mode lasted, so nothing launches during a cinematic or all at once after it, and an opening cinematic delays the first waves (`after` counts from its end).
- **What still happens:** missiles and spell deliveries already in flight land, statuses and auras run their course, buildings train and research, springs pulse, and the script and its `on_tick` run.
- **When the mode ends** (`cinematic(false)`, a skip that reaches it, or a script error) the cast is dismissed and every unit resumes its orders on the next tick. The hold is simulation state (`mission::held`), so saves, replays and every peer agree.

| Function | Does |
|---|---|
| `play(name[, skippable])` | Starts `scenes[name]` (skippable unless `false`); one scene at a time |
| `playing()` | The scene in play, or nil (query) |
| `cinematic(on)` | Cinematic mode: letterbox bars slide in, the HUD hides, the selection clears, player commands are refused (the game menu and the skip still work), the world's sounds duck while music and voices keep theirs, and the battle holds: only the units the script directs act, and the computer players wait (above) |
| `camera_to(id[, seconds])` | Cuts (0, the default) or blends, eased, to a saved camera |
| `shot(kind, subject…[, options])` | A shot preset on actors where they stand (below): `shot("single", unit)`, `shot("over", from, to)`, `shot("two", a, b)`, `shot("wide", unit_or_region)`; options `{side = "left"\|"right", distance, height, fov, roll, seconds}` |
| `camera_pan(x, y[, seconds])` | The current saved camera, looking at another point |
| `camera_reset([seconds][, x, y])` | Back to the game's camera, over the last shot's point (or `x`, `y`); the player has it again once the blend ends |
| `fade_out([seconds][, colour])`, `fade_in([seconds][, colour])` | A fade filter to opaque or clear from where it stands; `black` (default), `white` or `#rrggbb` |
| `animate(unit, name[, loop])` | The unit plays its model's animation (`attack`, `cast`, any state its model has; `spell` and `victory` fall back to `cast`) once, then its own again, or held with `loop`: a looping clip loops, a one-shot stays in its last pose (`kneel`). `idle` ends it, and so does any order the unit is given afterwards (the script's `move`, `attack`, `follow`, `transfer`, `transform`, or the player's); walking and fighting take precedence while they last. Scene poses: `kneel` (a one-shot) and `sleep` (a loop), on the models that have them |
| `effect_at(key, effect, x, y[, seconds])` | Plays an effect definition (`effect.…`) on the ground at a point: a one-shot for its own length, a looping one held until `seconds` pass or the script stops it. The same `key` replaces it. A skip passes over a one-shot |
| `light_at(key, x, y, colour, intensity, radius[, seconds][, height])` | A point light (`white` or `#rrggbb`; intensity 0–100, an effect light's is about 10–35; radius 0.5–60 cells; height above the ground, default 2) that eases from where the light of that `key` stands (nothing, when new) to `intensity` over `seconds`, then holds; ramped to 0 it is gone. A skip makes it jump to its end. At most four; they come before every other effect light |
| `stop_effect(key)` | Ends the effect and the light of that `key` at once |
| `speak(unit, line[, seconds][, name])` | The unit says a line (a text key; its definition's name, `<definition>.name`, and portrait unless a `speaker.*` key names it); in cinematic mode a transmission in the lower bar with its live portrait. Its length is the reading time of its English text (a second plus 15 characters a second, 2.5–20 s) unless given. In a scene it waits for the line to end. `say(name, icon, line[, seconds]…)` takes keys the same way, and `nil` seconds is the reading time |
| `wait(seconds)`, `wait_arrived(unit)`, `wait_facing([unit])`, `wait_line()` | Scene waits: time; the unit's walk (or any order) has ended, or a scene walk has had twice its straight time at the unit's speed and three seconds (a crowd or a base in its way never hangs the scene); the turn has; the line is over |
| `clicks(unit)` | How often the campaign player clicked the placed unit to select it (query; easter eggs) |

**Shot presets** (D91): a line is framed on its speaker where the speaker actually stands, so a walk that stops short of its mark (an army in the way, a staged scene) still has its lines on the right faces. The simulation keeps only what the script asked for (`Pose::preset`: the kind, the actors' placements, a wide shot's region, the options), as deterministic as a saved camera; the picture (`game/src/play/presets.rs`) computes the camera every frame from the actors as it draws them: their position, facing, face as posed (a humanoid rig's neck and head top through its current clip, so a kneel lowers the shot) and standing height (the face's height when upright: a kneeling speaker is framed as close as a standing one).
- `single`: the unit's face, head and shoulders to the waist, from its front side (35° off its face; 30° lens), the face high between the letterbox bars, the unit placed so it looks into the frame.
- `over`: past `from`'s shoulder onto `to`'s face, `from`'s head at the picture's edge. The camera stands back far enough for that whatever their distance: a long lens from further back for a speaker across a yard, a wide one close behind for one at arm's length.
- `two`: both from the side of the line between them, from the thighs up, the lens fitting both.
- `wide`: an establishing shot of a unit (from its front side, 6.5 statures off, a little above) or of a region (from the south side, all of it in view, the picture's reveal widened to cover it).
- Options: `side` (left or right, seen from the first actor: a single's camera at the unit's left or right front, an over-the-shoulder over `from`'s left or right shoulder, a two shot on the first unit's left or right of the line, a wide shot turned round that way), `distance` (eye to the framed point, 1–400 cells), `height` (the eye above the first actor's feet, −20 to 200), `fov` (10–100°), `roll` (−90 to 90°), `seconds` (a blend, as `camera_to`; 0, the default, cuts). Options are a Lua table of names; the sandbox passes it sorted by name.
- **The side, when the scene leaves it open**, is the one whose view is clearer: five lines of sight across the frame are tested against scenery (the giant forest pieces), buildings and units other than the shot's own, and the camera must stand over the map. Each side is tried at its usual angle, then turned further round; the eye is brought in front of anything still in the way of a close shot. A pair keeps one side of the line between them for the whole exchange, either way round (the 180° rule), and a model whose portrait names a turn (below) is seen from that side in a single first.
- `camera_reset` after a preset returns over where its first actor (or its region) stands. A preset whose actors are not drawn shows the game's camera over where the first stood when the shot began.

The Hollow Brood's `shot_on` and Founding's `close_on` helpers (a saved camera's angle panned onto a unit) predate the presets; new scenes use `shot`.

Older functions keep working inside and outside scenes (`move`, `face`, `say`, `spawn`, the unit shots of `camera`, …). The mission fades in from black: the picture stays black until `on_start` has run.

**Presentation** (`game/src/play/shots.rs`, `screens/match_ui.rs`, `config/ui/layouts/mission-hud.json`): letterbox bars of 10 % at the top and 21 % at the bottom, eased in and out over 0.65 s; the lower bar holds the transmission: the speaker's live model in a gold-framed portrait (its icon when it is not drawn), framed close: a humanoid rig (a `Neck` joint and a `HeadTop_End` or `Head` joint, with or without a prefix such as `mixamorig:`) shows head and shoulders round its head whatever its staff, wings or train span, any other model the top front of its bounds; a model whose automatic framing misses its face says what to frame in its `asset.json` (`display.portrait`: `{focus: [x, y, z], size, turn}` in the model's own space, `turn` in degrees, default −18; the ant general frames his head from his left, past his sickle). The name in warm gold Georgia, the line in Georgia 19 px; a quiet "Esc Skip" at the top bar's right while a skippable scene plays. Camera blends ease in and out (smoothstep), the distance in proportion, the rotation the short way round; a saved camera's eye keeps above the ground and water. The `@portrait` render target shows the speaker while a line plays in cinematic mode.

**Easter eggs.** Clicking a unit (selecting it) on a mission map sends a `clicked` command, counted per placement for the campaign player and saved (`mission.clicks`). Lines when a hero walks near something are `in_region` in `on_tick`. A lone creature placed for nobody outside any camp (a tree frog on a lily pad, a sleeping Alate) stays where it stands: no player fights it unprompted, it casts its own creature spells as camp creatures do but never a colony's (a neutral founds nothing, so a neutral Alate stays an Alate), takes only a forced attack, and answers a blow by attacking its striker until the striker is gone or out of its sight (a passive camp's members answer the same way).

The bench: `base/maps/showcase/cinematic-bench.utcm` (authored by `tools/maps/cinematic_bench.py`) plays the Hollow Brood's opening. Captures: `just shot --play --map cinematic-bench --tick <t> --out <png>`. Any mission scene, staged mid-mission when it needs to be: `just shot --play --map <mission.utcm> --scene <name>|all|list --out <folder> [--sheet] [--compare <rev>] [--set <variable>=<value>]` (`game/src/play/preview`; skill `canopy-scenes`, "Seeing it").

## Autoplay

A mission can play itself (D93): `autoplay.lua` beside `mission.lua` is a small plan that steers player 1 to victory with a player's commands. It is for checking a mission fast, headless or on screen, sped up with its scenes at normal speed.

**What it is.** The plan reads only what player 1 could know and issues ordinary commands through the normal queue, so an autoplay match is an ordinary replay (`canopy_sim::autoplay`). It runs in its own Lua sandbox, apart from the mission script: one state for the whole run, its chunk a coroutine resumed after every step; the steps that take time wait and the plan goes on when they are done. While a cinematic plays the plan waits (it never skips a scene). The plan is not part of the map's identity: adding or changing it changes no save, replay or multiplayer identity. A loaded save plays on by hand.

**The plan's API** (globals; a placement is the map's id, `x, z` are cells):

| Steps (they take time) | Do |
|---|---|
| `go(x, z [, how])` | The army attack-moves there (`"walk"`: a plain move; `"hero"`: the leading hero alone) and the plan waits until the leading hero arrives (10 cells, 3 alone), the army stands still near it, or 3 minutes pass. A goal nobody can reach comes nearer, 4 cells at a time, as a player clicks again; units that cannot get there are left out. Returns true on arrival |
| `attack(id)` | The army attacks the placement until it falls (seen, or a building gone from its place with the ground in sight); unseen, it walks to where it was last seen or the map puts it |
| `pickup([id [, unit]])` | The hero (the plan's leading one by default) walks up to the item and picks it up; without an id, every item lying in sight within 14 cells |
| `wait(seconds)` · `wait_clock(minutes)` · `wait_until(test [, seconds])` · `wait_objective(id [, seconds])` · `mass(n [, seconds])` | Time; the mission clock; a function of the plan's own; an objective completed (false if failed or timed out); an army of `n` |

| Standing orders and commands (at once) | Do |
|---|---|
| `hero{"queen", learn = {…}, heal = "binding", heal_below = 0.6, shield = "binding", shield_below = 0.4, potions = {"item.…"}, potion_below = 0.35, fight = {"binding", …}}` | A hero's own turn every 2 s: learns the list in order (a name twice is rank two), shields itself when hurt, heals the most wounded near it (never itself), drinks a potion when low, casts its `fight` spells at the fight near it. The first `hero{}` leads the army |
| `economy(on)` | The colony is the skirmish AI's, held to the mission's tech: it builds, trains, researches and defends; the plan's walks, attacks and pushes keep their units |
| `push(x, z [, n])` / `push()` | From now on the army attack-moves there every 2 s while it is at least `n` (between steps) |
| `keep(unit, x, z)` / `keep(unit)` | Holds a unit apart from the army, waiting at a point (the Queen out of an assault); badly hurt it is the AI's again |
| `click(id [, times])` · `train(unit [, n])` · `research(id)` · `build(definition, x, z)` · `cast(unit, binding [, id | x, z])` · `use_item(item [, unit])` · `learn(unit, ability)` | A player's commands; each answers whether it was sent |
| `army()` · `army_center()` · `alive(id)` · `position(id)` · `hp(id)` · `level(id)` · `objective(id)` · `stock(item)` · `count(definition)` · `scene()` · `clock()` | What player 1 knows: `alive` is nil while it cannot tell, `objective` `"active"`, `"completed"` or `"failed"` |
| `say(…)` | A line in the run's log |

The sandbox has the core language, `ipairs`, `select`, `type`, `tostring`, `tonumber`, `error`, `assert`, `string`, `table` and `math` without random numbers: no clock, files, `pairs` or coroutines of its own. A plan that errors stops with its line (`autoplay.lua:12: …`), and one that runs two million instructions without waiting stops too.

```lua
hero{"queen", learn = {"living-light", "cocoon", "one-body"},
     heal = "living-light", shield = "cocoon", potions = {"item.resin-salve"}}
attack("cache.salve")
pickup()
go(116, 296)
click("frog", 10)            -- the easter egg
go(330, 136)
attack("rubble")
wait_objective("survivors", 60)
go(300, 66)
wait_objective("ashka", 240)
```

**Headless.** `just autoplay <mission>` plays a mission (its map or step id, or a `.utcm` path) as its campaign starts it (the tech up to it, the level cap; no carried heroes) and prints the plan's steps, the objectives, refused commands, the time and the outcome; `--talk` adds the mission's lines, `--seed`, `--minutes`, `--plan <file>` plays another plan on the map, `--verify` replays the run's commands alone and compares. `AUTOPLAY_COMMANDS=1` prints every command and `AUTOPLAY_WATCH=<placement>` one unit's state every 10 s. `just campaign-test` plays every shipped mission's plan with `--verify` and fails unless all win (about 30 s; it is not part of `just check`).

**In the game.** Play with AI beside Play on the campaign screen (a mission whose map has a plan; the run's following missions play with theirs too), the editor's Test with AI beside Test in game, and `just play --map <mission> --autoplay` (a campaign's mission plays as its step). The match starts at 4× (1×, 2×, 4×, 8× at the top, or = and −); a cinematic drops it to 1× by itself and it returns after; Take over (F7) ends the plan and hands the match back at 1×. The camera follows the plan's fight, else its leading hero, else its army, until the viewer moves it (then six seconds). Refused commands show no red message while the plan plays; the plan's log goes to the game's log (`autoplay` target). Captures: `just shot --play --map <mission> --autoplay --tick <t>` plays the plan up to `t` and shows its line.

## Research: how Warcraft III and StarCraft II do it

- **Warcraft III.** A custom campaign is one `.w3n` archive made in the Campaign Editor: `war3campaign.w3f` (name, difficulty text, author, description, race, the menu's background scene and sound, and the mission buttons: chapter, title, visible at start, which map), the maps, and campaign-wide object data and imports every map shares. The Campaign Editor packages and lays out the menu; the logic lives in each map's triggers: `SetNextLevelBJ` stores the next map and `ChangeLevel` loads it after the score screen; `SetMissionAvailable` unlocks a menu button; heroes travel through a game cache (`StoreUnit`, `SaveGameCache`, `RestoreUnit`). Cutscenes are trigger scripts in the mission map itself: saved cameras, cinematic mode, transmissions (portrait, voice, subtitle), unit animations, fades and waits. Opponents run small campaign AI scripts: defenders kept at home, attack groups (`CampaignAttackerEx`) sent after a wait (`SuicideOnPlayerEx`), every number given for Easy, Normal and Hard.
- **StarCraft II.** No campaign container: layers of data (`Liberty` → `LibertyCampaign` → `LibertyStory`, and on for each episode), mission maps depending on the campaign layer, and between missions a playable hub that is itself a map (rooms, characters, conversations, upgrades). Progress lives in banks (named key-value files per player). Missions lean on a campaign script library (transmissions, objectives, heroes, the victory sequence, values per difficulty). The Cutscene Editor is a timeline studio (models, cameras, lights, sounds, dialogue lines, bookmarks the script reacts to and jumps between, filters for variants); conversations are data (lines, choices, conditions, states remembered with the progress).
- Sources: [jassdoc](https://lep.duckdns.org/jassbot/doc/ChangeLevel), [War3Net's `war3campaign.w3f`](https://github.com/Drake53/War3Net/blob/master/src/War3Net.Build.Core/Info/CampaignInfo.cs), [Hive: making a basic campaign](https://www.hiveworkshop.com/threads/making-a-basic-campaign.6287/), [SC2 Editor guides](https://s2editor-guides.readthedocs.io/New_Tutorials/01_Introduction/013_Standard_Dependencies/), [Galaxy API reference](https://mapster.talv.space/galaxy/reference), and the Heart of the Swarm campaign maps of a community mod on Jiri's machine, read with the StarCraft importer's archive reader.
