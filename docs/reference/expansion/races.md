# Races and starting rosters

`content/game.json` owns `rules.defaultRace`, `rules.races` and `rules.campaigns`.
The default race inherits `rules.startingSetup` and `rules.ai` composition/skill preferences;
other races can override starting setup and those two AI preferences. Shared AI scheduling,
worker targets, economy recipes, supply, damage rules and XP remain engine-wide.
`raceDefinition()` / `raceAI()` are the common resolvers. Unknown explicit races fail validation.

A race declares its name, description and roster membership. Worker `work.builds`, building
`production.outputs`, upgrade targets, research recipients, garrison accepts and hero choices
are ordinary definitions. Registry compilation rejects references into another race's exclusive
roster. Unassigned definitions can be shared or added by a scenario. Membership does not ban
controlling a captured or explicitly authored foreign unit: that entity keeps its own command
capabilities. There is no hardcoded switch on `ants` or `beetles` in commands or combat.

## Match and campaign boundaries

- Skirmish offers race and the matching hero for every human/AI slot. Changing race resets
  the hero to that race's default.
- Multiplayer `selectRace` / `selectHero` affect only the authenticated player's waiting seat.
  The server validates choices, broadcasts the catalogue, and freezes both in `MatchConfig.slots`.
- Starts resolve the slot's race; map start position/rotation are race-independent. The map's
  legacy `setup.ants` tag remains accepted and does not force ant units.
- Race travels through local/remote saves, restarts, replay configuration and world snapshots.
  Slot hashes distinguish race configurations. Update server and clients together; simulation
  build 111 introduces these mechanics.
- `rules.campaigns[campaign].race` supplies the mission default. Optional mission `race` and
  `playerRaces: {"player.2":"beetles"}` override it. The mission editor exposes both. Authored
  armies, scripts and company carry-over remain authoritative; choosing a race never replaces
  campaign entities with a skirmish spawn. No new beetle campaign is implied.

## Beetle baseline

Worker training, speed, body radius, carrying, harvesting, building work and economic research
match ants. Starting funds, five workers, starting gathering assignments, hall footprint/entrance,
drop-off and supply also match. This preserves equal income on equal geometry and orders.

Horn Guard uses one supply, costs 195 amber, trains in 28 seconds, and has 440 HP, 3 heavy armor,
15 melee damage every 1.6 seconds and speed 11. Stone Slinger uses one supply, costs 180 amber
and 25 wood, trains in 28 seconds, and has 330 HP, 1 light armor, 20 piercing damage every
1.9 seconds, range 16 and speed 11. Its visible stones share the authoritative missile timing.
These are starting balance values, not a claim of competitive balance.

Bastion, Brood Den, War Lodge, Shell Forge, Ancestor Shrine, Rootworks and Hornwatch cover the
basic colony. Bastion upgrades supply hero-capacity tiers. Hornwatch accepts Stone Slingers.
All beetle meshes reference the shared explicit missing-model asset; the slinger has a render
alias only to select the stone projectile. Replace render bindings when art is ready.

## Hornbreaker

Four supply, 820 initial HP, 200 mana, 3 hero armor, 37 attack damage, speed 12. Uses the existing
11-level XP curve; regular ranks unlock at 1/3/5 and Colossus at 5/8. Abilities live in
`content/abilities/ability.hornbreaker.*` and `/spells/heroes/hornbreaker` in Spell Studio.

- **Horn Rush:** terrain/body-checked charge, 70/115/160 damage and .75/1/1.25-second stun.
  The generic `charge` delivery moves the real caster, applies release operations only on
  contact, and stops on obstruction, timeout, source loss, silence/stun/root or a new order.
- **Overwhelm:** a damaging primary melee hit adds a six-second, source-specific mark, capped
  at three stacks. Subsequent source attacks gain 5/10/15 damage per stack, before armor.
  Slow is additive within that mark: 5/7/10% per stack. First hit has no bonus; fourth hit
  has all three. Refresh renews the entire mark. Allies/spells/cleave do not gain the damage
  bonus or add marks. Different targets retain independent marks. Multiple sources retain
  separate bonuses, while their copies of the same slow use the strongest value.
- **Steadfast Aura:** allied units within 16 world units gain 1/2/3 HP per second. Strongest
  copy only, ends outside the aura or when its source falls.
- **Colossus:** 18/24 seconds, +300/500 maximum/current HP, +12/20 damage, +3/5 armor and
  35/50% frontal cleave. Visual scale changes; physical clearance does not.

Stacking is a reusable duration-status capability (`stacking.max`, `stacking.scope`), not
Hornbreaker code. Numeric stat modifiers multiply per stack; complex forms/shields/periodic
payloads are rejected for stacked statuses. `sourceAttackBonus` uses source identity and owner,
so a converted hero cannot inherit their former controller's damage marks. Stack counts,
charge position and all clocks are saved and validated by the shared simulation.

Tests: `tests/shared/races.test.ts`, `tests/net/race-selection.test.ts`,
`tests/abilities/charge-and-marks.test.ts`, plus the existing economy, AI, save and lockstep suites.
