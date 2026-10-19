"""Makes a Tripo character's clips play in place, in its saved Blender file:

    /Applications/Blender.app/Contents/MacOS/Blender -b base/assets/units/<name>/source.blend \\
        -P tools/units/tripo_in_place.py [-- --pin attack,cast]

Tripo's preset clips (Mixamo style) carry root motion: the hip bone travels forward through a
walk or a run. The game moves the unit itself, so the model ran ahead of its selection circle
and snapped back every loop. Here every clip but `death` loses the hips' horizontal drift: the
straight line from the clip's first key to its last is taken out of both horizontal location
channels and the clip starts at the rest position, so the body stays over its circle and keeps
its sway; the vertical bob stays as it is.
Then the asset is re-exported with its asset.json unchanged. tools/units/tripo_unit.py does the
same for new units (`in_place`), and the cook refuses clips whose root still travels."""
import json, os, sys

import bpy
import mathutils

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402


def in_place(rig, keep=('death',), pinned=()):
    """Takes the horizontal drift out of the root bone's location in every action; the
    `pinned` clips (a shot fired from a step forward) lose their horizontal root motion."""
    root = next(b for b in rig.data.bones if b.parent is None)
    up = root.matrix_local.to_3x3().inverted() @ mathutils.Vector((0, 0, 1))
    vertical = max(range(3), key=lambda i: abs(up[i]))
    path = 'pose.bones["%s"].location' % root.name
    for action in bpy.data.actions:
        if action.name in keep:
            continue
        for fc in kit._fcurves(action):
            if fc.data_path != path or fc.array_index == vertical or len(fc.keyframe_points) < 2:
                continue
            keys = fc.keyframe_points
            x0, y0 = keys[0].co.x, keys[0].co.y
            x1, y1 = keys[-1].co.x, keys[-1].co.y
            slope = (y1 - y0) / (x1 - x0) if x1 != x0 else 0.0
            for k in keys:
                for point in (k.co, k.handle_left, k.handle_right):
                    point.y = 0.0 if action.name in pinned else point.y - (y0 + slope * (point.x - x0))
            print(action.name, root.name, 'channel', fc.array_index, 'drift', round(y1 - y0, 3),
                  'offset', round(y0, 3))


if __name__ == '__main__':
    OUT = os.path.dirname(bpy.data.filepath)
    NAME = os.path.basename(OUT)
    args = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
    pinned = args[args.index('--pin') + 1].split(',') if '--pin' in args else ()
    rig = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
    in_place(rig, pinned=pinned)
    kit.export(OUT, NAME, json.load(open(os.path.join(OUT, 'asset.json'))))
    blend1 = os.path.join(OUT, 'source.blend1')
    if os.path.exists(blend1):
        os.remove(blend1)
