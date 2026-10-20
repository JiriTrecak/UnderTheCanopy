# Presentation, events and effects

Authority: `Presentation` and `Animations` in `crates/content/src/abilities/definition.rs`; `crates/content/src/effects/mod.rs` (effects, layers, `EffectBinding`, `EffectEvent`) and `effects/rules.rs`; `abilities/publication.rs` (every bound effect and asset exists); emission in `crates/sim/src/abilities/events.rs`; playback in `crates/scene/src/effects/` and `crates/render/src/effects.rs`, cooked by `crates/cook/src/effects.rs`. Presentation is cosmetic: it never applies damage, selects targets, picks chain hops or delays simulation events.

## The `presentation` object

Every ability carries its presentation inline:

```jsonc
"presentation": {
  "icon": "asset.example.fireball-icon",       // optional: an asset with an image
  "animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle", "channel": "idle"},
  "effects": [ /* effect bindings, unique ids */ ]
}
```

- `icon` is the command-card icon: an asset id whose folder has an image (`image.png`). A binding's `command.icon` overrides it per unit.
- `animations` name animation states of the caster's model (its `asset.json` `capabilities.animations`, e.g. `idle`, `attack`, `cast`, `charge`). A missing state falls back to `fallback`, then `idle`. `channel` (optional) loops during channel waves (else `prepare`). When prepare, release and recover name the same state it plays once, with its contact point aligned to the release tick.
- `effects` may be empty while visuals are still missing.

## Effect binding

| Field | Meaning |
|---|---|
| `id` | Unique in the presentation (≤64 chars). |
| `effect` | An effect id; it must exist (the loader reports `Unknown visual effect`). |
| `event` | Simulation event that starts it (table below). |
| `anchor` | `target`: placed at the event point and attached to the event target entity; `caster`: at the event origin (the caster when the event fired). A layer with `anchor: "source"` always uses the caster side. Travel layers (missile, beam, wavefront, rain paths) run from origin to point. |
| `lifetime` | `finite` (default): plays its duration once. `status`: only with `event: "statusApplied"` and `anchor: "target"` (checked); lasts exactly as long as the status (expiry, dispel, death, shield exhaustion, aura range loss). |
| `statusId` | Restrict a `statusApplied` binding to one status id when the ability applies several. |
| `durationFrom: "delivery"` | Replace layer durations with the event's `durationTicks` (projectile flight, status duration, channel length, wave interval, vision/summon/containment lifetime, revival delay). |
| `sizeFrom: "radius"` / `spreadFrom: "radius"` | Replace layer `size` / `spread` with the event radius (point targeting radius, persistent pulse radius, line width, vision radius). |

Finite bindings bound sustained or looping layers to the effect duration (or the delivery duration with `durationFrom`). Status bindings and aura statuses are owned by the status. Aura refreshes emit no events, but their statuses still drive `statusApplied` bindings.

## Events

Origin is the caster position at the event (or the stated point); point is the target position.

| Event | When | Point / payload |
|---|---|---|
| `accepted` | Cast accepted (preparation starts) | aim; radius for point aims |
| `released` | Release succeeded; also manual weapon casts at the shot | aim; `durationTicks` = channel length for channels |
| `cancelled` | Unreleased cast cancelled, charge obstructed | aim; `reason` |
| `finished` | Recovery ended, persistent instance ended, toggle off | aim/anchor; `reason` |
| `waveStarted` | Channel start and after each wave | aim; `durationTicks` until the next wave |
| `wave` | Each channel wave and each persistent pulse (not the release run) | aim/anchor; radius |
| `projectile` | Projectile, chain, line, charge launch; each swarm seeker launch | aim; `durationTicks` travel estimate; line radius = width |
| `impact` | Projectile arrival, each chain hop, charge contact, each swarm bite | hit unit; origin = launch point / previous hop / seeker position |
| `healed`, `damaged`, `drained`, `manaRestored` | Per recipient of that operation | recipient; `amount` actually applied |
| `statusApplied` | Per recipient that gained the status | recipient; `statusId`, `durationTicks`, `untilDeath` |
| `dispelled` | Per dispel recipient | recipient; `amount` = statuses removed |
| `summoned` | Per created group | group centre (corpse position for corpse summons); `amount`, `spawned` ids, duration |
| `resurrected` | Per raised unit | raised unit |
| `teleported` | Per moved unit | new position; origin = old position |
| `visionCreated` | Per vision sensor | sensor point; `durationTicks`, radius |
| `converted`, `contained`, `sacrificed` | Per successful recipient | recipient |
| `releasedContained`, `digested` | Each passenger release / digestion pulse | passenger |
| `revivalStarted`, `revived`, `revivalCancelled` | Hero return scheduled / completed / failed | hero; delay as `durationTicks` |
| `splitStarted`, `splitEnded` | Linked form begins / ends | original unit; `spawned` member ids |
| `returned` | A swarm seeker delivers its cargo home | caster |
| `death`, `kill`, `weaponRelease`, `interval` | A trigger of that event qualified and ran | death/kill: victim point; weaponRelease: victim; interval: holder |
| `criticalStrike`, `evaded`, `cleaved` | Weapon modifier outcome from this ability | victim; caster = attacker |
| `weaponEnhanced` | Melee release enhanced by an attack bonus | victim |
| `enhancedHit` | Enhanced weapon hit dealt damage | victim |
| `weaponOrdered`, `weaponOrderCancelled` | Manual weapon cast ordered / abandoned | target |

`weaponHit`, `damaged` and `meleeDamaged` triggers have no event of their own; bind the events of their operations (`damaged`, `statusApplied`, ...).

## Effect file

One file each, `effects/<free folders>/<name>.json`, reusable by any number of abilities:

```jsonc
{"id": "effect.example.burst", "name": "...", "description": "", "durationTicks": 40, "loop": false, "layers": [ /* unique ids */ ]}
```

`id` must start with `effect.`. `name` is trimmed and must not be empty; `description` defaults to `""`, `loop` to `false`. `durationTicks` (positive, to 12000) is the effect period; `loop` repeats it while an owner keeps it alive. Each layer has its own timing. Complete examples: `examples.md` §1.

### Layer fields

Required on every layer: `id` (unique in the effect), `shape`, `colour`, `accent` (`#rrggbb`), `durationTicks` (positive, to 400), `count` (positive, to 128), `size` (0.01–20), `height` (0–30, offset above the anchor; actor and flight heights are added by the runtime).

Optional: `name`, `enabled` (true), `sustain` (false; keep alive until the owner ends), `startTick` (0, delay), `anchor` (`target` default, or `source`), `offset {x,y,z}`, `blend` (`additive`|`normal`), `orientation` (`camera`|`ground`), `follow` (track the anchored actor; beams track both actors), `intensity`, `core` (0–1: the dense centre of each additive sprite burns white, so sparks read crisp with their colour at the fringe), `spread`, `length`, `distribution` (`point`|`disc`), `motion`, `attachment`, `texture`, `flipbook`, plus shape-specific fields below.

More optional layer fields (D76):
- `ramp`: colour keys `[{at, colour}]` (1–8, `at` 0–1 increasing) that colour the texture's **brightness** (0 black … 1 white), then multiply by `colour`. One grey sprite gives a white-hot heart, an orange body and a red rim. Needs a texture.
- `opacityOverLife`, `scaleOverLife`: curves `[{at, value}]` (1–8 keys, `at` 0–1 increasing; held before the first key and after the last) over the **layer's** life. They replace the default fade in and out and the shape's own growth: a mark that appears in 0.2 s, holds and fades over 20 s; a shock ring that pops out and slows. Not on particles or ribbons (their curves are per particle, in `emitter`/`ribbon`).
- `soft`: metres over which sprites fade where they meet the ground, units and buildings (0–10; default 0.4, ground decals 0 = a hard cut). Every effect is hidden behind surfaces.
- `rides`: particles and ribbons only: the id of a `missile` or `rain` layer of the same effect whose path this layer's emitter follows (its `offset` is relative to the ridden object; give it the ridden layer's timing). With `emitter.space: world` the particles stay behind: fire trails behind a falling anvil, an arrow or a bolt.

| Shape | Use | Rules |
|---|---|---|
| `ring` | Ground ring; grows over a finite life (fixed size when sustained/`durationFrom`); `orientation: camera` stands it up facing the eye | no texture |
| `glow` | Soft light pool on the ground; `orientation: camera` makes it an orb in the air (a glow at a socket otherwise reads as a floating disc) | no texture |
| `pillar` | Vertical column; `height` is its height | no texture |
| `burst` | Radial flash of `count` sprites | texture optional |
| `billboard` | Textured sprite; `orientation: ground` for ground glyphs | texture required |
| `streaks` | Textured streak sprites | texture required |
| `light` | Point light; `size` = reach; at most 8 effect lights render at once, shared with working buildings | no texture |
| `rain` | `count` shards falling from `height` onto a golden-angle disc of radius `spread` over `durationTicks ÷ fallSpeed` (with `durationFrom` they land as the delivery ends): `fallSpeed` 0.25–8, `launchDelayMs` 0–500 (each drop up to this late), `fallAngleDegrees` 0–70, `fallAzimuthDegrees`, `length` (shard length × size, 1.8), `impact {durationTicks, count, size, spread, height, colour, accent}` (a flash and splash fragments where each lands). A `model` instead of a texture drops the model itself (a falling crucible) | texture or model required; fall/impact fields only here |
| `missile` | Travels origin→point over its duration (use `durationFrom: delivery`); follows the authoritative delivery; `arcHeight`; may carry a `model` instead of a texture | `trail`, `arcHeight` |
| `beam` | Line between origin and point; `follow: true` tracks both actors (omit for fixed chain-hop flashes) | no texture, no attachment |
| `wavefront` | Travelling front for line deliveries | texture optional, `trail` |
| `particles` | Emitter-driven particles | `emitter` required (and only here) |
| `ribbon` | A textured strip along the path of the layer it `rides` (or of the unit it follows) | `ribbon` required (and only here); texture optional |
| `sound` | Positional audio | `sound` required (and only here) |
| `mesh` | A 3D model | `model` required |

Cross-field rules (`effects/rules.rs`): `trail` only on missile/wavefront; `flipbook` needs a texture; `model` only on mesh, missile or rain and never with `texture`; `arcHeight` only on missile; `attachment` not on rain/missile/beam/wavefront; `motion` only on ring/glow/billboard/light/mesh; textures only on burst/billboard/streaks/rain/missile/wavefront/particles; billboard/streaks need one, rain a texture or a model.

### Asset references

Layers name files of asset folders (`assets/…/<asset>/`) by `{asset, role, index}`: index 1 is the file `<role>.<ext>`, index 2 `<role>_2.<ext>`, and so on. The loader checks that each referenced file of every enabled layer of every bound effect exists (`Missing published effect …`). Indexes start at 1.

- `texture`: `{asset, role: "image", index}`. Texture colour multiplies `colour`; use white (`#ffffff`) to keep painted colours. Transparent sprites need real alpha and edge padding; opaque painted backgrounds suit command icons, not projected glyphs.
- `sound`: `{asset, role: "audio", index, volume 0–1, pitch 0.25–4, loop, fadeInTicks 0–160, fadeOutTicks 0–160, referenceDistance, maxDistance (> referenceDistance)}`.
- `model`: `{asset, role: "geometry", index, rotation {x,y,z} degrees, animation?: {clip, speed 0.1–4, loop}}`; the clip must exist in the model.

### Other sub-objects

- `emitter`: `{mode: burst|continuous, lifetimeTicks (positive), rate (≥1), speed, speedVariation 0–1, direction {x,y,z} each −1…1, coneDegrees 0–180, gravity −40…40 (pulls down; negative lifts steam and embers), drag 0–10, spawnRadius 0–20, startSize, endSize, spin, fadeIn 0–1, fadeOut 0–1, stretch?}`. All listed fields required; the rest are optional:
  - `stretch` (0–1 s) draws each particle along the last `stretch` seconds of its path: sparks become streaks along their velocity (ground-facing particles streak along the ground).
  - `space` (`local` default | `world`): world-space particles stay where they were born, so a moving emitter (a layer it `rides`, a unit it follows) leaves a trail. `inheritVelocity` (0–1) keeps a share of the emitter's velocity.
  - `coneInnerDegrees` (≤ `coneDegrees`): directions start this far from `direction`; up with 78–88 throws a flat ring outwards (a blast wave over the ground). `spawnInner` (0–1): born no nearer the centre than this share of `spawnRadius` (a ring).
  - Curves over each particle's life, `[{at, value}]` or for colour `[{at, colour}]`: `colourOverLife` (replaces colour → accent), `opacityOverLife` (0–1, replaces the fades), `sizeOverLife` (metres, replaces start → end), `additiveOverLife` (0–1: 1 adds light, 0 covers like `normal`; any layer with it sorts with the covering layers), `erosionOverLife` (0–1: texels whose alpha is below it vanish, so sprites break up from their thin edges; `erosionSoftness` 0–1, default 0.1), `heatOverLife` (multiplies the texture's brightness before the layer's `ramp`; falling heat cools fire from white to red).
  - `turbulence {strength (m), scale (m, positive), speed (1/s)}`: smooth swirling drift through a noise field.
  - `collide {friction 0–1}`: particles land on the ground under the anchor and slide on (0) or stop (1).
  - `orbit` (degrees a second, ±3600) turns each particle around the emitter's up axis as it lives; `pull` (−1…1) draws it in towards that axis over its life (1 reaches the axis at death, negative pushes out). Together they make vortices, drains and things converging: a ring spawn (`spawnInner`) with `orbit` and `pull` 1 swirls in to the centre.
- `ribbon`: `{durationTicks (positive; how long a point of the strip stays), width (m, positive), tile? (metres per texture repeat; absent: the texture stretches over the strip), scroll? (repeats per second), colourOverLife?, opacityOverLife? (default 1 → 0 to the tail), widthOverLife? (× width), additiveOverLife?}`, curves over the strip's age (0 at the head). The texture's u runs along the strip, v across. `blend` sets the default additivity.
- `motion`: `{rotation?: {periodTicks, direction: clockwise|counterclockwise, phaseDegrees?}, scale?: pulse, opacity?: pulse}`, pulse = `{periodTicks, min, max (≥ min; opacity ≤ 1), easing: sine|smoothstep|bounce, phase? 0–1}`. Periods positive. 320 ticks = 8 s.
- `attachment`: `{socket, fallback: origin|hide, inheritRotation}`. Socket names come from the model's `asset.json` (`capabilities.sockets`); never guess bone names. `follow: true` tracks the animated socket; false captures its first pose.
- `trail`: `{durationTicks (positive, to 80), maxPoints 2–64, width, colour, opacity 0–1, breakDistance}`. A soft ribbon behind a missile or wavefront, removed with its delivery.
- `flipbook`: `{columns, rows, firstFrame, frames, frameTicks, mode: once|loop|ping-pong, randomStart, overLife?, blend?}`; frames run row-major from the top-left, `firstFrame + frames ≤ columns × rows`; image size must divide by the grid. `once` holds the last frame; the layer's lifetime still owns disappearance. `overLife` spreads the frames over each particle's life (`frameTicks` unused); `blend` cross-fades each frame into the next. Still variants: `frames` 4 of a 2 × 2 atlas, a long `frameTicks`, `mode: once`, `randomStart: true` gives each particle one of four shapes.
- Model `rain` drops keep the model's authored `rotation` as they fall (no tumble); `intensity` below 1 makes them translucent. To leave the model on the ground for a moment, add a `mesh` layer with the same model on the landing event with an `opacityOverLife` fade.

The loader enforces the cross-field rules, positive durations, counts, indexes and motion periods, emitter rate ≥ 1, trail `maxPoints` ≥ 2, `maxDistance` > `referenceDistance`, flipbook fit, pulse `max` ≥ `min`, opacity ≤ 1 and `#rrggbb` colours. Other bounds on this page are authoring limits from the old editor; stay inside them. Effect rule errors are reported as `effects · <effect id>/layers/<layer id>: …` (not by file); parse errors name the file.

### The spell recipe (D76–D78)

Forgefall (`effects/beetles/forgemaster/forgefall-*.json`) is the reference. Every spell gets the same treatment, scaled to its weight (hero ultimates biggest, unit abilities smaller, passives and auras quietest):
1. **Wind-up** on `accepted` at the caster (already in place for every spell): keep it, sharpen it.
2. **Release or telegraph**: a flash at the caster and, for delayed areas, a stable high-contrast ground footprint (the gameplay edge is the clearest element; darken the ground under big threats).
3. **Travel**: anything that moves trails in world space (`rides`, `space: world`; `tools/content/vfx_layers.py`).
4. **Impact**, layered and fast (flash and light 1–2 frames, a body that pops to ~80 % in 3–4 frames then spreads slowly): a ramp-coloured body (`ramp`, `heatOverLife`, partly covering with `additiveOverLife` ≈ 0.6–0.7 so it reads on sand), a ground ring or decal with `scaleOverLife`, debris or drops that land (`collide`), sparkles with `turbulence`, and a cool-down into covering smoke or motes.
5. **Linger and status marks**: decals that appear, hold and fade (`opacityOverLife`), status loops that read as the spell's colour on the unit without hiding it.

**One idea per spell** (D78): before any layer, decide what the spell *does* and the one picture that says it (Holy Light: a symbol, a cone and a pulse; Resurrection: an angel and light; a mana drain: motes spiralling out into an orb), and write it in `docs/spell-visuals.md`. Generic pieces (dust, sparks, smoke, debris) only support the idea. A disc or ring is used only where the mechanic is a ring (a pulse, an aura's edge); a pale additive disc on sand is the most common failure, so check every spell for one. Colours that sit near the ground's (sand dust, mid-brown rock) vanish: dust is pale, rocks and clods are dark, and spells darken their ground first (`vl.shade`).

Rules of thumb: every layer earns its place; at the game camera a metre is about 10–13 px, so particles under 0.15 m and strips under 0.3 m vanish; strip and puff textures must be bright with the shape in alpha; soft particles are on by default (decals cut hard); the race palettes stay (ants: violet liquid magic, gold royalty, amber nectar, scent violet-blue; beetles: substance violet-blue, fire orange, steam white). Review each spell at the game camera on its bench (`ant-bench`, `beetle-bench`, `ranged-bench`) with `shot --play` casts, `--effect id,age`, `--flight` and `--sequence`, against the before. `python3 tools/effects/review.py <out> [--only <ability id text>]` renders a looping clip of every effect of every ability (with its icon, casters and description) for the spell review page (`tools/effects/review.html`, published beside the clips).

### Visual guidance

- Visual sustain/loop is not a spell lifetime. A finite binding bounds sustained layers to the effect duration; status bindings end with their status. For a finite particle area, stop emission early enough that particle tails finish when the area ends.
- Use fixed (non-following) beams for chain-hop flashes whose source is a previous recipient.
- Keep readable silhouettes and restrained bloom: a small purposeful stack beats noise. Use restrained volume for sustained sounds, especially auras on many units.
- Art should fit the miniature woodland/ant RTS: broad painterly shapes, clear at 32–48 px icons, natural materials.
- **Fire on bright ground** (D76): purely additive warm colours wash out to white on sunlit sand. Darken first (a `normal` shadow or scorch under the spell), keep the fire body partly covering (`additiveOverLife` about 0.6–0.7 while hot), colour a grey puff through a fire `ramp` with `heatOverLife` falling, and let it cool into dark covering smoke (`additiveOverLife` → 0, `colourOverLife` → dark grey, erosion rising). Lay out an impact as WC3 does: a 1–2 frame flash and light, a fast pop then a slow spread (`sizeOverLife`, `scaleOverLife`), a ground ring, debris that lands, then lingering smoke or marks. Keep it short; the gameplay footprint (the warning ring, the pool) stays the clearest element.
- **Ranged attacks** (D77): every shot trails something. A unit's `launchEffect` flies the projectile's path, so its trail layers `ride` a `missile` layer there (`arcHeight` equal to the projectile's; `intensity: 0` hides it, a textured glowing missile is a head). Physical shots leave a faint wake (`effect.weapons.thorn-flight`); magical shots a glowing head, a coloured tail (`ribbon`, `additiveOverLife` about 0.6) and sparkles; shells fire puffs, smoke and embers. Build them with `tools/content/vfx_layers.py` (`carrier`, `wake`, `motes`, `drips`, `smoke_trail`, `fire_trail`, `burst`, `flare`, `ring`, `puff`). Trail strips and puffs need bright textures (grey values near white, the shape in alpha): additive light on sand vanishes otherwise. Review flights with `shot --flight <id>,<age>,<metres>,<ticks>` and fights on the `ranged-bench` map.
- **Shared grey textures** (`asset.effects.vfx.*`, `tools/content/vfx_art.py`): `puff` (four cloud puffs, 2 × 2: fire, smoke, dust; alpha graded for erosion), `flame-strip` (flame tongues repeating along a strip: ribbons), `ash` (four flakes, 2 × 2), `chunk` (four debris lumps, 2 × 2), `blast-ring` (a ground shock ring smeared outwards), `mote` (a soft dot with a dense heart: sparkles, drops, glowing heads), `wake` (a soft band repeating along a strip: air wakes), `bolt` (lightning along a strip: beams), `vortex` (four spiral arms around an empty eye: storms and drains, turned with `motion.rotation`), `shell` (a force shell face on: a bright rim and a bulging hexagon lattice, camera-facing shields and domes). Forgefall (`effects/beetles/forgemaster/forgefall-*.json`, made by `tools/content/beetles/gen_effects.py`) is the worked example of every field above.

### Budgets

Models: ≤10,000 triangles each, 16 active instances, 80,000 active triangles, GLB ≤16 MB. Images: ≤2048 px per side, ≤8 MB, 512 px is normal; 64 MB resident. Audio: 24 voices. Unit weapon projectile effects (not spells) are restricted to 1–8 static model missile layers. Per frame: 2048 emitters, 16,384 particles (256 per layer), 4096 ribbon segments, 16,384 path samples (a sample per 25 ms of live births, at most 32 per moving world-space emitter; past that, clouds move with their emitter). Live cues: 256 for ability events, 768 more for weapon launches, trails, impacts and building states (`MAX_CUES`, `MAX_PLAY_CUES`).
