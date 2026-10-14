# Building the world beneath the canopy

The shipped skirmish maps are Echo Isles and Turtle Rock. Ground textures, layered pines, ground cover, water-support textures and landmark models form the woodland kit. The asset editor and map editor share the runtime renderer.

Start with the [map-authoring guide](/development/editor), [art direction](/development/game/art), and [publication contract](/development/asset-pipeline/publication). Live procedural layers let authors paint broad forest and meadow masks and draw river courses. Biomes select compatible recipes and materials; terrain and water are generated before dependent foliage.

Large trees and roots establish scale. The renderer's unit-occlusion mask opens a dithered window where already-observed units would be hidden by large scenery; it does not reveal unknown enemies or alter collision. Decorative clutter stays subordinate to resources and army silhouettes.

Biomes own the visual profiles for [weather](/development/expansion/weather) and [volumetric atmosphere](/development/expansion/volumetric-atmosphere). Maps select time and optional weather kind; they cannot override the look. Overhead canopy shadows and cloud motion support the forest-floor lighting. Use a fixed view and lighting phase to compare changes; keep temporary captures outside the tracked repository.
