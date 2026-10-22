"""The beetles' station gate from Tripo, made game-ready in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/beetle_gate.py -- <gate.glb>

The gate is Tripo's image-to-3D model of the concept art/styles/beetle-gate-idle.png: two
masonry gate towers with brass domes and violet lamps, and between them a closed double gate
of riveted steel with royal-blue panels. It is cut in two along the towers' inner faces:
- the **leaves** are the building (`building.neutral.beetle-gate`, 16 × 4 cells): they fill
  the footprint exactly, take the team colour on their blue panels, and stand still (a gate
  has no work). Broken, they are gone, and the road is open;
- the **towers** are scenery (`scenery.beetle-gate-towers`), placed where the gate stands
  with the same turn: they stay when the leaves fall, so the broken gate is an open gateway.
  As neutral decoration their blue is turned to the station's slate steel, and their violet
  lamps burn like the station's lanterns.

Tripo's gate is tall for its width; the leaves are lowered to 6 m (the station's walls are
4.2 m) and the towers scaled down whole, so they keep their shape. Blender is Z-up and the
gate faces -Y (glTF +Z, towards the road). Writes base/assets/buildings/beetle-gate/ and
base/assets/environment/underleaf/beetle-gate-towers/ (geometry.glb, source.blend,
asset.json)."""
import os, sys

import bpy, bmesh
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
GLB = sys.argv[sys.argv.index('--') + 1:][0]
GATE = 'beetle-gate'
TOWERS = 'beetle-gate-towers'
OUT_GATE = os.path.join(ROOT, 'base', 'assets', 'buildings', GATE)
OUT_TOWERS = os.path.join(ROOT, 'base', 'assets', 'environment', 'underleaf', TOWERS)
LEAVES = 16.0  # the leaves fill the 16-cell footprint
DEPTH = 4.0  # ...and its 4 cells of depth
LOW = 0.6  # the leaves' height, as a share of Tripo's
TOWER = 0.62  # the towers' scale, as a share of Tripo's
LICENSE = ('Generated with Tripo (image to 3D) from an ImageGen (gpt-image-2) concept for Under the '
           'Canopy; made game-ready by tools/buildings/beetle_gate.py.')


def load():
    kit.reset()
    bpy.ops.import_scene.gltf(filepath=GLB)
    meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH']
    obj = meshes[0]
    for o in meshes:
        m = o.matrix_world.copy()
        o.parent = None
        o.data.transform(m)
        o.matrix_world = mathutils.Matrix.Identity(4)
    for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
        bpy.data.objects.remove(o, do_unlink=True)
    if len(meshes) > 1:
        bpy.ops.object.select_all(action='DESELECT')
        for o in meshes:
            o.select_set(True)
        bpy.context.view_layer.objects.active = obj
        bpy.ops.object.join()
    # Centre the footprint, ground at 0.
    co = np.array([v.co[:] for v in obj.data.vertices])
    lo, hi = co.min(0), co.max(0)
    obj.data.transform(mathutils.Matrix.Translation((-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -lo[2])))
    # The towers' inner faces: the deep part (the leaves are thin) nearest the middle.
    co = np.array([v.co[:] for v in obj.data.vertices])
    depth = hi[1] - lo[1]
    deep = np.abs(co[:, 1]) > 0.3 * depth
    inner = float(np.abs(co[deep, 0]).min())
    scale = LEAVES / (2 * inner)
    obj.data.transform(mathutils.Matrix.Scale(scale, 4))
    print('inner face', round(inner, 3), 'scale', round(scale, 2))
    return obj, LEAVES / 2


def cut(obj, keep_inside, at):
    """Keeps the part of the mesh inside (|x| < at) or outside the two planes x = ±at."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    for sign in (1, -1):
        geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
        normal = (sign, 0, 0)
        bmesh.ops.bisect_plane(bm, geom=geom, plane_co=(sign * at, 0, 0), plane_no=normal,
                               clear_outer=keep_inside, clear_inner=False)
    if not keep_inside:
        doomed = [f for f in bm.faces if abs(f.calc_center_median().x) < at - 1e-4]
        bmesh.ops.delete(bm, geom=doomed, context='FACES')
    bm.to_mesh(obj.data)
    bm.free()


def texture_node(mat):
    bsdf = mat.node_tree.nodes['Principled BSDF']
    link = bsdf.inputs['Base Color'].links
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


def pixels(img):
    w, h = img.size
    return np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)


def royal_blue(rgb):
    """The beetles' royal blue (blue leads, red no more than green), not the violet glass."""
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    return np.clip((b - np.maximum(r, g) - 0.04) / 0.1, 0.0, 1.0) * (b > 0.08) * (r <= g * 1.3 + 0.02)


def violet(rgb):
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    return np.clip((b - g - 0.25) / 0.2, 0.0, 1.0) * np.clip((r - g - 0.08) / 0.12, 0.0, 1.0)


def finish(obj):
    for p in obj.data.polygons:
        p.use_smooth = True
    for img in bpy.data.images:
        if img.size[0] > 2048:
            img.scale(2048, 2048)


# --- The leaves: the building. ----------------------------------------------------------
obj, half = load()
cut(obj, True, half)
obj.data.transform(mathutils.Matrix.Diagonal((1.0, 1.0, LOW, 1.0)))
co = np.array([v.co[:] for v in obj.data.vertices])
print('leaves', (co.max(0) - co.min(0)).round(2))
obj.name = 'gate-leaves'
mat = obj.data.materials[0]
node = texture_node(mat)
px = pixels(node.image)
mask = royal_blue(px[:, :, :3])
team = bpy.data.images.new('gate-team', node.image.size[0], node.image.size[1], alpha=True)
team.pixels[:] = np.concatenate([px[:, :, :3], mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
mat.name = 'TC_gate'
mat['teamColorMask'] = 'baseColorAlpha'
print('team', round(float(mask.mean()), 3))
finish(obj)
height = float(co[:, 2].max())
kit.export(OUT_GATE, GATE, {
    'id': 'asset.models.buildings.' + GATE, 'name': 'Station Gate', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'team-mask', 'glass-road'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': ['TC_gate']},
        'blocker': {'shape': 'box', 'size': [LEAVES, DEPTH], 'offset': [0, 0]},
        'vegetationClearance': 2,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'shading': 'building',
    'license': LICENSE,
})

# --- The towers: scenery that stays. ------------------------------------------------------
obj, half = load()
cut(obj, False, half)
# Each tower scaled down whole about its own inner foot, so its inner face stays at the leaves.
for v in obj.data.vertices:
    side = 1.0 if v.co.x > 0 else -1.0
    v.co.x = side * (half + (abs(v.co.x) - half) * TOWER)
    v.co.y *= TOWER
    v.co.z *= TOWER
obj.name = 'gate-towers'
mat = obj.data.materials[0]
mat.name = 'gate-towers'
node = texture_node(mat)
px = pixels(node.image)
rgb = px[:, :, :3]
blue = royal_blue(rgb)[..., None]
lum = (rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32))[..., None]
slate = np.clip(lum * 1.15, 0.0, 1.0) * np.array([0.82, 0.88, 1.0], np.float32)
recolored = rgb * (1 - blue) + slate * blue
img = bpy.data.images.new('gate-towers', node.image.size[0], node.image.size[1], alpha=False)
img.pixels[:] = np.concatenate([recolored, np.ones_like(lum)], axis=2).ravel()
img.pack()
node.image = img
glow = violet(rgb)[..., None] * np.array([0.55, 0.32, 1.0], np.float32)
em = bpy.data.images.new('gate-towers-glow', node.image.size[0], node.image.size[1], alpha=False)
em.pixels[:] = np.concatenate([glow, np.ones_like(lum)], axis=2).ravel()
em.pack()
tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = em
uv = node.inputs['Vector'].links[0].from_socket if node.inputs['Vector'].links else None
if uv:
    mat.node_tree.links.new(uv, tex.inputs['Vector'])
bsdf = mat.node_tree.nodes['Principled BSDF']
mat.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 3.0
finish(obj)
co = np.array([v.co[:] for v in obj.data.vertices])
size = co.max(0) - co.min(0)
outer = float(np.abs(co[:, 0]).max())
depth = float(co[:, 1].max() - co[:, 1].min())
print('towers', size.round(2), 'outer', round(outer, 2))
os.makedirs(OUT_TOWERS, exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT_TOWERS, 'source.blend'))
bpy.ops.export_scene.gltf(filepath=os.path.join(OUT_TOWERS, 'geometry.glb'), export_format='GLB',
                          export_yup=True, export_apply=True, export_image_format='AUTO')
import json  # noqa: E402
with open(os.path.join(OUT_TOWERS, 'asset.json'), 'w') as f:
    json.dump({
        'id': 'asset.models.environment.' + TOWERS, 'name': 'Glass Road: station gate towers', 'kind': 'prop',
        'tags': ['underleaf', 'glass-road', 'beetles', 'tripo', 'wall'],
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'vegetationClearance': 1, 'groundContact': {'mode': 'pivot'}},
        'license': LICENSE,
    }, f, indent=2, ensure_ascii=False)
    f.write('\n')
for folder in (OUT_GATE, OUT_TOWERS):
    blend1 = os.path.join(folder, 'source.blend1')
    if os.path.exists(blend1):
        os.remove(blend1)
print('tower blockers: width', round(outer - LEAVES / 2, 2), 'depth', round(depth, 2), 'at x ±',
      round((outer + LEAVES / 2) / 2, 2))
