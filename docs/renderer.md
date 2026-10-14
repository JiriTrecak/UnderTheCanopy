# Native renderer

Status: agreed direction, 2026-10-14. Based on a read-only audit of the old game's `src/render/` (in the old repository; summarised in §1). Implementation follows [the plan](plan.md), milestone M3. §7 was decided with Jiri on 2026-10-14: every recommendation accepted (D24).

## Decision in brief

The game moves from TypeScript + Three.js in a Tauri webview to a native Rust engine on **wgpu** (Metal first; Windows/D3D12 kept compiling from day one). The renderer is a **rewrite with its own architecture**, not a port. The current game is the *visual reference*: the target is "looks very close", and lighting is expected to be retuned. Priorities, in order: performance, predictable behaviour across machines, simple tweaking, full control.

The visual direction stays: stylized, readable, classic-RTS forest-floor game ([art direction](reference/game/art.md)).

## 1. What the audit found

The current renderer is about 13.6k dense lines in 144 files. Three.js itself provides relatively little; most features are custom. The problems come from many layered experiments built on top of Three.js internals.

### Cross-cutting problems

1. **Three different tone pipelines.** Outdoor biomes skip tone mapping entirely (`src/render/atmosphere/shaders.ts:297`: `sourceReference` is true whenever a day cycle exists), so HDR clips. Interiors and the selection portrait use ACES at exposure 1.15. A second, legacy grading system (day-cycle LUTs and height fog) is disabled in every biome (`src/content/biomes.ts:37`) but still loaded and compiled. The `0.28` exposure in `src/render/display/display.ts:120` has no effect. This is a likely root of "lighting feels wrong".
2. **Shader surgery on Three.js internals.** Roughly 55 `onBeforeCompile` patches string-replace Three's shader chunks, chained up to 5–8 per material. Fog of war is injected into every material by monkey-patching `shadowMap.render` and reading internal render lists. Programs are pinned by incrementing Three's private `usedTimes`. The look depends on patch order and is hard to reason about.
3. **Emulating another engine's lighting.** The "source"/"reference" material path recreates a different game's FX lighting (cube-map IBL, BRDF LUT, custom GGX, `RE_Direct_Physical` body rewrite) inside Three's PBR. It also drives terrain, grass and water.
4. **Light count changes shaders.** Changing the number of lights recompiles every lit program, which forced fixed pools of 4 effect lights and 4 scenery lights, a separate portrait warm-up, and dummy-mesh warm-up of every material permutation.
5. **Draw calls and CPU work scale per entity.** Health bars, status badges, selection circles, rally flags, command rings and decals each cost at least one draw per instance; circles and grids are re-draped on the CPU every frame. Buildings clone every material per instance and are never batched. Spell particles have one material each and are updated on the CPU.
6. **Duplicated truths.** Terrain colour is computed three times (shader, CPU foliage-tint bake, minimap name-matching palette) and they disagree. The asset contract is split across `asset.json` capabilities, glTF extras and material/asset names (`TC_TeamColor`, wind, grounding and the retired seasons by regex); `capabilities.teamColor` and `capabilities.wind` are validated but never read.
7. **Renderer guesses gameplay.** Hit sparks are inferred from HP decreases (with an O(N²) attacker search), aura pulses from tick rounding, melee trail windows are hard-coded (0.43–0.69).
8. **Expensive full-resolution work.** At native Retina there is no upscaling. The composite reconstructs world position about nine times per pixel. Water does full-screen colour and depth copies, a 64-tap SSR march, caustics regeneration and 60+ fetches per pixel. VSM shadows run two blur passes every frame. Fog changes rescan every cell and re-upload the whole texture (about 16 MB on a 2048 map).
9. **Terrain has no LOD.** One vertex per world unit, about 8.5M triangles on a 2048 map, and the fragment cost scales with the total number of layers on the map rather than the local ones. `terrainDisplace` runs the whole layer loop in the vertex shader and discards it (`src/render/terrain/sourceTerrainShader.ts:135`).
10. **Dead or unreachable code in hot paths.** Scenery LOD runs every frame with no data; imported "source" terrain, occlusion rasters, dew, resin shimmer, vivid foliage, underlays, `ReferenceTerrain`, the legacy effect-layer format (`src/content/effectLayers.ts`) and `Renderer.mountInspectionSubject` have no live producer.

### Good ideas worth carrying over

| Concept | Where today | Native form |
|---|---|---|
| Baked crowd animation (bone matrices in a texture, instanced per unit type) | `characters/bakedCrowd.ts` | Bake offline; per-instance bone palette from a compute pass; one draw per mesh×material |
| Animation phase-locked to sim ticks (contact frames match gameplay) | `characters/attackPhase.ts`, `abilities/castAnimation.ts` | Same pure functions |
| Analytic, seekable effects: playback is a function of `(tick − start, seed)` | `abilities/effectPlayer.ts`, `cueMotion.ts`, `flipbook.ts` | Stateless GPU particles evaluated in the vertex shader |
| Analytic projectile flight from launch/impact ticks | `settlement/projectileEffects.ts` | Effect "delivery" transform driver |
| Water ripples / unit wakes | `water/waterDisturbances.ts`, `waterInteraction.ts` | Port one-to-one (see §4.6) |
| Classic RTS camera framing and clamping | `camera/camera.ts` | Pure `CameraController`, reverse-Z |
| Texel-snapped, view-fitted sun shadow | `sky/sky.ts:112-135` | Same, with PCF |
| Day-cycle palette and canopy light pattern | `sky/`, `atmosphere/canopyLayer.ts` | One lighting environment; canopy as a sun light cookie |
| Masked team colour: luminance-preserving recolour of albedo alpha | `settlement/maskedTeamColor.js` | Same formula, per-instance colour, explicit mask flag |
| Screen-door concealment, scenery cutaway stipple | `characters/concealment.ts`, `visibility/sceneryCutaway.ts` | Shared dither function |
| Multi-floor fog (bridges, decks) and the fog look | `visibility/` | Shared fog function, dirty-rect uploads |
| Terrain concept: height-blended layers, corner-tile terrain atlases, slope-projected cliffs | `terrain/` | Texture-array splat, CPU-resolved tile ids, triplanar cliffs |
| Capsule unit picking with deterministic tie-break | `settlement/unitPicking.ts` | CPU picking service, decoupled from rendering |
| Per-instance budgets and leases for effects | `abilities/` | One global effect budget |

## 2. Goals and non-goals

**Goals**

- One explicit frame graph; every pass has named inputs and outputs.
- One lighting model and one shared shader library. No patching.
- Draw calls do not grow with unit, building, health-bar or particle counts.
- Pipelines do not depend on the number of lights or on the render target.
- All tunables are data, hot-reloadable, with live in-game controls.
- Rendering consumes a declared input contract; it never mutates or infers game state.
- The same renderer later powers the asset workbench and spell preview (natively, or compiled to WASM/WebGPU inside the existing tools).

**Non-goals for v1**

- Physically based metal/roughness, IBL cube maps, screen-space reflections.
- Imported "source" maps and their occlusion/sway/shading paths.
- Volumetric light shafts and depth of field (possible later quality tier).
- Dew, resin shimmer and speech jaw animation.

## 3. Engine layout

The crates are listed in [structure](structure.md). For rendering:

| Crate | Owns |
|---|---|
| `canopy-render` | wgpu device, frame graph, passes, materials, shader library. No game logic, no windowing. |
| `canopy-scene` | `RenderWorld`: flat tables (units, statics, effects, world UI, decals) built from simulation snapshots; interpolation; animation state selection. |
| `canopy-assets` / `canopy-cook` | Runtime asset format and loader / offline cooker (glTF → runtime format, textures → BC7). |
| `canopy-ui` | Game HUD and menus: layout, text, widgets, drawn through `canopy-render`. |
| `game/` | winit window, input, camera control, audio, settings, egui debug overlay. |

The renderer receives a `FrameData` value per frame (camera, lighting environment, render-world tables, overlays) and allocates nothing in the steady state.

## 4. Rendering design

### 4.1 Frame graph (v1)

1. **Upload**: dirty ranges only (instance buffers, fog rects, terrain edits, ripple ring buffer).
2. **Compute**: bone palettes for visible units; scenery/grass culling and indirect-argument compaction.
3. **Shadow**: one view-fitted, texel-snapped sun map with PCF. Separate depth-only vertex streams for skinned and wind-animated meshes.
4. **Ripple target**: one instanced draw into the 160×160 m wake texture.
5. **Opaque forward pass** (HDR, MSAA 4×; Apple GPUs resolve it in tile memory): terrain, statics, units, scenery, grass.
6. **Water**: samples one resolved copy of opaque colour and depth.
7. **Transparent and effects**: GPU particles, ribbons, effect meshes, ground decals.
8. **World UI**: one instanced screen-space quad pass for health/mana bars, badges, labels, selection and command rings, rally routes.
9. **Post**: optional half-resolution AO, bloom mip chain, then one *resolve + tone map + grade* pass.
10. **Game UI**: HUD, minimap and portrait textures, tooltips.
11. **Debug**: egui and debug-draw lines.

Render scale is a setting (with a sharpening upscale) so Retina displays need not render every pixel.

### 4.2 Lighting and colour

- One `LightingEnvironment` uniform block per frame: sun direction/colour/intensity, hemisphere sky/ground ambient, fog parameters, canopy cookie transform.
- One stylized surface function shared by every material: wrap diffuse, hemisphere ambient, shadow, AO on ambient only, optional rim/translucency, optional specular. Material models are an enum with numeric parameters: `unit`, `building`, `foliage`, `bark/prop`, `terrain`, `water`, `effect`.
- Point lights live in a buffer with a count (clustered later if needed). Light count never changes pipelines.
- One tone mapper for everything (decision Q1), followed by a per-biome/time-of-day 3D LUT. Grading sliders in the debug UI bake into that LUT.
- The canopy pattern becomes a cookie projected along the sun: it modulates direct light only, consistently on opaques, water and particles.

### 4.3 Shared shader library (WGSL)

`apply_fog(world_pos, color)`, `team_color(albedo, mask, instance_color)`, `wind_offset(...)`, `dither_discard(alpha, frag_coord)`, `cutaway(...)`, `surface_light(...)`, `terrain_height(...)`. Every material includes the library; no material is patched by another system.

### 4.4 Terrain

- Heightfield as an R16/R32F texture; chunked grid, vertex-pulled heights, chunk LOD (CDLOD or geomipmaps, chosen by maximum map size: Q9).
- Normals computed on the GPU or baked when terrain changes.
- Layer weights in RGBA8 weight textures (4 layers each); a per-cell "top 4 layers" index bounds per-pixel cost.
- Corner-tile terrain atlases resolved on the CPU into a per-cell tile-id texture: no corner maths per pixel.
- Cliffs use slope-triplanar projection.
- One GPU bake of terrain albedo after each edit feeds grass/tree ground tint and the minimap: one source of truth.
- Editor grid, brush and navigation overlay become terrain-shader overlay uniforms, not geometry.

### 4.5 Units, buildings and scenery

- **Units:** offline-baked clips (30 Hz, model-space 3×4 matrices in v1). A compute pass blends up to the current and one fading pose into a per-instance palette. One `draw_indexed` per mesh×material plus one depth draw. Instance data comes straight from the render world: transform, palette offset, team colour, dither alpha, flags.
- **Buildings and props:** always instanced per mesh×material; team colour and emissive masks per instance or per material. No material clones. Construction scaffold uses a per-instance clip height; placement ghost is a pipeline variant.
- **Scenery:** POD placement records in persistent GPU buffers sorted by prototype and cell; GPU frustum culling with compacted indirect draws per view. Felled trees use the unit animation path as rigid one-bone skins.
- **Grass:** per-chunk instance buffers, one indirect draw per asset, distance density fade, shared wind and ground tint.
- **Map border (forest surroundings):** an instance list generated once at load, using the shared foliage material with distance haze. Camera far plane and background are explicit renderer settings.

### 4.6 Water

Redesigned from scratch as stylized water: depth tint, shore foam, wave displacement and a sky gradient instead of SSR or cube maps. Keep three things from today:

- The data model: profile table, per-cell flow/foam/profile id, bed heights.
- Depth-dependent wave amplitude without phase change (`water/stylizedWaves.ts:14-17`).
- **Ripples, ported one-to-one.** The CPU emitter tracks wet, visible units, emits a ring every 1.7 m of travel (at most 3 per unit and 256 per frame), with strength from depth and speed and stretch from speed, into a 1,536-ring buffer that expires after 2.8 s. The GPU pass draws all rings in one instanced additive draw into a 768² RGBA16F texture covering 160×160 m around the camera focus: a derivative-of-Gaussian crest `exp(−2q²)` with antialiased width, encoding slope in RG and water level in BA. The water shader saturates the slope `rg/(1+3|rg|)`, rejects rings from a different water level, fades them in shallows, and adds a sun-facing light term so ripples stay readable.

### 4.7 Fog of war

The simulation sends fog changes with dirty rectangles. The renderer writes only those rects into an R8 state texture (one layer per stacked floor); smoothing happens on the GPU. Every lit shader calls `apply_fog`. The minimap samples the same texture.

### 4.8 Effects

A data-driven GPU effect system with six primitives: **particle emitter** (stateless, evaluated from seed and age with curves), **ground decal**, **mesh layer**, **ribbon** (trails, beams and melee trails), **light**, **sound**. Missiles, wavefronts and beams become *delivery drivers* that move layers. The 170 shipped effects (307 layers) use only these building blocks; a converter migrates the current JSON to a v2 format, checked against reference captures. One global budget replaces the three per-player budgets.

### 4.9 World UI and interpolation

- All in-world UI (bars, badges, rings, labels, rally routes, ability aim, placement grid) is one instanced quad/decal pass. Per-instance data: world anchor, pixel offset and size, kind, values, colour, icon layer. The current health-bar fragment logic ports as is.
- Transforms interpolate between the two latest snapshots using the fractional render tick (a fixed one-tick delay) instead of exponential chasing. `render_tick: f64` is the only time input for every system.
- The simulation emits explicit presentation events (hit with source and direction, aura pulse) instead of the renderer inferring them.

## 5. Asset contract and cooker

The runtime never loads glTF or matches names. An offline cook step produces, per asset:

- Mesh streams: quantized positions, octahedral normals, UVs, optional AO/tint, wind pivot data; joints and weights for skins.
- Material sections with a material-model enum and numeric parameters (replaces `referenceEnvironment`, `painted`, `foliage`, `TC_TeamColor` and similar extras).
- Pivot, scale and soil-line offset folded in.
- A merged depth-only stream (positions plus skin).
- Skeleton, clips, animation state table with loop flags and normalized event times, socket tracks.
- Team-colour mask channel and flag; local bounds with animation/wind padding.
- Per-prototype data: blockers, lights, wind.
- BC7 textures, with ownership masks preserved losslessly.

`asset.json` capabilities become the single source of these declarations; glTF extras and name conventions are import-time inputs only. In phase 1 the cook reads the published outputs mirrored into `assets/` from the old repository.

## 6. UI

- **Game HUD and menus:** custom, drawn by `canopy_render` in one batched quad pass. Layout with `taffy`, text shaping with `cosmic-text`, layouts in hot-reloaded data files. The UI reads renderer-neutral presentation models (the old game's `src/presentation/` is the behaviour reference) and issues commands; it holds no gameplay logic. Widgets: command cell/button, icon slot, bar, label/rich text, tooltip, scroll list, text field, slider/checkbox/dropdown.
- **Minimap and portrait:** rendered to textures on the same device and composited by the UI. The portrait uses the world tone mapper.
- **Debug and tuning:** egui. Performance scopes, AI debug, and live lighting/grading/water controls.
- **Editor, Asset Studio, Spell Editor:** stay web/React initially; adopt the native renderer later (Q12).

## 7. Open decisions

Decided 2026-10-14 (D24): every recommendation in **bold** is accepted.

| # | Question | Recommendation |
|---|---|---|
| Q1 | Tone mapper | **One filmic curve (AgX or tuned ACES) for everything, plus per-biome LUT** |
| Q2 | Antialiasing | **MSAA 4×**; TAA only if foliage shimmer requires it |
| Q3 | Shadows | **Single view-fitted map with PCF**; cascades only if close unit cameras need them |
| Q4 | Volumetric shafts, depth of field | **Defer to a later quality tier** |
| Q5 | Imported "source" maps and their shading | **Cut** |
| Q6 | Dew, resin, vivid foliage, speech rigs | **Cut** |
| Q7 | Terrain layer cap | **4 blended layers per cell plus atlas tiles and cliffs** |
| Q8 | Rock displacement | **Normal-mapped only in v1**, real displacement later if missed |
| Q9 | Largest map size and weakest target GPU | Decided: maps up to 1024×1024 (larger ones refused); weakest GPU base M1 (7-core), Apple Silicon only (D24) |
| Q10 | Seasons and tree colour variants | Decided: no seasons, ever. A biome is the whole configuration; a "winter forest" is just another biome (D24) |
| Q11 | Effect format | **v2 primitives with an automatic converter from v1** |
| Q12 | Workbench and spell preview renderer | Decided: editors stay in the old repository with TS previews until phase 2; the renderer stays WebGPU-compatible so they can embed it then |
| Q13 | Animation clips | **Model-space matrices in v1**; local TRS later for additive/IK |
| Q14 | Portrait | **Live 3D render to texture** |
| Q15 | Interpolation | **Two-snapshot interpolation with a one-tick delay** |
| Q16 | Presentation events from the simulation | **Yes**: hits with direction, aura pulses |

## 8. Delivery plan

Superseded by [the plan](plan.md): the renderer is milestone M3, after the Rust simulation (M2), so there is no snapshot recorder for the old game and no TypeScript bridge. Each step is still measured on the reference machine (base M1) with fixed scenes, and reference screenshots from the old game are captured before work starts.
