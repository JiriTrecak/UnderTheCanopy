"""Adds a breathing `idle` clip to a Tripo humanoid (Mixamo rig) whose export has none:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/humanoid_idle.py -- \\
        <in.glb> <out.glb> [--from <clip>] [--frame <n>]

Tripo's animated export leaves its idle presets out, so the idle is posed here: the stance at
`--frame` (default 0) of `--from` (default the walk) held for two seconds, the spine breathing
and the head looking slowly about. The output keeps every clip and goes on to
tools/units/tripo_unit.py."""
import math, sys

import bpy
import mathutils

ARGS = sys.argv[sys.argv.index('--') + 1:]
SRC, OUT = ARGS[0], ARGS[1]
FROM = ARGS[ARGS.index('--from') + 1] if '--from' in ARGS else 'walk'
FRAME = int(ARGS[ARGS.index('--frame') + 1]) if '--frame' in ARGS else 0
FRAMES = 60

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.context.scene.render.fps = 30
bpy.ops.import_scene.gltf(filepath=SRC)
rig = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
source = next(a for a in bpy.data.actions if FROM in a.name)
rig.animation_data.action = source
bpy.context.scene.frame_set(FRAME)
stance = {pb.name: (pb.location.copy(), pb.rotation_quaternion.copy()) for pb in rig.pose.bones}


def bone(*names):
    return [pb for pb in rig.pose.bones if any(pb.name.split(':')[-1] == n for n in names)]


spine = bone('Spine', 'Spine1', 'Spine2')
head = bone('Neck', 'Head')
arms = bone('LeftArm', 'RightArm')
idle = bpy.data.actions.new('idle')
rig.animation_data.action = idle
for f in list(range(0, FRAMES, 2)) + [FRAMES]:
    t = f / FRAMES
    breath = math.sin(2 * math.pi * t)
    look = math.sin(2 * math.pi * t + 0.8)
    for pb in rig.pose.bones:
        pb.rotation_mode = 'QUATERNION'
        loc, rot = stance[pb.name]
        extra = mathutils.Quaternion()
        if pb in spine:
            extra = mathutils.Quaternion((1, 0, 0), math.radians(1.2 * breath))
        elif pb in head:
            extra = mathutils.Quaternion((0, 1, 0), math.radians(4 * look))
        elif pb in arms:
            extra = mathutils.Quaternion((0, 0, 1), math.radians(1.5 * breath))
        pb.location = loc
        pb.rotation_quaternion = rot @ extra
        pb.keyframe_insert('location', frame=f)
        pb.keyframe_insert('rotation_quaternion', frame=f)
idle.use_fake_user = True
for a in bpy.data.actions:
    a.use_fake_user = True
rig.animation_data.action = None
print('clips', [(a.name, tuple(round(v) for v in a.frame_range)) for a in bpy.data.actions])
bpy.ops.export_scene.gltf(filepath=OUT, export_format='GLB', export_animations=True, export_animation_mode='ACTIONS',
                          export_force_sampling=True, export_yup=True, export_apply=False)
print('wrote', OUT)
