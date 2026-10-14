# RTS controls and input bindings

## Design decisions

The control set follows established classic RTS conventions: numbered groups, double-press camera focus, on-screen same-type selection, subgroup cycling, hero keys, idle-worker and town-hall cycling, recent-alert jumps, inventory keys, and queued orders.

The distinction between Move, Stop, Hold Position and Patrol matters mechanically: Move avoids automatic engagement; Stop permits subsequent acquisition; Hold attacks without pursuit; Patrol resumes its route after combat.

We retain the fixed game-camera orientation, existing momentum zoom, army-first box selection, worker-first construction assignment, and 16 pending orders. Selection has no twelve-unit cap. Enter remains reserved for chat. Editor tools retain their existing gestures. Game settings do not pause a multiplayer match.

This is an RTS control implementation, not a reproduction of campaign, transport, allied-unit sharing, replay, formation-speed synchronization, or siege attack-ground mechanics that our game does not yet expose. Subgroup special commands follow the focused type; ordinary movement and combat orders still affect the eligible selection. A Ctrl-only subgroup-order mode is not included in this pass.

## Local input architecture

`src/shared/input/shortcuts.ts` owns stable binding IDs, normalized physical key chords, browser storage and change notifications. Saved overrides affect presentation only; they never change simulation definitions or checksums. Storage failures fall back to in-memory settings.

`catalog.ts` derives command defaults from action/category/override, building recruitment, research and spell declarations. The same binding ID resolves keyboard activation, command icon labels and tooltips. Global input actions cover camera, hero selection, group assignment, inventory, debug and settings. Native browser/OS-reserved shortcuts may be intercepted before the game receives them; the settings provide alternatives.

Both menu and in-match settings expose a searchable editor, capture, clear and reset. Rebinding rejects occupied keys instead of silently stealing another command. Authored contextual duplicates remain valid on separate cards. Shift additionally permits queued command targeting; a deliberately shifted binding takes precedence. Typing, open dialogs, chat, blur and repeat handling isolate input from gameplay. Camera key release uses the physical key that began the action, even if modifiers were released first.

`commandShortcut` resolves exact chords across the available card before trying the implicit Shift queue fallback. This prevents, for example, an explicitly bound Shift+A Hold from being swallowed by Attack's ordinary A binding. Held health inspection uses `HeldShortcuts`: Alt for all, brackets for friendly/enemy. Both physical Alt keys work, bindings persist, and releasing either of two held Alt keys does not prematurely end the other. Health visibility is derived solely from `EntityView`; remembered, contained and dead entities never gain health bars through this shortcut. Observers can use Alt; friendly/enemy filtering requires an observed allegiance.

## Control groups and selection

`ControlGroups` stores ten lists of stable entity IDs in match-local presentation state. Assignment replaces; addition unions without duplicates; recall replaces selection; a second recall within 350 ms focuses the visible group centroid. Buttons above the HUD show effective keys, membership counts and current selection, and expose the same mouse operations through Ctrl/Shift clicks.

Only owned controllable entities enter groups. Removed/dead entities are pruned. Contained units cannot become an active selection. Local save files carry group membership separately from world state; loading resets selection and alert history, then reconciles groups against restored observations. Starting another match constructs a fresh group store.

F1–F3 use stable hero creation order. F8/backquote cycle truly idle Workers: no employment, cargo, queued order, job, release, stun or pending move. Backspace cycles owned drop-off buildings. Home focuses the selection, falling back to the objective Mound. Space cycles up to eight locally observed attack, completion and spawn locations, with nearby repeated damage coalesced. It never reads hidden enemy state.

Tab/Shift-Tab cycle definition subgroups while retaining the full selection. The focused type supplies construction and special abilities; compatible basic commands still apply to the whole group. Ctrl-click/double-click selects matching owned units on screen. Shift-click retains its add/remove behavior.

## Simulation commands

Hold, Patrol and Follow are schema-validated actions and saved `UnitOrder` variants. They use the existing ownership, capability, interrupt, queue and command transport paths. No UI-only movement or damage occurs.

- **Hold:** clear movement; acquire only within weapon range; never chase. It remains active until replaced. Workers held in place are not available for automatic work or wandering.
- **Patrol:** record the departure point, travel to the destination, reverse on arrival, engage perceived enemies and resume. Unreachable endpoints cancel with the ordinary path notice. The origin is established when a queued patrol starts, not when its packet arrives.
- **Follow:** maintain proximity to a visible friendly unit and replan as it moves; cancel when its leader becomes unavailable. This version follows without automatically adopting the leader's attack order.

Shift appends these commands. Patrol, Follow and Hold are continuous intentions, so later queued orders wait until the active intention ends or is replaced. Existing cargo delivery rules still apply to Workers. Stop remains the immediate clear-order operation.

Right-click world commands now submit on pointer-down, once per click, without waiting for release. The minimap supports right-click Move and explicit Move, Attack-move, Patrol and Rally targeting. All use the same authoritative action channel. Camera, group selection and UI changes are local; gameplay actions retain multiplayer validation.

## Validation

Automated coverage includes group replacement/add/prune/double-tap timing, persisted/cleared/corrupt bindings, physical-key release with changed modifiers, dialog isolation, Shift targeting, catalog IDs, hold range and stationary behavior, patrol endpoints and save replay, follow targeting/ownership, queued Hold, malformed commands and pointer-down dispatch. The complete suite and production build are run for the pass.

Browser checks exercise hero selection, worker cycling, group assignment/addition/recall, subgroup card changes, chat isolation, conflict rejection, and changing Move's key with immediate icon and targeting updates. The player guide contains the practical default-key reference.

The latest integration audit reconfirmed the primary-source reference above and the live Settings search/capture flow. F1 selected the Marshal; Ctrl+1 assigned it; F8 then Shift+1 added a Worker. Recall retained both members and Tab changed the active command card. Rebinding group 1 to J immediately updated its HUD label and recalled both members. Trying to assign Move to J reported the occupied group key. Typing A, 1 and F8 inside chat did not issue gameplay commands. Health inspection was rebound to J and back to Alt through Settings, including modifier-only capture on release. Temporary bindings were restored individually. The group strip is positioned above the center panel clear of the clock. These controls are implemented within the ongoing combat goal; this does not imply that dense army pathing is finished.

Validation for this audit: **647 tests in 155 files pass**, including shortcut persistence and isolation, control-group ownership/save handling, authoritative Hold/Patrol/Follow, command targeting and multiplayer/socket regressions. `npm run build` passes. The renderer still emits its existing large-chunk advisory.


## Command-card presentation

Action priorities in `content/game.json` put Move, Attack, Stop and Hold in the first row, followed by Patrol and Follow. Every command has its own asset reference; the three new painted icons are 128×128 PNGs. The HTML adapter renders twelve square wells using the same CSS grid tracks as the buttons, covering the obsolete slots in the decorative frame. Back remains reserved at row 3, column 1.

Category metadata accepts optional `placement: "banner"`. The hero learning category uses it for **New spell available**, a full-width fourth row outside the twelve paginated slots. The label, icon and shortcut are declared alongside the category; the adapter has no hero-specific button label or key. The banner remains keyboard-accessible through the normal command resolver.

Spell declarations use `placement: "bottom-row"` to reserve slots 9–12 for learned cast commands, independent of the number of movement controls. Each spell declares a `column` from 1 to 4, so learning other spells never shifts its position. The registry rejects duplicate columns within a hero’s ability set. The generic card layout respects reserved rows and pagination; Back still owns slot 9 in submenus. Learning entries keep their normal submenu layout. The presentation model omits unlearned cast commands. Learn commands populate the learning category only while skill points remain; learning returns to the root card. Locked higher ranks still describe their level requirement in the learning submenu. No simulation commands or ability costs change.

World health bars use a continuous red–amber–green fill, subtle vertical shading,
and separators at each 100 maximum HP. A 400-HP bar therefore has four sections;
partial final sections retain their true size. Mana appears below only when the
observed maximum mana is positive. Bars remain 52 CSS pixels wide for units and
76 for buildings across camera zoom, projection and model scale. Extremely dense
subpixel HP dividers fade to preserve readability. The renderer updates shared
shader code through per-entity uniforms without generating health-state textures.
