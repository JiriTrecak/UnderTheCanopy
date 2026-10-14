# Hero targeting and revival

## Ability targeting

Point-targeted commands display a terrain-following impact footprint before commitment. Faultline uses the simulation's flat-ended line with width `2 * rank.radius`; Crownfall uses a circle centered at the target with `rank.radius`. A dashed circle around the caster displays `rank.range`. Aim uses the same rounded destination as the submitted command. Out-of-range or unexplored targets are red and clicking them retains targeting mode. Escape, right-click, selection changes and a valid commitment clear the preview. The pointer target is recomputed while aiming so moving the camera or caster does not leave a stale footprint.

`AbilityTarget` is presentation only. Ranks and shapes derive from spell definitions, with no gameplay decision made by the renderer. Current spells have line/radial footprints; no cone spell exists yet. Self-cast Rally/Carapace remain immediate commands.

## Fallen heroes

A hero's existing entity identity is retained with `hp: 0` and `fallen: true` after authoritative death resolution. The entity is excluded from live gameplay, visibility sensors, collisions and battlefield selection. XP, learned skills, cooldown timestamps and equipment remain attached. Work/movement/transient effects are cleared. Ground drops still come from neutral camp rewards or deliberate item drops; hero death no longer spills inventory.

The owner's observation includes a separate `fallenHeroes` roster. Enemy players cannot read it. The simulation snapshot retains this state and validates revival references and duplicate queues.

## Amber Sanctuary

Workers can construct `building.ants.sanctuary` when its build action is available. Cost and command category come from the content registry. Its declarative `revival` behavior defines a four-entry queue and 400 ticks (10 seconds at 40 Hz) of work. Revival itself is currently free. Commands are `revive {actor, hero}` and `cancelRevival {actor, hero}`; a hero cannot be queued at two buildings. The selected building exposes a command per fallen hero and a cancelable percentage queue.

Completion restores the same hero at an available location near the entrance, with full derived HP and mana, preserved XP/skills/items, and unchanged absolute ability cooldowns. A blocked exit delays completion. Destroying the building discards its queue but leaves the fallen hero available elsewhere. Additional hero purchases are not implemented; revival does not manufacture a replacement identity.

## Validation

Check authoritative hero death, retained equipment/skills/XP, save/load during revival, duplicate queue rejection, ownership, cancellation, building loss and successful restoration. Model availability and live visual behavior must be checked separately from simulation state.
