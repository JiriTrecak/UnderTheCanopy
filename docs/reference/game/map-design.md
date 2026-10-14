# Competitive map design

Author skirmish maps through the live editor MCP (`author-canopy-map` skill).
The numerical contract is [Competitive foundation](../expansion/competitive-foundation.md).
These are initial balance targets; acceptance requires simulation measurements.

## Units and symmetry

Use **C**, the building cell, in design notes. One C is four world/navigation units.
The benchmark is 128 × 128 C (512 world units), with two players and point symmetry.
The navigation extent is −0.5 through size−0.5. Mirror coordinates as
`(x,z) → (size−1−x, size−1−z)`; rotate entity facing by 180 degrees and scenery yaw
by π. This preserves the footprint lattice. Do not mirror around size/2 or round
all entity centres to integers: odd/even footprints have different snapped centres.

Gameplay entities and starts use the editor's compulsory footprint snapping.
Building rotations are multiples of 90 degrees. Organic scenery and landscape
curves remain continuous; they must respect the measured movement corridors.
Use the editor Grid toggle and match Debug → Show world grid to inspect the same C grid.

## Bases, routes and timing

Each main needs a gently graded, dry **24 × 22 C construction shelf**, excluding its harvest
corridors. Prove that the full intended building roster fits, with worker access
between buildings. A Hall is 5 × 5 C, production/research 3 × 3 C, Mound/tower 2 × 2 C.
Leave additional room for actual unit bodies; touching footprints can seal a lane.
Construction allows up to 2 world units of total relief across a footprint and
0.4 world units between adjacent navigation cells. Small terrain bumps are legal;
sharp ledges and substantial rises are not. Keep bases comfortably inside these
limits, especially where the larger Hall may be rebuilt.

Initial travel targets, measured along navigable paths rather than straight lines:

- Opposing approach: about 135 C, or 45 seconds for baseline infantry at 3 C/s.
- Natural expansion: 30–42 C from its main; clearly closer to its owner.
- Centre: 54–72 C; first camp: 18–27 C.
- Primary lanes: 8–12 C clear; main choke: 6 C; flanks: 3–4 C.

Player starts expose `rotation: 0 | 90 | 180 | 270` for the Hall. The starting
formation independently faces the nearest active primary-resource deposit within
the Hall’s gathering radius: five workers on that side, the selected hero opposite.
With no nearby deposit, formation facing falls back to the authored Hall rotation.
Unit navigation rounding rotates with the formation, preserving grid-centred bodies.
Validate clearance for every race and selectable hero, not only the default race.

Skirmishes start with explored terrain under fog. Set `initialExploration` to
`hidden` to require discovery, or `explored` for a surveyed map; omission uses
explored for skirmishes and hidden for campaigns. Initial neutral resources are
known, but enemy units/buildings and subsequent resource changes still need sight.

Mirror gameplay features and verify timings from both starts within 3%. Provide
multiple approaches to contested ground. Camps sit off travel lanes so crossing
the map does not pull them. Resource trees **do block movement** and can be chopped
through: forests shape routes, early scouting and later openings. Measure routes
with the generated tree collisions, water, slopes and landmarks present. Never
assume painted dirt or visible ground proves a traversable passage.

## Economy

An amber site is **five nodes**, each holding 4,500 amber, with one active miner
and an arrival-ordered waiting queue. Arrange two outer nodes nearer the Hall and
three inner nodes farther away. For a Hall facing +Z, use relative node centres
`(-8,24), (-4,28), (0,28), (4,28), (8,24)` in world units; rotate or point-mirror
the whole arrangement with the base. The five-node cluster is centred on the Hall
and occupies the same 5-C width. Adjacent foundations touch at the outer corners
and along the inner row. Workers approach from the Hall-facing side.

Ten workers are the standard economical assignment: 10 per load and 2 seconds
mining. Five active nodes have a 1,500 amber/minute extraction ceiling; a full site
holds 22,500 amber. Measure 5/10/15-worker throughput after authoring;
longer travel can justify extra workers. The initial match starts with five
workers in one row along the Hall, automatically assigned to Amber; ten is the mining recommendation, not a starting-unit count or hard cap.

Keep each node's approach and Hall return lane open. Use the same tested node/Hall
arrangement at symmetric sites; allow room for a future snapped Hall at expansions.
Eight sites for the benchmark: two mains, two naturals, two thirds, two contested.
Mains are unguarded. Naturals have manageable guards; contested sites are harder.

Harvest trees hold **50 wood**. Place equal reachable opening wood at both bases;
wood distance and choke openings after depletion are balance inputs. Keep dense
wood behind base edges without obscuring the building shelf.

Root is advanced economy, **not a Tier 2 prerequisite**. Each deposit holds 1,500,
serves one miner at a time and produces loads of ten over five seconds. Two workers
are the standard target. A Rootworks must fit inside its declared 20-world-unit
placement radius while preserving resource access. Shared guarded sites should
have at least two approaches and an exposed delivery route. Do not require Root
for basic tech progression or place all access behind an unbeatable camp.

## Camps and readability

Use whole camp stamps discovered through `editor_entities action=compositions`.
The existing difficulty bands are total level 3–9 (easy), 10–19 (medium), 20+ (hard).
Match guards to the opening army and route: an accessible first camp, then natural
and contested objectives. Keep every member outside the travel lane's aggro range,
and keep camp leash ranges outside main construction/harvest areas. Test this with
actual generated member positions and declared aggro/leash values.

Equal rewards and equivalent difficulty matter more than decorative symmetry.
Use at most the parser's permitted three legendary camps per map. For larger
symmetric player counts, omit unequal legendary rewards rather than granting one
side an advantage. Do not claim a camp is suitable merely from its level band:
play the opening and measure casualties, clear time and hero progression.

## Landscape and art

Forest masses, shoreline and relief frame the clear playable routes. Avoid a bare
square with scattered props or a continuous circular racetrack. Give clearings
recognisable identities, with limited major landmarks at forest edges. Giant
trunks and mushrooms must not hide the main base or change its usable area.

Forests occupy substantial connected masses and define the map's clearings and
routes; they are not a thin border around a grass field. Author those masses first,
then cut out the measured base shelves, resource approaches and travel corridors.
Use live forest layers. In Vibrant Forest, `recipe.meadow.woodland-edge` derives
grass ground, grass props and undergrowth from distance to generated trees.
A broad mask is appropriate: the tree-distance coverage leaves open ground bare.
Do not cover the map with a grass-ground paint layer, which overrides that dirt
transition. `recipe.grass.meadow` is an additional grass scatter, not a replacement
for the forest-driven ground/undergrowth system. Discover current biome recipe
IDs through MCP before authoring other biomes.
Ferns, flowers, small mushrooms and bank stones dress edges without cluttering
movement space. Recipes and biome profiles own textures and lighting. Keep base
shelves flat; add gentle relief under woods and landmarks. Diagnose actual slopes
with navigation rather than a painted hill outline.

Water crossings must fit the declared lane width **after** body clearance.
A decorative 4-world-unit bridge is not a 6-C competitive choke. Test both deck
connections, ramps and crossing times with the largest supported ground body.
Land crossings are preferable to enlarging a small bridge into implausible art.

Forest and frost editions share gameplay geometry, heights, resource positions,
starts and blockers. Changing explicit foliage recipes/models can change collision;
prove topology equivalence rather than assuming a biome dropdown preserves it.

## Publication and acceptance

Create the document in the editor, author with its commands, inspect, then export
from the editor into `assets/maps/skirmish/<slug>.utcmap`. Command batches may use
coordinate arithmetic for symmetry, but a separate map generator is not the source
of truth. Save screenshots and temporary batches outside tracked assets.

Check map validation, two starts, eight amber sites, legal foundations, base capacity,
resource approaches, creep isolation and navigation timing. Capture overview and
unmodified game-camera views at bases and landmarks. Run AI openings from both
starts; record income, supply blocks, building completion and first engagement.
Verify a deterministic multiplayer replay and cold save restore on the exported map.
Do not label a map accepted while those checks are still pending.

### Hero experience budget

Budget camps from their members' declared combat levels and `rules.experience`,
not from hand-entered XP totals on map placements. The ordinary reward table starts
24 / 44 / 72 / 108 / 152 / 204 XP for levels 1–6. A three-level-1 opening camp
therefore has a 72-XP base pool, before sharing, level-dependent neutral reductions
and Hall-tier bonuses. Check actual fights as well as nominal budgets: successive
kills may cross a hero level and change the multiplier for later kills.

Heroes need 160 / 400 / 720 / 1120 / 1600 cumulative XP for levels 2–6. Neutral
camps can reach level 6 but cannot bank XP beyond it; the first ultimate unlocks
at level 5 and its second rank at level 8 requires player combat. Keep early camp
routes contestable and symmetric, and retain the forest shapes, build shelves and
measured travel-time rules above. Do not change map resource layouts to tune XP.
