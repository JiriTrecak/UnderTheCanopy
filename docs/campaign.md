# Campaigns

Agreed with Jiri, 2026-10-21 (D85). Nothing of it is built yet. The mockups are the canvas [Campaign Mockups](https://claude.ai/artifact/CbzEBp3GQZB5wqcrxA6aBT).

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

## The editor

- **Campaigns** on the editor's rail: the library of campaigns; a campaign's steps as a list (chapters as headings, then Story, Mission and Cinematic rows, reordered by dragging, "+ Chapter / Story / Mission / Cinematic"); every step with **Play**, missions also with **Edit**, which opens the map in the Maps editor. The panel shows, with nothing selected, the campaign's settings (name, race, background, card art); with a step selected, its fields.
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
    {"kind": "chapter", "id": "prologue", "title": "Prologue"},
    {"kind": "story", "id": "the-hollow", "title": "The Hollow",
     "pages": [{"art": "asset.interface.campaign.vanguard.story-hollow-1", "text": "…"}]},
    {"kind": "mission", "id": "hollow-brood", "map": "hollow-brood", "title": "The Hollow Brood",
     "chapterLine": "Prologue · Mission One", "art": "asset.interface.campaign.vanguard.hollow-brood",
     "story": "…\n\n…", "heroLevelCap": 3,
     "tech": {"new": ["building.ants.fort", "unit.ants.settler"], "blocked": []}},
    {"kind": "cinematic", "id": "condenser-hums", "title": "The Condenser Hums", "note": "What it will show."}
  ]
}
```

- Every step has an `id` unique in the campaign (progress, Play from here). Optional on every step: `music` (an audio asset). Story pages may add `narration` (an audio asset). Chapters may add `art`.
- A mission's `map` is a map id among the package's `missions/` maps. Its tech: everything listed in `new` by this mission or any before it, minus this mission's `blocked`; whatever no mission has listed is locked. A race's starting setup still places its units and buildings.
- **Carried heroes** need no list: when a mission is won, every hero of player 1 is saved by its definition (level, experience, learned abilities, items, permanent bonuses); when a mission starts, every hero of player 1 the map places whose definition has a saved state takes it on. `heroLevelCap` caps them.

**Progress** (the user folder, `campaigns/<campaign id>.json`): the index of the furthest step reached, the saved heroes by definition, and the campaign variables (`{name: number | string | boolean}`).

**A map's `mission` section** keeps objectives, regions and the script, and gains `players`; `campaign`, `order`, `nextMission`, `company`, `presentation`, `race`, `playerRaces` and `heroLevelCap` move to the campaign (the player's race is the campaign's):

```json
"players": [
  {"player": 2, "race": "ants", "team": "enemy", "vision": false,
   "ai": {"behaviour": "waves", "dormant": false, "rebuild": true,
          "waves": [{"after": 120, "every": 90,
                     "units": [{"unit": "unit.ants.warrior", "count": 2}],
                     "grow": {"every": 3, "unit": "unit.ants.warrior", "count": 1},
                     "hero": true, "from": "trained", "target": "base"}]}},
  {"player": 3, "race": "ants", "team": "ally", "vision": true, "ai": {"behaviour": "base"}}
]
```

- `team` is `ally` (player 1's team) or `enemy`; `behaviour` one of `passive`, `base`, `waves`, `skirmish`; a wave's `from` is `"trained"` or `{"region": "<id>"}`, its `target` `"base"`, `"nearest"` or `{"region": "<id>"}`; `until` (seconds) ends a wave line, and a later line can take over. `skirmish` takes `{"style", "armyCap", "pace", "income"}`. Players the map lists nowhere stay out of the match.

**Mission script additions** (Lua, the `mission` API): `campaign_get(name)`, `campaign_set(name, value)`; `allow(id)`, `forbid(id)` (tech, for the rest of the mission); `ai_waves(player, on)`, `ai_attack_now(player)`, `ai_target(player, region)`, `ai_behaviour(player, behaviour)`, `ai_wake(player)` (a dormant player arrives: its placements marked `activation: "script"` appear, then it plays its behaviour).

## Research: how Warcraft III and StarCraft II do it

- **Warcraft III.** A custom campaign is one `.w3n` archive made in the Campaign Editor: `war3campaign.w3f` (name, difficulty text, author, description, race, the menu's background scene and sound, and the mission buttons: chapter, title, visible at start, which map), the maps, and campaign-wide object data and imports every map shares. The Campaign Editor packages and lays out the menu; the logic lives in each map's triggers: `SetNextLevelBJ` stores the next map and `ChangeLevel` loads it after the score screen; `SetMissionAvailable` unlocks a menu button; heroes travel through a game cache (`StoreUnit`, `SaveGameCache`, `RestoreUnit`). Cutscenes are trigger scripts in the mission map itself: saved cameras, cinematic mode, transmissions (portrait, voice, subtitle), unit animations, fades and waits. Opponents run small campaign AI scripts: defenders kept at home, attack groups (`CampaignAttackerEx`) sent after a wait (`SuicideOnPlayerEx`), every number given for Easy, Normal and Hard.
- **StarCraft II.** No campaign container: layers of data (`Liberty` → `LibertyCampaign` → `LibertyStory`, and on for each episode), mission maps depending on the campaign layer, and between missions a playable hub that is itself a map (rooms, characters, conversations, upgrades). Progress lives in banks (named key-value files per player). Missions lean on a campaign script library (transmissions, objectives, heroes, the victory sequence, values per difficulty). The Cutscene Editor is a timeline studio (models, cameras, lights, sounds, dialogue lines, bookmarks the script reacts to and jumps between, filters for variants); conversations are data (lines, choices, conditions, states remembered with the progress).
- Sources: [jassdoc](https://lep.duckdns.org/jassbot/doc/ChangeLevel), [War3Net's `war3campaign.w3f`](https://github.com/Drake53/War3Net/blob/master/src/War3Net.Build.Core/Info/CampaignInfo.cs), [Hive: making a basic campaign](https://www.hiveworkshop.com/threads/making-a-basic-campaign.6287/), [SC2 Editor guides](https://s2editor-guides.readthedocs.io/New_Tutorials/01_Introduction/013_Standard_Dependencies/), [Galaxy API reference](https://mapster.talv.space/galaxy/reference), and the Heart of the Swarm campaign maps of a community mod on Jiri's machine, read with the StarCraft importer's archive reader.
