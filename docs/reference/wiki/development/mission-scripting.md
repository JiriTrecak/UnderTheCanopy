# Campaign missions and Lua

Campaign missions use the ordinary map, entity definitions, simulation, combat, pathfinding, renderers and save system. Lua supplies scenario decisions: objectives, dialogue, reinforcements, orders and victory. A mission does not create a Mound or default starting army, and does not use the skirmish Mound-defeat rule or economy AI.

## Authoring a mission

1. Open a map in the world editor. Choose **Mission & Lua** in the top file toolbar.
2. Enable **Campaign mission**. Set a campaign ID such as `woodland-story`, mission title, ordering, and an optional **Hero level cap** (1–10). Leave the cap empty to use the hero definition’s maximum. These fields determine the mission-selection entry; the map description supplies its briefing.
3. Place units using Place → Units & buildings. Select each entity and edit its **Script ID**. IDs are unique across the map. Letters, digits, dots, underscores and dashes are accepted by the rename control. Renaming updates camp membership; update corresponding Lua string references yourself.
4. For reinforcements, check **Spawn through Lua** on the placed entity. The editor still shows its intended location. Gameplay creates it only when the script calls `mission.spawn(id)`. Neutral creatures retain their authored camp, aggression and leash.
5. Enter named circular regions in the mission panel. Coordinates are map cells: `x`, `y` (the editor's world Z), and radius. Example: `[{"id":"crossing","x":121,"y":128,"radius":6}]`.
6. Write Lua, check syntax, then **Apply to map**. Save with the normal map controls. **Apply & play mission** stores the local map and opens a test match in another tab.

The script and regions live inside the `.utcmap` JSON `mission` field. There is no external script lookup at runtime.

Mission maps support one or more consecutive player starts, beginning at Player 1. Starts set owner slots and initial camera positions; their skirmish setup/Mound fields are ignored in missions. Player 1 is the local campaign player. Other slots are available for scripted actors, not skirmish AI. The current victory API targets Player 1.

## Script execution and saved state

`on_start()` runs on the first simulation tick. `on_tick()` runs every four ticks after that: 10 times per simulated second. The simulation runs at 40 ticks/second. Speed controls scale both together.

Each callback runs in a **fresh Lua VM**. Top-level code should define functions; the mission API is only callable inside the callback. Local variables, globals, tables and closures do not survive to the next callback. Store persistent booleans, numbers and strings through `mission.set`, and retrieve them with `mission.get`. This makes saves, replays and lockstep independent of hidden interpreter stacks or closures.

```lua
function on_start()
  mission.set("phase", "travel")
  mission.objective("Reach the crossing.")
end

function on_tick()
  if mission.get("phase") == "travel"
      and mission.in_region("marshal", "crossing") then
    mission.spawn("wolf-one")
    mission.attack("wolf-one", "marshal")
    mission.set("phase", "fight")
  elseif mission.get("phase") == "fight"
      and not mission.alive("wolf-one") then
    mission.win()
  end
end
```

For a timer, save a deadline such as `mission.tick() + 240`, then compare the current tick with it. That is a six-second simulation timer, preserved by saves and replay. Do not use wall-clock timers.

## API

- `mission.get(key)` / `mission.set(key, value)`: persistent scalar state. Setting `nil` removes the key. Maximum 128 keys; strings up to 2,000 characters.
- `mission.tick()`: authoritative simulation tick.
- `mission.count(owner, definition)`: living, completed, ready units/buildings of an exact definition. Foundations and fallen heroes do not count. Useful for training and construction objectives.
- `mission.stock(owner, item)`: available economy currency in the owner’s completed drop-off stores; carried cargo and unfinished stores do not count. This includes Root held at Rootworks. Both calls read authoritative simulation state and are deterministic.
- `mission.alive(id)`: true for an existing living entity; false before a deferred spawn or after death. Unknown authored IDs are errors.
- `mission.attack_move(id, x, y)`: move a military unit toward a map position, engaging visible enemies along the way. Use this for approaching reinforcements; a direct attack still obeys visibility and can end if its target is unseen.
- `mission.recover(id)`: restore a surviving unit's health and mana to its current maximum. Dead, missing or fallen members stay lost; items, cooldowns, orders and temporary effects remain unchanged. Call once from a saved stage transition for a recovery checkpoint.
- `mission.position(id)`: returns `x, y`, or `nil, nil` if the entity does not exist.
- `mission.in_region(id, regionId)`: tests a living entity's current cell against a named circle.
- `mission.spawn(id)`: activates a pre-authored entity marked `activation: "script"`. Each ID may spawn once. This permits preloading its model and validating definitions, ownership, camp membership and placement before a mission starts.
- `mission.move(id, x, y)`: submits an ordinary movement order through the native order system. Integer cell coordinates; bounds checked.
- `mission.attack(id, targetId)`: submits a forced attack order through the native order system.
- `mission.objective(text)`: replaces the displayed objective.
- `mission.say(speaker, portraitDefinition, text, seconds, cinematic, actorTag)`: shows portrait dialogue for 1–60 seconds at the current simulation speed. Omit `cinematic` (or use `false`) for inline speech while units keep moving. Use `true` for a cinematic pause: small black bars, the HUD slides out, and the camera lens eases in by 8%. Camera position, selection, and player zoom are retained and restored automatically. A new line replaces the current one. Optional `actorTag` identifies the living speaker and must match the portrait definition. Ant mandibles articulate with the text timing, including pauses at punctuation; without a tag, only a unique visible matching unit is animated. This is silent articulation, not voice or phoneme synchronization. Actor identity and duration survive save/load.
- `mission.win()` / `mission.lose()`: ends the mission with Player 1 victory or defeat.

Queries read the current simulation. Mutations are buffered in callback order, then applied after the callback succeeds. A spawn followed by an attack can reference that new entity. Queries within that callback still see the state before those buffered operations. Script errors discard the buffered operations and variable changes, stop further callbacks, and display the error above the battlefield.

## Determinism, boundaries and limits

Lua never receives a JavaScript object, native entity pointer, renderer, socket or unrestricted engine mutation API. Filesystem, network, `require`, JavaScript interop, `os`, `io`, `debug`, wall-clock time and random libraries are unavailable. The initial API deliberately exposes no standard libraries; Lua expressions, conditionals, functions and loops remain usable.

A callback is limited to 20,000 Lua instructions and 256 API calls. Strings in execution are checked against a 64 KiB limit every instruction, preventing accidental exponential string growth. Source is text-only and limited to 64,000 characters. These are practical authoring bounds, not a claim that the interpreter has undergone a hostile-code security audit.

Mission variables, spawn history, objective, dialogue deadline and errors are explicit simulation state, included in snapshots and checksums. Map revision includes the Lua source and regions. Save/load recreates interpreter execution from that source and saved state. Mission simulation tests compare checksums after restoring during the ambush.

The runtime can therefore execute the same script on every lockstep peer. Player commands still travel through the normal network command stream; script-generated orders are derived locally on each peer and must not be separately broadcast. Camera, portrait layout and presentation remain local. The current campaign launcher is single-player; campaign networking and synchronized dialogue choices/skipping need their own implementation. Authored chapter transfer is described below.

## Editor bridge

The existing editor control bridge supports `mission` with `action: "get"` or `action: "set"` and a complete `mission` object (`null` disables it). `entities` supports `action: "rename"`, `id`, `nextId`. Both use the same validated authoring transactions as the UI. Existing entity placement accepts `activation: "script"` on mission maps.

## Validation

Validate syntax errors, unavailable capabilities, instruction limits, deferred entities, victory/defeat, save/load continuation, rollback after script failure and Script ID renaming with a maintained mission fixture.

## Authored starting levels

A hero placement may set `initialState.experience`, for example `{"experience":450}` for a level-four Marshal. Health and mana initialize at that level, and unspent skill points remain available. Validation rejects XP on a non-hero or above the mission cap. The Entities panel’s **Instance initial state JSON** and normal entity bridge accept this field. Chapters with an explicit travelling-company connection can instead inherit survivors and hero development.

## Level limits and guaranteed rewards

`mission.heroLevelCap: 2` stops XP at the threshold **entering level 2**, not at the threshold for level 3. Excess XP is discarded. Level-up stats and skill points work normally; the HUD labels the mission maximum.

Camps accept either `lootPool` (the existing weighted random pool) or `fixedDrops` (an exact list of item definition IDs), never both. Example: `"fixedDrops": ["item.barkguard", "item.resin-salve"]`. The final defender drops every listed item once. Fixed drops consume no random rolls; normal ground-item pickup, inventory limits, and save/load still apply. An empty list gives no items. Existing legendary-camp restrictions apply to fixed T3 rewards too.

The **Camp rewards & encounters (JSON)** field in Mission & Lua edits these records together with mission metadata in one validated, undoable transaction. The editor MCP `mission` operation also accepts an optional complete `camps` array. Unknown/non-item drops are rejected before play.

## Cinematic time

Cinematics pause the **gameplay clock**, including movement, attack windups, effects, projectiles, cooldowns, regeneration, economy, and Lua `on_tick`. The outer match/transport clock continues consuming deterministic beats, decrementing a saved dialogue countdown. `pausedTicks` records their difference. This avoids shifting dozens of independent deadlines or letting buffs expire during a conversation. World saves validate both clocks, and the render snapshot follows gameplay time.

When the countdown finishes, Lua and gameplay resume. For consecutive cinematic lines, advance a saved stage and issue the next line in the next callback; a cinematic itself consumes no `mission.tick()` time. Use gameplay deadlines only for **inline** dialogue. The player cannot issue orders during a cinematic; orders from before it resume. Presentation transitions remain local. There is no skip/branch-choice command in this pass.

## Multi-stage objectives

Declare `mission.objectives` as an array of `{id, title, description, optional}` records. Start a stage with `mission.begin_objective(id)`, finish it with `mission.complete_objective(id)`, or mark failure with `mission.fail_objective(id)`. Only active objectives can finish; a completed/failed objective cannot restart. Complete the current primary objective before beginning the next. Optional objectives can remain active alongside it.

The HUD displays active stages and a collapsible completed-objective history. Future stages remain hidden until started. IDs and statuses are persisted, validated on restore, and included in checksums. The Mission & Lua panel exposes the declarations as **Objective stages (JSON)**; the editor MCP mission operation uses the same schema. `mission.objective(text)` remains available for dynamic mission notices and failure explanations.

```lua
mission.complete_objective("find-watch")
mission.begin_objective("rescue-watch")
```

## Rescuing and transferring units

`mission.transfer(entityId, "player.1")` changes a surviving military unit's ownership. It preserves the entity ID, position, health, progression and equipment, but clears its previous orders. The ordinary ownership-based rendering, selection, command permissions and visibility then apply. Destination owners must be declared map slots (or `"none"`). This initial transfer API supports non-camp military units; workers, buildings, camp creatures and contained units are rejected. Transfers are buffered with other Lua operations and work after a spawn in the same callback. Dead units are not resurrected.

## Local game menu and save boundaries

`GameMenu` is shared by campaign and skirmish. A local save envelope (version 4) records `mode: "campaign" | "skirmish"`, the full frozen `match` configuration, and the nullable local `player` slot (`null` means observer), alongside the world, mailbox pipeline, and control groups. This version is independent of the network save envelope. The map's `mission` declaration determines its mode; changing only the save's flag cannot bypass filtering.

`SaveLibrary` stores named snapshots atomically in IndexedDB, avoiding localStorage's quota for large worlds. Lists and imports are mode-scoped. Loading checks map revision and player assignment, then preflights world and pipeline restoration before destroying the running screen. Restart creates a fresh session from the original match configuration, without the saved world's progress. Local menu pause stops both gameplay and transport ticks; campaign cinematic countdowns therefore remain frozen while making a save.

## Moving cinematic scenes

Use `mission.begin_scene(x, y)` to pin the camera to a ground point, show cinematic bars, hide the HUD, and lock player orders **while simulation and scripted movement continue**. Call `mission.end_scene()` to release the camera and controls; the camera stays at the scene location. F10 remains available, and its local menu pauses the scene.

- `mission.arrived(id, x, y)` checks that a living unit has finished its order and is within half a cell of the destination. Wait for actual arrival instead of guessing travel time.
- `mission.face(id, x, y)` interrupts the unit's order and smoothly turns it toward the point using its normal declared turn rate.
- `mission.facing_done()` is true when all scripted turns have completed or been superseded.

Scene framing, pending turns, and Lua stages survive saves; unit navigation and animation use the normal engine systems. Do not start a paused dialogue until the pending movement and turns finish. The presentation temporarily reveals a 24-cell circle around the scene camera. This does not explore the map, grant simulation/AI vision, or alter combat rules; normal fog returns at scene end.

Rain is authored in `landscape.environment.weather` (`kind: "rain"`, intensity `0.7`, wind X `1.2`, Z `0.4`). It uses the existing bounded, instanced weather renderer. The shared default day/night period is 600 seconds, including editor previews and ordinary matches.


## Travelling between chapters

A mission may declare `nextMission: "woodland-chapter-two"` and a `company` list of entity Script IDs. The destination declares the matching company IDs and unit definitions at its arrival positions. The Mission & Lua editor exposes both fields; the complete mission MCP declaration uses the same schema.

After victory, **Continue to next chapter** starts that destination. Only living Player 1 units in the source company travel. Missing companions are removed from the destination's default party, so death is not silently undone. The receiving company must contain a living hero. The destination must belong to the same campaign, and its hero cap must accommodate the incoming XP.

The chapter checkpoint retains hero experience, learned ability ranks, inventory slot order and remaining item charges. Survivors recover health and mana. Combat orders, surface IDs, temporary effects, cooldowns and partial item-trigger counters reset. The destination supplies positions and facing; no coordinates from the previous map are reused. Colony research and resource stockpiles do not transfer in this first travelling-party implementation.

The incoming company is part of the frozen `MatchConfig`, not browser-only state. It survives local saves and **Restart scenario**, which recreates the original chapter-start company. Validation rejects unknown arrival IDs, mismatched unit definitions, invalid equipment or skill ranks, and incompatible caps. The simulation can construct identical starts from the same declaration on multiple peers; a network campaign lobby/transition protocol is still unimplemented.


## First- and third-person shots

`mission.camera(mode, subjectId, lookAtId, distance, height, fov, transitionSeconds)` starts a scene attached to a named unit. Modes are `rts`, `third-person` and `first-person`; optional arguments can be `nil`. A look-at ID points toward another unit. Call `mission.end_scene()` in the next stage to release control and restore the player's camera.

```lua
mission.camera('first-person', 'archer-1', 'marshal', nil, 1.5, 55, 0.6)
mission.say('Marshal', 'unit.ants.marshal', 'Keep to the amber lights.', 10, true)
```

Shots follow rendered movement and raised walk surfaces. Their declarations are validated and saved; interpolation stays local. The editor's Lua reference lists the parameters and ranges. Player-controlled views use the spyglass command or **J**, with **Escape** returning to RTS.

## Inventory exchanges, escorts, and destructibles

Reusable quest operations:

- `mission.has_item(heroId, definition)` checks carried equipment. `mission.take_item` removes one copy and returns whether it succeeded. `mission.give_item` returns false when the inventory is full; it never overwrites a slot or silently drops a reward. These calls see earlier inventory changes in the same callback and roll back when callback validation fails. Keep a quest active until its reward is delivered.
- `mission.follow(npcId, leaderId)` creates a scripted escort. It supports a neutral civilian following a player hero, even across the civilian's lack of a combat sensor. It does not give the player command permission. Ordinary player Follow remains restricted to friendly visible units.
- `mission.transform(id, definition, owner, optionalCamp)` changes an ordinary unit while preserving its identity, location and health fraction. It clears old orders and role state. Heroes, inventories, casters and contained units cannot be transformed. A camp-creature target requires an existing, uncleared camp and neutral ownership. This supports a disguised civilian revealing itself as a hostile bandit, with normal camp combat and loot.
- `mission.damage(id, amount, optionalSource)` queues integer damage through ordinary combat destruction on the next unpaused tick. It preserves death cues, cleanup and fixed loot, and the queued damage survives saving. It is appropriate for a collapsing cottage; use ordinary attack orders for fights.
- `mission.dialogue_busy()` prevents independent encounters from overwriting active speech. Cinematic countdowns and inline dialogue deadlines are both respected.

Deferred placements may overlap initial placements, allowing a ruin to replace a destroyed cottage. Bounds are still checked. The author must spawn the replacement only after the original obstruction is gone; initial buildings still cannot overlap.

Consumable item definitions can declare `permanent: {maxHp, damage, armor, maxMana}`. Optional `onPickup: true` applies such a power-up immediately, even with a full satchel. Bonuses are saved on hero progression, survive death, and travel with the campaign company. Ordinary equipment still occupies a slot.

### Mission selection and loading artwork

Optional `mission.presentation` controls the chapter page and the loading screen:

```json
{
  "loadingBackground": "asset.interface.campaign.amber-watch-loading",
  "selectionBackground": "asset.interface.campaign.amber-watch-selection",
  "chapter": "Chapter One",
  "briefing": "The watchfires have gone silent.\nRally the scattered guard and break the siege.",
  "tips": ["Attack-move to advance as a company.", "Explore side paths for supplies."],
  "features": ["One hero", "Allies along the road", "Break the siege"]
}
```

Images reference published canonical **interface** assets with an `image.webp` resource. They are bundled for offline play; external URLs are rejected. Missing artwork uses the campaign forest fallback. Titles, briefings, tips and progress remain live accessible UI; do not bake them into the illustration. The loading bar displays real asset counts during model loading and becomes indeterminate during terrain/shader preparation. Tips rotate every nine seconds while loading. Errors retain a Back to menu action.

The editor’s **Mission & Lua → Mission artwork & loading screen** fields edit these values. Briefings preserve line breaks; enter tips and selection features one per line. Existing maps can omit presentation entirely.

**The Amber Watch** is the first Queensguard chapter, authored on a 128 × 128 building-cell woodland map. A single Marshal and three escorts follow a lantern road, recruit two scattered patrols, optionally explore three neutral camps, cross a walkable twig bridge and relieve a neutral ant outpost. Lua camera shots and cinematic dialogue provide letterbox scenes. Victory requires all nine siege units and all three members of the incoming relief column to be defeated; Rowan’s death loses the mission. The hero is capped at level three.
