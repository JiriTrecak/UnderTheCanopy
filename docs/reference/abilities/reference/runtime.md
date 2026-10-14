# Runtime semantics

How the deterministic simulation executes a definition. Authority: `src/sim/abilities/*` (`runtime.ts`, `recipients.ts`, `statuses.ts`, `reactions.ts`, `swarm.ts`, `ai.ts`). The simulation runs at 40 ticks/second. Presentation never changes these outcomes.

## Cast lifecycle (targeted abilities)

1. **Accept**: binding is learned and allows the controller (`player`/`ai`/`scenario`), caster can act, not busy, off cooldown, enough mana, aim eligible (kind, relation, visibility, filter, condition, spell immunity, range) and the first world operation can execute. Mana is reserved. Event `accepted`. A player order out of range first walks into range.
2. **Turn + prepare**: the caster turns to face the aim, then waits `prepareTicks`. Stop, a new order, stun, silence, taunt, displacement, containment, death or owner change cancels: reserved mana is refunded, no cooldown, event `cancelled`.
3. **Release**: everything from step 1 is re-checked (target alive, visible, in range, same relation, filter and condition: a target healed by someone else can become ineligible). Failure cancels with refund. Success spends mana, starts the cooldown, emits `released`, then runs one of the release paths below.
4. **Recover** for `recoverTicks` (after the last wave for channels), then `finished`. Interrupting recovery never refunds or undoes anything.

Release paths:

| Definition shape | What happens at release |
|---|---|
| plain | `onRelease` runs once against the aim. |
| `cast.channel` | `waveStarted`; every `intervalTicks` a `wave` event and one `onRelease` run, `waves` times. The caster must stay put; moving, stun, silence, death or owner change ends remaining waves (unit aims also end on target death, invisibility, ineligibility or leaving `tetherRange`/`range`). Applied waves stay. |
| `delivery` | Launches the delivery (`projectile` event); the caster recovers independently. `onRelease` runs at each impact. |
| `persistent` | `onRelease` runs immediately, then the instance pulses on its own. |
| `weaponCast` | No spell timer: the command issues an ordinary attack; the enhancement, cost and cooldown apply at the weapon's real release (`weaponOrdered`, then `released`/`finished`, or `weaponOrderCancelled`). |

Mana regenerates per tick from `manaRegenPerSecond`; reserved mana occupies pool capacity. Toggle recasts (`persistent.toggle`) end the active instance immediately with no cost.

## Program context: what `caster`, `target` and the aim mean

| Context | caster | aim (`target: "target"`) |
|---|---|---|
| Release / channel wave | casting unit | unit aim, the caster for self aims, or the default area for point aims |
| Projectile impact | caster (captured if it died) | the projectile's target |
| Chain hop | caster | that hop's unit |
| Line | caster | each unit the line crosses |
| Charge contact | caster | the charged unit |
| Swarm bite | caster | the bitten enemy |
| Persistent pulse | caster (captured if gone) | anchor: aim point, caster's current position or target unit; default area around it for point targeting |
| Aura tick | aura holder | each unit in aura radius (caster/query operations run once per tick on the holder) |
| `weaponHit` trigger | attacking holder | the damaged victim |
| `damaged` / `meleeDamaged` | damaged holder | the attacker |
| `weaponRelease` | attacking holder | the victim if still alive and hostile, else its point |
| `kill` | killer holder | a point at the dead victim |
| `death` | dead holder (or the original applier with `executor: statusSource`) | a point at the death position |
| `interval` | timer holder | the holder itself |

## Recipient selection (shared by execution and AI)

For each operation, in order:

1. `target: "point"`: runs once at the aim point (summon, resurrect, vision).
2. `query`: every alive, targetable, non-immune unit (buildings with `includeBuildings`) whose relation is in `query.relations`, within `radius` of the centre, matching the operation `filter`; the caster is excluded unless `allowSelf`, the primary aim excluded with `excludePrimary`. Sorted by health fraction (`lowest-health`) or not, then distance, then entity ID; first `maxTargets`. Centres: `caster` = caster's current position, `target` = aim unit's current position, `point` = captured aim point. Queries do not apply `targeting.filter`/`condition`; repeat them in the operation `filter` if needed.
3. `target: "caster"`: the caster, if it still has its original owner and passes the operation filter and spell immunity.
4. `target: "target"` with a point aim during release/wave/pulse: the **default area**: eligible units by targeting rules (relations, allowSelf, includeBuildings, filter, condition) within `targeting.radius` of the aim, ID order, at most `targeting.maxTargets`. Membership is captured once at program start, so a heal does not drop a recipient from the following ward.
5. `target: "target"` otherwise: the aim unit, if it passes the operation filter.

Each chosen recipient must still be alive, targetable, pass the operation filter and spell immunity when the operation applies.

## Program order and composition

- Operations run in declaration order; each sees the result of the previous ones. Caster position and traits are re-read per operation (a blink followed by a caster-centred query hits the arrival area; a flight form followed by an air-only operation sees the new locomotion). Point centres stay fixed.
- `branch` decides per recipient, once, with a caster snapshot taken at branch entry; the chosen arm runs completely even if it changes the tested resource. A caster operation inside a branch evaluates `target` as the caster.
- Only the first operation (the selected arm's first operation for an initial branch) is checked before costs. Later operations validate when they apply; failures skip that operation without rolling back earlier ones (e.g. convert → sacrifice works, blink with blocked landing still applies earlier effects).
- Programs are not recursive: operations run by triggers cannot proc `weaponHit`/`damaged` triggers. Lifecycle reactions (`death`, `kill`, `weaponRelease`, `interval`) are queued in a saved FIFO: at most 128 run per tick, 512 pending; overflow is dropped deterministically.

## Deliveries

- **Projectile**: launches from the caster and homes on the target's current position and height at `speed`/40 units per tick. It fizzles if the target dies, becomes untargetable, ineligible, immune or changes relation, or the caster's owner changes; it survives caster death. Arrival emits `impact` (origin = launch point), then runs `onRelease`. Lifetime cap 4000 ticks.
- **Chain**: first hit one tick after release. Each hit emits `impact` with the previous hop as origin, runs `onRelease` at `power`, then picks the nearest visible eligible unit within `radius` of that hop that was not hit yet (and is injured with `skipFullHealth`), ID tie-break. Power is multiplied by `retentionPermille` per hop. Ends after `bounces` hits or when no candidate exists. Power scales heal/damage/drain/mana amounts only. `skipFullHealth` filters bounce candidates only; use a targeting condition to reject a full-health first target.
- **Line**: travels from the caster toward the aim for `length` at `speed`; every tick it tests units within `width` of the swept segment (no tunnelling) using the targeting rules; each unit once, at most 128. `onRelease` runs per victim. No `impact` events: bind per-operation events (`damaged`, `statusApplied`). `targeting.radius` is required by the schema but not used by the line.
- **Charge**: moves the caster up to `speed`/40 per tick toward the target body; contact emits `impact` and runs `onRelease`. Obstruction by terrain or units, target lost/invisible/ineligible, caster stun/silence/root/death/owner change, an airborne caster or target, or `maxTicks` stops it with `cancelled`. The caster is busy while charging.
- **Swarm**: `count` seekers launch every `launchIntervalTicks` (each emits `projectile`). A seeker picks a visible eligible enemy within `radius` of the caster (fewest seekers assigned up to `maxPerTarget`, then nearest, ID), bites every `attackIntervalTicks` (`impact` + `onRelease` on that enemy), and returns after `maxHitsPerTrip` bites, when carried health+mana reaches `returnThreshold`, or when no target remains. On reaching the caster it restores carried resources (`healed`/`manaRestored`) and emits `returned`. Drained resources are carried × `returnPermille`. After `durationTicks` no new bites; seekers still out are removed `returnTimeoutTicks` later. Caster death, removal, conversion or containment cancels all seekers. Seekers are untargetable deliveries, not units.
- At most 512 deliveries exist in the world (each seeker counts); casts that would exceed it are rejected.

## Persistent instances

- Created at release (at most 512 world-wide). Each pulse at `intervalTicks` pays `upkeepMana`, emits `wave` (with the targeting radius), and runs `onRelease` at the anchor. The release run is free and has no `wave` event.
- Ends with `finished` (reason) on expiry, toggle off, owner change, caster death/unavailability when `endsWithCaster`, target loss or ineligibility for `anchor: target`, tether break, or unpaid upkeep. Ending removes `lifetime: instance` statuses of that cast.
- Unlike channels, the caster is free to act while an instance runs.

## Auras

Every tick, before combat, each living passive holder applies its aura statuses to units within `aura.radius` (targeting relations, allowSelf, filter, condition, includeBuildings; `meleeOnly` limits to melee units). Aura statuses last one tick and are rebuilt, so leaving range or the source dying removes them immediately. The same aura from several sources does not stack: highest rank, then lowest source ID wins. Aura refreshes emit no events; visuals bind `statusApplied` with `lifetime: status`. Concealment and forms cannot come from auras; for periodic aura-like work use a passive `interval` trigger with a query.

## Statuses

- Refresh: reapplying the same ability+status ID replaces the record (new duration, new periodic clock, new interval-trigger clock). With `stacking.scope: source` each caster keeps its own record.
- Heroes use `heroDuration` when set. Stun and silence cancel an unreleased cast. A unit carries at most 32 spell statuses; further applications fail.
- Removal: duration end, `breakOnDamage` on any damage, `onShieldDepleted: remove`, dispel (dispellable only), death, persistent instance end (`instance`), or never (`untilDeath`, until death).
- Periodic damage is dealt by the original source/owner every `intervalTicks`; spell immunity suppresses it while active.
- Modifiers resolve through the same stat path as equipment (preview `stats` show the result).
- Forms: newest form wins; flight forms take off immediately, grounding searches nearby clear ground and stays airborne until space opens. Contained/garrisoned units cannot change movement.
- Concealment: hidden from enemies `fadeTicks` after application; breaks on attack/cast when configured (cancelled windups do not reveal). Detection needs ordinary sight plus a `detectionRadius` status or a `vision` with `detectInvisible`.
- Taunt: forces hostile combat units to attack the living source while it stays valid; respects perception, roots, stun, disarm, immunity; restores previous orders afterwards.

## Triggers

| Event | Fires when | Queued? |
|---|---|---|
| `weaponHit` | the holder's ordinary weapon deals damage > 0 | no: runs in the next reaction pass |
| `damaged` | the holder takes damage > 0 from any source except reaction programs | no |
| `meleeDamaged` | the holder takes damage > 0 from a melee weapon | no |
| `weaponRelease` | the holder commits an ordinary attack on a hostile unit (counts misses and evasions, not cancelled windups) | FIFO |
| `kill` | the holder lands the lethal hit on a hostile unit while alive and still the damage owner | FIFO |
| `death` | the holder dies (captured before cleanup) | FIFO |
| `interval` | every `intervalTicks`, first one full interval after learning/application | FIFO |

- Holders: a passive trigger (no `whileStatus`) belongs to units that learned the ability. A trigger with `whileStatus` belongs to whoever currently holds that status of this ability, at the status rank, including units the spell created (`grants`) or buffed. This is how one spell gives other units reactions, timers and death effects.
- Qualification order: trigger cooldown, `every` counter, `chancePermille`, `manaCost` (paid from the holder). Filters run before counting. Cooldowns and counters are per holder and trigger.
- `executor: statusSource` runs a death trigger as the original applier (owner, camp, position fallback), even after the applier died or converted.
- `interval` pulses skip while the holder is contained, garrisoned or releasing (no catch-up); `blockedBySilence` skips silenced holders. Status refresh, rank or controller change restart the clock.
- Kill triggers ignore friendly kills, forced removal, summon expiry and mutual deaths. Death operations must use a query or `point` (the holder is dead).
- Queued programs keep their captured source, rank and point. Dispelling the granting status after a proc qualified does not cancel it; the holder changing owner does. If the victim died or became allied, live-target operations are skipped while `caster`, `point` and query operations still run.

## Combat modifiers

- Sources: learned passives (`combatModifiers`) and active statuses. Duplicate grants of one ability/status do not multiply; strongest rank per grant.
- Critical: rolled at weapon release; the strongest successful multiplier applies before armor. Evasion: strongest chance, at direct melee/arrow impact (spells and splash cannot be evaded). Cleave: copies a share of a damaging melee hit to up to `maxTargets` enemies in the forward arc, mitigated separately; cannot chain or proc hit triggers.
- Attack bonus: at a valid release, the strongest affordable matching grant pays `manaCost` once and adds `amount` before critical scaling; unaffordable or silenced (`blockedBySilence`) shots stay ordinary. Its `status` template is applied on a damaging hit. Projectiles keep their captured enhancement after toggle-off or source loss.
- `sourceAttackBonus` marks add to the marking source's later primary attacks only.

## AI and autocast

- AI casts only bindings with an `ai` policy and `ai` control; autocast also requires `ai` control. Each binding is considered every `ai.intervalTicks` (autocast: `autocast.intervalTicks`).
- `ai.intent: utility` (default) scores every in-range eligible aim by predicting `onRelease` on observed copies: healing/damage/drain/mana by actual effect, status ±10 (0 if already present, fully control-immune, or a self buff with no enemies within 24 units), summons/splits only with nearby threats, conversion/containment by victim health, sacrifice as a cost. `preference` (`wounded-ally` or `enemy`) doubles the weight of that side. Teleport and vision score 0 under utility.
- `intent: escape` (point self-teleport), `reinforce` (unit-anchored group teleport) and `scout` (point vision) pick tactical destinations instead; the registry rejects bindings whose spell does not match the intent.
- Prediction runs on copies of observed units in program order: drains credit the caster (clamped), mana grants to enemies count as a cost, mana burn reduces target health before later branch conditions, conversions change later relations, and swarm cargo is not counted as immediate healing. Landing, mitigation and unseen events remain estimates.
- AI does not score persistent pulses, triggers or passive programs beyond the first release.

## Unit bindings (caster policy)

`bind` writes `{maxMana ≥1, manaRegenPerSecond, bindings[1–12]}` on a unit definition. Each binding: `id` (`^[a-z][a-z0-9-]*$`), `ability`, `initialRank` 0–10 (≤ ability ranks; 0 = must be learned), `controls` (unique subset of `player`, `ai`, `scenario`), optional `command {hotkey, column 1–4 unique, icon?}` (requires `player`), optional `learning {requiredLevels}` (hero progression only, one increasing level per rank), optional `ai {intent?, preference, intervalTicks 1–400}` (requires `ai`). Passives need a binding with rank ≥1 to work; they have no button unless a command is given. Hidden AI abilities use the same definitions as player ones. Preserve unrelated bindings and the mana policy when editing.

## Limits

| Limit | Value |
|---|---|
| Statuses per unit | 32 |
| Deliveries in the world (projectiles, chains, lines, charges, seekers) | 512 |
| Persistent instances | 512 |
| Vision sensors | 256 |
| Reaction FIFO | 128 per tick, 512 pending |
| Line victims per cast / area recipients | 128 |
| Summons per operation | 8 |
| Corpse retention / store | 2400 ticks / 512 corpses |
| Per-unit trigger timers, counters, cooldowns | 128 each |

## Out of scope

Terrain or destructible changes, resource/economy rewards, modifying other abilities' costs or cooldowns, charges/shared cooldown groups, temporary possession, siege weapon casts, nested branches, arbitrary expressions or scripts. Do not approximate these with visuals.
