# Beetles: the race

The design Jiri settled on 2026-10-18 on the race roster page (D66). It is the spec for the beetles' content, models, icons and abilities. Concept art for every building and unit is in `art/styles/` (buildings `beetle-<building>-idle.png`) and in the old repository's `.asset-work/concepts/buildings/` (units `unit-<unit>-<n>.png`, the chosen variant below).

## Identity

The forest runs on **substance**, its magic. Ants take it in and are changed by it; bees transform with it through blood and ritual. **Beetles never let it touch them**: they pipe, press, bottle and burn it in machines, and their whole technology depends on it.
- Bodies are plain glossy black chitin. Substance glows only inside glass (tanks, coils, gauges, canisters, exhaust), deep blue to violet, never green.
- Getting stronger is a better machine, never a mutation: research reads as engineering.
- Buildings at rest stand still; glow and motion mean work (D65).
- Slow, stubborn, territorial: expensive, durable units; a fortress race that grows outward.
- Team colour (royal blue in concepts) is an accent on units: a shield face, a pair of shoulder plates, a tabard. Most of a unit is black chitin, dark steel and brass.

## Economy and network

- **Amber** at the base, **Substance** (D62's root, renamed) out in the world. Every race builds a **Substance Well on top of a Substance deposit**; workers gather from the well.
- **The network (D66):** the main hall and Substance Wells are sources; every Brood Den extends it with a generous radius. Every other beetle building must be placed inside it and pauses its work while unpowered. Towers need power and are clearly stronger than other races' towers. The network shows only while placing a building or with the grid on.
- Tiers: Bastion → Great Bastion (tier 2) → Elder Bastion (tier 3).

## Buildings

Footprints in cells; models fill them (no walkable perimeter).

| Id | Name | Tier | Size | Role | Makes / researches | Concept |
|---|---|---|---|---|---|---|
| `building.beetles.bastion` | Bastion | 1 | 20 | Main hall: stores goods, supply, network source | Tinker; upgrade to Great Bastion | `beetle-hall-t1-idle.png` |
| `building.beetles.great-bastion` | Great Bastion | 2 | 20 | Hall tier 2: unlocks Alembic, Engine Yard, tier-2 units | Tinker; Pressure Mains; upgrade to Elder Bastion | `beetle-hall-t2-idle.png` |
| `building.beetles.elder-bastion` | Elder Bastion | 3 | 20 | Hall tier 3: unlocks Juggernaut, level III research, Arc Tower | Tinker | `beetle-hall-t3-idle.png` |
| `building.beetles.brood-den` | Brood Den | 1 | 8 | Supply; network node (generous radius) | | `beetle-brood-den-idle.png` |
| `building.beetles.substance-well` | Substance Well | 1 | on a deposit | Built on a Substance deposit; workers gather from it; network source. Raw substance in the deposit glows even at rest (the forest's own magic, the one exception to D65) | | `beetle-substance-well-idle.png` |
| `building.beetles.war-lodge` | War Lodge | 1 | 16 | Barracks | Shieldbearer, Bolter, Breacher (tier 2); Interlocking Shields, Drum Magazines, Heavy Pistons | `beetle-war-lodge-idle.png` |
| `building.beetles.shell-forge` | Shell Forge | 1 | 16 | Weapons and plating research | Weapon Edges I–III, Bolt Calibre I–III, Plating I–III (II needs tier 2, III tier 3) | in game |
| `building.beetles.alembic` | Alembic | 2 | 12 | Specialists who handle substance through devices | Arcwright, Mender, Warden; Specialist Training (adept, master) | `beetle-alembic-idle.png` |
| `building.beetles.engine-yard` | Engine Yard | 2 | 16 | Machines | Bombard, Steamwing, Juggernaut (tier 3); Fragmentation Shells, Riveted Hulls I–II, Afterburners | `beetle-engine-yard-idle.png` |
| `building.beetles.hornwatch` | Hornwatch | 1 | 8 | Tower; needs power; strong | Arc Tower upgrade (tier 3) | `beetle-hornwatch-idle.png` |
| `building.beetles.ancestor-shrine` | Ancestor Shrine | 1 | 12 | Heroes: trains and reconstitutes them | Hornbreaker, Forgemaster, Coilkeeper | `beetle-ancestor-shrine-idle.png` |

Cut in review: Call to Arms, Deep Taps (breaks the economy), the Rootworks as a separate drop-off (the Substance Well replaces it).

## Units

| Id | Name | From | Tier | Role | Kit | Concept |
|---|---|---|---|---|---|---|
| `unit.beetles.worker` | Tinker | Bastion | 1 | Gathers, builds, repairs buildings and machines | Huge brass wrench, backpack tank | `unit-tinker-2` |
| `unit.beetles.shieldbearer` | Shieldbearer | War Lodge | 1 | Front line tank. *Lock Shields*: braces in place, armour up | Riveted tower shield, piston hammer | `unit-shieldbearer-2` |
| `unit.beetles.bolter` | Bolter | War Lodge | 1 | Basic ranged; hits air | Multi-barrel pressure repeater fed from a back canister | `unit-bolter-1` |
| `unit.beetles.breacher` | Breacher | War Lodge | 2 | Breaks armour and buildings; charges in | Siege drill arm and a clamp | `unit-breacher-drill-2` |
| `unit.beetles.arcwright` | Arcwright | Alembic | 2 | Damage caster: chain arcs | Coil lance | `unit-arcwright-2` |
| `unit.beetles.mender` | Mender | Alembic | 2 | Support: patches beetles, repairs machines | Pressure syringe-gun, welding torch arm | `unit-mender-1` |
| `unit.beetles.warden` | Warden | Alembic | 2 | A directional field shield: ranged attacks can't pass it from the front | Barrier projector: a curved violet pane on two prongs | `unit-warden-projector-1` |
| `unit.beetles.bombard` | Bombard | Engine Yard | 2 | Siege: long-range splash, weak up close (minimum range) | Six-legged mortar crawler with a crewman | `unit-bombard-1` |
| `unit.beetles.steamwing` | Steamwing | Engine Yard | 2 | Flyer: scout, anti-air, harassment | Boiler wing rig; a substance beam under its chin fires violet bolts | `unit-steamwing-1` |
| `unit.beetles.juggernaut` | Juggernaut | Engine Yard | 3 | Ultimate walking fortress, one at a time | Six-legged walker, violet furnace window, two cannons, crew | `unit-juggernaut-1` |
| `unit.beetles.hornbreaker` | Hornbreaker | Ancestor Shrine | hero | Warlord, melee | Hercules horn, great hammer | `unit-hornbreaker-2` |
| `unit.beetles.forgemaster` | Forgemaster | Ancestor Shrine | hero | Engineer | Smokestack helm, forge apron, back arm rig with a folded turret | `unit-forgemaster-1` |
| `unit.beetles.coilkeeper` | Coilkeeper | Ancestor Shrine | hero | Substance savant | Coil halo, lantern-staff | `unit-coilkeeper-1` |

## Heroes

Every hero has three basic abilities and an ultimate (the shared hero rules: levels, learning order, mana).

**Hornbreaker** (exists): Horn Rush, Overwhelm, Steadfast Aura; ultimate **Colossus**.

**Forgemaster**, the engineer:
- **Bolt Turret**: deploys a brass repeater turret at a target point for 40 seconds; it shoots ground and air and can be destroyed.
- **Overclock**: an allied unit or machine attacks and moves 30/45/60% faster for 8 seconds.
- **Field Forge** (aura): nearby allies gain armour; nearby machines and buildings slowly repair.
- Ultimate **Forgefall**: after a 1.5-second warning circle, a molten crucible slams into the target area: heavy damage and a short stun, then a pool of molten metal that burns enemies standing in it for 6 seconds.

**Coilkeeper**, the substance savant:
- **Arc Lash**: a chain of violet lightning that jumps between up to five enemies.
- **Overload**: detonates a target enemy's mana: drains it and deals the drained amount as damage around the target (a flat blast against units without mana).
- **Recharge**: restores mana to allies in an area and gives them a short shield.
- Ultimate **Unbottled**: the one time a beetle lets substance loose. She shatters her lantern: for 6 seconds a violet storm stands around her, pulling enemies in a wide radius towards its centre and lifting them off the ground (stunned) while arcs strike among them; allies inside are shielded.

## Research

| Building | Research |
|---|---|
| Bastion | Great Bastion (upgrade) |
| Great Bastion | Pressure Mains (larger network radius), Elder Bastion (upgrade) |
| Shell Forge | Weapon Edges I–III (melee), Bolt Calibre I–III (ranged), Plating I–III (armour) |
| War Lodge | Interlocking Shields (Lock Shields gives more armour), Drum Magazines (Bolter attack speed), Heavy Pistons (Breacher charge cooldown) |
| Alembic | Specialist Training: adept and master, each unlocking a specialist's second ability |
| Engine Yard | Fragmentation Shells (Bombard splash), Riveted Hulls I–II (machine health), Afterburners (Steamwing speed) |
| Hornwatch | Arc Tower (tier 3): a substance coil with chain damage |

## Asset ids

Models: `asset.models.buildings.beetles-<building>` and `asset.models.units.beetles-<unit>` (the Shell Forge is `beetles-shell-forge`, the halls `beetles-bastion`, `beetles-great-bastion`, `beetles-elder-bastion`, the deposit `asset.models.resources.substance-deposit`). Until a model lands, a definition may point at an existing model as a stand-in.
