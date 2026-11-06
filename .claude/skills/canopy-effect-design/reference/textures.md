# The texture vocabulary

A good effect library is about 200 small textures, of which a few dozen do most of the work: five textures (soft dot, star glint, hard dot with halo, comet streak, soft ring) cover more than half of all layers studied. They are **greyscale, colourless, centred, and fall smoothly to exactly black (additive) or transparent (alpha) well before the edge**; colour comes from the layer's curve. 256–512 px; flipbooks 8×8 at 1024–2048 px.

Our own library is being made (D113: Blizzard's textures stay in the lab and never ship). `tools/effects/fx_textures.py` generates the primitives as `asset.effects.fx.<name>` (soft-dot, -tight, -wide, pin-dot, soft-disc, star-glint, cross-glint, burst-flash, burst-flash-streak, line-flare, comet, soft-beam, soft-beam-thin, streak-cone, ring-clean, ring-thick, ring-hard, ring-shock, ring-rays, ring-wispy, swirl, swirl-blur, embers 4×2, crack-web, flame 8×8, smoke 8×8); it writes into a package folder (the lab until Jiri agrees the look). Older one-offs are in `base/assets/effects/` (`vfx/`: mote, puff, ash, bolt, blast-ring, chunk, flame-strip, shell, vortex, wake). New textures are asset folders (skill **canopy-assets**).

**Fire and smoke are simulated** (`tools/effects/fire_textures.py`, a small 2D fluid simulation run with Blender's Python): `fire-loop` (a whole fire, 8×8, looping seamlessly, its temperature colour baked in: white-yellow core, orange, red edges), `fire-smoke` (a puff rolling up and thinning, 8×8, alpha) and `ground-scorch` (char with a ragged edge, alpha). Simulated flipbooks look far more alive than noise-painted ones (Jiri, 2026-11-06); reach for the simulation for anything fluid. The base package ships these with the primitives the fires use (`pin-dot`, `soft-disc`, `soft-dot-tight`, `soft-dot-wide`) in `base/assets/effects/fx/`.

**Alpha:** an additive layer adds the texture's colour weighted by its alpha (as PopcornFX's Additive), an alpha-blended one covers by its alpha. So an additive texture is brightness in RGB with alpha 1 (or alpha shaping it), and a smoke or debris texture puts its coverage in alpha; never leave bled colour where alpha is 0 unless alpha stays meaningful.

**Writing PNGs with OpenImageIO:** set `spec.attribute('oiio:UnassociatedAlpha', 1)`, or it divides RGB by alpha on write and every soft glow cooks to flat white with a hard edge.

## Glows and points (procedural is fine)

| Texture | Look | Used for |
|---|---|---|
| Soft dot | Gaussian, slightly hot centre; tight, medium and wide falloffs | Everything: cores, motes, flashes, sparks when stretched, dark pools (alpha) |
| Hard dot with halo | Tiny white pinpoint in a faint halo | Sparks and small motes that must read as points at any size |
| Soft disc | Flat-topped wide glow | Ground pools, big flashes |
| Star glint | Hot point, 4–8 hairline rays, faint halo | Glitter, lingering motes, magic heads |
| Cross glint | 4 long thin arms, no halo | Mixed with the star glint for variety |
| Burst flash | Fat uneven star with short ragged rays; a variant with a horizontal lens streak | The 0.2 s hit and cast flash |
| Line flare | Horizontal lens streak | Accent on big flashes |
| Comet streak | Rounded hot head, long fading tail, about 1:6 | Velocity streaks, falling stars, burning arrows |
| Soft beam | Brightest at the bottom centre, fading up, soft sides; two widths | Pillars, light shafts, rising rays, flame columns |
| Streak cone | V-shaped radial streak | Blink and burst rays |

## Rings and swirls (procedural)

Clean thin ring; thick glowing ring; ring with a hard inner edge and a soft outer falloff; soft noisy shockwave ring; radial-streak ring (dark centre, rays outward); soft radial puff with a darker centre; wispy broken ring; ragged energy ring; eclipse ring (dark disc, bright rim). Swirl: a 2–3 arm soft spiral, and a motion-blurred partial spiral for spin discs.

## Painted (needs an artist pass or image generation, then cleanup to greyscale)

- **Flipbooks 8×8:** flame wisp (a lick that forms, curls and dissipates over 64 frames), soft smoke puff, crisp smoke, fade-fire. These carry fire and smoke: no big painted fire textures.
- **Small atlases (random variant per particle):** embers 4×2 (crescents, broken rings), electric arcs 2×2 (forked cracks with a hot node), electric disc 2×1, leaves 2×2, bubbles 2×2, liquid blobs 2×2, smoke stills 2×2.
- **Debris 3×3, in colour, lit from above, dark edge:** stone, wood splinters, chitin, amber, bones.
- **Ground:** radial crack web (and a blurred copy for additive glow), scorch (colour, alpha), one element star per element (frost shards …).
- **Sigils and runes:** circular rune discs with a strong silhouette, a ring of glyphs, a 2×2 glyph atlas; one set per race in our own visual language (ants grown, organic; beetles riveted, mechanical).
- **Emblems:** radial petal or thorn bursts, one silhouette per element.
- **Beam tiles (horizontally seamless):** wavy multi-strand energy, straight core with wisps, a lightning strip flipbook.
- **Liquid:** splat, droplet blobs, tendrils with an erosion alpha.

## Rules

- No colour baked in (except debris, blood, dust and painted decals); no baked background; nothing touching the edge.
- Bright enough at the core that a value boost of +2…+5 blooms; the halo must stay soft or bloom shows its edge.
- One texture per idea; variety comes from mixing two (star and cross glint) and from size, roll and brightness ranges.
