---
name: canopy-units
description: How to make a unit's model for Under the Canopy, from concept to a rigged, animated, team-coloured model in the game — the readability standard, materials by race, turnaround concepts (front T-pose and side), equipment as separate models, Tripo image-to-3D in quads within triangle budgets, rigging and clips, attaching equipment, and checking units in the Combat Lab. Use before designing, painting, generating or importing any unit or hero model.
---

# Units

Read **canopy-assets** (asset folders, ids, cooking) and **canopy-definitions** (the unit's definition) first. The design work lives in the unit exploration workbench, `explore/units/` (open `index.html` from disk; its content is `data.js`): the readability standard, each race's identity, the agreed sheets and the open questions. The ants' first five (Worker, Soldier, Hunter, Door-head, Queen) are agreed there.

## Rules (Jiri's)

- **Readability first**, the way Blizzard designs for the RTS camera:
  - every unit has a body plan of its own and passes a black-shape test at game zoom, from above and at game angle;
  - proportions pushed past natural: big heads and signatures, thick limbs, nothing thin carrying the read;
  - one oversized feature that says the role;
  - a size ladder: worker < soldiers < elites < heroes < the centrepiece;
  - a large royal-blue team-colour area on the top of every unit.
- **Grounded in a true fact**: every unit grows from one real, surprising trait of its animal (a door-head ant is born a living door), exaggerated until it reads from above.
- **Body plans**: the ants' regular castes are bipedal with their real trait exaggerated; their mutants are wild creatures. Concepts are drawn from the RTS camera (about 55° down) and checked small.
- **Materials by race**: metal is the beetles' (technology), ivory and bone the bees' (the holy order); the ants wear only what the colony grows, gathers or secretes (chitin, amber, resin, leaf, silk, clay, fungus), never metal.
- **Every model comes from Tripo** from an approved concept. Blender fixes, splits, rigs and animates; it never invents the geometry.
- **Ask before anything visible**: bring concepts and captures, and wait for Jiri's choice.

## Pipeline

### 1. The lineup concept

Units are designed together, on a sheet of the race's units in a row (gpt-image-2.5 Sunburst through `tools/buildings/concept.sh`, references staged inside the old repository's `.asset-work/concepts/`). Iterate the sheet until Jiri agrees it; record every round in the workbench.

### 2. The Tripo reference

A lineup is not a modelling reference. For each agreed unit, paint **one single-unit front shot** from the agreed sheet (cut the unit out of the sheet as the reference):
- a bipedal unit in a **T-pose** (arms straight out to the sides, hands open and empty, legs straight, slightly apart); a creature (four or six legs) in a **neutral stance**, legs evenly spread, body level;
- the whole unit centred, seen straight from the front, orthographic-looking, on plain light grey, soft even light, no ground, shadow or floor, **no equipment** in the hands or on the back, no effects, no text;
- **the sheet's proportions**: the same head size, bulk, stance width and limb thickness as the agreed unit. Lay the shot beside the sheet before Tripo; a T-pose painting tends to come out taller and thinner, and Tripo reproduces its input faithfully.

Start the prompt with "ONE SINGLE FIGURE ONLY": without it the model often returns a whole character sheet. Keep the shots in `explore/units/images/turnarounds/` (`<unit>-front.webp`).

**No side shots for Tripo.** Painted side views invent their own body (the ants' Soldier got a long neck and a thin torso from its side shot), and Tripo's multiview trusts them over the front. One front image at full detail keeps the design (2026-10-29).

### 3. Equipment is its own model

Anything a unit holds or carries (a sword, a shield, a staff, a carried load such as the Worker's amber chunk) is a **separate Tripo model** from its own concept (the object alone, centred, light grey, one view). It is attached in Blender to the hand or back (below), so the body rigs cleanly in its T-pose and a weapon can change without a new body.

### 4. Tripo

Run Tripo with skill **canopy-tripo** (Claude in Chrome, no uploads by Jiri): **Image to 3D** of the front shot, single image, **Quad**, full detail; then **Retopo** to the budget:

| Model | Triangles | Retopo polygon count |
|---|---|---|
| Unit | 5k | about 2,000 quads |
| Hero | 10k | about 4,000 quads |
| Equipment and props | 5k (most far less) | what the piece needs |

Never decimate in Blender, which wrecks the quads.

- **Rig and animate in Tripo:** a biped as **humanoid** (Mixamo skeleton) with its preset clips; a creature as **Other** (`Root`, `Spine_n`, `Head_n`, limbs), whose clips are made in Blender (`tools/units/ant_creature.py`, `ant_centaur.py`). Export one GLB with the skeleton and every clip. Keep raw exports in the scratchpad, never in the repository.

### 5. Blender: game-ready

`tools/units/tripo_unit.py <spec.json>` makes a rigged Tripo character game-ready: scale to its height with its feet on the ground, facing −Y; the royal-blue plates become the team mask (base colour alpha); Tripo's clips renamed to the game's semantics with the attack contact event; clips in place (`tripo_in_place.py`); idle added where missing (`humanoid_idle.py`); scene poses (`scene_poses.py`); seams closed (`loop_clips.py`). It writes `base/assets/units/<race>-<unit>/` (`geometry.glb`, `source.blend`, `asset.json`).

**Equipment** is imported as its own mesh, scaled, and parented to the hand bone (or the spine for a carried load) with its grip at the hand: a rigid node, not skinned. Name the attach points as sockets (`socket_hand_r`, `socket_back`) so effects and swaps can find them. Give a group two or three variants of its move cycle so it never marches in lockstep.

### 6. Check

- The unit definition (skill canopy-definitions) names the asset; `cargo test -q -p canopy-content --test shipped_data`.
- Cook, then the roster lineup in the Combat Lab: `labs/ant-roster.lab.json` has every ant in blue and red; `canopy capture-screens --only lab` shoots it (`lab-ant-roster-*`). Compare against the agreed sheet at game zoom, in every player colour including white and dark (AGENTS.md, Visual previews).
- Note the player-visible change with `just release-note`; `just check` before committing.
