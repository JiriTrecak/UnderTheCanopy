# Parity with the old game

Phase 1 ends when this checklist is complete ([plan](plan.md)). It lists every feature a player or spectator of the old game (`../Settlers 3 Web`, commit `a6f0b625`) can see or use in a match and around it. The list was built from the old code, not from memory: `src/session/session/session.ts`, `src/ui/**` (settlement HUD, hero bar, tooltips, chat, observer panel, menus, game menu, settings, keyboard controls, mission HUD, match result, loading), `src/presentation/*`, `src/shared/input/*` (shortcut catalogue), `src/shared/settings/*`, `src/shared/save/*`, `src/app/game/*`, `src/render/{camera,input,minimap,portrait,settlement,visibility,sky,audio,abilities}`. Simulation rules (combat, economy, abilities, AI) are covered by D21–D27 and their tests; here they appear only where the player sees them.

Audited 2026-10-15 on `m-audit` after merging M3.5 (D40). Statuses:

- **done**: behaves and reads as in the old game (differences listed are agreed decisions).
- **partial**: present, with a named gap.
- **missing**: not in the new game.
- **dropped**: left out on purpose (phase 2 or a decision); not counted as a gap.
- **new**: not in the old game.

Evidence names a test (`play::tests::…`, module tests), a capture (`docs/captures/…`) or a code path.

## Summary

| Group | done | partial | missing | dropped | new |
|---|---|---|---|---|---|
| Match flow | 9 | 1 | 0 | 0 | 1 |
| Camera | 8 | 1 | 0 | 0 | 1 |
| Selection | 11 | 0 | 0 | 0 | 0 |
| Orders and command card | 17 | 0 | 0 | 0 | 0 |
| Building | 6 | 0 | 0 | 0 | 0 |
| Economy feedback | 10 | 0 | 0 | 0 | 0 |
| Heroes, items and spells UI | 13 | 0 | 0 | 0 | 0 |
| Combat feedback | 7 | 0 | 0 | 0 | 0 |
| Fog and minimap | 8 | 1 | 0 | 0 | 0 |
| HUD panels and tooltips | 15 | 0 | 0 | 0 | 1 |
| Chat, messages and alerts | 7 | 0 | 0 | 0 | 0 |
| Menus and screens | 22 | 1 | 0 | 3 | 1 |
| Settings | 12 | 1 | 0 | 0 | 0 |
| Saves | 8 | 0 | 0 | 0 | 2 |
| Multiplayer | 10 | 0 | 0 | 2 | 1 |
| Campaign and missions | 8 | 0 | 0 | 0 | 1 |
| Audio | 4 | 0 | 0 | 0 | 0 |
| Visuals | 19 | 1 | 0 | 4 | 0 |
| Debug tools | 2 | 1 | 0 | 0 | 0 |
| **Total** | **196** | **7** | **0** | **9** | **8** |

Nothing a player notices first is still open; the remaining rows are small. Owners and next steps are in [What remains](#what-remains).

## Match flow

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Skirmish from the menu: map, slots, start; loading; the match; the result; back | `GameApp.play` | done | `screens::flow` tests, `a_skirmish_runs_from_the_menu_to_the_result_and_back` |
| `?map=` jump into a match | `bootIntent.ts` | done | `--map`, `--spectate` (`options.rs`) |
| All-AI slots watch as an observer | `skirmish.ts` | done | `spectating_names_the_map_and_players`, `a_spectated_ai_match_runs_and_presents_every_tick` |
| Game menu pauses single player; multiplayer never pauses | `setMenuPaused` | done | `the_game_menu_pauses_and_its_pages_return_to_it`, D38 |
| Restart scenario with the same map, players, teams **and seed** | `GameMenu.restartPage`, `onRestart` | done | `start_rerun` keeps the setup (fixed in this audit; it used to draw a new seed) |
| Match result dialog: Victory / Defeat / Draw / observer "Match complete" | `presentation/matchResult.ts` | done | `ResultModel::build`; a draw now says "Draw · The match ended without a winning side." (fixed) |
| Result: keep watching, return to menu | `MatchResultDialog` | done (new: keep watching, player rows, goods) | `result.json`, captures `m4-screens/new/result-*.png` |
| A decided match stands still and refuses commands | session + D23 | done | `finish` in `card.rs` ("Match has ended") |
| HUD outcome line ("Victory — the enemy main fort fell." …) | `settlementHud.update` | done | `Hud::messages` (old wording restored in this audit) |
| Surrender | none | new | game menu `surrender`, D38 |
| Hidden-tab behaviour (match runs, drawing pauses) | `session.tick` | partial | the simulation keeps time; the window keeps drawing (desktop has no hidden tab; sound mutes in background, D29) |

## Camera

| Feature | Old | Status | Evidence |
|---|---|---|---|
| North-up 56° perspective, 70° lens, zoom 0.515–1.25, terrain-following eye | `camera.ts` | done | `canopy_render::camera` tests |
| View kept inside the map | `clamp` | done | `RtsCamera::clamp`, `view::frame` |
| Smooth wheel zoom, PageUp/PageDown | `zoomMomentum.ts` | done | `run.rs` `wheel`, `KEY_ZOOM` |
| Arrow pan 28 u/s, edge scroll 20 px | `PAN_SPEED`, `EDGE_PX` | done | `run.rs` `update_camera` |
| Middle-drag pan | `panPersp` | done | `run.rs`: pans from the centre ray as the old `panPersp` |
| Centre selection / main building (Home), alerts (Space), next drop-off (Backspace), double-press groups and heroes | `settlementHud.onKey` | done | `idle_workers_menu_keys_tab_and_chat`, `control_groups_assign_recall_and_centre_on_a_double_press` |
| **Unit cameras: RTS → third person → first person (J, "Camera: …" command), Esc and camera jumps return to RTS, pan/zoom ignored while following, smooth 350 ms blend, first-person body hidden** | `unitCamera.ts`, `renderer.unitCamera`, `unitCameraCommand` | done (added in this audit) | `play/follow.rs` and its tests, `the_camera_command_cycles_unit_cameras_and_escape_returns_to_rts`; `canopy shot --play --input "focus:marshal;key:J"` (captures `m4-play/camera-third-person.png`, `camera-first-person.png`) |
| Third-person boom stops at terrain | `updateUnitCamera` | done | `follow::clear_boom`, `the_boom_stops_in_front_of_rising_ground` |
| Third-person boom stops at scenery and buildings | `cameraObstruction` | partial | terrain only; the first-person body also loses its shadow (opacity 0) where the old kept it |
| Spectator Q/E turn, B largest fight | none | new | `run.rs` (fixed keys; `docs/play.md` corrected: they were never in the old game) |

## Selection

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Click selects; ground clears; Shift toggles own units | `session.click` | done | `clicks_select_one_shift_toggles_and_a_double_click_selects_the_type` |
| Ctrl-click / double click: every own unit of the type on screen | `sameType` | done | same test |
| Box ≥ 5 px: heroes and army first, else workers; Shift adds | `areaSelection` | done | `box_selection_prefers_the_hero_over_workers`, `selection::tests` |
| Observer selects anything, commands nothing | `observing` | done | `an_observer_selects_but_never_commands` |
| Focus order hero → army → worker, stable | `prioritizeSelection` | done | `selections_order_heroes_then_army_then_workers_and_keep_the_focus_first` |
| Tab / Shift+Tab subgroups | `cycleSelection` | done | `tab_cycles_types_in_rank_order` |
| Control groups 1–0: recall, Ctrl assign (Cmd on macOS), Shift add, double press centres | `ControlGroups` | done | `control_groups_assign_add_recall_and_focus_on_a_double_press` |
| Control group buttons above the dock, in their own column, key over count, tooltip with the bound keys | `renderGroups` | done | fixed in this audit (the "6 over 1" box near the hero was group 1's button, drawn count-first and centred); capture `m4-screens/new/match-selection.png` |
| Hover pre-selection (re-picked as units walk under the cursor) | `pickGameHover` | done | `Play::hover` |
| Dead, hidden and contained entities leave the selection and groups | `update` | done | `Play::prune` |
| Picking: ≥ 28 px capsules, footprint fallback | renderer | done | `capsules_are_at_least_28_pixels_wide_and_the_nearest_body_wins` |

## Orders and command card

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Smart right-click: resume site, enter lookout, follow, gather, pick up, attack, move; producers rally | `session.click` | done (allies are not attacked, D36) | `right_clicks_move_gather_attack_and_shift_appends` |
| Enemy in a lookout attacked through its tower | `garrison` redirect | done | `Play::lookout_redirect` |
| Shift appends; Shift keeps placing | `append` | done | same test, `placement_snaps_rotates_validates_and_builds` |
| Move, attack (target forces, ground attack-moves), stop, hold (Shift queues), patrol, follow | `commandCard` | done | `attack_mode_issues_attack_move_and_escape_cancels` |
| Rally (ground, resource, friendly unit; right-click the producer clears) | `rallyAt` | done | `a_selected_hall_offers_training_and_rally_and_the_card_reports_why` |
| Train, queue full / supply / resources reasons | `commandCard` produce | done | same test |
| Pause / resume production | `pause` | done | `card.rs` |
| Research with reasons; cancel research | `research` | done | `card.rs`, `Hud::workplace` |
| Upgrade / cancel upgrade with progress text | `upgrade` | done | `card.rs` |
| Revive fallen heroes with reasons | `revive` | done | `card.rs` |
| Unload a lookout; cancel construction with refund text | `unload`, `cancel` | done | `card.rs` |
| Categories, Back in slot 9, bottom-row spells at their columns, banners, pages | `commandMenu`, `commandPage` | done | `the_layout_fills_rows_reserves_back_and_keeps_spell_columns` |
| Command hotkeys; Shift + key queues unless the key itself has Shift | `commandShortcut` | done | `shortcuts_match_exactly_then_queue_with_shift` |
| **Command hotkeys are rebindable (`command.*` entries)** | `shortcutCatalog`, `keyboardControls` | done (added in this audit) | `keys::commands`, `Keymap::command`, `rebound_command_keys_press_their_commands`, `authored_hotkeys_read_like_the_old_authored_key` |
| Cards and hints show the player's keys | `keyLabel(shortcuts.key(…))` | done (added) | `Hud::card`, `Play::hint` |
| Disabled commands show the reason (red message on hotkey, tooltip) | `activate` | done (a click on a greyed button does nothing, D36) | `Play::activate` |
| Order markers (white move, red attack, green gather; pulses) | `commandFeedback` | done | `markers_pulse_and_expire_like_the_old_rings`, world UI (D40) |

## Building

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Build / advanced build menus from the worker | `commandCard` build | done | capture `m4-play/workers-groups-build-menu.png` |
| Ghost on the 4-cell lattice, R / Shift+R quarter turns, rotation in the hint | `snapPlacement`, `placement()` | done | `placement_snaps_rotates_validates_and_builds` |
| Ghost validity (red/white cells, entrance, problem text) | worker `placement` query | done (from the player's knowledge, D36) | `the_ghost_agrees_with_the_simulation_where_the_player_sees` |
| Placement grid under the ghost | `placementGrid.ts` | done | world UI (D40), capture `m3-effects/ui-play-placement.png` |
| Translucent ghost model in the owner's colour | `gamePreview` | done | `ghost_instance`, D40 |
| Click with a blocked site shows the problem | `click` | done | `Play::left_click` |

## Economy feedback

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Resource ribbon by `displayOrder`, tooltip available / reserved / stored / carried | `stock` badges | done | `Hud::resources` |
| Supply committed/capacity, red over capacity, old tooltip | population badge | done | `Hud::resources` |
| Supply refusals name the race's own supply building | sim | done (D23 fix) | economy tests |
| Active task (upgrade, training, research, revival) with progress and name | `workplaceCard` | done | `Hud::workplace` |
| Five waiting wells, click cancels with refund | `queueCard` | done | `Hud::workplace` |
| Mine and workplace status ("n workers · m recommended · k remaining", production status) | `workplace.ts` | done | `Hud::workplace`; world label (D40) |
| Mine "Workers n/m" label over the building | `mineLabels.ts` | done | world UI (D40), capture `m3-effects/ui-observer-all-bars.png` |
| Construction scaffold and dust | `constructionScaffold.ts` | done | live scene (D35), effects (D40) |
| Next idle worker (F8, `) selects and centres | `selection.worker` | done | `idle_workers_menu_keys_tab_and_chat`; the view carries stunned, held and releasing flags, which the key skips |
| Next drop-off (Backspace) | `camera.hall` | done | `Play::next_dropoff` |

## Heroes, items and spells UI

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Hero bar: icons, health, F-keys (bound labels), select, double click centres, fallen tooltip | `HeroBar` | done | `Hud::heroes` |
| F1–F3 select, double press centres | `hero.n` | done | `Play::hero_key` |
| Level badge, experience ring and tooltip (mission cap, neutral cap) | `experienceMeter` | done | `hud::experience` |
| Damage and armour stats with the old tooltips | `info` rows | done | `Hud::selection` |
| Health and mana bars with numbers | `portraitHp`, `portraitMana` | done | `Hud::selection` |
| Learn abilities from the "New spell available" banner menu, returns to the root | `learnAbility` | done | `the_hero_learns_from_the_banner_menu_and_casts_from_the_bottom_row` |
| Cast from the bottom row: mana cost, cooldown sweep and seconds, reasons, rank text | `castAbility` | done | same test |
| Autocast: highlighted, right-click toggles, tooltip note | `alternate` | done | `Play::alternate` |
| **Spell aim at the cursor: area, range ring, valid tint** | `abilityTarget.ts` | done | `Play::aim` → `world_ui::AimMark`: the old filled outline (disc, or a line delivery's rectangle) with its edge and the dashed range ring, drawn over everything in the valid or refused colours; the hero test checks the aim; capture `m4-play/spell-aim-line` |
| Inventory 2 × 2: charges, cooldown seconds, click use, right-click drop, Numpad keys | `inventoryCard` | done | `Hud::inventory` |
| Inventory tooltip: tier, charges remaining, cooldown, "Equipped. Right-click to drop." | `inventoryCard` tooltip | done (completed in this audit) | `Hud::inventory` |
| **Item status badges in the selection panel (aura / active / rescue, modifiers, time left)** | `itemStatusCard` | done (added in this audit) | `Hud::statuses`, `hud::modifier_text`, `hud.json` `statuses` |
| Item status badges over units | `statusBadges.ts` | done | world UI (D40) |

## Combat feedback

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Health bars: selected and damaged; Alt / `[` / `]` holds; "always show" setting | `healthBars.ts`, `healthVisibility.ts` | done | world UI (D40), `Play::world_ui` |
| Mana strip on casters | `healthBars.ts` | done | D40 |
| Selection and hover rings by relation | `selectionCircles.ts` | done | D40 |
| Hit sparks on confirmed health loss | `impactEffects.ts` | done | D40, renderer §9 |
| Melee blade trails | `meleeTrails.ts` | done | D40 (ribbons) |
| Missiles, shells and their impacts | `projectileEffects.ts`, `shellEffects.ts` | done | D35, D40 |
| Deaths: 2 s death clip, flyers fall; fallen heroes hidden | settlement layer | done | D35 |

## Fog and minimap

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Play through fog: only what the team sees, buildings remembered | sim observation | done | D19, D36 |
| Fog shading (unexplored black, explored dimmed) on every surface | `fogOfWar.ts` | done | D40, captures `m3-effects/fog-*.png` |
| Fog on stacked bridge floors | per-floor atlas | partial | decks fold into the ground cell (D40) |
| Minimap: markers in player colours, buildings by footprint, deposits, fog, view quad | `minimap.ts` | done (background is the world from above, D40) | capture `m3-effects/ui-observer-all-bars-minimap.png` |
| Minimap left click / drag moves the camera | `onDown` | done | `Hud::act` |
| Minimap right click orders (move, attack, patrol, rally modes) | `onOrder` | done | `Play::minimap_command` |
| Minimap tooltip "Tactical map …" | `minimapHost` data-tip | done | `hud.json` |
| Minimap grab / grabbing cursor | `minimap.ts` | done (added) | `canopy_ui::Cursor::Grab` for `drag` nodes, `Game::update_cursor` |
| Day/night dial beside the minimap | `DayNightIndicator` | done (hidden in the old HUD too: `.rts-clock-slot{display:none}`) | — |

## HUD panels and tooltips

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Woodland dock: minimap, portrait, name, stats, vitals, inventory, command card, ribbon | `commandDock.css` | done | capture `m4-screens/match.png` |
| Live 3D portrait | `selectionPortrait.ts` | done (no rim light, D40) | capture `m3-effects/ui-observer-all-bars-portrait.png` |
| HUD size 30–100 % | `--hud-scale` | done | `apply_hud` |
| **HUD layout: spread / compact (≤ 1240 px, centred)** | `data-layout=compact` | done (added) | `Hud::dock_scale`, `the_compact_hud_is_at_most_1240_pixels_wide` |
| Hide / show HUD (Ctrl+Shift+H); Esc brings it back | `enableHudToggle` | done (Esc added) | `run.rs` |
| Unit cards for groups (health, click focus, double click isolate, Shift remove) | `cards` | done | `Play::card_click` |
| Queued orders of the focus with tooltips | `unitOrderCard` | done | `Hud::order_queue` |
| Tooltips: title with key, cost rows in display order, description, reason, eligible actors, autocast note | `CommandTooltips` | done | `Hud::tooltip` |
| Command tooltip placed above the grid at its width | `render` | done | `tooltip_anchor` |
| Portrait tooltip with the definition's costs | `portrait` data-tip | done | `Hud` portrait tooltip with name, description and costs |
| Pager ‹ › n / m | `pages` | done | `hud.json` |
| Observer panel: colour, name, "Player n · Computer/Human/Defeated", stock and income per minute, units, workers, every hero with level and fallen | `ObserverPanel` | done | `Hud::observer`: names, controllers, every hero, resource and hero icons, "Reviving", cell tooltips |
| Cursor: crosshair while aiming an attack; pointer and text cursors over the UI | `canvas.style.cursor` | done (added) | `Game::update_cursor` |
| Menu button | `Hud` | done | `hud.json` `menu` |
| Settings button in a match | `Hud` | done (through the game menu) | |
| Tooltips on/off setting | none | new | D39 |

## Chat, messages and alerts

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Enter opens chat, Enter sends, Esc closes, 300 characters, control characters stripped | `GameChat`, `chatText` | done | `idle_workers_menu_keys_tab_and_chat` |
| Lines fade after 10 s; open chat shows history; sender colour | `GameChat.receive` | done | `Hud::chat` |
| Multiplayer chat through the match server | channel `chat` | done | D39 |
| Error facts in red for 2.5 s, 0.3 s fade, newest unseen under 5 ticks | `showError` | done | `Play::read_facts` |
| Targeting and placement hints with the player's keys | `hint` | done | `Play::hint` |
| Space cycles the last 8 alerts (damage, completion, births; merged 120 ticks / 10 cells) | `recordAlerts` | done | `Play::watch_alerts` |
| Text fields copy, cut and paste through the system clipboard | browser | done (added) | `game/src/clipboard.rs` (arboard) |

## Menus and screens

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Main menu: Campaign, Skirmish, Multiplayer, Settings, Exit; version plaque | `menu.ts` | done | capture `m4-screens/main-menu.png` |
| Map Editor entry | `menu.ts` | dropped (phase 2; Load Game instead, D38) | |
| "What's new" release notes and gallery | `releases.ts` | dropped (phase 2) | |
| Animated menu background | `menuEffects.ts` | partial | UI boxes approximate the canvas (no slanted shafts or smoke, D38) |
| Menu fades | `menuTransition.ts` | done | `FADE_OUT`/`FADE_IN` |
| Skirmish: maps with thumbnails, preview with starts, size · players · camps, description | `skirmish.ts`, `mapPreview.ts` | done | capture `m4-screens/skirmish.png` |
| Slots: Human/AI (one human), race, hero; all-AI watches | `skirmish.ts` | done (+ teams, D20) | `at_most_one_human_and_teams_reach_the_setup` |
| Sandbox / testbed maps | `skirmish.ts` | done | `skirmish.rs` |
| Start validation messages | `createSkirmishMatch` | done (the screen cannot reach an invalid setup) | |
| Campaign: faction cards, crest | `campaign.ts` | done (from `rules.campaigns`, D38) | capture `m4-screens/campaign.png` |
| Mission list: number, title, art, chapter, briefing, Begin, "More chapters to come" | `campaign.ts` | done (missions unlock in order) | `progress_round_trips_and_opens_missions_in_order` |
| Loading: plain panel, stage, progress, error, back | `loadingScreen.ts` | done | capture `m4-screens/loading.png` |
| Loading for missions: art, chapter, title, briefing, rotating tips | `loadingScreen.ts` | done | `show_loading`: crest and chapter on mission loads |
| Game menu: title by mode and map, Resume, Save, Load, Restart, Settings, Exit | `gameMenu.ts` | done | captures `m4-screens/game-menu*.png` |
| Save page with a name field | `savePage` | done | capture `game-menu-save.png` |
| Exit page: Save and exit / Exit without saving / Cancel | `main` | done | capture `game-menu-leave.png` |
| Restart page: Save and restart / Restart without saving / Cancel | `restartPage` | done | `game-menu.json` |
| F10 toggles the game menu; Esc closes it | `GameMenu` | done | D36 |
| Confirm dialog | `confirm.ts` | done | capture `confirm-delete.png` |
| Match result dialog | `matchResult.ts` | done | captures `result-*.png` |
| Map preview images | `mapPreview.ts` | done | `widgets.rs` |
| Settings dialog from the menu and from the game menu | `graphicsControls` | done (tabbed, D38) | captures `settings-*.png` |
| Player name | settings dialog | done | `settings/interface/name` |
| Boot screen with progress | `LoadingScreen` | done | capture `boot.png` |
| Exit | menu | done | |
| `?screen=campaign`, `?screen=skirmish` boot intents | `bootIntent.ts` | dropped (desktop has no URL; `--map`/`--spectate` remain) | |
| Load game list in the main menu | none | new | `saves_load_from_the_main_menu` |

## Settings

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Game resolution: Native or 8 fixed sizes, interface native | `RENDER_RESOLUTIONS` | done | `a_resolution_cap_lowers_the_render_scale` |
| Shadows: soft / filtered / off | `SHADOW_MODES` | partial | on/off; the filtered mode is not offered (decision D41, review) |
| Volumetric fog / light shafts off/low/medium/high | `ATMOSPHERE_QUALITIES` | done | `settings/graphics/haze` |
| HUD layout spread / compact | `settings/hud.ts` | done (added) | `settings/interface/hud-layout` |
| HUD size 30–100 %, default 65 | `settings/hud.ts` | done | `settings/interface/hud-scale` |
| Spell effect volume (default 0.65) | `settings/audio.ts` | done (+ master, interface, music, mute in background) | `settings/audio/*` |
| Keyboard: every engine shortcut with the old defaults | `globalShortcuts` | done | `every_default_is_bindable_and_unique` |
| **Keyboard: every command shortcut from the content** | `shortcutCatalog` | done (added) | `keys::full_catalog` |
| **Keyboard: "Find a shortcut" search** | `keyboardControls` | done (added) | `SettingsUi::shown` |
| Rebinding: press a key; Esc cancels; Clear unbinds; Restore defaults | `keyboardControls` | done | `rebinding_refuses_taken_reserved_and_unsupported_keys` |
| Messages: already used, Enter reserved, unsupported, "Shortcut saved.", "Default shortcuts restored." | `keyboardControls` | done (the last two added) | `settings_ui.rs` |
| Alt alone only for health holds | `keyboardControls` | done | `KeyBindings::bind` |
| Settings saved on the device, applied at once | `localStorage` | done | `settings_round_trip_through_the_user_folder` |

## Saves

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Save a local match with a name (unlimited saves) | `SaveLibrary.save` | done (new format, D16) | `a_saved_match_resumes_with_identical_fingerprints` |
| Saves keep control groups; selection and modes are not kept | `saveControls` | done (the old game did not keep them either) | D39, capture `match-loaded.png` |
| Load restores map, players and camera | `restoreLocal` | done | `saves_load_from_the_main_menu` |
| Load list in a match shows only saves of its mode | `savesForMode` | done (fixed in this audit) | `Game::open_saves` |
| Refuses saves of other maps or content | `validateSaveDestination` | done | `the_library_refuses_other_maps_and_content_and_lists_newest_first` |
| Save before leaving / before restarting | game menu | done | `gm.save-exit`, `gm.save-restart` |
| Autosave when the window closes | none | new | D38 |
| Delete a save | none | new | capture `confirm-delete.png` |
| Export a save to a file, import one | `export`, `Import save…` | done | Export/Import through native file dialogs (`rfd`, `game/src/files.rs`), imported saves checked as loading checks them |
| No multiplayer saves | `gameMenu` hooks | done | D32 |

## Multiplayer

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Server status, room list (name, map, open/full/in game), refresh (and auto refresh) | `multiplayer.ts` | done | capture `m4-screens/multiplayer.png` |
| Create: map, slot count, name | `multiplayer.ts` | done | capture `multiplayer-create.png` |
| Join with a name; errors | `enterRoom` | done | `multiplayer_lobbies_lead_to_matches_and_leaving_leaves_the_room` |
| Lobby: seats, races, heroes (own seat), host starts, round trips, leave | `RoomWaitScreen` | done | capture `lobby.png`, `a_lobby_joined_from_the_screen_plays_a_networked_match` |
| Lockstep match over the frozen protocol | `WebSocketChannel` | done | `canopy-net` tests (D32) |
| Waiting for players, desync, disconnect overlays | session | done | captures `match-desync.png` |
| Chat to everyone | `chat` | done | D39 |
| Result for every seat; leave goes back to multiplayer | `onMatchOutcome` | done | D38 |
| Observers in a room | spectator connection | done (joining a running match is not, D32) | `canopy-net` |
| Content mismatch refused at the start | identity | done | D32 |
| Server address setting | none | new | `settings/network/server` |
| "Delete all server sessions" button | `onClearSessions` | dropped (decision D41, review: it deletes every player's rooms on the shared server) | `Net::clear_rooms` stays for tools |
| Load-game lobby variant | `RoomWaitScreen load` | dropped (never wired in the old game) | |

## Campaign and missions

No mission maps are mirrored, so missions cannot be played yet; the HUD is shown with a sample state (captures `mission-*-sample.png`).

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Current objective, script error | `MissionHud` | done | `match_ui.rs` |
| Dialogue: speaker, portrait, text; lifted clear of a large HUD | `MissionHud` | done | D39 |
| Letterbox in cinematics | `mission-letterbox` | done | capture `mission-cinematic-sample.png` |
| Keys and clicks held during cinematics (game menu still opens), selection rings hidden | `blockKey`, `gameSelect([])` | done (added) | `Play::cinematic` |
| Cinematic camera: jumps to the scene and returns after | `session.tick` | done | `play/cinematic.rs`: jumps to the scene and returns; scripted unit shots (`scene.camera`) through their unit with boom, height, lens and blend; the −8 % lens; the scene's set revealed. Unit shots are tested with fixtures; no shipped mission map uses them yet |
| Mission result: "Mission complete" / "Mission failed", next chapter, return to campaign | `MissionHud.result` | done (wording restored in this audit) | `ResultModel::build` |
| Company carried to the next chapter | `onMissionContinue` | done | D38 |
| Campaign progress, missions unlock in order | none (old list had no locks) | new | `Progress` |
| Mission loading screen | `loadingScreen.ts` | done | see Menus |

## Audio

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Spell effect sounds with the old spatial model | `effectSound.ts` | done | `crates/audio` tests, D29 |
| Effect volume setting | `settings/audio.ts` | done | |
| Silence while paused; muted in the background | `effectAudioPlaying`, hidden tab | done | D29 |
| No music, interface or unit sounds | none | done (none in either game) | D36 |

## Visuals

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Terrain layers, cliffs | `terrain/*` | done | captures `m3-terrain/` |
| Water, ripples, wakes | `water/*` | done (no SSR/refraction, D34) | captures `m3-water/` |
| Sun, sky, shadows, haze, canopy shade, contact shading, bloom, grade | `sky.ts`, `atmosphere/*` | done | renderer §9, captures |
| **Day/night cycle in matches (600 s a day from the map's hour; sandboxes fixed)** | `Sky.tick`, `setPlaying` | done (added in this audit) | `world::DayCycle`, `a_match_day_lasts_ten_minutes`; `--hour` keeps a shot's hour fixed |
| Forest beyond the map | `forestSurroundings.ts` | done | D34 |
| Foliage wind, tree sway | `foliageWind.ts` | done | |
| Units: baked animation, blends, attack phases, work strokes, casts, hit reactions | `characters/*` | done | D35 |
| Team colours | `maskedTeamColor` | done | |
| Concealment dissolve | `concealment.ts` | done | |
| Buildings: scaffolds, depletion stages, glow, upgrades | settlement layer | done | D35 |
| Carried goods, felled trees, corpses | settlement layer | done | D35 |
| Spell effects (particles, flipbooks, models, lights, ribbons) | `abilities/*` | done | D40, captures `m3-effects/spells-*.png` |
| Construction dust, shell impacts | `constructionScaffold.ts`, `shellEffects.ts` | done | D40 |
| Effect model translucency (Uproot looks lighter) | `effectModel.ts` | partial | D40 (review) |
| **Scenery cutaway: canopy trees open around your units** | `sceneryCutaway.ts` | done | `canopy_render::cutaway` (the old 192 × 108 depth and coverage mask, stippled in `statics.wgsl`, never in shadows; none for close cameras); capture `m3-effects/scenery-cutaway`. No shipped map places a canopy model yet |
| Rain and beam layers, mist regions, close depth of field | old | dropped (nothing shipped uses them, renderer §9) | |
| Dew, resin shimmer, vivid foliage, snow surface | old | dropped (Q5/Q6) | |
| Speech jaw movement | `speech.ts` | dropped (Q6) | |
| Ground decals from maps | `decal/*` | dropped (no shipped map uses them) | |
| Placement ghost | `gamePreview` | done | D40 |
| Fog of war | `fogOfWar.ts` | done | D40 |
| World UI (rings, bars, rally flags, labels, badges, progress) | `settlement/*` | done | D40 |
| Minimap and portrait | `minimap.ts`, `selectionPortrait.ts` | done | D40 |
| Loading the art with progress | `assetLoading.ts` | done | |

## Debug tools

The old game showed its performance window in the HUD unless the profiler was switched off; these are developer tools, listed for completeness.

| Feature | Old | Status | Evidence |
|---|---|---|---|
| Timings, frame and simulation costs, AI summaries | `performanceWindow.tsx` | done | egui overlay (F9), `play/debug.rs` |
| Render feature switches | same | done | `play/debug.rs` |
| Match speed 1–4×, reveal map, fog perspective, navigation overlays | same | partial | `play/debug.rs` match tools: speed 0.5–4× paced on the simulation thread, reveal map, fog perspective; gap: navigation overlays |

## What remains

| Gap | Owner / next step |
|---|---|
| Third-person boom against scenery and buildings; first-person body shadow | game + renderer: a ray test against static instances; an actor flag "no camera draw" that keeps the shadow |
| Debug overlay: navigation overlays | game (`play/debug.rs`) |
| Shadows "filtered" mode | Jiri (D41) |
