# Combat and victory

An army competes for space, protects gatherers, clears camps and threatens the enemy's Mound. Units and buildings have declared health and armor. The [unit encyclopedia](/units/) lists current base values; equipment, levels and abilities can modify a hero during play.

## Orders and targets

Right-click the ground to move. Right-click a visible enemy to attack. **Attack** followed by a ground click is an attack-move: travel toward the destination and engage relevant enemies along the way.

During automatic combat, a unit pursuing a distant enemy can switch to a visible enemy already in weapon range. It keeps its current target while that target remains in range, and finishes a committed attack before reconsidering. An explicit attack order keeps your chosen target; a regular move order lets the unit leave combat.

Attack followed by a damageable entity is a forced attack, including a friendly target. This is an explicit order, so choose carefully. A red square marks the target your selection was instructed to attack.

Drag selection prioritizes army units. If the area contains no army, it selects Workers instead. This helps move a fighting force without accidentally pulling gatherers off their jobs.

## Roles

- **[Warriors](/units/unit-ants-warrior)** occupy the front line and fight at melee range.
- **[Archers](/units/unit-ants-archer)** attack at range and need protection when enemies close in.
- **[The Ant Marshal](/units/unit-ants-marshal)** is a stronger hero whose experience, abilities and equipment make survival valuable.
- **[Workers](/units/unit-ants-settler)** support the economy and construction. Converting too many into soldiers reduces income.

A [Watchtower](/buildings/building-ants-tower) currently supplies vision, **not an attack**. The Mound has no automatic defensive weapon either. Early aggression remains a viable strategy.

## Armor and damage

Ordinary attacks apply the attack-class multiplier, then multiply by `1 / (1 + 0.06 × armor)`, then apply temporary protection such as Iron Carapace. The result rounds once to the nearest whole HP. A positive hit does at least one damage; an explicit zero matchup remains immune. Spell damage uses its own class row and bypasses armor points.

{{stats:armor}}

Melee is effective against lightly armored ground units. Piercing loses effectiveness against heroes and buildings. Hero attacks are versatile against units but deal half damage to buildings. These are our own matchups: Light describes lightly armored **ground** units, not air units. Hover Damage and Armor in the selection panel to see the current attack interval and defenses.

## Health and repair

Health indicators appear on selected or damaged entities. Health changes from green through yellow and orange to red as it falls; a recent hit flashes the current pip. Buildings use a longer segmented bar, units a compact arc.

Workers can repair damaged friendly buildings without paying resources. Repair still takes time and worker attention.

## Fog and information

Your normal view depends on explored terrain and current visibility. A remembered mine or enemy building is not permission to see its live changing state. Ownership determines who can issue orders; neutral creatures belong to no player.

Cliffs, ramps and ridges also affect vision and weapon clearance. See [terrain and high ground](/guide/terrain) for the fixed sight-height rule and ranged combat.

The debug fog toggle reveals the battlefield visually and can be turned back on. It does not grant the AI additional knowledge or alter the simulation's visibility rules.

## Camps

Neutral camps defend their territory. They attack players in aggro range and have a home/leash area rather than pursuing forever. Defeating the camp grants its weighted [loot reward](/guide/loot); nearby eligible heroes share experience from defeated enemies. Friendly forced kills do not award experience.

Scouting a camp is useful even when it is too strong to fight. Ranged defenders and an Ogre can need a different approach from a small wolf pack. The [map atlas](/maps/) lists initial defenders and reward tiers.

## Objective

Protect your starting Mound and destroy the opponent's. See [first-match rules](/guide/getting-started#winning-and-losing) for the current win condition.
