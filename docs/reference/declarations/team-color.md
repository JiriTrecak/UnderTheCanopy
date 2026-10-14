# Team-color material contract

The only recognized material name is **`TC_TeamColor`**, exact and case-sensitive. Name the Blender material this, not the object or mesh. Preserve it unchanged through optimization and GLB export. No suffixes, numbered variants or aliases are accepted.

The runtime replaces this material's Base Color with the owner's player color. All other materials retain their authored colors, including roofs and insect chitin. Materials are cloned per instance; ownership changes recolor the tagged material, and unowned entities restore its authored Base Color.

Author team surfaces with a separate Principled BSDF material. Keep emblems, trim and metal on separate ordinary materials. Use white or neutral grayscale vertex colors/textures on team surfaces; the runtime does not remove baked faction hue from them. Avoid colored emission on those surfaces. Preserve the material assignment when merging geometry. A single mesh with multiple material slots is supported.

For the generation skill:

> All recolorable surfaces must use the shared material `TC_TeamColor`. No other material is recolored. Keep team hue in the material Base Color, never baked into vertex colors, textures or emission. Vertex/texture shading must be neutral grayscale. Keep emblems and trim separate. Preserve the exact material name on GLB export without suffixes. Verify with both blue and red player colors.

Implementation: `src/render/settlement/playerMaterials.ts`. Rootbound Hall's current baked banner does not meet this contract; its source/export must remove the baked red and apply this material name. Its roofs should retain their authored materials.
