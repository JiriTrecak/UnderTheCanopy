# Competitive gameplay foundation

This is the implementation contract for the agreed gameplay migration. Values below are initial balance targets, not claims about measured game performance. Existing combat damage, armor and health remain unchanged during the economy and spatial conversion.

## Spatial contract

Use a building cell, C, as the design unit. Navigation samples at 0.25 C; actors move continuously with explicit collision bodies. Model scale is presentation only. There is no global unit multiplier. Definitions own dimensions, movement and reach; `modelScale` affects presentation only. Projectiles and preview tools use the firing or previewed definition’s presentation size.

- Hall: 5 × 5 C; production and research: 3 × 3 C; Mound and Watchtower: 2 × 2 C.
- Worker/infantry radius: 0.375 C; hero/hunter: 0.5 C; bombardier: 0.625 C.
- Worker/infantry speed: 3 C/s; hero: 3.25 C/s; hunter: 3.5 C/s; siege: 2.5 C/s.
- Archer reach: 5 C; siege: 8 C; melee clearance: 0.15 C beyond the combatants' bodies.

The fixed conversion is **1 C = 4 engine world units = 4 navigation cells**. Navigation retains its existing integer cell centers and continuous fixed-point motion. Cell edges are half-integers; the building lattice starts at the outer navigation edge, −0.5, and repeats every four world units. Snap footprint edges onto that lattice, not the model origin onto an integer: a 5 C Hall occupies exactly 20 × 20 navigation cells; a 2 C Mound occupies exactly 8 × 8. Rotation swaps rectangular extents before snapping. These are coordinate units, not a per-map or per-unit balance multiplier.

Keep the optimized sector routing, ground mesh, body sweeps, shared corridors, spatial indexes and incremental occupancy invalidation. Change their inputs and boundary conversions where necessary, rather than replacing working algorithms. `src/shared/spatial/footprint.ts` owns physical bounds, conservative raster coverage, snapping and navigation-body/rectangle intersection. Simulation, authoring validation, AI blockers and placement rendering share its geometry. Foundation placement tests the same conservative square body footprint used by terrain navigation, at continuous positions; a center outside the footprint does not prove that the body is outside.

Calibrate the classic RTS camera by visible battlefield coverage and unit/building proportions, not by copying an unverified FOV constant. The map importer uses 25.6 source units per world unit: a 128-unit source terrain tile becomes 5 world units, while our building lattice remains 4, so a 512-unit source hall foundation matches our 5 C Hall. Imported atlas size/origin are saved separately from navigation; native map editing and body dimensions retain their established grid.

## Money and harvesting

Earned currency belongs to a player wallet. Destroying any building, including the last Hall, cannot destroy that wallet. Physical cargo is credited only on delivery and is lost with its carrier. Queued production and construction hold paid escrow; a destroyed project loses its remaining escrow. A cancellation refunds according to the relevant policy even if no Hall survives. Research and upgrade tasks also own their paid costs. Wallets are authoritative, saved, validated and included in multiplayer checksums.

An amber site contains five nodes with 4,500 amber each (22,500 total). A node serves one miner at a time. Others wait in a deterministic queue; there is no hard assignment limit. Each completed load carries 10 amber; extraction takes 2 seconds. The standard Hall-to-node arrangement is a centred, touching arc: two outer near nodes at local (−8,24)/(8,24), and three inner far nodes at (−4,28)/(0,28)/(4,28), in world units relative to the Hall. Ten workers are the economical baseline. The fifth node increases extraction capacity from 1,200 to 1,500 amber/minute; measured deliveries with ten workers are 1,410–1,420/minute after warm-up. More workers mostly wait but fill some far-node travel gaps. Measure these rates on each authored layout.

Trees contain 50 wood and yield five loads of 10. A load targets 8 seconds of productive chopping plus travel. Trees disappear only when depleted. No ranked forest regrowth. Root is an advanced economy resource, not a Tier 2 prerequisite; initial target: one active miner, 10 per 5 seconds, 1,500 reserve, two-worker standard saturation.

## Supply, production and construction

Start with 500 amber, 150 wood, five workers and the chosen level-one hero: 9/15 supply. No free starting warriors. Hall supplies 15; each Mound adds 8; maximum supply is 100, without upkeep. Worker, Warrior and Archer cost 1 supply; Hunter 2; Bombardier 3; every hero 4.

Supply is checked at queue admission and at the beginning of training. A started job is never paused by losing supply. A queued job that cannot start waits. Currency is paid once on queue admission; cancellation returns its paid cost once. Ordinary production refunds 100%; voluntary construction cancellation refunds 75%. Destroyed queues lose their paid costs. Construction requires one present builder and pauses without one. Resumption is an explicit worker order. Losing all buildings (foundations included) defeats a standard skirmish colony; losing the original Hall alone does not. Wallets survive defeat. Mission objectives remain separate.

Initial costs and durations:

- Worker: 100 amber, 15 s. Warrior: 135 amber, 20 s. Archer: 150 amber + 20 wood, 24 s.
- Hunter: 200 amber + 40 wood, 30 s. Bombardier: 220 amber + 60 wood + 30 root, 32 s.
- Hall: 400 amber + 150 wood, 90 s. Tier 2: 450 amber + 200 wood, 80 s; retains footprint, drop-off and supply while worker production waits.
- Mound: 100 amber + 25 wood, 25 s. Barracks: 200 amber + 60 wood, 50 s.
- Chitin Works: 150 amber + 75 wood, 40 s. Sanctuary: 180 amber + 60 wood, 40 s.
- Watchtower: 100 amber + 50 wood, 35 s; retains archer garrison behavior.
- Workshop: 220 amber + 120 wood, 50 s. Rootworks: 150 amber + 75 wood, 40 s.

## Hero roster

Choose the first hero before the match. Tier 2 allows a second distinct hero; Tier 3 allows a third. Death retains the hero's identity, progression and roster slot. Revive that hero rather than creating a replacement. Revival target: 200 + 50 × (level − 1) amber and 30 + 5 × (level − 1) seconds. The catalogue currently contains one faction hero; roster support must not invent additional hero assets. Initial tuning: additional heroes cost 425 amber + 100 wood and take 55 seconds; Tier 3 costs 600 amber + 250 wood and takes 100 seconds. Elder Hall reuses the approved Acorn Hall art.

## Maps and verification

Maps are authored or imported through the editor command system. The editor and the author-canopy-map skill share the visible building grid, compulsory snapping for gameplay placements, footprint and clearance previews, and measurements expressed in C. Scenery varies naturally, while generated resource trees and other pathing obstacles respect the competitive layout. The skill teaches classic RTS map-design practices with the current numeric rules.

Matches also expose the same terrain-following world grid through Debug → Show world grid. This is a local visual diagnostic, available in multiplayer without changing simulation, revealing fog or requesting hidden navigation state. Disabling Show world grid hides it and frees its geometry; CPU profiling is independent.

Author a 128 × 128 C benchmark 1v1 through the editor command system. Target a 135 C opposing approach (45 seconds for baseline infantry), natural expansion 30–42 C away, center 54–72 C away, first camp 18–27 C away. Give each base a 24 × 22 C building shelf excluding harvesting corridors. Primary lanes: 8–12 C, chokepoints: 6 C, flanks: 3–4 C. Eight resource sites: main/natural/third per player and two contested. Forest and frost variants must have identical competitive topology.

Acceptance includes conservation of currency, interrupted harvesting, oversaturation curves, legal building clearance, passage by different body sizes, queue cancellation, over-cap completion, hero death/revival, restored snapshots, deterministic command replays and multiplayer state equality. Record actual timings and throughput, not only theoretical rates.

## Implementation

The numerical values above are implemented initial tuning, not a claim of
tournament-ready balance.

### Spatial rules, placement and navigation

`src/shared/spatial/footprint.ts` owns the C lattice, exact rotated footprints,
rasterization and body/rectangle overlap. Construction, editor placement/drag,
90-degree building rotation, spawn formation, AI placement and grid rendering
use this contract. Static half-cell centers survive saves and observations;
movement retains fixed-point positions and integer navigation addresses.

All 48 units have explicit dimensions. Worker/infantry height is 4.8 world units,
hero height 5.6; movement and body-edge weapon reach are definition-driven.
Visual `modelScale` is separate and shared with the asset/spell previews. The
former global unit multiplier and assignment-cap fields are removed.

The existing ground mesh, sector corridors, spatial indexes, shared-route cache,
local detours and incremental occupancy invalidation remain. Actual body radius,
height, layer and locomotion flow into route validation, collision, formation,
production exits, revival and AI movement. AI reserves service lanes for future
large units and plans against known obstacles rather than hidden enemies.

Regression coverage includes `tests/shared/footprint.test.ts`,
`tests/shared/placement-unit-bodies.test.ts`, `tests/game/unit-bodies.test.ts`,
`tests/game/placement-bodies.test.ts`, `tests/game/route-origin.test.ts`,
`tests/game/movement-knowledge.test.ts` and `tests/ai/body-navigation.test.ts`.
These exercise all building rotations, mixed body sizes, body-overlapping
foundations, passage width, bridge headroom, dynamic blockers, cold caches and
hidden-occupancy isolation. The 24-infantry 6 C counterflow fixture completes
without overlaps or stranded units. Existing indexed/reference sweep and
navigation-cache regressions remain enabled.

### Wallets, extraction and production

Currency is held in `state.wallets`; escrow stays with the paid task. HUD, AI,
observer income and saved-state validation use those same funds. The wallet
survives Hall loss; cancelled escrow refunds once and destroyed escrow is lost.
Cargo is credited on delivery, including final partial extraction loads.

Harvest arrival order lives on saved jobs; one miner is active per amber node,
and assignment remains unlimited. Dead/stopped miners release capacity. Trees
hold 50 wood and produce five ten-unit loads; only depletion fells them. Ranked
content does not regrow wood. AI maintains gather orders and uses recommended
staffing rather than an assignment cap.

Training creates new units and leaves workers intact. Admission and start
supply checks are distinct; losing capacity never pauses started training.
Foundations need one explicit builder, pause without one, and resume by order.
Hall upgrades preserve footprint, drop-off and supply. Losing every building
(including foundations) defeats a skirmish colony; mission rules stay separate.

`tests/game/wallet.test.ts`, `tests/game/competitive-harvesting.test.ts`,
`tests/game/competitive-production.test.ts`, `tests/game/tree-felling.test.ts`
and `tests/ai/colony-recovery.test.ts` cover conservation, FIFO extraction,
oversaturation, interruption, finite reserves, worker retention, timed training,
refunds, supply loss, loaded-worker rebuilding and cold restoration.

### Hero selection and roster

Local setup and multiplayer lobbies expose the declared starting-hero catalogue.
The server accepts only an owned seat's valid choice, broadcasts it, and freezes
it into the match config before Start. Restart and saves retain that choice.

The highest completed Hall tier provides one/two/three distinct roster slots.
Living, fallen and paid queued heroes reserve identities; temporary summons do
not. Revival retains XP and learned state, captures its paid level/price, and
waits for a body-clear exit. Production and revival share the Sanctuary queue.
Upgraded Halls satisfy lower-tier prerequisites.

`tests/game/starting-hero.test.ts`, `tests/net/hero-selection.test.ts`,
`tests/game/hero-roster.test.ts` and `tests/game/competitive-revival.test.ts`
exercise real commands, duplicate/capacity rejection, mixed queues, Hall loss,
level-based payment, cancellation, blocked exits, multiplayer and cold saves.
Only the Marshal is currently shipped. Additional distinct heroes are proven
with declared test content; no unrequested hero models were invented.

### Camera, assets and authoring

The default perspective shows **32 C horizontally at 16:9**, with a classic RTS
vertical lens (about 34.32°), 56-degree downward pitch and north-up orientation.
The full viewport stays within map bounds, including at maximum zoom and after
resizing. The minimap uses that same projection, producing a horizontal trapezoid.
See `src/render/camera/camera.md` for the lens and framing contract. Infantry projects to the tested
40–60-pixel height band at 1080p. `tests/render/camera.test.ts` also covers terrain
following, camera modes and narrow editor panes.

`tests/assets/competitive-dimensions.test.ts` verifies the actual shipped
building meshes, including amber depletion stages, fit their declared foundations
at runtime scale. Asset Studio uses the same presentation scale. The match was
launched through the real menu, with the hero selector, 10/15 starting supply,
Hall worker training and Debug → Show world grid checked in the live UI.
Map menus, multiplayer lobbies and the editor all display dimensions in C.

The map editor enforces gameplay snapping and exposes the shared grid. The
`author-canopy-map` skill and [map design guide](../game/map-design.md) document
C-based authoring, base shelves, measured routes, whole camp stamps and export
through the live MCP command system. Scenery remains organic. Forest masses
define the clearings; the existing woodland-edge meadow generates grass ground,
grass fringe props and undergrowth from tree distance, leaving open dirt. That
recipe is now in the biome's foliage menu.

### Shipped maps

The shipped skirmish maps are Echo Isles and Turtle Rock in
`assets/maps/skirmish/`, converted by the map importer into ordinary editable
maps with five-node amber sites and neutral camps.
`tests/game/imported-map-playability.test.ts` checks every published map for
usable starts, reachable camps and a deterministic save continuation.

## Hero progression contract

The shipped Marshal caps at level **11**, with cumulative XP
`0, 160, 400, 720, 1120, 1600, 2160, 2800, 3520, 4320, 5200`.
Each level including the first provides one learn point. Three regular skills
use gates `1/3/5`; the two-rank ultimate uses `5/8`. Gate validation rejects
unreachable or non-increasing requirements. AI learns through the same command
path, preferring newly unlocked ranks and then breadth.

`rules.experience` owns the ordinary-unit and hero-level reward tables, summon
multiplier, neutral ceiling, per-level neutral multipliers, global fallback and
solo-hero Hall-tier bonuses. Shipped positive ad-hoc `experienceYield` values are
removed; explicit zero excludes workers and split bodies. Authored overrides
remain available for deliberate scenario content. Ordinary combat levels drive
rewards; Bombardiers are level 3. Kill credit comes from the authoritative lethal
hit's captured owner, including released spells whose caster died or changed
allegiance. Friendly kills and non-combat removals earn nothing.

Nearby eligible allied heroes share one pool within their authored radius
(Marshal: **10 C**). If none qualify, only the credited owner's eligible living
heroes receive the global fallback. Stable ID order assigns integer remainder
points. Apply recipient multipliers after splitting, round down once, then clamp;
no redistribution or banking of capped overflow. Neutral rewards at levels 1–5
are `100/90/80/70/60%`, ending exactly at level 6 (1600 XP). Player-controlled
victims continue progression to level 11. Summons pay 50%. A single rostered hero
receives tier-2/3 bonuses of 15/30%; fallen and paid queued heroes count in the
roster. Missions may cap progression earlier. Stat growth preserves wounds and
mana expenditure by adding only the maximum-pool increases.

Marshal's published bindings are `ability.marshal.faultline`,
`ability.marshal.rally`, `ability.marshal.carapace` and
`ability.marshal.crownfall`. Their ranks, target queries, damage, shields,
reflection, hero-specific stun durations and linked effects are ordinary authored
data, with no bespoke spell runtime. The [hero guide](../wiki/guide/heroes.md)
describes the kit.
