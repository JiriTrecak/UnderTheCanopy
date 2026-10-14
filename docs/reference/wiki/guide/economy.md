# Economy and supply

Your economy has three spendable resources: **[Amber](/resources/item-amber)**, **[Wood](/resources/item-wood)** and contested **[Root](/resources/item-root)**. Your workforce is a separate strategic constraint. Workers gather and build; military units train independently at their production buildings.

## Gather → carry → deposit

Workers assigned to an Amber Mine pass through other units on the outward and return trip. Terrain, water and buildings still block them. Tree gatherers pass through units the same way; ordinary movement retains unit collision.

Select Workers and right-click a [mine](/buildings/building-neutral-amber-mine) or [tree](/resources/resource-forest-tree). They approach, harvest and carry the load to a completed owned [Main Hall](/buildings/building-ants-fort). Resources become spendable **on delivery**, not when harvesting starts.

There are no lumber mills, planks, stone chains or economic piles on the ground in the current game. Ranked forests do not regrow. Hero loot chests are separate from economic resources.

A dead worker loses its cargo. Earned money remains in your wallet even when buildings are destroyed. Keep the paths between sources and your drop-off safe.

## Mines and trees

Amber Mines are neutral buildings already present on the map. You can select them, inspect their remaining yield and assign workers directly. They do not need to be captured or constructed.

A standard amber site contains **five nodes of 4,500 amber**. Each node serves **one miner at a time**; additional miners wait in arrival order. **Two workers per node** is the normal recommendation, not an assignment cap. Ten workers can nearly saturate a well-placed five-node site. Longer walks may justify extra workers. The label shows how many workers are assigned, and inspection shows the recommendation.

Workers spread across nearby matching nodes. A carrier remains assigned to its current node until its load is delivered, even when given a new gather order. Exhausted nodes can redirect to another matching node within their local search radius, not to a mine across the map.

Trees are directly targetable, including their canopies. Each contains **50 wood** collected in **ten-wood loads**. Forests remain solid until trees are depleted; a depleted tree falls and sinks away, opening its space for movement. Timber workers continue to nearby trees. Ranked forests have no automatic regrowth or Forester rebuilding.

## Contested Root and Tier 2

[Corrupted Root](/buildings/building-neutral-corrupted-root) grows outside the
starting bases, near guarded camps. Each deposit contains **1,500 Root** and
serves **one active miner**, with other workers waiting. Two assigned workers are the recommendation, shared across players. Workers carry
**10 Root per trip** and use the same unit pass-through rules as Amber miners.
Terrain and buildings still block them.

Build a Tier 1 [Rootworks](/buildings/building-ants-rootworks) within 20 cells of
a deposit. It is the **only Root drop-off**: a Mound or Bombardier Workshop cannot
receive a worker's Root cargo. Once delivered, Root enters the colony's shared
spendable account. Protect both the deposit and its delivery route.

Upgrade your starting [Main Hall](/buildings/building-ants-fort) in place to a
[Great Mound](/buildings/building-ants-great-mound) for **450 Amber and 200 Wood**, with no Root requirement. The upgrade takes 80 seconds and pauses that hall's unit training.
Canceling refunds its full price; destruction does not. The same building
remains your defeat-condition objective after upgrading.

Great Mound unlocks [Hunters](/units/unit-ants-hunter) at the Barracks and the
advanced [Bombardier Workshop](/buildings/building-ants-bombardier-workshop).
Hunters cost Amber and Wood; no existing worker is consumed. Bombardiers also require
Root for every recruit. The [Ironroot Forge](/buildings/building-ants-ironroot-forge)
buys permanent colony-wide research; some research requires Great Mound and Root.
Research benefits existing and future eligible units, survives losing the Forge,
and refunds in full if canceled while queued or in progress.

## Supply

{{stats:population}}

Every unit consumes its declared supply. Workers, Warriors and Archers cost 1 each, Hunters 2, Bombardiers 3 and the Marshal 4. The HUD shows **used + reserved / capacity**. Hover it to distinguish living units from queued reservations. Supply is shared across the colony, including garrisoned units; unfinished buildings provide none.

{{stats:limits}}

## Training an army

{{stats:recruitment}}

Train workers at the Main Hall and soldiers at their production buildings. Training reserves the complete price and supply immediately; no existing worker is consumed or needs to approach the building. A fresh colony begins with five workers and a hero at 9/15 supply; Mounds add eight capacity each.

Supply is checked again when a waiting unit starts training. **Losing a Mound does not pause units already in training.** They finish normally, even over capacity. Waiting entries keep their money and resume when enough supply is available. Existing units are never killed by capacity loss.

Cancel any queue entry to refund its held price and release its supply reservation. Blocked exits retain completed training until a deployment position opens. Destroying a trainer loses its reserved resources and releases its queue's supply. Reviving a hero also reserves supply; a started revival finishes despite later capacity loss.

## Construction and refunds

Workers can build on explored, clear, dry and sufficiently level terrain with accessible entrances. There is no owned territory radius or border-post requirement. Resource sources, water, obstacles and other buildings still restrict placement; mines reserve access space around themselves.

The full bill is reserved from your wallet when you commit. One assigned worker then reaches the site and builds. If that worker stops or dies, select another worker and right-click the foundation to resume. Idle workers do not take over automatically. No courier chain is needed. Damaged buildings can be repaired without a resource bill.

Cancelling construction returns **75%** of each paid resource, rounded down; cancelling recruitment returns **100%**. Both refunds go to your wallet even without a surviving Hall. Destroyed buildings never destroy earned money; unfinished purchases and undelivered cargo can still be lost. Cancelled training never consumes or creates a unit. A surviving building or foundation keeps your colony alive if the Hall falls; workers can build another Hall for 400 Amber and 150 Wood over 90 seconds.

## Load size and initial scale

Workers bring **10 resources per full trip**. Amber takes 2 seconds of extraction, wood 8 seconds and Root 5 seconds; travel is additional. A depleted source can produce a smaller final load without creating or losing resources. Prices and starting resources are declared in `content/game.json`; see the [economy contract](/development/game/economy).

## Chopping and felling trees

A worker swings regularly while extracting each ten-wood load. The tree remains standing through the first four loads. Its final depletion starts a **1.8-second fall**, then the last carrier takes the wood home. The fallen tree sinks at full size for six seconds and disappears.

Stopping an unfinished load resets its work progress, but previously removed wood stays removed. Only an admitted worker extracts; waiting workers cannot duplicate the harvest. Serrated Tools halves extraction time without changing the tree's fifty-wood reserve.
