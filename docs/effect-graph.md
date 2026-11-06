# Effect graphs and the node editor (D113, 2026-11-05)

Status: **agreed with Jiri** ("let's build it as node editor with all bells and whistles … we have time, so make it proper"; the canvas and previews from his reference, "figure it out"). D113 in docs/decisions.md records the decisions; this document is the milestone's reference. Being built.

Jiri, 2026-11-05: "let's build it as node editor with all bells and whistles … this will make authoring MUCH easier and MUCH more powerful. Each step should have preview right? As well as the end preview."

## Why

Warcraft III: Reforged's spell effects are PopcornFX effects: a spell's model (`HolyBoltSpecialArt.mdx`, 1.4 KB) is an empty shell holding one PopcornFX emitter, and the effect (`.pkb`) is a compiled node graph. We decoded the format (all 912 HD effects parse; dumper in the research notes) and counted what they use:

| | Reforged's 912 effects | Our effects today |
|---|---|---|
| Structure | A graph per layer, compiled to per-particle programs; median 4 drawn layers, up to 27; spawners are layers that fire events into others; 113 effects spawn particles when others die | A list of fixed-shape layers (ring, pillar, burst, billboard …) with fixed settings and a few curves over life |
| Per-particle values | Random ranges and curves on anything; colour shifted in HSV in 773 effects; damping, collision tests, payloads passed between layers | One value per layer; curves for colour, opacity, size, additivity, erosion and heat |
| Simulation | Stateful: velocity integrates, forces act, particles die and spawn children | Stateless: each particle a closed-form function of its age |
| Renderers | Billboards (3,974 layers): camera-facing 2,613, flat 525, along an axis 834; ribbons 17; distortion 6; flipbooks 1,475 | Billboards (camera, ground, stretched along motion), ribbons, meshes, lights, flipbooks |
| Materials | Additive or alpha, soft particles, colour ramps, alpha remap, atlases | All of these already |
| Textures | A shared library of 453 greyscale FX textures (213 in use), coloured per layer | 46 one-off images |

The renderer is nearly there; the gap is the **model**: per-particle programmability, events between layers, and authoring that uses them. Holy Light, decoded, is five layers (beams growing up along a curve, sparks thrown up with upward acceleration and stretched along their path, flashes, orbiting motes whose size pulses, a rune) in under a second. Our Holy Light heal is one layer of nine sparks.

## The model

An **effect** is one JSON document in `base/effects/` (the id, bindings to abilities and states stay as they are, D65). It holds:

- **Parameters** the effect exposes to whoever plays it: radius, duration, tint, team colour, a target point. An ability's binding fills them (`sizeFrom: radius` today becomes a parameter), so one effect serves a small and a large Flame Strike.
- **Layers**: particle systems, each one free-form graph on its own canvas (Jiri, 2026-11-05: "an actual node canvas editor, with draggable arrows connecting node to node"). A layer has:
  - **Spawners**: when particles are born. A burst, a rate (constant or a curve over the effect's time), or an **event** from another layer: its particles' birth, death, a collision, every n seconds, or a payload (position, velocity, colour) passed along.
  - **Nodes** that compute values and write particle attributes (position from a shape, life, velocity, size, rotation, colour, any custom attribute), forces, curves sampled by life, colour shifts, kill conditions.
  - **Renderers**: one or more renderer nodes whose input pins (position, size, axis, colour, frame, emissive …) take wires.
- **No stage lanes: the compiler infers the stages**, as PopcornFX does. A node whose inputs depend only on spawn-time values (randoms, shapes, the spawn index, effect parameters) runs once when the particle is born and its result is stored; a node that depends on age, life ratio, the step or another changing attribute runs every step. Nodes that depend on nothing per particle (constants, curves of the effect's time) run once per layer per step. A node can be pinned to "at birth" explicitly (sample this curve once, not every frame).
- **Templates** (sub-graphs): a group of nodes saved as one node with exposed inputs. "Hazy ring", "fire puff", "soft beam column" are templates with size, haze and colour inputs, so small and large variants are one template with different numbers (Jiri: "a small hazy ring, a bigger hazy ring, more haze, less haze"). Effects and templates live side by side; editing a template updates its users.

A graph is **nodes with typed pins** and **edges**. Types: float, vec2, vec3, vec4, int, bool, colour (HDR, linear), curve, gradient, shape, texture, orientation. Each node has an id, a kind, its settings, and its position on the canvas (the editor's layout, ignored by the game). Particle attributes are read and written by attribute nodes; the built-in ones are position, velocity, life, age, life ratio, size, size 2D, rotation, axis, colour, alpha, texture frame, seed, and authors may add their own.

### Authoring nodes (D115, 2026-11-06)

Jiri, 2026-11-06, after reading the imported layers (about 34 low-level nodes each, up to 162): "If popcorn allows only those nodes, then imo those are the nodes we should support ONLY … I'd still like to be as close as able." Authors build layers only from the high-level nodes below, one node per artist-level idea, as PopcornFX's templates are. The low-level nodes (the table after this section) remain only inside the compiler, as the parts these nodes compile to.

**Wired, as PopcornFX v2** (Jiri: "v2 also means that we will ACTUALLY be able to see results of nodes this time. As in - okay spawn particles. Now color them. Now rotate them as vortex"). A layer is a flow of high-level nodes from its Spawner to its renderers: the particle's values (position, velocity, size, colour, rotation, frame) travel along the wires, each node taking them in and passing them on changed. Every setting of a node is also an input pin: unwired, its value or range is typed on the node; wired, it is driven from elsewhere (colour from speed, size from distance to the target). Value nodes feed those pins from the side: **Random range**, **Curve over life**, **Gradient over life**, and the particle's own values (age, life share, speed, position). Plain maths nodes are not part of the vocabulary. Layers connect through events: an Emit node fires, another layer's Spawner listens.

**Previews show each step.** A node on the flow previews the layer as it stands after that node, drawing anything not set yet upstream as small white soft dots: the Spawner shows dots appearing, the Shape spreads them, Launch throws them, Colour over life tints them, Orbit swirls them, the Billboard shows the finished layer. Value nodes preview as what they are: a curve's plot, a gradient's strip, a random range's spread.

**Settings** take one of:
- **Number:** fixed, or a random range per particle (min, max).
- **Vector:** fixed, or a random range per component.
- **Over life:** a curve (Hermite keys, the curve editor) times a random range per particle.
- **Gradient:** colour and alpha keys over life, HDR (values above 1 glow).

Distances are metres, angles degrees, times seconds; sizes are full widths.

| Group | Node | Settings |
|---|---|---|
| Spawn (one per layer) | **Spawner** | Mode: burst (count, at time), stream (rate per second, delay, duration with 0 for "while the effect plays", an optional rate curve over the duration) or event (another layer's Emit: one per event, spread along the parent's motion within a step, or a spray of a rate per second from each event for some seconds along a rate curve); life (seconds, range); ends with its life or with the effect (fading out over a time) |
| Birth | **Shape** | Point, sphere, disc, ring, cylinder, box or ellipsoid; radius, inner share, height, size; surface or volume; offset; placed at the emitter (or the event, for event-born layers); follows the parent particle (dying with it), the emitter or the target |
| | **Launch** | Direction: outward from the shape, up, along an axis, in a cone around an axis (angle) or random; speed (range); share of the parent's velocity inherited |
| Motion (every step, in order) | **Physics** | Gravity (vector), drag, wind (the velocity drag pulls toward) |
| | **Turbulence** | Strength, scale (m), speed, detail (1–3 octaves) |
| | **Orbit** | Axis, angular speed (range, either sign) |
| | **Path** | From and to (emitter, caster, target, or an offset above one of them), progress over life (curve), arc height: a falling star, a homing mote |
| | **Beam** | Placed along the caster–target line by the particle's share of its batch; sideways wave (amplitude, frequency, speed), jitter |
| | **Bounce** | Ground collision: restitution, friction, die on hit |
| Appearance | **Size over life** | Start size (range) times a curve; optional stretch along velocity |
| | **Colour over life** | Gradient; brightness (range); hue jitter; saturation and value boost (HSV); tint by team colour |
| | **Spin** | Start angle (range), speed (range), slow-down |
| | **Flipbook** | Columns × rows, first and last frame, over life (loops), at a frame rate, or a random frame; random start |
| Events | **Emit** | When: birth, death, every interval (s), every distance (m), on bounce, when the effect ends; count (range); chance; carries position, velocity and colour to the listening layer |
| Draw (one or more) | **Billboard** | Texture (atlas from Flipbook); blend additive, alpha or distort; facing the camera, flat on the ground, along an axis or along velocity; soft edge |
| | **Ribbon** | Texture, tiling along its length, blend; a strip through the layer's particles in birth order (each parent's children a strip of their own) |

**The whole spell on one screen** (Jiri: "Spell is full graph visible at one screen, with previews for each connection - now with purpose-specific node, each preview should be different and it should make sense"): every layer's flow side by side on one canvas, the event links between them, each node previewing what it adds.

**Built (2026-11-06)**: every node above with the settings in `crates/fx/src/flow.rs` (the authority). Built since: a stream's `rateCurve` and Emit's `rateCurve` (how often over the particle's life: blood sprays, footprints and debris that taper off), the ellipsoid shape, colour's `hue` spread per particle, colour's `team` (tinted by the owner's player colour, as Reforged multiplies it), and Emit `when: end` (once, as the effect stops). Not built yet: an event spawner's count per event, turbulence detail, spin slow-down (no Reforged effect was found using it); placing a shape at the caster or the target is done by Path and Beam. Life, Follow and Ends with effect were folded into the Spawner and the Shape (Jiri, 2026-11-06, on a 13-lane blood splash: "what is this monstrosity?"), and a spray from each event is the Spawner's `spray`: the compiler adds its hidden sprayer (`canopy_fx::flow::parts`, a layer `<id>~spray` born of each event that fires the layer's births), so PopcornFX's invisible spray helpers are no lanes of their own. The editor's graph view shows the whole effect on one screen, read left to right (Jiri: "the classic left-to-right box-then-wire-then-box until they all merge into the ultimate desired effect"): every layer is a row, its chain from the Spawner to the renderer box by box; a layer born of events hangs under the Emit that fires it, linked down from it (magenta); every renderer wires into the Effect node on the right, which plays the whole effect (length, loop and layers above its preview). Nodes that change the look show a live preview; Spawner and Emit do not. Value nodes sit under the node they drive. `+` on a wire (or a right-click on a row) inserts a node there; a node's context menu moves it left or right or removes it (what came before then feeds what came after); the inspector edits its settings (ranges, curves and gradients with their keys, choices, the event a Spawner listens to) and drives a setting by a value node ("Drive by…"). Content holds only these nodes: a layer starts with a Spawner, and a primitive in an effect file is an error.

**A fixed set.** Nothing else is placeable, and authors cannot define nodes. If a spell needs something the set cannot do, the spell changes (Jiri: "maybe we should change the spell"). **Blueprints** come later: a selected part of a graph saved and placed into other effects, so authors build their own "hazy ring" without new code. Reforged's layers are converted into these nodes by the importer, as closely as each pattern allows; small differences are accepted (Blizzard's effects are reference only and will be removed).

### Compiler primitives (the first node set)

| Group | Nodes |
|---|---|
| Spawn | Burst, Rate, Rate over curve, On event (birth, death, collision, periodic), Payload in, Spawn limit |
| Shapes | Point, Sphere, Disc, Ring, Cylinder, Box, Ellipsoid, Cone, Line, Mesh surface (a model's triangles), Spline; each with volume/surface and inner radius |
| Values | Constant, Random (uniform, per-component, gaussian), Random from seed, Effect parameter, Effect time, Particle attribute, Spawn index, Noise (value, simplex, curl) |
| Curves and colour | Curve (Hermite keys with tangents, the editor's curve widget), Gradient, Sample by life / effect time / any input, Remap, HSV shift, Colour multiply, Blend colours, Team colour |
| Maths | Add, Subtract, Multiply, Divide, Lerp, Clamp, Min, Max, Abs, Pow, Exp, Sqrt, Sin, Cos, Dot, Cross, Length, Normalize, Rotate vector, Swizzle, Compare, Select |
| Forces | Gravity or constant acceleration, Drag (exponential), Orbit (vortex around an axis), Pull or push towards a point or axis, Turbulence (curl noise), Wind, Ground collision (bounce, stick, kill, event), Spring to target |
| Transform | Local or world space, Follow emitter, Inherit velocity, Look at, Orient to velocity |
| Events | Emit event, Kill, Payload out |
| Renderers | Billboard (camera-facing, flat on a plane, along an axis, spheroid along an axis, along velocity), Ribbon (trail or beam between two points, with UV tiling), Mesh (a model per particle, tinted), Light (point light per particle, budgeted), Decal (projected onto the ground), Distortion (screen refraction, later: six Reforged effects use it) |
| Materials on renderers | Texture or flipbook (atlas size, frame from life or random, loop), Blend (additive, alpha, the existing additivity curve), Soft particles distance, Colour ramp (texture brightness picks the colour), Alpha remap / erosion, Emissive strength for bloom, Fresnel fade for flat cards |

Every node's settings can be a fixed value or a pin, so any setting can be random, curved or computed.

## Runtime

- **The cooker compiles each layer's graph to WGSL** (spawn/init and update kernels, plus the renderer's attribute fetch), type-checks it and reports errors with the node's id. Shaders are generated once per effect at cook time and cached; identical graphs share one pipeline.
- **Particles are simulated on the GPU in compute passes**, stateful: one pool of particle storage with fixed budgets (as `MAX_PARTS` today, raised), a free list, and per-layer ranges. Events (death, collision, periodic) append to event buffers that spawn the next step's children. Rendering stays in today's effects pass, extended with the new billboard modes, decals and distortion. WebGPU baseline (D2): compute and storage buffers are core WebGPU; no extra features.
- **Time**: a fixed simulation step (60 Hz) with the render time interpolated between steps, so motion doesn't depend on frame rate. **Seeking** (the editor's timeline, replays jumping) re-simulates the effect from its start to the target time in fixed steps; effects are a few seconds, so this is cheap; looping effects re-simulate from one loop.
- **Bindings and cues stay**: the effect director keeps starting, following and ending cues exactly as now (anchors, lifetimes, statuses, deliveries); a cue now instantiates a graph with its parameters instead of fixed layers.
- **Budgets and level of detail**: per-layer spawn counts scale with a quality setting and distance; the director keeps its cue budgets. Measured with `just profile` on the base M1 (AGENTS.md).
- **Render-only**: effects never feed the simulation (D4 untouched); their randomness comes from a seed per cue.

## Authoring: the node editor

The Effects workbench (crates/editor) becomes a node editor. Every change is an editor command (undo, the shared history, MCP).

- **D115 replaced the free canvas below with lanes of authoring nodes laid out by the editor** ("Authoring nodes" above); the rest of this section is the original plan, kept for the parts still to come (templates as blueprints, the final preview's extras, MCP tools).
- **Canvas**: a free node canvas like PopcornFX's (Jiri's reference, "something like this but better"): nodes placed anywhere, curved wires dragged from an output pin to an input pin (type-checked: a wrong type refuses the drop, a convertible one inserts the conversion), pins coloured by type, zoom and pan with a minimap, a searchable node palette (space, right-click on the canvas, or dropping a wire on empty canvas offers only nodes that fit), backdrops (named, coloured frames grouping nodes, as Reforged's own effects use), comments, reroute dots, alignment, multi-select, copy and paste across effects, collapse a selection into a template. Wires show the stage they run at (thin: at birth, thick: every step), and each node shows its cost.
- **Layers panel**: the effect's layers with solo, mute, order, and a small live thumbnail each.
- **Inspector**: the selected node's settings, with the curve editor (Hermite keys, tangents, presets), gradient editor, shape gizmo, texture picker over the FX library with thumbnails.
- **Previews on the nodes themselves** (Jiri: "node itself has preview … and then the next node which connects to it has its own preview and shows the modified effect"):
  - **What a value looks like**: every node shows its output inline where that reads: a plotted curve over life (with the playhead), a gradient or colour strip, the spread of a random as a histogram, a shape with its sample points, a texture or flipbook thumbnail, a number.
  - **What the effect looks like at this node**: any node whose output reaches a renderer pin also shows a small live viewport of the layer **rendered as if this node's output went straight into that pin**, the rest of the graph unchanged. Follow a chain towards the renderer and each node's viewport shows the effect one step more finished: the bare cylinder of points, then with random sizes, then growing along the curve, then coloured, then HSV-shifted. A node feeding several pins previews them together.
  - **Each layer** has its own thumbnail; **the final preview** is a full viewport.
  - **Cost**: node viewports render small (about 128 px), share one GPU simulation per layer variant, update only while on screen, and the selected node runs at full rate; they can be collapsed per node or for the whole canvas.
- **The final preview**: the effect playing over the game's ground at game camera angle or an orbit camera, with a scale dummy (a Warrior and a hero), a timeline with play, pause, scrub, loop, speed and frame stepping, the cue played as its binding plays it (on a caster, on a target, a missile flying between them), the night and day lighting of a chosen biome, and a stats readout (particles alive, draws, GPU time).
- **For agents**: the format is plain JSON with the schema generated from the Rust types; MCP tools add nodes, connect, set, make templates, validate, and render a frame sheet of the preview, so agents author effects as well as people do.

## Reforged as the test of fidelity

- A **`.pkb` importer** (a tool, not shipped) translates a decoded PopcornFX effect into our graph: its layers, samplers (curves, shapes), scripts (node by node; the decoded instructions map onto our maths and attribute nodes) and renderers. Run over all 912 effects, it reports which nodes or renderer options we still lack: the delta, measured instead of guessed.
- Imported effects and Blizzard's textures stay in the **local lab** (`target/wc3-lab`, a copy-on-write clone of `base/` in the ignored `target/`), never committed or shipped. They are references to match against and to learn from.
- **One-to-one check**: the same spell captured in Reforged and in our preview at matching camera, time and scale (one PopcornFX unit is about 3.1 of our metres: Warcraft scales PopcornFX by 100, and one of their 128-unit cells is our 4-cell block, D84), side by side, frame by frame.

## Our own FX texture library

Reforged's library is the model: about 200 small greyscale textures in groups (flares, flashes, soft beams, rings and shockwaves, runes, wisps, smoke and fire flipbooks, trails and beam tiles, ground cracks, noise), coloured per layer. We make our own in the same spirit (painted or generated, greyscale, tileable where it matters), so imported reference effects can be rebuilt on our textures and shipped. Fire and smoke are simulated rather than painted (`tools/effects/fire_textures.py`, 2026-11-06): a small 2D fluid simulation (buoyant hot gas, swirl, sharp advection) gives the `fire-loop` flipbook (a whole fire, looping seamlessly, its temperature colour baked in), `fire-smoke` and `ground-scorch`. The first shipped fires built on them are `effect.fire.torch` and `effect.fire.ground` (`base/effects/fire/`).

## Existing effects

Every current layer shape (ring, pillar, burst, glow, streaks, rain, missile, beam, wavefront, particles, ribbon, mesh, light, sound) becomes a template, and the cooker converts every existing effect to graphs, so the game keeps one effect system. A capture test compares each converted effect with today's at fixed times before the old evaluator is removed.

## Order of work

1. **Format and runtime**: the graph types, validation, the WGSL compiler, the GPU simulation and the new renderers; Holy Light built by hand from the decoded numbers, side by side with Reforged footage.
2. **Importer and fidelity bench**: `.pkb` to graph, the coverage report over 912 effects, Flame Strike, Level Up, the Way Gate portal, Drain Life and the Black Arrow matched.
3. **Node editor**: canvas, palette, inspector, curve and gradient editors, per-node and layer thumbnails, the final preview with timeline and bindings, MCP tools.
4. **Templates and our texture library**.
5. **Conversion of every existing effect**, then the old layer evaluator removed.

## Progress

- **Events between layers** (built 2026-11-05): a layer's Emit event nodes (on death, birth, every interval, every distance travelled, every step, with a condition and a payload) spawn the particles of the layers listening with On event. On the GPU, without readback or atomics: each parent particle writes an event record per channel each step, and the child's birth kernel runs once per parent slot, filling the ring of child slots that parent slot owns; children step right after their parent in the same instance, so the same cue at the same time has the same particles. Children spread along the parent's motion within a step, read their parent particle live (Parent: follow it, die with it), and chain to any depth.
- **The rest of Reforged's capabilities:** particle attributes (state of the particle's own), compare, select and logic, kill, ground hits, noise, ellipsoid and box shapes, an "effect running" flag, RGB↔HSV, ribbons (strips through each parent's children in birth order), the cue's two ends (caster and target, followed as they move: beams, drains, links), team colour, and distortion (billboards that bend a copy of the scene taken before effects draw).
- **Authoring nodes** (D115, built 2026-11-06): the 19 nodes and 4 value nodes, expanded to primitives by `canopy_fx::flow::expand` (with `until`, a node's preview); the editor's lanes; content refusing primitives. Our showcase effects are rewritten in them (`tools/effects/showcase.py`), and every Reforged layer is converted by `tools/effects/pkb_flow.py`: it runs the transcription below on the CPU and fits the nodes to what its particles do (4,757 of 4,905 layers cleanly, 148 approximately, all compiling).
- **Importer** (`tools/effects/pkb_import.py`, lab only, now the first half of `pkb_flow.py`): transcribes the scripts literally (attributes in order, conditions, events, decoded opcodes, beams, distortion): of 4,906 layers in the 912 effects, about 4,850 whole, 60 partly, 1 file unreadable, every built one compiling to valid WGSL. Without reference footage the absolute scale (one PopcornFX unit taken as 3.125 m, billboard size as a radius) is unconfirmed.
- **The craft, distilled** into skill `canopy-effect-design`: layer roles, timing by lifetimes, colour in curves with HSV value boosts, drag and streak motion, variation, the texture vocabulary and recipes.
- **Our own textures:** `tools/effects/fx_textures.py` generates the primitives (glows, glints, streaks, beams, rings, swirls, embers, cracks, flame and smoke flipbooks) as `asset.effects.fx.*`; first showcase effects in `tools/effects/showcase.py`. Both are written into the lab until the look is agreed.
- **The Effects stage** casts nothing by hand: the effect plays by itself, again after a short rest (a looping one plays on), and from its start after every edit. Its panel holds only what changes the look: on the target or from the caster (beams), the units beside it and their distance, their player colour (the effect's team colour), and the ground; Pause, Restart, a tick on and the speed, and one timeline lane for the effect.
- **The stage** of the Abilities and Effects editors stands on plain zinc with a faint 1 m and 4 m grid (`asset.terrain.editor-stage-grid`).

## Decisions (D113)

1. **Layers as free-form node graphs** with inferred stages, spawners, events between layers, templates and exposed parameters; the node library above is the first set.
2. **GPU simulation of stateful particles**, fixed step, re-simulated for seeking, replacing today's closed-form particles.
3. **A real node canvas** with wires and previews on the nodes: values inline, and a live viewport on every node that reaches a renderer, showing the effect as if that node fed the renderer.
4. **Every existing effect converted** to graphs, then the old layer evaluator removed: one effect system.
5. **Reforged as the fidelity test**: the `.pkb` importer and Blizzard's textures stay in the local lab, never committed or shipped; side-by-sides need reference footage (Jiri records it, or Claude drives the game with his yes).
6. **Distortion** (heat haze) comes with the renderers in step 4 ("all bells and whistles"), after the core.
