# Ability definition reference

Authority: `crates/content/src/abilities/definition.rs` (fields), `operations.rs` (operations, statuses), `amount.rs` (amounts, ranked references), `crates/content/src/conditions.rs` (filters, conditions), `crates/content/src/items.rs` (fixed `modifiers`). Cross-field rules: `abilities/rules.rs` (one ability on its own) and `abilities/references.rs` (rules that need the rest of the content). Every struct denies unknown fields. This page explains meaning and the rules a type cannot show. Runtime behaviour is in `runtime.md`; every operation is in `operations.md`; the `presentation` object is in `presentation.md`.

## One file, ids

An ability is one file, `abilities/<free folders>/<name>.json`, holding the definition and its presentation (`presentation`: icon, caster animations, effect bindings) together. There is no separate presentation document, presentation id or `schemaVersion`. Visual effects are separate files (`effects/`) referenced by id from `presentation.effects`. Unit bindings live on unit definitions (`behaviors.abilities`), not in the ability.

Ids (`crates/content/src/id.rs`) are lowercase segments `[a-z][a-z0-9]*` joined by `.` or `-`. Convention: `ability.<scope>.<name>`; effects must start with `effect.` (`effect.<scope>.<name>[.<part>]`). Rank parameter, status, record, trigger and operation ids are plain names such as `heroDuration`; they are unique where stated below.

## Top-level fields

| Field | Required | Meaning |
|---|---|---|
| `id`, `name` | yes | `name` is trimmed and must not be empty. |
| `description` | no (`""`) | Player-facing tooltip text. |
| `activation` | yes | `targeted` (cast by command, AI or autocast) or `passive` (always on while learned). |
| `ranks` | yes | 1–10 rank objects of named integers. |
| `targeting` | yes | Aim rules. |
| `cast` | yes | Lifecycle, cost, cooldown. |
| `onRelease` | yes | 0–8 operations: the payload program. |
| `presentation` | yes | `{icon?, animations, effects}`, see `presentation.md`. |
| `delivery` | no | `projectile`, `chain`, `line`, `charge`, `swarm`. |
| `persistent` | no | Repeating instance (zone, toggle, sustained buff). |
| `aura` | no | Passive membership aura. |
| `autocast` | no | Allows the autocast toggle. |
| `weaponCast` | no | Manual enhanced weapon shot. |
| `combatModifiers` | no | Passive critical/evasion/cleave/attack bonus. |
| `piercesSpellImmunity` | no | Ignores the `spellImmunity` gate (not damage immunity, armour or shields). |
| `statuses` | no | ≤8 status templates applied only by reference. |
| `triggers` | no | ≤8 reaction programs. |

An ability needs at least one of: `onRelease` operations, `triggers`, `combatModifiers`, `weaponCast`.

## Fixed boilerplate

These literals have no alternatives (the types accept only these values); copy them unchanged:

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

`revalidateAtRelease` must be exactly that list in that order. Passives still need a complete `cast` block (0 ticks, cost and cooldown) and `targeting` of kind `self`.

## Ranks and amounts

- `ranks`: 1–10 objects. Every rank declares exactly the same keys, at most 24, integer values.
- An **amount** is a non-negative integer literal or `{"rankParameter": "name"}`. Every referenced parameter must exist in every rank; the resolved value must satisfy the field's range at every rank (table below).
- Amount literals cannot be negative. A negative ranked modifier (slow, armour loss) uses a rank parameter holding the negative number, e.g. `"rankedModifiers": {"moveSpeedPermille": {"rankParameter": "slow"}}` with `"slow": -300`. The fixed status `modifiers` object is not an amount and accepts signed literals (`"moveSpeedPermille": -300`).
- A **ranked reference** (`summon.definition`, `split.members[]`) is an id string or `{"byRank": [id, ...]}` with exactly one id per rank.
- Units: ticks at 40 ticks/second (1 s = 40, max timer 144,000 = 1 h); distances in world units, the same as unit `dimensions` (a Warrior's body radius is 1.5); `Permille` 1000 = 100 %.

Resolved ranges the loader enforces (`rules.rs`):

| Field | Range |
|---|---|
| `cast.cost.amount` | 0–1,000,000 mana |
| `cast.cooldown.ticks` | 0–144,000 |
| `cast.channel.waves` / `intervalTicks` | 1–64 / 1–400 |
| `targeting.range` | 0–4096 |
| `targeting.radius` (point only) | 1–20 |
| operation `query.radius` | 1–32 |
| `scale.permille` | 0–10,000 |
| status `amount` (duration) | 1–144,000; exactly 0 for `lifetime: untilDeath` |
| status `heroDuration` | 0–144,000 |
| status `shield` / `periodic.damage` | 0–1,000,000 |
| status `damageTakenPermille` values | 0–5000 (1–16 types) |
| status `detectionRadius` | 1–64 |
| status `stacking.max` | 2–16 |
| summon `amount` / `maxActive` / `durationTicks` | 0–8 / 1–128 / 1–144,000 |
| summon `corpses.radius` / `corpses.maxTargets` | 1–32 / 1–8, `amount × maxTargets ≤ 8` |
| resurrect `amount` / `corpses.radius` | 1–8 / 1–32 |
| teleport `amount` (landing search radius) | 0–12 |
| vision `amount` / `radius` | 1–144,000 / 1–64 |
| contain / split / revive `amount` | 1–144,000 ticks |
| `healthPermille`, `manaPermille`, split `return*Permille` | 0–1000 (health ≥ 1) |
| `persistent.durationTicks` / `intervalTicks` | interval 1–400, duration interval…144,000 |
| `delivery.chain.bounces` | 1–16 |
| `delivery.swarm.count` | 1–32; `durationTicks ≥ 1 + (count−1)×launchIntervalTicks` |
| trigger `intervalTicks` / `cooldownTicks` / `manaCost` | 1–144,000 / 0–144,000 / 0–1,000,000 |
| critical `chancePermille` / `multiplierPermille` | 0–1000 / 1000–10,000 |
| `evasionPermille`, cleave `damagePermille` | 0–1000 |
| cleave `radius` | 1–32 |
| all other operation amounts | 0–1,000,000 |

Other bounds in these pages (speeds, hop radius, maximum targets, form scale, placement radius, …) are authoring limits from the old editor that the loader does not check yet; stay inside them.

## targeting

| Field | Meaning |
|---|---|
| `kind` | `unit` (a living unit), `point` (ground position), `self` (the caster; aim is the caster). |
| `relations` | Non-empty distinct subset of `ally`, `enemy`. Neutral (non-hostile, non-allied) units are never eligible. The caster is its own ally. Hostile neutral camps are `enemy`. |
| `allowSelf` | Whether the caster may be the unit aim / default area recipient. Use `true` for `self`. |
| `range` | Amount, 0–4096. A player cast order outside range walks into range first; AI and autocast only choose aims already in range. `self` uses 0. |
| `visible` | Unit aims require `true`. Point aims may use `false` to target unseen/fogged ground (remote scouting). |
| `radius` | Point aims only, 1–20: the default area for operations that use `target: "target"`. Required for point aims except with a `line` delivery. Unit/self aims must use operation queries for areas instead. |
| `maxTargets` | Positive (1–128), cap for the point default area (stable entity-id order). Default 128. |
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
| `natures` | Distinct subset of `organic`, `undead`, `mechanical` (units default to organic; buildings have no nature, so they fail a nature filter). |
| `locomotion` | Distinct subset of `ground`, `air` (current, including flight forms). |
| `minLevel`, `maxLevel` | Inclusive unit level bounds, min ≤ max (units without progression are level 1). |

## Conditions (`targeting.condition`, `branch.condition`)

A leaf, or `{"kind": "all"|"any", "conditions": [1–8 leaves]}` (groups do not nest).

| Leaf | Fields | True when |
|---|---|---|
| `relation` | `of: "target"`, `to: "caster"`, `is: ally\|enemy` | Recipient's relationship to the caster. |
| `matches` | `of: target\|caster`, `filter`, optional `relations` | Traits match; with `relations`, the target relationship also matches (caster counts as ally). |
| `controller` | `of`, `is: player\|neutral` | Owned by a player vs. neutral (camp) ownership. Independent of hostility. |
| `resource` | `of`, `resource: health\|mana`, `measure: current\|missing\|fractionPermille`, `comparison: lt\|lte\|eq\|gte\|gt`, `value` 0–1,000,000 | Exact integer comparison; `fractionPermille` compares `current×1000` with `value×max`. Unknown pools or zero capacity fail. |

Examples: injured target = `resource/target/health/missing/gt/0`; at most half health = `fractionPermille/lte/500`; has mana = `mana/current/gt/0`.

## cast

| Field | Meaning |
|---|---|
| `prepareTicks` | Wind-up after turning to face the aim; cancellable with refund. Literal integer, not ranked. |
| `recoverTicks` | After release (after the last wave for channels). Interruptible, no refund. Literal integer. |
| `channel` | `{waves, intervalTicks, tetherRange?}`: the caster stays in place and `onRelease` runs once per wave. `tetherRange` (≥1) replaces `range` as the unit-aim leash. |
| `cost` | Mana reserved at acceptance, spent at release, refunded on pre-release cancel. |
| `cooldown` | Starts at release; reduced by the caster's `cooldownReductionPermille`. |

## delivery (one kind)

| Kind | Requires | Fields |
|---|---|---|
| `projectile` | unit aim | `speed` ≥1 (to 100) units/s. Homing; `onRelease` runs at impact on the aim. |
| `chain` | unit aim | `bounces` amount 1–16 (total hits including the first), `radius` ≥1 (to 20) hop search, `intervalTicks` ≥1 (to 80) between hops, `retentionPermille` 1–1000 per hop, `skipFullHealth` (default false). |
| `line` | point aim | `speed` ≥1 (to 100), `length` ≥1 (to 64), `width` ≥0.1 (to 10, half-width). Travels from the caster toward the aim for `length`, hitting each eligible unit once. `targeting.radius` is not needed. |
| `charge` | unit aim | `speed` ≥1 (to 160), `maxTicks` ≥1 (to 400). Moves the grounded caster to a grounded target; `onRelease` on contact. |
| `swarm` | self aim | `count` (1–32), `durationTicks`, `speed` ≥1 (to 100), `radius` ≥1 (to 32, search around the caster), `launchIntervalTicks` 0–40, `attackIntervalTicks` ≥1 (to 400), `maxPerTarget` ≥1 (to 32), `returnThreshold` (amount ≥1), `returnPermille` 0–1000, `maxHitsPerTrip` ≥1 (to 32), `returnTimeoutTicks` ≥1 (to 4000), optional `filter`, `includeBuildings`. |

Delivery cannot combine with `cast.channel`, `persistent` or `activation: passive`.

## persistent

`{durationTicks, intervalTicks, anchor: point|caster|target, endsWithCaster (default true), tetherRange?, upkeepMana (default 0), toggle (default false)}`.

- Active, direct (no delivery/channel). `onRelease` runs at release and then every `intervalTicks` until `durationTicks` elapses.
- `anchor: target` requires unit targeting; `tetherRange` (≥1, to 128) requires `anchor: target`.
- `toggle: true` requires self targeting, `anchor: caster` and `endsWithCaster: true`; recasting while active switches it off at no cost. Use duration 144000 for "until toggled off".
- `upkeepMana` is paid per pulse after the first; failure to pay ends the instance.

## aura

`{radius ≥1 (to 32), meleeOnly}` on `activation: passive` only. Status operations with `target: "target"` (no query) are applied every tick to each eligible unit in radius (targeting relations/filter/allowSelf); operations with `target: "caster"` or a `query` run once per tick on the source.

- Every `onRelease` operation of an aura ability must be a `status` without `periodic`, `taunt`, `stun`, `disarm`, `silence`, `itemBlocked`, `breakOnDamage`, `shield`, `immunity`, `spellImmunity` or `manaShield`.
- No status anywhere in an aura ability (templates and triggers included) may have a `form`, `concealment`, `stacking` or `lifetime: untilDeath`, and no `interval` trigger may have `whileStatus`. Otherwise triggers and templates of an aura ability are free.
- Use amount 1 for aura status durations.

## autocast

`{intervalTicks ≥1 (to 400), enabledByDefault}` on a targeted ability; not on passives. The binding must include `ai` control for autocast to fire. For `weaponCast` it decorates attacks already being made instead of picking targets.

## weaponCast

`{bonus, weapon: melee|projectile|any, status?}`. Requires targeted, unit aim, `relations: ["enemy"]`, `allowSelf: false`, prepare/recover 0, empty `onRelease`, and no delivery, channel, persistent, aura or top-level `combatModifiers`. `bonus` > 0 at every rank unless `status` names a template. Cost/cooldown come from `cast` and are paid once at the weapon release. Siege (shell) weapons are not supported.

## combatModifiers

Top-level only on passives; on targeted spells grant them through a status. At least one policy:

| Policy | Fields |
|---|---|
| `critical` | `chancePermille`, `multiplierPermille` (≥1000). |
| `evasionPermille` | Chance to evade direct melee/arrow hits. |
| `cleave` | `damagePermille`, `radius`, `arcDegrees` ≥1 (to 360), `maxTargets` ≥1 (to 32), `includeBuildings` (false), optional `filter`. Melee only. |
| `attackBonus` | `amount`, `manaCost`, `weapon: melee\|projectile\|any`, `blockedBySilence` (true), optional `filter`, optional `status` (template applied on a damaging hit). Amount 0 with a status is a mark-only policy. |

## statuses (templates)

Up to 8 `status` operation objects (with `"op": "status"`). Declaring does not apply them. They are referenced by `combatModifiers.attackBonus.status`, `weaponCast.status`, `summon.grants`, `resurrect.grants`. Referenced templates must use `target: "target"`, no `query`, and lifetime `duration` or `untilDeath`. Template ids share the ability-wide unique status id space and can be named by `trigger.whileStatus`.

## triggers

| Field | Meaning |
|---|---|
| `id` | Unique per ability. |
| `event` | `weaponHit`, `weaponRelease`, `damaged`, `meleeDamaged`, `death`, `kill`, `interval`. |
| `operations` | 1–8 operations (branches allowed). |
| `whileStatus` | The trigger belongs to holders of that status of this ability. Required on targeted abilities; omitted on passives means "while learned". Must name a status declared in the ability. |
| `intervalTicks` | Required for `interval`, forbidden otherwise. |
| `weapon` | `melee\|projectile\|siege`, `weaponRelease` only. |
| `filter`, `includeBuildings` | Victim filter, `kill`/`weaponRelease` only (buildings excluded by default). |
| `blockedBySilence` | `weaponRelease`/`interval` only. |
| `chancePermille` 1–1000 (1000), `every` ≥1 (1, to 100), `manaCost` (0), `cooldownTicks` | Qualification, checked in order: cooldown, every-N counter, chance, mana. |
| `executor: "statusSource"` | `death` + `whileStatus` + zero `manaCost` only: runs as the original applier. |

Operation targeting rules: `kill` operations cannot use bare `target: "target"` (the victim is dead): use `caster`, `point` or a query. `death` operations must use a query or `point` (or `caster` with `executor: statusSource`, or `revive`). An `interval` trigger with `whileStatus` cannot be in an aura ability.

## Programs, branches, records

- `onRelease` and each trigger's `operations` are programs: ordered lists of up to 8 operations. An operation is an effect operation (see `operations.md`) or `{"op": "branch", "condition": …, "then": [1–8], "else": [1–8]}`. Both arms need operations; arms contain effect operations, not branches.
- `record` (on heal/damage/drain/mana/sacrifice) names a result; names are unique per program. A later `scale: {"source": "result", "id": …}` must reference an earlier record in the same program (branch arms count in then→else order).
- `scale.source: "target"` is invalid with point targeting.

## Recipients

| Field | Meaning |
|---|---|
| `target` | `target` (the aim; for point aims the default area), `caster`, or `point` (once at the aim point; only `summon`, `resurrect`, `vision` accept it). `split` and `revive` accept only `caster`; `resurrect` only `caster` or `point`. |
| `query` | Optional explicit selection: `{center: caster\|target\|point, radius, relations, allowSelf (false), includeBuildings (false), excludePrimary (false), maxTargets 1–128 (128), order: nearest\|lowest-health (nearest)}`. When present it replaces the `target` selection. Not allowed with `target: "point"` or corpse summons. |
| `filter` | Trait filter for this operation's recipients. Present on every operation except `summon`, `split`, `resurrect`, `revive` and `vision`. |

**`query` on resurrect, revive and vision.** The Rust types give `resurrect`, `revive` and `vision` an optional `query`, so a file with one parses, but the rules reject it ("live-unit queries are not supported", "no recipient query", "single anchors, not unit queries"). Do not author it. `summon` accepts a `query` (one group per selected recipient) unless it consumes corpses. `split` has no `query` field.

## Unique ids within an ability

Status ids (templates and operations together), `contain` ids, `revive` ids and `vision` ids must each be unique. Trigger ids are unique. Record names are unique per program.
