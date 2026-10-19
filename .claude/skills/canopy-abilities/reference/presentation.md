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

| Shape | Use | Rules |
|---|---|---|
| `ring` | Ground ring; grows over a finite life (fixed size when sustained/`durationFrom`); `orientation: camera` stands it up facing the eye | no texture |
| `glow` | Soft light pool on the ground; `orientation: camera` makes it an orb in the air (a glow at a socket otherwise reads as a floating disc) | no texture |
| `pillar` | Vertical column; `height` is its height | no texture |
| `burst` | Radial flash of `count` sprites | texture optional |
| `billboard` | Textured sprite; `orientation: ground` for ground glyphs | texture required |
| `streaks` | Textured streak sprites | texture required |
| `light` | Point light; `size` = reach; at most 4 lights render at once | no texture |
| `rain` | `count` shards falling from `height` onto a golden-angle disc of radius `spread` over `durationTicks ÷ fallSpeed` (with `durationFrom` they land as the delivery ends): `fallSpeed` 0.25–8, `launchDelayMs` 0–500 (each drop up to this late), `fallAngleDegrees` 0–70, `fallAzimuthDegrees`, `length` (shard length × size, 1.8), `impact {durationTicks, count, size, spread, height, colour, accent}` (a flash and splash fragments where each lands). A `model` instead of a texture drops the model itself (a falling crucible) | texture or model required; fall/impact fields only here |
| `missile` | Travels origin→point over its duration (use `durationFrom: delivery`); follows the authoritative delivery; `arcHeight`; may carry a `model` instead of a texture | `trail`, `arcHeight` |
| `beam` | Line between origin and point; `follow: true` tracks both actors (omit for fixed chain-hop flashes) | no texture, no attachment |
| `wavefront` | Travelling front for line deliveries | texture optional, `trail` |
| `particles` | Emitter-driven particles | `emitter` required (and only here) |
| `sound` | Positional audio | `sound` required (and only here) |
| `mesh` | A 3D model | `model` required |

Cross-field rules (`effects/rules.rs`): `trail` only on missile/wavefront; `flipbook` needs a texture; `model` only on mesh, missile or rain and never with `texture`; `arcHeight` only on missile; `attachment` not on rain/missile/beam/wavefront; `motion` only on ring/glow/billboard/light/mesh; textures only on burst/billboard/streaks/rain/missile/wavefront/particles; billboard/streaks need one, rain a texture or a model.

### Asset references

Layers name files of asset folders (`assets/…/<asset>/`) by `{asset, role, index}`: index 1 is the file `<role>.<ext>`, index 2 `<role>_2.<ext>`, and so on. The loader checks that each referenced file of every enabled layer of every bound effect exists (`Missing published effect …`). Indexes start at 1.

- `texture`: `{asset, role: "image", index}`. Texture colour multiplies `colour`; use white (`#ffffff`) to keep painted colours. Transparent sprites need real alpha and edge padding; opaque painted backgrounds suit command icons, not projected glyphs.
- `sound`: `{asset, role: "audio", index, volume 0–1, pitch 0.25–4, loop, fadeInTicks 0–160, fadeOutTicks 0–160, referenceDistance, maxDistance (> referenceDistance)}`.
- `model`: `{asset, role: "geometry", index, rotation {x,y,z} degrees, animation?: {clip, speed 0.1–4, loop}}`; the clip must exist in the model.

### Other sub-objects

- `emitter`: `{mode: burst|continuous, lifetimeTicks (positive), rate (≥1), speed, speedVariation 0–1, direction {x,y,z} each −1…1, coneDegrees 0–180, gravity −40…40 (pulls down; negative lifts steam and embers), drag 0–10, spawnRadius 0–20, startSize, endSize, spin, fadeIn 0–1, fadeOut 0–1, stretch?}`. All fields but `stretch` required. `stretch` (0–1 s, optional) draws each particle along the last `stretch` seconds of its path: sparks become streaks along their velocity.
- `motion`: `{rotation?: {periodTicks, direction: clockwise|counterclockwise, phaseDegrees?}, scale?: pulse, opacity?: pulse}`, pulse = `{periodTicks, min, max (≥ min; opacity ≤ 1), easing: sine|smoothstep|bounce, phase? 0–1}`. Periods positive. 320 ticks = 8 s.
- `attachment`: `{socket, fallback: origin|hide, inheritRotation}`. Socket names come from the model's `asset.json` (`capabilities.sockets`); never guess bone names. `follow: true` tracks the animated socket; false captures its first pose.
- `trail`: `{durationTicks (positive, to 80), maxPoints 2–64, width, colour, opacity 0–1, breakDistance}`. A soft ribbon behind a missile or wavefront, removed with its delivery.
- `flipbook`: `{columns, rows, firstFrame, frames, frameTicks, mode: once|loop|ping-pong, randomStart}`; frames run row-major from the top-left, `firstFrame + frames ≤ columns × rows`; image size must divide by the grid. `once` holds the last frame; the layer's lifetime still owns disappearance.

The loader enforces the cross-field rules, positive durations, counts, indexes and motion periods, emitter rate ≥ 1, trail `maxPoints` ≥ 2, `maxDistance` > `referenceDistance`, flipbook fit, pulse `max` ≥ `min`, opacity ≤ 1 and `#rrggbb` colours. Other bounds on this page are authoring limits from the old editor; stay inside them. Effect rule errors are reported as `effects · <effect id>/layers/<layer id>: …` (not by file); parse errors name the file.

### Visual guidance

- Visual sustain/loop is not a spell lifetime. A finite binding bounds sustained layers to the effect duration; status bindings end with their status. For a finite particle area, stop emission early enough that particle tails finish when the area ends.
- Use fixed (non-following) beams for chain-hop flashes whose source is a previous recipient.
- Keep readable silhouettes and restrained bloom: a small purposeful stack beats noise. Use restrained volume for sustained sounds, especially auras on many units.
- Art should fit the miniature woodland/ant RTS: broad painterly shapes, clear at 32–48 px icons, natural materials.

### Budgets

Models: ≤10,000 triangles each, 16 active instances, 80,000 active triangles, GLB ≤16 MB. Images: ≤2048 px per side, ≤8 MB, 512 px is normal; 64 MB resident. Audio: 24 voices. Unit weapon projectile effects (not spells) are restricted to 1–8 static model missile layers.
