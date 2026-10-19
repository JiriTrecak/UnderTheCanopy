"""Gives the Warden its barrier pane, so it reads from the game camera and from above:

    /Applications/Blender.app/Contents/MacOS/Blender -b base/assets/units/beetles-warden/source.blend \\
        -P tools/units/warden_pane.py

The Warden (tools/units/tripo_unit.py) holds its projector low at its side, edge-on: from above
the pane that is its whole identity disappears. This adds the field it projects: a curved,
translucent violet pane in front of its body, skinned to the upper spine so it moves with the
torso, glowing from within (substance shows only inside devices and fields, never on bodies).
It rewrites the asset in place, keeping asset.json's animations and adding the pane material."""
import json, math, os, sys

import bmesh
import bpy
import mathutils

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
NAME = 'beetles-warden'
OUT = os.path.join(ROOT, 'base', 'assets', 'units', NAME)
V = mathutils.Vector

if 'barrier-pane' in bpy.data.objects:
    bpy.data.objects.remove(bpy.data.objects['barrier-pane'])
rig = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
body = [o for o in bpy.data.objects if o.type == 'MESH']
for o in rig.children:
    if o.animation_data:
        o.animation_data.action = None
rig.animation_data.action = None
bpy.context.view_layer.update()


# Everything below is in the armature's own space, where the bones and the skinned body agree.
to_rig = rig.matrix_world.inverted()


def bone(name):
    return rig.data.bones['mixamorig:' + name].head_local.copy()


hips, chest, neck = bone('Hips'), bone('Spine2'), bone('Neck')
# The body's front: the rest mesh's most forward points (-Y) at chest height.
pts = [to_rig @ o.matrix_world @ v.co for o in body for v in o.data.vertices]
band = [p for p in pts if hips.z < p.z < neck.z]
torso = [p for p in band if abs(p.x - chest.x) < 0.12]  # the centre column: no arms, no projector
front = min(p.y for p in torso)
half = (max(p.x for p in band) - min(p.x for p in band)) / 2
centre = V((chest.x, chest.y, 0))
radius = (chest.y - front) + 0.1  # just clear of the chest plate
bottom, top = hips.z - 0.08, neck.z + 0.02
ARC = math.radians(120)
print('pane radius', round(radius, 2), 'z', round(bottom, 2), round(top, 2), 'body half-width', round(half, 2))

bm = bmesh.new()
cols, rows = 16, 6
grid = []
for j in range(rows + 1):
    z = bottom + (top - bottom) * j / rows
    bulge = 1.0 + 0.06 * math.sin(math.pi * j / rows)  # a gentle outward belly
    row = []
    for i in range(cols + 1):
        a = -math.pi / 2 - ARC / 2 + ARC * i / cols  # centred on -Y (the front)
        row.append(bm.verts.new((centre.x + math.cos(a) * radius * bulge, centre.y + math.sin(a) * radius * bulge, z)))
    grid.append(row)
uv = bm.loops.layers.uv.new('UVMap')
for j in range(rows):
    for i in range(cols):
        f = bm.faces.new((grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]))
        for loop, (u, w) in zip(f.loops, ((i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1))):
            loop[uv].uv = (u / cols, w / rows)
mesh = bpy.data.meshes.new('barrier-pane')
bm.to_mesh(mesh)
bm.free()
pane = bpy.data.objects.new('barrier-pane', mesh)
bpy.context.scene.collection.objects.link(pane)

# A violet field: bright rim, fainter centre, all glowing.
size = 128
img = bpy.data.images.new('barrier-pane', size, size, alpha=True)
px = []
for y in range(size):
    for x in range(size):
        u, w = (x + 0.5) / size, (y + 0.5) / size
        edge = max(abs(u - 0.5), abs(w - 0.5)) * 2
        rim = max(0.0, (edge - 0.82) / 0.18) ** 1.5
        lines = 0.5 + 0.5 * math.cos(w * math.pi * 10)  # faint horizontal scan bands
        a = min(1.0, 0.28 + 0.10 * lines + 0.62 * rim)
        px += [0.55 + 0.35 * rim, 0.38 + 0.3 * rim, 1.0, a]
img.pixels[:] = px
img.pack()
mat = bpy.data.materials.new('barrier-pane')
mat.use_nodes = True
nt = mat.node_tree
bsdf = nt.nodes['Principled BSDF']
tex = nt.nodes.new('ShaderNodeTexImage')
tex.image = img
nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
nt.links.new(tex.outputs['Alpha'], bsdf.inputs['Alpha'])
nt.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 2.5
bsdf.inputs['Roughness'].default_value = 0.2
mat.surface_render_method = 'BLENDED'
mat.use_backface_culling = False
pane.data.materials.append(mat)

# Skinned to the upper spine, like the body.
pane.parent = rig
pane.matrix_parent_inverse = mathutils.Matrix.Identity(4)
group = pane.vertex_groups.new(name='mixamorig:Spine2')
group.add(list(range(len(mesh.vertices))), 1.0, 'REPLACE')
mod = pane.modifiers.new('Armature', 'ARMATURE')
mod.object = rig

asset = json.load(open(os.path.join(OUT, 'asset.json')))
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)
