"""The Substance deposit from Tripo, made game-ready in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/substance_deposit.py -- \\
        <substance-deposit.glb>

The deposit is the neutral resource every race builds its Substance Well on (docs/beetles.md):
a knot of ancient gnarled roots split open by violet cracks and crystal shards, Tripo's
image-to-3D model of the approved concept (art/styles/substance-deposit-concept.png). It holds
an 8 x 8-cell footprint, so its base stays inside 8 metres and the 12-metre Well covers it.

- Raw substance is the forest's own magic and ALWAYS glows (the one exception to D65, as on the
  Well's knot): the violet texels of Tripo's texture (blue over red and green) become a static
  emission texture, saturated towards the art direction's violet, so the deposit reads from
  across the map. The dark roots stay unlit. No `activeGlow`: nothing here waits for work.
- One material, neutral (no team colour), no dirt skirt (Tripo made none: the roots meet the
  ground themselves), the pivot at the centre of the footprint on the ground.
- Under 10,000 triangles; textures shrunk to 1024.
- `socket_motes` at the top of the knot, for an ambient effect once the engine can attach
  always-on effects to a neutral resource.

Writes geometry.glb, source.blend and asset.json into base/assets/resources/substance-deposit/."""
import os, sys

import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOURCE = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'substance-deposit'
OUT = os.path.join(ROOT, 'base', 'assets', 'resources', NAME)
WIDTH = 7.8  # widest side in metres: inside the 8-cell footprint, under the 12-metre Well
TRIANGLES = 9800
TEXTURE = 1024
VIOLET = np.array((0.55, 0.32, 1.0), np.float32)
GLOW = 3.5  # emission strength of the cracks and crystals

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
obj.name = obj.data.name = NAME
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# Normalise: footprint centred, lowest point on the ground, WIDTH across the widest side.
co = np.array([v.co[:] for v in obj.data.vertices])
lo, hi = co.min(axis=0), co.max(axis=0)
scale = WIDTH / max(hi[0] - lo[0], hi[1] - lo[1])
obj.data.transform(mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation(
    (-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -lo[2])))
size = (hi - lo) * scale

tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
if tris > TRIANGLES:
    mod = obj.modifiers.new('decimate', 'DECIMATE')
    mod.ratio = TRIANGLES / tris
    mod.use_collapse_triangulate = True
    bpy.ops.object.modifier_apply(modifier=mod.name)

# One material: Tripo's base colour, roughness/metal and normal maps, shrunk, plus the glow.
assert len(obj.data.materials) == 1, 'expected Tripo\'s single material'
mat = obj.data.materials[0]
mat.name = NAME
nt = mat.node_tree
bsdf = next(n for n in nt.nodes if n.type == 'BSDF_PRINCIPLED')
base_node = next(n for n in nt.nodes if n.type == 'TEX_IMAGE'
                 and any(l.to_socket.name == 'Base Color' for l in n.outputs['Color'].links))
base = base_node.image
w, h = base.size
px = np.array(base.pixels[:], np.float32).reshape(h, w, 4)[:, :, :3]

# Violet texels: blue clearly over red and green (lilac highlights included); the brown roots
# and the black gaps between the islands have none.
blueness = px[:, :, 2] - np.maximum(px[:, :, 0], px[:, :, 1])
weight = np.clip((blueness - 0.05) / 0.15, 0.0, 1.0)[..., None]
# Mostly pure violet (bright lilac washes out), and brighter where the texture is brighter
# (veins and crystal faces over the deep cracks), so the glow keeps the texture's shapes.
b = px[:, :, 2:3]
glow = weight * (0.2 * px + 0.8 * VIOLET * b) * (0.3 + 0.7 * b)
print('glow texels %.1f%%' % (100.0 * float((weight > 0.5).mean())))
emissive = kit._array_image(NAME + '-glow', glow.astype(np.float32))

for n in nt.nodes:
    if n.type == 'TEX_IMAGE' and n.image:
        n.image.name = NAME + '-' + {'Base Color': 'base'}.get(
            next((l.to_socket.name for l in n.outputs['Color'].links), ''), n.image.name[-1])
for img in [n.image for n in nt.nodes if n.type == 'TEX_IMAGE' and n.image] + [emissive]:
    if img.size[0] > TEXTURE:
        img.scale(TEXTURE, TEXTURE)
    img.pack()
tex = nt.nodes.new('ShaderNodeTexImage')
tex.image = emissive
nt.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = GLOW

kit.socket('socket_motes', (0.0, 0.0, float(size[2]) * 0.8))

tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
height = round(float(size[2]), 2)
asset = {
    'id': 'asset.models.resources.' + NAME,
    'name': 'Substance Deposit',
    'kind': 'prop',
    'tags': ['original', 'neutral', 'woodland', 'resource', 'substance', 'tripo'],
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'blocker': {'shape': 'box', 'size': [8, 8], 'offset': [0, 0]},
        'vegetationClearance': 4,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'license': 'Original artwork for Under the Canopy: concept by gpt-image-2, model by Tripo '
               '(image to 3D), made game-ready by tools/props/substance_deposit.py.',
}
os.makedirs(OUT, exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, 'source.blend'))
bpy.ops.export_scene.gltf(
    filepath=os.path.join(OUT, 'geometry.glb'), export_format='GLB', export_yup=True,
    export_apply=True, export_image_format='JPEG', export_jpeg_quality=90, export_extras=True)
with open(os.path.join(OUT, 'asset.json'), 'w') as f:
    import json
    json.dump(asset, f, indent=2, ensure_ascii=False)
    f.write('\n')
print('built', NAME, tris, 'triangles, size %.2f x %.2f x %.2f m' % tuple(size))
