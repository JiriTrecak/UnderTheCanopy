"""The beetles' Shell Forge from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/shell_forge.py -- \\
        <forge-segmented.glb> <forge-hammer.glb>

The forge is Tripo's image-to-3D model of the approved concept (art/styles/forge-idle.png,
source art/styles/forge-tripo-source.png), split by Tripo's segmentation into parts; the
hammer arm is a separate Tripo model (text to 3D). Here (D65):
- every part with royal-blue plates gets its team mask in its base colour's alpha;
- the furnace mouth and the quench tank get active emission (dark while the forge rests,
  alive while it works);
- the hammer arm turns on a pivot on the pillar beside the anvil: raised at rest, two
  strikes per `work` cycle (`strike` events for the sparks);
- sockets for the state effects: three chimney tops, the furnace mouth, the anvil, the tank."""
import math, os, sys

import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
FORGE, HAMMER = sys.argv[sys.argv.index('--') + 1:][:2]
NAME = 'beetles-shell-forge'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 11.5  # the 12-metre footprint, a little inside its edge

# Segmented parts by role (tripo_part_<n>).
FURNACE, TANK, ANVIL = 'tripo_part_6', 'tripo_part_3', 'tripo_part_5'
CHIMNEYS = {'a': 'tripo_part_9', 'b': 'tripo_part_8', 'c': 'tripo_part_10'}

kit.reset()
bpy.ops.import_scene.gltf(filepath=FORGE)
parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']
# Normalise: footprint centred, ground at 0, WIDTH across; transforms applied.
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        for c in o.children:
            m = c.matrix_world.copy()
            c.parent = None
            c.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
pts = [o.matrix_world @ v.co for o in parts for v in o.data.vertices]
lo = mathutils.Vector([min(p[i] for p in pts) for i in range(3)])
hi = mathutils.Vector([max(p[i] for p in pts) for i in range(3)])
scale = WIDTH / max(hi.x - lo.x, hi.y - lo.y)
shift = mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation((-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z))
for o in parts:
    o.data.transform(shift @ o.matrix_world)
    o.matrix_world = mathutils.Matrix.Identity(4)
    for p in o.data.polygons:
        p.use_smooth = True


def texture_of(mat, socket='Base Color'):
    bsdf = mat.node_tree.nodes['Principled BSDF']
    link = bsdf.inputs[socket].links
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


def pixels(img):
    w, h = img.size
    return np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)


# --- Team colour: blue plates become the mask in each part's base colour alpha.
team_slots = []
for o in parts:
    mat = o.data.materials[0]
    node = texture_of(mat)
    if node is None:
        continue
    px = pixels(node.image)
    rgb = px[:, :, :3]
    blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
    mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08)
    if mask.mean() < 0.002:
        continue
    out = bpy.data.images.new(o.name + '-team', node.image.size[0], node.image.size[1], alpha=True)
    out.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
    out.alpha_mode = 'CHANNEL_PACKED'
    out.pack()
    node.image = out
    mat.name = 'TC_' + o.name
    mat['teamColorMask'] = 'baseColorAlpha'
    team_slots.append(mat.name)
    print('team', o.name, round(float(mask.mean()), 3))


def active_glow(obj, color, weight, emission, pulse):
    """Emission from the part's own texture, strongest where `weight(rgb)` is, alive only
    while the forge works."""
    mat = obj.data.materials[0]
    node = texture_of(mat)
    px = pixels(node.image)
    glow = weight(px[:, :, :3])[..., None] * np.array(color, np.float32)
    img = bpy.data.images.new(obj.name + '-glow', node.image.size[0], node.image.size[1], alpha=False)
    img.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
    img.pack()
    tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
    tex.image = img
    bsdf = mat.node_tree.nodes['Principled BSDF']
    mat.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
    bsdf.inputs['Emission Strength'].default_value = emission
    mat['activeGlow'] = {'pulse': pulse, 'flow': 0.0}


lum = lambda rgb: rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32)  # noqa: E731
by_name = {o.name: o for o in parts}
# The furnace mouth: only its dark interior burns, never its stone ring.
active_glow(by_name[FURNACE], (1.0, 0.45, 0.1), lambda rgb: np.clip((0.1 - lum(rgb)) / 0.06, 0.0, 1.0) ** 1.5,
            5.0, 1.6)
# The quench tank: its dark glass glows with the forest's blue-purple magic from within;
# steel and brass stay as they are.
active_glow(by_name[TANK], (0.55, 0.32, 1.0), lambda rgb: np.clip((0.2 - lum(rgb)) / 0.12, 0.0, 1.0),
            5.0, 0.9)

# --- Sockets.
def top_of(obj):
    vs = [v.co for v in obj.data.vertices]
    zmax = max(v.z for v in vs)
    ring = [v for v in vs if v.z > zmax - 0.25]
    return mathutils.Vector((sum(v.x for v in ring) / len(ring), sum(v.y for v in ring) / len(ring), zmax))


for key, part in CHIMNEYS.items():
    kit.socket('socket_chimney_' + key, top_of(by_name[part]) + mathutils.Vector((0, 0, 0.3)))
fur = by_name[FURNACE]
fy = min(v.co.y for v in fur.data.vertices)
fx = sum(v.co.x for v in fur.data.vertices) / len(fur.data.vertices)
kit.socket('socket_furnace', (fx, fy - 0.4, 1.7))
anvil_top = top_of(by_name[ANVIL])
kit.socket('socket_anvil', anvil_top + mathutils.Vector((0, 0, 0.05)))
kit.socket('socket_tank', top_of(by_name[TANK]) - mathutils.Vector((0, 0, 0.8)))

# --- The hammer arm: Tripo's beam lies along +X with its head at +X. Turned head to −X,
# 2 m long, its pivot at the hub end on a post right of the anvil, so it swings across the
# RTS camera's view (a swing towards the camera would not read).
bpy.ops.import_scene.gltf(filepath=HAMMER)
hammer = [o for o in bpy.context.scene.objects if o.type == 'MESH' and o not in parts][0]
LENGTH = 3.0
hm = hammer.matrix_world.copy()
hammer.parent = None
hammer.data.transform(hm)
hammer.matrix_world = mathutils.Matrix.Identity(4)
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH' and not o.name.startswith('socket')]:
    bpy.data.objects.remove(o, do_unlink=True)
hv = [v.co for v in hammer.data.vertices]
hlo = mathutils.Vector([min(v[i] for v in hv) for i in range(3)])
hhi = mathutils.Vector([max(v[i] for v in hv) for i in range(3)])
hs = LENGTH / (hhi.x - hlo.x)
# Hub end (−X before the turn) to the origin at the beam's mid height, then head to −X.
beam_mid = hlo.z + (hhi.z - hlo.z) * 0.7
hammer.data.transform(mathutils.Matrix.Rotation(math.pi, 4, 'Z') @ mathutils.Matrix.Scale(hs, 4)
                      @ mathutils.Matrix.Translation((-hlo.x, -(hlo.y + hhi.y) / 2, -beam_mid)))
for p in hammer.data.polygons:
    p.use_smooth = True
dec = hammer.modifiers.new('decimate', 'DECIMATE')
dec.ratio = 1600 / len(hammer.data.polygons)
bpy.context.view_layer.objects.active = hammer
bpy.ops.object.modifier_apply(modifier=dec.name)
hv = [v.co for v in hammer.data.vertices]
head = [v for v in hv if v.x < -LENGTH * 0.6]
head_bottom = min(v.z for v in head)
head_x = sum(v.x for v in head) / len(head)
# The strike lands the head flat on the anvil; the pivot is set from that.
pivot = mathutils.Vector((anvil_top.x - head_x, anvil_top.y, anvil_top.z - head_bottom))
hammer.name = 'hammer-arm'
hammer.rotation_mode = 'XYZ'  # glTF imports use quaternions; the clips key Euler angles
hammer.location = pivot
hammer.data.materials[0].name = 'hammer'
# Its post: a plain steel column under the hub, in the hammer's own material.
post = kit.cylinder('hammer-post', 0.16, pivot.z - 0.6, at=(pivot.x, pivot.y, 0.6), verts=16)
post.data.materials.append(hammer.data.materials[0])
kit.cube_uv(post, 1.0)  # a textured material needs UVs (the exporter writes −1 without)
for p in post.data.polygons:
    p.use_smooth = True
STRIKE, REST = 0.0, 0.6  # about Y: positive lifts the head (at −X)
kit.clip(hammer, 'idle', [(0, REST), (2, REST)], index=1)
F = 48
kit.clip(hammer, 'work', [(0, REST), (10, REST + 0.1), (18, STRIKE), (21, STRIKE + 0.05), (28, REST * 0.55),
                          (35, REST * 0.7), (42, STRIKE), (45, STRIKE + 0.05), (F, REST)], index=1)
print('pivot', [round(v, 2) for v in pivot], 'head', round(head_x, 2), round(head_bottom, 2))

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Shell Forge', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': team_slots},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            {'semantic': 'work', 'clip': 'work', 'loop': True,
             # Event times are seconds into the clip.
             'events': [{'name': 'strike', 'time': round(18 / kit.FPS, 3)}, {'name': 'strike', 'time': round(42 / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [12, 12], 'offset': [0, 0]},
        'vegetationClearance': 6,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': 8.5},
    'shading': 'building',
    'license': 'Generated with Tripo (image and text to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/buildings/shell_forge.py.',
}
kit.export(OUT, NAME, asset)
