# Canonical asset publication

The asset editor is at `http://127.0.0.1:5175/asset-editor.html` (`npm run dev:tools`).

Each authored asset lives in `art/assets/<id>/asset.json`, with role-named files in the same folder: `geometry.glb`, `source.blend`, `image.png`, etc. The definition declares roles and sequence indices, never arbitrary filenames. `source`, `recipe`, `build`, `reference`, `preview`, and `generation` are authoring-only roles and are never published.

## Working versus published state

- **Save definition** changes the working copy only. The editor shows both working and runtime revisions.
- **Save & publish** saves, checks resources and dependencies, then commits canonical runtime files, the manifest, URL module, procedural catalogue, and published snapshot together.
- **Archive** removes an asset from runtime publication while retaining authored files. Dependencies from other published assets, game content, and existing maps prevent archival until references are removed. Publish again to restore it.
- Reload an existing game or map-editor page deliberately to load the new revision. Publication does not replace the world editor's in-progress document.

The released definitions are in `assets/authoring/published.json`. Runtime binaries are in `assets/library/<id>/`; pure recipes and profiles receive `definition.json`. The procedural runtime catalogue is derived exclusively from published snapshots, so saving a draft cannot accidentally alter a different asset's publication.

`npm run assets:compile` verifies released bytes and regenerates deterministic indices. It does **not** publish working copies. Initial migration uses `npm run assets:publish` for a preflight audit, then `npm run assets:publish -- --apply` once. Initialization has already been completed for this project.

## Shared commands

The asset editor and `asset_author` MCP tool dispatch the same validated commands:

- `asset.list`, `asset.get`, `asset.publication`
- `asset.create`, `asset.save`, `asset.upload`
- `asset.validate`, `asset.publish`, `asset.archive`

Mutations include `expectedRevision`; stale writers are rejected. Publish/archive use the current working revision. The cross-process write lock covers optimistic revision checks, and a journal rolls back multi-file commits on failure. Startup recovery waits for live publishers rather than rolling back their work.

Image-generation studio publication uses the same path. The selected reviewed output becomes `image.png` in the published library and its original becomes `source.png`. The studio also attaches reference images and generation receipts while a design is under review; once it is approved, prune them so the package keeps a single master (see below).

## Editable sources and dimensions

`art/assets/<id>/` is the sole editable authority. Each package keeps exactly one file that represents the current asset, plus `asset.json` and its published outputs:

- models: `source.blend` (all textures packed), next to the published `geometry.glb` and texture/mask outputs;
- icons, interface art and painted textures: the full-resolution `source.png` original, next to the published `image.png`/`albedo.png`;
- terrain materials: the primary `source.png` or extracted `source.bin` diffuse, next to the published runtime maps;
- synthesized audio: `recipe.ts`, the generator that writes the published `audio.wav`.

Assets without a separate master (small icons, data profiles, landscape recipes) keep only their published files. Provider downloads (Tripo GLBs/JPEGs), numbered alternates, reference images, preview renders, generation receipts, staging configs and build recipes are not retained: re-export changed models from `source.blend` and upload the new `geometry.glb` through the workbench. Shared procedural generators remain in `art/recipes/` and `scripts/assets/`.

`python3 experiments/building-studio/source_workspace.py <asset-id>` and `scripts/assets/capture-build.ts` stage and capture packages that declare a `build.json` manifest. Existing packages no longer carry one, so stage a new Blender build only for a new asset and prune its staging inputs before final publication. Tool-facing names exist only in ignored `.asset-work/build/`.

Unit/creature assets may declare authoring `dimensions: {radius, height, formationSpacing}` in world units. These describe the intended body, not sword/antenna span, and guide calibration. Every gameplay unit definition must declare its own dimensions; assets never silently change balance. Height controls underpasses, and formation spacing must fit the collision diameter. Terrain clearance, routing, separation and spawn placement consume the definition's dimensions. The definition's `modelScale` is presentation-only and is shared by the game and preview tools. Movement speed and weapon reach remain explicit gameplay values.

512px is the starting target for individual unit texture maps, with 128–256px suitable for small props. Terrain tiles, packed foliage atlases and large landmarks need screen-space review before reduction. Keep high-resolution textures in the master. `node --import tsx scripts/assets/texture-size.ts <asset-id> 512` creates a smaller canonical draft; the previous GLB is kept as the master source only when the package has no `source.blend`. It records `exportSettings.textureSize`, which is reapplied when capturing subsequent Blender builds. It retains geometry, rigs, animation and team-color metadata and resizes mask alpha independently. Review at game distance and in the portrait camera, then publish. This does not globally shrink terrain, LUTs or data maps.
