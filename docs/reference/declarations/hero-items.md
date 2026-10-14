# Hero items and recipient effects

The 30 tiered items live in `content/game.json`. `src/content/items.ts` defines the vocabulary. There are ten items per tier; item IDs are data and are never dispatched on by simulation or presentation.

## Authoring

An equipment item retains `damage`, `armor`, and `maxHp`. Optional `modifiers` add mana, regeneration, attack/movement speed, lifesteal, or ability cooldown reduction. Permille values use 1000 = 100%. Ordinary equipment stats stack as before. Total movement is clamped to 20–180% of base, lifesteal to 40%, and cooldown reduction to 40%.

`active` declares self/allied/enemy targeting, radius in map tiles, cooldown in ticks, optional charges, healing/mana, damage and damage type, timed status/shield, or remaining ability cooldown reduction. One second is 40 simulation ticks. Self actions use radius zero. Area actions affect living, active units; they do not affect buildings or units inside production structures. Roots prevent movement but not attacks. Control immunity suppresses roots and stuns while active.

`aura` declares radius, relationship and modifiers. Auras include their bearer if allied. They are recomputed before regeneration and after movement; leaving range, losing the item, entering a structure, or the source dying removes the aura on the next simulation refresh. Identical item auras do not stack, including different bearers. Different auras stack. Teams are respected; neutral creatures do not become allies merely because their owner is `none`.

`onHit` declares an every-N-basic-hit pulse centered on the struck target, with radius, target cap, damage type and flat or attack-scaled damage. Identical attack-trigger items do not multiply triggers. Pulses do not recursively trigger more pulses or lifesteal. Shielded/invulnerable hits do not produce healing or procs. Lifesteal uses effective basic damage capped at the target's remaining health.

`rescue` declares charges, restored maximum-health fraction and a brief invulnerability duration. It intercepts a lethal hit in the normal death pipeline; it is not a resurrection after death. Phoenix Chrysalis breaks after one rescue. Hourglass of Amber subtracts 12 seconds from remaining hero ability cooldowns, never item cooldowns. It is not a full reset.

## Runtime and presentation

`equipment` remains the stable array of item IDs. Parallel optional `equipmentState` stores charges, next ready tick and attack counter. Dropped ground items carry the same runtime in `item.runtime`. Expended items disappear. Charges and cooldowns survive pickup, dropping, save/load and hero revival. Duplicate auras and temporary effects refresh by item/kind instead of multiplying. Save validation rejects undeclared status kinds, invalid charge counts, excessive shields and forged queued item damage.

Public `itemStatuses` on observed recipients drive both the small world badges and center-panel status icons. Hover/focus a panel badge to see actual modifiers, aura range, remaining shield and remaining time. These statuses do not reveal private enemy inventories or hidden units. Inventory tiles show tier borders, charges and seconds until ready. AI evaluates active items from their declarations and respects their cooldowns.

## Loot

Pools declare `maxTier`; validation rejects entries above it. Easy camps draw T1. Medium and ordinary hard camps draw T2. Only explicitly `legendary: true` camps may use a pool containing T3. Map validation allows at most three potential T3 camp rewards. Amberfall Wilds designates its three highest-experience hard camps (23, 17, 18); each awards one T3. Maps without legendary camps award no T3. The planned endgame map will contain all three tiers and a contested amber root.

## Icons and checks

All thirty icons ship as painted 128 × 128 PNGs on black backgrounds, published as `asset.icons.item-*` assets. High-resolution masters and generation metadata are retained in canonical `art/assets/<id>/` source roles. Use the Asset Workbench for current exports.

Coverage: `tests/game/hero-items.test.ts` exercises every item, aura relationships/range/death, charges, drop/pickup, save/load determinism, shields, roots, immunity, rescue, triggers, lifesteal, cooldowns and UI state. Existing inventory, spells, balance, AI and camp tests remain relevant.

## Tier 1

- **Barkguard Charm** (`item.barkguard`): +2 armor.
- **Thornband** (`item.thornband`): +6 attack damage.
- **Heartseed** (`item.heartseed`): +120 maximum health.
- **Resin Salve** (`item.resin-salve`): Consume to restore 160 health.
- **Moon Dew** (`item.moon-dew`): Restore 80 mana. Consumed on use.
- **Trailkeeper’s Flask** (`item.trailkeeper-flask`): 3 charges: restore 70 health and 25 mana. 10s cooldown between uses.
- **Firefly Pendant** (`item.firefly-pendant`): +50 maximum mana and +0.5 mana per second.
- **Mossweave Wraps** (`item.mossweave-wraps`): Regenerate 1.5 extra health per second.
- **Quickstep Spurs** (`item.quickstep-spurs`): +8% movement speed.
- **Scout’s Whistle** (`item.scout-whistle`): 2 charges: allied units within 6 tiles gain 15% movement speed for 8s. 25s cooldown.

## Tier 2

- **Royal Crest** (`item.royal-crest`): +10 damage, +3 armor and +100 maximum health.
- **Ancient Heartwood** (`item.ancient-heartwood`): +280 maximum health and +2 health per second.
- **Moonwell Chalice** (`item.moonwell-chalice`): +140 maximum mana and +1.5 mana per second.
- **Predator’s Talon** (`item.predator-talon`): +10 damage. Basic attacks restore 10% of damage dealt as health.
- **Amber Carapace** (`item.amber-carapace`): +4 armor. Activate: absorb 180 damage for up to 8s. 45s cooldown.
- **Broodkeeper’s Lantern** (`item.broodkeeper-lantern`): Aura: allied living units within 6 tiles regenerate 2 health per second.
- **Warcaller’s Standard** (`item.warcaller-standard`): Aura: allied units within 6 tiles gain 10% attack damage.
- **Stormwing Spurs** (`item.stormwing-spurs`): +10% movement speed and +12% attack speed.
- **Restoration Draught** (`item.restoration-draught`): 2 charges: restore 300 health and 120 mana. 10s cooldown.
- **Rootbinder’s Idol** (`item.rootbinder-idol`): Activate: enemies within 4 tiles take 60 magic damage and are rooted for 2s (they can still attack). 35s cooldown.

## Tier 3

- **Crown of the First Queen** (`item.first-queen-crown`): +150 maximum health. Aura: allied units within 7 tiles gain 15% damage and +2 armor.
- **Worldroot Heart** (`item.worldroot-heart`): +450 maximum health and +4 health per second. Activate: restore 20% maximum health. 70s cooldown.
- **Reaper’s Mandible** (`item.reaper-mandible`): +18 damage. Every fourth basic hit cleaves up to 4 enemies within 3 tiles of the target for 50% attack damage.
- **Aegis of the Canopy** (`item.canopy-aegis`): +5 armor. Activate: shield allied units within 6 tiles for 180 damage each for up to 8s. 80s cooldown.
- **Tempest Antennae** (`item.tempest-antennae`): +15% attack speed. Every third basic hit arcs 45 magic damage to up to 3 enemies within 4 tiles of the target.
- **Phoenix Chrysalis** (`item.phoenix-chrysalis`): +120 maximum health. Once: prevent a lethal hit, restore 35% health and become invulnerable for 1.5s. The chrysalis then breaks.
- **Scepter of the Deep Moon** (`item.deep-moon-scepter`): +220 maximum mana, +2 mana per second and 20% shorter hero ability cooldowns. Does not affect items.
- **Banner of the Endless Brood** (`item.endless-brood-banner`): Aura: allied units within 7 tiles gain 15% attack speed. Activate: nearby allies ignore roots and stuns for 4s. 90s cooldown.
- **Hourglass of Amber** (`item.amber-hourglass`): +100 maximum mana. Activate: reduce remaining hero ability cooldowns by 12s. Does not affect items. 100s cooldown.
- **Heart of Winter** (`item.winter-heart`): +180 maximum health. Aura: enemies within 5 tiles move 15% slower. Activate: deal 120 magic damage within 5 tiles and slow attack speed by 20% for 4s. 60s cooldown.
