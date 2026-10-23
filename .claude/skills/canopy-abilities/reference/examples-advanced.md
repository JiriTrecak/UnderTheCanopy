# Worked examples: advanced compositions

Complete abilities (`json ability`, parsed by `crates/content/tests/skills.rs`) and operation fragments (plain `json`) for the less common capabilities. Same conventions as `examples.md`: ids are examples, and a real file needs ids that exist. Operation fragments drop into any `onRelease`, trigger or branch arm that satisfies their stated requirements.

## Manual enhanced weapon shot (weaponCast)

The unit's own weapon fires once; cost/cooldown are paid at the real release; autocast decorates ordinary attacks. The caster needs a projectile weapon.

```json ability
{
  "id": "ability.example.frost-shot", "activation": "targeted",
  "weaponCast": {"bonus": {"rankParameter": "bonus"}, "weapon": "projectile", "status": "chilled"},
  "autocast": {"intervalTicks": 1, "enabledByDefault": false},
  "ranks": [{"bonus": 5, "slow": -300}, {"bonus": 10, "slow": -500}],
  "statuses": [{"op": "status", "target": "target", "id": "chilled", "amount": 200, "heroDuration": 60, "polarity": "negative", "dispel": true,
    "rankedModifiers": {"moveSpeedPermille": {"rankParameter": "slow"}, "attackSpeedPermille": {"rankParameter": "slow"}}}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 30, "outOfRange": "reject"},
  "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 10, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "ready", "effect": "effect.example.frost-ready", "event": "weaponOrdered", "anchor": "caster"},
    {"id": "hit", "effect": "effect.example.frost-hit", "event": "enhancedHit", "anchor": "target"},
    {"id": "chill", "effect": "effect.example.frost-chill", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "chilled"}
  ]}
}
```

Bind `weaponOrdered`, `projectile` (the real arrow), `enhancedHit`, `statusApplied`.

## Toggled attack bonus with an impact mark

A toggle grants an `instance` status whose `combatModifiers.attackBonus` pays mana per enhanced shot and applies a template. A `death` trigger with `executor: "statusSource"` makes the marked victim's death summon for the archer.

```json ability
{
  "id": "ability.example.dusk-arrows", "activation": "targeted",
  "persistent": {"durationTicks": 144000, "intervalTicks": 400, "anchor": "caster", "endsWithCaster": true, "toggle": true},
  "ranks": [{"bonus": 10, "cost": 6}],
  "statuses": [{"op": "status", "target": "target", "filter": {"heroes": false, "natures": ["organic"]}, "id": "duskMark", "amount": 80, "polarity": "negative", "dispel": true}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 0, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "status", "target": "caster", "id": "duskQuiver", "amount": 800, "polarity": "positive", "dispel": false, "lifetime": "instance",
    "combatModifiers": {"attackBonus": {"status": "duskMark", "amount": {"rankParameter": "bonus"}, "manaCost": {"rankParameter": "cost"}, "weapon": "projectile", "blockedBySilence": true}}}],
  "triggers": [{"id": "rise", "event": "death", "executor": "statusSource", "whileStatus": "duskMark",
    "operations": [{"op": "summon", "target": "point", "amount": 1, "definition": "unit.example.tadpole-spirit", "durationTicks": 3200, "replace": false, "radius": 2}]}],
  "presentation": {"animations": {"prepare": "idle", "release": "idle", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "quiver", "effect": "effect.example.dusk-quiver", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "duskQuiver"},
    {"id": "mark", "effect": "effect.example.dusk-mark", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "duskMark"}
  ]}
}
```

## Curse until death with an applier-owned payoff

`untilDeath` uses amount 0. The death trigger runs as the original applier even if it died or changed owner.

```json ability
{
  "id": "ability.example.rot-curse", "activation": "targeted",
  "ranks": [{"damage": 40}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 13, "outOfRange": "reject", "filter": {"heroes": false, "maxLevel": 5}},
  "cast": {"prepareTicks": 12, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 150, "commit": "release"}, "cooldown": {"ticks": 4800, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "status", "target": "target", "id": "rot", "amount": 0, "lifetime": "untilDeath", "polarity": "negative", "dispel": false, "silence": true, "itemBlocked": true,
    "periodic": {"intervalTicks": 40, "damage": {"rankParameter": "damage"}, "damageType": "spell"}}],
  "triggers": [{"id": "bloom", "event": "death", "executor": "statusSource", "whileStatus": "rot",
    "operations": [{"op": "heal", "target": "caster", "amount": 100}]}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "rot", "effect": "effect.example.rot", "event": "statusApplied", "anchor": "target", "lifetime": "status"}
  ]}
}
```

## Ownership: charm with a controller condition

Player-owned victims of any level, neutral creatures only up to level 5.

```json ability
{
  "id": "ability.example.pheromone-charm", "activation": "targeted",
  "ranks": [{}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 14, "outOfRange": "reject",
    "filter": {"heroes": false, "natures": ["organic", "undead"]},
    "condition": {"kind": "any", "conditions": [{"kind": "controller", "of": "target", "is": "player"}, {"kind": "matches", "of": "target", "filter": {"maxLevel": 5}}]}},
  "cast": {"prepareTicks": 12, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 150, "commit": "release"}, "cooldown": {"ticks": 1800, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "convert", "target": "target", "amount": 1, "supply": "allow-over-cap"}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "charm", "effect": "effect.example.charm", "event": "converted", "anchor": "target"}
  ]}
}
```

## Containment and unloading

Swallow with digestion; a second self spell releases passengers (`releaseContained` on the caster).

```json ability
{
  "id": "ability.example.gullet", "activation": "targeted",
  "ranks": [{}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 8, "outOfRange": "reject", "filter": {"heroes": false, "locomotion": ["ground"]}},
  "cast": {"prepareTicks": 12, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 0, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "contain", "id": "stomach", "target": "target", "shareVision": true, "amount": 2400, "lifetime": "untilDeath", "capacity": 1,
    "digestion": {"intervalTicks": 40, "damage": 5, "damageType": "spell"}}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "gulp", "effect": "effect.example.gulp", "event": "contained", "anchor": "caster"},
    {"id": "spit", "effect": "effect.example.spit", "event": "releasedContained", "anchor": "target"}
  ]}
}
```

```json
{"op": "releaseContained", "target": "caster", "amount": 1}
```

## Sacrifice converted into healing (records and result scaling)

```json ability
{
  "id": "ability.example.colony-offering", "activation": "targeted",
  "ranks": [{"factor": 1000}, {"factor": 2000}],
  "targeting": {"kind": "unit", "relations": ["ally"], "allowSelf": false, "alive": true, "visible": true, "range": 12, "outOfRange": "reject", "filter": {"heroes": false}},
  "cast": {"prepareTicks": 10, "recoverTicks": 10, "cost": {"resource": "mana", "amount": 50, "commit": "release"}, "cooldown": {"ticks": 600, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "sacrifice", "target": "target", "amount": 1, "record": "offering"},
    {"op": "heal", "target": "caster", "amount": 0, "scale": {"source": "result", "id": "offering", "stat": "maxHealth", "permille": {"rankParameter": "factor"}}}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "offer", "effect": "effect.example.offering", "event": "sacrificed", "anchor": "target"},
    {"id": "gain", "effect": "effect.example.offering-heal", "event": "healed", "anchor": "target"}
  ]}
}
```

Other scaling patterns (operations):

```json
{"op": "damage", "target": "target", "amount": 0, "damageType": "spell", "scale": {"source": "recipient", "stat": "maxHealth", "permille": 40}}
```

```json
{"op": "heal", "target": "caster", "amount": 0, "scale": {"source": "caster", "stat": "missingHealth", "permille": 250}}
```

Heal the caster for 10 per enemy actually damaged by an earlier `"record": "struck"` area damage: `{"op": "heal", "target": "caster", "amount": 0, "scale": {"source": "result", "id": "struck", "stat": "count", "permille": 10000}}` (count × 10000 / 1000).

## Mana drain variants

```json
{"op": "drain", "target": "target", "amount": 30, "resource": "mana", "restoreCaster": true, "damageType": "spell"}
```

```json
{"op": "drain", "target": "target", "amount": 50, "resource": "mana", "restoreCaster": false, "damageType": "spell", "damagePerDrainedPermille": 1000}
```

## Returning swarm

Self-cast seekers bite visible enemies, carry drained health home and restore the caster on return. Bind `projectile` (seeker flight, missile with trail), `impact` (bite), `returned`.

```json ability
{
  "id": "ability.example.gnat-cloud", "activation": "targeted",
  "delivery": {"kind": "swarm", "count": 12, "durationTicks": 800, "speed": 18, "radius": 16, "launchIntervalTicks": 8, "attackIntervalTicks": 20,
    "maxPerTarget": 4, "returnThreshold": 20, "returnPermille": 750, "maxHitsPerTrip": 4, "returnTimeoutTicks": 400},
  "ranks": [{}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 10, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 150, "commit": "release"}, "cooldown": {"ticks": 4800, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "drain", "target": "target", "amount": 10, "resource": "health", "restoreCaster": true, "damageType": "spell"}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "seeker", "effect": "effect.example.gnat-flight", "event": "projectile", "anchor": "target", "durationFrom": "delivery"},
    {"id": "bite", "effect": "effect.example.gnat-bite", "event": "impact", "anchor": "target"},
    {"id": "home", "effect": "effect.example.gnat-return", "event": "returned", "anchor": "caster"}
  ]}
}
```

## Linked split form

`split` must be the only `onRelease` operation of a targeted self spell (no delivery, channel or persistent). Every member must be a controllable, zero-supply, non-hero unit definition; add such units under `units/` first if they do not exist.

```json ability
{
  "id": "ability.example.trinity", "activation": "targeted",
  "ranks": [{}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 16, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 125, "commit": "release"}, "cooldown": {"ticks": 4800, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "split", "id": "trinity", "target": "caster", "amount": 2400, "members": ["unit.example.earth-form", "unit.example.storm-form", "unit.example.fire-form"],
    "radius": 3, "returnHealthPermille": 1000, "returnManaPermille": 1000, "onAllLost": "kill"}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "part", "effect": "effect.example.split-flash", "event": "splitStarted", "anchor": "target"},
    {"id": "merge", "effect": "effect.example.merge-flash", "event": "splitEnded", "anchor": "target"}
  ]}
}
```

## Remote vision (scouting)

Unseen point targeting needs `visible: false`. Pair with `"ai": {"intent": "scout", ...}`.

```json ability
{
  "id": "ability.example.far-antennae", "activation": "targeted",
  "ranks": [{"range": 128}],
  "targeting": {"kind": "point", "radius": 8, "relations": ["ally"], "allowSelf": false, "alive": true, "visible": false, "range": {"rankParameter": "range"}, "outOfRange": "reject"},
  "cast": {"prepareTicks": 8, "recoverTicks": 8, "cost": {"resource": "mana", "amount": 25, "commit": "release"}, "cooldown": {"ticks": 200, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "vision", "id": "scouting", "target": "point", "amount": 480, "radius": 8, "ignoreTerrain": true, "detectInvisible": false, "endsWithCaster": false}],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "eye", "effect": "effect.example.far-eye", "event": "visionCreated", "anchor": "target", "durationFrom": "delivery", "sizeFrom": "radius"}
  ]}
}
```

## Concealment and detection

Self invisibility that breaks on attack/cast with a revealing-strike bonus:

```json
{"op": "status", "target": "caster", "id": "veil", "amount": 400, "polarity": "positive", "dispel": true, "modifiers": {"moveSpeedPermille": 300},
 "concealment": {"fadeTicks": 16, "breakOnAttack": true, "breakOnCast": true, "attackBonus": 60}}
```

Permanent detector (passive aura whose caster operation keeps a detection status on the holder):

```json ability
{
  "id": "ability.example.keen-antennae", "activation": "passive",
  "aura": {"radius": 1, "meleeOnly": false},
  "ranks": [{"radius": 12}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 0, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [{"op": "status", "target": "caster", "id": "detector", "amount": 1, "polarity": "positive", "dispel": false, "detectionRadius": {"rankParameter": "radius"}}],
  "presentation": {"animations": {"prepare": "idle", "release": "idle", "recover": "idle", "fallback": "idle"}, "effects": []}
}
```

## Taunt with self-protection

```json ability
{
  "id": "ability.example.challenge-hiss", "activation": "targeted",
  "ranks": [{"taunt": 100, "heroTaunt": 50, "armor": 6}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 16, "recoverTicks": 20, "cost": {"resource": "mana", "amount": 65, "commit": "release"}, "cooldown": {"ticks": 520, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [
    {"op": "status", "target": "target", "query": {"center": "caster", "radius": 12, "relations": ["enemy"], "maxTargets": 32}, "id": "challenged",
     "amount": {"rankParameter": "taunt"}, "heroDuration": {"rankParameter": "heroTaunt"}, "polarity": "negative", "dispel": true, "taunt": true},
    {"op": "status", "target": "caster", "id": "braced", "amount": 200, "polarity": "positive", "dispel": true, "rankedModifiers": {"armor": {"rankParameter": "armor"}}}
  ],
  "presentation": {"animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "roar", "effect": "effect.example.hiss-wave", "event": "released", "anchor": "caster"},
    {"id": "challenged", "effect": "effect.example.challenged", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "challenged"},
    {"id": "braced", "effect": "effect.example.braced", "event": "statusApplied", "anchor": "target", "lifetime": "status", "statusId": "braced"}
  ]}
}
```

## Protection statuses (operations)

Hostile-spell ward (friendly spells still land):

```json
{"op": "status", "target": "target", "id": "ward", "amount": 240, "polarity": "positive", "dispel": true, "spellImmunity": "hostile"}
```

Control immunity without other effects:

```json
{"op": "status", "target": "target", "id": "unshaken", "amount": 240, "polarity": "positive", "dispel": true,
 "controlImmunity": ["stun", "root", "silence", "disarm", "itemBlocked", "moveSlow", "attackSlow", "taunt"]}
```

Damage reduction by type plus melee reflection (ranked values from parameters `taken`, `reflection`):

```json
{"op": "status", "target": "caster", "id": "carapace", "amount": 240, "polarity": "positive", "dispel": true,
 "damageTakenPermille": {"melee": {"rankParameter": "taken"}, "piercing": {"rankParameter": "taken"}, "spell": {"rankParameter": "taken"}},
 "rankedModifiers": {"meleeReflectionPermille": {"rankParameter": "reflection"}}}
```

Banish-style ethereal state with spell vulnerability and a half-transparent look:

```json
{"op": "status", "target": "target", "id": "banished", "amount": 480, "heroDuration": 160, "polarity": "negative", "dispel": true,
 "ethereal": true, "damageTakenPermille": {"spell": 1660}, "modifiers": {"moveSpeedPermille": -500}, "form": {"opacity": 0.5}}
```

Damage immunity by category (combine both for full immunity):

```json
{"op": "status", "target": "target", "id": "stoneSkin", "amount": 200, "polarity": "positive", "dispel": false, "immunity": "physical"}
```

Mana shield inside a toggle (persistent self toggle as in `examples.md` §12; the status needs `lifetime: "instance"`):

```json
{"op": "status", "target": "caster", "id": "manaShield", "amount": 80, "polarity": "positive", "dispel": false, "lifetime": "instance",
 "manaShield": {"damagePerMana": 2, "absorbPermille": 1000}}
```

## Dispel

Area purge at a point (`targeting.kind: "point"` with radius, relations ally+enemy): removes all dispellable statuses and deals 200 to hostile summons.

```json
{"op": "dispel", "target": "target", "amount": 200, "damageType": "spell", "polarity": "all"}
```

Cleanse an ally (negative only, no damage) and purge buffs from an enemy:

```json
{"op": "dispel", "target": "target", "amount": 0, "damageType": "spell", "polarity": "negative"}
```

```json
{"op": "dispel", "target": "target", "amount": 0, "damageType": "spell", "polarity": "positive"}
```

## Branch on caster state

A branch can test the caster too; operations in the chosen arm still choose their own recipients.

```json
{"op": "branch",
 "condition": {"kind": "resource", "of": "caster", "resource": "health", "measure": "fractionPermille", "comparison": "lte", "value": 500},
 "then": [{"op": "heal", "target": "caster", "amount": 150}],
 "else": [{"op": "damage", "target": "target", "amount": 90, "damageType": "spell"}]}
```

## Interval aura-like pulse from a passive

Auras cannot run periodic damage; a passive interval trigger with a query can.

```json ability
{
  "id": "ability.example.spore-halo", "activation": "passive",
  "ranks": [{"damage": 6}],
  "targeting": {"kind": "self", "relations": ["ally"], "allowSelf": true, "alive": true, "visible": true, "range": 0, "outOfRange": "reject"},
  "cast": {"prepareTicks": 0, "recoverTicks": 0, "cost": {"resource": "mana", "amount": 0, "commit": "release"}, "cooldown": {"ticks": 0, "start": "release", "scope": "ability"}, "cancelBeforeRelease": "refund", "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]},
  "onRelease": [],
  "triggers": [{"id": "pulse", "event": "interval", "intervalTicks": 40, "blockedBySilence": true,
    "operations": [{"op": "damage", "target": "target", "query": {"center": "caster", "radius": 4, "relations": ["enemy"]}, "amount": {"rankParameter": "damage"}, "damageType": "spell"}]}],
  "presentation": {"animations": {"prepare": "idle", "release": "idle", "recover": "idle", "fallback": "idle"}, "effects": [
    {"id": "pulse", "effect": "effect.example.spore-pulse", "event": "interval", "anchor": "caster"}
  ]}
}
```

A sound layer for the pulse (`{asset, role: "audio", index}`; index 1 is `audio.wav`):

```json effect
{"id": "effect.example.spore-pulse", "name": "Spore pulse", "durationTicks": 30, "layers": [
  {"id": "puff", "shape": "ring", "colour": "#8fae5a", "accent": "#e2f0b8", "durationTicks": 30, "count": 1, "size": 4, "height": 0.05},
  {"id": "hiss", "shape": "sound", "colour": "#ffffff", "accent": "#ffffff", "durationTicks": 30, "count": 1, "size": 1, "height": 0,
   "sound": {"asset": "asset.audio.example.spore-hiss", "role": "audio", "index": 1, "volume": 0.4, "pitch": 1, "loop": false, "fadeInTicks": 0, "fadeOutTicks": 8, "referenceDistance": 6, "maxDistance": 40}}
]}
```
