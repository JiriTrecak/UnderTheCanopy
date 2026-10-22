"""The beetles' Supply Cart from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/supply_cart.py -- \\
        <beetles-supply-cart-segmented.glb>

The cart is Tripo's image-to-3D model of the concept (the old repository's
.asset-work/concepts/buildings/beetle-supply-cart-pulled-2.png: a stout draft beetle pulling a
low four-wheeled brass wagon of three glass tanks), split by Tripo's detailed segmentation into
parts that share one texture atlas. Like the Bombard it is a rigid machine, animated by moving
whole parts as nodes, never skinned. Here:
- Tripo stood the beetle a hand's breadth above the ground: it is lowered onto its claws and
  the draw bars bend down to it;
- every leg is its own node pivoting at its hip, the beetle's body (head, shell) and the wagon
  are nodes, and each wheel turns on its own axle; the walk is the insect tripod gait with a
  body bob, the wagon rocking a little and its wheels rolling with the ground it covers;
- `hit` jolts beetle and wagon; on `death` the beetle's legs give way, it drops onto its belly,
  and the wagon lurches and lists onto one side;
- the royal-blue plates become the team mask in the atlas's alpha (the violet glass excluded);
- the violet substance in the tanks glows (constant emission: units have no activity);
- sockets: `socket_tanks` over the tanks, `socket_beetle` on the beetle's back.

Blender is Z-up in metres; the unit faces -Y (glTF +Z, the asset transform's forward)."""
import math, os, sys

import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
GLB = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-supply-cart'
OUT = os.path.join(ROOT, 'base', 'assets', 'units', NAME)
LENGTH = 5.0  # beetle's mandibles to the wagon's tail, metres (a beetle soldier stands 2.0)
V = mathutils.Vector
ATLAS = 1024

# Segmented parts by role (tripo_part_<n>; numbers hold for this one export). Positions are in
# the inspection frame: 4 m long, centred, ground at 0, the beetle towards -X.
WHEELS = {'wheel_fl': [3, 81], 'wheel_fr': [5, 109], 'wheel_rl': [1, 102], 'wheel_rr': [0, 64]}
LEGS = {  # a: +Y side, b: -Y side; 1 front, 2 middle, 3 rear
    'leg_a1': [101, 27, 28], 'leg_b1': [49, 88, 62],
    'leg_a2': [20, 80], 'leg_b2': [38, 75],
    'leg_a3': [14, 95, 108], 'leg_b3': [51, 74, 60],
}
BEETLE = [68, 83, 21, 12, 4, 11, 73, 45, 97]  # mandibles, head, shell, belly, tail knob
BARS = [89, 98, 36, 41, 32]  # the draw bars between the beetle and the wagon (bent down)
BAR_SPAN = (-0.6, 0.3)  # x over which the bars bend from the beetle's drop to the wagon

kit.reset()
bpy.ops.import_scene.gltf(filepath=GLB)
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        for c in o.children:
            m = c.matrix_world.copy()
            c.parent = None
            c.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
parts = {int(o.name.rsplit('_', 1)[1]): o for o in bpy.context.scene.objects if o.type == 'MESH'}

# The inspection frame.
pts = [o.matrix_world @ v.co for o in parts.values() for v in o.data.vertices]
lo = V([min(p[i] for p in pts) for i in range(3)])
hi = V([max(p[i] for p in pts) for i in range(3)])
f = 4.0 / max(hi.x - lo.x, hi.y - lo.y)
frame = mathutils.Matrix.Scale(f, 4) @ mathutils.Matrix.Translation((-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z))
for o in parts.values():
    o.data.transform(frame @ o.matrix_world)
    o.matrix_world = mathutils.Matrix.Identity(4)
    for p in o.data.polygons:
        p.use_smooth = True
MAT = parts[0].data.materials[0]  # the one shared atlas material
for o in parts.values():
    o.data.materials.clear()
    o.data.materials.append(MAT)

# --- The beetle onto its claws: down by the gap under its lowest claw; the bars bend to it.
legs_v = [o.matrix_world @ v.co for n in sum(LEGS.values(), []) for v in parts[n].data.vertices]
GAP = min(p.z for p in legs_v) - 0.01
for n in BEETLE + sum(LEGS.values(), []):
    parts[n].data.transform(mathutils.Matrix.Translation((0, 0, -GAP)))
for n in BARS:
    for v in parts[n].data.vertices:
        t = min(1.0, max(0.0, (BAR_SPAN[1] - v.co.x) / (BAR_SPAN[1] - BAR_SPAN[0])))
        v.co.z -= GAP * t
print('beetle lowered', round(GAP, 3), 'm (inspection frame)')


def join(numbers, name):
    objs = [parts.pop(n) for n in numbers]
    with bpy.context.temp_override(active_object=objs[0], object=objs[0], selected_objects=objs,
                                   selected_editable_objects=objs):
        bpy.ops.object.join()
    objs[0].name = objs[0].data.name = name
    return objs[0]


wheels = {name: join(numbers, name) for name, numbers in WHEELS.items()}
legs = {name: join(numbers, name) for name, numbers in LEGS.items()}
beetle = join(BEETLE, 'beetle')
wagon = join(sorted(parts), 'wagon')

# --- Final frame: the beetle to -Y, LENGTH long, centred, on the ground.
objs = [wagon, beetle, *legs.values(), *wheels.values()]
turn = mathutils.Matrix.Rotation(math.pi / 2, 4, 'Z')
for o in objs:
    o.data.transform(turn)
allv = [v.co for o in objs for v in o.data.vertices]
lo = V([min(v[i] for v in allv) for i in range(3)])
hi = V([max(v[i] for v in allv) for i in range(3)])
k = LENGTH / (hi.y - lo.y)
place = mathutils.Matrix.Scale(k, 4) @ mathutils.Matrix.Translation((-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z))
for o in objs:
    o.data.transform(place)
S = k  # metres of the inspection frame -> metres now
HEIGHT = (hi.z - lo.z) * k
print('size %.2f x %.2f x %.2f m' % ((hi.x - lo.x) * k, LENGTH, HEIGHT))


def verts(obj):
    return np.array([v.co[:] for v in obj.data.vertices])


def set_pivot(obj, pivot, parent=None, heading=0.0):
    """Puts the node origin at `pivot` (world) turned by `heading` about Z, mesh unmoved."""
    obj.data.transform(mathutils.Matrix.Rotation(-heading, 4, 'Z') @ mathutils.Matrix.Translation(-pivot))
    obj.rotation_mode = 'XYZ'
    obj.rotation_euler = (0, 0, heading)
    if parent is None:
        obj.location = pivot
    else:
        obj.parent = parent
        obj.matrix_parent_inverse = mathutils.Matrix.Identity(4)
        obj.location = pivot - parent.location
    return obj


# Wagon: its node at the axles' height under the wagon's middle (rock, list, lurch).
wv = verts(wagon)
WAGON = V((0.0, float((wv[:, 1].min() + wv[:, 1].max()) / 2), 0.36 * S))
set_pivot(wagon, WAGON)

# Wheels: each turns on its axle (X), its node at the hub, carried by the wagon.
RADIUS = 0.0
for name, wheel in wheels.items():
    v = verts(wheel)
    hub = V(((v[:, 0].min() + v[:, 0].max()) / 2, (v[:, 1].min() + v[:, 1].max()) / 2, (v[:, 2].min() + v[:, 2].max()) / 2))
    RADIUS = max(RADIUS, float(v[:, 2].max() - v[:, 2].min()) / 2)
    set_pivot(wheel, hub, wagon)
print('wheel radius', round(RADIUS, 3))

# Beetle: its node at the middle of its body at hip height (bob, sway, drop).
bv = verts(beetle)
BODY = V((0.0, float((bv[:, 1].min() + bv[:, 1].max()) / 2), float(bv[:, 2].min())))
set_pivot(beetle, BODY)

# Legs: each pivots at its hip (the end nearest the body's long axis), heading outwards; they
# hang from the world (not the body), as the Bombard's do, so a bob never lifts a planted claw.
leg_info = {}
for name, leg in legs.items():
    lv = verts(leg)
    d = np.abs(lv[:, 0])
    inner = lv[d <= np.quantile(d, 0.08)]
    tip = lv[lv[:, 2] <= lv[:, 2].min() + 0.08 * S]
    hip = V(inner.mean(axis=0))
    t = V(tip.mean(axis=0))
    heading = math.atan2(t.y - hip.y, t.x - hip.x)
    set_pivot(leg, hip, heading=heading)
    leg_info[name] = (hip, t, heading, (t.xy - hip.xy).length)
    print(name, 'hip', [round(a, 2) for a in hip], 'tip', [round(a, 2) for a in t],
          'heading', round(math.degrees(heading)))


# --- Team colour: the royal-blue plates into the atlas's alpha (violet glass is not team).
def texture_of(mat, socket='Base Color'):
    bsdf = mat.node_tree.nodes['Principled BSDF']
    link = bsdf.inputs[socket].links
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


node = texture_of(MAT)
W, H = node.image.size
px = np.array(node.image.pixels[:], np.float32).reshape(H, W, 4)
rgb = px[:, :, :3]
blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
royal = rgb[:, :, 0] <= rgb[:, :, 1] + 0.03
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08) * royal
team = bpy.data.images.new(NAME + '-team', W, H, alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
MAT.name = 'TC_' + NAME
MAT['teamColorMask'] = 'baseColorAlpha'
print('team mask mean', round(float(mask.mean()), 4))

# --- The substance glows: the violet texels of the atlas (the tanks' glass), dark elsewhere.
violet = np.clip((rgb[:, :, 2] - rgb[:, :, 1] - 0.05) / 0.12, 0, 1) * np.clip((rgb[:, :, 0] - rgb[:, :, 1] - 0.02) / 0.06, 0, 1)
glow = violet[..., None] * np.array((0.5, 0.3, 1.0), np.float32)
gimg = bpy.data.images.new(NAME + '-glow', W, H, alpha=False)
gimg.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
gimg.pack()
print('glass texels', int((violet > 0.5).sum()))
tex = MAT.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = gimg
bsdf = MAT.node_tree.nodes['Principled BSDF']
MAT.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 1.6
# A unit needs no 2048 atlas: 1024 keeps the file small and reads the same at game zoom.
for n in MAT.node_tree.nodes:
    if n.type == 'TEX_IMAGE' and n.image and n.image.size[0] > ATLAS:
        n.image.scale(ATLAS, ATLAS)
        n.image.pack()

# --- Sockets.
top = float(verts(wagon)[:, 2].max())
tanks = kit.socket('socket_tanks', (0, 0, 0))
tanks.parent = wagon
tanks.matrix_parent_inverse = mathutils.Matrix.Identity(4)
tanks.location = (0, 0, top + 0.1 * S)
back = kit.socket('socket_beetle', (0, 0, 0))
back.parent = beetle
back.matrix_parent_inverse = mathutils.Matrix.Identity(4)
back.location = (0, 0, float(bv[:, 2].max() - bv[:, 2].min()) + 0.05 * S)

tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in objs)
print('triangles', tris)


# --- Clips: one action per clip, a slot per moving node (merged into one glTF animation).
def key(obj, name, path, index, keys, interpolation='BEZIER'):
    action = bpy.data.actions.get(name) or bpy.data.actions.new(name)
    ad = obj.animation_data_create()
    ad.action = action
    rest = getattr(obj, path)[index]
    for fr, value in keys:
        getattr(obj, path)[index] = value
        obj.keyframe_insert(data_path=path, index=index, frame=fr)
    getattr(obj, path)[index] = rest
    for fc in kit._fcurves(action):
        if fc.data_path == path and fc.array_index == index:
            for kp in fc.keyframe_points:
                kp.interpolation = interpolation
    ad.action = None


def offsets(obj, name, path, index, keys, interpolation='BEZIER'):
    """Keys as offsets from the node's rest value."""
    rest = getattr(obj, path)[index]
    key(obj, name, path, index, [(fr, rest + d) for fr, d in keys], interpolation)


CHANNELS = [('location', 0), ('location', 1), ('location', 2),
            ('rotation_euler', 0), ('rotation_euler', 1), ('rotation_euler', 2)]
MOVING = [wagon, beetle, *legs.values(), *wheels.values()]


def clip(name, length, motion):
    """`motion[(obj, path, index)] = [(frame, offset), ...]`; every channel of every moving
    node is keyed (held at rest where it does not move), so the clips blend cleanly."""
    for obj in MOVING:
        for path, index in CHANNELS:
            keys = motion.get((obj, path, index), [(0, 0.0), (length, 0.0)])
            interp = 'LINEAR' if (obj, path, index, 'linear') in motion else 'BEZIER'
            offsets(obj, name, path, index, keys, interp)
    for obj in MOVING:
        ad = obj.animation_data
        track = ad.nla_tracks.new()
        track.name = name
        track.strips.new(name, 0, bpy.data.actions[name])


def linear(motion, obj, path, index, keys):
    motion[(obj, path, index)] = keys
    motion[(obj, path, index, 'linear')] = True


# idle (2 s loop): the draft beetle breathes, its shell rising and settling.
IDLE = 60
motion = {}
linear(motion, beetle, 'location', 2, [(fr, 0.012 * S * 0.5 * (1 - math.cos(2 * math.pi * fr / IDLE))) for fr in range(0, IDLE + 1, 5)])
linear(motion, beetle, 'rotation_euler', 0, [(fr, 0.012 * math.sin(2 * math.pi * fr / IDLE)) for fr in range(0, IDLE + 1, 5)])
clip('idle', IDLE, motion)

# walk (1 s loop): the beetle's tripod gait (triplet A pushes back while triplet B swings
# forward lifted, then they swap), its body bobbing once per step; the wagon rocks gently and
# its wheels roll a quarter turn, the ground the beetle's stride covers.
WALK = 30
SWING, LIFT = 0.3, 0.3
A = {'leg_a1', 'leg_b2', 'leg_a3'}
motion = {}
stride = []
for name, leg in legs.items():
    hip, tip, heading, reach = leg_info[name]
    side = 1.0 if math.cos(heading) > 0 else -1.0
    zs, ys = [], []
    for fr in range(0, WALK + 1, 2):
        p = (fr / WALK + (0.0 if name in A else 0.5)) % 1.0
        if p < 0.5:
            fwd, lift = 1 - 4 * p, 0.0
        else:
            fwd, lift = -1 + 4 * (p - 0.5), math.sin(math.pi * (p - 0.5) * 2)
        zs.append((fr, -fwd * SWING * side))
        ys.append((fr, -lift * LIFT))
    linear(motion, leg, 'rotation_euler', 2, zs)
    linear(motion, leg, 'rotation_euler', 1, ys)
    stride.append(2 * SWING * reach)
# Each half cycle the planted triplet carries the body one stride: two strides a second.
SPEED = 2 * sum(stride) / len(stride) / (WALK / kit.FPS)
ROLL = SPEED * (WALK / kit.FPS) / RADIUS
print('walk speed', round(SPEED, 3), 'm/s; wheel turn', round(math.degrees(ROLL)), 'degrees a loop')
bob = 0.03 * S
linear(motion, beetle, 'location', 2, [(fr, bob * 0.5 * (1 - math.cos(4 * math.pi * fr / WALK))) for fr in range(0, WALK + 1, 3)])
linear(motion, beetle, 'rotation_euler', 1, [(fr, 0.02 * math.sin(2 * math.pi * fr / WALK)) for fr in range(0, WALK + 1, 3)])
linear(motion, wagon, 'rotation_euler', 1, [(fr, 0.012 * math.sin(2 * math.pi * fr / WALK + 1.2)) for fr in range(0, WALK + 1, 3)])
linear(motion, wagon, 'location', 2, [(fr, 0.008 * S * math.sin(4 * math.pi * fr / WALK)) for fr in range(0, WALK + 1, 3)])
for wheel in wheels.values():
    linear(motion, wheel, 'rotation_euler', 0, [(0, 0.0), (WALK, ROLL)])
clip('walk', WALK, motion)

# hit (0.4 s): the beetle flinches and the wagon jolts.
HIT = 12
motion = {
    (beetle, 'rotation_euler', 1): [(0, 0), (3, 0.07), (7, -0.03), (HIT, 0)],
    (beetle, 'location', 2): [(0, 0), (3, -0.03 * S), (HIT, 0)],
    (wagon, 'rotation_euler', 1): [(0, 0), (3, -0.05), (7, 0.02), (HIT, 0)],
    (wagon, 'location', 2): [(0, 0), (3, 0.03 * S), (HIT, 0)],
}
clip('hit', HIT, motion)

# death (1.6 s, held): the beetle's legs give way and splay, it drops onto its belly; the
# wagon lurches forward into it, a wheel side sinks and it lists over, tanks still glowing.
DEATH = 48
belly = float(bv[:, 2].min())  # the body node sits at the belly: drop it to the ground
DROP = BODY.z - 0.02 * S
motion = {
    (beetle, 'location', 2): [(0, 0), (6, 0.03 * S), (18, -DROP), (22, -DROP + 0.04 * S), (26, -DROP), (DEATH, -DROP)],
    (beetle, 'rotation_euler', 1): [(0, 0), (18, 0.1), (24, 0.08), (DEATH, 0.08)],
    (beetle, 'rotation_euler', 0): [(0, 0), (18, -0.06), (DEATH, -0.05)],
    (wagon, 'location', 1): [(0, 0), (14, 0), (24, -0.12 * S), (DEATH, -0.12 * S)],
    (wagon, 'location', 0): [(0, 0), (20, 0), (34, -0.08 * S), (DEATH, -0.08 * S)],
    (wagon, 'location', 2): [(0, 0), (20, 0), (34, -0.1 * S), (38, -0.08 * S), (DEATH, -0.09 * S)],
    (wagon, 'rotation_euler', 1): [(0, 0), (20, 0), (34, -0.3), (38, -0.26), (DEATH, -0.28)],
    (wagon, 'rotation_euler', 0): [(0, 0), (16, 0), (24, 0.06), (DEATH, 0.05)],
}
for i, (name, leg) in enumerate(legs.items()):
    hip, tip, heading, reach = leg_info[name]
    lift = min(0.55, math.asin(min(0.95, DROP / max(reach, 0.1))) + 0.1)
    spread = (0.12 if i % 2 else -0.1) * (1 if math.cos(heading) > 0 else -1)
    motion[(leg, 'location', 2)] = [(0, 0), (6, 0.03 * S), (18, -DROP), (22, -DROP + 0.04 * S), (26, -DROP), (DEATH, -DROP)]
    motion[(leg, 'rotation_euler', 1)] = [(0, 0), (6, 0.05), (18, -lift), (24, -lift * 0.9), (DEATH, -lift * 0.92)]
    motion[(leg, 'rotation_euler', 2)] = [(0, 0), (18, spread), (DEATH, spread)]
clip('death', DEATH, motion)

asset = {
    'id': 'asset.models.units.' + NAME, 'name': 'Supply Cart', 'kind': 'unit',
    'tags': ['beetles', 'machine', 'cart', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': [MAT.name]},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            {'semantic': 'walk', 'clip': 'walk', 'loop': True, 'events': [], 'nominalSpeed': round(SPEED, 3)},
            {'semantic': 'run', 'clip': 'walk', 'loop': True, 'events': [], 'nominalSpeed': round(SPEED, 3)},
            {'semantic': 'hit', 'clip': 'hit', 'loop': False, 'events': []},
            {'semantic': 'death', 'clip': 'death', 'loop': False, 'events': []},
        ],
        'sockets': [{'name': 'tanks', 'node': 'socket_tanks', 'offset': [0, 0, 0]},
                    {'name': 'beetle', 'node': 'socket_beetle', 'offset': [0, 0, 0]}],
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 2, 'character': 'base', 'healthHeight': round(HEIGHT + 0.4, 1)},
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) concept for '
               'Under the Canopy; grounded and animated by tools/units/supply_cart.py.',
}
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)
