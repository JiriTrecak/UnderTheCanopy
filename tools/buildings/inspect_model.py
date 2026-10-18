"""Inspects a GLB (a Tripo export or a built building) before and after its building script:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/inspect_model.py -- \\
        <model.glb> <out prefix> <width in metres> [frame]

Scales the model to `width` across with its pivot at the footprint centre on the ground (as
the building scripts do), lists every mesh part (faces, centre, extent in metres, textures),
and renders `<prefix>-top.png`, `-front.png`, `-side.png` (orthographic, 1 m grid) and
`-rts.png` (the game's overhead angle) at `frame` of the first action, so parts, sockets and
pivots can be found by eye. Blender is Z-up; the building faces -Y."""
import math, sys

import bpy
import mathutils

args = sys.argv[sys.argv.index('--') + 1:]
GLB, OUT, WIDTH = args[0], args[1], float(args[2])
FRAME = int(args[3]) if len(args) > 3 else 0

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.ops.import_scene.gltf(filepath=GLB)
sc = bpy.context.scene
sc.frame_set(FRAME)
meshes = [o for o in sc.objects if o.type == 'MESH']
pts = [o.matrix_world @ v.co for o in meshes for v in o.data.vertices]
lo = mathutils.Vector([min(p[i] for p in pts) for i in range(3)])
hi = mathutils.Vector([max(p[i] for p in pts) for i in range(3)])
s = WIDTH / max(hi.x - lo.x, hi.y - lo.y)
c = (lo + hi) / 2
print('size %.2f x %.2f x %.2f m' % tuple(v * s for v in (hi - lo)))
for o in sorted(meshes, key=lambda o: o.name):
    ps = [o.matrix_world @ v.co for v in o.data.vertices]
    mn = [(min(p[i] for p in ps) - (c[i] if i < 2 else lo.z)) * s for i in range(3)]
    mx = [(max(p[i] for p in ps) - (c[i] if i < 2 else lo.z)) * s for i in range(3)]
    tex = sorted({n.image.name for m in o.data.materials if m and m.use_nodes for n in m.node_tree.nodes
                  if n.type == 'TEX_IMAGE' and n.image})
    print('%-16s %6d faces  centre (%5.2f %5.2f %5.2f)  x %5.2f..%5.2f  y %5.2f..%5.2f  z %5.2f..%5.2f  %s' % (
        o.name, len(o.data.polygons), *[(a + b) / 2 for a, b in zip(mn, mx)], mn[0], mx[0], mn[1], mx[1], mn[2], mx[2],
        ', '.join(tex)))
for o in sc.objects:
    if o.parent is None:
        o.location = (o.location - mathutils.Vector((c.x, c.y, lo.z))) * s
        o.scale = o.scale * s

sc.render.engine = 'BLENDER_EEVEE'
sc.render.resolution_x = sc.render.resolution_y = 900
world = bpy.data.worlds.new('w')
sc.world = world
world.use_nodes = True
world.node_tree.nodes['Background'].inputs['Color'].default_value = (0.094, 0.094, 0.106, 1)  # zinc #18181b
world.node_tree.nodes['Background'].inputs['Strength'].default_value = 1.5
sun = bpy.data.objects.new('sun', bpy.data.lights.new('sun', 'SUN'))
sc.collection.objects.link(sun)
sun.data.energy = 3.0
sun.rotation_euler = (math.radians(50), 0, math.radians(-40))
side = math.ceil(WIDTH / 2) * 2 + 4
bpy.ops.mesh.primitive_grid_add(x_subdivisions=side + 1, y_subdivisions=side + 1, size=side)
grid = bpy.context.active_object
grid.modifiers.new('lines', 'WIREFRAME').thickness = 0.04
lines = bpy.data.materials.new('lines')
lines.diffuse_color = (0.32, 0.32, 0.36, 1)  # zinc #52525b
grid.data.materials.append(lines)
cam = bpy.data.objects.new('cam', bpy.data.cameras.new('cam'))
sc.collection.objects.link(cam)
sc.camera = cam
cam.data.type = 'ORTHO'
cam.data.ortho_scale = WIDTH * 1.3
mid = (hi.z - lo.z) * s / 2
for name, loc, rot in [('top', (0, 0, 60), (0, 0, 0)),
                       ('front', (0, -60, mid), (math.pi / 2, 0, 0)),
                       ('side', (60, 0, mid), (math.pi / 2, 0, math.pi / 2))]:
    cam.location, cam.rotation_euler = loc, rot
    sc.render.filepath = f'{OUT}-{name}.png'
    bpy.ops.render.render(write_still=True)
cam.data.type = 'PERSP'
cam.data.lens = 50
target = mathutils.Vector((0, 0, mid * 0.6))
pitch, d = math.radians(56), WIDTH * 2.4
cam.location = target + mathutils.Vector((0, -math.cos(pitch) * d, math.sin(pitch) * d))
cam.rotation_euler = (target - cam.location).to_track_quat('-Z', 'Y').to_euler()
sc.render.filepath = f'{OUT}-rts.png'
bpy.ops.render.render(write_still=True)
