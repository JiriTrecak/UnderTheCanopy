# Map authoring

Open the game's Editor, or `/?screen=editor` to start a blank map. The top strip names, creates, saves, loads and exports maps. New maps choose a name, dimensions, biome and weather. The biome establishes the base ground and the available river, forest, foliage and landform presets; it is not a separate season switch.

Unsaved maps recover in the same editor tab after a reload, including imported maps. Recovery stores the committed map document in IndexedDB; session storage holds only a small tab identifier. If storage is unavailable or full, the editor displays a notice to save or export instead of silently losing recovery. Disk `.utcmap` files remain the portable source of truth.

MCP can transfer large maps directly through files: `editor_landscape` with `action: "export", path: "tmp/my-map.utcmap"` creates a new file inside the project and returns its path and size. Existing files are never overwritten by this operation. Use `action: "load", path: "/absolute/path/my-map.utcmap"` to load it. This avoids MCP text-message limits; inline `map` load/export remains available for small documents. Loading or importing through MCP detaches the prior disk file handle. `editor_scene` inspection returns terrain metadata rather than encoded terrain arrays; use `get` with `offset` and `limit` to page through placed objects. The selected object is included separately in edit replies.

## Layout and tools

The left panel is a searchable hierarchy with All/Layers/Objects filters. Selecting a generated object selects its owning layer. Select a layer or authored object and choose **Go to selection** in the inspector to move the camera there; use the hierarchy to locate forests, shore objects and camps. Independent scenery and gameplay entities have their own groups. Group expansion and list scrolling persist while changing selection.

The bottom icon strip contains Select, Place, Terrain, Foliage, Decals, Water Spawn and Clean, plus edit undo/redo, Environment and MCP. Place switches between single units/buildings and scenery/landmarks. The right inspector holds contextual settings; authored scenery exposes position, elevation, rotation and scale. Top down, Game and Free camera presets sit above the viewport.

Select picks an object, then an intersecting layer, or clears selection on empty ground. Right-click the canvas to choose any overlapping visible layer from a context menu; right-drag still orbits. With an object or unit selected, Q/E snap backward/forward to the next 15° increment; Shift+Q/E use 90° increments. Bridges can be moved and rotated over water, with their rendered orientation and walk surface updated together. Their ends must still meet a walkable bank at a compatible height. Paint and path authoring work best in Top down view. Water supports Paint lake, Line and Curve. Painted masks have Add/Subtract and one brush-size control; Shift temporarily subtracts. Line/curve drawing uses Enter to finish and Escape to cancel. Curves expose draggable anchors and tangent handles. Terrain offers painted landforms and ground surfaces as well as direct height tools.

## The map is the applied result

A version 3 `.utcmap` stores committed terrain heights, local water surfaces and flow, ground-material weights, ordinary placed objects and decals. These are editable map data. Loading never runs a river, mountain, forest or foliage generator. There is no publishing bake and no separate runtime-map artifact. Earlier procedural map formats are retired rather than migrated.

Generators are tools in the editor. Draw or paint a temporary preview, adjust it, then choose **Apply previews**. Apply writes its terrain, water and objects into the map in one undoable operation and removes the preview. Every resulting tree or rock is an ordinary editable placement. Later edits and deletions cannot trigger regrowth. Unapplied preview shapes, seeds and recipe references are never included in saves, exports or map drafts; export before Apply contains the unchanged committed map.

The ground tint beneath trees and foliage follows the placed objects. Apply does not turn that tint into permanent grass paint; moving or cleaning vegetation removes its old tint. Deliberate grass coverage, including applied meadow terrain, remains editable terrain and survives scenery cleanup.

**Decals** in the bottom toolbar opens the ground-detail tool. Drag in **Paint preview** mode, then choose **Apply decal preview** to commit all patches in one undo step, or **Discard decal preview** to remove them. Preview patches are visible but absent from saves, exports and drafts. Erase removes a preview patch before a saved patch underneath it. Select edits ordinary applied decals; Clean can remove a filtered area. Decals follow later sculpting and support the full map size, including 1,024- and 2,048-unit maps. MCP `editor_decals` exposes the same `preview`, `apply` and `discard` operations; `place`, `update` and `delete` edit ordinary saved patches directly.

**Terrain** provides absolute grid levels, ground-material painting, dry ground, shallow water and deep water. Set-level selections offer a cliff edge or a sloped bank with an explicit width in cells. **Grid · ramp** connects a start level to an end level in the chosen compass direction; its endpoints use the selected region bounds. Water selections expose bank width as well. Selected cells reach the exact target height or bed depth; banks slope outside the selection, so neighboring regions join without an internal ridge and even a single cell reaches its requested depth. Reapplying a selection preserves its banks. Failed applications retain the selection for correction, and a pending application cannot be submitted twice. Terrain cells use the building grid: four world units per cell with an origin of -0.5. Rectangles select whole cells; polygon anchors snap to corners and edge midpoints. Optional sculpting writes into the same terrain data. Apply or discard a generator preview before directly editing terrain.

Editable terrain heights range from -16 to 24 metres. Grid level controls use the same limits as sculpting; water levels additionally leave room for their requested bed depth. Generator previews report out-of-range terrain or water and cannot be applied until corrected. Saved arrays validate dimensions before decoding, finite bounded heights, coverage weights in 0–1, and unique water-profile references. Invalid edits leave the document and undo history untouched.

Shallow and deep water have separate bed and surface heights. Shallow crossings are traversable by ground units; deep channels block them. Building foundations must remain dry and level. Navigation uses the same saved heights and water surfaces that rendering uses; textures do not determine traversability.

**Clean** supports brush, rectangle and lasso selections, category and asset filters, an exact removal count, and locked-object protection. Apply removes matching authored scenery, placed stamps and decals together; Undo restores the complete removal. Individual scenery placement, brush Apply, movement, rotation, walk-surface edits and deletion use the same history as terrain and cleanup. Locked scenery cannot be moved or deleted. Remaining objects keep efficient instanced rendering. MCP `editor_clean` accepts an independent disc with the same category/asset filters and an optional `preview: true`; it never consumes an unfinished editor selection. Use `editor_scene` cleanup for rectangle or lasso areas.

**Environment → Biome & conditions** selects the biome, time of day and optional weather kind. Biomes own lighting, grading, atmosphere, canopy and water styles; maps cannot override their appearance. Define a biome variant in `src/content/biomes.ts` to change that direction consistently in the game, editor and asset workbench. MCP exposes the same validated operations: `editor_landscape` with `action: "biome"` and a biome ID, or `action: "environment"` with hour, playing and weather kind. Asset definitions and publication are managed in the [asset editor](asset-pipeline/publication.md), separate from placing instances on a map.

## Forest-scale scenery

Vibrant Forest and Amberleaf Forest offer **Woodland mushroom patches** in Foliage: paint an additive/subtractive mask and adjust density as with other ground cover. Place **Ancient canopy oak**, **Spreading canopy oak**, **Great broken trunk**, **Great fallen hollow log**, and the two **Giant umbrella** mushrooms individually. These are sparse landmarks; their normal scale already dwarfs harvestable pines. Only assets with `capabilities.canopy` (the two canopy oaks and the ancient canopy tree) dissolve around observed units behind them. Buildings, ordinary trees, mushrooms and rocks stay solid so they still hide units. Everything casts full shadows. Collision covers trunks/stems, not overhangs; the fallen log is a solid obstacle.

The editable source and geometry live in each canonical asset package. Republish changed models from their `source.blend` through the Asset Studio. Painted bark and mushroom albedos, including their ImageGen prompts, are in `art/assets/asset.textures.forest-giants-*`. Temporary renders stay outside the repository.

## New map composition

Read [the map-design contract](game/map-design.md) before authoring starts, resource sites and construction shelves. Previous procedural maps are retired. New maps should be authored through the live editor and exported into `assets/maps/skirmish`; do not create a separate generation script.

The Environment panel also has **Starting terrain visibility**: mode default, explored under fog, or fully hidden. This persists as `initialExploration` and supports entity/spawn undo. MCP uses `editor_landscape` with `action:"exploration"` and `initialExploration:"explored" | "hidden" | null`; null restores the mode default. This setting changes initial knowledge only, never current enemy visibility.

## Save and verify

**Save** writes a `.utcmap` file. The first save chooses a destination; subsequent saves update that file when the browser supports file handles. **Save as** chooses another destination. Browsers without file-picker support download the file. Unfinished maps can be saved; gameplay clearance is checked when starting a test or match. No map documents are autosaved or restored from browser storage. Unsaved edits are lost when the editor is closed or reloaded. Existing legacy browser data is not read or erased.

**Test mission** sends a snapshot directly to a temporary preview tab. The preview never enters the map or campaign library and cannot create game saves. It expires when closed or reloaded; start another test from the editor. Project map lists contain only maps shipped in `assets/maps`.

Save writes the committed map, including applied scenery instances. Generator previews remain outside the document. Check navigation at bridges and water margins, player spawns, resource access, fog visibility and landmark readability in the game view. Use the minimap as a spatial check, then play the map; a visual preview alone does not prove traversal.

## Performance

Object dragging updates only the selected model’s instance transforms. The document, procedural placement, terrain uploads, inspector and autosave update once on release. Pointer cancellation restores the original pose. Unit-only edits reuse the compiled landscape; object/layer commits reuse their validated compilation instead of compiling twice.

Generation uses exact indexed spline queries, broad river/path bounds and cached vertex wetness. Moving scenery refreshes vegetation coverage but preserves unchanged terrain geometry and water meshes. Debug includes `Editor drag preview` and `Editor drag commit` scopes; commit timings include synchronous regeneration and renderer updates.

Measure loading and editor Apply separately. Loading decodes committed map data and builds ordinary render/navigation structures; generation cost belongs exclusively to editor previews and Apply. Do not claim loading speedups without measuring them.

## Map import and ground textures

**Import WC3** (Warcraft III map importer, kept as a possible future feature) converts `.w3x`/`.w3m` files into a normal editable `.utcmap`: terrain, water, ground tile IDs and variations, cliff-adjacent ground, trees, starting positions, gold mines (five-node amber sites) and neutral camps. Source scripts, units, archives and generators are never retained. Hostile neutrals within 1500 source units of occupied starts are cleared; surviving camps keep their homes, and summed creep levels select easy (≤9), medium (10–19) or hard (20+) compositions. Starting pads are leveled to fit the declared hall and formation; the import report lists these and any placement adjustments. Missing tile IDs are an error (MCP imports may map them explicitly). Policy: `content/import/warcraft/policy.json`. Offline: `npx vite-node --config vitest.config.ts scripts/maps/import-warcraft.ts source.w3x target.utcmap`. Details: [importer README](../src/editor/import/warcraft/README.md).

Ground materials are ordinary declared terrain assets with editor thumbnails and painted diffuse shading. Fully erased paint layers are removed when applying terrain edits; partial layers and their variants remain editable.

Cliff-face textures are ordinary terrain assets with a declared vertical projection and slope range. Select a **Cliff face** thumbnail and paint a grid region; its coverage is independent of ground paint. It appears on steep terrain and follows later height edits. **Erase selected texture** removes only the selected material in the painted region, including cliff coverage, and supports undo. MCP uses the same `material` operation with `erase: true`. Saved maps contain material IDs and coverage only; projection settings remain in the asset declaration.
