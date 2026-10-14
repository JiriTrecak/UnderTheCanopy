# Spatial sectors

Local questions should depend on the nearby world, rather than the total size of the map. The simulation now uses 16×16-cell sectors for sight candidates, harvesting candidates and hierarchical navigation. Existing unit collision and combat target buckets remain in use.

## Visibility and resources

`SectorIndex` stores an entity in every sector intersected by its bounds. A query visits only intersecting sectors, deduplicates candidates and rejects non-overlapping bounds. Sight sensors are indexed by their coverage bounds, allowing a target to find only sensors that could see it. The precise range, ownership, containment and terrain line-of-sight checks still decide visibility. Large building footprints and fractional movement have a conservative border so the broad phase cannot discard a valid sight test.

The sight index synchronizes once per changed simulation phase. This maintenance still scales with live sensors; queries no longer multiply every target by every sensor. It is not a claim that all observation work is constant-time. The incremental sight masks and fog memory described in [combat feedback and profiling](/development/expansion/combat-feedback-performance) remain responsible for explored cells and last-seen information.

Harvesting finds resources within the existing 32-cell search radius through resource sectors. Creation, depletion, regrowth, removal and restore update the index. A specifically targeted capacity-limited mine remains that mine. Candidate distance and entity-ID ordering are unchanged.

These indexes contain authoritative simulation data. Player-facing results still pass observation rules; AI does not gain knowledge of hidden changes.

## Navigation

Each sector contains separate connected regions. A river dividing one square therefore creates two regions, rather than an imaginary crossing. Legal neighboring cell edges establish connections between sectors. Height limits, diagonal corner rules and authored bridge entrances participate in those connections.

For destinations at least 32 cells away on either horizontal axis:

1. Search the small region graph for a sector route.
2. Include one neighboring sector around that route for approaches and local traffic.
3. Run the existing detailed A* inside that corridor.
4. If temporary occupants block the corridor, retry the unrestricted detailed search.

Short routes retain the ordinary local search. Ground and elevated surfaces use the same sector hierarchy, but maintain distinct graph nodes and explicit legal transitions. An arch does not connect to the floor below merely because both occupy the same horizontal sector.

Tree and building blocker changes rebuild the affected sector regions and adjacent boundary links. Connectivity is recomputed on the much smaller region graph. Up to 128 coarse plans are cached and invalidated when topology changes. Loading still constructs the initial indexes for the full map.

All decisions use stable ordering and integer path costs. No command queue delay or time-sliced response was introduced. The corridor prioritizes a plausible coarse route; it does **not** guarantee the globally shortest fine-grid path. The unrestricted fallback preserves reachability when the preferred corridor is blocked, but can still be expensive.

## Reproduce and inspect

```sh
node --import tsx scripts/bench/sector-scaling.ts --output /tmp/sector-scaling.json
npm run bench:sim -- --map echo-isles --ticks 12000 --output /tmp/sector-match.json
```

Debug profiling reports cumulative navigation searches, fine cells expanded, coarse regions expanded and corridor fallbacks.

Regression coverage includes sector edges and corners, disconnected pieces within a sector, local topology changes, dynamic corridor fallback, spanning footprints, sensor movement and containment, resource removal/restore, and distant routes onto elevated surfaces.
