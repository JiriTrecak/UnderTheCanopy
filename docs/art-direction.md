# Art direction

**Agreed with Jiri, 2026-10-17 and 2026-10-18 (D61):**
- **The look is a hybrid.** Stylized, crisp, saturated Warcraft III: Reforged-style units and buildings stand on a realistic, ZeroSpace-style forest floor at insect scale.
- **The army comes first.** The playable ground is quiet and toned down, the non-playable frame (trunks, roots, ferns, leaves) carries the detail, and the army is the brightest, sharpest thing on screen ([below](#environment-the-forest-floor-at-low-sun)).
- **Target frames**: `art/styles/target-autumn.jpg`, `target-summer.jpg` and `target-frost.jpg` for an army on the ground; `target-frost-amber.jpg`, `target-frost-cliff.jpg` and `target-frost-base.jpg` for a resource, a cliff with its ramp, and a base with buildings under construction. `target-fall-wide-map.jpg` shows a wider view: the main base, an expansion with its own amber, shallow water units wade through and deep water that blocks the edge. All are generated concepts, not renders: they set the look the game aims for.
- **Units** are dark fantasy in Warcraft's style: readable and chunky. They look like Jiri's three boards in `art/styles/` (`units-ants.webp`, `units-beetles.webp`, `units-bees.webp`; their backgrounds don't count).
- **Factions**: ants are a biological army, beetles run on steam and contained magic, bees are a holy order with blood rituals.
- **Buildings** are made of the faction's creature and materials. The beetle hall is a colossal armoured beetle turned into a fortress (`art/styles/buildings-beetles.jpg`, with alternatives in `buildings-beetle-hall-variations.jpg`).
- **Huge assets** keep the scale in view everywhere.

Except for these points, everything below the research section is Claude's proposal for how to get there, until Jiri settles each part. Each step comes with side-by-side captures, as for every visual decision (plan).

## What ZeroSpace does

From its 15 Steam store screenshots (Starlance Studios and Ironward, early access since 2026-07-20; art direction by Aaron Kambeitz of Homeworld and Relic, who is reported to be steering it towards a heavier, grimmer, industrial look rather than a glossy sci-fi one).

1. **Light carries the frame.** A low, raking sun throws long, crisp shadows across the play area; the shadows are compositional shapes in their own right. The key light is warm and the shadows are cool and saturated (blue to violet). Highlights roll off; nothing clips.
2. **The ground is photographic in material and quiet in value.** Single grass blades, sand ripples, cracked mud, wet sheen and specks of wildflowers. The detail is high in frequency and low in contrast, so units sit on it as on a calm carpet.
3. **A strict value hierarchy.** The ground is mid to dark and desaturated. Units are a step lighter, with crisp silhouettes and panels of team colour. Emissives and spell effects are the only very bright, very saturated things. Dusk and night maps lean on this.
4. **Every map has a material and a colour key** of two or three hues: desert sand, ice sheet, swamp, volcanic plate, city.
5. **Set dressing comes in clusters** along the edges of lanes: rock shelves, crystal growths, beds of alien plants, debris. The lanes themselves stay clean.
6. **The ground remembers fights**: track marks, scorches, blood, dust trails.
7. **Frame edges fall away into depth**: cliffs drop into darkness and haze, and there is never a hard map edge.
8. **Atmosphere** in play is rain, drifting dust and distant haze. Volumetric shafts appear only in cinematics.

What we don't take: sci-fi hard surfaces, neon, and its budget (its recommended GPU is an RTX 3060 Ti).

## Environment: the forest floor at low sun

**The army comes first (Jiri, 2026-10-18).** ZeroSpace reads because its backdrops are clean and toned down against the army, and the same rule holds here:
- **Playable ground** is a calm, smooth, almost uniform realistic surface. It has low contrast, a slightly muted colour and only soft, large-scale variation: just enough detail to sell the illusion of a real surface. It never carries busy texture or scattered objects.
- **Non-playable areas**, such as trunks, roots, ferns and the map border, carry the rich detail, and are still a little darker and less saturated than the units.
- **Units and buildings** are by far the most saturated, sharpest and highest-contrast things on screen.
- **No noise anywhere (Jiri, 2026-10-18).** Surfaces are clean: no grain, speckle, dust-like pixels or sparkle. Detail lives in medium-scale forms (bark grooves, roots, leaf veins, pebble shapes), not in high-frequency texture. Frost is a soft, smooth coat on upward-facing surfaces, never thousands of crystals. For the engine this means:
  - texture detail filtered and capped by mip level;
  - no high-frequency glints or sparkle in materials;
  - anti-aliasing and post effects tuned so nothing shimmers in motion.

  `art/styles/target-frost-cliff.jpg` shows the level.

The rest of this section serves that rule.

- **Light.** Late-afternoon sun, low and warm (amber), with cool teal-blue shadows. The canopy breaks the sun into patches that drift slowly (a light pattern multiplied into the sun term). The tone mapper gets a shoulder instead of today's clip (D31).
- **Ground.** Moss, compacted soil, leaf litter and needle dust as soft, muted materials: height-blended layers and gentle detail normals, never dense grass geometry. Litter, pebbles and twigs belong to the non-playable areas.
- **Biomes swap only ground, frame and light (tested 2026-10-18).** The same frame, with the same beetle base, amber and workers, was re-dressed in five biomes. In every one, the base and the amber stayed the clear focus: `art/styles/target-frost-base.jpg` and `target-biome-light-forest.jpg`, `-dark-forest.jpg`, `-corrupted.jpg`, `-fall.jpg`. A biome is therefore three things over shared units, buildings and resources:
  - a calm ground material;
  - a frame set (vegetation and the colossal trunk);
  - lighting with a colour grade.

  Frame props can be shared across biomes through tinting and the frost coat. Corrupted magic stays magenta-violet, never blue, so blue players keep reading.

  | Biome | Ground | Frame | Light |
  |---|---|---|---|
  | Light forest | fresh light-green moss, pale sandy patches | young ferns, clover, anemones, pale bark | warm, soft, high sun |
  | Dark forest | dark green-grey moss and soil | black-green ferns, near-black trunk, faint teal fungi | one cool pool of light on the play area, the rest dark |
  | Corrupted | grey-violet ash with faint glowing purple veins | twisted black-purple roots, withered ferns, magenta-glowing fungi and motes | dim violet with a cold rim |
  | Fall | cool grey-brown compacted soil, faint litter | gold, copper and rust leaf litter, orange mushrooms, acorns | low golden sun |
  | Frost | pale grey-blue frost | frosted ferns, oak leaves, mushrooms, trunk | cold, clear, low sun |

- **Colour reserved for meaning.** Amber is the resource and glows, so the ground never uses saturated orange-yellow. Team colours belong to units and buildings only.
- **Ground memory.** Trails through moss, scorches, ichor splatter, crushed litter.
- **Edges.** The map's border becomes the giant world (Scale, point 1).

## Scale: the world is enormous

Ranked by effect for the cost:
1. **Giants on the border.** Trunks, roots and fern fronds rise out of frame at the map's edges; their tops are never seen. The surroundings forest (renderer §surroundings) is brought into view at the play pitch, where the map fit hides it today.
2. **Landmarks inside the map**, one to three per map. Each is a forest-floor object used as terrain:

   | Object | Plays as |
   |---|---|
   | acorn cap | plateau |
   | fallen leaf | ramp |
   | mushroom ring | choke |
   | root | cliff |
   | puddle | lake |
   | empty snail shell | ruin |

   Partly there already: `mega-oak-trunk`, `mega-mushroom`, `mega-fallen-log`, `canopy-ancient-tree`.
3. **Canopy shadows.** Large leaf shadows drift across the field, the role cloud shadows play elsewhere.
4. **Air.** Pollen and dust in the light, a drifting seed, a falling leaf crossing the screen. These are cheap particles.
5. **Overhanging foreground.** Out-of-focus fronds at the top edge of the frame, never over play. Riskiest; try last.
6. **Macro optics.** A very faint defocus at the frame's edges at most. A strong tilt-shift reads as a toy miniature, which fights the grim tone.

Giant objects are scenery and never hide a unit; the scenery cutaway already opens around units.

## Units: the three boards

Jiri's boards set the look: `art/styles/units-ants.webp`, `units-beetles.webp` and `units-bees.webp`. Their bright grass backgrounds are not part of it. They are heroic Warcraft with dark undertones rather than grimdark: glossy chitin and big black eyes stay, and the darkness lives in the materials and rituals (bone, contained magic, blood).

**Shared rules, read from all three boards:**
- **Warcraft proportions.** Big heads, oversized weapons, shields and shoulders, short sturdy legs. Two or three large shapes per silhouette, so a unit reads from directly above at normal zoom.
- **One oversized prop per unit tells its role**: a shield, a bow, a hammer, a net, a censer, a cannon.
- **Heroes** are about 1.5× a soldier, with a unique crown shape: horns, a halo, a vine-wrapped boulder.
- **Siege and beasts** are big creatures carrying the weapon (a pod-carrier, a mortar beetle, a reliquary grub).
- **HD materials on simple forms.** Glossy chitin, carved bone, brass, cloth and leaves, with clear edge highlights and no noisy surface detail.

**The factions:**

| | Ants: biological army | Beetles: steam and contained magic | Bees: holy order, blood rituals |
|---|---|---|---|
| Body | glossy rust-orange chitin, ivory mandible tusks | black chitin, squat and armoured | black and gold striped, fur collars, amber wings |
| Materials | everything grown: leaf cloaks and hoods, bark-rimmed leaf shields, bone and thorn blades, silk nets and spools, seed pods, gourds | brass pipes, rivets and gauges; stone-and-brass hammers; tower shields with steel bosses; cannons | ivory plate, gold filigree, bee-skull masks, tabards and banners with a sigil |
| Magic | sap, nectar, silk, roots | green-yellow glowing canisters, steam exhaust | blood in glass reliquaries, red censer smoke, halos, blood circles under casters |
| Board's red, which becomes team colour | maple-leaf cloaks, hoods and shields | elytra plates and shield faces | tabards, banners, capes and shield faces |
| Units in the roster today | warrior (bone axe), thornblade (paired blades), archer (hooded), queensguard (leaf tower shield and spear), winglancer (flying lance), silkwarden (net and spool), nectarist (gourds and seed-head staff), bombardier (back-mounted pod), settler (carried crate), rootbreaker (vine-wrapped boulder club) | horn guard (hammer and tower shield), stone slinger (the board's gunner), worker (wrench), hornbreaker (horned hero, great hammer) | none: bees are a new race |

**What the boards leave open:**
- **Team colour is red on every board.** In the game the red zones become the team colour mask, and faction identity has to come from everything that isn't red. Checked in every player colour, white and dark included (AGENTS.md).
- **Clashes to test.**
  - Ant chitin against orange and red players: darkening the chitin towards burnt umber is the likely fix.
  - The beetles' green glow against a green player.
  - The bees' gold against a yellow player.
- **Value over the new ground.** The boards are saturated and bright, which works: the ZeroSpace-style ground is darker and muted, so units become the brightest thing on screen apart from spells.
- **Bees need game design first.** A new race is gameplay (roster, buildings, abilities). Its art waits for that discussion; the board fixes only how it looks.
- **Buildings** are made of the faction's creature and materials. The beetles' are settled (`art/styles/buildings-beetles.jpg`): the hall is a colossal armoured beetle turned into a fortress, and the rest of the set shares its royal-blue plates, black chitin, brass, riveted steel bases and green glowing tanks. Ants and bees get the same treatment: grown mounds and leaf roofs for ants, ivory-and-gold chapels and hives for bees.
- **Clean models.** Models are crisp, with big calm surfaces; detail clusters at one or two focal points per building, not everywhere. Every building function gets its own silhouette (too many domes read alike), and hall tiers change shape, not only size.
- **Neutrals** keep the corrupted forest (rot, fungus, spore light; the roster already has a corrupted root and a rotwood ancient), drawn with the same proportions.

The bright cartoon image `art/styles/forest-settlement.png` is no longer the target.

## Icons

Every icon in the game (portraits, abilities, research, items, resources, commands, stats) reads at a glance at HUD size and shows exactly what it does (2026-10-20, Jiri: "clean icons that exactly represent what is happening, with the right amount of detail", D76; replaces D67's effect-heavy family). The models are Warcraft III's icons (one painted object in close-up) and StarCraft II's (plain backgrounds, one shared look for commands).

**The HUD's size sets the rules.** At 1080p the command card shows an icon at 46 px, a group's unit cards at about 34 px, the hero bar at 58 px, status badges at 22 px; HiDPI doubles them. Every icon is designed to read at 32 px.

**Shared by every icon:**
- **One subject showing one idea:** the object, body part or effect that does it, in close-up (Storm Bolt is the hammer). Never the caster's whole body, a battle scene or a crowd of small figures; no scenery or ground.
- **A big silhouette**, about 70 % of the frame, readable as a black shape alone. Chunky forms; no filigree, thin strands or sparkle.
- **One warm key light** from the upper left; the subject's lit side is the brightest thing in the icon, a thin rim separates it.
- **Two or three colours.** Effects only where the effect is the subject, as one bold shape.
- **A caster's abilities differ in silhouette and dominant colour**, not only in subject.
- **The bottom-right corner stays calm** for the hotkey; no text, runes or painted frame.

**Painted transparent, composed by the tool.** Icons are painted with a real alpha channel (GPT Image 2.5 Sunburst); glows are part of the icon and semi-transparent. The background comes from a small library in `tools/icons/icons.py` (`BACKGROUNDS`), one per type and nearly invisible on purpose, so changing it for every command is one edit and a recompose. Each icon folder keeps the painted foreground as `source.png` (a working file) and its choices in `generation.json`.

| Type | Subject | Background | Mark the tool adds |
|---|---|---|---|
| Active ability (and autocast, toggles) | the acting object, diagonal; a glow of its energy colour close around it | near-black violet | — |
| Passive and aura | a still, frontal, symmetric emblem; the top-left corner kept calm | near-black neutral | none in the image: the HUD draws a white pulse mark in the cell's top-left corner |
| Research | one improved part as a still life | near-black steel (beetles), umber (ants) | 1–3 gold diamonds top-left for the level |
| Command and stat | one near-monochrome steel symbol, the same look for all | near-black slate | — |
| Unit, machine and hero portrait | the head about half the frame, three-quarter view, its one signature feature; a warm (hero: golden) glow behind the head | near-black warm | — |
| Building | the whole building simplified to its two or three biggest features | near-black warm | — |
| Item | the object, diagonal, a faint glow of its own colour | near-black | — |
| Resource | the resource with a thin dark outline | none: a cut-out for the resource ribbon | — |

**Passives are marked by the HUD, not the icon** (Jiri): a passive is drawn at full brightness like a ready command, with one white pulse mark (a dot inside two rings, `asset.interface.hud.passive-mark`, drawn by `icons.py mark`) in its top-left corner and no hotkey. One colour and no frame, so the HUD can later dim it for an aura that is switched off.

**Upgrade lines get better, not different.** Each level of a research line is the same object in the same pose and size, painted from the previous level's chosen image; only its materials improve (beetles: plain iron → polished steel and brass → royal-blue enamel with a violet substance-glass core; ants: plain chitin → hardened with gold and amber → royal with violet substance veins). The gold diamonds say the level.

**Energy colours** say what an ability does: violet substance and mana, orange fire, forge and impact, warm gold healing, auras and royalty, steel-white armour and blocking, white steam. Substance is a liquid and never green.

**Factions.** Icons are not team-coloured. Ants: glossy sand-and-bronze chitin, ivory mandibles, royal-blue silk and plates, gold trim, amber resin, violet liquid substance. Beetles: glossy black chitin, dark riveted steel, brass, royal-blue plates, violet substance only inside glass, orange furnace fire, white steam. Neutral creatures: their own natural materials; corrupted ones show rot and magenta-violet spore light.

**Making one.** A spec in `tools/icons/specs/` gives each icon its folder, type, faction, concept reference and one subject line; `icons.py requests` writes the requests for `tools/buildings/concept.sh` (two candidates each), `icons.py candidates` lays them out composed over their backgrounds, `icons.py pick` installs the chosen one (256 × 256 PNG; the cooker makes BC7 mips), `icons.py recompose` rebuilds after a background change.

**Review.** `tools/icons/review.py` collects every icon the game uses with what uses it, and `tools/icons/review.html` shows them at 46 and 22 px and enlarged, with a yay or nay (repaint) vote and a comment per icon kept in the published page's database. A repainted icon asks for a new vote. Check each batch in the game too, with `canopy shot --play`.

## References

| For | Reference | What to take |
|---|---|---|
| Whole frame | `art/styles/target-*.jpg` | the look itself |
| Environment | ZeroSpace | a quiet, realistic play ground with detail in the frame |
| Units | Jiri's boards in `art/styles/units-*.webp` | the look itself |
| Buildings | `art/styles/buildings-*.jpg` | the beetle set and hall |
| Units and buildings | Warcraft III: Reforged | chunky, clean, readable models seen in game |

## Budget

The target is unchanged: 60 FPS on a base M1 with about 400 units (plan). Most of this look is lighting, grading, terrain materials and placement, which are cheap. Not planned: ray tracing, volumetrics in play, or dense geometry grass everywhere.

## Order of work

**The next goal is one scene rendered in the game (Jiri, 2026-10-18):** `art/styles/target-biome-fall.jpg`, the beetle base by the amber pit in the fall biome. Faction concepts wait until it renders. Cliffs are left out of this scene.

### What the scene is made of, and what the engine has

| Layer | Engine system | Today | Needed |
|---|---|---|---|
| Calm soil of the clearing | biome `terrain.soil` layer, tiled | layer exists; `woodland-soil` is busier than the target | a calmer soil material; a script that packs ground textures, since nothing in the repository packs `data.bin` |
| Leaf litter towards the edges | biome `terrain.grass` layer (`autumn-leaf-litter`), driven by the map's `grass.grid` and by `groundCover` around props, height-blended with breakup noise | works | a painted band along the clearing's edge |
| Fallen leaves, acorns, mushrooms, ferns | statics from `scenery.json` (instanced, GPU culled, foliage cutout, wind on ferns) | pieces exist: `autumn-leaf-litter`, `canopy-acorns`, `canopy-mushrooms-*`, `pl-fern-*` | dense placement: the editor's prop brush places at most one prop per 3.5 cells, so the first pass uses a script that writes `scenery.json`; later a scatter by mask, per-instance tint and statics LOD |
| Colossal trunk at the corner | a static: `scenery.mega-oak-trunk` (Tripo) | exists | placement and scale |
| Amber pit | the amber deposit model | exists, in the old style | a crater version, later |
| Beetle hall, tower, brood den, workers | live models | only the old beetle buildings | the hall from `buildings-beetles.jpg` through Tripo and Blender; the old buildings stand in until then |
| Warm low sun, long shadows | biome `light` (`sunHeight`, `sunTint`, `sunDirection`) and `--hour` | exists | values |
| Filmic finish, warm grade | biome `grade` (`highlightShoulder`, contrast, saturation, tints, vignette) | a single shoulder knee; every biome uses 0 (clip) | values first; a proper filmic curve if the knee isn't enough |
| Frame darker than the clearing | nothing darkens outside `playable` today | screen vignette only | a playable-area mask in `scene_out` (new, small) |

**Sources (Jiri, 2026-10-18):**
- **Textures** are generated with gpt-image from reference images; its transparent output covers leaf decals and cutout sprites.
- **3D** is made with Tripo and Blender.
- **The beetle main hall** is a new model.
- **Vegetation** is decoration only (D62): nothing is felled, so frame props never need damage states or harvesting.

### Steps

1. **Data-only first render (no engine code).** A small flat test map (256 cells) in the fall biome:
   - soil in the middle and a painted litter band;
   - a script that places the leaf, acorn, mushroom and fern pieces densely around the clearing, with the trunk at the corner;
   - the old beetle buildings as stand-ins;
   - biome light and grade tuned towards the target.

   Shot next to the target at the same framing. This shows how far data alone gets and turns the rest into a measured gap list.
2. **Ground materials.** A packing script and a calm soil and litter pair that match the target.
3. **Engine.** The playable-area darkening, a filmic shoulder if needed, then per-instance tint, statics LOD and scatter by mask, as the gap list asks.
4. **Art.** The frame props at the target's quality, the beetle hall model and the amber crater.
5. **Again side by side, and on the base M1 Medium preset**, until Jiri calls it.

### The look-dev map (2026-10-18)

**Target:** `art/styles/target-fall-lookdev.jpg`. An earlier version of this section compared against `target-fall-wide-map.jpg`, a different generation with another layout, and called the result close. It was not. The layout is now traced from the right frame.

`base/maps/showcase/lookdev-fall.utcm` rebuilds that frame on today's cell terrain, scenery and water, plus the renderer additions of D63 (ground-clutter shadows, foliage variation, a painted frame shade, a painted frame overlay, water glint). The buildings are left out, as the scene's subject is the environment.

**Rendering it:**
- Map: `tools/maps/lookdev_fall.py`. It runs on Blender's bundled Python, which has numpy.
- Overhead shot: `just shot --map lookdev-fall --camera 240,240,2.1 --hour 16.5`.
- Third-person shot: `canopy shot --map lookdev-fall --play --input "focus:warrior;key:J"`.
- The map's layout is traced on the target in pixel coordinates and projected through the game camera, so the shot frames the same composition.

**What it uses:**
- **Ground.** Four gpt-image tileable textures (soil, leaf litter, sparse litter, waterbed), made seamless and packed by `tools/terrain/pack_ground.py`. They are the `autumn-forest` biome's soil, grass, dirt and waterbed layers.
  - The leaf litter follows the map's `grass.grid`.
  - A debris band along the clearing's rim is the map's first paint layer, a Warcraft-style transition atlas (below), toned to the soil's value so it reads as debris on soil, not a stain.
- **The painted frame.** A gpt-image painting of the forest floor (`asset.texture.lookdev-fall-floor`) is the map's terrain overlay (D63). `tools/terrain/build_overlay.py` fades it in smoothly from 1 m outside the clearing and stores it premultiplied, so it never bleeds onto the clearing. The 3D props stand on it; it supplies the dense small detail between them.
- **The frame's edge is leaves, not a cut line** (Jiri: dark, with a sprinkle of nice colour, so it reads at once as not walkable). The same tool cuts 136 single leaves out of gpt-image leaf sheets (separate leaves photographed on white) and scatters them over the edge: about 28,000 dark, dry fragments whose density makes the gradient from litter to bare soil, a few hundred twigs, and 1,900 whole leaves, most of them dry and dark brown, a few golden, each with a soft shadow. Leaves thin out into single small ones on the soil and sink into the frame's shade deeper in. The frame shade (`shade.grid`) starts 14 m out, behind the edge.
- **Frame shade.** `terrain/shade.grid` darkens the frame with distance from the clearing, up to 45 %.
- **Roots.** `tools/props/build_roots.py` makes gnarled bark-root tubes in Blender. Up to eight fan out from each giant trunk, kept in the frame along their whole length, and more are scattered through the frame pointing away from the clearing.
- **Frame props from Tripo** (text to 3D), made game-ready by `tools/props/import_tripo.py` in Blender. It decimates to a triangle budget, shrinks the textures and tones them down.

  | Prop | Triangles |
  |---|---|
  | two giant trunks | 24k each |
  | stump | 8k |
  | arching root | 6k |
  | round mushroom | 1.6k |
  | mushroom cluster | 1.4k |
  | acorn | 700 |
  | amber crystals | 5k, emissive |

- **Leaf and fern props from gpt-image cut-outs.** `tools/props/build_cards.py` keys out the white and bleeds the edge colour. It builds clusters of subdivided, gently curled cards, 144–240 triangles each, alpha-tested. Ferns arch off the ground, so they cast shadows and hold up in third person.
- **Water.** Two profiles: `water.fall-ford`, a pale, murky shallow ford units wade through, with sun glint (D63), given per cell through the map's water flow; and `water.fall-pool`, deep blue-black. Plus fallen leaves floating on both.
- **Amber.** Crystals, tinted orange and faintly self-lit, in a crater dug into the heightfield, with damp soil in the bowl and pebbles thrown up on the rim. On this map they are scenery, not a mine, because a mine's foundation would flatten the crater.
- **Light.** The `autumn-forest` biome: low warm sun from the side (`sunDirection` 120) for long diagonal trunk shadows; a highlight shoulder of 0.6; a darker, warmer grade; a vignette; drifting canopy light. No shipped map uses this biome.

**Measured** on an M4 Max at 1080p (`just shot`, frame to idle):
- High preset: 3.1 ms.
- Medium preset: 2.3 ms. A base M1 is roughly 3–4× slower; that is unmeasured.
- Region averages are within a few levels of 255 of the target for the soil, the pool and the frame's left and bottom. The frame along the top is brighter than the target's.

**Still apart from the target:**
- **Pool.** The target's pool mirrors the dark trunks; ours is a flat dark blue with floating leaves.
- **Amber.** The target's crystals are faceted and glow from within; ours read as a flat orange cluster at this zoom.
- **Frame top.** The target's top edge is dark trunks and roots; ours shows more lit leaves there.
- **Third person.** The fog of war greys everything beyond the unit's sight, and the Tripo trunks end in flat tops that show from eye level. Beyond the map there is still only haze and the old surroundings forest.

**Transitions, Warcraft-style (2026-10-18).** The band where clean soil meets the litter, and the rim of the ford, is a debris layer of twigs and leaf crumbs on soil that breaks up into clumps and speckle. It is built the way Warcraft III builds tileset transitions:
- **The atlas.** An atlas ground (`asset.terrain.lookdev-debris`, made by `tools/terrain/build_atlas.py` from one seamless gpt-image texture) has 16 corner-transition tiles, one per combination of covered cell corners, and 48 full-tile variants. The engine already picks a tile per texture cell from the four corners' paint (`atlas_codes`), as the Warcraft imports do.
- **Seamless meeting.** Every tile's border is the same self-tiling content, so any two tiles meet without a seam.
- **The edges.** They come from noise periodic over one tile and fade out where a tile is fully covered or empty. A speckle dither thins the debris out instead of ending it on a line.
- **What to watch.** One shape per corner combination repeats every texture cell (5 m on this map), as Warcraft's tiles do. More variants per corner combination would need the engine to pick among them.

**Engine work this points to** (each a visible change to discuss first; the frame darkening, the per-instance tint and the painted frame are done, D63):
- a scatter by mask instead of a placement script;
- statics LOD for the giant props;
- art for the surroundings forest at this quality, for third person.

### The canopy (2026-10-18, D64)

The same forest serves two cameras. From the RTS view it is a calm frame around a lit clearing; the canopy shows only as its moving shade and a whisper of light in the air. From the third-person view (J) it is a cathedral: giant oak and beech columns rising into a backlit ceiling, autumn saplings at ant height swaying in the wind, steep beams falling from the canopy's gaps, dust glinting in them, leaves spiralling down, and the distant forest soft with depth of field.

What the scene uses for it:
- **Giants:** Tripo trunks and bare limbs (oak, beech), dressed with leaf-spray cards by `tools/props/build_tree.py`. They dissolve above 75 m at RTS pitches and cast no crown shadow on the playable ground.
- **Saplings:** a Tripo sapling with the same cards, 20 m tall, in the frame away from the clearing's rim.
- **Leaf sprays:** gpt-image cut-outs of autumn oak, maple and beech twigs.
- **Light:** `atmosphere.shaftElevation` 72° for beams from above, `overheadStrength` 0.08 for the RTS view, thin mist.
- **Motes:** dust in the beams and falling maple leaves (`motes` in the biome).
- **Surroundings:** the biome's distant rings now use the look-dev trunk, without the old fly agarics and logs, with a warmer haze.

Still to do: the overhead leaf sheet beyond the map is the old procedural one; a painted far-forest backdrop; leaf litter that piles where the falling leaves land is not attempted.

## Open questions

- **Human leftovers as landmarks** (a lost button, a nail, a coin)? They show scale instantly but are a lore choice.
- **Default skirmish light**: late afternoon (recommended) or midday?
- **How dark is the deep forest**: near night with glowing fungi, or daylight like the others?
- **Ant chitin**: keep the board's rust orange, or darken it so orange and red players stay distinct?
- **Bees**: when does their game design start?
