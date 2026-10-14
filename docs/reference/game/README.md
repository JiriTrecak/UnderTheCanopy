# Game

The map catalogue is being replaced for the [competitive spatial contract](../expansion/competitive-foundation.md). Skirmish selects the map and each player’s Human/AI controller. Taking Player 2 changes your start and authority; all-AI games open observer mode. No campaign maps are currently shipped. Definitions and setup values come from `content/game.json`.

Each starting setup supplies a Main Hall, five workers, the chosen starting hero (currently the Marshal), and the resources declared by `rules.startingSetup`. All five workers start gathering amber, spread across the available nodes. A label above each Hall shows assigned Amber workers against the nearby nodes’ recommended capacity (normally 5/10). Workers carry harvests directly home. There are no production chains or economic ground stacks. Ranked trees contain 50 wood and do not regrow. Buildings can be placed on any explored legal ground; territory ownership and border posts have been removed.

The Main Hall trains workers and provides 15 supply. Completed Mounds provide 8 more, up to a colony maximum of 100. Workers and soldiers are trained directly; workers are never exchanged for military units. See [the economy contract](economy.md) for supply, funding and cancellation rules. All balance values live in `content/game.json`.

Barracks train new Warriors and Archers without claiming economic workers. Both cost 1 supply. Buildings and units have HP; repairs cost worker time but no resources. Losing every living building, including foundations, defeats a colony; losing its Hall alone does not.

Neutral camps aggro and leash home. They drop selectable hero loot. The Marshal levels to ten, learns three abilities and an ultimate, and preserves inventory and progression through death. A sanctuary revives that same hero. Ability targeting previews share their geometry with damage resolution.

## Controls

Click to select; Shift adds/removes. Drag selection favors controllable soldiers, then workers when no soldiers are in the area. Right-click ground to move; right-click a hostile unit/building to attack. Right-click a tree or neutral amber mine with workers selected to gather. S stops and releases their assignment; a carried load is returned before the worker becomes available. Left-click selects or confirms an explicit command. Right-click cancels active targeting. A followed by ground issues attack-move; A followed by a visible damageable entity forces an attack, including a friendly one. Edge-pan or arrow keys move the camera; Home returns to your hall. WASD does not pan.

Build menus expose the structures currently declared in the command configuration; definitions hidden from those menus can still exist in the content registry. R rotates placement; Shift+R reverses it. Placement retains the workers' selection. Escape cancels targeting, then navigates back. Command slots fill left-to-right across four columns and three rows; submenu Back occupies slot nine. Tooltips show declared names, descriptions, costs, shortcuts and unavailable reasons.

Selection cards show HP: click to focus, double-click to isolate, Shift-click to remove. A focused barracks exposes recruitment, rally, pause and its stable queue. Hall inspection shows worker training; Mounds provide supply. Node labels show assigned workers, with recommended staffing in inspection. One miner extracts while other arrivals wait; there is no assignment cap.

Resource counters show available amber and wood; tooltips distinguish stored, reserved and carried quantities. The supply counter shows committed supply/capacity; its tooltip separates living units and queued reservations. Observer mode shows each player's income, bank, units, workers and hero level.

F3 exposes performance scopes, AI decisions, visual-only fog reveal and local match speeds 1×–4×. Settings persist rendering resolution. Save downloads a `.utcsave`; Load restores the same map/content revision, including pending commands, training progress and supply reservations. Incompatible previous formats are intentionally unsupported.

## Authoring

Editor → Place → Units & buildings places units, buildings, hero items and resources with explicit owners. Amber mines are neutral buildings; currencies cannot be placed as loose entities. Select edits placement/initial state. Spawn moves the whole declared setup. Entity undo restores entities, camps and spawns while retaining unrelated terrain/lighting changes. New/Save/Load operate on `.utcmap` JSON. Edit definitions opens the shared content graph; saving requires the development server.

The HTML HUD and Three.js scene consume observation data. Fog hides live enemy activity and preserves last-seen static information. The deterministic engine and lockstep transport are shared by local and multiplayer games. See [implementation contracts](../declarations/README.md) and [art direction](art.md).
