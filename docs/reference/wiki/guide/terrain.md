# Terrain and high ground

Height is part of the battlefield. A cliff can shelter a base, a ramp can funnel an army, and a scout on a ridge can reveal targets for ranged units below.

## Moving and building

Ground units can cross gentle slopes and authored ramps. Steep cliff faces block movement, including diagonal shortcuts around corners. Orders still use the same continuous movement and pathfinding system: clicking across a cliff sends the unit toward a reachable ramp.

Buildings need dry, level foundations. The maximum height difference across a footprint is **0.5 metres**. Bridge decks remain movement surfaces; buildings cannot be placed on them. Workers must reach the appropriate side of a tree, mine or building to work on it.

## Seeing uphill

Every observer can see ground up to **2 metres above its own ground level**, within its normal vision radius. This is an engine rule; a taller model does not see farther uphill.

- Terrain higher than that offset stays hidden from an observer below.
- Climbing a ramp gradually raises the observer's viewpoint and reveals the upper ground.
- Lower ground remains visible throughout the vision radius, including past small intervening hills. Solid bridge floors still conceal the space beneath them.
- Allied players share vision. A scout on the plateau can reveal enemies to an allied army below.
- Leaving the area preserves explored terrain, but enemy units disappear from current view. Previously seen static structures retain their last known appearance.

Debug reveal changes the picture, not the information available to the simulation or AI.

## Fighting across heights

Melee attacks cannot reach across a cliff edge. A visible enemy within horizontal range may still require an approach through a ramp.

Archers can attack uphill or downhill if a friendly observer reveals the target and terrain clears the shot. Another ridge can intercept the firing line. Ranged units look for an accessible firing position when their current position is obstructed. A released arrow does not deal damage through intervening terrain if the target moves behind it.

There is no elevation damage bonus or random uphill miss chance. Existing spell area effects keep their declared behaviour; this first pass does not add a separate terrain interaction to every spell.

## Authoring terrain

Open **Terrain** in the world editor. Set an absolute grid level, or paint shallow water, deep water or dry ground using a rectangle or a polygon. Polygon points snap to corners and edge midpoints. Shallow water has a visible submerged bed and remains traversable; deep water blocks ordinary ground units.

Optional sculpting changes the same terrain data. A ramp samples its endpoint heights; extend it if its grade is too steep. Buildings need dry, level foundations even beside a shallow crossing.

River, mountain and forest generators are editor tools. Preview, adjust, then **Apply previews** to commit terrain and placed objects. Save contains only this editable result. There is no generator to rerun at map load and no separate publishing bake.

The editor MCP exposes these same operations through `editor_scene`. Verify crossings, cliffs and ramps with unit movement after shaping the terrain; a visual preview alone does not prove traversal.
