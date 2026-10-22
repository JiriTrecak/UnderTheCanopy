---
name: canopy-definitions
description: The fields of Under the Canopy's game definitions (units, heroes, buildings, items, resources) and of races, research, loot pools, behaviour sets and rules, with working examples. Use when designing, balancing or adding gameplay content in base/ or in a map or mod package.
---

# Definitions and rules

Read the skill **canopy-packages** first for files, ids and layering. The Rust types are the authority:
- definitions, rules, races, research, loot: `crates/content/src/game.rs`;
- items: `items.rs`;
- abilities: `abilities/` (skill **canopy-abilities**).

Cross-reference rules live in `crates/content/src/registry.rs`. When unsure of a field, read the type: unknown fields are errors.

**Units:** ticks are authored in 25 ms ticks (`*Ticks`, 40 per second). Distances are in cells. `*Permille` values are thousandths.

## A unit

`units/<free folders>/<name>.json`; the kind comes from the folder.

```json unit
{
  "id": "unit.example.spearman",
  "name": "Spearman",
  "description": "Cheap infantry with a long reach.",
  "model": "asset.models.units.ants-cleaver-warrior",
  "icon": "asset.portraits.units.ants-cleaver-warrior",
  "selectionClass": "army",
  "body": {"maxHp": 260, "armor": 1, "armorType": "heavy"},
  "vision": 30,
  "behaviorSets": ["behavior-set.ground-army"],
  "behaviors": {
    "combat": {
      "damage": 12, "damageType": "piercing", "range": 1.2, "cooldownTicks": 60, "aggroRange": 20,
      "attack": {"windupTicks": 14, "recoveryTicks": 10, "rangeBuffer": 0.6}
    },
    "movement": {"speed": 11}
  },
  "creation": {"method": "train", "items": [{"item": "item.amber", "amount": 110}], "workTicks": 700},
  "level": 1,
  "supplyCost": 1,
  "dimensions": {"radius": 1.5, "height": 4.8, "formationSpacing": 3.25},
  "modelScale": 2.4,
  "presentation": {"healthHeight": 2.4}
}
```

- **Required:** `model` (an asset or scenery id: what it is drawn with), `icon` (an icon asset), `body`, `dimensions`.
- **Unit-only:** `supplyCost`; `maxOwned` (at most that many living or in training per player: one Juggernaut at a time).
- **`bounty`** (units and buildings): `[{"item": "item.amber", "amount": 75}]`, currencies its killer's owner gains when an opponent kills it (Warcraft III's bounty; the beetles' Supply Cart spills its amber), rising as "+N" where it fell.
- **`presentation`** says how *this* unit shows its model:
  - `projectile` and `projectileSocket` (the missile's visual effect and the model node it leaves from);
  - `launchEffect`: a full effect played as each missile or shell leaves, from the projectile socket to the target and lasting the flight: muzzle flash, steam, a `beam` to the target, and the shot's trail: a `missile` layer on the projectile's arc (a glowing head, or invisible with `intensity: 0`) that particle and ribbon layers `ride` in world space (D77; builders in `tools/content/vfx_layers.py`, shared flights `effect.weapons.*-flight`). A magical shot may drop `projectile` and let the launch effect's glowing head be the shot;
  - `impactEffect` (where its shells and missiles land, and where its melee blows strike);
  - `castContact` (share of the cast animation at which a spell lands);
  - `cameraAnchor`;
  - and, when they differ from the model's own `display`, `healthHeight` and `character` (animation profile);
  - `effects`, `[{effect, state | event, socket}]` (D65): an effect looping from a model socket while the building or unit is in `state`, or played once whenever its clip crosses `event` (a clip event of the model's `capabilities.animations`). `socket` is a declared socket name or a model node named `socket*`. Looping effects follow their socket as the model moves.
    - Building states: `work` (it trains, researches, upgrades, revives, earns its `income`, or workers gather from it), `spawn` (once per trained unit), `attack` (a tower's model plays its attack clip, once per shot of its crew), `idle` (resting, complete), `always` (complete, in sight). Example: `base/buildings/beetles/shell-forge.json`.
    - Unit states: `move` (walking, running, carrying or charging), `attack`, `cast` (any cast), `idle`, `always`, or any engine animation state (`walk`, `run`, `charge`, `hit`, `carry`, `carry_walk`, `carry_run`, `build`, `chop`).
    - A unit's `activeGlow` materials light while it moves, fights, casts or works.

```jsonc
"presentation": {"effects": [
  {"effect": "effect.example.muzzle-blast", "event": "release", "socket": "socket_muzzle"},
  {"effect": "effect.example.footstep-dust", "event": "step", "socket": "socket_foot_fl"},
  {"effect": "effect.example.exhaust", "state": "move", "socket": "socket_steam"}
]}
```
- **Heroes:** `"hero": true`, `supplyCost` 4, `behaviors.progression` (levels) and `behaviors.abilities` (bindings). Keep them under `units/<race>/heroes/`.
- **Size** (`modelScale`, with `dimensions` to match: the selection circle and health bar follow them, D58). A regular infantry unit such as the Warrior is 1×; heroes and large tier-3 units about 2×. Neutral creeps grow with their `level`: size (the average of rendered height and footprint) is the Warrior's × (1 + 1.4 × (level − 1) / 8), so level 1 is Warrior-sized and level 9 is 2.4×; body radius 1.5 × that factor (at most 3.0), formation spacing 13/6 of the radius, height the rendered height. Measure a model with `just cook --inspect <asset id>` (its rest bounds).
- **Behaviours** (all optional): `movement`, `combat` (with `projectile`, `shell` or `charge`), `work` (workers: `carryCapacity`, `builds`, `harvests`), `storage`, `production`, `research`, `inventory`, `progression`, `abilities`, `revival`, `playerControl`, `campDefense`.
- **`combat` extras:** `chain {bounces 1–8, radius 1–32, retentionPermille}` makes each missile jump on to the nearest other visible enemy it has not hit, keeping that share of the damage (arcs); `minRange` (body edges) is the reach inside which the weapon cannot fire (siege mortars); a unit then picks targets further out.

## A building

```json building
{
  "id": "building.example.lodge",
  "name": "Lodge",
  "description": "Trains infantry.",
  "model": "asset.models.buildings.ants-leaf-garrison",
  "icon": "asset.portraits.buildings.ants-leaf-garrison",
  "body": {"maxHp": 1200, "armor": 5, "armorType": "structure"},
  "vision": 14,
  "footprint": {"width": 12, "depth": 12},
  "entrance": {"x": 0.5, "y": 6.5},
  "creation": {"method": "construct", "items": [{"item": "item.amber", "amount": 200}], "workTicks": 2000, "workAnimation": "build"},
  "behaviorSets": ["behavior-set.player-building"],
  "behaviors": {
    "production": {"mode": "queued", "outputs": ["unit.example.spearman"], "workerSlots": 0, "queueCapacity": 5}
  },
  "level": 1,
  "modelScale": 1.25,
  "command": {"category": "category.build", "priority": 80}
}
```

- **Footprints** are whole building cells: multiples of 4 navigation cells.
- **`command`** places the building on the workers' command card.
- **`supplyProvided`** is building-only.
- **`upgrade`** turns one building into another; both need the same behaviours and footprint. While it upgrades, nothing queued there advances.
- **One queue per building** (D73): queued training, research and hero revivals at one building are one queue in the order the player queued them, as in Warcraft III; only the first advances. A building that trains and researches stops training while it researches, so research that should not hold up an army belongs in a building of its own. Capacities stay per kind: training and revivals count together against their `queueCapacity`, research against `research.queueCapacity`. Automatic production (`mode: "automatic"`) is not queued and runs beside it.
- **`garrison.crew`** (a unit id) staffs a lookout with its own gunner: whenever the building stands complete, that unit stands in it, shoots from it and takes no orders; it goes with the building, and an upgrade to a tower with another crew swaps it. The crew is a ranged unit with no `playerControl` and no supply (`selectable: false` keeps it out of selections).
- **`placementNear`** (`{source, radius}`) keeps a building within `radius` cells of a standing resource of `source`.
- **`placementOn`** (`{source}`) places a building exactly on a standing resource of `source`, as StarCraft's refinery on its geyser (D62): the placement snaps onto it, the footprints must be equal, and when the building falls the resource is left intact. The base game's Substance Wells (`building.<race>.substance-well`) stand on Substance Deposits this way and pump Substance by themselves (`income`, below); no worker gathers there. A building placed on a resource still obeys `power` if it declares it; the beetles' well declares none and is a network source instead.
- **`income`** (`{item, amount, intervalTicks}`) makes a building earn by itself: while it stands complete and alive, held by a player, and powered if it declares `power` (D66), it adds `amount` of `item` (a currency) to its owner's stock every `intervalTicks`. An unpowered building holds its count and goes on where it stopped. It works (D65 `work` state) for as long as it earns, its card says what it makes, and its owner sees each payment rise as "+N" above it. The base wells make 10 Substance every 200 ticks (120 a minute):

```json building
{
  "id": "building.example.well",
  "name": "Well",
  "description": "Built on a deposit; pumps Substance by itself.",
  "model": "asset.models.buildings.ants-rootworks",
  "icon": "asset.portraits.buildings.ants-rootworks",
  "body": {"maxHp": 1000, "armor": 5, "armorType": "structure"},
  "footprint": {"width": 8, "depth": 8},
  "entrance": {"x": 0.5, "y": 4.5},
  "creation": {"method": "construct", "items": [{"item": "item.amber", "amount": 150}], "workTicks": 1600, "workAnimation": "build"},
  "behaviorSets": ["behavior-set.player-building"],
  "behaviors": {"storage": {"capacity": 100000, "accepts": ["item.root"], "dropoff": true}},
  "placementOn": {"source": "building.neutral.corrupted-root"},
  "income": {"item": "item.root", "amount": 10, "intervalTicks": 200}
}
```

## A deposit

A harvested building (a mine, a deposit) has a `yield` and a `harvesting` policy: `activeWorkers` gather at once, `recommendedWorkers` saturate it, `searchRadius` is how far gatherers look for another of its kind. `needsBuilding: true` lets workers gather only through a finished building placed on it (`placementOn`), and only its owner's; `endless: true` never runs out (loads leave the yield as it is). Which workers gather a deposit is the workers' `work.harvests`: the base game's workers harvest Amber only, so the Substance Deposit (`building.neutral.corrupted-root`, needs a building, endless) takes no gatherers (`recommendedWorkers: 0`) and pays only through its well's `income`. A deposit some worker harvests looks like this:

```json building
{
  "id": "building.example.deposit",
  "name": "Deposit",
  "description": "Never runs out; build a well on it.",
  "model": "asset.models.resources.corrupted-root",
  "icon": "asset.icons.resource-root",
  "footprint": {"width": 8, "depth": 8},
  "entrance": {"x": 0.5, "y": 4.5},
  "constructionClearance": 3,
  "behaviors": {},
  "yield": 1500,
  "body": {"maxHp": 1500, "armor": 5, "armorType": "structure"},
  "gatheringUnitCollision": false,
  "harvesting": {"activeWorkers": 2, "recommendedWorkers": 2, "searchRadius": 0, "needsBuilding": true, "endless": true}
}
```

### The power network (D66)

StarCraft's pylon power, as content: a race whose buildings need power places them only inside its own network, and they pause while it does not reach them.

```jsonc
"network": {"role": "source", "radius": 32}   // a main hall (every tier)
"network": {"role": "source", "radius": 20}   // the beetles' Substance Well: no power, it goes on its deposit
"network": {"role": "node", "radius": 28}     // the Brood Den
"power": "required"                           // placed only inside the network; work pauses outside it
```

- **`network.role`**: a `source` powers the disc of `radius` cells around its centre whenever it stands complete. A `node` powers its disc only while its own centre is powered (by a source or a powered node), so the network grows outward in chains, and losing a node unpowers everything that hung on it.
- **`power: "required"`**: the building's centre must be inside its owner's network to place it ("Build inside your network: extend it with a Brood Den", naming the race's cheapest node). Once complete and outside the network it stops working: training, research, upgrades and revival hold their progress, and a lookout's garrison holds its fire; it stands, keeps its supply, and resumes when power returns. Construction goes on regardless; a site neither powers nor needs power. A node usually needs power too (so it can only be placed in the chain); a source never does.
- **Research** widens discs: `{"units": ["building.beetles.bastion", "building.beetles.brood-den"], "networkRadius": 8}` adds 8 cells to the listed buildings' radius. Such an effect lists only buildings with a `network` and changes nothing else.
- A race with buildings that need power must have a source in its roster. Players see the network as a blue-violet field on the ground while placing a building that needs power (and with the debug overlay's **Show power network**); an unpowered building goes dark and shows a struck-bolt badge.

## Neutral map features (crates, rubble, springs, shops)

Warcraft III's destructibles, fountains and Goblin Merchant, as neutral buildings a map places (`owner: "none"`, `crates/sim/src/neutrals.rs`). None of them is ever built (no `creation`), and a building is at most one of the three.

- **`breakable`** (crates, barrels, rubble): no unit attacks it by itself; a right-click (a forced attack) breaks it, and when it breaks it drops what it holds as items on the ground. What it holds is fixed when the match starts: the placement's `initialState.inventory` (items and counts), else a roll of the placement's `initialState.lootPool`, else of the definition's `lootPool`, else the definition's `drops`. Only hero items. Its footprint blocks the ground until it breaks, so a wide breakable (rubble, 12 × 4) closes a path; quarter turns keep a footprint like 12 × 4 or 4 × 4 on the building lattice. Use `armorType` light for crates (two blows) and structure for rubble.
- **`restoration`** (springs): every `intervalTicks`, every living unit of any owner within `radius` cells of its footprint regains `health` + `healthPermille` of its maximum health and `mana` + `manaPermille` of its maximum mana. A spring and a shop may have no `body`: nothing can attack them.
- **`shop`**: a player buys a ware for their nearest hero (a unit with an inventory) within `range` cells of the footprint, paying `price` from their stock; the item goes into a free slot (a power-up is used at once). Each ware starts with `stock` and grows back one every `restockTicks` after a sale. Selecting the shop shows its wares on the command card. A placement may sell other wares: `initialState.wares`, the same list.

```json building
{
  "id": "building.example.crate",
  "name": "Supply Crate",
  "description": "Break it open to see what is inside.",
  "model": "asset.models.props.neutral-supply-crate",
  "icon": "asset.portraits.buildings.neutral-supply-crate",
  "body": {"maxHp": 40, "armor": 0, "armorType": "light"},
  "footprint": {"width": 4, "depth": 4},
  "entrance": {"x": 0.5, "y": 2.5},
  "behaviors": {},
  "breakable": {"lootPool": "loot.camp.easy"}
}
```

```json building
{
  "id": "building.example.spring",
  "name": "Sap Spring",
  "description": "Restores the health of every unit near it.",
  "model": "asset.models.props.neutral-sap-spring",
  "icon": "asset.portraits.buildings.neutral-sap-spring",
  "footprint": {"width": 12, "depth": 12},
  "entrance": {"x": 0.5, "y": 6.5},
  "behaviors": {},
  "restoration": {"radius": 8, "intervalTicks": 40, "health": 3, "healthPermille": 15}
}
```

```json building
{
  "id": "building.example.merchant",
  "name": "Merchant",
  "description": "Sells items to heroes standing near.",
  "model": "asset.models.props.neutral-merchant",
  "icon": "asset.portraits.buildings.neutral-merchant",
  "footprint": {"width": 12, "depth": 12},
  "entrance": {"x": 0.5, "y": 6.5},
  "behaviors": {},
  "shop": {"range": 6, "wares": [
    {"item": "item.resin-salve", "price": [{"item": "item.amber", "amount": 75}], "stock": 3, "restockTicks": 2400}
  ]}
}
```

## An item

Items placed on a map (`entities`, owner `none`) lie on the ground until a hero picks them up, like dropped loot.

On the ground an item shows its `model` (the base game's items all lie as the loot sack, `asset.models.props.neutral-loot-bundle`) and plays the rules' `itemEffects` (`rules/game.json`: `[{effect, state: "always"}]`, the gold loot glint, D90) unless its own `presentation.effects` declares item effects (state `always`). Currencies never take the rules' effects.

```json item
{
  "id": "item.example.ward",
  "itemTier": 1,
  "name": "Bark Ward",
  "description": "+2 armor.",
  "model": "asset.placeholder.missing-model",
  "icon": "asset.icons.item-barkguard",
  "stackLimit": 1,
  "behaviors": {},
  "itemEffect": {"type": "equipment", "damage": 0, "armor": 2, "maxHp": 0}
}
```

Goods the economy counts are items with `"currency": true` and a `harvest` creation. The base game has two (D62): Amber (`item.amber`, finite mines at the base, carried in by workers) and Substance (`item.root`, endless deposits out in the world, pumped by the wells on them). An item's `presentation.color` (`"#rrggbb"`) is the colour its amounts rise in where they reach a player's stock (Amber gold, Substance blue-violet); without one they rise in the label cream. Trees are no resource in the base game; the engine still fells and plants harvestable trees for content that wants them.

## Race, research, loot pool, behaviour set

`races/<id>.json`: the roster (every unit and building the race may own) and its starting setup. `rules/game.json` → `defaultRace` names the race of seats that choose none.

```json race
{
  "id": "example",
  "name": "Example Folk",
  "description": "A test race.",
  "construction": {"scaffold": "asset.models.construction.ants", "padding": 0.4, "revealAt": 0.95, "retractUntil": 0.35, "dustEffect": "effect.construction.dust", "dustIntervalTicks": 20},
  "startingSetup": {
    "id": "setup.example",
    "fort": "building.example.lodge",
    "inventory": {"item.amber": 500},
    "units": [{"definition": "unit.example.spearman", "offset": {"x": 0, "y": 14}}]
  },
  "roster": ["unit.example.spearman", "building.example.lodge"]
}
```

```json research
{
  "id": "research.example.sharper-spears",
  "name": "Sharper Spears",
  "description": "Spearmen deal 25% more damage.",
  "icon": "asset.portraits.units.ants-archer",
  "priority": 80,
  "items": [{"item": "item.amber", "amount": 150}],
  "workTicks": 1600,
  "effects": [{"units": ["unit.example.spearman"], "damagePermille": 250}]
}
```

Research `requires` may name buildings and earlier research (levels I–III: II requires I and the tier-2 hall). Effect fields, each for the listed `units`: `maxHp`, `armor`, `damagePermille`, `attackSpeedPermille`, `moveSpeedPermille`, `splashRadius` and `splashSlowPermille` (shells), `chargeCooldownPermille` (charges), `treeWorkRate` (workers), `abilityRanks` (`{bindingId: rank}`: raises that binding to at least the rank, on living and future units — 1 unlocks an ability trained at rank 0), and `networkRadius` (for buildings with a `network`).

```json loot-pool
{
  "id": "loot.example.small",
  "rolls": 1,
  "maxTier": 1,
  "entries": [{"item": "item.example.ward", "weight": 10}, {"item": null, "weight": 5}]
}
```

```json behavior
{
  "id": "behavior-set.example-army",
  "behaviors": {"movement": {"speed": 4, "turnRate": 720}, "playerControl": {}}
}
```

## Combat numbers

`rules/combat.json` → `damageMultipliers[damageType][armorType]` is in thousandths: 1500 is +50%. Armour reduces damage by the Warcraft III formula with `armorCoefficient` (0.06): `armor × c / (1 + armor × c)`.

## Checking

```bash
cargo test -q -p canopy-content --test shipped_data
```

Each definition is checked against every cross-reference rule:
- references resolve to the right kind;
- the roster covers everything a race can build, train or research;
- workers can build their buildings;
- heroes are controllable with supply 4.
