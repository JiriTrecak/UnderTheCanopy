# Playing a match

Status: M4, in-match part (2026-10-15, [D36](decisions.md), joined with the screens in D39). Matches start from the menus (skirmish, campaign, multiplayer, loaded saves); `just play --map <id> [--players N]` jumps through the flow straight into a skirmish: you on the map's first start, computer players on the next `N − 1` (all starts by default). `just play --spectate <id>` watches computer players with the observer HUD. The old game (`src/session/session/session.ts`, `src/ui/settlement/*`, `src/presentation/*`) is the behaviour reference.

## Shape

Everything lives in `game/src/play/`:

| Module | What |
|---|---|
| `run.rs` | `InMatch`: a running match (simulation thread, live scene, camera, controller, HUD, debug overlay, effect sounds). API for the screens' `Playing` (`screens/in_match.rs`), which own the window, the one `Ui` and the one `Audio` (lent through `Host`): `start(gpu, renderer, ui, audio, loaded, match, Start)`, `set_options` (shortcuts, health bars), `window_event(window, event, ui, consumed)`, `update(host, free)` before the UI lays out, `act(ui, hud actions)` → `Vec<Event>` (`OpenMenu`, `Chat`), `decorate(draw list)`, `render(host, output, ui_view)`, `world_ui()`, `plug_render_target("@minimap" \| "@portrait", …)`. |
| `keys.rs` | `Keymap`: the engine's shortcuts from the player's rebindable catalogue (`settings/keys.rs`). |
| `mod.rs` | `Play`: the controller. Gets the seat's newest view, the frame's camera and pickable bodies, and window input; produces protocol `Action`s through a `Sink` (the session's `Commands` handle; a list in tests), camera requests, the HUD's contents and `WorldUi`. Holds no simulation state. |
| `know.rs` | `Knowledge`: the view plus content, with the questions input asks (own, controlled, selectable, rank). |
| `selection.rs` | Selection order and focus, box-selection rules, Tab cycling, control groups. |
| `orders.rs` | Right-click smart orders, targeting modes, rally, acknowledgement markers. |
| `card.rs` | The command card model: commands, costs, reasons, cooldowns, menus, the 4 × 3 layout, hotkey resolution (each command's chord from the player's bindings). |
| `follow.rs` | Unit cameras (J): third- and first-person poses of the focused unit, the terrain-clear boom, the 350 ms blend. |
| `placement.rs` | Building placement: lattice snap, quarter turns, validity from knowledge. |
| `picking.rs` | CPU picking: unit capsules, the ground under the cursor, box tests. |
| `hud.rs` | Binds `Play` to `config/ui/layouts/hud.json` and maps UI interactions back. |
| `overlay.rs` | The drag box (everything else in the world is the renderer's world UI pass). |
| `debug.rs` | The egui debug overlay. |
| `capture.rs` | `canopy shot --play`: offscreen captures with the HUD after a scripted input. |
| `tests.rs` | The headless input harness. |

Commands are only `Action`s sent through the session (D10); the HUD never touches simulation state. The simulation thread presents the seat's view (`sim::Seat`), so the player sees through fog and the observer sees everything. Snapshots carry the view (`Arc<View>`), the seat's held units (from the view's private `contained` and `releasing` flags), AI summaries when the debug overlay asks, and every colony for the observer panel.

## Controls

Default keys follow the content (`content/game.json` `actions`, categories and navigation) and the old shortcut catalogue. Keys are physical (the old `KeyboardEvent.code`). The engine's keys below (camera, menu, cancel, heroes, groups, selection, placement, inventory, health holds, debug, hide HUD) follow the player's bindings (Settings → Keyboard, `play/keys.rs`, D39); the table lists the defaults. Command hotkeys come from the content and are matched after them.

| Input | Does | Parity |
|---|---|---|
| Left click | Selects the entity under the cursor (unit capsule ≥ 28 px wide, nearest wins; else the building, item or resource whose footprint holds the ground point). Ground clears (Shift keeps). Enemies, neutrals and buildings select alone. | as old |
| Shift + click | Toggles an own unit in the selection. | as old |
| Ctrl + click, double click (350 ms, 6 px) | Every own unit of that type on screen (Shift adds). | as old |
| Left drag (≥ 5 px) | Box: own controlled units; heroes and army first, else workers; observers get every unit. Shift adds. | as old |
| Right click (on press) | Smart order, first rule that applies: resume an own construction site (builders) → enter an own lookout → follow an own unit → gather a resource (harvesters) → pick up an item (first unit with an inventory) → army attacks an enemy or a neutral creature → move. Producers alone: rally (right-click the producer clears it). An enemy in a lookout is attacked through the tower. Any mode: cancels it. | allies are not attacked by right-click (old did; D36) |
| Shift + order | Appends (`append: true`); Shift keeps placing buildings. | as old |
| M / A / S / H / P | Move, attack (target: forced attack; ground: attack-move), stop, hold, patrol. Reachable from any submenu. | as old |
| B / V / K | Build, advanced build, the hero's "New spell available" banner. | as old |
| Q W E R … | Spell hotkeys from the content bindings. | as old |
| Escape | Cancels the mode, else Back, else opens the game menu. | old: F10 only for the menu (D36) |
| R / Shift+R | Rotates the placement a quarter turn / three quarters. | as old |
| Ctrl+1…0 (or Cmd), Shift+1…0, 1…0 | Assign, add to, recall a control group; a second recall within 350 ms centres the camera. | Cmd also assigns on macOS (D36) |
| F1–F3 | Select hero n; a second press within 350 ms centres it. | as old |
| F8, ` | Next idle worker (selects and centres). | as old |
| Backspace | Centres on the next drop-off building. | as old |
| Space | Cycles the last 8 alerts (own damage, completion, births; merged within 120 ticks and 10 cells). | as old |
| Tab / Shift+Tab | Focus the next / previous unit type in the selection. | as old |
| Home | Centres on the selection, else the main building. | as old |
| Numpad 7, 8, 4, 5 | Use inventory slots 1–4. | as old |
| Alt, `[`, `]` (held) | Health bars for all, friendly, hostile (`WorldUi::health`). | as old |
| Enter | Chat (Enter sends, Escape closes; 300 characters). Single player shows the line at once; multiplayer sends it to the match server, which delivers it to every seat (D39). | as old |
| F10, Menu button | Opens the game menu (`Event::OpenMenu`; the screens pause single player). | as old |
| F9 (Ctrl+F3) | Debug overlay. | old F3 |
| Arrows, screen edges, wheel, PageUp/PageDown, middle drag | Camera (unchanged from M3). | as old |
| Q / E, B (spectating) | Turn the view; jump to the largest fight. | new (not in the old game; fixed keys) |
| J (the content's camera command) | Cycles the unit camera: RTS → third person → first person, locked to the focused unit (offered while it is a visible, living unit outside buildings, for observers too). Pan and zoom are ignored while following; Escape, a camera jump (alerts, double presses, the minimap) or losing the unit returns to RTS. | as old |
| Minimap | Left click/drag moves the camera; right click moves the selection or completes move, attack, patrol and rally modes. | as old |

Disabled commands show their reason as the red message when pressed by hotkey; the HUD greys them (a disabled button takes no click in `canopy-ui`; the old HUD let the click show the reason).

Command hotkeys are entries of the rebindable catalogue too (`command.<id>`, Settings → Keyboard, with search), defaulting to the content's keys; the card, the hints, the group buttons, the hero bar and the inventory show the player's keys. During a mission cinematic the world takes no keys or clicks except the game menu (F10 or Escape), selection rings hide, and the camera looks at the scene, returning afterwards. The cursor is a crosshair while aiming an attack, a hand over the minimap.

## HUD bindings

`hud.json` (root `hud`) and templates in `common.json` and `play.json`; styles appended to `theme.json`.

- **Resource ribbon**: each drop-off currency by `displayOrder` with `available`, tooltip available/reserved/stored/carried; supply `committed/capacity` (red over capacity) with the old tooltip.
- **Hero bar**: own heroes (fallen included) by id: icon, health ramp, F-key, selected state, availability tooltips; click selects, double click centres.
- **Selection panel**: focus name, portrait (`@portrait` when plugged, else the icon), level badge, experience ring and tooltip, damage and armour stats with the old tooltips, item status badges above the bars (auras, active item effects, rescue: modifiers and time left), health and mana bars, the 2 × 2 inventory (charges, cooldown seconds, use / right-click drop), status line of workplaces and mines, the active task (upgrade, training, research, revival, construction) with progress and the waiting wells (click cancels), the focused unit's queued orders. Two or more selected: unit cards (icon, health; click focuses, double click isolates, Shift removes).
- **Command card**: 12 cells (empty ones stay as wells), Back in slot 9, bottom-row spells at their columns, banners below, pager when a menu has more pages, icons, hotkey labels, enabled state, cooldown sweep and seconds, autocast (checked; right-click toggles), tooltips (title and key, costs, description, reason, eligible actor count, autocast note).
- **Messages**: the targeting hint, error facts (newest unseen, under 5 ticks old) and HUD errors in red for 2.5 s then a 0.3 s fade, the outcome (Victory, Defeat, Draw).
- **Control groups**: one column per group (1–0, as the old grid), a button with the bound key over the count for non-empty groups; click recalls, Ctrl assigns, Shift adds.
- **Chat log**: lines fade after 10 s; open chat shows the history and the input.
- **Observer panel** (spectating): per player colour, stored goods with income per minute (gross deliveries over 60 s of game time), units, workers, hero level or fallen.
- **Plug-in points**: the minimap and portrait are image nodes named `@minimap` and `@portrait`; `InMatch::plug_render_target` binds a texture view to them. The game menu is `Event::OpenMenu`.

## Placement

The ghost snaps the rotated footprint's edges to the 4-cell building lattice (origin −0.5), turns in quarter turns, and is checked every frame **from the player's knowledge**: the simulation's foundation rules (`economy::construction::can_build`, in its order) applied to the view — explored cells, known buildings (seen or remembered), known resources and scenery with their collision discs, ground units in sight, terrain walkability, decks, water, relief, the doorway, prerequisites, the building limit and spendable goods. It never reads simulation state, so it can neither desync nor reveal hidden buildings; the click sends a plain `build` action and the simulation may still refuse it (its error fact shows). A test sweeps 1,444 sites and rotations around a base and requires the ghost to agree with `World::can_build` wherever the player sees. The old game asked the authoritative world on its worker, which leaked blockers through the ghost colour (D36).

The ghost model is the building's live model at 0.48 opacity in the owner's colour, tinted 70 % toward `#f26960` when invalid; the footprint cells (white or red) and the entrance marker come from `Ghost` in `WorldUi`.

## World UI contract

`WorldUi` (from `Play::world_ui`, `InMatch::world_ui`) is the plain input the renderer's world UI pass reads: selected and hovered ids, the health bar mode, order markers with their ages (white move ×1, red attack ×2, green gather ×2; 0.4 s pulses, 0.15 s gaps, at most 24), the placement ghost (centre, rotation, cells, entrance, problem) and definition, the drag box and the focused producer's rally point. `run.rs` converts it each frame (`world_ui_input`) for the world UI pass (D40), which draws the rings, markers, placement grid, rally points and health bars; `overlay.rs` draws only the drag box. The minimap and the live portrait plug into the HUD as render targets.

## Debug overlay

egui (debug only): frames per second, frame time, render CPU, live-scene and UI time, draws and actors, per-pass CPU and GPU timings, the simulation's step/view/present cost per tick, slowdowns and late frames, AI summaries (each computer player's mission, plan and latest decisions; the simulation thread computes them only while the section is open), the commands sent, and switches for contact shadows, bloom, haze quality and render scale. F9 or Ctrl+F3 toggles; `CANOPY_DEBUG_OVERLAY=1` opens it at start.

## Sound

The old game had no interface or acknowledgement sounds; the match feeds every new view to `canopy-audio` for effect sounds (listener at the camera focus).

## Tests and captures

- `play::tests`: a real match on the simulation's test fixture, the player's view, a synthetic camera and synthesized window input; asserts the actions produced and that the simulation accepts them (selection, box rules, smart orders, Shift queueing, groups and hero keys, attack-move and modes, placement, training and rally, learning and casting, ghost agreement).
- Unit tests: selection order, box rules, Tab, groups; card layout and shortcut resolution; picking.
- `canopy shot --map <id> --play --tick N --input "select:settler;key:B;cmd:build:building.ants.house;move:1300,600" --out x.png`: the match from player 1's seat with the HUD after a script (`--observe` for the spectator HUD; steps in `play/capture.rs`). It also prints how the simulation judges the ghost's site. Captures: `docs/captures/m4-play/`.

## Not yet

See [parity](parity.md#what-remains): the third-person boom against scenery, scripted cinematic unit shots, the spell aim as the old filled outline (it shows as ground rings: the area in the old valid or invalid colour and a faint range ring), the observer panel's icons.
