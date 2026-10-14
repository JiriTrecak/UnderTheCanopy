# Art direction

Under the Canopy is a readable stylized RTS about forest-floor warfare: chunky upright ants, acorn shields, heavy wooden weapons, stick construction and structural leaf roofs. Use the [approved settlement concept](../../art/styles/forest-settlement.png) for the faction's visual direction; current unit/building coverage still includes deliberate missing-model placeholders.

The environment uses original woodland assets: simple layered pine silhouettes, broad painted color areas, subdued green foliage, warm earth, natural pebble trails and stick/root bridges. Avoid high-frequency needle detail, washed-out trees and oversaturated greens. Autumn uses dry grasses and warm leaves; frozen woodland uses its own biome materials.

Ownership must read at RTS zoom. Put team color on substantial armor and architectural surfaces, using the [team-color material contract](../declarations/team-color.md). Keep decorative litter smaller and visually quieter than harvestable trees and interactive resources. Large trunks and canopy shade establish insect scale without hiding units.

Geometry, materials, light and ground-cover density work together. Review assets in the actual renderer on a representative map, at gameplay distance as well as close up. Changes to a texture should not silently change tree geometry or branch arrangement.

Retain approved source art and editable models in `art/`; put temporary renders and comparisons in ignored `tmp/`. See [publication](../asset-pipeline/publication.md) for the asset contract.

## Current ant roster

The supplied roster updates existing gameplay definitions: Acorn Main Hall / Great Acorn Hall, Mound (worker house), Barracks, Rootworks, Chitin Works (upgrade building), Bombardier Workshop and Watchtower. The watchtower accepts one archer. The worker, archer and bombardier use rigged Tripo/Blender assets; the previously approved warrior remains in use. The neutral amber deposit and corrupted-root deposit are published models. Threewater includes two accessible Root sites for tier-two progression.

All published buildings stay below 10,000 triangles. Complete unit budgets are warrior 4,905, worker 4,172 including its largest cargo, archer 4,715 including the bow/arrow, and bombardier 4,957 including the mortar. Team ownership lives in opaque texture masks; 512px is the initial unit texture target; the warrior has been converted and visually checked, while other models retain their current exports pending review. Editable masters retain their original resolution. Each package in `art/assets/<id>/` keeps one master (`source.blend` for models) next to its published outputs. Global unit scale remains a gameplay setting, not baked model size.

Running clips retain 20% of their extra forward torso lean relative to the relaxed pose, including the worker's carrying run. `experiments/building-studio/locomotion.py` bakes this correction without changing leg tracks or clip timing; per-source `locomotion.json` records the measured result. Gameplay samples attack poses while advancing crossfades so contact stays simulation-aligned without snapping between clips.

Arrows and mortar shells launch from animated sockets, with release aligned to simulation impact ticks. Worker cargo attaches to the backpack socket. The tower's raised roof and firing-side balcony offset accommodate the archer at the current 1.7× unit scale.

Explicitly deferred faction models: Marshal (hero), Forester, Sanctuary and Spear Hunter. Unreferenced legacy campaign/neutral creature bindings are outside this supplied-roster pass and may still use diagnostic placeholders; they are not evidence of completed creature artwork. Campaign villagers reuse the worker model. Source silhouettes and textures are reference-based, with unseen surfaces and movement inferred and adapted; no new facial speech rig is claimed.
