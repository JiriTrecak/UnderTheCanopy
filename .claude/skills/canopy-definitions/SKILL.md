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
- **Unit-only:** `supplyCost`.
- **`presentation`** says how *this* unit shows its model:
  - `projectile` and `projectileSocket` (the missile's visual effect and the model node it leaves from);
  - `impactEffect` (where shells land);
  - `castContact` (share of the cast animation at which a spell lands);
  - `cameraAnchor`;
  - and, when they differ from the model's own `display`, `healthHeight` and `character` (animation profile).
- **Heroes:** `"hero": true`, `supplyCost` 4, `behaviors.progression` (levels) and `behaviors.abilities` (bindings). Keep them under `units/<race>/heroes/`.
- **Behaviours** (all optional): `movement`, `combat` (with `projectile`, `shell` or `charge`), `work` (workers: `carryCapacity`, `builds`, `harvests`), `storage`, `production`, `research`, `inventory`, `progression`, `abilities`, `revival`, `playerControl`, `campDefense`.

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
- **`upgrade`** turns one building into another; both need the same behaviours and footprint.

## An item

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

Goods the economy counts (wood, amber, root) are items with `"currency": true` and a `harvest` creation.

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
