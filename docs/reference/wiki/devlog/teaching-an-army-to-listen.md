---
title: Teaching an army to listen
description: How we rebuilt movement and combat feel in Under the Canopy—and why finding a path was only the beginning.
date: 2026-09-10
prev: false
next: false
---

# Teaching an army to listen

*Behind the Canopy · Devlog 01 · September 10, 2026*

When building any RTS, there are many important systems, but among the most important is unit control and behavior. Microing units is, after all, one of the biggest differentiators between noobs and top-tier players. Knowing when to retreat, saving a wounded unit, getting an archer into range, or squeezing an army through a choke can decide an entire match.

For that to work, the player needs to trust the army.

That was the part of *Under the Canopy* I wanted us to really attack. We already had our ants, buildings, economy, heroes, and a forest worth fighting over. But sometimes an order felt late. Units took strange paths. An attack could fail because its target moved a tiny amount. The animation and the damage did not always tell the same story.

You can add a hundred interesting units to a game like that, and every one of them will inherit the same frustration.

So we spent a substantial pass on the foundation: what actually happens between clicking somewhere and an ant doing what you intended. We made a huge improvement. We also discovered that “fix the pathfinding” is a deceptively small phrase for a surprisingly large problem.

## Start with the army the player already has

One of our most useful discoveries came from an embarrassingly ordinary test: twelve warriors, open ground, a short move to the right.

There was no river to cross. No building to walk around. No enemy army. Just six cells of empty space.

The group took 4.075 seconds to finish the move.

At their movement speed, the distance itself called for roughly 1.5 seconds. Where was the rest going?

We were assigning a fresh formation at the destination. Even when the army was already compact, the new arrangement could send units across each other's paths. A small reposition became an unnecessary reshuffle. Local collision handling then had to deal with the traffic we had just created ourselves.

The useful question was: **does this group need a new shape at all?**

Now, when a compact group can safely translate its existing footprint to the destination, it keeps those relative positions. We check that the destinations are valid and distinct, and that terrain permits the straight routes. If those checks fail, the ordinary destination search takes over. The ants still turn and respect physical collision along the way.

![Illustrative crossing routes versus a translated formation, with the measured short-move completion times of 4.075 seconds before and 1.6 seconds after.](/media/devlog/formation.svg)

*The routes above are explanatory sketches. The timings come from the same twelve-warrior, six-cell simulation fixture—not from a render-speed test.*

That short move dropped to **1.6 seconds**. Average distance traveled fell from 7.3 cells to exactly six. The same twelve-unit group's longer diagonal move improved from 390 simulation ticks to 249.

We had made movement feel faster without increasing the units' speed.

There were still tradeoffs: two larger reversal fixtures finished a few ticks later. That matters. An improvement to one maneuver does not automatically improve every maneuver. But the short-move result gave us a very clear lesson: before making avoidance smarter, stop asking it to resolve avoidable conflicts.

## Finding the route was only the beginning

Our earlier movement had that familiar grid-like feel: units appeared to favor a handful of directions when I wanted them to travel naturally toward the point I clicked.

The navigation layer now searches an eight-neighbor grid with A*, then straightens routes where the terrain allows it. Actual unit positions have finer precision than the grid. An ant can travel along a clear arbitrary angle; it is not restricted to performing a staircase of cardinal or diagonal steps.

But this introduces a subtle problem. A pathfinder might reason from the center of a cell while a unit is physically standing near its corner. A route that is valid from the center can cut a wall from that actual position.

We found exactly that kind of failure during interrupted movement. The correction connects the unit's real position to the route through a checked cell-center waypoint when necessary. The unit walks that connection normally. No snapping it into place to make the math convenient.

This became a recurring theme. A plausible plan still needs a physically valid next step.

Terrain routing answers how to get around a wall. It does not, by itself, answer how two armies should negotiate the wall's only opening, or where the last bombardier should stand when its faster allies have already filled the destination.

Those need their own decisions.

## Research helped us ask better questions

Classic RTS games were an obvious reference for the feel we wanted. Their command sets make the distinctions between Move, Stop, Hold Position, Attack-move, and Patrol concrete. Move follows the destination despite enemies; Hold can fire without chasing; Patrol engages and then resumes its route.

Those are player-facing promises. Two buttons that look different but collapse into the same underlying behavior cannot support the same decisions.

For crowd movement, we also looked at [Optimal Reciprocal Collision Avoidance, or ORCA](https://gamma.cs.unc.edu/ORCA/). The authors describe agents sharing responsibility for avoiding a collision. That is a useful way to think about a crowd: each participant's movement affects the options available to the others.

We did **not** implement ORCA. Our game has its own constraints, including finite turning, collision rules, and a simulation that must agree across multiplayer peers. Reading an algorithm does not make it an automatic fit.

Research gave us vocabulary and questions to investigate. The rules we shipped still needed to make sense in our game and survive our tests.

## The ants at the doorway

Imagine two lines of ants meeting at a narrow opening. One ant wants to enter, another wants to leave, and a third stands immediately behind the second.

Everybody has a route. Nobody has a free next step.

An early temptation is to treat the other ants like additional walls and run the global search again. We saw the consequence: temporary congestion could encourage enormous detours around terrain. A unit was willing to solve a local traffic problem by trying to go around the distant end of a wall.

We bounded traffic reroutes relative to the terrain route and the corridor already being followed. A briefly occupied entrance should not justify an expedition across the map.

Persistent jams needed more coordination. We now derive a temporary dependency graph: who is actually blocking whom? If stalled friendly movers form a cycle, the system can look for a nearby, physically reachable pocket where one can step aside, briefly wait, and then resume its original order.

![A schematic narrow passage: ant A can step into a side pocket, allowing B and C to pass through the doorway. A retains its original destination.](/media/devlog/doorway.svg)

*Illustration of the recovery idea, not a replay capture. Every actual escape still has to pass terrain, body-clearance, and reservation checks.*

The word “reachable” does a lot of work here. A stable priority rule might nominate the ant inside the opening to yield. But perhaps the ant outside has space, while the one inside has another body behind it. We had to check the proposed escape, not merely pick a winner by ID and hope geometry cooperated.

Even turning matters to that graph. An ant that is still rotating toward its route cannot attempt that movement step yet. Treating its prospective blocker as an immediate waiting dependency could create a false cycle. The dependency calculation now respects the same facing requirement as movement.

The recovery maneuver also remains subordinate to the player. A replacement order cancels it immediately. We do not want a carefully implemented traffic routine to become another reason an ant ignores you.

## Sometimes the destination is the problem

Several frustrating cases happened after the difficult part was already over.

Almost the entire army had crossed a passage. The remaining unit was close to home. Yet its route was trying to reconnect through an intermediate point now occupied by an ally who had finished moving.

The local repair was trying to reach the blocked waypoint itself. There was space around it, but the choice of endpoint rejected that space before the path search had a chance.

We changed how those local routes reconnect: skip occupied intermediate endpoints and look for a checked connection farther along the corridor. A related mixed-army case needed a small fallback beyond the original set of nearby candidates. The slower bombardier was surrounded by its parked army, and all nine original candidates were occupied, although another nearby connection was available.

This is why I care about testing mixed armies. An army of identical warriors tells you something useful. It does not tell you what happens when hunters, a Marshal, archers, and slower bombardiers all arrive at different times.

After the corridor-reconnection pass, all 96 units finished their orders in each of eight long-running passage fixtures. That removed the demonstrated permanent stalls in those cases. The 300-second test horizon also makes the limitation obvious: finishing eventually and crossing comfortably during a real battle are different acceptance criteria.

## Performance bought us room to investigate

Traffic problems were also expensive.

Repeated searches and repeated checks against every unit were doing a lot of work precisely when the army was least able to move. We added a temporary spatial index for the movement pass, updated as units moved or were released. Collision queries could inspect nearby buckets while retaining their exact collision tests.

Combined with bounded traffic searches, one recorded one-cell, 24-per-side fixture dropped from about **40.1 seconds of CPU time to 0.77 seconds**. Its crossing counts were unchanged.

That last sentence is essential. The experiment became dramatically cheaper to run; the ants did not magically clear the passage faster. It was a headless benchmark, not a claim that the game gained fifty times the frame rate.

Still, making the simulation cheaper bought us something very practical: faster iteration, better diagnostics, and more room in the frame budget. We could investigate behavior without every crowded test becoming a prolonged wait.

For the indexing change, we also compared checksums against the full-scan implementation. An optimization that quietly changes which bodies a query finds can become a multiplayer bug, even if it looks fine in one local match.

## Give the sword a real moment of impact

Movement and combat meet at the edge of weapon range.

Our starting attack implementation could apply damage when the weapon cooldown was ready, then let the renderer infer an attack from the changed cooldown. The result was backwards: health changed, and the sword's visible swing followed.

There was another nasty interaction. An attacker could stop because its victim was in range, then lose the hit when the victim moved slightly before the later range check. Without a committed attack phase, tiny movements made combat look indecisive.

We gave attacks an explicit timeline: **windup, contact or release, and recovery**. The simulation owns the timestamps. The renderer samples the corresponding animation phase using the model's authored contact point.

![Attack timeline showing a cancellable windup, authoritative contact or projectile release, recovery, and a separate later projectile impact.](/media/devlog/attack-timeline.svg)

*Conceptual timing, not a particular unit's frame count. For melee, contact is the damage check. For an arrow, release launches a projectile whose impact is resolved separately.*

A valid melee swing starts at normal weapon range. Once committed, it receives a bounded allowance for small target movement. A genuine escape can still defeat the hit. So can the relevant interruptions. The goal is readable commitment, with meaningful opportunities to respond.

The point of release also matters for micro. Move before release and the unreleased attack is canceled; the cooldown does not reset as a reward. Move after release and the unit can cancel its recovery movement restriction without undoing the attack it already delivered.

Arrows now have authoritative launch and impact state. They can outlive the archer who fired them. Their visible origin comes from the animated bow-hand socket at release, captured once so the arrow does not remain attached to a moving hand. Bombardier shells keep their committed ground target, leaving their area attack dodgeable.

The animation tells the player what is happening. It does not decide whether damage exists.

## One fight needs one clock

We also had to stop letting animation run ahead of the world.

If rendering keeps producing frames while multiplayer simulation waits, a wall-clock-driven animation can keep swinging or running even though the authoritative unit has stopped advancing. When the next update arrives, the two versions of the fight disagree visibly.

Combat presentation now follows a bounded simulation clock. During a stall it reaches the allowed fractional point and holds. Movement animation, attack poses, arrows, shells, and spell effects share that time basis. Transform smoothing can still settle onto the last observed pose, but it cannot invent continued progress through the fight.

We built a specific lab control that stalls simulation while leaving the renderer alive. Watching an arrow and bow pose hold together was a much more useful check than pausing the entire application and assuming that proved synchronization.

Our tests also exercise the actual ant animation assets. A timing rule that passes against a dummy object can still look wrong on the model the player sees.

## Respond immediately, then respect the unit

I wanted immediate response to commands. I also wanted units to turn, with weight and direction.

These requirements can coexist. The new intention should replace the old one promptly. The ant should begin reacting. It still has to physically face the direction before moving or beginning a directional attack or cast.

Our initial turn rate is 720 degrees per second: a full reversal takes a quarter of a second. That is our own tuning value.

On the input side, right-click orders submit on pointer-down. Local commands enter the next 25 ms simulation tick without an extra empty tick. The ground marker acknowledges the click immediately, while authoritative execution follows the simulation.

Multiplayer exposed a more surprising delay. During a stalled client frame, the old confirmation logic could promise that many future ticks contained no input. A later click then had to wait behind those promises. In a reproduced case, that meant roughly five seconds of unnecessary delay.

We bounded confirmations against displayed simulation progress and adjusted the shared starting input buffer using server-measured connection latency. That removes avoidable waiting; it cannot remove network latency. Lockstep deserves its own post, because the difficult part is preserving responsiveness while every peer agrees on what actually happened.

## Build a laboratory, then try to break your favorite fix

We ended up building a Combat Lab alongside the game. It uses the real simulation and renderer, with disposable duels, moving targets, mixed armies, tight passages, pause, slow motion, and single-tick stepping.

The automated side became equally important. We rotate bottlenecks, vary army sizes and compositions, reverse orders, save and restore halfway through movement, and compare replay checksums. We measure arrivals and crossings separately. We check actual body clearance and distance traveled, not just whether an order eventually disappears.

One measurement was particularly humbling: an ant traveled **315 cells for a roughly 40-cell displacement**. There was only one coordinated-yield episode in that run. That pointed us toward ordinary route churn instead of assuming repeated recovery maneuvers explained everything.

Several fixes looked excellent in the smaller mixed-army cases. One cut accumulated travel by about 41%. But the larger warrior matrix exposed regressions, including 23 fewer arrivals in one crossing at the same checkpoint. We removed it.

A later experiment removed repeated sidestep anchors and improved narrow-gap recognition. Again, the smaller cases looked better. Again, the long runs left units unfinished where the accepted version completed them. We removed that too.

I think these failed experiments are worth talking about. “This looks much better” is how a useful investigation starts. It still needs to survive the situations you were not watching when you formed that opinion.

## Where we are now

The engine is substantially better. Short army moves waste less motion. Previously trapped units can reconnect to their routes. Targets get approached from useful positions. Attack, release, and impact have explicit meaning. Units turn visibly, orders interrupt promptly, and the presentation follows the same fight as the simulation.

At this checkpoint, the complete suite passes **647 tests across 155 files**, and the production build passes. Those cover much more than movement alone, so I would not present the count as a certificate of perfect pathing.

Dense opposing traffic still has room to improve. Some crossings are slow, and route churn remains an active investigation. The game also needs the kind of testing that comes from playing real matches, making ugly decisions, and trying to save an ant that really should be dead.

But the foundation has changed in a way you can feel.

That is the standard I want to keep: when I lose a fight, I want to be thinking about the flank I missed, the ability I used too early, or the worker I should have protected. I want the controls to be reliable enough that the interesting mistakes are mine.

And yes, all of this effort is so a tiny ant with a sword goes where you told it to go. I think that is a pretty good use of our time.

---

### Notes for the technically curious

This post describes the development checkpoint of September 10, 2026. Measurements are specific fixtures, not universal speedups or internet-latency guarantees. The diagrams are original explanatory illustrations. Network changes describe the updated client/server implementation, not a measured production deployment.

The [combat and movement contract](/development/declarations/combat-feel) describes current implementation boundaries. Our [controls guide](/guide/controls) describes the current player-facing commands.

The reproducible repository entry points are `scripts/bench/army-movement.ts`, `scripts/bench/combat-traffic.ts`, `scripts/bench/mixed-movement.ts`, and `scripts/bench/combat-frontline.ts`. The interactive lab is `combat-lab.html` on the game development server. The wiki does not run the game engine.
