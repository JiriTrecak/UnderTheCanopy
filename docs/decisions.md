# Decisions

Decisions that shape the code. Newest last. Each entry says what was decided and why; reverse one by adding a new entry that supersedes it.

## D1. Rust and wgpu, shaders written once (2026-10-14)

The game is Rust on wgpu with WGSL shaders, driving Metal on macOS, DX12 on Windows and Vulkan as fallback. No per-backend shaders or code paths. No engine framework (no Bevy): our own architecture on a few libraries (wgpu, winit, an audio library, taffy, cosmic-text). Why: full control, predictable performance, one renderer everywhere.

## D2. WebGPU feature baseline (2026-10-14)

Devices are created with WebGPU's default limits and only features WebGPU also offers (plus BC texture compression when the cooker needs it). Native-only features are allowed only as optional fast paths with a fallback, and each one is agreed with Jiri first. Why: the editors will embed the same renderer through WASM and WebGPU in phase 2.

## D3. Simulation at 40 Hz on its own thread (2026-10-14)

The simulation ticks every 25 ms on its own thread; the renderer runs at the display rate and interpolates between snapshots. Why: the frozen server and lockstep input delay are built on 25 ms ticks, finer ticks keep commands responsive, gameplay is tuned at this rate, and the cost in Rust is small. The tick length is one constant. Content durations (`*Ticks` fields) are authored in fixed 25 ms units (`AUTHORED_TICK_MS`) and converted to simulation ticks on load, so a different tick rate would not change the data. Parallel work inside a tick merges results in a fixed order.

## D4. Determinism from day one (2026-10-14)

Simulation math uses deterministic functions (`libm` or fixed point), never platform math libraries; iteration orders are fixed; randomness comes only from the simulation's seeded generator. CI runs replay-hash tests on macOS and Windows, and hashes must match. Why: lockstep multiplayer and replays depend on it, and adding it later means rewriting the simulation.

## D5. Lua 5.4 through mlua for scenario scripting (2026-10-14)

Map and campaign scripts run on vendored Lua 5.4 via `mlua` with no standard libraries opened, exactly like the old sandbox: scripts reach only the `mission` API, so platform maths and Lua's own randomness are unreachable, and the unfixed string-hash seed only affects table iteration, which scripts cannot observe. Why: today's scripts target Lua 5.3 (fengari) and run on 5.4 nearly unchanged; Luau would break integer and bitwise semantics. Differences scripts can notice are listed in [simulation](simulation.md#lua).

## D6. Platform layer owns every path (2026-10-14)

No code depends on the working directory. Game data is found at `Contents/Resources/data` in the macOS bundle, `data/` beside the Windows executable, or `--data <folder>` in development; user files live in the OS user-data folder (`com.jt.canopy`). Why: packaging for any platform then needs no code changes.

## D7. Packaging and CI from M0 (2026-10-14)

CI builds, lints, tests and packages on macOS (Apple Silicon) and Windows on every push: an unsigned `.app` and an NSIS installer through `cargo-packager`. Checkouts skip Git LFS. macOS minimum version 13. Why: portability problems surface immediately; signing and installers are added later without code changes; LFS bandwidth is billed.

## D8. Apple Silicon only (2026-10-14)

macOS builds target arm64; no Intel support. The reference (weakest) machine is a base M1 with a 7-core GPU. Why: Intel integrated GPUs would set a far lower budget for little benefit.

## D9. Data mirrored, formats unchanged during phase 1 (2026-10-14)

`content/`, `assets/` and `art/` are mirrored from the old repository with `just sync-data` and keep their formats exactly. `canopy-content` is a strict reader of those formats. Why: the editors stay in the old repository until phase 2, so its output must work here unchanged. Identical binaries in `art/` and `assets/library/` cost nothing extra in LFS (content-addressed).

## D10. Match server and protocol frozen (2026-10-14)

The TypeScript match server stays in the old repository and stays deployed. `canopy-net` implements the client side of its protocol exactly. The server embeds `content/game.json` (races, starting heroes) and action-shape validation, so new races or heroes need the server redeployed with the new content, and new action types need a server change. Why: it works, and porting it adds nothing to the game.

## D11. Profiling built in (2026-10-14)

`tracing` spans everywhere; the `profile` feature streams them and frame marks to Tracy (`just profile`). The `profiling` Cargo profile is release with full symbols. Why: performance claims come from measurements on the reference machine.

## D12. Content validation scope in phase 1 (2026-10-14)

`canopy-content` rejects anything structurally wrong (unknown fields, wrong types, unknown enum values) and enforces every cross-reference and invariant the simulation relies on, matching the old loader's rules. Pure authoring limits (for example a maximum speed of 40) are not duplicated while the old editors still validate the data; they move to Rust with the editors in phase 2. `just parity` compares the Rust reader with the TypeScript loader on the same files. Why: full constraint parity is editor work; the game needs correct reading and sound invariants.

## D13. Sampled multiplayer hash (2026-10-14)

Desync detection keeps the old design: every 8 ticks a cheap fingerprint of the id counters, jobs, stocks, outcome and a rotating sample of at most 32 actors, in the existing `hash` message. A full-state hash is not computed. Why: it is enough to catch desyncs, and hashing everything every few ticks costs too much.

## D14. Fix bugs found in the old simulation (2026-10-14)

"Behaves the same" covers intended gameplay, not defects. Clear bugs found while rebuilding are fixed and noted in the commit and, if players would notice, in the release notes. Changes to how the game plays still go to Jiri first.

## D15. Movement and pathfinding keep the old rules for now (2026-10-14)

Movement, pathfinding, collision and traffic are rebuilt with the same rules and behaviour as the old game, implemented efficiently. A proper redesign (flow fields and more) comes later as its own piece of work.

## D16. New save format (2026-10-14)

No compatibility with the old game's saves; nobody depends on them. The Rust save format is designed fresh.

## D17. Verification by play (2026-10-14)

No automated behaviour comparison against the old game. Correctness comes from scenario and determinism tests, and from Jiri playing the game once it is close.

## D18. One unsafe exception: the Lua instruction hook (2026-10-14)

`crates/sim/src/mission/hook.rs` uses about 15 lines of `unsafe` to install a raw Lua count hook that counts instructions and inspects the running function's registers for strings over 64 KiB, as the old sandbox did. mlua's safe hook cannot see registers and is several times slower per instruction. The rest of the workspace keeps `unsafe_code = "deny"`; any further exception needs a decision here.

## D19. Vision: knowledge per team, followed by comparison (2026-10-14)

Fog and remembered entities are kept once per team (allies share sight) and saved compactly (fog as runs, memories as snapshots of the entity as last seen). Sensors read the one exact `entity.position`; vision compares each sensor's position, height, radius and team with the previous pass every tick and updates only what changed, so any system that moves an entity is followed without notifying vision. Derived coverage is rebuilt on load and verified against the incremental data in tests.

## D20. Every player has a team (2026-10-14)

Teams are the only source of "ally": `canopy_sim::teams::Teams` resolves the match's slots so that every player has a team; a slot without a chosen team gets its own (the smallest number no other slot uses). Sight sharing, spells and auras that affect allies, AI and views all ask `Teams::relation` (own, ally, enemy, neutral) and never compare owners or special-case solo players. The UI need not show teams in free-for-all. Fixes the old quirk where team numbers and player indices shared one numbering.

## D21. Movement ported rule for rule, checked against the old game (2026-10-14)

Spatial model, navigation (grid A*, sectors, navmesh with the three.js 0.185 earcut ported exactly, walk-surface paths), traffic and orders follow the old code's algorithms, costs and tie-breaks (D15). During development, trajectories (positions and rotations every tick) were compared with the old game on scenarios covering crowds at chokepoints, deadlock yields, 400 units crossing a shipped map, a bridge, air units, patrol, follow and queued orders, and matched exactly; such comparisons are not kept as tests (D17). Derived data is new: compact body records in 4×4-cell buckets, per-cell blocker counts instead of owner stacks, edge and step caches, incremental mesh tiles; each is a pure function of state, so a restored match rebuilds it identically. The old envelope proof of clear sweeps is sound, so it now runs before every ground sweep, not only long ones. Units cooperate in traffic (waits-for negotiation, yielding, local claims, idle allies stepping aside) by team (D20), where the old code compared owners; route sharing and opposing-traffic search weighting stay per owner (one player's orders), and follow targets stay owned units. Game tick numbers start one lower than the old game's, which only shifts the phase of tick-hashed timing (idle strolls, separation stagger).

## D22. Combat ported rule for rule; engine ids in content are checked contracts (2026-10-14)

Targeting, swings, missiles and shells, damage, deaths, experience, camps and lookouts follow the old code's rules, numbers and tie-breaks: acquisition staggered by `tick % 8 == id % 8`, nearest by (distance, id), wind-up and recovery scaled by the attack cycle, damage summed per target and applied in id order, armour `1/(1 + k·armour)` rounded half up. During development, 25 scenarios (melee, missile and shell duels, armour, splash and slows, charges, flyers, focus fire, a 120-unit battle, assaults on buildings, camps with leash and loot, lookouts, attack, force, append, hold and attack-move orders, experience, levels, caps and fallen heroes) were run in both games and matched every tick: positions, health, swings, cooldowns, charges, slows, projectiles, corpses, loot and the random generator. As with D21 these comparisons are not kept as tests (D17).

- Damage is held in integer thousandths of a point (missile and shell damage are saved as `damageMilli`); mitigation is computed in `f64` and rounded half up as the old `Math.round`.
- Resolution asks exact sight after movement, so the world brings vision sensors up to date before step 7, and again after deaths so the dead no longer see, as the old live sensor index did.
- A dead hero stays in the entity table as **fallen** (no health, a fresh unit, on the cell it fell on) rather than being removed and re-inserted; revival (M2.5) brings the same entity back.
- **Engine-level content ids are named, documented constants checked when a world starts**, never literals in systems. The only one so far is `combat::rules::SPELL_DAMAGE = "spell"`: the damage type of untyped spell damage (mission `damage`, and later ability and item damage), which `rules.damageTypes` and `rules.damageMultipliers` must declare; content without it cannot start a match. The old `"normal"` fallback for units without a weapon was dead code (such a unit can target nothing) and is gone. New contracts go next to `SPELL_DAMAGE` and are checked in `Rules::new`.


## D23. Economy ported rule for rule; derived indexes; explicit starting workers (2026-10-14)

Jobs (deliver, construct, repair, harvest, plant with walk, wait, work, fall and return phases), the first-come harvest queue capped by `activeWorkers`, felling, depletion and regrowth, construction and its placement rules, training, rally points, pause, research, upgrades, revival, supply, wallets, accounting and defeat follow the old code's rules, numbers, messages and tie-breaks. During development, scripted matches on both shipped maps (starting gathering, mining, felling, a house and a forge built, training with a rally onto a tree, pause, research and its felling bonus, cancelled entries and sites with refunds, an upgrade refused, carriers and buildings removed with their goods and a building rebuilt, repairs, moves while carrying, defeat of three colonies and the outcome) were run in both games for up to 9,000 ticks and matched every tick: worker positions and cargo, jobs with phases and progress, wallets, accounting, building health, construction, production and research progress, the latest facts and every tree's felling state. Idle strolls and separation are tick-hashed, so these comparisons ran the old game with its tick shifted by one (D21). As with D21 and D22 they are not kept as tests (D17).

- **Derived indexes replace the old scans**: unit and building membership (followed lazily: ids are never reused, so creation shows as an advanced id counter and removal as a smaller live count), resources in 16-cell buckets, regrowing resources, and per-job aggregates (goods inbound per store, sites held by construct, repair and plant jobs) kept with the jobs and rebuilt when a save loads. `World::verify_economy` compares each with a cold rebuild. Static occupancy follows a depleted, regrown, placed, upgraded or removed blocker at once through `Spatial::sync_entity`; nothing rebuilds the whole forest.
- **Starting workers are explicit**: `spawn::placements` lists the starting units' placements, and the world gives their entities to start gathering. Nothing tests a placement-id prefix.
- **Map buildings are complete**, as in the old game, which never read `initialState.construction`; only the build command makes construction sites.
- **Supply messages name the player's own supply building**: the cheapest constructable building with `supplyProvided` in the player's race roster. The old messages said "Build a Mound" to every race.
- Every order change goes through `economy::interrupt` (via `orders::issue`), so a carrier finishes its delivery before a move, and workplaces lose their staff, as in the old game. Combat deaths release jobs, employment and paid tasks through `economy::work::release_references`; a fallen hero is revived in place.
- A decided skirmish stands still: the tick advances, nothing else runs, and commands are refused with "Match has ended".

## D24. Renderer decisions for M3 (2026-10-14)

- Every recommendation in [renderer §7](renderer.md#7-open-decisions) is accepted: one tonemapper plus a per-biome grade, MSAA 4×, one view-fitted PCF shadow map, no volumetrics or depth of field for now, the listed legacy shading paths cut, 4 blended terrain layers plus cliffs, normal-mapped rocks, authored effects converted by the cooker (their files unchanged), baked animation, live 3D portrait, presentation events from the simulation.
- **Maps up to 1024×1024**; larger maps are refused. This sets the terrain level-of-detail scheme and memory budget.
- **Interpolation delay accepted**: the picture trails the simulation by one 25 ms tick for smooth motion at any refresh rate.
- **No seasons, ever.** Biomes are the only environment configuration; a "winter forest" is simply another biome, not a season of one.
- **Lighting**: M3 matches the old game's current look inside the new, structurally clean pipeline; a separate graphics pass retunes it later.
- **Review**: each visual step comes with side-by-side captures (old game vs new renderer, same map, camera and units) for Jiri to judge.

## D25. Abilities, statuses and items ported rule for rule (2026-10-14)

Casting (targeting, eligibility, mana escrow, cooldowns, ranks, learning, autocast, cast orders and approach, prepare, channel and recover), every delivery (projectile, chain, line, charge, swarm), all sixteen operations with branches, queries, records and scales, persistent zones, toggles and auras, triggers and lifecycle reactions, summons and corpses, containment, split forms, forms, taunt, concealment and detection, spell vision, statuses (stacking, controls, damage policies, periodic effects, shields and mana shields) and items (inventory, pickup, drop and use, equipment, item auras, actives, rescue, on-hit procs, lifesteal and reflection, consumables and power-ups, regeneration accumulators) follow the old code's rules, numbers, messages and tie-breaks. During development, 49 scripted scenarios covering all of these (including a 32-unit brawl with heroes and casters autocasting, neutral camp casters, learning by level, weapon casts with autocast, death-raised summons and hero revival by ability) were run in both games and matched every tick: positions, health, mana and both regeneration accumulators, casts with phases, escrow and channels, cooldowns, ranks, autocast flags, weapon orders, statuses with timers, stacks and shields, item slots, runtimes, item statuses and bonuses, summons, containment, splits, deliveries, persistent instances, spell visions, corpses, missiles, every ability event and error, reaction queues and the random generator. As with D21–D23 these comparisons are not kept as tests (D17).

- **The ability and item libraries compile into the catalog** (`catalog.spells`, `catalog.items`) when content loads: abilities are `AbilityId` indices, statuses `StatusKey { ability, status }`, modifiers and control bits precomputed, forms resolved to definition indices. Nothing looks content up by string while a match runs.
- **Ability and item commands see the last completed tick**, like the old game, where commands ran between ticks: casts accepted with the command release on the same tick as there.
- **Ability events are presentation, not state**: about forty kinds (cast phases, impacts, statuses, summons, splits, containment, revival, combat modifiers, weapon orders), with viewers fixed when emitted; they are kept for 400 ticks (at most 1,024), drained by the client and never saved. Their presence never changes the simulation.
- **Delivery positions are integers in 1/128,000 cells**, exact for both milli-cell positions and the old game's 1/1024-cell snapping.
- **Budgets**: at most 128 lifecycle reactions (death, kill, interval, weapon release) run per tick from a queue of 512; combat reactions (hits, damage) are bounded by 2,048 queued events per tick; at most 512 deliveries and 512 persistent instances, 32 statuses per unit.
- **Spell damage that names no type uses `rules::SPELL_DAMAGE`** (D22); abilities and items add no other engine ids.
- **Old-game bugs fixed** (D14): a passive aura may carry triggers with any operations (the old content rule also scanned trigger operations, so an aura could not have a damage trigger); a line delivery needs no aim radius it never used (without one, the AI predicts a disc of the line's width at the aim).
- Revival at a building and by ability share one return: health, mana, casting state and both regeneration accumulators restart; a hero returning through an ability cannot be queued at a building, and supply counts it as returning.
- **Kept on purpose**: split members must be controllable, zero-supply non-hero units (the parent keeps its own supply and members are commanded by the player); only the first release operation is checked against the world when a cast is accepted, as later ones see the world the earlier ones changed; autocast and camp casting score only the release program, as before (the player AI that reuses it is M2.8).
- **Autocast considers only units that can matter**: those within the spell's range plus its widest area, delivery reach and the 24-cell threat radius, and the caster's own units anywhere. It skips the exact sight test for the rest, with identical choices.
- Taunted units do no work, haul nothing and enter no lookout; stunned workers neither start nor advance jobs and leave the harvest queue's waiting list, as in the old game.
- Removing an entity takes its unit body out of the spatial index at once (`Spatial::sync_entity`), as the old game did; units held by containment or a split form follow their host's whole cell.

## D26. Runtime asset pack and cooker (2026-10-14)

`canopy-cook` reads the mirrored data exactly as it is (D9) and writes one pack, `target/cooked/canopy.pack` (`canopy.pack` in the data folder of a package; `canopy_assets::find_pack` knows both). The game decodes no glTF, PNG or WAV.

- **Layout** (`canopy-assets`): a 64-byte header (magic, format version, digest of the cooked inputs), blobs, then a postcard index sorted by name. Little-endian, every blob and array 16-byte aligned. A blob is a 16-byte header, postcard metadata and plain arrays that metadata points into, so views are cast in place with `bytemuck` (no `unsafe`). `Pack::load` reads the file with one read; `Pack::open` reads the index and fetches entries on demand. Identical blobs are stored once. Entry names are the source paths (`assets/library/<id>/geometry.glb`, the path content already binds), with `#suffix` for parts (`#material0.base`, `#day`) and `@scenery` for the prototype table.
- **Models**: the authored transform (scale, pivot, forward), node hierarchy and soil offset (`groundContact: terrain` → −min y, as the old game) are folded into the vertices. Vertex: 16 bytes (unorm16 position in the model's box with the wind bend weight in w, octahedral snorm16 normal, unorm16 UV in the model's UV box); optional streams for vertex colour, leaf pivots (`_LEAF`) and skin (u8 joints, unorm8 weights summing to 255). One section per material with 16-bit section-local indices when they fit. A welded depth-only stream (positions and skin) holds every opaque shadow caster; alpha-tested sections are listed for the depth pass instead. Animated models get a joint palette in which rigidly animated nodes are joints too (felled trees, effect models), clips baked at 30 Hz as model-space 3×4 skinning matrices (Q13; frames `max(2, ⌈30·d⌉+1)` as the old bake), socket transforms per frame, the state table from `capabilities.animations` with event times normalised, and culling bounds over every frame and the wind sway. Materials are a model enum (unit, building, foliage, prop, effect) with numbers and flags.
- **Conventions are import-time inputs only**: team colour slots come from `capabilities.teamColor` (falling back to the `TC_TeamColor` name with a warning); `teamColorMask: "baseColorAlpha"` splits the base colour into opaque BC7 and a lossless R8 mask; the material extras the old renderer reads become fields, and those of cut paths (`referenceEnvironment`, `sourceShader`, `resinShimmer`…; Q5, Q6) are dropped; `foliageWind` node extras give the sway; nodes named `socket*` are sockets. The old game ignored `capabilities.wind` and the published `team_mask.png`; the cooker follows what it actually rendered and carries `capabilities.wind` alongside for the renderer.
- **Textures**: BC7, sRGB for colour roles (albedo, emissive, icons, interface, effect images, albedo tiles) and unorm otherwise, all mips, filtered in linear light: premultiplied for translucency, coverage-preserving for alpha-tested foliage, renormalised for normal maps, alpha kept independent where it is data (terrain tiles). Sizes round to whole 4×4 blocks (Catmull-Rom; the source size is kept for layout). Team masks stay uncompressed R8 (lossless). Colour grades become RGBA8 volume textures (BC volumes are not in the WebGPU baseline). No uncompressed copies are shipped: a device without `texture-compression-bc` (D2; Apple Silicon and DX12 have it) decodes BC7 at load (`Texture::decode_level`).
- **Sounds** stay 16-bit PCM: the two effect sounds are 82 KB; decoding Vorbis or Opus at load would cost more than it saves. Revisit when music arrives (M4).
- **What is cooked** is decided from the data's declarations: every glb; images of records without geometry; of model records only the images a render binding shows (the rest are copies of textures the glb embeds); not the channel images of terrain materials whose catalogue entry declares packed tiles (editor previews), but the gzip tiles themselves; other `.bin` as data; colour-grade JSON; map previews. Recipes and profiles stay in the catalogue JSON the game reads.
- **BC7 encoder: Intel's ISPC Texture Compressor through `intel_tex_2`** (prebuilt kernels for macOS arm64 and Windows x64), "fast" settings for colour and data, "basic" for normal maps. Measured on an M4 Max, 16 threads: "fast" is within 0.1–0.5 dB of "basic" on colour at about half the time; normal maps lose up to 1.5 dB with it. The pure-Rust port of the same kernels (`block_compression`, CPU path) gives the same PSNR (within 0.02 dB) and is bit-identical across platforms, but is 2–3× slower (a cold cook of 162 s against 31 s). Rejected: GPU compute encoders (results differ by GPU), `texpresso` (no BC7), bc7enc-rdo and Compressonator (C++ builds for little gain). Kept from `block_compression`: the BC7 decoder for the fallback and tests. The prebuilt kernels lack a macOS platform load command, so the cook binaries allow `linker_messages`.
- **Incremental, parallel, deterministic**: source files are hashed with BLAKE3 behind a size/mtime stamp cache; a job's hash covers the cooker and format versions, its parameters and its sources; textures are cached by image bytes and parameters, so models sharing an image encode it once; unreferenced cache files are removed. Jobs and BC7 strips run on every core; output order is fixed, maths uses `libm`, so the same inputs give a byte-identical pack. On Apple Silicon the ISPC kernel is a single NEON build; on x86 it dispatches by CPU instruction set, so Windows cooks may differ between machines (cache keys are per machine anyway).
- **Numbers** (M4 Max, shipped data, 441 MiB of cooked sources): cold cook 31 s, no-op 0.03 s, one model or icon changed 0.3–0.8 s (mostly relinking); pack 681 MiB (textures 625, models 56); loading the whole pack 89 ms (one read, warm file cache); validating every entry's metadata and views 0.4 ms.
- **Packaging** ships the pack and the JSON the simulation reads (`content/`, `assets/manifest.json`, the authoring indexes, maps) instead of `assets/library`. CI has no LFS art, so the package step cooks with `--allow-lfs-pointers` and ships a pack without it.
- **Not yet**: `Content::load` still checks that every published output exists on disk, so a packaged game cannot load content until that check consults the pack (game integration step). Pack supercompression (zstd over BC7), half-precision or sparser clip storage and the v2 effect conversion (Q11) wait for measurements and the effects step.

## D27. Biome look and presentation settings are data in `config/` (2026-10-15)

The old game defines biome lighting, atmosphere, grading, ground layers and surroundings in code (`src/content/biomes.ts`, `src/shared/environment/*`, `sky.ts`). They now live in this repository's own top-level `config/` folder, owned here and outside `content/`, `assets/` and `art/` (which `just sync-data` mirrors and would overwrite): `config/lighting.json` (day and night palettes per daylight profile, dawn and dusk phases, the ground probe and environment irradiance, shadow fitting), `config/biomes/<biome id>.json` (one per id a map may name), `config/players.json` (player colours) and `config/glow.json` (deposit glow). `canopy-render` reads and validates them strictly (unknown fields, the old editor's ranges, references to daylight profiles); `canopy-scene` resolves asset references against content; packages ship `config/` in their data folder. Values reproduce today's look: resolved numbers were checked against the old renderer (sun 3.2 from (−0.406, 0.707, −0.579) at noon in the vibrant forest, ambient 0.82, hemisphere 0.65/0.84/1.14 over 0.18) and the frames agree within a few levels of 255 (renderer §10). Derived constants are stored as data with their source: the ground probe is the old reflection cube's last mip, the environment irradiance the SH9 projection of the old procedural forest environment as it was actually sampled (upside down). Settings for effects not rendered yet (bloom, contact shadows, volumetric atmosphere) are carried so the biome files are complete. Why: biomes are content in all but location; Jiri tunes looks without code, and a later live-tuning UI writes the same files.

## D28. Renderer core for M3 (2026-10-15)

- **Crates**: `canopy-render` (device, frame graph, passes, shaders, camera maths, config types) and `canopy-scene` (the only bridge from maps and the simulation to the renderer's plain `World`). The game owns input and the window; `canopy shot` renders offscreen.
- **WebGPU baseline (D2)**: default limits; only WebGPU features, optional with fallbacks: `texture-compression-bc` (D26) and `timestamp-query` (pass timings). `indirect-first-instance` is not used (visible lists are reached through dynamic storage offsets), nor multi-draw-indirect, push constants or timestamps inside passes. MSAA attachments use the WebGPU `TRANSIENT_ATTACHMENT` usage. No exceptions are needed.
- **Today's look first (D24)**: one lighting function with a *ground* ambient model (the old terrain path) and a *standard* one (three.js physical as the old units, buildings and painted foliage used it); the one tone mapper is the old highlight shoulder, bypassed (clip) for every current biome as the old outdoor frames were; the biome grade is one baked 32³ volume. The canopy shade multiplies everything under it, as the old composite did. A retune is a separate, agreed step.
- **Shadows**: one 2048² map fitted to the view's ground slice (−6…+12 around the focus, padded, size in 4-unit steps), anchored at the world origin and snapped to texels; 8-tap rotated Vogel PCF with the biome's softness as radius (the old VSM blur radius). Casters shorter than 0.75 m are skipped, as before.
- **Terrain**: 32-cell chunks, 4 geomipmap levels chosen by projected height error ≤ 1 px, vertex-pulled heights, stitched edges; at most 4 blended layers per one-unit cell (Q7), cells aligned to the texture grid; layer lists, masks and atlas codes built once at load.
- **Scenery and buildings**: one instance buffer culled on the GPU for camera and sun every frame; one indirect draw per model section; alpha-to-coverage for alpha-tested foliage under MSAA; the old trunk sway and leaf flutter in both passes. Draw calls depend on the models present (29 on the shipped maps), not on instance counts.
- **Positions follow the old renderer**: decorative stamps stand at the cell centre `(x + 0.5, y + 0.5)`, standing trees at their resource cell, buildings at their footprint centre; scale is render asset × appearance × definition for settlement models and appearance alone for trees.
