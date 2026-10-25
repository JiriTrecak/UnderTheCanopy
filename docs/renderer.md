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

As built (M3.4, D34): the surface keeps the old shading model term for term (absorption, body colour, caustics, swell bands, the old woodland reflection cube, glints, shore/crest/flow foam, cloud shading) but needs no copy of the scene: it is drawn inside the MSAA forward pass in two draws over the same fragments, a multiplicative one that tints the bed by the hue of the column's absorption and a premultiplied one for everything else. Refraction and screen-space reflections are left out (about 2 px and 2–4 % of the colour at RTS pitch). See §9 for the details.

### 4.7 Fog of war

The simulation sends fog changes with dirty rectangles. The renderer writes only those rects into an R8 state texture (one layer per stacked floor); smoothing happens on the GPU. Every lit shader calls `apply_fog`. The minimap samples the same texture.

### 4.8 Effects

A data-driven GPU effect system with six primitives: **particle emitter** (stateless, evaluated from seed and age with curves), **ground decal**, **mesh layer**, **ribbon** (trails, beams and melee trails), **light**, **sound**. Missiles, wavefronts and beams become *delivery drivers* that move layers. The 170 shipped effects (307 layers) use only these building blocks; a converter migrates the current JSON to a v2 format, checked against reference captures. One global budget replaces the three per-player budgets.

As built (`crates/render/src/effects.rs`, `shaders/effects.wgsl`, `crates/scene/src/effects/frame.rs`, D69, D76):
- Particles are stateless: the vertex shader evaluates each from its emitter record (304 bytes), index, seed and age. Curves over life live in curve blocks (3 rows of 32 samples: colour and opacity; size, additivity, erosion and heat; the ramp) loaded once; world-space emitters carry 32 samples of their path (where the emitter was over the births still alive), written each frame, from which each particle finds its birth point and inherited velocity.
- Effects draw in their own pass after the forward pass, into the resolved HDR colour without a depth attachment, reading the resolved linear depth: hidden behind surfaces, soft where they meet them (ground decals cut hard). Every part blends premultiplied (`one, one − src alpha`) with its additivity; buckets keep additive parts first, then covering parts back to front.
- Ribbons: trails and beams (camera-facing segments), melee blade quads, and textured strips grouped by texture. Beams are jittered segments redrawn 15 times a second.
- Rain draws shards (or models) falling onto a disc, a flash and splashes where each lands.
- Effect lights: 8 (`MAX_EFFECT_LIGHTS`), shared with working buildings (D65).

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

## 9. Implementation status (M3.5 effects, fog and world UI; M3.4 water and post; M3.3 live world)

What exists, in `crates/render` (`canopy-render`), `crates/scene` (`canopy-scene`), `config/` and `game/`:

- **Device** (`device.rs`): WebGPU default limits; optional WebGPU features `texture-compression-bc` (D26, CPU decode without it) and `timestamp-query` (pass timings, none without it). Nothing native-only.
- **Frame graph** (`graph.rs`): ordered passes with named inputs and outputs, checked when built (no read before write, no dead writes, one output); the renderer records exactly that list, with a tracing span and a GPU timestamp pair per pass. The graph is rebuilt when the world, the biome or the quality settings change: passes exist only when they run (`FrameFeatures`).

  | Pass | Kind | Reads | Writes | When |
  |---|---|---|---|---|
  | `cull` | compute | — | visible statics (camera and sun lists, indirect counts) | always |
  | `skin` | compute | — | joint palettes of the visible live models | always |
  | `shadow` | render | visible statics, palettes | sun shadow map (2048², depth) | always |
  | `ripples` | render | — | ripple field (768² RGBA16F, 160 m around the focus) | the world has water |
  | `caustics` | render | — | caustics (256² R16F) | the world has water |
  | `forward` | render, MSAA 4× HDR | visible statics, palettes, shadow map, ripples, caustics | scene colour and linear depth (resolved in the pass), depth | always |
  | `contact` | render | linear depth | contact shade (R8, effect size) | biome contact strength > 0 and the setting on |
  | `bloom` | render ×3 | scene colour | three bloom levels (effect size, ½, ¼) | biome bloom strength > 0 and the setting on |
  | `haze` | render ×2 | linear depth, shadow map | haze (march, then depth-aware filter) | biome atmosphere on and the haze quality not off |
  | `post` | render | everything above | output (surface or capture) | always |
  | `ui` | render | output | output (through a non-sRGB view) | always (draws when the game submitted a HUD frame; `ui::UiLayer`, D36) |

  MSAA colour, depth and linear depth are transient attachments (memoryless on Apple GPUs), cleared and discarded inside the forward pass. A depth prepass only for the opaque sections of canopy scenery the cutaway or the RTS ceiling may cut this frame (see Statics). **Linear depth** is a second colour target of the forward pass (`targets::LINEAR_DEPTH_FORMAT`, RG16F holding the view depth rounded to 11 significant bits and the remainder, so interior pixels keep full precision through the averaging resolve; WebGPU cannot resolve depth or R32F). Every pipeline drawn in the forward pass declares `targets::forward_targets(blend, writes_depth)` and opaque shaders return `scene_out(colour, world)` (`frame.wgsl`); transparent ones mask the depth target. The water writes its surface, so post effects stop at the water as the old ones did.
- **Shared WGSL library** (`shaders/lib/`, `shader.rs`): `frame` (group 0: frame uniform, shadow map, canopy pattern, samplers, point lights, `SceneOut`), `lighting` (`surface_light`), `material` (group 1: cooked materials and `shade_material`, shared by statics and live models; `material.rs` builds the bind groups and caches textures), `team`, `wind`, `dither`, `packing`, `color`, `view` (post passes: view uniform, linear depth, surface reconstruction, full-screen vertex). A shader is library modules followed by its own source; every shader is validated by naga in the tests.
- **One lighting model** (`lighting.wgsl`). `surface_light` with two ambient models: *standard* (units, buildings, props, foliage: Lambert + GGX sun, flat ambient + sky/ground hemisphere over π, environment irradiance from 9 SH coefficients, split-sum reflections) and *ground* (terrain: the old ground lighting, energy-conserving GGX, ambient tinted by the ground probe). Both take the 8-tap rotated Vogel PCF sun shadow and the canopy shade. Painted materials drop specular; foliage wraps light (`backside`). Point lights come from a buffer with a count; light count never changes pipelines. The same buffer holds a light grid over the ground (`frame::light_buffer_contents`, at most 64×64 cells over the lights' reach, a 64-bit mask per cell of the lights whose range reaches it): a surface sums only its cell's lights, in their order, so the sum is the same as over all of them. **Night on units** (D83, `night_unit_light` in `material.wgsl`): live models (units and buildings, not effect models or scenery) add a moonlit rim where their surface turns from the camera, strongest on its upper side, and a fill of their own albedo in the rim's hue that no shadow takes away, from the biome's `light.nightUnits` (`rimTint`, `rim`, `fill`) weighted by the night (full by night, half at dusk and dawn, none by day; `Frame::units`); the portrait keeps its own light. The ground and scenery keep the night's mood.
- **Final pass** (`post.rs`, `post.wgsl`): upscale from the render scale (bilinear with a light unsharp mask; one texel at native scale), contact shade (bilinear × surface-similarity upsampling, old `contactShade`), bloom (levels weighted 0.2/0.3/0.5, times the canopy shade at the surface), haze (`colour · transmission + scattered`, depth-aware upsampling), then display encoding → the one tone mapper → baked grade volume → vignette. The tone mapper is the old hue-preserving highlight shoulder; every current biome has knee 0, so values clip at 1 exactly as today's outdoor frames (D24: today's look first). The grade volume (32³) is baked on the CPU from the biome finish and, when a biome asks for it, the pack's daytime lookup.
- **Post effects** (`post.rs`; old `BeautyPass` and `AtmospherePass`, data from `base/landscape/biomes/*.json` `contact`, `bloom`, `atmosphere`):
  - *Contact shading*: at the effect size (half the scene at most, about 180,000 pixels: 565×318 at 1080p), 12 golden-angle taps within the biome's world radius, horizon over the reconstructed tangent plane minus the bias, `clamp(1 − 3·mean, 0.15, 1)`; mixed by the biome strength.
  - *Bloom*: soft-threshold prefilter, then three 3×3 tents (radius 1, then the biome radius × 2) at the effect size, ½ and ¼.
  - *Volumetric haze*: a bounded march (the last 96 m before the surface, inside the mist layer `base + 5·falloff`) with height-falling density, a 32³ hashed noise volume drifting with the wind, the sun through one shadow-map tap and the canopy's openings (a second canopy texture: the unshaded part of the pattern feathered by finer noise, old `canopyMask` green), and separate clear-air shafts. Quality low/medium/high = scale 0.33/0.5/0.66, 16/24/40 steps, at most 180k/360k/900k pixels (old `atmosphereQualitySpec`). Then a depth-aware 3×3 filter. Deep forest and amberleaf use it; no shipped map does.
  - Old quirk not kept: the old composite clipped every surface at a water plane of height `waterLevel − 0.03`, and committed-cell maps never set `waterLevel` (0), so contact shading was off on all ground below y ≈ 0.05. Here the water's own surface stops the effects (D34).
- **Quality** (`quality.rs`, `Renderer::set_quality`): render scale 0.5–1 (the scene, with MSAA, renders at the scaled size; the final pass upsamples), upscale sharpening, contact shadows on/off, bloom on/off, haze off/low/medium/high. `canopy` and `canopy shot` take `--render-scale`, `--no-contact`, `--no-bloom`, `--haze`.
- **Terrain** (`terrain/`): one vertex per unit over map and halo, 32-cell chunks with 4 geometric mip levels chosen by projected height error (≤ 1 px), heights pulled from a texture, edges stitched to coarser neighbours in the vertex shader (no cracks), frustum-culled on the CPU (≤ 1089 chunks on a 1024 map); one draw per level. Layers (old `authoredTerrain`): six built-in biome layers, the map's atlas grounds (corner tiles per texture cell, full-tile variants) and biplanar cliffs; each one-unit cell lists at most 4 layers starting from the lowest that covers it completely (Q7). Masks, atlas codes and layer lists are computed once at load (`canopy-scene` + `terrain::select_layers`). Tile textures are 2D arrays of the packed runtime tiles. Bridge decks need nothing special: terrain under them renders as everywhere (the deck is a model).
- **Water** (`water/`, `water.wgsl`, `ripples.wgsl`, `caustics.wgsl`; old `waterSurface`, `sourceWaterShaders`, `stylizedWaves`):
  - *Data*: the terrain's bed and water heights per vertex, the map's flow grid per cell (direction, foam, profile slot) and a profile table: slot 0 the biome's water, slots 1–255 the map's `waterProfiles` (old rule: painting a river never recolours other lakes). Profiles come from the content catalogue (`waves` or the old `DEFAULT_WATER_WAVES`). `config/water.json` names the shared textures: the old woodland reflection cube (a data asset of sRGB cube faces) and the caustics image.
  - *Surface*: the cells the old water drew (centre or a corner within 0.15 of the water), as one u32 index list over the terrain's vertex grid, split in 32-cell chunks culled on the CPU and merged into contiguous draws. Levels (R32F) and depth (R16F, filtered) are pulled from textures. Swells move vertices with the old field: depth-scaled amplitude `min(0.18·depth, height·mix(shallowStrength, 1, open))·smoothstep(0, 0.16, depth)`, phase advected by the flow, never by depth.
  - *Shading*, per fragment, the old terms: absorption `exp(−column/clarity·(1.7, 0.9, 0.65))` and the shallow-colour tint on the bed, caustics (generated each frame by the old mirrored-tiling generator into a 256² texture), body colour from shallow to deep by column, ripple light, swell bands by `n·l`, the reflection cube compressed `r/(1+r)·0.45` with Fresnel × reflection strength, sun glints `pow(n·h, 120)`, shore foam from the bank distance (depth / bed slope) with rounded cell islands, crest foam, flow foam, cloud shading, the canopy shade. Normals: forward differences of the displaced height (amplitude evaluated at the neighbours) plus the old fine ripple. Noise uses a 256² hash lattice with one filtered fetch per value-noise sample.
  - *Blending without a scene copy*: two draws over identical fragments in the forward pass, after every opaque surface, depth-tested without depth writes: `fs_absorb` multiplies the bed by the absorption's hue (luminance 1), `fs_main` blends the surface with premultiplied alpha carrying the rest of the bed's dimming. The result is the old per-channel transmittance exactly (measured on echo-isles shallows: within 1–2 of 255 of the old frame). Left out: refraction (≈ 2 px at RTS zoom) and screen-space reflections (2–4 % of the colour at the 56° pitch).
  - *Ripples* (`water/ripples.rs`): the emitter above, one-to-one, with an API the live world feeds every frame: `Renderer::ripples_mut().update(&movers, &field, seconds, focus)`, where a `Mover` is an id, position, walking surface and elevation, and `WaterField` answers water and walking heights (`TerrainDesc` implements it for the ground; the scene wraps it for bridge decks). The caller passes only bodies that may wake water (visible, alive ground units not contained, garrisoned or flying). `canopy shot --movers <n>` walks synthetic bodies through the view.
- **Surroundings** (`surroundings.rs`, `surroundings.wgsl`; old `ForestSurroundingsLayer`, data in `base/landscape/biomes/*.json` `surroundings.forest`): the floor (three rows to 420 units, with the terrain) and the forest: trunks on square rings around the map (the old seeded LCG and `forestAnchors`, so placement matches), forked limbs (centripetal Catmull-Rom tubes with the bark texture), giant mushrooms and fallen logs, and the overhead sheet of broad leaves (44k on a 640 map). Trunks, mushrooms and logs are pack models, CPU-culled per instance, one draw per model section; limbs and leaves only draw when the camera looks towards the horizon. As in the old game, the tall parts of trunks beyond the border dissolve at RTS pitches, everything hazes towards the biome's haze colour with a glow towards the sun, and the background turns towards the haze as the view rises. At the play pitch the camera's map fit keeps all of it out of view; it shows in free views (`canopy shot --pitch`).
- **Statics** (`statics.rs`): every scenery stamp and every tree of the map in one instance buffer (the live scene hides struck and gone trees with a 16-byte write each, `set_hidden`); a compute pass culls them against the camera and the sun and builds per-model visible lists and indirect counts; one indirect draw per model section (+ depth-stream and alpha-tested shadow draws). Opaque sections draw without a discard, so the GPU shades only visible surfaces; a canopy instance the cutaway or the RTS ceiling may cut this frame draws a depth prepass carrying those discards first (`vs_prepass`/`fs_prepass_cut`), then shades with depth equal (`vs_lit`/`fs_lit`, invariant positions). The culling pass decides per instance and puts those in a third list of their model: it reaches the RTS ceiling, or a unit stands more than the cutaway's 0.4 m behind its nearest point in a block of the cutaway mask its screen rectangle reads (`CutawayMask::block_depths`, 8×8-texel blocks of the farthest covering unit's depth). The CPU keeps the instances bucketed into 32 m ground cells (`ModelCells`): a model none of whose cells touches the camera's or the sun's frustum records no draws at all instead of empty indirect ones. The sun's list keeps only casters whose shadow can reach the camera's view (`shadow::CasterCull`: the bounding sphere, grown by every offset a shadow read takes, swept away from the sun to the map's far plane or below the lowest ground, against the camera's side and near planes); depth-only shadow draws have no fragment stage unless a giant reaches the biome's shadow ceiling. Live models cull their casters the same way, and opaque sections of fully opaque live instances draw without the screen-door discard. Without `indirect-first-instance` each list is reached through a dynamic storage offset. Foliage is alpha-tested with alpha-to-coverage under MSAA and sways (old "source" trunk sway + leaf flutter, same in the shadow pass). Team colour is the old masked formula; owner colours come from `config/players.json`. Data-driven deposit glow (`config/glow.json`). Static poses only.
- **Live models** (`actors.rs`, `shaders/actors.wgsl`, `shaders/skin.wgsl`; D35): units, buildings, deposits, items, construction scaffolds, carried goods, corpses, felled trees, missiles and shells. The caller hands over posed instances (model, affine transform, owner colour, opacity, up to three clip poses); each frame they are culled on the CPU against the camera and the sun, sorted by model with the camera-visible ones first, and uploaded with one skinning job each; the `skin` pass blends each instance's joint palette from the baked 30 Hz model-space clips (two frames per pose, three poses); one instanced draw per model section (one depth-stream draw and the alpha-tested sections for shadows) reads its contiguous range. Rigid models skin with one identity joint. Draw calls follow the models in view, never the instance count. Models load when first listed (the scene preloads the match's rosters).
- **Live scene** (`canopy-scene`: `live.rs`, `animation.rs`, `interpolate.rs`, `models.rs`; D35): every simulation tick the presenter turns the view into posed models on the simulation thread, with the old game's animation state choice and clocks advanced per tick; the render thread interpolates the two presentations around its clock, one tick behind the simulation (Q15).
- **Camera** (`camera.rs`): the old play camera as a pure controller (see "Reference captures"); `game/` adds input: arrow keys and edge scrolling (28 units/s), wheel and PageUp/PageDown zoom with the old momentum, middle-drag grab pan; as a spectator also Q/E to turn and zoom out to 6×.
- **Configuration** (`config/`, D30): `lighting.json` (daylight palettes, phases, probes, shadow fitting), `biomes/<id>.json` (light, canopy, grade, built-in ground layers, water profile, surroundings with the forest, ground cover, bloom, contact shading and atmosphere), `water.json` (reflection cube and caustics), `players.json`, `glow.json`.

### Performance (M3.4)

Measured on the development machine (M4 Max, 40-core GPU; no base M1 was available) with `canopy shot --frames 120`: the median submit-to-idle time of a frame, best of three runs, before this step and after it, on three views (echo-isles player start 117.5,225.5 zoom 1; echo-isles shallow channels 200,360 zoom 1; turtle-rock west edge 20,320 zoom 1.25 over deep water). Submit-to-idle includes about 1 ms of fixed CPU and synchronisation cost; per-pass GPU timestamps on Apple GPUs attribute work to neighbouring passes (the final pass always reads 0 ms), so they are only indicative.

| Setting | Before | After |
|---|---|---|
| 1920×1080 | 1.80–1.92 ms | 1.96–2.07 ms |
| 1920×1080, contact shading off | — | 1.74–1.90 ms |
| 2560×1600 | 2.42–2.51 ms | 2.78–3.18 ms |
| 2560×1600, render scale 0.75 | — | 2.25–2.36 ms |
| 1920×1080, deep forest (bloom and haze medium) | — | 2.42–2.46 ms |
| 1920×1080, deep forest, haze low | — | 2.22–2.30 ms |
| 2560×1600, deep forest, render scale 0.75 | — | 2.64–2.89 ms |

What it costs: the water with its two draws, ripple field and caustics about 0.1–0.2 ms in a view that is 60 % water (the full old shading cost 0.75 ms until shore foam was evaluated only where it can show and the water took one shadow tap); the contact pass and its upsampling about 0.15–0.25 ms at 1080p; the forest nothing at play pitch (CPU-culled); the linear depth target nothing measurable. Bloom and medium haze add about 0.4 ms, low haze 0.25 ms. Scaled to the base M1 (about 7× less GPU throughput and 8× less bandwidth), the vibrant forest's additions are roughly 1–2 ms at 1080p and 3–5 ms at 2560×1600 native; at render scale 0.75 a Retina frame costs less than before this step. Budget recommendation (D34): render scale 1 at 1080p, 0.75 on Retina panels, haze low.

### Effects, fog of war, world UI, minimap and portrait (M3.5, D40)

- **Effect conversion** (`canopy-cook`, `effects.rs`; Q11): the cooker reads the base package's effects (`base/effects/`) unchanged and writes `@effects` (`canopy_assets::EffectTable`): every enabled layer becomes a runtime primitive — *particle* (camera-facing sprites, or soft spheres when untextured), *decal* (quads on the ground: textured `orientation: ground` layers, untextured rings and glows), *mesh* (mesh layers and model missiles), *light*, *sound* — with a *motion* (billboard, burst, streaks, emitter, ring, glow, pillar, missile, wavefront, model, light, sound) and a *ribbon* when it leaves a trail. Ticks become seconds, colours linear RGB, degrees radians, texture and model references pack entry names. The old `EFFECT_IMAGE_LIMITS` are checked at cook time (2048 px a side, 8 MB a file, 64 MB resident); model triangles (`EFFECT_MODEL_LIMITS`) at run time. `rain` and `beam` (no shipped effect uses them) are converted but not drawn (cook warning).
- **Cues** (`canopy-scene`, `effects/mod.rs`, simulation thread, part of the presenter): recipes bind the table to each ability's presentation; every tick the director plays the view's new ability events, follows statuses (aura pulses every 200 ticks), deliveries in flight (with their trail history), returning heroes and landed shells (impact effects), and keeps each cue's root on its anchor (`follow`), socket (attachments) or the weapon missile carrying its cast. The presenter adds construction dust (three puffs per interval while work advances in sight, old seeds), hit sparks on confirmed health loss (8 sparks over 12 ticks, direction from the swinging attacker), dust along the swing on melee blows that take health, fires on damaged buildings in sight (D46, `damage.rs`) and blade ribbons (the `socket_blade_base`/`socket_blade_tip` sockets through phase 0.43–0.69 of a swing, four samples per tick). Timing is the old player's exactly: start delays, durations (`durationFrom: delivery`), finite ends of sustained or looping layers, loops, status lifetimes, `cancelled`, lifetimes plus particle life. Budgets: 256 cues, 4 lights, 16 effect models and 80,000 of their triangles, 10,000 per model.
- **Evaluation** (`effects/frame.rs`, render thread): the cues of the two presentations around the render time are blended (positions) and evaluated at that exact time — the old envelope, cue motion pulses, ring growth, pillar taper, emitter emission count — into compact `Emitter` records (224 bytes); effect models become live models with a tint (`ActorInstance::tint`: alpha below 1 blends over the scene and casts no shadow), lights join the frame's point lights after the map's own.
- **GPU** (`canopy-render`, `effects.rs`, `shaders/effects.wgsl`): each part's position, size, rotation, colour and alpha are computed in the vertex shader from its emitter, index and the old player's seeded `noise` (the same integer hash, so a cast looks the same every time): burst spirals, streak columns, emitter cones with drag and downward gravity (particles may stretch into streaks along their velocity), rain (shards falling onto a disc, each landing in a flash and splash fragments; model rain draws the model per drop as an effect model), flipbooks, hit sparks, travelling sprites (elongated heads follow their flight), and the damage effects (D46: flame licks with noise-torn edges and a base glow, light smoke wisps and hit dust, all with procedural masks; flames drawn last; the nearest 28 fire spots and 64 dust bursts within 2,048 parts after the cues); the fragment applies the shape (texture, soft sphere drawn as its two faces, fresnel missile, ring band, glow falloff, pillar fade), a layer's white-hot `core` and the fog. Untextured rings and glows lie flat unless authored facing the camera. Parts are grouped by geometry (quads, the 24-sided pillar), blend (additive, normal; normal ones back to front by emitter) and texture: one instanced draw per group, plus one for every ribbon. Budgets: 2,048 emitters, 16,384 parts, 4,096 ribbon quads, fixed buffers.
- **Fog of war** (`fog.rs`, `canopy-scene` `fog.rs`): the presentation carries the viewer's `FogView`; the render thread follows it by revision (`changes_since`), folds bridge decks into the ground cell under them, and the renderer keeps the old raster (in sight 255, explored 90, unexplored 0, 5×5 box filter) incrementally: only the rectangle around changed cells is filtered and uploaded into an R8 texture sampled bilinearly. Every forward surface goes through it in `scene_out` (old look: `mix(grey·(0.82, 0.91, 1), colour, smoothstep(0.25, 0.85, light))·light`, black outside the map), so terrain, water, scenery, units, remembered structures and effects all darken alike; the haze march is masked by it (old `visibleAt`). Observers have none (a white texel; the function returns the colour).
- **World UI** (`world_ui.rs`, `shaders/world_ui.wgsl`, `canopy-scene` `world_ui.rs`): the presentation lists every unit and building's facts (`Body`: health, mana, radius, pick height, rally, workforce, item badges, progress, cast, footprint); the game fills a plain `WorldUiInput` (selected and hovered ids, health-bar mode, order markers, placement, viewer, DPI) — in play from `play::WorldUi`. One shader, two draws: *ground* triangles inside the forward pass, depth-tested (selection rings draped on the walking surface with the old radial profile per pixel, rally routes, the rally ring, the placement grid and entrance), and the *overlay* in pixels after the final pass and before the HUD (the old vital bar shader with 100-HP separators, mana, item badges, the mine's "Workers n/m" label in the bundled HUD font rasterised once with cosmic-text, rally flags, order markers, optional progress and cast bars). Draw calls do not depend on how much is shown. Buildings' contact shadows are draped once and kept while the building and the ground stay (`WorldUiInput::ground_revision`), culled to the view and passed as the frame's `under` list, shared, which the renderer uploads only when it changes (D104).
- **Minimap** (`minimap.rs`): its background is the world rendered once straight down with an orthographic camera when the match starts (no post effects, fog, vignette or shadows); each frame a composite darkens it by the fog (old overlay: explored 65 %, unexplored black) and draws markers (units 2 px, buildings at their footprint, player colours lifted ×1.4, deposits yellow, remembered at half strength) and the camera's ground quad. 256² sRGB, premultiplied.
- **Portrait** (`portrait.rs`): the selected unit's live model (the same buffers, skinning and materials, its idle clip playing) rendered alone into a 256² MSAA HDR target with the old portrait's framing (orthographic, front and a little above, turned −0.32 rad) and key light, resolved to sRGB with premultiplied alpha.
- **Frame graph**: `cull`, `skin`, `shadow`, [`ripples`, `caustics`], `forward` (… water, world UI ground, effects), [`contact`, `bloom`, `haze`], `post`, `world_ui`, [`minimap`, `portrait`], `ui`.
- **Measured** (M4 Max, 1080p, `--frames 120`, median submit-to-idle): an open echo-isles view at zoom 0.6, 1.49–1.55 ms without effects and 1.81–1.85 ms with Holy Light, Crownfall and Faultline's impact filling the middle (149 parts, 2 lights, 6 draws); twelve Crownfall impacts at zoom 1.25 (72 emitters, 1,260 parts, 4 lights, 5 draws) 2.19–2.23 ms against 2.08–2.11 ms; the 8 × 50 crowd battle with 256 cues 2.27 ms. CPU on the render thread (interpolation, effect evaluation, world UI): 0.05–0.1 ms a frame, 0.5 ms with all 450 units of the crowd selected. Scaled to the base M1 (about 7× less GPU) effects-heavy views add 1–2 ms; within budget.

### Performance and presets (D42)

`just bench-render` measures fixed scenes at 1920×1080 and 2560×1600 (wall time from recording to idle, and every pass alone in its own submit minus the submit floor; the fastest fifth of 60 frames) and writes a table to `target/bench-render/latest`; `--preset`, `--scenes` and `--label` select and name runs. Its *Live CPU* column is the shot's live-scene cost (`LiveScene::show` and `draw_world_ui` on the shown pair, `CANOPY_SHOT_LIVE_LOOPS` times, 20 by default). `menu` reports paced frames (their GPU share includes the GPU's wake-up between frames, about 0.45 ms on the M4 Max) and the UI pass back to back minus the floor. `--scenes match` (not a default) plays a skirmish on the first battlefield through the real game headless and reports the main thread's CPU per frame and, built with `--features count-allocs`, its heap allocations per frame (D104; most of what remains is wgpu's own per-submit bookkeeping). Live models draw cooked levels of detail chosen by on-screen error (`Quality::lod_pixels`); the shadow taps rotate a fixed Vogel disk once per pixel and are skipped on surfaces facing away from the sun. Presets (`Preset`, Low/Medium/High/Ultra) set render scale budget, contact shading, bloom, haze, LOD error and shadow map size; the adapter picks one on first run (base Apple chips Medium).

M4 Max, frame ms (GPU ms):

| Scene | High 1080p | High 2560×1600 | Medium 1080p | Medium 2560×1600 |
|---|---|---|---|---|
| player base (echo-isles, zoom 1) | 2.08 (1.74) | 2.87 (2.56) | 1.43 (1.17) | 1.51 (1.26) |
| spectator (zoom 3) | 3.47 (3.07) | 4.81 (4.36) | 2.60 (2.26) | 2.67 (2.33) |
| 8 × 50 crowd battle, 256 cues | 3.03 (2.59) | 4.13 (3.64) | 2.00 (1.62) | 2.19 (1.80) |
| deep water | 1.80 (1.56) | 2.95 (2.69) | 1.43 (1.19) | 1.50 (1.27) |
| deep forest (bloom, haze) | 2.34 (2.04) | 3.38 (3.07) | 1.69 (1.40) | 1.87 (1.55) |

A base M1 has about a seventh of this GPU: Medium's worst view (spectator) lands near 16 ms, player views near 10 ms. The spectator view is vertex-bound (about 1 M forward and 1 M shadow triangles). Unmeasured on a base M1.

### GPU frame cost on the campaign forests (2026-10-25)

`just bench-render` also covers the campaign's Underleaf forests: `forest-road` (The Glass Road at 200,200 zoom 1), `forest-road-east` (its densest giant scenery, 330,150 zoom 1) and `forest-hollow` (The Hollow Brood at 200,200 zoom 1). They carry about 1,700 static instances, 100 forward draws, the forest's 88 trunks and 34k leaves, water, haze and bloom.

M4 Max, 1920×1080, frame ms from recording to idle (`just bench-render`, the fastest of three interleaved runs), before and after the same-picture work of D103:

| Scene | High before | High after | Medium before | Medium after |
|---|---|---|---|---|
| default | 2.68 | 2.26 | 1.89 | 1.76 |
| spectator | 3.69 | 3.26 | 2.91 | 2.68 |
| crowd | 4.06 | 3.65 | 3.03 | 2.60 |
| forest-road | 4.34 | 2.65 | 3.35 | 2.05 |
| forest-road-east | 5.78 | 2.99 | 4.36 | 2.51 |
| forest-hollow | 5.19 | 3.47 | 3.95 | 2.60–2.90 |

Where it went: on the forests the forward pass halves (opaque scenery shaded only where visible, point lights summed only where they reach) and the shadow pass loses 30–60 % of its triangles (casters that cannot shade the view). Every capture matches the build before at float-noise level (at most 0.004 % of pixels differ by more than 2/255). What is left on the forests is mostly geometry: the Tripo giants draw their full 9,000-triangle meshes everywhere (their cooked levels of detail serve live models only), about 1.2 M triangles in view and 2 M in the sun's map on The Hollow Brood. Targets on this machine: forest views ≤ 2.4 ms Medium (about 60 FPS on a base M1) and ≤ 3.0 ms High, the default view ≤ 1.8 ms, the crowd ≤ 3.0 ms.

The second same-picture pass (D103, after the CPU frame pass of D104):

M4 Max, 1920×1080, frame ms from recording to idle, the fastest of five interleaved runs of each build:

| Scene | High before | High after | Medium before | Medium after |
|---|---|---|---|---|
| default | 2.02 | 1.96 | 1.48 | 1.42 |
| spectator | 3.07 | 3.03 | 2.46 | 2.39 |
| crowd | 3.40 | 3.28 | 2.35 | 2.23 |
| forest-road | 2.51 | 2.37 | 1.90 | 1.75 |
| forest-road-east | 2.89 | 2.81 | 2.20 | 2.06 |
| forest-hollow | 3.37 | 3.18 | 2.63 | 2.44 |

What it changed: the canopy prepass now covers only the instances a unit stands behind (on The Hollow Brood about every giant took it before, since a unit is always on screen); models with nothing in a view record no draws (The Hollow Brood 236 → 177 draws, The Glass Road 222 → 159), which saves the GPU about 1.5 µs per empty indirect draw and the CPU its encoding; frame buffer writes go through a reused staging belt (`upload.rs`, `Uploads`) instead of a staging buffer each; skinning shares the culling's compute pass and the empty ripple and UI passes are no longer begun outside timed frames.

Captures against the build before: crowd identical, default 1–2 pixels off by 1/255, the forests and the spectator view at most 0.0024 % of pixels off by more than 2/255 (canopy instances that left the prepass, edge pixels). Still above the targets: The Hollow Brood (High 3.18, Medium 2.44), the crowd at High (3.28) and the default view's GPU share (about 1.65 ms); what would close them changes the picture (Jiri's call): levels of detail for scenery (the giants' cooked LODs), fewer PCF taps (8 → 4 saves about 0.2 ms on the crowd), or back-face culling of closed casters.

Where a frame goes now, from Metal System Trace (`xctrace record --template 'Metal System Trace' --all-processes` while a shot benchmarks; per-encoder GPU time): on The Hollow Brood the forward pass is about 0.4 ms of vertex and 1.2 ms of fragment work, the shadow pass 0.25 + 0.25 ms, the final pass 0.24, the haze march 0.18. Fragment work dominates everywhere, and within it the sun's 8-tap PCF: without the taps the crowd's forward pass is 0.55 ms shorter, the forests' and the default view's 0.4 ms; each tap costs about 0.05 ms on the crowd. Measured and ruled out (no gain or not the same picture): skipping the PCF on 8×8-texel tiles whose depth range proves the result (the receiver's own surface spans each tile's range, so almost no tile decides), reordering opaque and discarding draws, dropping the linear-depth target, fewer varyings, cheaper skinning, vertex-cache reordering (the Tripo meshes are already near optimal, ACMR 0.71), oriented-box culling (1–2 % fewer triangles), and culling back faces of closed meshes in the shadow pass (half the forests' shadow fragment time, but 0.1 % of pixels shift along self-shadowed edges). On the CPU, `CommandEncoder::finish` (wgpu's encoding into Metal) is most of a frame's recording: render-pass begins about a fifth, bind groups and indirect draws a sixth each, wgpu's indirect validation 6 %.

### Building foundations (D44)

Buildings stand at their centre's ground height; `terrain/foundations.rs` keeps them from floating. The game passes the presentation's footprints each frame (`canopy_scene::live::foundations` → `Renderer::set_foundations`; an equal list costs a comparison). The terrain owns the drawing: a pad (rounded corners) and a mound per footprint, vertex-pulled from the height texture (`vs_foundation`, 80 spokes × 8 bands + the pad's fan = 4,080 vertices), smoothstep-profiled into the ground with normals turning into the terrain's at the toe, and shaded by `ground_at` — the terrain's own layer composite at that place, nothing added — in one instanced forward draw and one shadow draw for the footprints in view (CPU-culled). Buried where the ground is level or higher. The world UI drapes its ground marks over mounds through `canopy_scene::live::FoundationGround` (the mound height on the CPU, `Foundation::surface`). `canopy shot` prints the count.

### Not yet (later M3 steps)

Terrain displacement (no shipped map uses rock coverage), egui tuning, hot reload of `config/`. Mist regions (`atmosphere.regions`, used by no biome), rain and the close-camera depth of field and shaft density of the old surroundings (no camera uses them) are not carried. Rain and beam effect layers (no shipped effect uses them) are not drawn. Fog of war on stacked floors: bridge decks fold into the ground cell under them (in sight when any floor is) rather than keeping the old per-floor atlas. The portrait has no rim light (one sun light in the lighting model).

## 10. Reference captures

`just shot --map <id> --camera <x,z,zoom[,yaw]> --out <png> [--size 1920x1080] [--hour 12] [--time 12] [--frames n] [--no-shadows] [--pitch <deg>] [--movers <n>] [--biome <id>] [--render-scale <0.5–1>] [--no-contact] [--no-bloom] [--haze off|low|medium|high]` renders one view offscreen (no window) and writes a PNG, printing draws, terrain triangles, water and forest counts, memory, CPU recording time, the median submit-to-idle time of a frame and GPU time per pass (from `--frames` frames). `--pitch` frees the camera from the map fit (views past the border), `--movers` walks synthetic bodies through the view for wakes, `--biome` previews the map in another biome's look. Effects, fog and world UI (M3.5): `--effect <id>[,<age>[,<radius>]]` (repeatable) plays effects standalone at the focus, 8 m apart, started `age` ticks before the shot, travelling layers from 6 m west (the old `EffectPlayer.play`); `--flight <id>,<age>,<metres>,<ticks>` (repeatable) flies a weapon's launch effect towards the focus over that distance and flight time, in rows 4 m apart, as a shooter's would be (trails, D77); `--sequence <frames>,<step>` then advances the simulation `step` ticks per frame and writes `<out stem>-001.png` onwards (the spell review clips, D78); `--viewer <slot>` shows that player's view and fog; `--select all|player:<slot>|<ids>` and `--health damaged|always|selected` feed the world UI; `--damage <percent>[,…]` sets every built building to those shares of its health in turn before the shot (fires, D46); `--kill <text>[,<ticks>]` kills every unit whose definition id contains the text that many ticks (40) before the shot (death clips and their effects); `--crowd <n> --races <a,b>` stages a crowd battle of those races within range of each other, each with its towers and a network source behind them (weapons and towers under review); `CANOPY_SHOT_HUD=1` also writes the minimap and portrait images (`<out>-minimap.png`, `-portrait.png`); `CANOPY_SHOT_EFFECTS=1` lists the cues by effect layer. The old game's effects are captured with the same placement by injecting the old `EffectPlayer` into the reference stage through Chrome's DevTools protocol (`docs/captures/README.md`).

The view is a match's start: computer players on every start (or the first `--players n`). `--tick N` first simulates the match to tick N on the same thread, presenting every tick, so the picture of a tick is always the same; `--camera battle[,zoom]` looks at the densest group of swinging units at that tick; `--crowd N` replaces the match by eight players with N animated soldiers each around the middle of the starts (the stress view); `--colors i,j,…` gives slot k the k-th listed player colour. The output adds the simulation's cost per tick, the live scene's counts and where corpses, felled trees, missiles and scaffolds are. `CANOPY_SHOT_MODELS=1` lists the live models in view.

`just play --spectate <map> [--players n] [--tick N]` watches the match in a window (B jumps to the largest fight); every two seconds the log has a `frame` line (fps, slowest frame, CPU and GPU time, live models in view) and a `simulation` line (tick, the tick shown, step, view and presentation cost per tick, frames that had to hold the newest tick, schedule slowdowns, counts).

### Camera parameters

They are the old game's play camera (`src/render/camera/camera.ts`):

| Parameter | Meaning | Old game |
|---|---|---|
| `x`, `z` | focus on the ground, in cells (world units; +x east, +z south) | `camera.lookAt(x, z)` |
| `zoom` | eye distance / 116.591 (zoom 1 shows 128 units across at 16:9) | `gameZoom`, `pose({gameZoom})` |
| `yaw` | degrees, 0 looks north (eye due south of the focus) | `yaw` (play keeps 0) |
| pitch | fixed 56° below the horizon | `GAME_PITCH` |
| field of view | 34.318° vertical at 16:9, widened for narrower windows | `GAME_FOV` |
| near / far | 1 / 2 · distance + 32 | `applyTo` |

The rig follows the ground at the focus and keeps 4 units above the ground under the eye and 12 above water, and the view's ground footprint is kept inside the map (`[0, size − 1]` on the plane `min(0, water level)`), shrinking the distance when needed — both exactly as the old camera, so the same pose frames the same ground. Players use zoom 0.515–1.25; a spectator may go to 6 (the map fit still applies).

### Reproducing a view in the old game

1. In the old repository: `npm run dev` (Vite on `http://127.0.0.1:5173`).
2. Open the reference stage with the same pose and hour, at a 16:9 window (1920×1080 CSS px, any device pixel ratio):

   `http://127.0.0.1:5173/reference-stage.html?map=<id>&x=<x>&z=<z>&zoom=<zoom>&yaw=<yaw>&pitch=56&hour=12`

   Pass `yaw` and `pitch=56` explicitly: the stage defaults to yaw −45°, pitch 45°. It accepts zoom 0.5–2 and then applies the game's 0.515–1.25 clamp. It draws the map's authored entities (units included) with no simulation, fog or UI. Wait for `document.body.dataset.ready === "true"`.
3. Click **Save scene PNG**: it renders 1920×1080 offscreen at animation time 12 s, like `just shot`'s defaults (`--size 1920x1080 --time 12`).

Live match alternative: `http://127.0.0.1:5173/?map=<id>` starts at the first player's start at zoom 1; `utcPerformance.match.lookAt(x, z, distance)` in the console moves the camera (distance = zoom × 116.591), `utcPerformance.match.onReveal(true)` lifts the fog, Ctrl+Shift+H hides the HUD. The match runs its day cycle from 9.5 h, so use the reference stage for lighting comparisons.

### Captures

`target/shots/` holds both shipped maps from three views, with the old game's captures of the same poses in `target/shots/old/` and side by side (old left) in `target/shots/compare/`:

| File | Map | `--camera` |
|---|---|---|
| `echo-isles-a.png` | echo-isles | `117.5,225.5,1` (player 1 start) |
| `echo-isles-b.png` | echo-isles | `320,320,1.25` (centre, farthest player zoom) |
| `echo-isles-c.png` | echo-isles | `501.5,225.5,1,45` (player 2 start, turned 45°) |
| `turtle-rock-a.png` | turtle-rock | `529.5,401.5,1` |
| `turtle-rock-b.png` | turtle-rock | `320,320,1.25` |
| `turtle-rock-c.png` | turtle-rock | `113.5,257.5,1,30` |

Mean colours of matching regions agree within 1–3 of 255 on open ground, forest and dirt (for example 64,131,55 on echo-isles grass in both; whole frames within 4). Visible differences: units (next step), the placeholder water (the old water is lighter and textured), the old contact shadows and softer VSM shadow edges, slightly brighter old deposit crystals.

Live captures (M3.3, D35) are in `target/shots/` (new), `target/shots/old/` and `target/shots/compare/`, and the review set in `docs/captures/m3-live/`:

| File | View |
|---|---|
| `live-echo-p1-t800.png` | echo-isles `--tick 886 --hour 9.5 --camera 117.5,225.5,1` (old tick 887) |
| `live-echo-p2-t2400.png` | echo-isles `--tick 2487 --hour 9.5 --camera 501.5,225.5,1` (old tick 2488) |
| `live-echo-battle-t16000.png` | echo-isles `--tick 16000 --camera battle,0.8` |
| `live-crowd-battle.png`, `compare/battle-lab-vs-crowd.png` | turtle-rock `--crowd 50 --tick 400 --camera 350,343,0.52`; old: combat lab, 24 vs 24 mixed army |
| `live/crowd-400-t120.png` | turtle-rock `--crowd 50 --tick 120 --camera 325.5,331.5,1.6` (the 8 × 50 stress view) |
| `live/crowd-white.png`, `live/crowd-purple.png` | the white and purple players close up |
| `live/fall-5740.png`, `live/tree-*.png` | felled trees (echo-isles, ticks 2400–5900) |

The old live match was captured headless: Chrome with `--remote-debugging-port`, `/?map=<id>`, then `utcPerformance.match.onReveal(true)`, `onSpeed(4)`, Ctrl+Shift+H, polling `utcPerformance.match.snapshot()` (`world.tick`, `world.game.state.entities`) until the tick and `lookAt(x, z, zoom · 116.591)` before `Page.captureScreenshot`. The old game's ticks are ours + 1 (D21).
