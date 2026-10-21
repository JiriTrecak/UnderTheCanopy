---
name: canopy-packages
description: How Under the Canopy's content is organised into packages (the base game, campaigns, maps, mods), how packages layer and override each other, and how to find, add, change or remove any element. Use before touching anything under base/ or a .utcm/.utcc package.
---

# Content packages

All game content is in **packages** (docs/packages.md, decision D53). The shipped game is the **base package**, `base/` in the repository. A map (`<id>.utcm`), a campaign (`<id>.utcc`) and, later, a mod are packages too. Every package has the same tree; kinds differ only in their manifest and a few extra files.

## The tree

```
base/
├─ package.json      manifest (below)
├─ rules/            game.json, combat.json, experience.json, commands.json (fixed names)
├─ races/  behaviors/  research/  loot-pools/
├─ units/  buildings/  items/  resources/      one definition per file
├─ abilities/  effects/                        one file each (skill canopy-abilities)
├─ scenery/                                    placeable scenery pieces
├─ assets/                                     one folder per asset (skill canopy-assets)
├─ landscape/  biomes/  water/
├─ campaigns/<id>.utcc/                        nested campaign packages: campaign.json, missions/, assets/
└─ maps/<group>/<id>.utcm/                     nested map packages (skill canopy-maps)
```

- **The top-level folder gives the type.** Everything below it is free organisation, read recursively, with no meaning to the game: `units/ants/heroes/marshal.json` and `units/marshal.json` behave the same.
- **The id is inside the file** (`"id": "unit.ants.marshal"`), never taken from the path. File names follow the last part of the id by convention. Moving or renaming a file breaks nothing.
- **Definition ids start with their kind**: `unit.`, `building.`, `item.`, `resource.`; the loader checks this. Units, buildings, items and resources share one id space.
- Hidden files are skipped. An empty folder keeps a `.folder` marker so git retains it.
- Two files with the same id in one package is an error.

## The manifest

```json package
{
  "format": 1,
  "id": "frost-tuning",
  "kind": "mod",
  "name": "Frost tuning",
  "version": "1.0.0",
  "description": "Hardier winter units.",
  "dependencies": {"base": ">=0.3.5"}
}
```

`kind` is `base`, `campaign`, `map` or `mod`. Only the first loaded package may be the base.

## Layering: how packages override

Packages load in order: base, then mods (in the player's order), then the campaign, then the map, always last. A package loads after everything it depends on.

- **The same id in a later package overrides it.** The later file holds only what it changes. It is merged in: objects merge field by field; everything else replaces whole, **lists included**; `null` removes a field.

  ```jsonc
  // maps/frost-isle.utcm/units/warrior.json: warriors on this map only
  {"id": "unit.ants.warrior", "body": {"maxHp": 450}}
  ```

- **`{"id": "unit.ants.bombardier", "removed": true}`** deletes an element from that layer on. A later package may add it again.
- **`"base": "<id>"`** starts a new element as a copy of another one in the same collection. The new element's own fields merge on top. A base is resolved *after* layering, so a map's change to the base also reaches elements based on it. Chains are allowed; cycles are errors.

  ```jsonc
  {"id": "unit.ants.elite-warrior", "base": "unit.ants.warrior", "name": "Elite Warrior", "body": {"maxHp": 500}}
  ```

- No namespaces: two packages using one id resolve as an override, and the later one wins.

Behaviour sets (`behaviors/`, `"behaviorSets": [...]` on a definition) are a separate, older mechanism. They resolve after layering and base, merging one level deep inside each behaviour. Lists replace there too.

## Rules files

`rules/` holds four documents with fixed names:
- `game.json`: caps, supply, construction, repair, `defaultRace`, `supplyIcon`.
- `combat.json`: armour types, damage types, the damage table, the armour formula.
- `experience.json`: experience tables.
- `commands.json`: command-card actions, categories, navigation.

A building's command-card override lives in the building, as `command` (`{priority, category, hidden, hotkey}`).

**AI behaviour is code**, never data (D54). The AI recruits from what a race's buildings train.

## Checking your work

```bash
cargo test -q -p canopy-content --test shipped_data   # loads and validates everything shipped
just check                                            # the gate before every commit
```

Errors name the package and file, e.g. `base/units/ants/warrior.json · body.maxHp: invalid type`.

## Identity

A package's identity is the SHA-256 of its `.json`, `.lua` and `.grid` files (`Package::digest`). A match, a save and a lobby agree on the content when these digests agree. Art files never change a match, so they are not hashed.

## Where packages live

- **Base:** `base/` in the repository. A packaged game ships its documents (staged by `just cook` into `target/cooked/base`) beside the pack.
- **Player maps:** `<user folder>/maps/` (the OS data folder, `com.jt.canopy`), plus `downloads/` for maps received from others.
- **Zips:** a package as a zip of the same tree is planned with multiplayer map transfer. Today packages are folders.
