# Worked examples: core families

Each `json ability` block is a complete ability file and each `json effect` block a complete effect file; `crates/content/tests/skills.rs` parses them all, so they match the Rust types. Plain `json` blocks are fragments. Ids are examples (`ability.example.*`, `effect.example.*`, `unit.example.*`, `asset.*.example.*`): a real file needs ids that exist (units, assets, effects) or that you add. Example 1 shows every file involved; the others show the ability with a short inline presentation. More compositions are in `examples-advanced.md`.

Standard blocks used below:

- Active cast: `"cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {...}, "cooldown": {...}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]}`.
- Passive: self targeting with range 0 and a zero-cost, zero-tick cast block.

## 1. Complete package: conditional single target

Heals an injured ally, damages an enemy. Shows ranks, a targeting condition, a branch, and every file involved.

`abilities/example/sap-blessing.json`:

```json ability
{
  "id": "ability.example.sap-blessing", "name": "Sap Blessing",
  "description": "Heals an injured ally or scorches an enemy with hot sap.",
  "activation": "targeted",
  "ranks": [{"mana": 30, "range": 12, "heal": 120, "damage": 60}, {"mana": 35, "range": 12, "heal": 180, "damage": 90}],
  "targeting": {
    "kind": "unit", "relations": ["ally", "enemy"], "allowSelf": true, "alive": true, "visible": true,
    "range": {"rankParameter": "range"}, "outOfRange": "reject",
    "condition": {"kind": "any", "conditions": [
      {"kind": "relation", "of": "target", "to": "caster", "is": "enemy"},
      {"kind": "resource", "of": "target", "resource": "health", "measure": "missing", "comparison": "gt", "value": 0}
    ]}
  },
  "cast": {"prepareTicks": 12, "recoverTicks": 8,
    "cost": {"resource": "mana", "amount": {"rankParameter": "mana"}, "commit": "release"},
    "cooldown": {"ticks": 200, "start": "release", "scope": "ability"},
    "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "branch", "condition": {"kind": "relation", "of": "target", "to": "caster", "is": "ally"},
      "then": [{"op": "heal", "target": "target", "amount": {"rankParameter": "heal"}}],
      "else": [{"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"}]}
  ],
  "presentation": {
    "icon": "asset.icons.example.sap-blessing",
    "animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"},
    "effects": [
      {"id": "gather", "effect": "effect.example.sap-blessing.gather", "event": "accepted", "anchor": "caster"},
      {"id": "mend", "effect": "effect.example.sap-blessing.mend", "event": "healed", "anchor": "target"},
      {"id": "scorch", "effect": "effect.example.sap-blessing.scorch", "event": "damaged", "anchor": "target"}
    ]
  }
}
```

Its effects are separate files, `effects/example/sap-blessing-gather.json` and so on:

```json effect
{"id": "effect.example.sap-blessing.gather", "name": "Sap Blessing gather", "description": "Amber glow and rising motes at the caster during preparation.",
 "durationTicks": 20, "loop": false, "layers": [
  {"id": "glow", "shape": "glow", "colour": "#f0a530", "accent": "#ffe2a0", "durationTicks": 20, "count": 1, "size": 1.4, "height": 0.1, "follow": true},
  {"id": "motes", "shape": "particles", "colour": "#ffc04a", "accent": "#fff1c8", "durationTicks": 20, "count": 16, "size": 0.12, "height": 0.6, "blend": "additive", "follow": true,
   "emitter": {"mode": "continuous", "lifetimeTicks": 16, "rate": 30, "speed": 1.5, "speedVariation": 0.3, "direction": {"x": 0, "y": 1, "z": 0}, "coneDegrees": 25, "gravity": -1, "drag": 0.5, "spawnRadius": 0.6, "startSize": 0.14, "endSize": 0.02, "spin": 0, "fadeIn": 0.1, "fadeOut": 0.6}}
 ]}
```

```json effect
{"id": "effect.example.sap-blessing.mend", "name": "Sap Blessing mend", "description": "Warm burst and ground ring on the healed unit.",
 "durationTicks": 28, "loop": false, "layers": [
  {"id": "burst", "shape": "burst", "colour": "#ffd36b", "accent": "#fff6d8", "durationTicks": 20, "count": 10, "size": 1.2, "height": 1, "blend": "additive"},
  {"id": "ring", "shape": "ring", "colour": "#e8a33a", "accent": "#ffe6a8", "durationTicks": 28, "count": 1, "size": 1.6, "height": 0.05}
 ]}
```

A textured layer names an image of an asset folder, `{asset, role: "image", index}` (index 1 is `image.png`):

```json effect
{"id": "effect.example.sap-blessing.scorch", "name": "Sap Blessing scorch", "description": "Sap splash and a short flash on the damaged enemy.",
 "durationTicks": 24, "loop": false, "layers": [
  {"id": "splash", "shape": "billboard", "colour": "#ffffff", "accent": "#ffb347", "durationTicks": 24, "count": 1, "size": 1.8, "height": 0.08,
   "orientation": "ground", "texture": {"asset": "asset.effects.example.sap-splash", "role": "image", "index": 1}},
  {"id": "flash", "shape": "light", "colour": "#ff9a2e", "accent": "#ffd9a0", "durationTicks": 10, "count": 1, "size": 4, "height": 1, "intensity": 1.5}
 ]}
```

The caster's unit definition binds it (`units/…/<unit>.json`, field `behaviors.abilities`; a fragment):

```json
{"behaviors": {"abilities": {"maxMana": 200, "manaRegenPerSecond": 1, "bindings": [
  {"id": "sap-blessing", "ability": "ability.example.sap-blessing", "initialRank": 1, "controls": ["player", "ai", "scenario"],
   "command": {"hotkey": "Q", "column": 1}, "ai": {"preference": "wounded-ally", "intervalTicks": 20}}
]}}}
```

## 2. Area around the primary target

Primary hit, splash to others, slow on everyone in the area (negative ranked modifiers come from rank parameters).

```json ability
{
  "id": "ability.example.hoarfrost-burst", "name": "Hoarfrost Burst", "activation": "targeted",
  "ranks": [{"mana": 40, "range": 14, "slowMove": -300, "slowAttack": -200}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": {"rankParameter": "range"}, "outOfRange": "reject"},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": {"rankParameter": "mana"}, "commit": "release"}, "cooldown": {"ticks": 240, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "damage", "target": "target", "amount": 90, "damageType": "spell"},
    {"op": "damage", "target": "target", "query": {"center": "target", "radius": 5, "relations": ["enemy"], "excludePrimary": true}, "amount": 45, "damageType": "spell"},
    {"op": "status", "target": "target", "query": {"center": "target", "radius": 5, "relations": ["enemy"]}, "id": "chilled", "amount": 160, "heroDuration": 80,
     "polarity": "negative", "dispel": true, "rankedModifiers": {"moveSpeedPermille": {"rankParameter": "slowMove"}, "attackSpeedPermille": {"rankParameter": "slowAttack"}}}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "blast", "effect": "effect.example.frost-blast", "event": "released", "anchor": "target"},
    {"id": "chill", "effect": "effect.example.frost-chill", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "chilled"}
  ]}
}
```

## 3. Ground-targeted instant area

Point aims require `targeting.radius`; `target: "target"` operations hit the default area (id order, `maxTargets`). Bind `released` with `sizeFrom: "radius"` for the area marker and `damaged`/`statusApplied` per victim.

```json ability
{
  "id": "ability.example.seed-bomb", "name": "Seed Bomb", "activation": "targeted",
  "ranks": [{"radius": 4, "damage": 70}, {"radius": 5, "damage": 110}],
  "targeting": {"kind": "point", "radius": {"rankParameter": "radius"}, "maxTargets": 12, "includeBuildings": true, "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 18, "outOfRange": "reject"},
  "cast": {"prepareTicks": 14, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 60, "commit": "release"}, "cooldown": {"ticks": 400, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "siege"},
    {"op": "status", "target": "target", "filter": {"heroes": false}, "id": "dazed", "amount": 60, "polarity": "negative", "dispel": false, "stun": true}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "blast", "effect": "effect.example.seed-blast", "event": "released", "anchor": "target", "sizeFrom": "radius"},
    {"id": "hit", "effect": "effect.example.seed-hit", "event": "damaged", "anchor": "target"}
  ]}
}
```

## 4. Self-centred area (stomp)

```json ability
{
  "id": "ability.example.earth-stomp", "name": "Earth Stomp", "activation": "targeted",
  "ranks": [{"damage": 50, "stun": 80, "heroStun": 40}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 50, "commit": "release"}, "cooldown": {"ticks": 320, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 5, "relations": ["enemy"]}, "filter": {"locomotion": ["ground"]}, "amount": {"rankParameter": "damage"}, "damageType": "spell"},
    {"op": "status", "target": "target", "query": {"center": "caster", "radius": 5, "relations": ["enemy"]}, "filter": {"locomotion": ["ground"]}, "id": "stunned",
     "amount": {"rankParameter": "stun"}, "heroDuration": {"rankParameter": "heroStun"}, "polarity": "negative", "dispel": false, "stun": true}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "quake", "effect": "effect.example.stomp-ring", "event": "released", "anchor": "caster"},
    {"id": "dizzy", "effect": "effect.example.stunned", "event": "statusApplied", "anchor": "target", "lifetime": "status"}
  ]}
}
```

## 5. Channelled ground area

`onRelease` runs once per wave at the fixed point; victims are reselected each wave. Bind `waveStarted` (`durationFrom: "delivery"`, `spreadFrom: "radius"`) for falling visuals and `wave` for impact sound.

```json ability
{
  "id": "ability.example.hailstorm", "name": "Hailstorm", "activation": "targeted",
  "ranks": [{"mana": 75, "damage": 25, "waves": 6}, {"mana": 90, "damage": 35, "waves": 8}],
  "targeting": {"kind": "point", "radius": 5, "maxTargets": 64, "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 18, "outOfRange": "reject"},
  "cast": {"prepareTicks": 12, "recoverTicks": 8,
    "channel": {"waves": {"rankParameter": "waves"}, "intervalTicks": 40},
    "cost": {"resource": "mana", "amount": {"rankParameter": "mana"}, "commit": "release"}, "cooldown": {"ticks": 480, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle", "channel": "attack"}, "effects": [
    {"id": "hail", "effect": "effect.example.hail-rain", "event": "waveStarted", "anchor": "target", "durationFrom": "delivery", "spreadFrom": "radius"},
    {"id": "clatter", "effect": "effect.example.hail-sound", "event": "wave", "anchor": "target"}
  ]}
}
```

## 6. Channelled unit link (drain with tether)

Ends if the target dies, hides, becomes ineligible or leaves `tetherRange`. Bind a `beam` layer with `follow: true` to `wave` or `drained`.

```json ability
{
  "id": "ability.example.sap-siphon", "name": "Sap Siphon", "activation": "targeted",
  "ranks": [{"drain": 25}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 12, "outOfRange": "reject", "filter": {"natures": ["organic"]}},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "channel": {"waves": 8, "intervalTicks": 40, "tetherRange": 14},
    "cost": {"resource": "mana", "amount": 40, "commit": "release"}, "cooldown": {"ticks": 320, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "drain", "target": "target", "amount": {"rankParameter": "drain"}, "resource": "health", "restoreCaster": true, "damageType": "spell"}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "tether", "effect": "effect.example.siphon-beam", "event": "drained", "anchor": "target"}
  ]}
}
```

## 7. Homing projectile

`onRelease` runs on impact. Bind `projectile` (missile layer, `durationFrom: "delivery"`) and `impact`.

```json ability
{
  "id": "ability.example.pebble-bolt", "name": "Pebble Bolt", "activation": "targeted",
  "delivery": {"kind": "projectile", "speed": 20},
  "ranks": [{"damage": 90, "stun": 80, "heroStun": 40}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 16, "outOfRange": "reject"},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 45, "commit": "release"}, "cooldown": {"ticks": 360, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"},
    {"op": "status", "target": "target", "id": "stunned", "amount": {"rankParameter": "stun"}, "heroDuration": {"rankParameter": "heroStun"}, "polarity": "negative", "dispel": false, "stun": true}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "flight", "effect": "effect.example.pebble-flight", "event": "projectile", "anchor": "target", "durationFrom": "delivery"},
    {"id": "hit", "effect": "effect.example.pebble-hit", "event": "impact", "anchor": "target"}
  ]}
}
```

A missile effect for it, carrying a model and a trail:

```json effect
{"id": "effect.example.pebble-flight", "name": "Pebble flight", "durationTicks": 40, "loop": false, "layers": [
  {"id": "pebble", "shape": "missile", "colour": "#ffffff", "accent": "#ffffff", "durationTicks": 40, "count": 1, "size": 1.2, "height": 1, "arcHeight": 0.8,
   "model": {"asset": "asset.models.example.pebble", "role": "geometry", "index": 1, "rotation": {"x": 0, "y": 0, "z": 0}},
   "trail": {"durationTicks": 14, "maxPoints": 20, "width": 0.18, "colour": "#c9b08a", "opacity": 0.6, "breakDistance": 8}}
]}
```

## 8. Chain (healing that skips healthy allies, plus a ward)

Retention scales heal/damage/drain/mana per hop, not status durations. Bind `impact` with a fixed (non-following) beam: each hop's origin is the previous recipient.

```json ability
{
  "id": "ability.example.dew-chain", "name": "Dew Chain", "activation": "targeted",
  "delivery": {"kind": "chain", "bounces": {"rankParameter": "hits"}, "radius": 8, "intervalTicks": 6, "retentionPermille": 800, "skipFullHealth": true},
  "ranks": [{"hits": 3, "heal": 100, "armor": 2}, {"hits": 4, "heal": 130, "armor": 3}],
  "targeting": {"kind": "unit", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 16, "outOfRange": "reject",
    "condition": {"kind": "resource", "of": "target", "resource": "health", "measure": "missing", "comparison": "gt", "value": 0}},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 50, "commit": "release"}, "cooldown": {"ticks": 280, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "heal", "target": "target", "amount": {"rankParameter": "heal"}},
    {"op": "status", "target": "target", "id": "dewWard", "amount": 120, "polarity": "positive", "dispel": true, "rankedModifiers": {"armor": {"rankParameter": "armor"}}}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "hop", "effect": "effect.example.dew-hop", "event": "impact", "anchor": "target"},
    {"id": "ward", "effect": "effect.example.dew-ward", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "dewWard"}
  ]}
}
```

A fixed hop flash (a beam without `follow`):

```json effect
{"id": "effect.example.dew-hop", "name": "Dew hop", "durationTicks": 12, "layers": [
  {"id": "arc", "shape": "beam", "colour": "#7fd6ff", "accent": "#e8f8ff", "durationTicks": 12, "count": 1, "size": 0.25, "height": 1, "blend": "additive"},
  {"id": "splash", "shape": "burst", "colour": "#9fe3ff", "accent": "#ffffff", "durationTicks": 10, "count": 8, "size": 0.8, "height": 1}
]}
```

## 9. Line wave

Point aim (no `targeting.radius` needed: the line uses `width`). Each crossed unit is hit once; there is no `impact` event, so bind `projectile` (wavefront, `durationFrom: "delivery"`) and per-victim `damaged`.

```json ability
{
  "id": "ability.example.burrow-wave", "name": "Burrow Wave", "activation": "targeted",
  "delivery": {"kind": "line", "speed": 24, "length": 20, "width": 1.6},
  "ranks": [{"damage": 75, "slow": -350}],
  "targeting": {"kind": "point", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 20, "outOfRange": "reject", "filter": {"locomotion": ["ground"]}},
  "cast": {"prepareTicks": 12, "recoverTicks": 10, "cost": {"resource": "mana", "amount": 60, "commit": "release"}, "cooldown": {"ticks": 360, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"},
    {"op": "status", "target": "target", "id": "churned", "amount": 80, "polarity": "negative", "dispel": true, "rankedModifiers": {"moveSpeedPermille": {"rankParameter": "slow"}}}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "front", "effect": "effect.example.burrow-front", "event": "projectile", "anchor": "caster", "durationFrom": "delivery", "spreadFrom": "radius"},
    {"id": "hit", "effect": "effect.example.burrow-hit", "event": "damaged", "anchor": "target"}
  ]}
}
```

## 10. Passive aura with a source marker

Recipient statuses use amount 1 (rebuilt every tick). The caster-targeted marker runs once per tick on the holder so its glyph can be bound separately. Bind both with `event: "statusApplied"`, `lifetime: "status"`, `anchor: "target"`, and `statusId`. An enemy-debuff aura uses `relations: ["enemy"]`, `allowSelf: false`.

```json ability
{
  "id": "ability.example.thornward-aura", "name": "Thornward Aura", "activation": "passive",
  "aura": {"radius": 10, "meleeOnly": false},
  "ranks": [{"reflect": 150}, {"reflect": 250}, {"reflect": 350}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 0, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "status", "target": "target", "id": "thornward", "amount": 1, "polarity": "positive", "dispel": false, "rankedModifiers": {"meleeReflectionPermille": {"rankParameter": "reflect"}}},
    {"op": "status", "target": "caster", "id": "thornwardSource", "amount": 1, "polarity": "positive", "dispel": false}
  ],
  "presentation": {"animations": {"prepare": "idle", "release": "idle", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "source", "effect": "effect.example.thornward-glyph", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "thornwardSource"},
    {"id": "member", "effect": "effect.example.thornward-motes", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "thornward"}
  ]}
}
```

A looping ground glyph for the source (sustained layers, rotation and a pulse):

```json effect
{"id": "effect.example.thornward-glyph", "name": "Thornward glyph", "durationTicks": 320, "loop": true, "layers": [
  {"id": "pool", "shape": "glow", "sustain": true, "colour": "#6fa64a", "accent": "#d6f0a8", "durationTicks": 320, "count": 1, "size": 2.6, "height": 0.08, "follow": true, "intensity": 0.25},
  {"id": "glyph", "shape": "billboard", "sustain": true, "colour": "#ffffff", "accent": "#d6f0a8", "durationTicks": 320, "count": 1, "size": 6, "height": 0.12,
   "orientation": "ground", "follow": true, "texture": {"asset": "asset.effects.example.thorn-glyph", "role": "image", "index": 1},
   "motion": {"rotation": {"periodTicks": 320, "direction": "clockwise"}, "opacity": {"periodTicks": 80, "min": 0.6, "max": 1, "easing": "sine"}}}
]}
```

## 11. Persistent ground zone

Pulses at release and every interval at the fixed point; the caster is free meanwhile. Short statuses refreshed by each pulse act as "while inside". For the zone visual bind `released` to an effect whose `durationTicks` equals the zone duration with `sustain: true` layers, and `wave` for per-pulse flashes.

```json ability
{
  "id": "ability.example.spore-field", "name": "Spore Field", "activation": "targeted",
  "persistent": {"durationTicks": 240, "intervalTicks": 40, "anchor": "point", "endsWithCaster": false},
  "ranks": [{"damage": 8}],
  "targeting": {"kind": "point", "radius": 4, "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 18, "outOfRange": "reject"},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 65, "commit": "release"}, "cooldown": {"ticks": 600, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"},
    {"op": "status", "target": "target", "id": "sporeChoke", "amount": 50, "polarity": "negative", "dispel": true, "modifiers": {"moveSpeedPermille": -350}}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "zone", "effect": "effect.example.spore-zone", "event": "released", "anchor": "target", "sizeFrom": "radius"},
    {"id": "puff", "effect": "effect.example.spore-puff", "event": "wave", "anchor": "target", "spreadFrom": "radius"}
  ]}
}
```

## 12. Toggle aura with upkeep

A toggle is a caster-anchored self persistent; recasting turns it off. The `instance` status exists exactly while the toggle is on (bind it with `lifetime: "status"` for the visual) and must outlast the pulse interval.

```json ability
{
  "id": "ability.example.ember-mantle", "name": "Ember Mantle", "activation": "targeted",
  "persistent": {"durationTicks": 144000, "intervalTicks": 40, "anchor": "caster", "endsWithCaster": true, "upkeepMana": 4, "toggle": true},
  "ranks": [{"damage": 10}, {"damage": 16}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 4, "recoverTicks": 4, "cost": {"resource": "mana", "amount": 10, "commit": "release"}, "cooldown": {"ticks": 40, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 4, "relations": ["enemy"]}, "amount": {"rankParameter": "damage"}, "damageType": "spell"},
    {"op": "status", "target": "caster", "id": "mantle", "amount": 80, "polarity": "positive", "dispel": false, "lifetime": "instance", "modifiers": {"armor": 2}}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "mantle", "effect": "effect.example.ember-mantle", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "mantle"}
  ]}
}
```

## 13. Buff with autocast

Autocast fires only if the unit binding includes `ai` control. AI scores positive statuses on allies that lack them.

```json ability
{
  "id": "ability.example.frenzy-pheromone", "name": "Frenzy Pheromone", "activation": "targeted",
  "autocast": {"intervalTicks": 12, "enabledByDefault": false},
  "ranks": [{"haste": 300, "speed": 200}],
  "targeting": {"kind": "unit", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 14, "outOfRange": "reject"},
  "cast": {"prepareTicks": 8, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 40, "commit": "release"}, "cooldown": {"ticks": 40, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "status", "target": "target", "id": "frenzy", "amount": 1200, "polarity": "positive", "dispel": true,
    "rankedModifiers": {"attackSpeedPermille": {"rankParameter": "haste"}, "moveSpeedPermille": {"rankParameter": "speed"}}}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "frenzy", "effect": "effect.example.frenzy", "event": "statusApplied", "anchor": "target", "lifetime": "status"}
  ]}
}
```

## 14. Debuffs: root with damage over time, sleep, and a breakable shield

```json ability
{
  "id": "ability.example.vine-snare", "name": "Vine Snare", "activation": "targeted",
  "ranks": [{"damage": 12, "duration": 240, "heroDuration": 80}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 16, "outOfRange": "reject", "filter": {"locomotion": ["ground"]}},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 45, "commit": "release"}, "cooldown": {"ticks": 320, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "status", "target": "target", "id": "snared", "amount": {"rankParameter": "duration"}, "heroDuration": {"rankParameter": "heroDuration"},
    "polarity": "negative", "dispel": true, "modifiers": {"rooted": true}, "disarm": true,
    "periodic": {"intervalTicks": 40, "damage": {"rankParameter": "damage"}, "damageType": "spell"}}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "vines", "effect": "effect.example.vines", "event": "statusApplied", "anchor": "target", "lifetime": "status"}
  ]}
}
```

Sleep: stunned until damaged (an operation):

```json
{"op": "status", "target": "target", "id": "drowse", "amount": 400, "heroDuration": 160, "polarity": "negative", "dispel": true, "stun": true, "breakOnDamage": true}
```

Absorb shield that ends the whole status when the pool is spent:

```json
{"op": "status", "target": "target", "id": "amberShell", "amount": 400, "polarity": "positive", "dispel": true, "shield": 150, "onShieldDepleted": "remove"}
```

## 15. Stacking weapon mark (passive)

A mark-only attack bonus (amount 0) applies a source-scoped stacking template on each damaging melee hit; each stack adds `sourceAttackBonus` to this attacker's later hits and deepens the slow.

```json ability
{
  "id": "ability.example.pressure-strikes", "name": "Pressure Strikes", "activation": "passive",
  "combatModifiers": {"attackBonus": {"status": "pressure", "amount": 0, "manaCost": 0, "weapon": "melee", "blockedBySilence": false}},
  "ranks": [{"bonus": 5, "slow": -50}, {"bonus": 10, "slow": -80}],
  "statuses": [{"op": "status", "target": "target", "id": "pressure", "amount": 240, "polarity": "negative", "dispel": true,
    "stacking": {"max": 3, "scope": "source"}, "sourceAttackBonus": {"rankParameter": "bonus"}, "rankedModifiers": {"moveSpeedPermille": {"rankParameter": "slow"}}}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 0, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [],
  "presentation": {"animations": {"prepare": "idle", "release": "idle", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "mark", "effect": "effect.example.pressure-mark", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "pressure"}
  ]}
}
```

## 16. Summons

Timed group that replaces the previous group (`summoned` event per group):

```json ability
{
  "id": "ability.example.call-spirits", "name": "Call Spirits", "activation": "targeted",
  "ranks": [{"count": 2}, {"count": 3}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 12, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 75, "commit": "release"}, "cooldown": {"ticks": 1200, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "summon", "target": "caster", "amount": {"rankParameter": "count"}, "definition": "unit.example.dew-sprite", "durationTicks": 2400, "replace": true, "radius": 3}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "arrive", "effect": "effect.example.spirit-arrival", "event": "summoned", "anchor": "target", "spreadFrom": "radius"}
  ]}
}
```

Corpse-fed summon at a point (one group per consumed visible corpse; AI samples corpse positions):

```json ability
{
  "id": "ability.example.husk-raise", "name": "Husk Raise", "activation": "targeted",
  "autocast": {"intervalTicks": 20, "enabledByDefault": false},
  "ranks": [{}],
  "targeting": {"kind": "point", "radius": 3, "relations": ["ally", "enemy"], "allowSelf": true, "alive": true, "visible": true, "range": 14, "outOfRange": "reject"},
  "cast": {"prepareTicks": 12, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 75, "commit": "release"}, "cooldown": {"ticks": 320, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "summon", "target": "point", "amount": 2, "definition": "unit.example.tadpole-spirit", "durationTicks": 1600, "replace": false, "radius": 2,
    "corpses": {"radius": 3, "relations": ["ally", "enemy", "neutral"], "order": "nearest", "maxTargets": 1}}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "rise", "effect": "effect.example.husk-rise", "event": "summoned", "anchor": "target"}
  ]}
}
```

Factory composition: the summoned producer receives a template whose `interval` trigger produces capped, source-linked children; each child carries a death-burst trigger through its own grant. Triggers follow whoever holds the status. An immobile producer needs a unit definition without movement (skill canopy-definitions).

```json ability
{
  "id": "ability.example.brood-mother", "name": "Brood Mother", "activation": "targeted",
  "ranks": [{"period": 200, "burst": 40}, {"period": 160, "burst": 60}],
  "statuses": [
    {"op": "status", "target": "target", "id": "producing", "amount": 0, "polarity": "positive", "dispel": false, "lifetime": "untilDeath"},
    {"op": "status", "target": "target", "id": "volatile", "amount": 0, "polarity": "positive", "dispel": false, "lifetime": "untilDeath"}
  ],
  "targeting": {"kind": "point", "radius": 2, "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 12, "outOfRange": "reject"},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 125, "commit": "release"}, "cooldown": {"ticks": 1400, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "summon", "target": "point", "amount": 1, "definition": "unit.example.nest-mother", "durationTicks": 1600, "maxActive": 1, "replace": true, "radius": 2, "grants": ["producing"]}],
  "triggers": [
    {"id": "produce", "event": "interval", "intervalTicks": {"rankParameter": "period"}, "whileStatus": "producing",
     "operations": [{"op": "summon", "target": "point", "amount": 1, "definition": "unit.example.webling", "durationTicks": 1200, "endsWithCaster": true, "maxActive": 6, "replace": false, "radius": 4, "grants": ["volatile"]}]},
    {"id": "burst", "event": "death", "whileStatus": "volatile",
     "operations": [{"op": "damage", "target": "target", "query": {"center": "caster", "radius": 4, "relations": ["enemy"], "maxTargets": 32}, "amount": {"rankParameter": "burst"}, "damageType": "spell"}]}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "nest", "effect": "effect.example.nest-arrival", "event": "summoned", "anchor": "target"},
    {"id": "pop", "effect": "effect.example.webling-pop", "event": "death", "anchor": "target"}
  ]}
}
```

## 17. Transformations

Self form: new model and weapon profile, stat boost, then an explicit heal (raised max health does not heal by itself).

```json ability
{
  "id": "ability.example.hunter-form", "name": "Hunter Form", "activation": "targeted",
  "ranks": [{"duration": 800, "health": 150, "damage": 15}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 12, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 75, "commit": "release"}, "cooldown": {"ticks": 1600, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "status", "target": "caster", "id": "form", "amount": {"rankParameter": "duration"}, "polarity": "positive", "dispel": false,
     "form": {"asset": "asset.models.example.hunter", "combatProfile": "unit.example.archer", "scale": 1.3},
     "rankedModifiers": {"maxHp": {"rankParameter": "health"}, "damage": {"rankParameter": "damage"}}},
    {"op": "heal", "target": "caster", "amount": {"rankParameter": "health"}}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "change", "effect": "effect.example.form-change", "event": "statusApplied", "anchor": "target"}
  ]}
}
```

Hostile polymorph (one operation):

```json
{"op": "status", "target": "target", "id": "frogged", "amount": 320, "heroDuration": 120, "polarity": "negative", "dispel": true,
 "form": {"asset": "asset.models.example.tree-frog", "scale": 0.65}, "modifiers": {"moveSpeedPermille": -700}, "disarm": true, "silence": true, "itemBlocked": true}
```

Flight toggle: an instance-lifetime air form; switching off lands the unit (or keeps it airborne until clear ground exists). Grounding an enemy flyer is a status with `"form": {"movement": {"locomotion": "ground"}}` plus a root.

```json ability
{
  "id": "ability.example.wing-form", "name": "Wing Form", "activation": "targeted",
  "persistent": {"durationTicks": 144000, "intervalTicks": 40, "anchor": "caster", "endsWithCaster": true, "toggle": true},
  "ranks": [{}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 25, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "status", "target": "caster", "id": "soaring", "amount": 80, "polarity": "positive", "dispel": true, "lifetime": "instance",
    "form": {"asset": "asset.models.example.needle-wasp", "movement": {"locomotion": "air", "flightHeight": 7}}}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "takeoff", "effect": "effect.example.wing-gust", "event": "statusApplied", "anchor": "target"}
  ]}
}
```

## 18. Movement

Blink to a point (bind `teleported`: origin is the departure, point the arrival). Pair with `"ai": {"intent": "escape", ...}`.

```json ability
{
  "id": "ability.example.shadow-step", "name": "Shadow Step", "activation": "targeted",
  "ranks": [{"range": 20}],
  "targeting": {"kind": "point", "radius": 1, "relations": ["ally"], "allowSelf": false, "alive": true, "visible": true, "range": {"rankParameter": "range"}, "outOfRange": "reject"},
  "cast": {"prepareTicks": 4, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 25, "commit": "release"}, "cooldown": {"ticks": 200, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "teleport", "target": "caster", "amount": 3, "destination": "point", "preserveOffset": false},
    {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 3, "relations": ["enemy"]}, "amount": 40, "damageType": "spell"}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "depart", "effect": "effect.example.shadow-puff", "event": "teleported", "anchor": "caster"},
    {"id": "arrive", "effect": "effect.example.shadow-burst", "event": "teleported", "anchor": "target"}
  ]}
}
```

Group transport to an allied unit, keeping formation (pair with `"intent": "reinforce"`):

```json ability
{
  "id": "ability.example.colony-recall", "name": "Colony Recall", "activation": "targeted",
  "ranks": [{}],
  "targeting": {"kind": "unit", "relations": ["ally"], "allowSelf": false, "alive": true, "visible": true, "range": 128, "outOfRange": "reject"},
  "cast": {"prepareTicks": 120, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 100, "commit": "release"}, "cooldown": {"ticks": 2400, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "teleport", "target": "target", "query": {"center": "caster", "radius": 6, "relations": ["ally"], "allowSelf": true, "excludePrimary": true, "maxTargets": 12},
    "amount": 8, "destination": "target", "preserveOffset": true}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "gather", "effect": "effect.example.recall-gather", "event": "accepted", "anchor": "caster"},
    {"id": "arrive", "effect": "effect.example.recall-arrive", "event": "teleported", "anchor": "target"}
  ]}
}
```

Charge: the caster physically runs to a grounded target; `onRelease` on contact. Bind `projectile`/`impact` for travel and contact, `cancelled` for obstruction.

```json ability
{
  "id": "ability.example.mandible-rush", "name": "Mandible Rush", "activation": "targeted",
  "delivery": {"kind": "charge", "speed": 32, "maxTicks": 80},
  "ranks": [{"damage": 70}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 20, "outOfRange": "reject", "filter": {"locomotion": ["ground"]}},
  "cast": {"prepareTicks": 8, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 55, "commit": "release"}, "cooldown": {"ticks": 480, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "melee"},
    {"op": "status", "target": "target", "id": "impact", "amount": 30, "polarity": "negative", "dispel": true, "stun": true}
  ],
  "presentation": {"animations": {"prepare": "charge", "release": "charge", "recover": "attack", "fallback": "idle"}, "effects": [
    {"id": "dash", "effect": "effect.example.rush-trail", "event": "projectile", "anchor": "caster", "durationFrom": "delivery"},
    {"id": "contact", "effect": "effect.example.rush-impact", "event": "impact", "anchor": "target"}
  ]}
}
```

## 19. Passive reactions

Learned passives need no `whileStatus`. Several triggers can share one ability. Here: on-hit stun chance, every-third-attack haste, retaliation when damaged (`target` is the attacker; the cooldown is per holder), restoration on kills, a burst on death, and weapon modifiers.

```json ability
{
  "id": "ability.example.veteran-instincts", "name": "Veteran Instincts", "activation": "passive",
  "combatModifiers": {"critical": {"chancePermille": 150, "multiplierPermille": {"rankParameter": "crit"}}, "evasionPermille": 100,
    "cleave": {"damagePermille": 300, "radius": 3, "arcDegrees": 120, "maxTargets": 4}},
  "ranks": [{"crit": 1750, "haste": 150, "heal": 30}, {"crit": 2000, "haste": 250, "heal": 45}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 0, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [],
  "triggers": [
    {"id": "bash", "event": "weaponHit", "chancePermille": 200, "operations": [
      {"op": "damage", "target": "target", "amount": 20, "damageType": "spell"},
      {"op": "status", "target": "target", "id": "bashed", "amount": 40, "polarity": "negative", "dispel": false, "stun": true}]},
    {"id": "rhythm", "event": "weaponRelease", "every": 3, "blockedBySilence": false, "operations": [
      {"op": "status", "target": "caster", "id": "momentum", "amount": 160, "polarity": "positive", "dispel": true, "rankedModifiers": {"attackSpeedPermille": {"rankParameter": "haste"}}}]},
    {"id": "retaliate", "event": "meleeDamaged", "cooldownTicks": 80, "operations": [
      {"op": "damage", "target": "target", "amount": 15, "damageType": "spell"}]},
    {"id": "feast", "event": "kill", "filter": {"summoned": false}, "operations": [
      {"op": "heal", "target": "caster", "amount": {"rankParameter": "heal"}}]},
    {"id": "lastStand", "event": "death", "operations": [
      {"op": "damage", "target": "target", "query": {"center": "caster", "radius": 5, "relations": ["enemy"], "maxTargets": 16}, "amount": 60, "damageType": "spell"}]}
  ],
  "presentation": {"animations": {"prepare": "idle", "release": "idle", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "crit", "effect": "effect.example.crit-spark", "event": "criticalStrike", "anchor": "target"},
    {"id": "bash", "effect": "effect.example.stunned", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "bashed"},
    {"id": "lastStand", "effect": "effect.example.death-burst", "event": "death", "anchor": "target"}
  ]}
}
```

Timed grant of a reaction from a targeted spell: give the status an id and set the trigger's `whileStatus` to it (required for targeted abilities).

## 20. Revival

Hero self-rebirth on death, once per cooldown (needs an actual hero):

```json ability
{
  "id": "ability.example.second-molt", "name": "Second Molt", "activation": "passive",
  "ranks": [{"delay": 280, "cooldown": 9600}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 0, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [],
  "triggers": [{"id": "rebirth", "event": "death", "cooldownTicks": {"rankParameter": "cooldown"},
    "operations": [{"op": "revive", "id": "molt", "target": "caster", "amount": {"rankParameter": "delay"}, "healthPermille": 1000, "manaPermille": 500, "placementRadius": 3}]}],
  "presentation": {"animations": {"prepare": "idle", "release": "idle", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "cocoon", "effect": "effect.example.molt-cocoon", "event": "revivalStarted", "anchor": "target", "durationFrom": "delivery"},
    {"id": "emerge", "effect": "effect.example.molt-emerge", "event": "revived", "anchor": "target"}
  ]}
}
```

Mass resurrection of allied corpses (permanent, original owners):

```json ability
{
  "id": "ability.example.spring-awakening", "name": "Spring Awakening", "activation": "targeted",
  "ranks": [{}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 20, "recoverTicks": 16, "cost": {"resource": "mana", "amount": 200, "commit": "release"}, "cooldown": {"ticks": 9600, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "resurrect", "target": "caster", "amount": 6, "corpses": {"radius": 12, "relations": ["ally"], "order": "strongest"}, "healthPermille": 1000, "manaPermille": 0, "placementRadius": 3, "supply": "allow-over-cap"}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "bloom", "effect": "effect.example.awakening-bloom", "event": "released", "anchor": "caster"},
    {"id": "risen", "effect": "effect.example.risen", "event": "resurrected", "anchor": "target"}
  ]}
}
```

Temporary reanimation under the caster's control with protective grants (templates: target-only, no query):

```json ability
{
  "id": "ability.example.hollow-march", "name": "Hollow March", "activation": "targeted",
  "ranks": [{"duration": 1600}],
  "statuses": [
    {"op": "status", "target": "target", "id": "hollowWard", "amount": {"rankParameter": "duration"}, "polarity": "positive", "dispel": false, "immunity": "physical"},
    {"op": "status", "target": "target", "id": "hollowSpirit", "amount": {"rankParameter": "duration"}, "polarity": "positive", "dispel": false, "spellImmunity": "hostile"}
  ],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 20, "recoverTicks": 16, "cost": {"resource": "mana", "amount": 175, "commit": "release"}, "cooldown": {"ticks": 7200, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "resurrect", "target": "caster", "ownership": "caster", "durationTicks": {"rankParameter": "duration"}, "grants": ["hollowWard", "hollowSpirit"],
    "amount": 6, "corpses": {"radius": 12, "relations": ["ally", "enemy", "neutral"], "order": "strongest"}, "healthPermille": 1000, "manaPermille": 0, "placementRadius": 3, "supply": "allow-over-cap"}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "risen", "effect": "effect.example.hollow-risen", "event": "resurrected", "anchor": "target"}
  ]}
}
```
