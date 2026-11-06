---
name: canopy-effect-design
description: What makes a spell or impact effect look great and how to build one in Under the Canopy's effect graphs (D113) — the craft distilled from 900 studied RTS effects: layer roles, timing by lifetimes, colour in curves with HSV value boosts, drag and streak motion, variation, the texture vocabulary, and recipes (impact, nova, missile, heal, strike from above, shield, aura, portal, blink, level-up, beam, fire, smoke, summon). Use before designing, authoring, reviewing or improving any visual effect, spell look or particle graph.
---

# Effect design

How to make an effect that reads at RTS distance and feels rich, and how to build it with our graph nodes. Read **canopy-abilities** for binding an effect to an ability and [docs/effect-graph.md](../../../docs/effect-graph.md) (D113) for the graph model. The craft below was distilled from Warcraft III: Reforged's 912 PopcornFX effects (decoded and imported into the local lab, `target/wc3-lab`): **principles, not copies**. Never ship their textures or effects; build our own on our textures, in our races' colours.

Units here are ours: metres, **y up**, seconds. (A PopcornFX unit is about 3.1 m.) Billboard `size` is the **full quad width**; glow textures are mostly falloff, so the visible glow is a third to a half of the quad.

## What "good" means

1. **It reads in 0.3 s from the overhead camera.** A bright core says where, a ground ring says how big, and direction says what kind (up = blessing, out = release, in = gathering, down = from the sky).
2. **It is several kinds of particle at very different scales**, not many of one kind: a 6 m flash, 0.3 m glints, a 10 m column. Four to eight roles; 50–250 particles for a big spell, about 50 for an impact, one or two quads for an aura.
3. **It decays in a cascade.** Every layer starts at once; lifetimes do the timing: flash gone at 0.3 s, ring at 0.5 s, sparks at 1 s, smoke and motes at 2 s. Nothing pops off.
4. **A few particles are much brighter than the rest.** Random brightness ×1–6 per particle is the cheapest richness there is.
5. **Light is additive; matter is alpha.** Glow, sparks and magic add light. Smoke, dust, debris, blood, darkness and loss are alpha-blended, and one dark alpha layer under bright additive ones gives contrast.
6. **It is a kit.** A ring, a spark burst and a mote layer, recoloured and rescaled, make dozens of effects. Build templates and palettes, not one-offs.

## Anatomy: the layer roles

| Role | Billboard | Count | Life | Size | Job |
|---|---|---|---|---|---|
| Core flash | camera, additive, burst flash or soft dot | 1–3 burst | 0.2–0.5 s | grows fast to 4–12 m, peaks at 10–20 % of life | The instant. The brightest pixel. Gone before anything else peaks. |
| Shock ring | plane (axis up), 0.3–0.6 m above ground, soft ring | 1, or a stream of 5–20/s for 0.4 s | 0.3–0.5 s | grows 0.3→5–10 m, ease-out | The radius and the push. A stream of rings reads as a thick smeared wave. |
| Ground glow | plane, big soft dot | 1–4 | 0.5–1 s | 3–12 m | Fakes the light on the ground. |
| Ground decal | plane, crack, rune or scorch | 1 | 1–2 s, or the duration | 6–10 m | The mark that outlasts the burst; the identity seen from straight above. |
| Sparks | spheroid along velocity, additive, soft dot or comet | 15–70 burst, or 30–150/s | 0.2–1 s | 0.1–0.5 m wide | Energy and direction. Long streaks at birth, dots once drag slows them. |
| Motes / glints | camera, star glint or pin dot | 15–100 | 0.6–3 s | 0.1–1 m | The lingering sparkle after the peak; they drift, rise or orbit and twinkle. |
| Column / rays | spheroid along up, soft beam | 4–35/s for 0.4–1.5 s | 0.3–1.5 s | 0.3–1.5 m wide, 10–40 m long | "From the sky" and "to the sky". Many thin short beams flicker and breathe; one mesh doesn't. |
| Flame / wisp | camera, 8×8 flipbook, additive | 3–50 | 0.5–1.4 s | 1–4 m | Volume and the shape of fire or energy; the flipbook animates it. |
| Smoke / dust | camera, flipbook, **alpha** | 5–30 | 0.5–3 s | 1–6 m, growing | Weight and contrast; fire smoke starts lit and turns dark. Dust is tan, never grey. |
| Debris | camera, 3×3 atlas random frame, **alpha** | 2–17 | 1–3 s | 0.2–0.5 m | Physical punch: arcs out, falls, spins, flashes hot then cools. |
| Sigil / rune | plane on the ground (or camera overhead) | 1–2 | 0.7 s to the duration | 1–8 m | Identity and school. Always turning slowly, pulsing or flickering. |
| Vortex | plane, swirl ring, spinning 2–15 rad/s | 4–90/s | 0.7 s | 1.5–5 m | Wind, portals, channelling. |
| Arcs | camera or plane, 2×2 crack atlas, random frame and roll | 10–30/s | 0.1–0.15 s | 1–3 m | Lightning. Very short life is the whole trick; no beams. |

Each role gets its own spawner (its own layer for us) so it has its own window and life.

## Timing

- **One-shots are short:** impact 0.3 s readable, 1 s tail; spells 0.8–2 s; level-up 1.6 s; a teleport 3–4 s.
- **Start everything at t = 0; let lifetimes stagger it.** Explicit delays are rare. Real delays come from events (a meteor's 0.8 s fall triggers its impact).
- **Short streams instead of single-frame bursts:** 40/s for 0.5 s, not 20 at once. Births spread over frames make the peak thick and the decay ragged. Instant bursts only for hard impacts.
- **Inside a particle: fast attack, long decay.** Colour and alpha peak at 10–30 % of life (flashes and sparks at 4–15 %), then ease out. Slow-in (peak at 40–90 %) is for rings and things that gather.
- **In with late alpha = charging; out with early alpha = release.** Anticipation is implosion: a ground ring shrinking 6→1.5 m with its alpha peaking at 85 % of life, glints pulled inward.
- **Persistent effects have intro, loop and outro.** Intro: pop in over 0.3–1 s (scale 1.2→1 and fade in). Loop: breathe 1 → 0.9 → 1 or alpha 0.8 → 0.3 → 0.8 over 1.5–5 s. Outro: every layer fades on its own timer over 0.5–1 s, streams stop at once, and a closing burst (an expanding ring and 30 glints thrown out) is a nice flourish.
- **Pulsed auras** pulse every 1.5–2.5 s with 1.3–2.5 s lives so pulses overlap; two copies with slightly different lives drift out of phase.

## Colour and light

- **Textures are greyscale; colour lives in a curve over life.** Three keys: `(0,0,0,0)` → hot peak → cooled tail. Additive particles fade by going to black.
- **Colours cool as they die.** Fire: white-yellow → orange → deep red → nothing; gold → orange-red; blue → violet. The tail keeps only the dominant channel.
- **Brightness above 1 drives bloom.** Push value with Colour over life's **shift** (`[0, ds, dv]` in hue, saturation and value), not RGB multiplies, so the hue holds while the core burns white: +0.5–1.5 on long-lived motes, +2.5–7 on flashes, rings and sparks, up to +10 on thin streaks and rays. **Add saturation with brightness** (+0.1–0.6) or bloom washes to white.
- **Brightness × screen time stays about constant.** Big boosts on short, rare things (flashes, rings); long-lived and area layers stay dim (value boost ≤ 1, or below the texture's own brightness) so units standing in them stay readable.
- **Random per-particle brightness:** Colour over life's `brightness` range 1–3 … 1–6; value jitter large.
- **Retint by hue shift.** Author one ramp per kind (hot fire ramp, holy ramp, cool magic ramp) and rotate the hue for other elements.
- **Darkness is saturated deep colour next to a hot core**, plus a small alpha-blended dark layer underneath. Never black additive.
- **Our palette** (art direction, D61, canopy-icons): substance and mana are **violet** (#8a5cff–#b89cff), fire and forge orange (#ff8a35), healing, auras and royalty warm gold, armour and blocking steel-white, steam white. **Never green magic.** Ants: amber resin, violet liquid substance (never crystals). Beetles: furnace orange, steam, violet only inside glass.

## Motion

- **Exponential drag is the signature:** Physics `drag` 1–3. Particles shoot out and hang; total travel ≈ v₀ / strength. Bursts 2–3, sparks 1, smoke 0.25–0.5.
- **Radial bursts:** Launch `outward` from a sphere or ring Shape; a negative speed implodes.
- **Up is the default for blessing, healing and power:** Physics gravity `[0, +3 … +25, 0]` m/s², starting slow and speeding up. Gravity only for physical things: −12 to −45 m/s² for sparks, rocks and blood (snappier than real).
- **Orbit** 140–340 °/s, a range per particle and sometimes either sign, while rising: spirals where no two particles share a path.
- **Spin** (°/s): sigils 30–90 (portal discs counter-rotate), flashes and rings a random start angle, smoke ±60, flames ±170, embers ±340, debris ±1100–1700, vortex rings 570–860.
- **Streaks:** Size over life `stretch` 0.03–0.6 s with the Billboard facing `velocity`. Length follows speed, so drag turns streaks into dots; from overhead a streak flying at the camera turns round instead of vanishing.
- **Size over life:** flashes grow fast then shrink; rings grow linearly ×1–4; motes bump 0 → 1 → 0 (or twinkle 1 ↔ 0.4 a few times); sparks shrink; smoke grows ×1.5–2.
- **Birth heights** (Shape `offset`): feet 0.3–0.6 m, chest 2 m, overhead 5–12 m. Area effects use a `ring` at the radius.

## Variation

- **Every birth value is a range:** life ±20–50 % (the most important: it stops a burst dying in one frame), size ×0.5–2 (glints ×0.1–12: a few big among many tiny), brightness ×1–6, rotation full, speed ×1.5–2, orbit speed.
- **Atlases:** small atlases (2×2, 3×3, 4×2) give a random *variant* per particle; 8×8 flipbooks play over life (Flipbook `play: life`; smoke trails start 18–24 frames in, `first` 18, already puffed).
- **Mix two variants of one element** (a star glint and a cross glint; two beam widths) in one effect.
- **Sustained without repetition:** continuous streams with random life, random roll, overlapping crossfades of static sprites, decals on one cycle peaking at different phases.

## Building it with our nodes

One graph per layer, written only in the authoring nodes (D115; `crates/fx/src/flow.rs` is the authority, docs/effect-graph.md "Authoring nodes" the reference). A layer is a chain from its **Spawner** down to its renderer, wired `out` → `in`, in the order things happen to the particles; the compiler turns it into GPU kernels. Settings are values on the node, or wired from a **value node**. Units: metres, degrees, seconds; sizes are full widths. Nothing else is placeable: if a look needs something these cannot say, change the look (Jiri: "maybe we should change the spell").

| Node | Settings | Job |
|---|---|---|
| **Spawner** (one per layer) | `mode` burst (`count`, `time`), stream (`rate`, `delay`, `duration`; 0: while the effect plays; `rateCurve` over that span) or event (`layer`, `event`: another layer's Emit; one per event, or with `spray` seconds a stream of `rate` a second from each event along `rateCurve`); `life` range; `ends` life / effect, `fade` | When particles are born and how long each lives. A burst at `time` is a timed burst; a falling `rateCurve` tapers a spray off; `ends: effect` fades them out over `fade` seconds when the effect stops. |
| **Shape** | `shape` point / sphere / disc / ring / cylinder / box / ellipsoid (`radius` scaled by `size`); `radius`, `inner`, `height`, `size`, `fill` volume or surface, `offset`; `follow` none / parent / emitter / target | Where they start, at the emitter (a missile's head as it flies) or where the event happened; `follow` keeps them at that offset as it moves (with the parent: they die with it). |
| **Launch** | `direction` outward / up / axis / cone (`angle`) / random; `axis`; `speed` range; `inherit` (share of the parent's velocity) | Start velocity. |
| **Physics** | `gravity`, `drag`, `wind` | Forces every step. |
| **Turbulence** | `strength`, `scale`, `speed` | Swirling noise push. |
| **Orbit** | `speed` range (°/s), `axis` | Swirl about the emitter's axis. |
| **Path** | `from`, `to` (emitter, caster, target, each with an offset), `progress` curve, `arc` | Travel from one place to another over life: a falling star, a homing mote. |
| **Beam** | `amplitude`, `waves`, `speed`, `jitter` | Lay the batch along the caster–target line. |
| **Bounce** | `restitution`, `friction`, `onHit` bounce / die | The ground under the emitter. |
| **Size over life** | `size` range, `curve`, `stretch` (s of travel) | Size; stretch makes streaks. |
| **Colour over life** | `gradient` (RGBA keys, above 1 glows), `brightness` range, `shift` [dh, ds, dv], `hue` (random spread per particle, share of the colour wheel), `team` (`on`: times the owner's player colour, before the shift) | Colour and alpha; the HSV shift pushes value for bloom; `hue` makes a shimmer of neighbouring colours; `team` for banners, auras and glows in the player's colour. |
| **Spin** | `angle` range, `speed` range (°/s) | Roll. |
| **Flipbook** | `columns`, `rows`, `first`, `last`, `play` life / rate / random, `loops`, `fps`, `start` | Atlas frames. |
| **Emit** | `when` death / birth / interval / distance / step / bounce / end (once, as the effect stops), `interval` (s or m), `count` range, `chance`, `rateCurve` (how often over the particle's life) | Fires an event other layers' Spawners listen to; children start where it happened, with its colour. |
| **Billboard** | `texture`, `blend` additive / alpha / distort, `facing` camera / ground / axis / velocity, `axis`, `soft` | Draws sprites (`distort` bends the scene behind: heat haze, ripples). |
| **Ribbon** | `texture`, `blend`, `tiling` | A strip through the particles in birth order (each parent's children their own). |

Value nodes, wired into a setting (the editor's "Drive by…"): **Random range** (`min`, `max` per component), **Curve over life**, **Gradient over life**, **Particle value** (life share, age, speed, height, position, velocity). A layer that ends without a renderer draws nothing in the game: it only emits (a missile's head, a timer). The Graph view reads left to right: each layer a row from its Spawner to its renderer, layers born of events hanging under the Emit that fires them, every renderer wired into the Effect node, which plays the whole effect. Each node that changes the look previews the layer as the chain makes it up to there.

Patterns:
- **Radial burst:** Shape sphere (`fill: surface`) → Launch outward, speed 8–16 → Physics drag 2.
- **Streaks:** Size over life `stretch` 0.03–0.08 → Billboard facing `velocity`.
- **Colour:** Colour over life: a hot gradient, `brightness` 1–3, `shift` [0, 0.2, 2].
- **Ground decal or ring:** Shape point at offset y 0.3–0.6 → Spin angle 0–360 → Billboard facing `ground`, `soft` 0.5–1.
- **Flipbook over life:** Flipbook 8 × 8, frames 0–63, `play: life`.
- **Twinkle:** a size curve with 4–8 alternating keys 1 / 0.4.
- **Persistent layers:** a stream for the duration (`duration` 0) with lives overlapping, a long-lived disc whose size and colour curves fade in, hold and fade out, and the Spawner's `ends: effect` to fade when the effect stops.
- **Beam:** a burst of 30–50 → Beam (a sideways wave) → Ribbon; or flickering arcs: several short-lived beams with `jitter`.
- **Impact when it ends:** the moving layer has Emit `when: death`; the burst layers' Spawners listen to it.
- **Trail:** Emit `when: distance` with `interval` = spacing in metres (children spread along each step's motion, so fast movers never bead); smoke `when: interval`.
- **Attached emitter:** a child layer whose Shape has `follow: parent`.
- **Spray from each hit** (blood, footprints, debris): the child's Spawner listens to the hit with `spray` 0.3–0.6 s, `rate` 60–150 and a falling `rateCurve`.

Missing today (say so rather than faking it): collisions with anything but the ground plane under the emitter.

## How to design a new effect

1. **Say what it does in one line** and pick its direction (up, out, in, down) and its element palette.
2. **Pick 4–8 roles** from the anatomy table; at least one of each scale (big flash or ring, mid flames or column, small sparks or motes).
3. **Set lifetimes for the cascade** before anything else, then windows, then counts.
4. **Colour ramps** with hot peaks and cooled tails, value boosts by role, random brightness.
5. **Motion**: drag on bursts, lift or gravity by meaning, streaks on fast things, orbit on motes.
6. **Variation on every birth value.**
7. **Look at it** on the editor's Stage (Effects → Stage: it plays by itself, again after each end) and on every node's preview in the Graph view, at game zoom and from straight above, on the zinc stage and on a real biome at day and night. Check: does it read at 0.3 s, is anything still at full brightness after 1 s, does it wash out the units, does it pop off?
8. Show Jiri side-by-side captures before calling a look done (AGENTS.md: anything visible is his call).

## Reference

| File | Contents |
|---|---|
| `reference/recipes.md` | Generalised recipes with numbers in metres: impact, nova and ground slam, missiles (magic, physical, explosive), strike from above, heal pulse, shield, aura, portal, blink, level-up, summon column, drain beam, lightning shell, fire, smoke, explosions and debris |
| `reference/textures.md` | The texture vocabulary an effect library needs, what each is for, and how to paint it |
| `tools/effects/showcase.py` | Worked examples in the authoring nodes: Pillar of Flame (fire), Substance Nova (nova), Violet Gate (portal), Golden Mending (heal), Firework (events three layers deep); each layer is a list of (node, settings) |
| `reference/lab.md` | The local Reforged lab: importing, cooking, opening imported effects in the editor, and the reference effects worth studying |
