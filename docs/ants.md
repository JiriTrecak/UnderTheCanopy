# Ants: the race

The design Jiri settled with Claude on 2026-10-19, before the concept review on the race roster page. It is the spec for the ants' content, models, icons and abilities. The current ant roster (Cleaver Warrior, Leaf Archer, Rootweaver, …) was a test and is replaced entirely. Concept art is in the old repository's `.asset-work/concepts/buildings/`: buildings `ant-sc-<id>-<n>.png`, units `ant-<id>-<n>.png` (a first pass whose look was rejected; to be redrawn).

The kits were designed and built by Claude on 2026-10-20 at Jiri's request ("build everything, add abilities that you think are appropriate"); they await his review in play.

## Identity

The forest runs on **substance**, its magic. Beetles keep it in glass; bees transform with it through blood and ritual. **Ants take it into their bodies.**
- **Bio-magic, never corruption** (Jiri, 2026-10-19): substance is a living light, like bioluminescence, glowing sap, blossoms and fireflies. No cracks with glowing veins, no spikes or tusks, no ooze.
- **Buildings** are sculpted **sand-coloured clay** like a Gaudí termite cathedral: flowing organic spires with rounded tips, oval windows glowing blue-violet, climbing vines with small blue-violet blossoms, royal-blue silk banners and drapes. The main hall is concept `ant-base-spire-cathedral-2` (Jiri: the right amount of detail, and still fits ants). Every building keeps that style and adds one or two signature features that say what it is at a glance. Concepts `ant-sc-<id>-<n>.png`.
- **The basic soldiers are ant creatures** (Jiri, 2026-10-20): the Raider is the ants' Zergling in every way (small, stocky, cheap, fast, hatched in pairs, swept-back blade fins) and the Needler fires chitin needles like a Hydralisk. Both read from above: identity on the back, blue only on the back plates.
- **Specialists and heroes are humanoid** (Jiri, 2026-10-19): they stand upright on two legs with arms, an ant's head, mandibles and antennae, and carry grown props (chitin blades, amber, silk, leaves, fungus); each reads by its one or two signature props. Concepts `ant-u2-<unit>-<n>.png`.
- **Only heavily mutated ants keep the full ant body**, and mutation always costs something: the Burster, the Honeypot and the Reaver Queen. **Mutated units can't fit through tunnels**, and each has its own drawback (see Units).
- Mutation is never something the player manages (too hard for beginners): a unit is mutated or not, and its drawbacks come with it.
- **One body, ten thousand legs**: the colony is a superorganism. No machines and no metal; everything is grown or secreted (packed clay, amber resin, shed chitin, silk, fungus). **Pheromones are their magic.**
- No healers in the usual sense and no summons: no broodlings, no larvae that hatch into units, nothing auto-spawns. The Gardener's fungus circles are the one healing (with the Queen's Living Light and the Alchemist's Transfusion).
- Team colour (royal blue in concepts): war-paint on the big carapace plates and a silk band on units; dyed silk banners and painted resin plates on buildings.
- Ants are the first race in the campaign. The Queen is its protagonist; she turns at the end.

## Economy and tunnels

- **Amber** at the base, **Substance** in the world: a **Substance Siphon** is built on top of each Substance deposit, as for every race.
- **Tunnels, the signature** (built, D74). Every mound is a tunnel entrance: the Nest and its tiers, and every Brood Mound. A unit ordered to walk somewhere travels underground automatically when a trip from the mound nearest it to the mound nearest its destination is faster than walking (by two seconds or more). It walks to the first mound and goes under at its mouth, travels underground at 60 cells a second, then comes up at the other mound and walks on.
  - Each mound lets one unit under every 12 ticks (a little over three a second); the rest queue at its mouth. Wider Tunnels I and II each let 50% more through.
  - Units underground can't be seen, attacked or ordered. If the mound they are headed to falls, they come up where it stood. Units walking or queued to a fallen mound walk on.
  - Only mounds a unit can walk to, and from which it can walk on to its destination, are used: a mound behind a closed gate is no way in or out. Heroes, too broad to stand right at a mouth, go under from a little further off; a unit that cannot reach the mouth in time (a crowd stands in front of it) walks instead.
  - A mound works (D65: its animation, glow and effects) while ants pass through it.
  - The mutated castes can't use tunnels: the Burster, Honeypot and Reaver Queen. Flyers never do.
  - Routing is always automatic; whether a player may switch it off stays open.
- **Founding (the Nydus attack):** the Alate **Founds** a Brood Mound where it lands, and is used up (the Queen founded too until D86). The mound is finished at once, a full tunnel mound with its 8 supply. The computer founds mounds out on the frontier, more than 64 cells from its other mounds with no enemy near, and its army then tunnels to them.
- Tiers: Nest → Great Nest (tier 2) → Queen's Chamber (tier 3, once a Deep Gallery stands).

## Buildings

The definitions keep their old ids (Building models below).

| Building | Tier | Role | Makes / researches |
|---|---|---|---|
| Nest | 1 | Main hall: stores goods, 15 supply, tunnel mound | Worker; upgrade to Great Nest (400 amber, 150 substance) |
| Great Nest | 2 | Hall tier 2, tunnel mound | Worker; upgrade to Queen's Chamber (600, 250; needs a Deep Gallery) |
| Queen's Chamber | 3 | Hall tier 3, tunnel mound | Worker |
| Brood Mound | 1 | 8 supply; tunnel mound | Wider Tunnels I–II |
| Substance Siphon | 1 | On a Substance deposit | |
| Soldier Burrow | 1 | Barracks | Raider (two at a time), Needler, Burster (tier 2), Major (tier 2) |
| Chitin Works | 1 | Levelled research | Mandibles, Needle Hardening, Carapace, I–III each |
| Fungus Garden | 2 | One-off upgrades | See Research |
| Scent Hall | 2 | Pheromone castes | Herald, Gardener |
| Deep Gallery | 2 | Specialised castes | Honeypot, Alate, Bullet Ant (tier 3) |
| Queen's Cradle | 3 | The ultimate | Reaver Queen |
| Acid Spire | 1 | Tower: a Needler spits from its mouth at ground and air (20 damage) | Upgrade to Venom Spire (tier 3) |
| Venom Spire | 3 | Upgraded tower: 26 spell damage, each hit lowers armor by 2 | |
| Royal Chamber | 1 | Heroes: trains and revives them | The Queen, Royal Alchemist, General |

## Units

Numbers are per unit; costs are amber/substance, time in seconds.

| Unit | Tier | Cost, supply | Role | Kit |
|---|---|---|---|---|
| Worker | 1 | 100, 1 | Gathers, builds | Like every ant, uses the tunnels |
| Raider | 1 | 150 for **two**, 1 each | Core melee, fast (185 hp, 15 speed) | *Frenzy* (passive): every second a Raider fights beside other small ants, they gain 6% damage, up to 24% |
| Needler | 1 | 150, 2 | Ranged, hits air | *Barbed Needles* (Fungus Garden): each hit lowers armor by 1 for 5 s, up to 3 |
| Burster | 2 | 110/60, 1 | Suicide unit | **Mutated.** *Unstable*: bursts on its first bite or when it dies: 100 siege damage to enemies and buildings within 4, and 40 to allies (Volatile Sacs: 150, sparing allies). *Rupture*: burst on command. No tunnels |
| Major | 2 | 230/60, 3 | Tank (960 hp, 4 armor) | *Guard* (aura): small ants within 6 take 15% less damage (Living Wall: 25%, and +2 armor). *Plug* (toggle): roots itself, +5 armor, half damage from missiles, 30% less from siege |
| Honeypot | 2 | 240/60, 3 | Siege: 60 splash at 30 range, minimum range 7, slow | **Mutated.** Sticky Nectar: wider splash that slows by 30%. No tunnels |
| Alate | 2 | 175/50, 2 | Flyer: scout, harasser | *Found*: becomes a Brood Mound |
| Bullet Ant | 3 | 200/75, 3 | Elite duellist | *Giant Killer*: +15 damage against heroes and level-3+ units. *Agony*: 40 damage and a 1 s stun, then 30% slower and 25% more damage taken for 6 s (Venom Glands: 10 s) |
| Herald | 2 | 160/40, 2 | Pheromone caster | *Trail*: a 12 s trail where allies move 35% faster. *Alarm*: allies within 9 deal 35% more damage and attack 15% faster for 6 s. *Panic* (Pheromone Mastery): enemies in an area can't attack or cast for 3 s and move slower |
| Gardener | 2 | 150/40, 2 | Healer | *Fungus Circle*: a 10 s ring healing allies 15 a second (never the Reaver Queen). *Spore Cloud* (Pheromone Mastery): enemies in it attack 40% and move 20% slower |
| Reaver Queen | 3 | 550/250, 8 | Ultimate (2800 hp) | **Mutated.** *Ravenous*: loses 10 hp a second, out of reach of the Gardeners. *Devour*: swallows a non-hero enemy, heals 300 and digests it. *Rampage*: charges an enemy, 90 damage and a stun around it. *Molt*: heals 35% and runs faster, with less armor. *Royal Presence* (aura): ants nearby attack 15% faster. No tunnels |

Mutation is never something the player manages: a unit is mutated or not, and its drawbacks come with it.

## Heroes

Three abilities with three ranks each (levels 1, 3, 5) and an ultimate (level 6; the Queen's has one rank).

**The Queen**, Aurelith, the colony's young queen and the campaign's protagonist (redesigned 2026-10-21, D86: her first kit of Found, Royal Jelly, One Body and Exodus was logistics, and she felt "totally meh"):
- **Living Light**, her signature, her Holy Light: heals an ally 200/400/600, or sears an enemy for 100/200/300 spell damage; never herself. 65 mana, 5 s cooldown, range 16. Refuses an unhurt ally.
- **Cocoon**, her Divine Shield: she spins a cocoon of living silk around herself and sheds every hostile spell; nothing can harm her for 6/9/12 s, and she keeps moving, fighting and casting. 50 mana, 40 s cooldown, instant.
- **One Body** (aura), her Devotion Aura: allies within 10, the Queen too, take 10/15/20% less damage.
- Ultimate **Rebirth**, her Resurrection: she pours raw substance into her fallen; up to 8 dead allies within 14 rise at full health and mana, and the substance stays in them (+25% damage until they fall again). 175 mana, 150 s cooldown.

Why: Arthas is strong because every one of his spells is one sentence and swings a fight. Holy Light is cheap, quick and big enough to undo a death, and it is her button too, used all match; the cocoon keeps the colony's queen alive while she heals (and she cannot heal herself, so the two lean on each other); the aura makes the army around her tougher without a click; Rebirth turns a lost fight around. Each is the ants' own: living light and blossoms, silk around the brood, the colony as one body, and the ultimate is the first time she uses raw substance, the thing that ruined the other broods: the reborn come back brighter, and changed. It is the hint of what she becomes at the Wellspring. In early missions (level 3 cap) she already has two of Living Light, Cocoon and One Body, at least one at rank 2.

Directions weighed (2026-10-21):
- **The Lightmother** (chosen): the four above. The simplest kit with the most felt power, every spell a mother's: heal and protect, then the forbidden cure.
- **The Substance Sovereign**: a caster-nuker. A lance of light along a line (hurts enemies, heals allies on it), Bloom (a flower ring that roots enemies), an attack-speed aura, and Ascension (she becomes a towering substance being for 30 s). Rejected: a line skillshot is not instantly understood or used constantly; it overlaps the Herald and the Alchemist; Ascension repeats the Hornbreaker's Colossus and the Reaver Queen, and gives her ending away too literally.
- **The Hive Mother**: the first kit made stronger (Found, Royal Jelly, One Body, Exodus). Rejected: her power stays in the economy and the tunnels, so in a fight the player never feels her; Found stays with the Alate, the race's founder.

**Royal Alchemist**, who distils substance in her own body:
- **Volatile Draught**: a flask bursting for 90/140/190 spell damage around the target.
- **Transfusion**: draws 80/120/160 health out of an enemy into the most wounded ally near her.
- **Crystallise**: an enemy is stunned for 2/3/4 s (half on heroes); an ally can't act but takes no damage.
- Ultimate **Overdose**: allies in an area attack 40/60% faster, deal 25/40% more damage and move faster for 10 s, losing 10 health a second.

**General**, the army-ant marshal:
- **Mandible Charge**: rushes an enemy: 80/130/180 damage and a 1/1.5/2 s stun.
- **Mark Prey**: an enemy takes 15/25/35% more damage for 12 s.
- **War Column** (aura): allies nearby move and hit 10/15/20% harder.
- Ultimate **Bivouac**: the General and up to 24 allies lock into a living fortress for 8/10 s: rooted, 60/70% less damage, 35% of melee damage returned.

## Research

| Building | Research |
|---|---|
| Nest | Great Nest (upgrade) |
| Great Nest | Queen's Chamber (upgrade) |
| Brood Mound | Wider Tunnels I–II (50% more throughput each; II needs tier 3) |
| Chitin Works | Mandibles I–III (Raider, Major, Bullet Ant +10% damage), Needle Hardening I–III (Needler, Honeypot, Alate, spires +10%), Carapace I–III (all troops +1 armor); II needs tier 2, III tier 3 |
| Fungus Garden | Barbed Needles, Volatile Sacs, Living Wall, Sticky Nectar, Pheromone Mastery (needs the Scent Hall), Hardened Wings (Alate +100 hp, +20% speed), Venom Glands (tier 3) |
| Acid Spire | Venom Spire (upgrade, tier 3) |

Everything here is written by `tools/content/ants/gen.py`, which holds every number; balance passes edit it and regenerate.

## Building models

Every ant building is a Tripo model of its approved concept, made game-ready and animated by `tools/buildings/ant_building.py` from one config each in `tools/buildings/ants/` (its docstring is the reference). Tripo's segmentation only says where the moving parts are; they are cut from the whole model so a building keeps one material and atlas. Each building animates while it works (D65): swaying banners, a breathing crown, opening mandibles, a dipping blade, beating wings, an opening flower. The definitions keep the old ids (`building.ants.fort` is the Nest, `great-mound` the Great Nest, `elder-hall` the Queen's Chamber, `house` the Brood Mound, `substance-well` the Siphon, `barracks` the Soldier Burrow, `ironroot-forge` the Chitin Works, `bombardier-workshop` the Deep Gallery, `tower` the Acid Spire, `sanctuary` the Royal Chamber); the Fungus Garden, Scent Hall, Queen's Cradle and Venom Spire are new.

Known gap: the Acid Spire plays its recoil as `attack`, once per shot.

## Unit models

Every ant unit is a Tripo model of its picked concept, rigged and animated in Tripo, then made game-ready by `tools/units/tripo_unit.py` (scale, ground, team mask, in-place clips). The three body plans take different paths:
- **Ant creatures** (Worker, Raider, Needler, Burster, Honeypot, Alate, Bullet Ant, Reaver Queen) use Tripo's creature rig, whose only motion is its walk. `tools/units/ant_creature.py` adds the rest from the bones' roles: run, a breathing idle, a bite, sting, spit or slam attack, a recoil, rearing to cast, and a death roll. Creatures are sized by their footprint, not their height, so long-legged ones don't dwarf the rest.
- **Humanoids** (Major, Queen, General) use Tripo's Mixamo rig and presets: walk, run, slash, hit, fall and cast. Tripo's export leaves idle presets out, so `tools/units/humanoid_idle.py` adds a breathing idle.
- **Centaurs** (Herald, Gardener, Royal Alchemist: an upright torso on six ant legs) get two legs from Tripo whatever rig type is chosen. `tools/units/ant_centaur.py` keeps Tripo's humanoid rig and presets for the torso and replaces the lower body: it finds the six feet where the mesh meets the ground, follows each leg up the mesh to the body, adds two bones per leg and a body bone, skins them, and keys a tripod gait into walk and run (front and back legs of one side step with the middle leg of the other). It adds an idle and a death (legs curl, the body sinks); the biped's hips and arm pumping are damped so the body stays level and staffs don't sweep the legs. Its docstring has the per-unit options and the three units' commands. Their `tripo_unit.py` specs set `keepViolet`, so the violet substance (flasks, mushrooms, flowers) stays out of the team mask.

The definitions keep the old ids: `unit.ants.settler` is the Worker, `warrior` the Raider, `archer` the Needler, `queensguard` the Major, `bombardier` the Honeypot, `winglancer` the Alate, `thornblade` the Bullet Ant, `marshal` the General, `banneret` the Herald and `nectarist` the Gardener. The Burster, Reaver Queen, Queen, Royal Alchemist, Venom Spire and the spires' crews (`unit.ants.spire-spitter`, `venom-spitter`) are new definitions. The Amber Adept, Hunter, Rootweaver, Silkwarden, Rootbreaker and Forester lodge are retired.

Known gaps:
- The Gardener's Tripo skeleton missed its torso (it sat inside the basket; a second rig ran its spine up one side), so it has a small skeleton of its own (`--upper`) and every clip is keyed in Blender: a swaying walk, the staff drawn back and flung, the staff raised to cast. Its arms move less freely than the Mixamo humanoids'.
- The Herald's back legs disappear under its cape, so only their lower half steps.
- The Honeypot and Bullet Ant show little team colour: their concepts carry almost no blue.
- The Raider shows little: its fins came out of Tripo more bronze than blue.

## Asset ids

Models: `asset.models.buildings.ants-<building>` and `asset.models.units.ants-<unit>`. Until a model lands, a definition may point at an existing model as a stand-in.
