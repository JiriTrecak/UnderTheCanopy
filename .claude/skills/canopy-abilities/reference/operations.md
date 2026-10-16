# Operation reference

Authority: `crates/content/src/abilities/operations.rs` (fields), `abilities/rules.rs` (cross-field rules), `crates/content/src/items.rs` (fixed modifiers); runtime in `crates/sim/src/abilities/`.

Every entry in `onRelease`, a trigger's `operations`, a branch arm or `statuses` is one of these `op` values. Recipient fields (`target`, `query`, `filter`) are described in `definition.md`; how recipients are chosen at runtime is in `runtime.md`. The event column names the presentation event emitted per successful recipient (none for aura refreshes).

| op | Purpose | Event |
|---|---|---|
| `heal` | Restore health | `healed` |
| `damage` | Deal typed damage | `damaged` |
| `drain` | Remove health or mana, optionally give it to the caster | `drained` |
| `mana` | Grant mana | `manaRestored` |
| `status` | Apply a timed/linked status | `statusApplied` |
| `dispel` | Remove dispellable statuses, hurt hostile summons | `dispelled` |
| `summon` | Create units | `summoned` |
| `resurrect` | Raise corpses | `resurrected` |
| `revive` | Return the fallen holder hero (death trigger) | `revivalStarted`, then `revived`/`revivalCancelled` |
| `split` | Replace the caster with linked member units | `splitStarted`, `splitEnded` |
| `teleport` | Move units with clearance | `teleported` |
| `vision` | Temporary fog-of-war sensor | `visionCreated` |
| `convert` | Permanent ownership transfer | `converted` |
| `contain` | Swallow units into the source | `contained`, `digested`, `releasedContained` |
| `releaseContained` | Unload a carrier | `releasedContained` |
| `sacrifice` | Kill an owned unit as a cost | `sacrificed` |

## Resource operations: heal, damage, drain, mana

Shared fields: `target`, `query?`, `filter?`, `amount`, `scale?`, `record?`.

- `heal`: clamps at max health; never affects dead units.
- `damage`: requires `damageType`, a key of `damageTypes` in `rules/combat.json` (currently `melee`, `piercing`, `hero`, `siege` apply armour, `spell` ignores it). Mitigation order: damage immunity and the strongest `damageTakenPermille` increase/decrease → armor-class matchup and armor (types with `appliesArmor`) → `invulnerable` and item shields → mana shields → finite spell shields → health. Lethal hits use ordinary death, loot, experience and kill credit.
- `drain`: `resource: health|mana`, `restoreCaster`, `damageType`, `damagePerDrainedPermille` 0–2000 (default 0). Health drain is ordinary damage of `damageType` and transfers the damage actually dealt. Mana drain removes up to the recipient's mana; with `damagePerDrainedPermille` it also deals `floor(drained × permille / 1000)` damage (mana burn). `restoreCaster` gives the actual amount to a living caster that still has the original owner (clamped). In a swarm, restoration is carried home instead.
- `mana`: grants mana up to the recipient's capacity minus any reserved cast cost. Units without mana pools gain nothing.
- Chain retention multiplies these four amounts per hop (not status durations).

### scale and record

`scale` adds `floor(stat × permille / 1000)` to `amount` (result clamped 0–1,000,000):

```jsonc
{"source": "caster" | "target" | "recipient", "stat": "health" | "maxHealth" | "missingHealth" | "mana" | "maxMana" | "missingMana", "permille": amount}
{"source": "result", "id": "recordName", "stat": "applied" | "count" | "health" | "maxHealth" | "mana" | "maxMana", "permille": amount}
```

- `caster` and `target` (the primary unit aim) are snapshots for one release/impact/wave/pulse. `recipient` is read fresh for each recipient. Point aims have no `target` stats.
- `record: "name"` (heal, damage, drain, mana, sacrifice) aggregates successful recipients only: summed actual `applied` amount, `count`, and their pre-operation `health`/`maxHealth`/`mana`/`maxMana`. Records exist only within one program run; they never carry between waves, chain hops, pulses or casts. A record from an unchosen branch reads 0.

## status

| Field | Meaning |
|---|---|
| `id` | Status ID, unique in the ability. Reapplying the same ability+ID refreshes it (replaces; restarts duration and periodic clock). |
| `amount` | Duration in ticks (≥1); `0` with `lifetime: untilDeath`. Aura statuses use 1 (they refresh every tick). |
| `heroDuration` | Replaces `amount` on heroes. |
| `polarity` | `positive` or `negative`; used by dispels and AI scoring. |
| `dispel` | `true` if dispels can remove it. |
| `lifetime` | `duration` (default); `instance` (persistent abilities only: removed when the persistent instance ends; `amount` must outlast the pulse interval, e.g. 2× interval); `untilDeath`. |
| `modifiers` | Fixed stat modifiers (below). Default `{}`. |
| `rankedModifiers` | Amount-valued modifiers for `maxHp`, `healthRegenPerSecond`, `manaRegenPerSecond`, `damage`, `armor`, `damagePermille`, `attackSpeedPermille`, `moveSpeedPermille`, `lifestealPermille`, `meleeReflectionPermille`. Negative values come from rank parameters. Merged over `modifiers`. |
| `stun` | No orders, attacks, movement or casting; cancels an unreleased cast. |
| `silence` | No spellcasting; cancels an unreleased cast; blocks `blockedBySilence` procs. |
| `disarm` | No weapon attacks. |
| `itemBlocked` | No item use. |
| `taunt` | Hostile combat units attack the living status source, then resume orders. Latest taunt wins. Rejected on weaponless or non-hostile recipients. |
| `breakOnDamage` | Any damage removes the status (sleep). |
| `shield` | Finite absorb pool (after armor). |
| `onShieldDepleted` | `retain` (default) or `remove` the whole status when the pool empties. Requires a positive shield at every rank. The exhausting hit keeps its mitigation; overflow continues into later shields and health. |
| `manaShield` | `{damagePerMana 1–10, absorbPermille 1–1000}`: absorbs that share of damage, spending 1 mana per `damagePerMana` absorbed. |
| `immunity` | `spell` blocks `spell` damage; `physical` blocks every non-spell damage type. Checked before shields. |
| `spellImmunity` | `hostile` rejects operations from enemies; `all` rejects every spell operation, including friendly and self (unless the ability `piercesSpellImmunity`). Existing periodic damage is suppressed, not removed. |
| `controlImmunity` | 1–8 of `stun`, `root`, `silence`, `disarm`, `itemBlocked`, `moveSlow`, `attackSlow`, `taunt`: blocks incoming components and suppresses existing ones while active (their timers keep running). A control-only application that is fully blocked creates no status. |
| `ethereal` | Cannot attack with weapons and takes no non-spell damage; still visible, collidable and spell-targetable. |
| `damageTakenPermille` | Map of 1–16 damage type IDs to 0–5000 (1000 = unchanged). Strongest increase and strongest decrease per type combine once. |
| `periodic` | `{intervalTicks ≥1, damage, damageType}`: damage every interval, first tick one interval after application. |
| `concealment` | `{fadeTicks 0–400, breakOnAttack, breakOnCast, attackBonus (0)}`: hidden from enemies after the fade unless detected; allies see a faded model. `attackBonus` is added to the revealing attack. Area effects still hit concealed units. |
| `detectionRadius` | 1–64: reveals concealed enemies within radius (needs ordinary sight). |
| `form` | `{asset?, scale 0.25–3 (1), opacity 0.1–1 (1), combatProfile?, movement?}`. `asset` is a model asset id (an asset with geometry). `combatProfile` is an armed unit definition that supplies weapon damage/range/timing/projectile. `movement: {locomotion: ground\|air, flightHeight 1–30 (air only)}` changes navigation, collision class and targeting; ground forces landing. Latest form wins; expiry reveals the previous/original form. Body size never changes. Units only. |
| `combatModifiers` | Same policies as the top-level field, active while the status lasts. |
| `stacking` | `{max 2–16, scope: source\|ability}`: each application adds a stack (to max) and refreshes the duration; numeric modifiers multiply by stacks. `source` keeps a stack count per caster; overlapping sources use the strongest value per modifier. Only plain `duration` statuses without form, periodic, shields, mana shield, combat modifiers, concealment, and not in aura abilities. |
| `sourceAttackBonus` | Flat bonus per stack to later primary weapon attacks from this status's source (same owner) against the holder, before armor. Requires `stacking.scope: source`. |

Fixed `modifiers` keys (from item modifiers): `maxHp` ≥0, `maxMana` ≥0, `damage` −10000…10000, `armor` −1000…1000, `healthRegenPerSecond` ≥0, `manaRegenPerSecond` ≥0, `damagePermille` −1000…1000, `attackSpeedPermille` −800…1000, `moveSpeedPermille` −800…1000, `cooldownReductionPermille` 0–500, `lifestealPermille` 0–500 (ordinary attack damage only), `meleeReflectionPermille` 0–1000 (returns a share of hostile melee damage as spell damage), `rooted` (no movement), `controlImmune` (all control kinds), `invulnerable` (absorbs all damage). Ranked values must stay inside the same bounds (stacked maxima included). Raising `maxHp` does not heal; add a `heal` operation.

Application rules: control components, forms, concealment and ethereal apply only to units. A unit holds at most 32 spell statuses. Recipients behind spell immunity reject the whole operation.

Statuses are also how other units carry this ability's `triggers`: a trigger with `whileStatus` runs for whoever currently holds that status (see `runtime.md`).

## dispel

`{target, query?, filter?, amount, damageType, polarity: all|positive|negative}`. Removes every spell status with `dispel: true` and matching polarity (aura statuses and item statuses are untouched). If the recipient is a summoned unit hostile to the caster it also takes `amount` damage (use 0 for none). Event amount is the number of removed statuses.

## summon

| Field | Meaning |
|---|---|
| `target` | `caster`, `target` (the aim unit; for point aims the default area units) or `point` (once at the aim). With `query`, one group per selected recipient. Summon has no `filter`. |
| `amount` | Units per group, 0–8. |
| `definition` | Unit definition id or `{byRank}`; must be a unit definition (`units/`). |
| `durationTicks` | Omit for permanent; otherwise 1–144,000. |
| `replace` | Remove this caster's previous summons from this ability once a new unit is placed. |
| `radius` | 1–12: placement ring around the group centre. |
| `maxActive` | 1–128 living summons from this caster and ability (all ranks). |
| `endsWithCaster` | Remove them when the source dies, is removed or changes controller (cascades to their own linked summons). |
| `grants` | ≤8 template IDs applied to each created unit (bypassing spell immunity for that initial grant). |
| `corpses` | `{radius, relations ⊆ ally/enemy/neutral, filter?, order: nearest\|strongest, maxTargets}`: create one group per consumed visible corpse instead. `target` must be `caster` or `point`, no query, `amount × maxTargets ≤ 8`. |

Summons belong to the caster's owner (and neutral camp) and are flagged `summoned`. Creation checks only the hard unit limit, but a living summon's `supplyCost` then counts as used supply, so summon-only unit definitions normally have zero supply. Summoned units are ordinary unit definitions; add one under `units/` when needed (skill canopy-definitions). A group that cannot be placed consumes nothing and replaces nothing. A summoned unit with `grants` that carry triggers becomes a holder of those triggers (factories, death bursts).

## resurrect

`{target: caster|point, amount 1–8, corpses {radius, relations, filter?, order}, healthPermille 1–1000, manaPermille 0–1000, placementRadius 0–12, supply: require|allow-over-cap (require), ownership?: original|caster (original), durationTicks?, grants?}`. Raises up to `amount` visible corpses near the anchor with fresh entity IDs, keeping definition, progression and learned ranks (not inventory or statuses). `ownership: caster` + `durationTicks` makes temporary reanimated units (flagged summoned; leave no corpse). Corpses persist 2400 ticks; heroes, summons, mechanical and contained units leave none. `strongest` orders by level, then max health, distance, ID. `allow-over-cap` ignores supply, never the hard unit limit. No `query` or `filter`: the type has a `query` field but the rules reject it; select corpses with `corpses` (and its `filter`).

## revive

Only inside a `death` trigger without `executor`: `{id, target: "caster", amount (delay ticks ≥1), healthPermille 1–1000, manaPermille 0–1000, placementRadius 0–12, placementWaitTicks 0–1200 (400)}`. Schedules the same fallen hero to return at its death point with identity, items and progression. Blocked placement retries until the wait expires, then the hero stays fallen. Owner change cancels it. Pair with trigger `cooldownTicks` for a once-per-N rule. The `query` field parses but is rejected.

## split

`{id, target: "caster", amount (lifetime ticks), members [2–8 ranked unit refs], radius 1–12 (integer), returnHealthPermille?, returnManaPermille?, onAllLost: kill|return (kill)}`. Must be the only `onRelease` operation of a targeted self spell without delivery/channel/persistent. Members must be controllable, zero-supply, non-hero units, all placeable or the cast is rejected. The original unit is hidden and keeps identity, items, progression and supply; on expiry it returns at the first surviving member (list order). Omitted return fractions keep current pools.

## teleport

`{target, query?, filter?, amount (landing search radius 0–12), destination: point|caster|target, preserveOffset}`. Moves each recipient unit to the destination (point needs point aim; target needs unit aim and follows the target's current position) with spatial clearance; `preserveOffset` keeps group formation relative to the caster. Clears movement/attack orders; other units' pending casts are cancelled. Units that cannot land stay. When a caster self-teleport is the first operation, a missing landing rejects the cast before costs.

## vision

`{id, target: point|caster|target, amount (lifetime), radius 1–64, ignoreTerrain, detectInvisible (false), endsWithCaster}`. Creates a fixed sensor at the anchor's position at creation (it does not follow units), sharing team sight and fog exploration. At most 256 active world sensors. Use with `targeting.visible: false` to scout unseen ground. The `query` field parses but is rejected: a vision is one sensor at its anchor.

## convert

`{target, query?, filter?, amount: 1, supply: require|allow-over-cap}`. Permanently transfers a living non-hero unit to the caster's owner, keeping identity, health, items, spells and statuses; orders and casts stop. A player destination requires the unit definition's `playerControl`. Use `controller`/`matches` conditions to restrict neutral levels.

## contain

`{id, target, query?, filter?, shareVision?, amount (max duration), lifetime: duration|untilDeath, capacity 1–8, digestion?: {intervalTicks 1–400, damage, damageType}}`. Holds non-hero units inside the source (untargetable, inactive, keeping owner and supply). `target: "caster"` needs a query (a carrier cannot hold itself). Expiry, carrier death/removal/conversion or `releaseContained` releases survivors nearby. Digestion damage uses the ordinary damage pipeline; digested units leave no corpse. `shareVision` lends the carrier's sight to the passenger's team.

## releaseContained

`{target, query?, filter?, amount: 1}`: the recipient is a carrier; releases all its passengers. For an "unload" button use a self spell with `target: "caster"`.

## sacrifice

`{target, query?, filter?, amount: 1, record?}`: kills an owned, living, non-hero unit other than the caster (same camp for neutral casters), bypassing mitigation but respecting spell immunity; ordinary death, corpse and death triggers follow. Reward with a later operation scaled by its record.
