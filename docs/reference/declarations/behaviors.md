# Definitions and behavior reference

## Definition fields

Every definition has `id`, `kind`, `name`, `description`, `asset`, `icon`, and resolved `behaviors`. `body` holds maximum HP, integer armor, and armor type. `vision` is a sight radius in cells. Buildings declare an odd-cell rectangular `footprint` and an entrance offset outside it. Rotation is in degrees; building placement uses quarter turns. Unit speed is cells per second at a fixed 40 Hz simulation.

Items declare `stackLimit`; currencies declare `currency: true` and cannot exist as ground pickups. Resource sources declare `yield` and optionally `regrowthTicks`; a building can also have yield, and `harvesting` declares active extraction slots, recommended staffing and local search radius (assignments are uncapped). `hero` is metadata on units only; inventory, progression and abilities are separate explicit capabilities. `selectable` controls selection; `selectionClass` selects army or worker priority for area selection.

The `asset` record selects a project-relative model file, optional carry model, scale, HP-bar height, and (for batched resources) an explicit scenery catalogue asset. `icon` selects an atlas entry. Model and icon roles are validated; build/content-save also check model files and scenery catalogue links. Resource models are batched through the scenery adapter. Asset names never confer gameplay capabilities.

## Composed capabilities

- `movement { speed }`: participates in native navigation and movement. Path search and replanning are not content options.
- `playerControl {}`: permits owner-issued orders. Ownership alone does not grant control. Removing this also removes direct command bindings.
- `combat { damage, damageType, range, cooldownTicks, aggroRange }`: unit auto-acquisition and combat. Every armor type must be covered by every damage row in `rules.damageMultipliers`.
- `work { carryCapacity, builds, harvests }`: mobile workers can carry goods and perform native work. `builds` lists constructible definition IDs. `harvests` lists direct gathering recipes. Employment is runtime state; builder/carrier/forester are jobs, not unit definitions.
- `storage { capacity, accepts, dropoff? }`: capacity and accepted currency IDs. `dropoff: true` makes a completed owned store a harvest destination and spendable bank. A producer holds reserved costs, not a relay economy.
- `production { mode, outputs, workerSlots, ... }`: automatic single-output production or a queued producer. `workerSlots` is currently zero or one. Queued production declares `queueCapacity`; external work declares `workRadius`. `jobName` labels employment in the UI.
- `progression { levels, experienceRadius }`: explicit level records containing cumulative `experience`, `maxHp`, `damage`, `armor`, `cooldownTicks`, `maxMana`, `healthRegenPerSecond` and `manaRegenPerSecond`. The first record must agree with the body's/combat's/spellcaster's level-one fields. XP starts at zero and strictly increases; HP and mana pools cannot shrink. There is one representation, with no STR/AGI/INT or old per-level increment fields.
- `abilities { bindings, maxMana, manaRegenPerSecond }`: stable binding IDs reference published ability definitions; control permissions, command placement, learning and AI policy belong to each binding. See [Spell Editor](../expansion/spell-effects.md). Progression overrides pool/rate at later levels. Regeneration rates have millipoint-per-second precision. Native recovery retains integer remainders across fixed ticks and saves.
- `inventory { slots, pickupRange }`: hero equipment/pickup participation. Items modify resolved stats before percentage attack buffs. Death retains slots and contents.
- `revival { items, itemsPerLevel, workTicks, workTicksPerLevel, queueCapacity }`: paid building queue returning fallen owned heroes with their identity, experience and items. Level one uses the base price/time; each additional level adds the declared increments. The accepted level is saved on the queue. Cancellation refunds the paid price; destruction loses it.
- `campDefense {}`: mobile combat unit participates in its authored neutral camp's aggression/leash policy. The map must supply a camp.

Behavior sets are flat bundles. Arrays replace; known object fields merge. Conflicting set defaults require an explicit field override. `disabledBehaviors` removes a capability after expansion. Sets cannot include other sets, scripts, formulas, or engine callbacks.

## Creation belongs to the output

All creation records contain `method`, `items`, and `workTicks`. The output definition owns its price; producers only enumerate output IDs.

- `construct`: building, full currency bill reserved from owned banks, then physical builder work.
- `train`: unit, currency bill and work time. A queued producer reserves the bill and declared `supplyCost`; completion deploys a new entity without consuming a worker.
- `harvest`: currency with a `source` yielding resource or building definition; a directly assigned worker removes finite yield and carries it to an owned drop-off. `amount` is the number gathered per completed `workTicks` cycle, capped by reserved load space and remaining source yield. Built-in workers carry ten; amber gathers ten in 100 ticks and wood ten in 400 ticks. Travel is additional.
- `plant`: resource with regrowth duration; one employed worker restores an eligible depleted site.

Unit definitions declare `supplyCost` (nonnegative integer); only building definitions can declare `supplyProvided`. Supply capacity is capped by `rules.maxSupply`. See [economy](../game/economy.md#supply-and-training) for queue reservations and capacity-loss rules.

These are native verbs. Adding a new building that uses an existing verb is content work. Adding a new verb is an explicit system change, with schema validation, state, lifecycle rules, snapshot support, and acceptance tests.

## Actions and command cards

Action metadata defines name, description, icon, priority and optional hotkey. Targeted build/produce bindings use the output's name, description, icon and price. Overrides keyed by `build:<definition>` or `produce:<definition>` may change priority/hotkey/category.

Bindings sort by descending priority, then ordinal binding ID. The adapter fills top-left first, across four columns and three rows, then another page. Disabled bindings retain position. Move/Attack/Stop shortcuts are semantic; target-specific shortcuts apply to the visible page. Duplicate shortcuts fail validation.

A focused building shows that building's workplace actions. Mixed units contribute only eligible actors to each binding; area selection prefers owned army units over workers. Unknown/enemy/remembered entities provide inspection data without private command cards. Queue cancellation is also a projected permission: removing playerControl leaves an owned queue inspectable but cannot leave active cancel buttons behind.

## Command categories

`actions.categories` declares named menus keyed by persistent IDs. Each has `name`, `description`, `icon`, `priority`, optional `hotkey`, and optional `parent` category ID. Menus appear only when eligible selected actors have commands within them (including descendants). A category does not grant a capability or unlock a building.

An optional `category` on action metadata supplies the default grouping. An output definition's optional `category` overrides it for build/produce commands. A binding override's `category` takes precedence over both; explicit `null` there puts the command at the root. Ungrouped commands appear at the root. This is presentation metadata, independent of construction/production mechanics.

The initial worker card uses Build (B) for economy buildings and Advanced Build (V) for Barracks, Watchtower and Sanctuary. Build is the action default; advanced definitions override it. These are sibling menus; assigning `parent` creates a submenu without changing renderer or simulation code. Registry validation rejects unknown categories, cycles, invalid icons and duplicate declared shortcuts.

`commandCard` discovers executable bindings; `commandMenu` projects those into the current category and local navigation entries. Opening a category and Back never enter the multiplayer command queue. Higher priority still fills from the top-left. Each submenu page reserves bottom-left for Back, leaving eleven content slots. Empty/stale categories return to the root. Selection changes reset navigation; regular simulation updates preserve it. Escape cancels targeting first, then returns to the parent. Move/Attack/Stop shortcuts remain usable within categories; other shortcuts operate on the visible page only.

Back metadata (name, description, icon, priority and shortcut) lives in actions.navigation.back. Keyboard dispatch and tooltip text consume the same binding, including Escape. Commands fill slots 1–12 left-to-right; submenu content skips reserved slot 9. Escape while actively targeting remains a local targeting-cancel control.

## Damage and spell rules

`rules.damageTypes` declares display names and whether armor points apply. `damageMultipliers` must have exactly one row per attack type and a column for every armor class. Values are permille; zero is immunity. `armorCoefficient` supplies the positive-armor percentage curve. `heroStunDurationPermille` scales unit stuns against heroes; buildings never receive stun effects.

Ability ranks use `damageBonusPermille` for percentage attack buffs, not flat bonuses. Identical buffs refresh instead of stacking; distinct percentage bonuses add before one multiplication of equipped base damage. `reductionPermille` uses the strongest active protection. Offensive area abilities may declare `damageTargetBudget`: raw damage is scaled by `min(1, budget / eligible targets)` before individual defenses. Immune targets do not consume this budget. Damage immunity alone does not imply stun immunity.

Harvest recipes may declare `impactTick` for sources with a `felling` policy. This is one authoritative hit per work cycle, not a general animation-event scripting system. Felling yields one complete load; the loader requires sufficient worker capacity and matching recipe/source amounts. `felling.maxHp` is chopping durability, separate from combat armor/health. Workers still need only their existing `work.harvests` capability.
