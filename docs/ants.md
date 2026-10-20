# Ants: the race

The design Jiri settled with Claude on 2026-10-19, before the concept review on the race roster page. It is the spec for the ants' content, models, icons and abilities. The current ant roster (Cleaver Warrior, Leaf Archer, Rootweaver, …) was a test and is replaced entirely. Concept art is in the old repository's `.asset-work/concepts/buildings/`: buildings `ant-sc-<id>-<n>.png`, units `ant-<id>-<n>.png` (a first pass whose look was rejected; to be redrawn).

Rows marked *proposed* are Claude's proposals awaiting Jiri's review on the roster page; everything else is settled.

## Identity

The forest runs on **substance**, its magic. Beetles keep it in glass; bees transform with it through blood and ritual. **Ants take it into their bodies.**
- **Bio-magic, never corruption** (Jiri, 2026-10-19): substance is a living light, like bioluminescence, glowing sap, blossoms and fireflies. No cracks with glowing veins, no spikes or tusks, no ooze.
- **Buildings** are sculpted **sand-coloured clay** like a Gaudí termite cathedral: flowing organic spires with rounded tips, oval windows glowing blue-violet, climbing vines with small blue-violet blossoms, royal-blue silk banners and drapes. The main hall is concept `ant-base-spire-cathedral-2` (Jiri: the right amount of detail, and still fits ants). Every building keeps that style and adds one or two signature features that say what it is at a glance. Concepts `ant-sc-<id>-<n>.png`.
- **The basic soldiers are ant creatures** (Jiri, 2026-10-20): the Raider is the ants' Zergling in every way (small, stocky, cheap, fast, hatched in pairs, swept-back blade fins) and the Needler fires chitin needles like a Hydralisk. Both read from above: identity on the back, blue only on the back plates.
- **Specialists and heroes are humanoid** (Jiri, 2026-10-19): they stand upright on two legs with arms, an ant's head, mandibles and antennae, and carry grown props (chitin blades, amber, silk, leaves, fungus); each reads by its one or two signature props. Concepts `ant-u2-<unit>-<n>.png`.
- **Only heavily mutated ants keep the full ant body**, and mutation always costs something (*proposed*): the Burster, the Honeypot and the Reaver Queen. **Mutated units can't fit through tunnels**, and each has its own drawback (see Units).
- Mutation is never something the player manages (too hard for beginners): a unit is mutated or not, and its drawbacks come with it.
- **One body, ten thousand legs**: the colony is a superorganism. No machines and no metal; everything is grown or secreted (packed clay, amber resin, shed chitin, silk, fungus). **Pheromones are their magic.**
- No healers in the usual sense and no summons: no broodlings, no larvae that hatch into units, nothing auto-spawns. The Gardener's fungus circles are the one healing.
- Team colour (royal blue in concepts): war-paint on the big carapace plates and a silk band on units; dyed silk banners and painted resin plates on buildings.
- Ants are the first race in the campaign. The Queen is its protagonist; she turns at the end.

## Economy and tunnels

- **Amber** at the base, **Substance** in the world: a **Substance Siphon** is built on top of each Substance deposit, as for every race.
- **Tunnels, the signature.** Every mound is a tunnel entrance: the Nest and its tiers, and every Brood Mound. Ants ordered to a destination travel underground automatically when a route through two mounds is faster than walking: they enter the nearest mound and come out at the one nearest the target.
  - Each mound lets through a limited number of units per second (its **throughput**); the rest queue at its mouth or walk. You can't move a whole army in an instant. Wider Tunnels raises it.
  - Units underground can't be seen or attacked. A mound destroyed with units queued inside it spills them out at its position.
  - Whether a player can switch the routing off is open (Jiri: "maybe that is part of the mechanic"); the default is automatic.
- **Founding (the Nydus attack):** the Alate (and the Queen) **Found** a new mound anywhere they can reach. It is a full tunnel mound. Expected to be strong; it will be balanced in play.
- Tiers: Nest → Great Nest (tier 2) → Queen's Chamber (tier 3).

## Buildings

| Id | Name | Tier | Size | Role | Makes / researches |
|---|---|---|---|---|---|
| `building.ants.nest` | Nest | 1 | 20 | Main hall: stores goods, supply, tunnel mound | Worker; upgrade to Great Nest |
| `building.ants.great-nest` | Great Nest | 2 | 20 | Hall tier 2: unlocks the Fungus Garden, Scent Hall, Deep Gallery, tier-2 units | Worker; upgrade to Queen's Chamber |
| `building.ants.queens-chamber` | Queen's Chamber | 3 | 20 | Hall tier 3: unlocks the Queen's Cradle, level III research, Venom Spire | Worker |
| `building.ants.brood-mound` | Brood Mound | 1 | 8 | Supply; tunnel mound | Wider Tunnels I–II |
| `building.ants.substance-siphon` | Substance Siphon | 1 | on a deposit | Built on a Substance deposit; workers gather from it | |
| `building.ants.soldier-burrow` | Soldier Burrow | 1 | 16 | Barracks | Raider, Needler, Burster (tier 2), Major (tier 2) |
| `building.ants.chitin-works` | Chitin Works | 1 | 16 | Forge: levelled research | Mandibles I–III, Needle Hardening I–III, Carapace I–III |
| `building.ants.fungus-garden` | Fungus Garden | 2 | 12 | The substance lab: one-off upgrades. Costs substance | See Research |
| `building.ants.scent-hall` | Scent Hall *proposed* | 2 | 12 | Pheromone specialists | Herald, Gardener |
| `building.ants.deep-gallery` | Deep Gallery *proposed* | 2 | 16 | Specialised castes | Honeypot, Alate, Bullet Ant (tier 3) |
| `building.ants.queens-cradle` | Queen's Cradle *proposed* | 3 | 16 | Needs the Queen's Chamber | Reaver Queen (one at a time) |
| `building.ants.acid-spire` | Acid Spire | 1 | 8 | Tower: spits acid, hits air | Venom Spire upgrade (tier 3) |
| `building.ants.royal-chamber` | Royal Chamber | 1 | 12 | Heroes: trains and revives them | The Queen, Royal Alchemist, General |

The Scent Hall and Deep Gallery split the earlier single "specialist building" the way the beetles split the Alembic and the Engine Yard.

## Units

| Id | Name | From | Tier | Role | Kit and abilities |
|---|---|---|---|---|---|
| `unit.ants.worker` | Worker | Nest | 1 | Gathers, builds | Carries amber in its mandibles; builds by secreting resin |
| `unit.ants.raider` | Raider | Soldier Burrow | 1 | Core melee; hatched two at a time | The ants' Zergling: a small, stocky biting ant with blue blade fins on its back. *Swarm* (innate): more damage for each other Raider nearby, up to a cap |
| `unit.ants.needler` | Needler | Soldier Burrow | 1 | Ranged; hits air | An ant creature (not humanoid) that fires sharp chitin needles, inspired by the Hydralisk but standing, not crawling. Barbed Needles (research): hits lower armour |
| `unit.ants.burster` | Burster | Soldier Burrow | 2 | Suicide unit: runs in and bursts (splash, strong against buildings and clumps) | **Mutated.** A huge sloshing sac of liquid substance. *Rupture*: bursts on command. *Unstable*: killed, it bursts anyway and hurts nearby allies too; can't use tunnels |
| `unit.ants.major` | Major | Soldier Burrow | 2 | Tank | Huge armoured head. *Guard* (innate): takes part of the damage dealt to small allies next to it |
| `unit.ants.gardener` | Gardener | Scent Hall | 2 | Support | *Fungus Circle*: grows a ring of glowing fungus that heals allies in it over time. *Spore Cloud* (adept): enemies in a cloud miss attacks |
| `unit.ants.herald` | Herald | Scent Hall | 2 | Pheromone caster | *Trail*: allies in an area move faster. *Alarm*: a large, short damage buff for a lot of mana. *Panic* (adept): enemies in an area flee in fear |
| `unit.ants.honeypot` | Honeypot | Deep Gallery | 2 | Siege: long-range splash, slow, minimum range | **Mutated.** A replete with a giant amber abdomen; lobs nectar globs. *Bloated*: slow, can't fire up close, can't use tunnels. Sticky Nectar (research): impacts leave a slowing pool |
| `unit.ants.alate` | Alate | Deep Gallery | 2 | Flyer: scout, light harassment | Winged. *Found*: lands, sheds its wings and becomes a tunnel mound (the Alate is used up) |
| `unit.ants.bullet-ant` | Bullet Ant | Deep Gallery | 3 | Elite duellist against big targets | *Agony*: a sting that stuns, then leaves the target slowed and taking more damage |
| `unit.ants.reaver-queen` | Reaver Queen | Queen's Cradle | 3 | Ultimate monster, one at a time | See below |

**Reaver Queen** *(Devour settled; the rest proposed)*: **mutated**: *Ravenous*, she loses health over time unless she Devours, Gardeners can't heal her, and she can't use tunnels.
- **Devour**: swallows a non-hero enemy unit whole and heals; a large one is chewed over a few seconds and can be freed by killing her or interrupting.
- **Rampage**: charges through a line, knocking enemies aside.
- **Royal Presence** (aura): nearby ants attack faster.
- **Molt**: sheds her carapace: heals a large part of her health and moves faster, with lower armour until it hardens.

Cut in design: Larva Bearer, any broodlings, auto-summoning, the mutation mechanic, the Young Queen's Nuptial Swarm, Trap-Jaw, Doorhead, Weaver, Leafcutter, Mycelial Oracle, Tunneler (may come back).

## Heroes *(kits proposed)*

Every hero has three basic abilities and an ultimate (the shared hero rules).

**The Queen**, the colony's mother and the campaign's protagonist:
- **Found**: raises a tunnel mound at a target point in range (one at a time; long cooldown).
- **Royal Jelly**: heals a target ally and raises its damage for a while.
- **One Body** (aura): damage dealt to an ally near her is shared out among all allies near her.
- Ultimate **Exodus**: every ant around her sinks underground and comes out at a friendly mound she chooses.

**Royal Alchemist**, who distils substance in her own body:
- **Volatile Draught**: throws a flask of substance venom that bursts in an area.
- **Transfusion**: drains health and mana from an enemy into an ally.
- **Crystallise**: encases a target in violet crystal: an enemy is stunned; an ally can't act and takes no damage.
- Ultimate **Overdose**: allies in an area swell with substance: much faster and stronger for 10 seconds, then lose part of their health when it ends.

**General**, the army-ant marshal:
- **Mandible Charge**: leaps to a target enemy and seizes it (stun).
- **Mark Prey**: marks an enemy; allies deal it more damage.
- **War Column** (aura): nearby allies move faster and deal more damage.
- Ultimate **Bivouac**: the General and the ants around lock together into a living fortress: they can't move, take much less damage, and enemies attacking them are hurt.

## Research

| Building | Research |
|---|---|
| Nest | Great Nest (upgrade) |
| Great Nest | Queen's Chamber (upgrade) |
| Brood Mound | Wider Tunnels I–II (mound throughput; II needs tier 3) |
| Chitin Works | Mandibles I–III (melee), Needle Hardening I–III (ranged), Carapace I–III (armour); II needs tier 2, III tier 3 |
| Fungus Garden | Barbed Needles (Needler hits lower armour), Volatile Sacs (Burster radius), Living Wall (Major's Guard takes more and it gains armour), Sticky Nectar (Honeypot slowing pool), Pheromone Mastery (adept: Herald's Panic, Gardener's Spore Cloud), Hardened Wings (Alate health and speed), Venom Glands (Bullet Ant's Agony lasts longer) |
| Acid Spire | Venom Spire (tier 3): acid splash that lowers armour |

## Building models

Every ant building is a Tripo model of its approved concept, made game-ready and animated by `tools/buildings/ant_building.py` from one config each in `tools/buildings/ants/` (its docstring is the reference). Tripo's segmentation only says where the moving parts are; they are cut from the whole model so a building keeps one material and atlas. Each building animates while it works (D65): swaying banners, a breathing crown, opening mandibles, a dipping blade, beating wings, an opening flower. The definitions keep the old ids (`building.ants.fort` is the Nest, `great-mound` the Great Nest, `elder-hall` the Queen's Chamber, `house` the Brood Mound, `substance-well` the Siphon, `barracks` the Soldier Burrow, `ironroot-forge` the Chitin Works, `bombardier-workshop` the Deep Gallery, `tower` the Acid Spire, `sanctuary` the Royal Chamber); the Fungus Garden, Scent Hall and Queen's Cradle are new and not yet in the build list (their units and research don't exist yet).

Known gaps: buildings without production or research (Brood Mound, Fungus Garden, Scent Hall, Queen's Cradle) never enter `work` yet, so their animation waits for tunnels and research; the Acid Spire plays its recoil as `attack`, once per shot.

## Unit models

Every ant unit is a Tripo model of its picked concept, rigged and animated in Tripo, then made game-ready by `tools/units/tripo_unit.py` (scale, ground, team mask, in-place clips). The two body plans take different paths:
- **Ant creatures** (Worker, Raider, Needler, Burster, Honeypot, Alate, Bullet Ant, Reaver Queen) use Tripo's creature rig, whose only motion is its walk. `tools/units/ant_creature.py` adds the rest from the bones' roles: run, a breathing idle, a bite, sting, spit or slam attack, a recoil, rearing to cast, and a death roll. Creatures are sized by their footprint, not their height, so long-legged ones don't dwarf the rest.
- **Humanoids** (Major, Queen, General) use Tripo's Mixamo rig and presets: walk, run, slash, hit, fall and cast. Tripo's export leaves idle presets out, so `tools/units/humanoid_idle.py` adds a breathing idle.

The definitions keep the old ids: `unit.ants.settler` is the Worker, `warrior` the Raider, `archer` the Needler, `queensguard` the Major, `bombardier` the Honeypot, `winglancer` the Alate, `thornblade` the Bullet Ant, and `marshal` the General. Stats and abilities are unchanged. The Burster, Reaver Queen and Queen are new definitions. They show in the gallery but nothing trains them yet, because their abilities aren't designed.

Known gaps:
- The Herald, Gardener and Royal Alchemist are six-legged centaurs. Tripo rigs them as two-legged bipeds whatever rig type is chosen, so they still need a rig of their own. Until then `banneret` and `nectarist` keep their old models; their portraits are ready (`asset.portraits.units.ants-herald`, `-gardener`, `-royal-alchemist`).
- The Honeypot and Bullet Ant show little team colour: their concepts carry almost no blue.
- The Raider shows little: its fins came out of Tripo more bronze than blue.

## Asset ids

Models: `asset.models.buildings.ants-<building>` and `asset.models.units.ants-<unit>`. Until a model lands, a definition may point at an existing model as a stand-in.
