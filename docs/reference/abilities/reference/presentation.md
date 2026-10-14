# Presentation, events and effects

Authority: `presentationSchema` in `src/content/abilities/schema.ts`, `src/content/effects/schema.ts`, emission in `src/sim/abilities/runtime.ts`, playback in `src/render/abilities/`. Presentation is cosmetic: it never applies damage, selects targets, picks chain hops or delays simulation events.

## Presentation document

```jsonc
{
  "schemaVersion": 1,
  "id": "presentation.custom.name",
  "icon": "asset.custom.name-icon",          // optional published icon asset (catalog.icons)
  "animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle", "channel": "idle"},
  "effects": [ /* ≤32 effect bindings, unique ids */ ]
}
```

`animations` are clip names on the caster's model (`catalog.models[].animations`). A missing clip falls back to `fallback`, then `idle`. `channel` (optional) loops during channel waves (else `prepare`). When prepare, release and recover name the same clip it plays once, with its contact point aligned to the release tick.

## Effect binding

| Field | Meaning |
|---|---|
| `id` | 1–64 chars, unique in the presentation. |
| `effect` | Published effect ID. Publish effects before the spell that binds them. |
| `event` | Simulation event that starts it (table below). |
| `anchor` | `target`: placed at the event point and attached to the event target entity; `caster`: at the event origin (the caster when the event fired). A layer with `anchor: "source"` always uses the caster side. Travel layers (missile, beam, wavefront, rain paths) run from origin to point. |
| `lifetime` | `finite` (default): plays its duration once. `status`: only with `event: "statusApplied"` and `anchor: "target"`; lasts exactly as long as the status (expiry, dispel, death, shield exhaustion, aura range loss). |
| `statusId` | Restrict a `statusApplied` binding to one status ID when the ability applies several. |
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
| `summoned` | Per created group | group centre (corpse position for corpse summons); `amount`, `spawned` IDs, duration |
| `resurrected` | Per raised unit | raised unit |
| `teleported` | Per moved unit | new position; origin = old position |
| `visionCreated` | Per vision sensor | sensor point; `durationTicks`, radius |
| `converted`, `contained`, `sacrificed` | Per successful recipient | recipient |
| `releasedContained`, `digested` | Each passenger release / digestion pulse | passenger |
| `revivalStarted`, `revived`, `revivalCancelled` | Hero return scheduled / completed / failed | hero; delay as `durationTicks` |
| `splitStarted`, `splitEnded` | Linked form begins / ends | original unit; `spawned` member IDs |
| `returned` | A swarm seeker delivers its cargo home | caster |
| `death`, `kill`, `weaponRelease`, `interval` | A trigger of that event qualified and ran | death/kill: victim point; weaponRelease: victim; interval: holder |
| `criticalStrike`, `evaded`, `cleaved` | Weapon modifier outcome from this ability | victim; caster = attacker |
| `weaponEnhanced` | Melee release enhanced by an attack bonus | victim |
| `enhancedHit` | Enhanced weapon hit dealt damage | victim |
| `weaponOrdered`, `weaponOrderCancelled` | Manual weapon cast ordered / abandoned | target |

`weaponHit`, `damaged` and `meleeDamaged` triggers have no event of their own; bind the events of their operations (`damaged`, `statusApplied`, ...).

## Effect document

```jsonc
{"schemaVersion": 1, "id": "effect.custom.name.part", "name": "...", "description": "", "durationTicks": 40, "loop": false, "layers": [ /* ≤32, unique ids */ ]}
```

`durationTicks` 1–12000 is the effect period; `loop` repeats it while an owner keeps it alive. Each layer has its own timing. Effects are reusable across spells.

### Layer fields

Required on every layer: `id` (1–64), `shape`, `colour`, `accent` (`#rrggbb`), `durationTicks` 1–400, `count` 1–128, `size` 0.01–20, `height` 0–30 (offset above the anchor; actor and flight heights are added by the runtime).

Optional: `name`, `enabled` (true), `sustain` (false; keep alive until the owner ends), `startTick` 0–12000 (delay), `anchor` (`target`|`source`), `offset {x,y,z}` ±100, `blend` (`additive`|`normal`), `orientation` (`camera`|`ground`), `follow` (track the anchored actor; beams track both actors), `intensity` 0–500, `spread` 0–20, `length` 0.01–20, `distribution` (`point`|`disc`), `motion`, `attachment`, `texture`, `flipbook`, plus shape-specific fields below.

| Shape | Use | Rules |
|---|---|---|
| `ring` | Ground ring; grows over a finite life (fixed size when sustained/`durationFrom`) | no texture |
| `glow` | Soft light pool or orb | no texture |
| `pillar` | Vertical column; `height` is its height | no texture |
| `burst` | Radial flash of `count` sprites | texture optional |
| `billboard` | Textured sprite; `orientation: ground` for ground glyphs | texture required |
| `streaks` | Textured streak sprites | texture required |
| `light` | Point light; `size` = reach; at most 4 lights render at once | no texture |
| `rain` | Falling shards over `spread`: `fallSpeed` 0.25–8, `launchDelayMs` 0–500, `fallAngleDegrees` 0–70, `fallAzimuthDegrees`, `impact {durationTicks 1–80, count 1–12, size, spread, height, colour, accent}` | texture required; fall/impact fields only here |
| `missile` | Travels origin→point over its duration (use `durationFrom: delivery`); follows the authoritative delivery; `arcHeight` 0–20; may carry a `model` instead of a texture | `trail`, `arcHeight` |
| `beam` | Line between origin and point; `follow: true` tracks both actors (omit for fixed chain-hop flashes) | no texture, no attachment |
| `wavefront` | Travelling front for line deliveries | texture optional, `trail` |
| `particles` | Emitter-driven particles | `emitter` required |
| `sound` | Positional audio | `sound` required |
| `mesh` | Published 3D model | `model` required |

Cross-field rules: `trail` only on missile/wavefront; `flipbook` needs a texture; `model` only on mesh or missile and never with `texture`; `arcHeight` only on missile; `attachment` not on rain/missile/beam/wavefront; `motion` only on ring/glow/billboard/light/mesh; textures only on burst/billboard/streaks/rain/missile/wavefront/particles.

### Sub-objects

- `texture`: `{asset, role: "image", index}` (published image, `catalog.textures`). Texture colour multiplies `colour`; use white to keep painted colours.
- `emitter`: `{mode: burst|continuous, lifetimeTicks 1–400, rate 1–240, speed 0–40, speedVariation 0–1, direction {x,y,z} each −1…1, coneDegrees 0–180, gravity −40…40, drag 0–10, spawnRadius 0–20, startSize 0.01–20, endSize 0–20, spin −30…30, fadeIn 0–1, fadeOut 0–1}`.
- `motion`: `{rotation?: {periodTicks 1–2400, direction: clockwise|counterclockwise, phaseDegrees?}, scale?: pulse, opacity?: pulse}`, pulse = `{periodTicks, min 0–4, max 0–4 (opacity ≤1), easing: sine|smoothstep|bounce, phase? 0–1}`.
- `attachment`: `{socket, fallback: origin|hide, inheritRotation}`. Socket names per model from `catalog.sockets`; never guess bone names.
- `sound`: `{asset, role: "audio", index, volume 0–1, pitch 0.25–4, loop, fadeInTicks 0–160, fadeOutTicks 0–160, referenceDistance 0.1–100, maxDistance 1–256 (> reference)}` (`catalog.sounds`).
- `model`: `{asset, role: "geometry", index, rotation {x,y,z} degrees, animation?: {clip, speed 0.1–4, loop}}` (`catalog.models`; clip must exist in the GLB).
- `trail`: `{durationTicks 1–80, maxPoints 2–64, width 0.01–4, colour, opacity 0–1, breakDistance 0.1–100}`.
- `flipbook`: `{columns 1–8, rows 1–8, firstFrame 0–63, frames 1–64, frameTicks ≥1, mode: once|loop|ping-pong, randomStart}`; frames run row-major from the top-left and must fit the grid; image size must divide by the grid.

### Budgets

Models: ≤10,000 triangles each, 16 active instances, 80,000 active triangles, GLB ≤16 MB. Images: ≤2048 px per side, ≤8 MB, 512 px is normal; 64 MB resident. Audio: 24 voices. Unit weapon projectile effects (not spells) are restricted to 1–8 static model missile layers.
