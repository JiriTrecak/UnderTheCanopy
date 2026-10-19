"""The Forgemaster's Bolt Turret from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/bolt_turret.py -- <bolt-turret.glb>

The turret is Tripo's image-to-3D model of the chosen concept (the old repository's
.asset-work/concepts/buildings/beetle-bolt-turret-2.png), one textured mesh, unsegmented. It is
a rigid machine: the whole gun turns to its target with the unit, so it is animated as one node.
Here:
- it is scaled to HEIGHT metres and turned to face -Y (glTF +Z): the side the barrels reach
  towards is found from the mesh (the upper half's centre leans that way);
- the royal-blue shield becomes the team mask in the base colour's alpha (violet glass excluded);
- `attack` kicks the gun back and settles it (`release` at the kick), `hit` jolts it, `death`
  tips it over and sinks it; `idle` holds still;
- `socket_projectile` sits at the barrels' mouth, where the bolts leave.

Blender is Z-up in metres; the unit faces -Y (glTF +Z, the asset transform's forward)."""
import math, os, sys

import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
GLB = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-bolt-turret'
OUT = os.path.join(ROOT, 'base', 'assets', 'units', NAME)
HEIGHT = 0.9  # a little under a beetle soldier (1.0 m before display scale)
V = mathutils.Vector

kit.reset()
bpy.ops.import_scene.gltf(filepath=GLB)
meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH']
for o in meshes:
    m = o.matrix_world.copy()
    o.parent = None
    o.matrix_world = m
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        bpy.data.objects.remove(o)
bpy.ops.object.select_all(action='DESELECT')
for o in meshes:
    o.select_set(True)
bpy.context.view_layer.objects.active = meshes[0]
if len(meshes) > 1:
    bpy.ops.object.join()
gun = bpy.context.view_layer.objects.active
gun.name = 'turret'
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# --- Scale, ground and face -Y.
pts = np.array([v.co[:] for v in gun.data.vertices])
lo, hi = pts.min(0), pts.max(0)
s = HEIGHT / (hi[2] - lo[2])
centre = (lo + hi) / 2
pts = (pts - [centre[0], centre[1], lo[2]]) * s
upper = pts[pts[:, 2] > HEIGHT * 0.45]
lean = upper[:, :2].mean(0)
yaw = -math.atan2(lean[0], -lean[1])  # turns the barrels' side to -Y
rot = mathutils.Matrix.Rotation(yaw, 4, 'Z')
for v, p in zip(gun.data.vertices, pts):
    v.co = rot @ V(p)
pts = np.array([v.co[:] for v in gun.data.vertices])
lo, hi = pts.min(0), pts.max(0)
for v in gun.data.vertices:
    v.co.x -= (lo[0] + hi[0]) / 2
    v.co.y -= (lo[1] + hi[1]) / 2
gun.data.update()
pts = np.array([v.co[:] for v in gun.data.vertices])
print('size', np.round(pts.max(0) - pts.min(0), 2), 'yaw', round(math.degrees(yaw)))

# --- Team colour: the royal-blue shield into the base colour's alpha.
slots = []
for mat in {m for m in gun.data.materials if m and m.use_nodes}:
    bsdf = next((n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
    links = bsdf.inputs['Base Color'].links if bsdf else []
    node = links[0].from_node if links else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    if node is None:
        continue
    w, h = node.image.size
    px = np.array(node.image.pixels[:], np.float32).reshape(h, w, 4)
    rgb = px[:, :, :3]
    blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
    violet = rgb[:, :, 0] > rgb[:, :, 1] * 1.6  # the glowing glass: red well above green
    mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08) * ~violet
    img = bpy.data.images.new(NAME + '-team', w, h, alpha=True)
    img.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
    img.alpha_mode = 'CHANNEL_PACKED'
    img.pack()
    node.image = img
    mat.name = 'TC_' + NAME
    mat['teamColorMask'] = 'baseColorAlpha'
    slots.append(mat.name)
    print('team mask', round(float(mask.mean()), 3))

# --- The muzzle: the front-most vertices in the upper half.
upper = pts[pts[:, 2] > HEIGHT * 0.4]
front = upper[upper[:, 1] < upper[:, 1].min() + 0.03]
muzzle = V(front.mean(0)) + V((0, -0.03, 0))
pivot = kit.socket('turret_root', (0, 0, 0))
gun.parent = pivot
sock = kit.socket('socket_projectile', muzzle)
sock.parent = gun

# --- Clips on the root (30 fps): a whole-gun recoil, a jolt, a tip-over.
FIRE = 3
kit.clip(pivot, 'idle', [(0, 0.0), (30, 0.0)], 'location', 1)
kit.clip(pivot, 'attack', [(0, 0.0), (FIRE, 0.07), (8, 0.03), (18, 0.0)], 'location', 1)
kit.clip(pivot, 'hit', [(0, 0.0), (3, 0.08), (10, 0.0)], 'rotation_euler', 0)
kit.clip(pivot, 'death', [(0, 0.0), (10, 0.5), (18, 1.2), (24, 1.15)], 'rotation_euler', 0)

asset = {
    'id': 'asset.models.units.' + NAME, 'name': 'Bolt Turret', 'kind': 'unit',
    'tags': ['beetles', 'machine', 'summon', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': slots},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            # Event times are seconds into the clip.
            {'semantic': 'attack', 'clip': 'attack', 'loop': False,
             'events': [{'name': 'release', 'time': round(FIRE / kit.FPS, 3)}]},
            {'semantic': 'hit', 'clip': 'hit', 'loop': False, 'events': []},
            {'semantic': 'death', 'clip': 'death', 'loop': False, 'events': []},
        ],
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 2, 'character': 'base', 'healthHeight': round(HEIGHT * 1.25, 2)},
    'license': 'Generated with Tripo (image to 3D) from an ImageGen (gpt-image-2) concept for Under the '
               'Canopy; prepared and animated by tools/units/bolt_turret.py.',
}
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)
