"""Makes looping clips of a skinned unit loop, in its saved Blender file:

    /Applications/Blender.app/Contents/MacOS/Blender -b base/assets/units/<name>/source.blend \\
        -P tools/units/loop_clips.py -- run [walk ...]

A gait exported from a preset (a Tripo/Mixamo run) may end in a different pose than it
starts: the game loops it, and the arms pop back once a cycle. Every channel of the named
clips eases its end onto its start over the clip's second half (a smooth correction that is
zero at the start and the whole difference at the end), so the last frame meets the first.
Channels already closed change by nothing. The asset is re-exported as the other unit tools
do (`kit.export`); its asset.json is kept as it is.
"""
import json, os, sys

import bpy

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

CLIPS = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else []
OUT = os.path.dirname(bpy.data.filepath)


def fcurves(action):
    if getattr(action, 'layers', None):
        return [fc for layer in action.layers for strip in layer.strips for bag in strip.channelbags for fc in bag.fcurves]
    return list(action.fcurves)


def smooth(t):
    t = max(0.0, min(1.0, t))
    return t * t * (3 - 2 * t)


for name in CLIPS:
    action = bpy.data.actions[name]
    a, b = action.frame_range
    worst = 0.0
    for fc in fcurves(action):
        gap = fc.evaluate(a) - fc.evaluate(b)
        if abs(gap) < 1e-6:
            continue
        worst = max(worst, abs(gap))
        for kp in fc.keyframe_points:
            for p in (kp.co, kp.handle_left, kp.handle_right):
                p.y += gap * smooth((p.x - (a + b) / 2) / ((b - a) / 2))
        fc.update()
    print('looped', name, 'largest gap', round(worst, 4))

kit.export(OUT, os.path.basename(OUT), json.load(open(os.path.join(OUT, 'asset.json'))))
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)
