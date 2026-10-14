# Canopy light and atmosphere

Biomes define continuous overhead canopy shade and localized sunlight shafts. Forest air stays clear at gameplay height; stronger rays must not require a full-screen fog veil. Rain reduces the shaft contribution. The shared day/night sun supplies direction and color.

## Art direction

Set `environment.atmosphere` in `src/content/biomes.ts`. The game, map editor and asset workbench resolve that same definition. The map Environment panel selects a biome and simulation conditions; it offers no mist or lighting overrides. A variant such as Deep Forest can reuse Vibrant Forest assets while changing canopy coverage, lighting and mist.

`src/shared/landscape/atmosphere.ts` defines the settings schema and defaults: color, density, base height, height falloff, shaft strength, independent shaft density, patchiness, texture scale and drift speed. `density` controls haze extinction; `shaftDensity` adds scattered sunlight through openings without reducing scene transmission. This separation is an art-direction approximation, not a physically complete atmosphere. The renderer also supports up to 16 soft ellipsoid regions in the biome profile. Region positions are absolute world coordinates, so reusable profiles normally leave `regions` empty. Zero global density supports local-only mist, and zero drift freezes the density field.

Persisted maps contain only hour, clock playback and optional weather kind in `landscape.environment`. Legacy map appearance fields are stripped on load and export. MCP rejects appearance overrides; adjust the biome definition instead. Graphics quality remains a separate player preference.

## Rendering and performance

`AtmospherePass` renders the scene into a linear HDR color target with a depth texture. A reduced-resolution pass reconstructs world positions and integrates single-scattered sunlight and ambient mist along the view ray. Ray integration skips clear air above the highest mist region or five global falloff lengths, concentrating the existing sample budget around canopy occlusion. A broad scattering lobe keeps sunlight readable from the overhead RTS camera; ambient scatter stays subdued. Exponential height falloff and soft ellipsoids supply density; a drifting procedural noise field breaks up uniform areas.

A deterministic 256², periodically filtered world-space transmission field represents enormous overhead crowns. Its height, scale, coverage, penumbra and shade strength belong to the biome. It contains no individual leaf shapes or invisible alpha-cutout shadow mesh. Surface composition and the raymarch project the same field toward the sun, aligning large soft ground shadows with clear-air shafts. A second channel contains sparse feathered apertures strictly inside the broad openings, so an open clearing does not become a blanket of luminous air. Surface shade is a composite approximation that retains a bounded amount of sky fill; it does not separately relight each material’s direct and indirect terms. Bloom is attenuated by the same field so it cannot flood shade with unattenuated highlights.

The current sun shadow map supplies additional occlusion from visible trees and structures. Soft shadows use Three r185's RG variance-shadow distribution; filtered shadows use its comparison depth texture. No extra light shadow pass is rendered. Turning shadows off retains ambient mist and disables the shafts.

A nine-tap depth-aware filter first smooths the bounded low-resolution fog target. The final pass uses depth-aware four-neighbor upsampling, applies contact shading, HDR bloom, extinction/scattering and the day/night LUT, then biome grading and output color conversion. Outdoor scenes keep their existing day/night color pipeline; environments without a daytime sample use ACES. The portrait renders afterward, and HTML interface elements are unaffected. The shader handles orthographic and perspective cameras. Integration terminates at opaque depth, or at the water plane when the opaque surface is underwater. This is tailored to the game's single water level; arbitrary transparent volumes do not receive individually depth-correct fog.

Local graphics settings offer **Off / Low / Medium / High**, default Medium. They persist independently of map authoring:

- Low: 16 steps, up to one-third resolution per axis, capped at 180,000 fog pixels.
- Medium: 24 steps, up to half resolution per axis, capped at 360,000 fog pixels.
- High: 40 steps, up to 66% resolution per axis, capped at 900,000 fog pixels.

The full scene target still follows the player's game resolution. Off skips the volumetric raymarch and fog filter. Day/night grading, water composition and the biome finish still run; the setting does not override biome art direction. Resources are reused and resized only when needed, and disposed with the renderer. The normal match loader's graphics warmup includes the atmosphere before play.

There is no temporal accumulation, so rapid camera movement cannot leave history trails. Spatial jitter and depth-aware upsampling reduce undersampling; fine foliage may still show grain or shimmer at Low. This version supports one directional light, not volumetric contributions from every lantern or spell, multiple scattering, or volumetric clouds.

The debug panel shows the active fog dimensions/sample count, CPU submission, whole-frame GPU timing, and sampled scene/atmosphere/biome-finish GPU scopes. Only one query scope is sampled per frame; queries never nest or block. Do not add subpass timings together: tile-based drivers can attribute shared work to more than one scope. Compare whole-frame timings at a fixed view, resolution, weather and shadow setting with atmosphere enabled/disabled. Browser background throttling means displayed FPS alone is not a reliable GPU benchmark.

## Gameplay visibility

Atmosphere is cosmetic. It does not change line of sight, height-based fog of war, combat, navigation, or lockstep. Integration samples the existing visibility texture and the full-resolution composite masks unexplored terrain again, preventing a bright veil from revealing the map outside explored areas. Already-hidden units remain excluded by the observation system. Debug reveal removes this visual restriction through the existing visibility controls.

## Validation

Automated coverage checks biome resolution, legacy map cleanup, map condition roundtripping, malformed and unbounded volume rejection, unique IDs, quality persistence, pixel budgets and the Off bypass. Visual checks should cover soft/filtered shadows, the editor authoring path and shader compilation on a maintained map. Treat performance as hardware- and scene-dependent; the pixel caps bound fog work, not the rest of the forest renderer.

## Biome finish

`environment.postProcessing` in `src/content/biomes.ts` owns exposure, highlight rolloff, contrast, saturation, shadow/highlight tints, split-tone strength, bounded colored shadow lift, vignette, bloom and contact shading. Forest, winter, autumn and deep forest have separate profiles. All use the same `Renderer` in the game, editor, reference stage and asset workbench. These values never enter map files or the map editor's controls; old appearance snapshots are stripped by the conditions parser.

`BeautyPass` reuses the HDR scene and depth that the atmosphere already captures. It adds at most four bounded fullscreen draws:

- Twelve depth samples reconstruct nearby contact occlusion. The radius is in world units, projected correctly for both orthographic and perspective cameras. Water and background pixels are excluded. Four-tap depth-aware upsampling avoids dark outlines bleeding over silhouettes.
- A soft-knee HDR threshold feeds three progressively smaller, tent-filtered bloom levels. It spreads bright highlights without blurring the original scene. The snow profile has a higher threshold to avoid turning the entire snowfield into a glow.

The largest contact and bloom buffers each have at most 180,000 pixels and at most half scene resolution per axis. All bloom levels together are at most 236,250 pixels (normal landscape aspect ratios). Targets are reused, resized as necessary, and disposed with the renderer. Zero contact/bloom strength skips the corresponding draws. The final grading adds no extra pass. The debug GPU scope is `GPU biome finish`; measure whole-frame cost on target hardware rather than inferring FPS from these caps.

A hue-preserving highlight shoulder compresses HDR peaks before the day/night LUT clamps its input. Color grading operates in display sRGB after that LUT and explicitly converts back before destination encoding, so screenshots and the canvas agree. Split tone and contrast preserve black. Positive exposure favors midtones, and saturation boosts favor muted colors so saturated team colors do not clip. A second hue-preserving shoulder catches peaks created by grading. Bloom and grading are gated by the existing full-resolution gameplay visibility. Portraits and HTML HUD render afterward and remain untouched. There is no depth-of-field, motion blur or temporal accumulation: unit silhouettes and fast camera motion remain sharp. This contact effect is screen-space approximation, not baked or ray-traced ambient occlusion; it cannot account for occluders outside the view.

## Visible biome surroundings

`Biome.surroundings` declares the trunk, mushroom and fallen-log assets, perimeter bands, leaf palette, backdrop depth colors and close-view shaft density. `ForestSurroundingsLayer` builds these automatically from map dimensions, boundary heights and the biome canopy seed. The map does not store these objects. Forest, frost and autumn use different palettes; Deep Forest inherits the forest recipe.

The exterior uses shared GLB geometry in quadrant batches, curved procedural upper limbs, broad three-dimensional leaves, and a ground skirt beyond the terrain halo. Crown leaves have a fixed grid budget; trunks grow with the map perimeter up to a cap. Geometry has no collision, selection, simulation, minimap or gameplay visibility. Only these decorative materials opt out of the fog-of-war material patch. Pending asset loads and generated GPU resources are disposed when the renderer is destroyed.

Overhead leaf groups sample the same world-space opening field as the canopy lighting. Outside the map, branch-attached crowns complete the distant silhouettes. This is an approximate crown envelope, not per-leaf shadow-map occlusion. Pitch-based cutaway hides overhead crowns and upper exterior trunks from the RTS view. Close views expose them, extend the far plane and enable the biome's clear-air shafts; distance haze is restricted to the generated scenery materials. The playable terrain does not receive extra mist.

For repeatable ground-level inspection, `editor_screenshot` accepts `inspection: {eye: [x,y,z], target: [x,y,z], fov: 65}`. This renders through the game renderer without moving units, altering the map or retaining a different editor camera. Omit `inspection` for normal RTS screenshots. Oakfall Hollow is the main visual test scene. The current reused trunk assets and generated limb junctions remain candidates for an art-quality pass; no new imported model assets are required by this implementation.

Close-camera forest depth of field is owned by `Biome.surroundings.depthOfField`: `start` and `end` are forward distances in world units; `radius` is the maximum disk blur in pixels at 1080p. The lens fades with the canopy camera transition, stays off for orthographic/RTS and interior views, and respects the Atmosphere Off graphics preference. The existing composite gathers 24 depth-aware samples only beyond the focused range; no extra scene draw or framebuffer is allocated. Foreground silhouettes and fog-of-war boundaries reject blur samples across them. HUD rendering is unaffected.
