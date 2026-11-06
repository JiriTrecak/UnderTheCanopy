"""Contact sheets of a unit's animation clips, to judge motion instead of guessing from one frame:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/clip_sheet.py -- \\
        <geometry.glb> <out folder> [--clips attack,walk] [--step 4] [--size 220]
        [--views game,side,front]

For each clip, every `--step`th frame (and the last) is rendered from each view and laid out in
rows, one row per view, frames left to right, into <out>/<clip>.png: the frame number is stamped
at each tile's foot. Views: `game` (the RTS camera, about 55 degrees down, from the front-left),
`side` (level, from the unit's right) and `front`. Framing is fixed per clip from the extent of
the whole clip, so motion reads against still tiles. The unit faces -Y (+Z in glTF), as
tools/units/tripo_unit.py writes it. Neutral zinc background (AGENTS.md, Visual previews)."""
import math, os, subprocess, sys

import bpy
import mathutils

ARGS = sys.argv[sys.argv.index('--') + 1:]
SRC, OUT = ARGS[0], ARGS[1]
CLIPS = ARGS[ARGS.index('--clips') + 1].split(',') if '--clips' in ARGS else None
STEP = int(ARGS[ARGS.index('--step') + 1]) if '--step' in ARGS else 4
SIZE = int(ARGS[ARGS.index('--size') + 1]) if '--size' in ARGS else 220
VIEWS = ARGS[ARGS.index('--views') + 1].split(',') if '--views' in ARGS else ['game', 'side']
DIRS = {'game': (-35, 55), 'side': (90, 5), 'front': (0, 8)}  # (yaw from front, degrees down)

os.makedirs(OUT, exist_ok=True)
bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=SRC)
scene = bpy.context.scene
scene.render.engine = 'BLENDER_WORKBENCH'
scene.display.shading.light = 'STUDIO'
scene.display.shading.color_type = 'TEXTURE'
scene.render.resolution_x = scene.render.resolution_y = SIZE
scene.render.film_transparent = False
# The frame number is burnt into each tile by Blender's stamp (this ffmpeg has no text filter).
r = scene.render
r.use_stamp = True
for flag in ('date', 'time', 'render_time', 'frame_range', 'memory', 'hostname', 'camera', 'lens', 'scene',
             'marker', 'filename', 'sequencer_strip', 'note'):
    setattr(r, 'use_stamp_' + flag, False)
r.use_stamp_frame = True
r.stamp_font_size = max(12, SIZE // 12)
scene.world = bpy.data.worlds.new('zinc')
scene.world.color = (0.0091, 0.0091, 0.0103)  # #18181b in linear
scene.display.shading.background_type = 'WORLD'
cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam'))
cam.data.type = 'ORTHO'
scene.collection.objects.link(cam)
scene.camera = cam
rig = next(o for o in scene.objects if o.type == 'ARMATURE')
# Only what hangs on the rig: the glTF importer adds a stray Icosphere (a bone display shape).
meshes = [o for o in scene.objects if o.type == 'MESH' and o.parent is rig]
actions = [a for a in bpy.data.actions if CLIPS is None or a.name in CLIPS]
rig.animation_data_create()


def extent(action, frames):
    """The box around every mesh vertex over the clip's frames (deformed)."""
    lo = mathutils.Vector((1e9,) * 3)
    hi = -lo
    rig.animation_data.action = action
    dg = bpy.context.evaluated_depsgraph_get()
    for f in frames[:: max(1, len(frames) // 6)] + [frames[-1]]:
        scene.frame_set(f)
        dg = bpy.context.evaluated_depsgraph_get()
        for o in meshes:
            ev = o.evaluated_get(dg)
            m = ev.matrix_world
            for v in ev.to_mesh().vertices:
                p = m @ v.co
                lo = mathutils.Vector(map(min, lo, p))
                hi = mathutils.Vector(map(max, hi, p))
            ev.to_mesh_clear()
    return lo, hi


for action in actions:
    a, b = (int(x) for x in action.frame_range)
    frames = list(range(a, b + 1, STEP))
    if frames[-1] != b:
        frames.append(b)
    lo, hi = extent(action, frames)
    centre = (lo + hi) / 2
    radius = (hi - lo).length / 2
    cam.data.ortho_scale = radius * 2.1
    tiles = []
    for view in VIEWS:
        yaw, down = (math.radians(d) for d in DIRS[view])
        d = mathutils.Vector((math.sin(yaw) * math.cos(down), -math.cos(yaw) * math.cos(down), math.sin(down)))
        cam.location = centre + d * radius * 6
        cam.rotation_euler = (-d).to_track_quat('-Z', 'Y').to_euler()
        for f in frames:
            rig.animation_data.action = action
            scene.frame_set(f)
            path = os.path.join(OUT, '_%s_%s_%04d.png' % (action.name, view, f))
            scene.render.filepath = path
            bpy.ops.render.render(write_still=True)
            tiles.append((view, f, path))
    # Lay out: one row per view, frames left to right, each tile labelled with its frame.
    cols = len(frames)
    inputs, filters = [], []
    for i, (view, f, path) in enumerate(tiles):
        inputs += ['-i', path]
        filters.append('[%d]null[t%d]' % (i, i))
    rows = []
    for r in range(len(VIEWS)):
        row = ''.join('[t%d]' % (r * cols + c) for c in range(cols))
        filters.append('%shstack=inputs=%d[r%d]' % (row, cols, r) if cols > 1 else '%snull[r%d]' % (row, r))
        rows.append('[r%d]' % r)
    filters.append('%svstack=inputs=%d' % (''.join(rows), len(rows)) if len(rows) > 1 else '%snull' % rows[0])
    sheet = os.path.join(OUT, action.name + '.png')
    subprocess.run(['ffmpeg', '-loglevel', 'error', '-y'] + inputs + ['-filter_complex', ';'.join(filters), sheet], check=True)
    for _, _, path in tiles:
        os.remove(path)
    print('sheet', sheet, len(frames), 'frames x', len(VIEWS), 'views')
