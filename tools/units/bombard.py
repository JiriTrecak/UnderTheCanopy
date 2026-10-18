"""The beetles' Bombard from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/bombard.py -- \\
        <unit-bombard-segmented.glb>

The Bombard is Tripo's image-to-3D model of the chosen concept (the old repository's
.asset-work/concepts/buildings/machine-bombard-v2-1.png), split by Tripo's detailed
segmentation into parts that share one texture atlas. It is a rigid machine, animated by
moving whole parts as nodes (like the buildings, D65), never skinned. Here:
- Tripo made it with eight legs, which reads as a spider: the second pair is removed and the
  third pair moves forward to close the gap, so it walks on six (three per side);
- every leg is its own node pivoting at its hip, the hull carries the mortar and the
  crewman; the walk is an insect tripod gait (two alternating triplets) with a hull bob;
- the mortar barrel recoils down its own axis into the hull on `attack` (`release` event at
  the shot), the crewman ducks; `hit` jolts the hull; on `death` the hull drops, the legs
  splay and the mortar tips over;
- the royal-blue plate becomes the team mask in the atlas's alpha (violet glass excluded);
- the violet glass band of the barrel glows (constant emission: units have no activity);
- sockets: `socket_muzzle` at the barrel's mouth (rides the recoil), `socket_glass`;
- a second model, `geometry_2.glb`, is the lobbed shell with its smoke trail, drawn by the
  projectile effect `effect.units.bombard-shell`.

Blender is Z-up in metres; the unit faces -Y (glTF +Z, the asset transform's forward)."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
GLB = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-bombard'
OUT = os.path.join(ROOT, 'base', 'assets', 'units', NAME)
HEIGHT = 4.0  # the muzzle's top at rest: twice a beetle soldier (2.0 m), both shown at display scale 2
V = mathutils.Vector
ATLAS = 1024  # texture size

# Segmented parts by role (tripo_part_<n>; numbers hold for this one export). Positions below
# are in the inspection frame: 4 m across, centred, ground at 0, the head towards -X.
LEGS = {  # tip first; `l` legs are on +Y, `r` on -Y; 1 front, 3 middle, 4 rear
    'leg_l1': [62, 42, 50, 32, 15], 'leg_l3': [55, 5, 30, 31], 'leg_l4': [58, 40, 23, 49],
    'leg_r1': [66, 14, 51, 29], 'leg_r3': [65, 3, 46, 18], 'leg_r4': [59, 26, 7],
}
DROPPED = [33, 8, 63, 10, 34]  # the second pair: eight legs read as a spider
MIDDLE_SHIFT = -0.42  # the third pair moves this far forward (x) into the gap
MORTAR = [2, 11, 20, 28, 67, 68, 71, 73, 74, 37, 44]  # barrel, muzzle ring, glass, collars
GLASS = [28, 67, 68, 71]  # the violet glass band under the muzzle ring
CREW = [16, 21, 36, 39, 69, 70]  # the crewman in his seat (the seat, 0 and 45, is hull)

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

# --- Six legs: drop the second pair, move the third forward into its place.
for n in DROPPED:
    bpy.data.objects.remove(parts.pop(n), do_unlink=True)
for n in LEGS['leg_l3'] + LEGS['leg_r3']:
    parts[n].data.transform(mathutils.Matrix.Translation((MIDDLE_SHIFT, 0, 0)))


def join(numbers, name):
    objs = [parts.pop(n) for n in numbers]
    with bpy.context.temp_override(active_object=objs[0], object=objs[0], selected_objects=objs,
                                   selected_editable_objects=objs):
        bpy.ops.object.join()
    objs[0].name = objs[0].data.name = name
    return objs[0]


# The glass's own triangles, kept before the joins for the glow map.
glass_parts = [parts[n].copy() for n in GLASS]
for g in glass_parts:
    g.data = g.data.copy()
legs = {name: join(numbers, name) for name, numbers in LEGS.items()}
mortar = join(MORTAR, 'mortar')
crew = join(CREW, 'crewman')
hull = join(sorted(parts), 'hull')

# --- Final frame: the head to -Y, the muzzle's top at HEIGHT, centred on the footprint.
objs = [hull, mortar, crew, *legs.values()]
turn = mathutils.Matrix.Rotation(math.pi / 2, 4, 'Z')
for o in objs + glass_parts:
    o.data.transform(turn)
allv = [v.co for o in objs for v in o.data.vertices]
lo = V([min(v[i] for v in allv) for i in range(3)])
hi = V([max(v[i] for v in allv) for i in range(3)])
k = HEIGHT / (hi.z - lo.z)
place = mathutils.Matrix.Scale(k, 4) @ mathutils.Matrix.Translation((-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z))
for o in objs + glass_parts:
    o.data.transform(place)
S = k  # metres of the 4 m inspection frame -> metres now
print('size %.2f x %.2f x %.2f m' % ((hi.x - lo.x) * k, (hi.y - lo.y) * k, HEIGHT))


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


# Hull: its node at the hips' height over the footprint centre (bob, rock, drop).
hv = verts(hull)
HULL = V((0.0, float((hv[:, 1].min() + hv[:, 1].max()) / 2), 0.95 * S))
set_pivot(hull, HULL)

# Mortar: pivots at the base of the barrel on its axis; recoils along the axis.
mv = verts(mortar)
centre = mv.mean(axis=0)
_, _, vt = np.linalg.svd(mv - centre)
axis = V(vt[0]).normalized()
if axis.z < 0:
    axis = -axis
proj = (mv - centre) @ np.array(axis[:])
BASE = V(centre) + axis * float(proj.min() + 0.15 * (proj.max() - proj.min()))
MUZZLE = V(centre) + axis * float(proj.max())
set_pivot(mortar, BASE, hull)
print('mortar axis', [round(a, 2) for a in axis], 'elevation', round(math.degrees(math.asin(axis.z)), 1),
      'length', round(float(proj.max() - proj.min()), 2))

# Crewman: pivots at his seat.
cv = verts(crew)
SEAT = V((float(cv[:, 0].mean()), float(cv[:, 1].mean()), float(cv[:, 2].min())))
set_pivot(crew, SEAT, hull)

# Legs: each pivots at its hip (the end nearest the body's long axis), heading outwards.
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

# --- Team colour: the royal-blue plate into the atlas's alpha (violet glass is not team).
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
team = bpy.data.images.new('bombard-team', W, H, alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
MAT.name = 'TC_' + NAME
MAT['teamColorMask'] = 'baseColorAlpha'
print('team mask mean', round(float(mask.mean()), 4))


# --- The glass band glows: texels of the glass parts' UV islands that are violet.
def uv_mask(obj, size):
    out = np.zeros((size, size), np.float32)
    me = obj.data
    me.calc_loop_triangles()
    uvl = me.uv_layers.active.data
    for tri in me.loop_triangles:
        uv = np.array([uvl[i].uv[:] for i in tri.loops]) * size - 0.5
        (x0, y0), (x1, y1) = np.floor(uv.min(0)).astype(int), np.ceil(uv.max(0)).astype(int)
        xs, ys = np.meshgrid(np.arange(max(x0, 0), min(x1, size - 1) + 1), np.arange(max(y0, 0), min(y1, size - 1) + 1))
        (ax, ay), (bx, by), (cx, cy) = uv
        den = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
        if abs(den) < 1e-12:
            continue
        l0 = ((by - cy) * (xs - cx) + (cx - bx) * (ys - cy)) / den
        l1 = ((cy - ay) * (xs - cx) + (ax - cx) * (ys - cy)) / den
        l2 = 1 - l0 - l1
        on = (l0 > -0.05) & (l1 > -0.05) & (l2 > -0.05)
        out[ys, xs] = np.maximum(out[ys, xs], on.astype(np.float32))
    return out


G = 512
small = rgb.reshape(G, H // G, G, W // G, 3).mean(axis=(1, 3))
glass = np.zeros((G, G), np.float32)
for g in glass_parts:
    glass = np.maximum(glass, uv_mask(g, G))
others = np.zeros_like(glass)
for o in [hull, crew, *legs.values()]:
    others = np.maximum(others, uv_mask(o, G))
violet = np.clip((small[:, :, 2] - small[:, :, 1] - 0.05) / 0.12, 0, 1) * np.clip((small[:, :, 0] - small[:, :, 1]) / 0.06, 0, 1)
weight = glass * (1.0 - others) * np.clip(0.35 + violet, 0, 1)
print('glass texels', int((weight > 0.5).sum()))
glow = weight[..., None] * np.array((0.6, 0.36, 1.0), np.float32)
gimg = bpy.data.images.new('bombard-glass-glow', G, G, alpha=False)
gimg.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
gimg.pack()
tex = MAT.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = gimg
bsdf = MAT.node_tree.nodes['Principled BSDF']
MAT.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 3.0
for g in glass_parts:
    bpy.data.meshes.remove(g.data)
# A unit needs no 2048 atlas: 1024 keeps the file small and reads the same at game zoom.
for n in MAT.node_tree.nodes:
    if n.type == 'TEX_IMAGE' and n.image and n.image.size[0] > ATLAS:
        n.image.scale(ATLAS, ATLAS)
        n.image.pack()

# --- Sockets.
muzzle = kit.socket('socket_muzzle', (0, 0, 0))
muzzle.parent = mortar
muzzle.matrix_parent_inverse = mathutils.Matrix.Identity(4)
muzzle.location = MUZZLE - BASE + axis * 0.15 * S
glass_socket = kit.socket('socket_glass', (0, 0, 0))
glass_socket.parent = mortar
glass_socket.matrix_parent_inverse = mathutils.Matrix.Identity(4)
glass_socket.location = (MUZZLE - BASE) * 0.78
core = kit.socket('socket_hull', (0, 0, 0))
core.parent = hull
core.matrix_parent_inverse = mathutils.Matrix.Identity(4)
core.location = (0, 0, 0.3 * S)

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
MOVING = [hull, mortar, crew, *legs.values()]


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


# idle: the rest pose held still.
clip('idle', 2, {})

# walk (1 s loop): an insect tripod gait. Triplet A (left front, right middle, left rear)
# pushes back while triplet B swings forward lifted, then they swap; the hull bobs once per
# step and rolls a little towards the planted side.
WALK = 30
SWING, LIFT = 0.26, 0.32
A = {'leg_l1', 'leg_r3', 'leg_l4'}
motion = {}
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
        # the tip moves forward (-Y) by -reach * cos(heading) * dtheta
        zs.append((fr, -fwd * SWING * side))
        ys.append((fr, -lift * LIFT))
    motion[(leg, 'rotation_euler', 2)] = zs
    motion[(leg, 'rotation_euler', 1)] = ys
    motion[(leg, 'rotation_euler', 2, 'linear')] = True
    motion[(leg, 'rotation_euler', 1, 'linear')] = True
bob = 0.045 * S
motion[(hull, 'location', 2)] = [(fr, bob * 0.5 * (1 - math.cos(4 * math.pi * fr / WALK))) for fr in range(0, WALK + 1, 3)]
motion[(hull, 'rotation_euler', 1)] = [(fr, 0.025 * math.sin(2 * math.pi * fr / WALK)) for fr in range(0, WALK + 1, 3)]
motion[(hull, 'location', 2, 'linear')] = True
motion[(hull, 'rotation_euler', 1, 'linear')] = True
motion[(crew, 'rotation_euler', 0)] = [(fr, 0.04 * math.sin(4 * math.pi * fr / WALK + 0.8)) for fr in range(0, WALK + 1, 3)]
motion[(crew, 'rotation_euler', 0, 'linear')] = True
clip('walk', WALK, motion)

# attack (1.2 s): the crewman ducks, the mortar fires at frame 10 (`release`): the barrel
# slams down its own axis into the hull, the hull squats and rocks back, then the barrel
# eases back out and the crewman comes up.
ATTACK, FIRE = 36, 10
RECOIL = 0.42 * S
back = -axis * RECOIL
motion = {
    (mortar, 'location', 1): [(0, 0), (FIRE, 0), (FIRE + 2, back.y), (FIRE + 6, back.y * 0.9), (28, 0), (ATTACK, 0)],
    (mortar, 'location', 2): [(0, 0), (FIRE, 0), (FIRE + 2, back.z), (FIRE + 6, back.z * 0.9), (28, 0), (ATTACK, 0)],
    (mortar, 'rotation_euler', 0): [(0, 0), (FIRE - 3, -0.03), (FIRE, -0.03), (FIRE + 2, 0.09), (22, -0.02), (30, 0), (ATTACK, 0)],
    (hull, 'location', 2): [(0, 0), (FIRE, 0), (FIRE + 2, -0.07 * S), (20, 0.01 * S), (26, 0), (ATTACK, 0)],
    (hull, 'rotation_euler', 0): [(0, 0), (FIRE, 0), (FIRE + 2, -0.07), (18, 0.02), (24, 0), (ATTACK, 0)],
    (crew, 'location', 2): [(0, 0), (6, -0.14 * S), (FIRE + 8, -0.14 * S), (30, 0), (ATTACK, 0)],
    (crew, 'rotation_euler', 0): [(0, 0), (6, 0.42), (FIRE + 1, 0.5), (FIRE + 8, 0.4), (30, 0), (ATTACK, 0)],
}
for name, leg in legs.items():  # the legs flex under the kick (tips lift a hair, never sink)
    motion[(leg, 'rotation_euler', 1)] = [(0, 0), (FIRE, 0), (FIRE + 2, -0.04), (24, 0), (ATTACK, 0)]
clip('attack', ATTACK, motion)

# hit (0.4 s): the hull jolts sideways, the crewman flinches, the barrel wobbles.
HIT = 12
motion = {
    (hull, 'rotation_euler', 1): [(0, 0), (3, 0.08), (7, -0.03), (HIT, 0)],
    (hull, 'location', 2): [(0, 0), (3, -0.05 * S), (HIT, 0)],
    (crew, 'rotation_euler', 0): [(0, 0), (3, -0.3), (HIT, 0)],
    (mortar, 'rotation_euler', 1): [(0, 0), (4, -0.06), (8, 0.03), (HIT, 0)],
}
clip('hit', HIT, motion)

# death (1.5 s, held): the legs give way and splay, the hull drops onto its belly and lists,
# the mortar tips over sideways and the crewman slumps.
DEATH = 45
belly = float(verts(hull)[:, 2].min()) + HULL.z
DROP = belly - 0.16 * S  # leaves room for the list, so no edge sinks
motion = {
    (hull, 'location', 2): [(0, 0), (6, 0.04 * S), (18, -DROP), (22, -DROP + 0.05 * S), (26, -DROP), (DEATH, -DROP)],
    (hull, 'rotation_euler', 1): [(0, 0), (18, -0.12), (24, -0.1), (DEATH, -0.1)],
    (hull, 'rotation_euler', 0): [(0, 0), (18, 0.08), (24, 0.06), (DEATH, 0.06)],
    (mortar, 'rotation_euler', 1): [(0, 0), (16, 0), (30, -0.58), (34, -0.52), (38, -0.56), (DEATH, -0.56)],
    (mortar, 'rotation_euler', 0): [(0, 0), (16, 0), (30, 0.3), (DEATH, 0.28)],
    (mortar, 'location', 2): [(0, 0), (16, 0), (30, -0.15 * S), (DEATH, -0.15 * S)],
    (crew, 'rotation_euler', 0): [(0, 0), (10, -0.2), (24, 0.55), (DEATH, 0.55)],
    (crew, 'location', 2): [(0, 0), (24, -0.12 * S), (DEATH, -0.12 * S)],
}
for i, (name, leg) in enumerate(legs.items()):
    hip, tip, heading, reach = leg_info[name]
    lift = min(0.55, math.asin(min(0.95, DROP / max(reach, 0.1))) + 0.1)
    spread = (0.12 if i % 2 else -0.1) * (1 if math.cos(heading) > 0 else -1)
    motion[(leg, 'location', 2)] = [(0, 0), (6, 0.04 * S), (18, -DROP), (22, -DROP + 0.05 * S), (26, -DROP), (DEATH, -DROP)]
    motion[(leg, 'rotation_euler', 1)] = [(0, 0), (6, 0.05), (18, -lift), (24, -lift * 0.9), (DEATH, -lift * 0.92)]
    motion[(leg, 'rotation_euler', 2)] = [(0, 0), (18, spread), (DEATH, spread)]
clip('death', DEATH, motion)

asset = {
    'id': 'asset.models.units.' + NAME, 'name': 'Bombard', 'kind': 'unit',
    'tags': ['beetles', 'machine', 'siege', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'geometry', 'index': 2, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': [MAT.name]},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            {'semantic': 'walk', 'clip': 'walk', 'loop': True, 'events': []},
            {'semantic': 'run', 'clip': 'walk', 'loop': True, 'events': []},
            # Event times are seconds into the clip.
            {'semantic': 'attack', 'clip': 'attack', 'loop': False,
             'events': [{'name': 'release', 'time': round(FIRE / kit.FPS, 3)}]},
            {'semantic': 'hit', 'clip': 'hit', 'loop': False, 'events': []},
            {'semantic': 'death', 'clip': 'death', 'loop': False, 'events': []},
        ],
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 2, 'character': 'base', 'healthHeight': round(HEIGHT + 0.4, 1)},
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) concept for '
               'Under the Canopy; made six-legged and animated by tools/units/bombard.py, which also '
               'builds the shell (geometry_2).',
}
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)


# --- The shell (geometry_2): a brass-nosed iron shell with a glowing violet core band, a
# hot streak and a smoke trail behind it. Its nose points +Z (glTF +Y: the flight tangent);
# metres at a definition modelScale of 1.
kit.reset()


def image(name, rgba):
    h, w = rgba.shape[:2]
    img = bpy.data.images.new(name, w, h, alpha=True)
    img.pixels[:] = rgba.astype(np.float32).ravel()
    img.alpha_mode = 'STRAIGHT'
    img.pack()
    return img


def lathe(name, profile, segments=16, v_along=True):
    """A surface of revolution about Z from (radius, z) points; U around, V along."""
    bm = bmesh.new()
    uvl = bm.loops.layers.uv.new()
    rings = []
    for r, z in profile:
        rings.append([bm.verts.new((r * math.cos(2 * math.pi * i / segments), r * math.sin(2 * math.pi * i / segments), z))
                      for i in range(segments)])
    zs = [z for _, z in profile]
    z0, z1 = min(zs), max(zs)
    for j in range(len(rings) - 1):
        for i in range(segments):
            a, b = rings[j], rings[j + 1]
            face = bm.faces.new([a[i], a[(i + 1) % segments], b[(i + 1) % segments], b[i]])
            for loop, (ii, jj) in zip(face.loops, [(i, j), (i + 1, j), (i + 1, j + 1), (i, j + 1)]):
                v = (profile[jj][1] - z0) / (z1 - z0) if v_along else jj / (len(rings) - 1)
                loop[uvl].uv = (ii / segments, v)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = bpy.data.objects.new(name, me)
    bpy.context.scene.collection.objects.link(obj)
    for p in me.polygons:
        p.use_smooth = True
    return obj


def flat(name, color, emission=None, strength=0.0, metallic=0.0, roughness=0.5):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    b = mat.node_tree.nodes['Principled BSDF']
    b.inputs['Base Color'].default_value = (*color, 1)
    b.inputs['Metallic'].default_value = metallic
    b.inputs['Roughness'].default_value = roughness
    if emission is not None:
        b.inputs['Emission Color'].default_value = (*emission, 1)
        b.inputs['Emission Strength'].default_value = strength
    return mat


def blended(name, img, emission=False, strength=1.0):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    b = nt.nodes['Principled BSDF']
    t = nt.nodes.new('ShaderNodeTexImage')
    t.image = img
    nt.links.new(t.outputs['Color'], b.inputs['Base Color'])
    nt.links.new(t.outputs['Alpha'], b.inputs['Alpha'])
    b.inputs['Roughness'].default_value = 1.0
    if emission:
        nt.links.new(t.outputs['Color'], b.inputs['Emission Color'])
        b.inputs['Emission Strength'].default_value = strength
    mat.surface_render_method = 'BLENDED'
    mat.use_backface_culling = False
    return mat


R = 0.3  # shell radius
iron = flat('shell-iron', (0.06, 0.06, 0.07), metallic=0.7, roughness=0.4)
brass = flat('shell-brass', (0.78, 0.55, 0.22), metallic=0.9, roughness=0.3)
core = flat('shell-core', (0.6, 0.36, 1.0), emission=(0.62, 0.38, 1.0), strength=6.0)
body = lathe('shell-body', [(0.0, -0.42), (R * 0.75, -0.42), (R, -0.34), (R, -0.12)])
band = lathe('shell-band', [(R * 0.98, -0.16), (R * 1.1, -0.12), (R * 1.12, 0.0), (R * 1.1, 0.1), (R * 0.98, 0.14)])
nose = lathe('shell-nose', [(R, 0.1), (R * 0.95, 0.22), (R * 0.75, 0.36), (R * 0.4, 0.47), (0.0, 0.52)])
fins = lathe('shell-tail', [(R * 0.55, -0.62), (R * 0.75, -0.42)])
for o, m in ((body, iron), (band, core), (nose, brass), (fins, brass)):
    o.data.materials.append(m)

# The hot streak: violet-white at the shell, fading to orange and nothing.
n = 128
u, v = np.meshgrid(np.linspace(0, 1, 32), np.linspace(0, 1, n))  # v = 0 at the far end
heat = v ** 1.6
streak_rgb = np.dstack([1.0 - 0.4 * heat, 0.45 - 0.05 * heat, 0.15 + 0.85 * heat])  # orange tail, violet at the shell
streak = image('shell-streak', np.dstack([streak_rgb, np.clip(v ** 1.5, 0, 1) * 0.75]))
hot = lathe('shell-hot', [(0.0, -2.4), (R * 0.3, -1.5), (R * 0.5, -0.8), (R * 0.6, -0.5)], segments=12)
hot.data.materials.append(blended('shell-streak', streak, emission=True, strength=4.0))

# The smoke trail: a widening grey plume with puffy edges, fading out behind.
rng = np.random.default_rng(7)
n = 128
u, v = np.meshgrid(np.linspace(0, 1, 64, endpoint=False), np.linspace(0, 1, n))
puff = np.zeros_like(u)
for kk in (2, 3, 5, 8):
    ph = rng.uniform(0, 2 * np.pi, 2)
    puff += np.sin(2 * np.pi * (u * kk + ph[0]) + np.sin(2 * np.pi * v * kk * 1.5 + ph[1])) / kk
puff = (puff - puff.min()) / (puff.max() - puff.min())
smoke_a = np.clip(v ** 1.6 * (0.3 + 0.7 * puff) * 0.75, 0, 1) * np.clip((1 - v) / 0.25, 0, 1)  # thin at the shell, gone behind
grey = 0.42 + 0.2 * puff
smoke = image('shell-smoke', np.dstack([grey, grey * 0.97, grey * 0.92, smoke_a]))
trail = lathe('shell-smoke', [(R * 2.6, -4.6), (R * 2.1, -3.2), (R * 1.5, -1.8), (R * 1.0, -0.8), (R * 0.8, -0.45)], segments=14)
trail.data.materials.append(blended('shell-smoke', smoke))

shell_tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
print('shell triangles', shell_tris)
bpy.ops.export_scene.gltf(
    filepath=os.path.join(OUT, 'geometry_2.glb'), export_format='GLB', export_yup=True, export_apply=True,
    export_image_format='AUTO', export_extras=True, export_animations=False)
print('wrote', OUT)
