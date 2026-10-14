# Colony economy

This describes the running economy. The broader migration targets are in [competitive foundation](../expansion/competitive-foundation.md); values below remain current until their migration stage lands. The canonical declarations are in `content/game.json`; engine rules are in `src/sim/game/economy.ts` and `supply.ts`.

## Harvesting and money

Harvest traffic ghosts through units, SC2-style, for every resource: active gather orders, committed return jobs and cargo deliveries pass through units in both path planning and motion execution, and other units pass through these workers. A resource can opt back into collisions with `gatheringUnitCollision: true`. Pending (queued) gather orders do not grant this early; static terrain, buildings and resource footprints remain solid. Carriers drop off, and builders build or repair, at the building side nearest to them, not only at the door. See [unit orders](../declarations/unit-orders.md) for queued delivery and construction semantics.

`item.amber`, `item.wood`, and `item.root` are currencies. Workers carry physical loads to an owned completed drop-off (Main Hall for Amber/Wood; Rootworks exclusively for Root). Money becomes available only on delivery. A full load is **10**. Amber extraction takes **80 ticks / 2 seconds**, wood **320 ticks / 8 seconds**, Root **200 ticks / 5 seconds**; travel is additional.

A standard amber site has **five independent 1 C nodes**, each with **4,500 amber**. Each node admits **one active miner**. Other arrivals wait in a deterministic FIFO queue, with job ID breaking equal-arrival ties. There is **no assignment cap**: two workers per node is a recommendation, not a permission check. Assignment balances over matching nearby nodes within the source's declared search radius. Only completed extraction removes stock; a waiting worker reserves no unmined resource. The last load can be partial. Queues are shared across owners and stored on jobs using `phase` and `arrivedTick`, without a second mutable queue on the source.

A worker's committed trip counts at its current source until delivery; a pending retarget never counts it twice. Node labels show the assignment count; inspection separately reports the recommended count. Queue cancellation, death and depletion release extraction slots. Snapshot validation rejects invalid arrivals or more active miners than the declared policy, and harvesting jobs contribute to lockstep divergence checks.

Each tree contains **50 wood**, collected in five loads. Axe contacts animate once per second while productive extraction advances separately. A tree remains blocking until its final load, then falls for 1.8 seconds before the last carrier leaves. The fallen tree sinks at full scale over six seconds. There are no loose logs or shrinking trees. Interrupting an unfinished load loses that load's work progress, but does not restore previously harvested wood. Serrated Tools doubles productive wood extraction speed. Ranked trees do not regrow, and the unused Forester lodge is absent from worker construction choices. Generic planting/regrowth remains available to explicitly authored custom content.

Standing trees block a movement disc of `collisionRadius` (2 cells, times the tree's scale), not just the trunk cell. Pines are drawn with branch skirts several cells wide and forests are spaced about 3.7 cells apart, so the discs merge: forests are walls that are chopped from the edge inward. Workers chop from the disc's edge. The tree's `footprint` stays one cell for placement clearance, fog and picking; the AI planner uses the same disc.

Delivered currency lives in an authoritative player wallet, independent of buildings. Buildings and training reserve their whole cost from that wallet atomically. Construction still requires an actual worker to reach the site and work, but no material delivery relay is required. Reserved money is unavailable to another order. Cancellation returns currency directly to the wallet, even without a surviving Hall. Destroyed project escrow and dead workers' cargo are lost; earned wallet balances survive every building loss. Wallet state is saved, owner-private, and included in lockstep checksums. No refund or death creates economic ground stacks.

## Supply and training

Each player starts with five workers (automatically distributed across nearby amber nodes) and a hero: **9/15 supply**. Workers are purchased at the Main Hall for 100 amber and take 600 ticks (15 seconds). There are no automatic births or worker-to-soldier conversions.

Every unit definition explicitly declares `supplyCost`; buildings can declare `supplyProvided`. Main Halls and Great Acorn Halls provide 15; completed Mounds provide 8. Worker, Warrior and Archer cost 1 each; Hunter costs 2, Bombardier 3, Marshal 4. `rules.maxSupply` caps colony capacity at 100. These are capacity costs, not periodically consumed resources. Engine unit/building safety limits remain separate.

Supply is derived from living owned units and paid production/revival queues, never maintained as a mutable saved counter. Garrisoned and otherwise contained living units count. Fallen heroes do not count until queued for revival; revival reserves their declared cost. Unfinished, dead and enemy buildings provide no capacity. Upgrading a completed hall retains its current capacity. Authored/scenario spawns may exceed capacity, but still count and prevent further training until space is available.

The HUD displays **used + reserved / capacity**, with the living and queued amounts explained in its tooltip. All queued orders reserve supply and their entire currency price immediately. Insufficient supply or money rejects the request without charging anything. Cancelling a queue entry releases its reservation and refunds only that entry's bill. A Hall can receive physical deliveries and train workers simultaneously. Its worker queue holds escrow outside the wallet, so it cannot fund another purchase.

Before a waiting unit starts, supply is checked again against living units plus **already-started** training. Waiting reservations do not obstruct an earlier queue from starting. Starts are evaluated in deterministic building order, so competing trainers cannot overbook the remaining space. **Once training has started, losing a Mound never pauses it or prevents deployment.** Existing units survive over-capacity. Waiting entries retain their funds and wait for supply; rebuilding capacity or losing units lets them start. Manual pause preserves progress and the started reservation.

Training creates a new unit at the producing building's available exit; workers never walk to or disappear into barracks. Rally orders apply on deployment. Blocked exits retain the finished unit, funds and reservation until a free deployment position exists. Destruction of a trainer releases its queue reservations and loses its reserved inventory. Hero revival follows the same admission/start rules, preserving hero identity and progression. All queue, progress and capacity-loss cases survive save/load deterministically.

Warriors cost 135 amber and take 20 seconds; Archers cost 150 amber and 20 wood and take 24 seconds, both with 1 supply. A match starts with 500 amber and 150 wood. Prices and training times are declared on the output unit, independent of the producing building. Future balance changes need only edit these definitions.

## Placement, AI and authoring

No territory boundary or ownership radius participates in placement. Ownership still governs commands, inventories and hostility. Construction requires explored, clear, dry and suitably flat terrain, legal entrance/access, source clearance, a compatible worker and sufficient funds. Fog is still meaningful.

Placement keeps the workers selected. The first selected eligible worker who is not already building takes the order; if all are building, the first is redirected to the new site. A lone selected worker is always the chosen builder. A carrying worker delivers its current load first, while remaining reserved for the construction order. If no compatible completed drop-off survives, it keeps that cargo while building, then delivers once a replacement opens. The assignment is saved and restored with the match.

A foundation progresses only with one assigned builder present. Stop or a replacement order pauses it; idle workers never take over automatically. Select a compatible worker and right-click the foundation to resume, optionally with Shift to queue it. Repeated assignments do not accelerate construction. Cancellation refunds `rules.constructionRefundPermille` (750: **75%**), rounded down per resource; the remainder is recorded as loss. Unit training, research and upgrades refund **100%** of their paid cost on cancellation.

Workers can construct a replacement Hall for 400 amber and 150 wood in 90 seconds. Standard skirmish defeat requires losing **all owned buildings**, including unfinished foundations. A surviving Mound or paid foundation keeps the colony alive; the starting Hall is not a special defeat target. Mission outcomes remain authored by their scenario. The Home shortcut follows a surviving or replacement Hall, falling back to another building.

The Hall upgrades in place for 450 amber and 200 wood over 80 seconds, without Root. Its footprint, identity, drop-off and supply remain active. Existing worker training pauses and new training waits until the upgrade ends. The upgraded Hall is acquired through upgrading, not a separate build command.

AI reads the same public definitions and owner observations: it trains workers, expands supply ahead of demand, uses recommended node saturation and pays normal costs. It issues explicit replacement-builder orders and can rebuild lost worker production while another building survives. The editor hides currencies from ground placement, forces mines to owner `none`, and preserves yields as initial resource amounts. Authored maps use current definition IDs. There is no migration fallback for old content or saves.

## Hero revival

Sanctuaries charge **200 + 50 × (level − 1) amber**, taking **30 + 5 × (level − 1) seconds**. Definitions declare both base values and per-level increments; simulation, UI, AI and the wiki consume the same terms. Acceptance pays the full price and stores the accepted level. Money held by a revival is included in reserved goods. Cancellation refunds it once; destruction loses the paid cost without touching the remaining wallet or removing the fallen hero.

Revival rechecks supply when work starts, then completes even if capacity falls. A finished revival waits for a body-sized exit and the unit limit, preserving its funds and reservation. Identity, items, XP and learned abilities survive. Ability-driven returns (the `revive` operation) retain their own explicit ability costs and do not incur Sanctuary fees. AI saves for the next fallen veteran while allowing gathering and emergency worker replacement.

Amber deposit presentation is declared on the resource definition through `resourceVisuals`.
Map entity `appearance.asset` selects one of five full crystal shapes. The shared renderer
switches to the low model at one third of the site's starting amount, and to the empty
model at zero. `resource.initialAmount` preserves that authored starting amount in
observations and saved matches. All variants and stages preload with the match.
