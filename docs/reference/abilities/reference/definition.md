# Ability definition reference

Authority: `src/content/abilities/schema.ts` (+ `conditions.ts`, `src/content/items.ts` for modifiers). The live `studio_schema` `spell` schema is the final word; this page explains meaning and the cross-field rules that JSON Schema cannot show. Runtime behaviour is in `runtime`; every operation is in `operations`.

## Documents and IDs

A spell is two documents saved together: `{definition, presentation}`. `definition.presentation` must equal `presentation.id`, and a presentation can belong to only one ability. Visual effects are separate documents referenced by ID from the presentation. Unit bindings live on unit definitions (`bind` command), not in the spell.

IDs match `^[a-z][a-z0-9]*(?:[.-][a-z0-9]+)+$` (max 160): `ability.<scope>.<name>`, `presentation.<scope>.<name>`, `effect.<scope>.<name>[.<part>]` (effects must start with `effect.`). Rank parameter, status, record, trigger and operation IDs match `^[a-z][a-zA-Z0-9]*$` (max 48), e.g. `heroDuration`.

## Top-level fields

| Field | Required | Meaning |
|---|---|---|
| `schemaVersion` | yes | Always `1`. |
| `id`, `name` (1–100), `description` (≤2000) | id, name | Description is player-facing tooltip text. |
| `activation` | yes | `targeted` (cast by command/AI/autocast) or `passive` (always on while learned). |
| `ranks` | yes | 1–10 rank objects of named integers. |
| `targeting` | yes | Aim rules. |
| `cast` | yes | Lifecycle, cost, cooldown. |
| `onRelease` | yes | 0–8 operations: the payload program. |
| `presentation` | yes | Presentation ID. |
| `delivery` | no | `projectile`, `chain`, `line`, `charge`, `swarm`. |
| `persistent` | no | Repeating instance (zone, toggle, sustained buff). |
| `aura` | no | Passive membership aura. |
| `autocast` | no | Allows the autocast toggle. |
| `weaponCast` | no | Manual enhanced weapon shot. |
| `combatModifiers` | no | Passive critical/evasion/cleave/attack bonus. |
| `piercesSpellImmunity` | no | Ignores the `spellImmunity` gate (not damage immunity, armor or shields). |
| `statuses` | no | ≤8 status templates applied only by reference. |
| `triggers` | no | ≤8 reaction programs. |

An ability needs at least one of: `onRelease` operations, `triggers`, `combatModifiers`, `weaponCast`.

## Fixed boilerplate

These literals have no alternatives; copy them unchanged:

```jsonc
"targeting": {"alive": true, "outOfRange": "reject", ...},
"cast": {
  "cost": {"resource": "mana", "amount": 40, "commit": "release"},
  "cooldown": {"ticks": 200, "start": "release", "scope": "ability"},
  "cancelBeforeRelease": "refund",
  "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"],
  ...
}
```

Passives still need a complete `cast` block (use 0 ticks/cost/cooldown) and `targeting` of kind `self`.

## Ranks and amounts

- `ranks`: 1–10 objects. Every rank declares exactly the same keys, at most 24, integer values −1,000,000…1,000,000.
- An **amount** is a non-negative integer literal or `{"rankParameter": "name"}`. Every referenced parameter must exist in every rank; the resolved value must satisfy the field's range at every rank (table below).
- Amount literals cannot be negative. A negative ranked modifier (slow, armor loss) uses a rank parameter holding the negative number, e.g. `"rankedModifiers": {"moveSpeedPermille": {"rankParameter": "slow"}}` with `"slow": -300`. The fixed status `modifiers` object is not an amount and accepts signed literals (`"moveSpeedPermille": -300`).
- A **ranked reference** (`summon.definition`, `split.members[]`) is an ID string or `{"byRank": [id, ...]}` with exactly one ID per rank.
- Units: ticks at 40 ticks/second (1 s = 40, max timer 144,000 = 1 h); distances in world units (a warrior is about 1 unit wide); `Permille` 1000 = 100 %.

Resolved ranges enforced at validation:

| Field | Range |
|---|---|
| `cast.cost.amount` | 0–1,000,000 mana |
| `cast.cooldown.ticks` | 0–144,000 |
| `cast.prepareTicks`, `recoverTicks` | 0–144,000 (literal integers, not ranked) |
| `cast.channel.waves` / `intervalTicks` | 1–64 / 1–400 |
| `targeting.range` | 0–4096 |
| `targeting.radius` (point only) | 1–20 |
| operation `query.radius` | 1–32 |
| `scale.permille` | 0–10,000 |
| status `amount` (duration) | 1–144,000; exactly 0 for `lifetime: untilDeath` |
| status `heroDuration` | 0–144,000 |
| status `shield` / `periodic.damage` | 0–1,000,000 |
| status `damageTakenPermille` values | 0–5000 |
| status `detectionRadius` | 1–64 |
| summon `amount` / `maxActive` / `durationTicks` | 0–8 / 1–128 / 1–144,000 |
| resurrect `amount` | 1–8 |
| teleport `amount` (landing search radius) | 0–12 |
| vision `amount` / `radius` | 1–144,000 / 1–64 |
| contain / split / revive `amount` | 1–144,000 ticks |
| `persistent.durationTicks` / `intervalTicks` | interval 1–400, duration interval…144,000 |
| `delivery.chain.bounces` | 1–16 |
| `delivery.swarm.count` | 1–32 |
| trigger `intervalTicks` / `cooldownTicks` / `manaCost` | 1–144,000 / 0–144,000 / 0–1,000,000 |
| critical `chancePermille` / `multiplierPermille` | 0–1000 / 1000–10,000 |
| `evasionPermille`, cleave `damagePermille` | 0–1000 |
| cleave `radius` | 1–32 |
| all other amounts | 0–1,000,000 |

## targeting

| Field | Meaning |
|---|---|
| `kind` | `unit` (a living unit), `point` (ground position), `self` (the caster; aim is the caster). |
| `relations` | Non-empty unique subset of `ally`, `enemy`. Neutral (non-hostile, non-allied) units are never eligible. The caster is its own ally. Hostile neutral camps are `enemy`. |
| `allowSelf` | Whether the caster may be the unit aim / default area recipient. Use `true` for `self`. |
| `range` | Amount, 0–4096. A player cast order outside range walks into range first; AI and autocast only choose aims already in range. `self` uses 0. |
| `visible` | Unit aims require `true`. Point aims may use `false` to target unseen/fogged ground (remote scouting). |
| `radius` | Point aims only, required, 1–20: the default area for operations that use `target: "target"`. Unit/self aims must use operation queries for areas instead. |
| `maxTargets` | 1–128, cap for the point default area (stable entity-ID order). Default 128. |
| `includeBuildings` | Buildings can be the unit aim / default area recipients. |
| `filter` | Trait filter for the aim and default area/chain/line/aura recipients. |
| `condition` | Condition for the aim (and default recipients). Checked at acceptance, release, delivery impact and by AI. |

Heal-only programs (every resolved release operation is `heal`) reject full-health unit aims at acceptance. Use a resource condition for other "injured-only" spells.

## Filters (`filter`)

All fields optional; omission accepts both:

| Field | Meaning |
|---|---|
| `heroes` | `true` heroes only, `false` non-heroes only. |
| `summoned` | Summoned/temporary units only, or exclude them. |
| `natures` | Subset of `organic`, `undead`, `mechanical` (units default to organic; buildings have no nature, so they fail a nature filter). |
| `locomotion` | Subset of `ground`, `air` (current, including flight forms). |
| `minLevel`, `maxLevel` | Inclusive unit level bounds (units without progression are level 1). |

## Conditions (`targeting.condition`, `branch.condition`)

A leaf, or `{"kind":"all"|"any","conditions":[1–8 leaves]}` (groups do not nest).

| Leaf | Fields | True when |
|---|---|---|
| `relation` | `of:"target"`, `to:"caster"`, `is: ally\|enemy` | Recipient's relationship to the caster. |
| `matches` | `of: target\|caster`, `filter`, optional `relations` | Traits match; with `relations`, the target relationship also matches (caster counts as ally). |
| `controller` | `of`, `is: player\|neutral` | Owned by a player vs. neutral (camp) ownership. Independent of hostility. |
| `resource` | `of`, `resource: health\|mana`, `measure: current\|missing\|fractionPermille`, `comparison: lt\|lte\|eq\|gte\|gt`, `value` 0–1,000,000 | Exact integer comparison; `fractionPermille` compares `current×1000` with `value×max`. Unknown pools or zero capacity fail. |

Examples: injured target = `resource/target/health/missing/gt/0`; at most half health = `fractionPermille/lte/500`; has mana = `mana/current/gt/0`.

## cast

| Field | Meaning |
|---|---|
| `prepareTicks` | Wind-up after turning to face the aim; cancellable with refund. |
| `recoverTicks` | After release (after the last wave for channels). Interruptible, no refund. |
| `channel` | `{waves, intervalTicks, tetherRange?}`: the caster stays in place and `onRelease` runs once per wave. `tetherRange` (1–128) replaces `range` as the unit-aim leash. |
| `cost` | Mana reserved at acceptance, spent at release, refunded on pre-release cancel. |
| `cooldown` | Starts at release; reduced by the caster's `cooldownReductionPermille`. |

## delivery (one kind)

| Kind | Requires | Fields |
|---|---|---|
| `projectile` | unit aim | `speed` 1–100 units/s. Homing; `onRelease` runs at impact on the aim. |
| `chain` | unit aim | `bounces` amount 1–16 (total hits including the first), `radius` 1–20 hop search, `intervalTicks` 1–80 between hops, `retentionPermille` 1–1000 per hop, `skipFullHealth` (default false). |
| `line` | point aim (with `targeting.radius`) | `speed` 1–100, `length` 1–64, `width` 0.1–10 (half-width). Travels from the caster toward the aim for `length`, hitting each eligible unit once. |
| `charge` | unit aim | `speed` 1–160, `maxTicks` 1–400. Moves the grounded caster to a grounded target; `onRelease` on contact. |
| `swarm` | self aim | `count` (1–32), `durationTicks`, `speed` 1–100, `radius` 1–32 (search around the caster), `launchIntervalTicks` 0–40, `attackIntervalTicks` 1–400, `maxPerTarget` 1–32, `returnThreshold` (amount ≥1), `returnPermille` 0–1000, `maxHitsPerTrip` 1–32, `returnTimeoutTicks` 1–4000, optional `filter`, `includeBuildings`. `durationTicks ≥ 1 + (count−1)×launchIntervalTicks`. |

Delivery cannot combine with `cast.channel`, `persistent` or `activation: passive`.

## persistent

`{durationTicks, intervalTicks, anchor: point|caster|target, endsWithCaster (default true), tetherRange?, upkeepMana (default 0), toggle (default false)}`.

- Active, direct (no delivery/channel). `onRelease` runs at release and then every `intervalTicks` until `durationTicks` elapses.
- `anchor: target` requires unit targeting; `tetherRange` (1–128) requires `anchor: target`.
- `toggle: true` requires self targeting, `anchor: caster` and `endsWithCaster: true`; recasting while active switches it off at no cost. Use duration 144000 for "until toggled off".
- `upkeepMana` is paid per pulse after the first; failure to pay ends the instance.

## aura

`{radius 1–32, meleeOnly}` on `activation: passive` only. Status operations with `target: "target"` (no query) are applied every tick to each eligible unit in radius (targeting relations/filter/allowSelf); operations with `target: "caster"` or a `query` run once per tick on the source. Aura abilities may contain only `status` operations (in all programs and templates) without `periodic`, `taunt`, `stun`, `disarm`, `silence`, `itemBlocked`, `breakOnDamage`, `shield`, `immunity`, `spellImmunity`, `manaShield`, `form`, `concealment` or `stacking`, and no `interval` trigger tied to a status. Use amount 1 for aura status durations.

## autocast

`{intervalTicks 1–400, enabledByDefault}` on a targeted ability; not on passives. The binding must include `ai` control for autocast to fire. For `weaponCast` it decorates attacks already being made instead of picking targets.

## weaponCast

`{bonus, weapon: melee|projectile|any, status?}`. Requires targeted, unit aim, `relations: ["enemy"]`, `allowSelf: false`, prepare/recover 0, empty `onRelease`, and no delivery, channel, persistent, aura or top-level `combatModifiers`. `bonus` > 0 at every rank unless `status` names a template. Cost/cooldown come from `cast` and are paid once at the weapon release. Siege (shell) weapons are not supported.

## combatModifiers

Top-level only on passives; on targeted spells grant them through a status. At least one policy:

| Policy | Fields |
|---|---|
| `critical` | `chancePermille`, `multiplierPermille` (≥1000). |
| `evasionPermille` | Chance to evade direct melee/arrow hits. |
| `cleave` | `damagePermille`, `radius`, `arcDegrees` 1–360, `maxTargets` 1–32, `includeBuildings` (false), optional `filter`. Melee only. |
| `attackBonus` | `amount`, `manaCost`, `weapon: melee\|projectile\|any`, `blockedBySilence` (true), optional `filter`, optional `status` (template applied on a damaging hit). Amount 0 with a status is a mark-only policy. |

## statuses (templates)

Up to 8 `status` operation objects. Declaring does not apply them. They are referenced by `combatModifiers.attackBonus.status`, `weaponCast.status`, `summon.grants`, `resurrect.grants`. Referenced templates must use `target: "target"`, no `query`, and lifetime `duration` or `untilDeath`. Template IDs share the ability-wide unique status ID space and can be named by `trigger.whileStatus`.

## triggers

| Field | Meaning |
|---|---|
| `id` | Unique per ability. |
| `event` | `weaponHit`, `weaponRelease`, `damaged`, `meleeDamaged`, `death`, `kill`, `interval`. |
| `operations` | 1–8 operations (branches allowed). |
| `whileStatus` | The trigger belongs to holders of that status of this ability. Required on targeted abilities; omitted on passives means "while learned". |
| `intervalTicks` | Required for `interval`, forbidden otherwise. |
| `weapon` | `melee\|projectile\|siege`, `weaponRelease` only. |
| `filter`, `includeBuildings` | Victim filter, `kill`/`weaponRelease` only (buildings excluded by default). |
| `blockedBySilence` | `weaponRelease`/`interval` only. |
| `chancePermille` 1–1000 (1000), `every` 1–100 (1), `manaCost` (0), `cooldownTicks` | Qualification, checked in order: cooldown, every-N counter, chance, mana. |
| `executor: "statusSource"` | `death` + `whileStatus` + zero `manaCost` only: runs as the original applier. |

Operation targeting rules: `kill` operations cannot use bare `target: "target"` (the victim is dead): use `caster`, `point` or a query. `death` operations must use a query or `point` (or `caster` with `executor: statusSource`, or `revive`). An `interval` trigger with `whileStatus` cannot be in an aura ability.

## Programs, branches, records

- `onRelease` and each trigger's `operations` are programs: ordered lists of up to 8 operations. An operation is an effect operation (see `operations`) or `{"op":"branch","condition":…,"then":[1–8],"else":[1–8]}` (arms contain effect operations, not branches).
- `record` (on heal/damage/drain/mana/sacrifice) names a result; names are unique per program. A later `scale: {"source":"result","id":…}` must reference an earlier record in the same program (branch arms count in then→else order).
- `scale.source: "target"` is invalid with point targeting.

## Recipients (every operation except point-only ones)

| Field | Meaning |
|---|---|
| `target` | `target` (the aim; for point aims the default area), `caster`, or `point` (once at the aim point; only `summon`, `resurrect`, `vision` accept it). |
| `query` | Optional explicit selection: `{center: caster\|target\|point, radius, relations, allowSelf (false), includeBuildings (false), excludePrimary (false), maxTargets 1–128 (128), order: nearest\|lowest-health (nearest)}`. When present it replaces the `target` selection. Not allowed with `target: "point"`, `vision`, `revive`, `resurrect`, corpse summons, `split`. |
| `filter` | Trait filter for this operation's recipients. |

## Unique IDs within an ability

Status IDs (templates and operations together), `contain` IDs, `revive` IDs and `vision` IDs must each be unique. Trigger IDs are unique. Record names are unique per program.
