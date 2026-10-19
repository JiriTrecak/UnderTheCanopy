"""Stands a Tripo character (tools/units/tripo_unit.py) back on the ground, in place:

    /Applications/Blender.app/Contents/MacOS/Blender -b base/assets/units/<name>/source.blend \\
        -P tools/units/tripo_reground.py

Tripo's exports carry a stray icosphere at the origin. tripo_unit.py once measured it with the
body, took its bottom for the feet and lifted the skeleton by half the body's height, so the
unit floated that high in the game. This drops the skeleton to the ground (the body's feet at
the origin), removes the sphere, puts the health bar just over the head and re-exports the
asset with its asset.json otherwise unchanged."""
import json, os, sys

import bpy

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

OUT = os.path.dirname(bpy.data.filepath)
NAME = os.path.basename(OUT)

rig = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
for o in [o for o in bpy.data.objects if o.type == 'MESH' and o.parent is None]:
    print('removing stray', o.name)
    bpy.data.objects.remove(o, do_unlink=True)
for o in rig.children:
    if o.animation_data:
        o.animation_data.action = None
rig.animation_data.action = None
bpy.context.view_layer.update()
body = [o for o in rig.children if o.type == 'MESH' and not o.name.startswith('barrier')]
zs = [(o.matrix_world @ v.co).z for o in body for v in o.data.vertices]
feet, top = min(zs), max(zs)
rig.location.z -= feet
print(NAME, 'lowered by', round(feet, 3))

path = os.path.join(OUT, 'asset.json')
asset = json.load(open(path))
display = asset.setdefault('display', {})
display['healthHeight'] = round(top - feet + 0.15, 2)  # just over the head
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)
