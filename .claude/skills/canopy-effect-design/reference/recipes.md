# Recipes

Generalised from the studied effects: the structure and the numbers that make each kind work, in our units (metres, y up, seconds; sizes are full quad widths). They are starting points: change the palette, the scale and the identity mark, keep the anatomy and the timing. Everything is additive unless marked **(alpha)**. "Hot ramp" is the cooling fire curve: `(0,0,0)` → at 10–20 % `(1.5, 0.55, 0.24)` → at 1 `(0.3–1.8, 0, 0)`; "value +n" is Colour over life's `shift` `[0, ~0.2, n]`. The recipes describe what the studied effects do; in our nodes "velocity = position × k" is Launch `outward` at the shape radius × k, rates in rad/s are × 57.3 for Orbit and Spin (°/s), and "spheroid, axis = velocity × k" is Size over life `stretch` k with the Billboard facing `velocity`.

## Impacts and novas

**Hit spark (melee or bolt impact), ~1 s, ~50 particles.** The projectile's flight is the anticipation; the impact has none.
- Flash: 1, camera, soft dot or burst flash, 0.2–0.45 s, 0.8 → 2–4 m, colour peak at 10–20 % ×2–5.
- Ring: 1, camera (reads from any angle) or plane for heavy hits, 0.3–0.4 s, 2.5 → 5–10 m ease-out, random roll ±45°, ×3–5.
- Sparks: 15–70 (40 default), spheroid soft dot, sphere radius 0.15–1 m with velocity = position × 15–70 (5–11 m/s), drag 1–3, axis = velocity × 0.03–0.05, life 0.2–0.6 s, size pops to its peak at 4–8 % then shrinks to 0, value +2…+5 with jitter ±2.
- Optional: 3–10 puffs of smoke **(alpha)** for earth and fire, 0.5–1 s; 1 lingering soft glow 0.7 s for magic.

**Ground slam / nova, ~1.2 s, ~60–80 particles.**
1. Flash: plane, 1, 0.2 s, 4 → 12 m ease-out, starts white-hot ×2–3.
2. Shock ring: plane, 1, 0.3–0.4 s, 1.5 → 11 m, value +3.
3. Smear ring: plane, 20–30/s for 0.4 s, life 0.3 s, 1.2 → 9 m. Reads as a thick wave.
4. Ground glow: plane, 1–4, 0.7 s, 12 m.
5. Decal (crack, element star): plane, 9 m, 1–2 s, alpha peak at 15 % then a slow fade.
6. Sparks: 40–50 streaks born on a ring (radius 5–12 m, `inner` ≈ 0.95), outward or upward 12–28 m/s, drag 3 or gravity −120, life 0.5–1 s, pop size curve (peak at 7 %), value +3…+5.5, jitter ±4.
7. Debris **(alpha)**: 15 rocks thrown from a wide sphere, gravity −120, hot-to-cool colour (colour ×8 → ×1 by 30–50 % of life), spin ±20.
8. Element puffs: 7 smoke or wisp particles from the ring, drag 3, 0.5–0.8 s.

**Explosion (shell, fire bomb).**
- t = 0: flat ground ring (plane, 1 → 5 m, 0.4 s, ×3) and a flat soft shockwave (1 → 6 m, 0.3 s, value +1).
- Flame columns: 8 rising soft beams (spheroid along up, lengthening as they rise, life 0.3–0.5 s).
- Sparks: 70, gravity −45, life 0.3–1.3 s, brightness ×0.5–1, hot ramp.
- Fireball: 9 flame licks (8×8 wisp flipbook) pushed out from a 3 m cylinder with drag 2, life 0.6–1 s, colour ×3, soft.
- Smoke **(alpha)**: 9 puffs spreading from a 6 m disc (drag 2), lift 0–12 m/s², life 0.5–1 s, brightness ×0.2–0.6, orange → black.
- Big: smear the flash, ring and dust over 0.1 s windows, add 20 debris (life 1.5–3.5 s) and two long smoke layers (1.5–3 s), and fire 2–3 smaller copies at other points. Dust from buildings is tan additive wisps, not orange: collapse is not fire.

## Missiles

A missile is one effect with two phases: an in-flight loop (head, trail, shed sparks) and an impact burst at the last position when it arrives (needs events; until then bind the impact separately to `impact`).

**Magic bolt.**
- Head: soft dot, re-emitted 10–15/s with life 0.1–0.35 s and growing ×2 over it (1.5–2 m), so one or two overlap: a living pulse, never a static sprite. Value +2…+5, hue jitter ±0.02.
- Core mist: smoke flipbook, **additive** in the element colour, world-fixed (left behind), 40/s, life 0.4–0.6 s, 0.8 m.
- Shed motes: star glints 40–80/s from a 0.3 m sphere, drift ≤ 2.5 m/s with drag 3, life 0.6–1.2 s, value +5 ±2.
- Line trail (needs trails by distance): ring dots every 2 cm, 0.15 m, life 0.4 s: a tube that never gaps.
- Impact: 40 spheroid sparks (5–9 m/s, drag 1, life 0.2–0.4 s), 1 camera ring (0.8 → 3 m in 0.3 s, ×5), 10 additive mist puffs.

**Physical missile (arrow, spear, rock).** The weapon sprite or model; a thin faint warm speed line; impact: a tiny ring flash, 15 warm-yellow sparks (6–11 m/s, life 0.2–0.4 s), and for earth 3–10 dust puffs **(alpha)** with 5–10 stones. Burning variant: a comet streak overlay (value +6, orange), flame licks 15/s (life 0.2 s), embers 80/s.

**Shadow / death bolt.** Dark smoke core **(alpha)** 30/s, a halo flare 6/s, flickering core sparks 70/s with a stepped size curve, 3–5 tendrils flying out at 37 m/s with drag 3 leaving dense dark dot trails. Impact: flash, ring, 50 streaks (value +2.8, drag 3), 5–10 smoke puffs.

## From above and from below

**Strike from above (holy bolt, smite), 0.8–1.6 s.**
1. Sigil: 1 rune 10–12 m above the target, life 0.7 s, alpha peak at 27 %.
2. Column: 8–20 soft beams over 0.4 s from a 0.6 m cylinder, stretched 15–40 m up, value +0.5.
3. Up-sparks: 15–20 spheroid streaks shooting up at 9–22 m/s with +25 m/s².
4. Collapsing flash: 3–6 starbursts shrinking 7 → 0.3 m while their colour peaks late (90 % of life). This is the snap.
5. Ground ring: 1 growing ring, 0.3–0.6 s.
Falling variant (meteor, star): add the impact set: a 3 m ground shockwave at ×3, 20 radial glints with drag 1–3.5, a 1 s flickering after-flash.

**Falling meteor with impact, ~1.6 s.** A body on a path from (−6, 30, 0) to the ground over 0.8 s, re-spawned at its point 10/s with life 0.1 s; a trail of flame wisps (0.5 s), embers (0.4–1.3 s) and warm-lit smoke **(alpha)** (0.7 s); at arrival, the nova at a third of its size. (Needs events and paths; until then, two effects timed by the ability.)

**Summon / revival column, 2–2.5 s.**
1. Rays: soft beams, spheroid along up ×15, 10/s for 0.8–1.6 s, rising to 10–20 m on an ease-in curve, width 1–2.5 m shrinking to 15 %.
2. Ground flare: plane, 5/s, 0.5 s, 2 → 9 m.
3. Descending rings: plane, 20/s for 1.3 s, born at 12 m and falling at 37–43 m/s with drag 2.5, landing on the target.
4. Spiral motes: 50/s for 1.4 s, orbit π–2π rad/s, rising with +30–90 m/s² and drag 1, twinkle size, brightness ×0.5–3.
5. Optional: a ground crack (9 m, 1 s, value +5.5) and wisps blowing out from a 9 m ring.

## Blessings and buffs

**Heal / restorative pulse, 1.2–1.5 s, ~25 alive.**
1. Core glow: soft dot at chest height, 0 → 4.5 m, peak at 25 % of 1 s, value +0.5.
2. Rising glints: star glints, 15–20 over 0.5 s on a 1.2 m shell; orbit 2.5–5 rad/s; velocity eased up toward 3–30 m/s (`v = v·e^(−dt) + target·(1 − e^(−dt))`); size 0.3 × random(0.5, 2.5) with a twinkle 1 ↔ 0.4; brightness ×1.5–2.5; life 0.4–0.8 s.
3. Up-glows: soft dots stretched up, 5–6 over 0.5 s from a 0.5 m cylinder at the feet, +6 m/s², length growing with life.
4. Heal over time: add a ground ring at 4–5/s and slow motes for the duration.
Colour: gold peak cooling to orange-red.

**Level-up / power-up burst, ~1.5 s, ~225 particles; all spawn for 0.6–0.8 s.**
1. Ground glow pool: 2, 4 → 12 m, life 1 s.
2. Ground rings: 4, 0.3 → 5 m in 0.4 s, value +8, ×3.
3. Inner column: 50 fast streaks from a 0.8 m cylinder at 80–110 m/s, accelerating; brightness random(0.1, 1.5).
4. Outer streaks: 70 on a 2 m cylinder at 15–30 m/s, life 0.4–0.6 s, peak at 40 %.
5. Spiral glints: 80 stars on an annulus 0.7–2.5 m, orbit 0.6–5 rad/s, rising 3–30 m/s, brightness ×1–3, white-gold → red.
6. Rising rings: 20 planar rings climbing at 6–28 m/s, shrinking 1 → 0.55, alpha peaking hard (2.0).
Minor version (an item, a tome): ~60 particles: an imploding ground flash, ~21 streaks, a 1 s rune, 2–3 rings, ~20 spiral glints.

**Protective shell / sustained buff.**
- Intro (0.4 s): a ground ring shrinking 6 → 1.5 m with late alpha; 40–60 glints on a 4.5 m shell pulled inward (velocity = position × −3.5, drag 5); the sigil decal pops to size over 0.3 s.
- Loop: the ground sigil turning 0.5–1.5 rad/s with a very high colour multiplier; halos around the body at 4–6/s, life 0.6–0.8 s, growing 2 → 5 m, one copy hue-shifted for contrast; motes 20/s and glints 50/s rising slowly (+9 m/s²).
- Outro (0.5 s): the sigil fades, one expanding ring, 35 glints thrown outward (position × 8, drag 2).

**Buff on a unit (intro + loop).** On apply: one energy ring 0.06 → 1.5 m in 0.3 s ×6 and a planar shock puff 4 → 0.6 m. A rune overhead that plays its intro over 1 s then loops with 4–5 irregular flickers (keys 0.72, 1.02, 0.86, 1, 0.84, 1, not a sine), fading over 1.5 s on stop. Ring pulses 4/s at head height (life 0.5 s, ×1–1.3). Theme sparks ~15/s at 0.1 s life (atlas, random frame and roll). Motes ~30/s from a 0.5 m sphere at 15–45 m/s, drag 1.

**Lightning shell.** 2–3 orbiting points each firing short bursts: arcs (2×2 atlas, 0.1 s), a flare (0.25 s), a tiny ring puff (0.4 s), spark dots (0.2–0.4 s). An electric disc: plane, 30/s, 0.15 s life, random roll, brightness ×1–3. A ring 3/s, 1 s. Everything lives 0.1–0.25 s, so it flickers by itself.

## Areas

**Aura on the ground (persistent).** Ground language only: nothing above knee height, so it reads as "on the army".
- Minimal: one thick soft ring decal 0.6 m above the ground; a new copy every 1.5–2 s, each living 2.5 s with a symmetric fade to alpha ≈ 0.45 and a random roll; size nearly constant (1.17 → 1.05) so it inhales.
- Rich: 3–4 decal pieces on one 2 s cycle, each peaking at a different phase (0.2 / 0.3 / 0.4), fading in over 0.4 s and out over 0.33 s.
- Pulse version: every 1.5–2.5 s one planar emblem grows 0 → 3.5–4.5 m (two copies, lives 1.3 and 1.4 s, 0.45 rad apart), a wave ring 0 → 6 m, alpha peak at 5–20 %, saturation +0.6–1, value +1.6…+10 on the brief pulse only.
- Keep the value low (−0.3…−0.7) on anything that stays.

**Hostile zone or cloud.** Rune or decal as the aura above, with smoke **(alpha)** 15/s, life 1 s, size 8–15 m, brightness ×0.2–0.4, rising +3; motes 50/s, 1–2 s, hue ±0.045, value ±2. Outro: everything fades over 0.5–1 s.

**Rain from the sky (blizzard, starfall, rain of fire).** Falling streaks at 120/s over a 28 m radius (spheroid along velocity, falling with −15 m/s² or a fixed fast speed), small splash rings at random ground points, a column or glow at the caster; each impact a miniature hit spark.

## Gates and movement

**Portal / gate (persistent), ~250 alive.** Local axis = the portal's normal; author in a neutral hue and hue-shift per gate.
1. Rim streaks: 40/s on the exact rim circle, moving inward at 0.3–1.5 m/s, axis along the normal growing ×5–13 with life, life 1–2 s, value −0.6, hue ±0.014, value ±0.8: light pulled into the tunnel.
2. Receding rings: 9/s planar wispy rings at 2.7 m, random roll, drifting along −normal with 3 m/s², life 2 s, alpha in by 14 %, ×2: depth.
3. Imploding swirl rings: 5/s, 2.3 → 0 m over 1.3 s, spinning 3–5 rad/s, value +10, ×1–2: the bright heart.
4. Drifting dust: 70/s tiny dots (0.06–1 m), life 2–3 s, damped drift along the normal, size peak at 15 %, value +4.
5. Optional: a ground glow pool and one slow soft swirl disc (≤ 1 rad/s) behind.
Ground-portal variant: two sigil discs counter-rotating (+0.5 and −1 rad/s), one lifted 3–12 m in the intro, a glyph cycling 2×2 variants at the centre, a curtain of thin streaks rising at 15 m/s (30/s, life 1.3–1.5 s). Intro 1 s scale 0 → 1; loop breathing ±10 % over 3 s; outro 0.75 s with the lifted disc sinking back.

**Blink / teleport flash (< 0.8 s).** 10–15 radial streak cones from a 6 m sphere (full length at 10 % of life, life 0.3–0.4 s); 1 starburst 0 → 6 m by 17 % with a ×8 flicker; 1 clean ring 1 → 4 m at value +8; 1 soft ring peaking late as the after-image. Mirror at the destination without the streaks.

**Mass teleport (3–4 s).** A master envelope (colour in by 5 %, held to 40 %, out by 100 %; size 0.5 → 1.3 → 0) multiplying: 2–3 pulsing starbursts rising 4.5 m; planar swirl discs 4/s, 0 → 20 m in 1.5 s, spinning −2; wispy rings 2/s, 1.5 → 20 m, spinning −3; soft puffs 5/s, 0 → 9 m; 100 motes bursting at 12 m/s, drag 0.5–1.5, life 1.5–3 s, value +3.5 ±2.

**Drain / link beam (needs beams).** A ribbon from source to target, 1–2 m wide, a tiling wave texture scrolling toward the receiver; alpha 0 at both ends and full from 15 % to 85 % (born inside the units); white near the source, saturated after 30 %; a 1 s brightness pulse; packets 1.5/s sweeping from target to source emitting motes at (length × 10)/s so density per metre is constant. The giver gets dark **(alpha)** streaks; the receiver bright rising streaks and glints.

## Fire and matter

**Burning (a building, a unit, a ground fire), looping.** Shipped and worth copying: `effect.fire.torch` and `effect.fire.ground` (`base/effects/fire/`, built by `tools/effects/showcase.py`): whole flames are `fire-loop` sprites started at random frames (`play: rate`, `start: random`), a sprite's centre 0.38 of its size above the ground, tinted near white; plus a hot heart, sparks, `fire-smoke`, a scorch, a ground glow and a flickering light layer.
- Flames: two flame-wisp flipbook layers, 6–7/s each, life 0.9–1.3 s, growing ×3–7 (0.5 → 6 m), rising (start 9 m/s, drag 1.5, +14 m/s² lift). One layer gets a brightness bump ×1 → 2.2 → 1 at 13 % of life. Hot ramp, soft.
- Dark body: an alpha copy of the flame flipbook in a darkened colour (saturation +1, value −0.2) behind the bright one: contrast makes the fire.
- Embers: 30/s, life 0.7–1.35 s, rising 12–18 m/s with drag 0.6, 0.45 m, hot ramp, brightness ×0.5–2, spin ±6.
- Smoke **(alpha)** (its own effect so it sits higher): 8/s, life 2.8–3.2 s, rising 7.5 m/s with drag 0.25, size 3.5 → 6 m, grey alpha 0 → 0.5 at 19 % → 0.13, soft 9 m.
- Light the ground: a plane glow under it, flickering (8 keys 1 / 0.6).
No layer uses a big painted fire texture: shape comes from flipbooks, energy from dots and streaks.

**Immolation (fire around a unit, persistent).** A ring of flame wisps from a cylinder of radius ≈ the unit's, `inner` 0.7, 15–25/s, life 0.5–0.8 s, rising with +6 m/s², random roll; embers 20/s spiralling up (orbit 1–2 rad/s); a ground glow ring; occasional brighter licks (random brightness ×1–4). Outro: streams stop, live flames finish.

**Pillar of flame (flame strike).** Warning: a rune on the ground pulsing ~1.7 Hz while rings contract 3 → 0.5 m with late alpha. Eruption: a column of flame wisps from a 2–3 m cylinder rising fast (20–40 m/s with drag 2), a burst flash, a ground ring, embers; then the burning ground loop at the radius with heat glow, ending with a slow fade.

**Blood and gore (alpha only, must darken, never glow).** Hit: 1 splat (0.45 s, 1.5 → 4.5 m), 3 droplet layers of blobs stretched along velocity (axis ×0.2–0.45), rates falling from 140/s to 0 over 0.5 s, 6–20 m/s away from the hit with gravity −40, darkening as they fly.

**Construction dust.** A trickle of tan dust smoke (7 + 18/s), debris 1.5–3/s, and at 4.5/s a random point on the footprint ring pops a mini burst (5 dust + 2 debris + 10 sparks and a flash).
