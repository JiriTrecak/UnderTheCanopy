"""The neutral map features from Tripo, made game-ready in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/neutral_features.py -- \\
        <name> <tripo.glb>

Breakable crates and rubble, the two springs, the merchant's cart and the loot bundle every
hero item lies on the ground as (D86). Each is Tripo's image-to-3D model of an approved
gpt-image concept (`art/styles/neutral-<name>.png`); this script, after
`tools/props/substance_deposit.py`:

- joins it into one mesh, centres its footprint, puts its lowest point on the ground and
  scales it to fit its footprint (rubble turns its long side along x, the footprint's width);
- decimates it to a budget and shrinks its textures;
- turns the texels that are raw magic into a static emission texture: golden sap and amber
  (`gold`), the violet of dew and substance (`violet`). These glow always, as the Substance
  Deposit does: nothing on them waits for work;
- adds `socket_pool` (springs) where their always-on light and motes rise;
- writes geometry.glb, source.blend and asset.json into base/assets/props/neutral-<name>/.

Neutral: no team colour, no dirt skirt (the concepts stand clean on the floor)."""
import json, os, sys

import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
NAME, SOURCE = sys.argv[sys.argv.index('--') + 1:][:2]

# name: title, footprint (cells = metres), size across (fit inside the footprint), triangle
# budget, texture size, glow (kind, strength), pool socket height (share of the height).
SPECS = {
    # Crates stand a little over their 4-cell footprint, about a soldier's size, as Warcraft
    # III's crates do; springs and the merchant fill 12 cells, as its fountains and shops.
    'amber-cache': ('Amber Cache', (4, 4), 4.4, 3000, 1024, ('gold', 2.2), None),
    'supply-crate': ('Beetle Supply Crate', (4, 4), 4.4, 3000, 1024, ('violet', 2.4), None),
    'seed-pod': ('Fallen Seed Pod', (4, 4), 4.8, 3000, 1024, ('gold', 1.4), None),
    'rubble': ('Collapsed Rubble', (12, 4), None, 9000, 1024, None, None),
    'sap-spring': ('Sap Spring', (12, 12), 11.4, 6000, 1024, ('gold', 2.2), 0.62),
    'dew-spring': ('Dew Spring', (12, 12), 10.6, 6000, 1024, ('violet', 1.6), 0.42),
    'merchant': ('Pill-Bug Peddler', (12, 12), 11.6, 9000, 1024, ('gold', 1.2), None),
    'loot-bundle': ('Loot Bundle', None, 2.2, 1200, 512, ('gold', 2.0), None),
}
TITLE, FOOTPRINT, ACROSS, TRIANGLES, TEXTURE, GLOW, POOL = SPECS[NAME]
OUT = os.path.join(ROOT, 'base', 'assets', 'props', 'neutral-' + NAME)
VIOLET = np.array((0.42, 0.34, 1.0), np.float32)  # blue-violet: the bowl must not read pink

kit.reset()
bpy.ops.import_scene.gltf(filepath=SOURCE)
meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH']
for o in meshes:
    m = o.matrix_world.copy()
    o.parent = None
    o.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
bpy.ops.object.select_all(action='DESELECT')
for o in meshes:
    o.select_set(True)
bpy.context.view_layer.objects.active = meshes[0]
if len(meshes) > 1:
    bpy.ops.object.join()
obj = bpy.context.view_layer.objects.active
obj.name = obj.data.name = 'neutral-' + NAME
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# Normalise: the long side along x for a long footprint, the footprint centred, the lowest
# point on the ground, scaled to fit.
co = np.array([v.co[:] for v in obj.data.vertices])
lo, hi = co.min(axis=0), co.max(axis=0)
if FOOTPRINT and FOOTPRINT[0] != FOOTPRINT[1] and (hi[1] - lo[1]) > (hi[0] - lo[0]):
    obj.data.transform(mathutils.Matrix.Rotation(np.pi / 2, 4, 'Z'))
    co = np.array([v.co[:] for v in obj.data.vertices])
    lo, hi = co.min(axis=0), co.max(axis=0)
if ACROSS is None:
    # Rubble fills its footprint's depth; a pile shorter than the footprint's length is laid
    # twice, the second turned half round, overlapping in the middle into one long wall.
    scale = min(FOOTPRINT[0] / (hi[0] - lo[0]), (FOOTPRINT[1] + 0.4) / (hi[1] - lo[1]))
else:
    scale = ACROSS / max(hi[0] - lo[0], hi[1] - lo[1])
obj.data.transform(mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation(
    (-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -lo[2])))
size = (hi - lo) * scale
if ACROSS is None and size[0] < FOOTPRINT[0] * 0.8:
    shift = (FOOTPRINT[0] - size[0]) / 2
    twin = obj.copy()
    twin.data = obj.data.copy()
    bpy.context.collection.objects.link(twin)
    twin.data.transform(mathutils.Matrix.Translation((shift, 0, 0)) @ mathutils.Matrix.Rotation(np.pi, 4, 'Z'))
    obj.data.transform(mathutils.Matrix.Translation((-shift, 0, 0)))
    bpy.ops.object.select_all(action='DESELECT')
    twin.select_set(True)
    obj.select_set(True)
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.join()
    size[0] = FOOTPRINT[0]

tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
if tris > TRIANGLES:
    mod = obj.modifiers.new('decimate', 'DECIMATE')
    mod.ratio = TRIANGLES / tris
    mod.use_collapse_triangulate = True
    bpy.ops.object.modifier_apply(modifier=mod.name)

assert len(obj.data.materials) == 1, 'expected Tripo\'s single material'
mat = obj.data.materials[0]
mat.name = 'neutral-' + NAME
nt = mat.node_tree
bsdf = next(n for n in nt.nodes if n.type == 'BSDF_PRINCIPLED')
base_node = next(n for n in nt.nodes if n.type == 'TEX_IMAGE'
                 and any(l.to_socket.name == 'Base Color' for l in n.outputs['Color'].links))
emissive = None
if GLOW:
    kind, strength = GLOW
    base = base_node.image
    w, h = base.size
    px = np.array(base.pixels[:], np.float32).reshape(h, w, 4)[:, :, :3]
    r, g, b = px[:, :, 0], px[:, :, 1], px[:, :, 2]
    if kind == 'violet':
        # Blue clearly over red and green: dew, substance glass.
        weight = np.clip((b - np.maximum(r, g) - 0.05) / 0.15, 0.0, 1.0)[..., None]
        glow = weight * (0.2 * px + 0.8 * VIOLET * px[:, :, 2:3]) * (0.3 + 0.7 * px[:, :, 2:3])
    else:
        # Warm and bright: red high, green over blue by a margin (amber, golden sap). Brown
        # wood and bark are too dark; cream and grey are too unsaturated.
        warm = np.clip((r - 0.55) / 0.25, 0.0, 1.0) * np.clip((g - b - 0.18) / 0.2, 0.0, 1.0)
        weight = warm[..., None]
        glow = weight * px * (0.4 + 0.6 * r[..., None])
    print('glow texels %.1f%%' % (100.0 * float((weight > 0.5).mean())))
    emissive = kit._array_image(obj.name + '-glow', glow.astype(np.float32))

for n in nt.nodes:
    if n.type == 'TEX_IMAGE' and n.image:
        n.image.name = obj.name + '-' + {'Base Color': 'base'}.get(
            next((l.to_socket.name for l in n.outputs['Color'].links), ''), n.image.name[-1])
for img in [n.image for n in nt.nodes if n.type == 'TEX_IMAGE' and n.image] + ([emissive] if emissive else []):
    if img.size[0] > TEXTURE:
        img.scale(TEXTURE, TEXTURE)
    img.pack()
if emissive:
    tex = nt.nodes.new('ShaderNodeTexImage')
    tex.image = emissive
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
    bsdf.inputs['Emission Strength'].default_value = GLOW[1]

if POOL is not None:
    kit.socket('socket_pool', (0.0, 0.0, float(size[2]) * POOL))

height = round(float(size[2]), 2)
capabilities = {'groundContact': {'mode': 'pivot'}}
if FOOTPRINT:
    capabilities['blocker'] = {'shape': 'box', 'size': list(FOOTPRINT), 'offset': [0, 0]}
    capabilities['vegetationClearance'] = max(FOOTPRINT) / 2
asset = {
    'id': 'asset.models.props.neutral-' + NAME,
    'name': TITLE,
    'kind': 'prop',
    'tags': ['original', 'neutral', 'tripo'],
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': capabilities,
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'license': 'Original artwork for Under the Canopy: concept by gpt-image-2, model by Tripo '
               '(image to 3D), made game-ready by tools/props/neutral_features.py.',
}
os.makedirs(OUT, exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, 'source.blend'))
bpy.ops.export_scene.gltf(
    filepath=os.path.join(OUT, 'geometry.glb'), export_format='GLB', export_yup=True,
    export_apply=True, export_image_format='JPEG', export_jpeg_quality=90, export_extras=True)
with open(os.path.join(OUT, 'asset.json'), 'w') as f:
    json.dump(asset, f, indent=2, ensure_ascii=False)
    f.write('\n')
tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
print('built', NAME, tris, 'triangles, size %.2f x %.2f x %.2f m' % tuple(size))
