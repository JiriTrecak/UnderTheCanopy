# UI toolkit (`canopy-ui`)

Status: M4 toolkit, 2026-10-15 ([D33](decisions.md)). Implements [renderer §6](renderer.md#6-ui): the HUD and menus drawn in one batched quad pass, layout with taffy, text with cosmic-text, layouts in hot-reloaded data files. egui stays for debug tools only.

The old game's DOM HUD (`src/ui/**` and its CSS in the old repository) is the visual and behavioural reference. Its stylesheets port almost literally: the theme is written in CSS terms.

## Design

**A retained tree built from data, updated immediate-style.**

- Screens and widget templates are layout documents (`config/ui/layouts/*.json`) styled by a theme (`config/ui/theme.json`). They are compiled once into a node tree; taffy keeps its layout cache in that tree and every text node keeps its shaped buffer and glyph list.
- Each frame the game pushes its presentation models into named nodes: `set_text`, `set_value`, `set_image`, `set_enabled`, `set_checked`, `set_tooltip`, `set_count` for lists; `set_opacity` (fades) and `set_shift` (an offset in logical pixels for animations) change paint and placement only, never flex layout. Setters compare and invalidate only what changed: a value or colour change repaints, a text change reshapes one block, and only text that sizes its box re-runs layout. Hover and press states change paint only (state blocks may not change layout).
- `Ui::frame(time)` lays out what is dirty, places tooltips and paints the tree into a `DrawList`. `Ui::input(&event)` returns whether the UI consumed an event; interactions come back from `take_actions()` as `Action`s carrying the node's path, its layout `action` name and the index of the list entry it sits in.

Why not immediate mode: layouts are data (designers edit files while the game runs), taffy and cosmic-text are expensive to recompute from scratch, and a steady HUD should cost almost nothing. Why not a fully retained widget API: the game's models change every tick; pushing values into stable handles is as simple as immediate mode without rebuilding anything.

The crate holds no gameplay logic and never sees simulation types. It does no I/O except `UiConfig::load` and the optional system font scan; the game passes textures as ids.

## Files

| Path | What |
|---|---|
| `config/ui/theme.json` | palette, font roles and files, image names → asset paths, tooltip behaviour, styles |
| `config/ui/layouts/*.json` | one document per file (the file stem is its name): `templates` and an optional `root` screen |
| `config/ui/fonts/` | bundled fonts (Git LFS) and their licences |

`config/` is this repository's own data (D30), not mirrored from the old repository.

## Theme and styles

A style is a map of CSS properties to values, with state blocks `":hover"`, `":active"`, `":focus"`, `":checked"` (toggled buttons, `aria-pressed`), `":disabled"`. A node's `class` lists styles applied in order, then its inline `style`. `$name` refers to a palette entry. Styles named `@<kind>` (`@button`, `@image`) apply to every node of that kind first; `root` styles the tree root (inherited text defaults).

- **Layout** (taffy): `display` (flex, grid, block, none), `position`, `left/top/right/bottom/inset`, `width/height`, `min-*`, `max-*`, `aspect-ratio`, `margin*`, `padding*`, `border-width`, `box-sizing`, `flex*`, `justify-*`, `align-*`, `gap`, `grid-template-columns/rows` (with `repeat()` and `minmax()`), `grid-auto-flow`, `grid-row/column`, `overflow` (`hidden` clips, `scroll` scrolls). Every box defaults to `position: relative`, so absolute children are placed in their parent (CSS `static` would place them against a farther ancestor). Bare numbers are pixels.
- **Painting**: `background` (colour, `linear-gradient`, `radial-gradient`; first and last stop), `background-image` + `background-size` (`100% 100%`, `contain`, `cover`) + `background-inset` (an inset or outset like a pseudo element's), `border`, `border-color`, `border-radius` (px or %), `border-image` (nine-slice: `image slice / width / outset [fill]`), `box-shadow` (outer and `inset`, blur, spread, lists), `outline` + `outline-offset`, `opacity` and `filter: brightness() grayscale()` (both apply to the subtree), `image-tint`, `image-radius`, `object-fit`.
- **Bars and sweeps**: `fill`, `fill-image`, `fill-inset`, `fill-radius`, `fill-mode: luminosity` (the old health bar's `background-blend-mode`); `sweep-rest`, `sweep-ring`. Bar images are cut, not squeezed, by the value.
- **Text** (inherited): `font` shorthand, `font-family` (a role from `fonts.families`), `font-size`, `font-weight`, `font-style`, `line-height`, `color`, `letter-spacing` (em or px), `text-shadow`, `text-align`, `vertical-align` (`top`, `middle`, `bottom` inside the box), `text-transform: uppercase`, `font-variant: small-caps` (synthesised), `font-variant-numeric: tabular-nums`, `white-space: nowrap`, `text-overflow: ellipsis`, `line-clamp`.
- **Behaviour**: `pointer-events` (inherited; `none` lets clicks through to the world), `z-index` (paint and hit order layer for the subtree), `transform: scale()` + `transform-origin` (the HUD scale; text is shaped at the final size).
- **Widget parts** are styled by `<class>::<part>` (or `<kind>::<part>` for every widget of a kind): slider `track`, `fill`, `thumb`; checkbox `box`, `mark`; dropdown `arrow`, `popup`, `option`; text field `caret`, `selection`, `placeholder`; scrolling boxes `scrollbar`, `scroll-thumb`. Parts read `width`/`height` in px and may have states.

Unknown properties, bad values, unknown styles or templates and state blocks that change layout are errors naming the file and style; a hot reload with an error keeps the running UI.

## Layout documents

```json
{ "visible": false,
  "templates": { "command": { "kind": "button", "class": "command", "action": "command",
                              "tooltip_anchor": "actions",
                              "children": [ { "id": "icon", "kind": "image", "class": "command-icon" } ] } },
  "root": { "class": "hud", "children": [
      { "id": "commands", "class": "command-grid", "repeat": "command", "count": 12 } ] } }
```

Node fields: `id`, `kind`, `class`, `style`, `text`, `image`, `value`, `min`/`max`/`step` (sliders), `options`/`selected` (dropdowns), `placeholder`/`max_length` (text fields), `action`, `hotkey` (`"Q"`, `"Shift+F1"`), `tooltip` (static), `tooltip_anchor` (an ancestor id whose box places and sizes the tooltip, like the old command grid), `toggle`, `checked`, `drag`, `visible`, `enabled`, `repeat` + `count`, `children`.

**Paths** join the ids of addressable ancestors: the screen is the document name, list entries are numbers (`hud/dock/actions/commands/3/icon`). `ui.find(path)`, `ui.child(list, i)` and `ui.find_in(node, "icon")` return `NodeId` handles that stay valid across hot reloads and list resizes (setters on a vanished node do nothing). `ui.show(screen, visible)` shows a screen; `ui.dump()` prints the laid-out tree.

## Widgets

| Widget | How |
|---|---|
| Command cell / button | `button` (click, double click, hotkey, toggle, tooltip, focus) composed in a template: icon image, cooldown `sweep` with seconds, rim image, hotkey label (`common.json` → `command`) |
| Icon slot | `button` template with icon, rim and count badge (`slot`) |
| Bar | `bar`: value 0–1, fill colour (`set_color` for the health ramp), fill image, child label for the numbers |
| Label / rich text | `label` with markup: `[b]`, `[i]`, `[color=#hex]`, `[key]` (the theme's hotkey colour), `[[` for a bracket; wrapping, ellipsis, line clamp |
| Tooltip | the theme's tooltip template (`title`, `costs` list of icon + amount, `body`, `note`) filled from the hovered node's `Tooltip`; `delay-ms` before the first one, then switching is instant for `warm-ms` |
| Scroll list | any box with `overflow: scroll` and a `repeat` template: wheel, draggable thumb, page clicks on the track, clipped by a scissor rectangle |
| Text field | `text_field`: caret, selection (shift and the mouse), Home/End, Backspace/Delete, select all, copy/cut (`ActionKind::Copy`), paste (`PasteRequest` → `ui.paste`), IME composition shown at the caret, `max_length` in characters, Enter submits, Escape leaves |
| Slider, checkbox, dropdown | `slider` (click, drag, arrow keys, step), `checkbox`, `dropdown` (popup above everything, keyboard selection) |
| Minimap, portrait | `image` nodes whose image is a render target the game registers; `drag: true` reports `DragStart/Drag/DragEnd` with positions 0–1 for minimap panning |
| Sweep | clockwise from twelve o'clock (cooldowns, the experience ring) |

The shipped HUD (`layouts/hud.json`, templates in `common.json`) reproduces the old woodland dock: connected frame art, minimap, portrait, name, stats, vitals with bar rims, satchel inventory, 4 × 3 command card, resource ribbon and hero bar, scaled by `transform: scale(0.65)` like the old `--hud-scale`. `layouts/gallery.json` shows the remaining widgets in the old menu and settings styles.

Races may supply `hud: {frame, resources}` in their package definition. Both fields are image asset ids: the connected dock and the separate resource ribbon above the command grid. The local seat's race in the match setup chooses them once when the match opens, including saved and multiplayer matches; selection and fog perspective do not change it. Observers use the default race. Missing fields keep the theme's art. The Ants use the woodland pair; the Beetles use steel, brass and contained violet magic, with the same layout and controls. Background texture overrides survive UI hot reloads.

## Text and fonts

Font roles map to family stacks; the first family present wins. `system-ui`, `serif`, `sans-serif` and `monospace` name the platform's fonts.

| Role | Stack | Old game |
|---|---|---|
| `dock` | Outfit | `Outfit Variable` from `@fontsource-variable/outfit` (HUD, resource numbers) |
| `serif` | Georgia, Outfit | `Georgia, serif` (titles, menu, stats, vitals) |
| `ui` | system-ui, Outfit | `system-ui` (tooltips, hotkeys, notes) |
| `mono` | ui-monospace, Outfit | debug text |

- **Outfit** (variable, weights 100–900) is bundled: `config/ui/fonts/Outfit-Latin.ttf` and `Outfit-LatinExt.ttf`, the old game's own npm subsets converted from WOFF2 to TrueType unchanged. Licence: SIL Open Font License 1.1, © 2021 The Outfit Project Authors; the licence text is `config/ui/fonts/Outfit-OFL.txt` and must ship with the fonts.
- **Georgia** and the **system UI font** are what the old game used, from the operating system (both ship with macOS and Windows). They are not redistributable, so they are not bundled: the game loads system fonts (`FontSources { system: true }`) and falls back to Outfit where they are missing. Bundling Gelasio (OFL, metric-compatible with Georgia) would make the look independent of the system (D33, review).
- cosmic-text only matches exact weights of static fonts, so requested weights snap to the family's available ones as CSS font matching would (600 → Georgia Bold); variable fonts take any weight.
- Text is shaped at its final pixel size (font size × display scale × transforms) and keeps its buffer and positioned glyphs until text, style, width or scale change. Glyphs are rasterised by swash into one R8 atlas (1024², growing to 4096², cleared and refilled when full).

## Draw list and the renderer contract

`DrawList` = `quads` (64-byte instances: rectangle, texture coordinates, two colours, radius, border, softness, a parameter, brightness, kind and flags), `batches` (quad ranges sharing one image texture and one scissor rectangle) and `atlas` changes. Shapes, gradients, shadows, sweeps and glyphs never break a batch; only a different image texture or clip does. One pipeline, one instanced `draw(0..6, range)` per batch.

- **Colours are sRGB with straight alpha and the pass blends premultiplied in sRGB space**, as browsers do, so the old stylesheets' colours, gradients and translucency match. Draw into a non-sRGB view of the target (WebGPU allows `rgba8unorm` / `bgra8unorm` views of sRGB textures through `view_formats`). With an sRGB target the pass linearises its output instead (blending then happens in linear space and translucent overlays look slightly different).
- **Image textures** are premultiplied when `ImageDesc::premultiplied` (the pack's translucent images are, D26). Bind them through a non-sRGB view, or set `ImageDesc::srgb_view` when binding the pack's sRGB view so the pass re-encodes the samples. Mipmaps matter: the woodland art is drawn far below its source size.
- **Atlas changes are incremental**: every frame's draw list must reach the renderer (`UiRenderer::prepare`). When a renderer is created after the UI or loses its textures, call `Ui::reset_glyph_atlas()`.
- The reference pass is `canopy_ui::gpu::UiRenderer` (feature `wgpu`): WebGPU baseline only (D2), WGSL in `crates/ui/src/ui.wgsl`. `gpu::render_to_rgba`, `gpu::upload_rgba` and `gpu::headless_device` serve tools and tests.

## Input

`canopy_ui::winit::translate(&WindowEvent)` (feature `winit`) turns window events into `InputEvent`s: pointer moves in physical pixels, buttons, wheel lines (trackpad pixels / 40), keys (logical, so hotkeys follow the keyboard layout), modifiers, typed text from `KeyEvent::text`, IME preedit and commit, focus loss.

- A pointer event is consumed when it hits a painted node with `pointer-events: auto` or a press that began on one is held (`wants_pointer()`), so clicks never fall through to the world. Hit testing uses the regions of the last paint, top layer first.
- Clicks fire on release over the pressed node; `count` is 2 for a double click (hero portraits centre the camera). Disabled nodes still block the world.
- Hotkeys trigger the topmost visible, enabled node with that key; shift held with a plain hotkey still triggers it and is reported (shift-queued commands). A focused text field takes keys instead (`wants_keyboard()`); `ime_area()` gives the caret rectangle for `Window::set_ime_cursor_area`.
- Tab and Shift-Tab move focus once something has focus (`ui.focus(id)` starts keyboard navigation in menus); Enter and Space activate; arrows drive sliders and dropdowns; Escape closes a dropdown, leaves a field, then hides a tooltip.
- `cursor()` says which mouse cursor to show: pointer over controls, text over fields, grab over draggable nodes and grabbing while dragging one. The game sets it on the window (a crosshair while aiming an attack). Copy, cut and paste reach the system clipboard through the game (`ActionKind::Copy`, `PasteRequest` → `game/src/clipboard.rs`).

## Hot reload

`UiConfig::load(config/ui)` remembers file times; debug builds call `ui.reload_if_changed()` a few times a second. A reload recompiles everything and rebuilds the tree, keeping every value the game set (texts, values, images, list counts, visibility, focus) for paths that still exist. On error the old UI stays and the error names the file and style.

## Game integration

The match binds the HUD in `game/src/play/hud.rs` and draws it through the renderer's `ui` pass (`canopy_render::ui::UiLayer`, D36); see [play](play.md). The sketch below is the general shape.

### Sketch

```rust
let mut ui = Ui::new(UiConfig::load(&paths.config().join("ui"))?, FontSources { bundled: vec![], system: true })?;
for (name, asset) in ui.theme_images().map(|(n, a)| (n.to_owned(), a.to_owned())).collect::<Vec<_>>() {
    let (id, w, h) = renderer.ui_texture(&pack, &asset)?; // a non-sRGB (or srgb_view) view of the BC7 texture
    ui.register_image(&name, ImageDesc::new(id, w, h));
}
// per window event:   for e in canopy_ui::winit::translate(&event) { if !ui.input(&e) { world_input(&e) } }
// per frame:          hud.update(&mut ui, &presentation);  let list = ui.frame(t);  renderer.ui_pass(list);
//                     for a in ui.take_actions() { commands.handle(a) }
```

`Ui` is not `Send` (taffy styles hold raw calc pointers): it lives on the window thread.

## Performance

`cargo run --release -p canopy-ui --example bench` on an M4 Max, 2560×1440: the woodland HUD with a full 4 × 3 command card, hero bar, minimap and portrait frames, inventory, resource ribbon, a tooltip shown, the 100-line log, the settings dialog and the main menu (305 nodes, about 1,500 quads in 64 batches):

| Frame | Mean | p99 |
|---|---|---|
| Steady: bars, cooldown sweep and experience ring animate | 0.033 ms | 0.037 ms |
| Busy: plus five texts change every frame and the pointer moves across commands (tooltip switches) | 0.094 ms | 0.152 ms |
| Worst: all 100 log lines change text every frame | 1.26 ms | 1.38 ms |

Display scale 2 gives the same numbers. Start-up (scanning system fonts, compiling, building) takes about 140 ms. Batches are mostly one per distinct icon texture; an icon atlas or texture array would bring them down to a handful if draw calls ever matter. Measure on the base M1 before setting budgets.

## Screens (the game)

The game's menus are layout documents in `config/ui/layouts/`, one per screen, driven by `game/src/screens/` ([D38](decisions.md)):

| Document | Screen | Code |
|---|---|---|
| `main-menu` | logo panel, six actions, animated forest art, the release plaque | `screens/menu.rs` (effects), `screens/releases.rs` (plaque) |
| `campaign`, `missions` | faction cards; mission list with art, chapter, briefing | `screens/campaign.rs` |
| `skirmish` | battlefields, preview with start markers, players (controller, team, race, hero) | `screens/skirmish.rs` |
| `replays`, `replay` | the recorded matches; the replay viewer's bar, statistics and transport | `screens/replays.rs`, `screens/replay_ui.rs`, `replay/` |
| `multiplayer`, `lobby` | server status, lobbies, create and join; the room's seats, races, heroes, round trips | `screens/multiplayer.rs` |
| `loading` | plain panel (skirmish, multiplayer) or mission art with progress | `screens/loading.rs`, `mod.rs` |
| `hud` (with `play.json`) | the in-match HUD, chat included (the play module) | `play/hud.rs` |
| `match-overlay`, `mission-hud` | waiting for players, connection problems; objectives, dialogue, letterbox | `screens/match_ui.rs` |
| `game-menu`, `result`, `saves`, `settings`, `dialog` | overlays | `match_ui.rs`, `settings_ui.rs`, `mod.rs` |
| `releases` | What's new and the update details, with the gallery viewer | `screens/releases.rs`, `update/` |
| `feedback` | Submit feedback: the screenshot, category, description; sending, sent, saved for later | `screens/feedback.rs`, `feedback/` |
| `fade`, `backdrop` | screen-change fade; the 3D frame behind captures | |

Paths include every ancestor with an id (`settings/graphics/shadows`); a path the code asks for that no layout has is logged once. Theme images (`config/ui/theme.json`) and on-demand images (map previews, crests, portraits) are bound from the cooked pack (`screens/ui_host.rs`); pack textures are straight alpha, sRGB ones bound through their sRGB views. The one `Ui` (menus, overlays and the HUD) draws through the renderer's `ui` pass into a non-sRGB view of the swapchain (its `view_formats`): after the scene in a match, or alone onto a cleared target on menu screens (no 3D frame is rendered there, `UiLayer::encode_alone`).

`canopy capture-screens --out <folder> [--size 1600x900] [--scale 1] [--only releases|multiplayer|editor|feedback|replays]` renders every screen offscreen through the real game (headless, a temporary user folder, updates never reach the network); with `CANOPY_MATCH_SERVER` naming a local match server (`just server`) it also captures a real lobby as a guest (`lobby`) and as a host with computer and closed seats and a second player (`lobby-host`). `--only releases` captures just the release plaque in each update state, What's new (this copy's history, the whole log, a release with its gallery, the notes scrolled, the enlarged image) and the update details (checking, current, failed, offered with structured and with plain notes, downloading, ready); scripted update states are marked `-sample`.

### Submit feedback

[D59](decisions.md). **Submit feedback** in the main menu and the game menu opens `layouts/feedback.json` (overlay `Feedback`, over the main menu or over the game menu, which keeps a single-player match paused under it; Escape or Cancel goes back to what was under it).

- **The screenshot** is what the player was looking at: the frame after the click is drawn without the form and the game menu into a texture of its own (`feedback::grab::Target`; the window keeps showing the previous frame), copied into a read-back buffer and polled without blocking, then scaled to at most 1,600 px wide and encoded as JPEG (quality 85) with a 640 px thumbnail on a worker thread (`feedback/grab.rs`). The form shows the thumbnail with "Screenshot of the game" or "Screenshot of the menu" and says it is submitted with the report; it always is (no opt-out for now). If no screenshot can be taken, the form says so and the report goes without one.
- **The form**: a category (Bug, the default; Idea; Balance; Other) and a description of up to 5,000 characters, required: Submit stays disabled until it has text (and while the screenshot is being taken). The description is a single-line text field (the toolkit's field does not wrap). A line says what is attached: the game version, OS, graphics adapter and preset, and in a match the map, mode and tick, plus this installation's anonymous id (`install-id` in the user folder, random, made on first use). An unsent description stays if the form is closed and opened again.
- **Sending** (`feedback/mod.rs`): Submit writes the report (screenshot included) to `feedback/<report id>.json` in the user folder, then sends it to the match server (`POST /api/feedback`, [server §Feedback](server.md#feedback)) through `canopy-net`'s HTTPS client, with a 20 s limit. The form shows **Sending…**, then **Thank you!** with the server's reference, or **Saved for later** when the server cannot be reached, is busy or does not have the route yet; a refused report goes back to the form with the reason. Waiting reports are sent again a few seconds after start and every five minutes, oldest first, and removed once taken; closing the form while sending does not stop it. Reports sent later are only logged.
- **From a match** the report also carries the replay of the match so far and an exported save of the moment (D60), made on a worker thread as the form opens; the form says "Replay of this match is attached, with a save of this moment" and waits for them as for the screenshot. Watching a replay, the replay is attached. Files the server would refuse as too large (a replay over 1 MB, a save over 4 MB) are left out.
- Captures: `--only feedback` shows the form over the main menu (empty, filled), sending, saved for later (the report sent to an address nobody listens on), sent (with `CANOPY_MATCH_SERVER` naming a local server) and the form over a match; it writes the screenshots it took beside them as `feedback-screenshot-*.jpg`. Captures never send to the deployed server.

### Replays

[D60](decisions.md). Every match is recorded (`replay/record.rs`) and kept as `Last match` in the replays folder (`Under the Canopy/Replays` in the documents folder) when it ends, is left or the game closes; the result dialog's **Save replay** keeps it as `<Map> <date> <time> <result>` and says so. **Replays** on the Skirmish screen opens `layouts/replays.json`.

- **The list**: every `.utcr` file, newest first, with the map preview, the map, date and time, length and result, and the players. The selected one shows its details: mode, map, result, date, length, the game version and size, every player's controller, race, hero, team and colour, and who recorded it. **Watch replay** (or a double click) opens it; **Rename** renames the file (a field with Rename and Cancel), **Delete** asks first, **Show in Finder** opens the folder. A replay this game cannot play is dimmed and Watch is disabled with the reason: `Recorded in 0.3.5` (other content), the map is not in this game or has changed since. A replay that carries its map plays without it.
- **The viewer** is an observer match over `layouts/replay.json` (`screens/replay_ui.rs`): a bar along the top with each player's colour, name, goods, supply and first hero (dimmed when defeated; tooltips give income per minute and the heroes), and **Statistics** (Tab) for a panel with four tabs: Resources (each good stored with its income per minute in green, gathered in all, supply, workers), Army (army units, their value, workers, buildings, heroes' levels, supply), Units lost (units and buildings lost and killed, and their value; a kill counts for the player whose blow was lethal) and Production (what each player's buildings make, research, upgrade or revive, the furthest along first). The HUD's observer panel gives way to it.
- **The transport**, above the HUD dock: Pause/Play (Space; at the end, Replay starts over), the time, the timeline (click or drag; the thumb and time follow the drag, the jump happens on release; the lighter track is what the background analysis has reached; a red mark where the replay went out of sync), ½× 1× 2× 4× 8×, and **Vision** (Everyone, or one player's fog of war). ← and → skip 10 s, so they do not pan in replays. A seek lands at most one keyframe spacing (5–10 s) before its target when simulating the rest would take too long (see D60).
- **Out of sync**: "This replay is out of sync from mm:ss" shows above the transport (with a tooltip) as soon as playback or the background analysis finds a fingerprint that differs; the replay plays on.
- Selecting units and buildings shows them as to an observer; the camera, minimap and unit cameras work as when spectating. The game menu offers Leave the replay, which returns to the replays.
- `canopy replay <file>` (`just replay`) opens a replay from the command line; `--verify` checks one headless (see D60).
- Captures: `--only replays` plays a minute and a half at 8×, shows the result with the replay saved, the list (with a replay from another version), the viewer with the bar, the statistics tabs, one player's vision, the timeline dragged, a jump made and a replay out of sync.

### Release plaque, What's new and updates

[D43](decisions.md). The plaque at the bottom right of the main menu is the old zinc version indicator: the installed version (the workspace version, `CARGO_PKG_VERSION`) and the update check's state — **Checking…**, **Latest ✓**, **Update available ↓** (green) or **Check unavailable** (a failed check never says latest) — beside **What's new**. The game checks the feed each time the main menu comes up, unless an update is already on offer (an offer, a download or a verified package stays).

- **What's new** (a new design, review): a timeline of every release up to the installed one, newest first, beside the selected release — version badge, date, the installed tag, the title as a headline, counts by kind, the gallery and the notes grouped as New, Improved, Fixed and Known issues, each with its colour and glyph. The history is `releases/log.json` built into the executable, so it reads offline. Gallery images come from `releases/media/` in the data folder (shipped as `data/releases/media`; Git LFS, so a checkout without them shows "Image unavailable"), else from the release host; clicking one enlarges it (Left and Right step, Escape returns).
- **Update details** (clicking the version, which also retries a failed check): a status card with the old messages, download progress and errors, the offered release's notes (its versioned `changelog.json`, else the feed's plain notes read back by kind, else the plain text) or, without an offer, the installed release's. The one action is the old one for the state: Download update, Install and restart, Restart now, Check again, Try again. Close and Escape do nothing while checking, downloading, installing or restarting.
- The flow (`update/mod.rs`, a polled state machine) and its backend (`update/remote.rs`: the feed over HTTPS on its own thread, the download into the user cache folder, signature and version verification, the install over the running `.app` and the relaunch) are described in their modules. A copy that cannot install (a development build not running from an `.app`, a quarantined copy run from Downloads, a platform without packages) still shows the update and says why instead of offering the download.

## Gallery

`cargo run -p canopy-ui --example gallery` renders the HUD and every widget offscreen to `target/shots/ui-gallery.png` (`--size 2560x1440 --scale 2` for Retina). It decodes the art from the base package's asset folders (pull it with Git LFS); the game binds the pack's textures instead.
