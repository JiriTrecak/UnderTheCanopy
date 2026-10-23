---
name: canopy-abilities
description: Authoring or changing Under the Canopy abilities (spells, passives, auras, triggers, summons, transformations) and their visual effects, in the base package or in a map or mod package. Use before editing anything under abilities/ or effects/, or a unit's behaviors.abilities bindings.
---

# Abilities and effects

Read the skill **canopy-packages** first for packages, ids and layering. The Rust types are the authority; every struct rejects unknown fields:

- ability shape: `crates/content/src/abilities/definition.rs` (`Ability`, `Presentation`, `AbilityCaster`, `AbilityBinding`), `operations.rs` (operations, statuses), `amount.rs`, and `crates/content/src/conditions.rs` (filters, conditions);
- ability rules: `abilities/rules.rs` (each ability on its own), `abilities/references.rs` (bindings, damage types, summons, icons), `abilities/publication.rs` (bound effects and their asset files exist);
- effects: `crates/content/src/effects/mod.rs` (`VisualEffect`, layers, `EffectBinding`, events) and `effects/rules.rs`;
- runtime: `crates/sim/src/abilities/`.

An ability's name and description are in the package's text library (D96, skill **canopy-packages**): `<ability id>.name`, `<ability id>.description`. Effects keep their own `name` (authoring only).

When this skill and the code disagree, the code wins; fix the skill.

## Where the files are

- **One ability per file**: `abilities/<free folders>/<name>.json`. **One effect per file**: `effects/<free folders>/<name>.json`. Folders are organisation only, by owner: `abilities/heroes/marshal/rally.json`, `effects/heroes/marshal/rally-cast.json`.
- **The id is inside the file**; the loader never reads ids from paths. Abilities are `ability.<scope>.<name>` by convention; effect ids must start with `effect.` (`effect.<scope>.<name>[.<part>]`). File names follow the end of the id.
- To find an ability, search its id (`rg -l '"id": "ability.marshal.rally"' base/abilities`) or the unit that binds it (`rg ability.marshal.rally base/units`).
- A map or mod package adds or overrides abilities and effects the same way, under its own `abilities/` and `effects/`. An override holds only the fields it changes, but **lists replace whole**: to change one operation of `onRelease` or one binding of `presentation.effects`, restate the whole list.
- There is no `schemaVersion`, no separate presentation document or presentation id, no `published.json` and no folder registry.

## An ability file

The definition and its presentation are one document. `presentation` holds the command icon (an asset id), the caster's animations and the effects bound to simulation events:

```json ability
{
  "id": "ability.example.fireball",
  "activation": "targeted",
  "delivery": {"kind": "projectile", "speed": 22},
  "ranks": [{"damage": 80, "splash": 30}, {"damage": 120, "splash": 50}],
  "targeting": {"kind": "unit", "relations": ["enemy"], "allowSelf": false, "alive": true, "visible": true, "range": 16, "outOfRange": "reject"},
  "cast": {
    "prepareTicks": 12, "recoverTicks": 8,
    "cost": {"resource": "mana", "amount": 60, "commit": "release"},
    "cooldown": {"ticks": 320, "start": "release", "scope": "ability"},
    "cancelBeforeRelease": "refund",
    "revalidateAtRelease": ["caster", "target", "range", "visibility", "relationship"]
  },
  "onRelease": [
    {"op": "damage", "target": "target", "amount": {"rankParameter": "damage"}, "damageType": "spell"},
    {"op": "damage", "target": "target", "query": {"center": "target", "radius": 4, "relations": ["enemy"], "excludePrimary": true}, "amount": {"rankParameter": "splash"}, "damageType": "spell"}
  ],
  "presentation": {
    "icon": "asset.icons.example.fireball",
    "animations": {"prepare": "attack", "release": "attack", "recover": "idle", "fallback": "idle"},
    "effects": [
      {"id": "flight", "effect": "effect.example.fireball-flight", "event": "projectile", "anchor": "target", "durationFrom": "delivery"},
      {"id": "blast", "effect": "effect.example.burst", "event": "impact", "anchor": "target"}
    ]
  }
}
```

- `activation`: `targeted` or `passive`. `ranks`: every rank has the same named integers; any number may instead be `{"rankParameter": "<name>"}`.
- `targeting`, `cast`: aim and lifecycle. `alive: true`, `outOfRange: "reject"`, the cost/cooldown literals, `cancelBeforeRelease` and the `revalidateAtRelease` list are fixed values: copy them.
- `onRelease`: ordered operations (`heal`, `damage`, `status`, `summon`, `teleport`, …, and one-level `branch`). Optional: `delivery`, `persistent`, `aura`, `autocast`, `weaponCast`, `combatModifiers`, `statuses` (templates), `triggers`, `piercesSpellImmunity`.
- Ticks are 1/40 s. Distances are world units, as unit `dimensions` (a Warrior's body radius is 1.5). `*Permille` is thousandths.

Every field and rule: `reference/definition.md`; every operation and status field: `reference/operations.md`.

## Giving a unit an ability

The caster's unit definition binds abilities in `behaviors.abilities` (a fragment):

```json
{"behaviors": {"abilities": {
  "maxMana": 200, "manaRegenPerSecond": 1.5,
  "bindings": [
    {"id": "fireball", "ability": "ability.example.fireball", "initialRank": 1,
     "controls": ["player", "ai", "scenario"],
     "command": {"hotkey": "Q", "column": 1},
     "ai": {"preference": "enemy", "intervalTicks": 20}}
  ]
}}}
```

- `initialRank` 0 means the ability must be learned; `learning: {"requiredLevels": [...]}` (one increasing hero level per rank) needs hero progression.
- `command` (needs `player` control) puts a button in `column` 1–4, unique per unit; `command.icon` overrides the ability's icon. Passives work without a command but need rank ≥ 1.
- `ai` (needs `ai` control): `preference` `wounded-ally` or `enemy`, optional `intent` `escape`, `reinforce` or `scout` (each requires a matching ability shape). Autocast also needs `ai` control.
- A map overriding `behaviors.abilities.bindings` replaces the whole list.

## Effects

An effect is a reusable stack of render-only layers (`ring`, `glow`, `pillar`, `burst`, `billboard`, `streaks`, `light`, `rain`, `missile`, `beam`, `wavefront`, `particles`, `sound`, `mesh`):

```json effect
{
  "id": "effect.example.burst",
  "name": "Ember burst",
  "description": "Hot flash, sparks and a scorched ring where the seed lands.",
  "durationTicks": 32,
  "loop": false,
  "layers": [
    {"id": "flash", "shape": "light", "colour": "#ff8a2a", "accent": "#ffd9a0", "durationTicks": 10, "count": 1, "size": 5, "height": 1, "intensity": 2},
    {"id": "sparks", "shape": "burst", "colour": "#ffb347", "accent": "#fff1c8", "durationTicks": 20, "count": 12, "size": 1.4, "height": 1, "blend": "additive"},
    {"id": "scorch", "shape": "billboard", "colour": "#ffffff", "accent": "#ffb347", "durationTicks": 32, "count": 1, "size": 3, "height": 0.06,
     "orientation": "ground", "texture": {"asset": "asset.effects.example.scorch-mark", "role": "image", "index": 1}}
  ]
}
```

- An ability's `presentation.effects` binds an effect by id to a simulation `event` (`released`, `projectile`, `impact`, `damaged`, `statusApplied`, …) at the `caster` or `target` anchor. `lifetime: "status"` (only with `statusApplied` and `anchor: "target"`) keeps it as long as the status; `durationFrom: "delivery"` and `sizeFrom`/`spreadFrom: "radius"` take timing and size from the event.
- **Assets.** `presentation.icon` and `command.icon` are asset ids (folders under `assets/`, e.g. `asset.spells.marshal.rally-icon`) whose folder has an image. Layers name an asset's file by `{asset, role, index}` with role `image` (`texture`), `audio` (`sound`) or `geometry` (`model`): index 1 is `<role>.<ext>`, index 2 `<role>_2.<ext>`. The loader checks every one exists. New art is a new asset folder (skill **canopy-assets**).
- Presentation never changes the simulation: no damage, targeting or timing comes from effects.

Layers, sub-objects, every event and its payload: `reference/presentation.md`.

## Deterministic mechanics

The simulation runs at **40 ticks/second** (25 ms per tick). Use integer ticks and fixed or ranked amounts; every referenced rank parameter must exist in every rank. A match freezes its content. Area damage is simultaneous even when shards have cosmetic launch jitter. Abilities own targeting, preparation/recovery, costs/cooldowns, delivery, operations, statuses, persistence and triggers; effects are independent render-only layer stacks; presentation links effect ids to events, anchors and lifetimes; unit bindings attach an ability to a unit. There are no per-ability scripts or hard-coded UI.

Capability map (details in the reference):

| Need | Use |
|---|---|
| Single target, ally/enemy variants | unit targeting + `branch` on relation/traits/resources |
| Area at a point / around target / around self | point `targeting.radius` default area / `query` centred on `target` / `query` centred on `caster` |
| Repeated pulses holding the caster | `cast.channel` |
| Repeated pulses independent of the caster | `persistent` (point, caster or target anchor; `toggle` for on/off) |
| Travel | `delivery`: projectile, chain, line, charge, swarm |
| Buffs, debuffs, controls, shields, immunities, forms, stealth | `status` fields |
| Always-on area effect | passive `aura` (status payload only) or passive `interval` trigger with a query |
| On-hit, on-attack, on-damaged, on-kill, on-death, timers | `triggers` (passive, or `whileStatus` for granted reactions) |
| Weapon crit/evasion/cleave/bonus damage/marks | `combatModifiers` (passive or status), `weaponCast` |
| Units and corpses | `summon` (+`corpses`, `grants`), `resurrect`, `revive`, `split` |
| World/ownership | `teleport`, `vision`, `convert`, `contain`/`releaseContained`, `sacrifice` |
| Values derived from pools or earlier results | `scale` + `record` |

Rules that most often decide validity:

- Point aims need `targeting.radius` (except with a `line` delivery); their `target: "target"` operations hit the default area. Unit and self aims express areas with an operation `query`. Queries do not inherit `targeting.filter`/`condition`; repeat them as the operation `filter`.
- Amount literals are non-negative; negative ranked modifiers (slows, armour loss) come from negative rank parameter values. Fixed `modifiers` accept signed literals.
- Operations run in order and see earlier results; a branch decides once per recipient; only the first operation is checked before costs; later failures do not roll back earlier ones.
- Statuses refresh by ability + status id. Aura statuses use amount 1. `lifetime: instance` needs `persistent`; `untilDeath` needs amount 0 and no `heroDuration`.
- Triggers on targeted abilities need `whileStatus`; whoever holds that status (including units the spell creates) runs the trigger. Death and kill operations target `caster`, `point` or a query, never the dead unit.
- Delivery pairs: projectile/chain/charge need unit aims, line a point aim, swarm a self aim; none combine with channels, persistent or passives.
- `resurrect`, `revive` and `vision` parse with a `query` field but the rules reject it. `revive` only runs inside a `death` trigger; `split` must be the only `onRelease` operation of a self spell.
- Referenced units, assets, effects and damage types (`rules/combat.json`) must exist. `split` members must be controllable, zero-supply, non-hero units; summoned units are ordinary unit definitions (skill **canopy-definitions**).
- Out of scope: terrain/destructible changes, economy rewards, modifying other abilities' costs/cooldowns, charges, temporary possession, siege weapon casts, nested branches, scripts. Say so instead of faking them with visuals.

## Validate

```sh
cargo test -q -p canopy-content --test shipped_data   # loads base/ and checks every rule
cargo test -q -p canopy-content --test skills         # the examples in the skills still parse
```

Errors read `<package>/<path> · <id>: <message>`, e.g. `base/abilities/example/fireball.json · ability.example.fireball: Delivery does not match targeting`. Effect rule errors name the effect's file and the layer: `base/effects/example/burst.json · effect.example.burst/layers/scorch: This layer requires a texture`. A parse error (unknown field, wrong literal) stops the ability rules from running, so fix those first. Fix the field the error names; do not delete requested mechanics to make validation pass. To see an ability in play, open it on the editor's Stage (its tab's Stage view: cast on test targets with the real simulation, with a timeline), run the editor MCP's `encounter` tool (the same take headless: events, refusals, health at the end), or bind it to a unit and run the game (`docs/play.md`).

## Reference

| File | Contents |
|---|---|
| `reference/definition.md` | Every ability field and enum, ranges, targeting, filters, conditions, cast, delivery, persistent, aura, autocast, weaponCast, combatModifiers, templates, triggers, programs, recipients and cross-field rules. |
| `reference/operations.md` | All 17 operations, every status field and modifier, scale and record. |
| `reference/runtime.md` | Cast lifecycle, what caster/target mean in each context, recipient selection, program order, deliveries, persistent instances, auras, statuses, triggers, combat modifiers, AI and autocast, unit bindings, engine limits. |
| `reference/presentation.md` | The `presentation` object, effect bindings, the event catalogue with payloads, effect files, layers per shape, asset references, budgets. |
| `reference/examples.md` | Complete abilities and effects: single target with branch, areas, channels, projectile, chain, line, aura, persistent zone, toggle, buffs and debuffs, stacking marks, summons and factories, transformations, movement and charge, passive reactions, revival. |
| `reference/examples-advanced.md` | Weapon casts, toggled attack bonuses and marks, until-death curses, charm, containment, sacrifice and result scaling, mana drain, swarm, split, scouting vision, concealment and detection, taunt, protection statuses, dispels, caster branches, interval pulses, sound layers. |

Before designing a mechanic you have not used yet, read `runtime.md` and the matching example; read `presentation.md` before binding effects.
