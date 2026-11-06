"""Rigs and animates a many-legged creature from a static Tripo model (no skeleton needed):

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/legged_rig.py -- \\
        <tripo.glb> <out.glb> [--legs 4|6|8] [--attack spit|bite] [--speed 1.0]
        [--head x,y,z] [--back x,y,z] [--mouth x,y,z] [--feet x,y;...] [--root-spread 0.07]

Tripo's own "Other" rig is unreliable for creatures (a four-legged spitter came back with a
biped skeleton), so the skeleton is built here from the mesh, the way tools/units/ant_centaur.py
finds a centaur's legs:
- the feet are where the mesh touches the ground: clusters of the lowest vertices, merged when
  they are near along the mesh (one foot's claws), the `--legs` farthest from the centre; or
  `--feet` names them (x,y each). Mandibles and claws that hang above the ground are not feet;
- each leg is followed up from its foot by geodesic distance until the rings of equal distance
  spread into the body (`--root-spread`, a share of the height): its root, knee and tip;
- bones: `Root` on the ground, `Body` at the legs' roots, `Head` towards the front (`--head`,
  default the centre of the front-most low vertices) and `Back` up to the mass on top
  (`--back`, default the centre of the top quarter: a sac, an abdomen, a shell); two per leg
  (`Leg<n>_Upper`, `Leg<n>_Lower`). Legs skin by geodesic distance (blended at the knee, faded
  into the body at the root); the rest by geodesic nearness to Body, Head and Back;
- `socket_projectile` sits at `--mouth` (default the front-most point of the Back mass) on
  `Back`, where a ranged attack leaves;
- clips (front is -Y, up +Z): `walk` and `run` with alternating legs (a trot for four, a
  tripod for six: neighbours along a side and across the body step apart), `idle` (breathing,
  the back swaying), `attack` (`spit`: the back rears and thrusts forward, the release at 45%;
  `bite`: the head lunges), `hit` (a recoil), `cast` (rearing up) and `death` (legs curl, the
  body sinks and rolls; held at the end). `--speed` scales the gait (heavier creatures below 1).

The output goes on to tools/units/tripo_unit.py (scale, ground, team mask, export) with clips
named as here and `"trimAttack": false`, `"attackEvent": {"name": "release", "share": 0.45}`.

The ants' Hunter (spitter, four legs, mandibles in front): `hunter.glb out.glb --legs 4 --attack spit`."""
import heapq, math, sys

import bpy
import mathutils
import numpy as np

ARGS = sys.argv[sys.argv.index('--') + 1:]
SRC, OUT = ARGS[0], ARGS[1]


def arg(flag, default=None, kind=str):
    return kind(ARGS[ARGS.index(flag) + 1]) if flag in ARGS else default


def point(flag):
    return np.array([float(c) for c in ARGS[ARGS.index(flag) + 1].split(',')]) if flag in ARGS else None


NLEGS = arg('--legs', 6, int)
ATTACK = arg('--attack', 'bite')
SPEED = arg('--speed', 1.0, float)
SPREAD = arg('--root-spread', 0.07, float)
FEET = [tuple(float(c) for c in p.split(',')) for p in arg('--feet', '').split(';') if p]
FPS = 30
X, Y, Z = mathutils.Vector((1, 0, 0)), mathutils.Vector((0, 1, 0)), mathutils.Vector((0, 0, 1))

bpy.ops.wm.read_factory_settings(use_empty=True)
scene = bpy.context.scene
scene.render.fps = FPS
bpy.ops.import_scene.gltf(filepath=SRC)
# Any skeleton Tripo added goes: the mesh keeps only its shape and material.
for o in [o for o in scene.objects if o.type == 'MESH']:
    o.modifiers.clear()
    w = o.matrix_world.copy()
    o.parent = None
    o.matrix_world = w
    o.vertex_groups.clear()
for o in [o for o in scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
for a in list(bpy.data.actions):
    bpy.data.actions.remove(a)
meshes = [o for o in scene.objects if o.type == 'MESH']
bpy.ops.object.select_all(action='DESELECT')
for o in meshes:
    o.select_set(True)
bpy.context.view_layer.objects.active = meshes[0]
if len(meshes) > 1:
    bpy.ops.object.join()
body_mesh = bpy.context.view_layer.objects.active
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# --- The welded mesh: Tripo splits vertices along UV seams; geodesics need them joined.
co = np.array([v.co[:] for v in body_mesh.data.vertices])
keys = {}
weld = np.array([keys.setdefault(tuple(np.round(c, 5)), len(keys)) for c in co])
n = len(keys)
wco = np.zeros((n, 3))
wco[weld] = co
adj = [dict() for _ in range(n)]
for e in body_mesh.data.edges:
    a, b = weld[e.vertices[0]], weld[e.vertices[1]]
    if a != b:
        d = float(np.linalg.norm(wco[a] - wco[b]))
        adj[a][b] = adj[b][a] = d


def geodesic(sources):
    dist = np.full(n, np.inf)
    heap = [(0.0, s) for s in sources]
    for s in sources:
        dist[s] = 0.0
    while heap:
        d, v = heapq.heappop(heap)
        if d > dist[v]:
            continue
        for w, l in adj[v].items():
            if d + l < dist[w]:
                dist[w] = d + l
                heapq.heappush(heap, (d + l, w))
    return dist


# --- Feet.
ground = wco[:, 2].min()
height = wco[:, 2].max() - ground
centre = wco[:, :2].mean(axis=0)
if FEET:
    feet = []
    for p in FEET:
        near_p = np.nonzero(np.linalg.norm(wco[:, :2] - np.array(p), axis=1) < 0.06 * height)[0]
        floor = wco[near_p, 2].min()
        feet.append(near_p[wco[near_p, 2] < floor + 0.03 * height])
else:
    low = np.nonzero(wco[:, 2] < ground + 0.04 * height)[0]
    parent = {i: i for i in low}

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i

    for i in range(len(low)):
        for j in range(i + 1, len(low)):
            if np.linalg.norm(wco[low[i], :2] - wco[low[j], :2]) < 0.04 * height:
                parent[find(low[i])] = find(low[j])
    groups = {}
    for i in low:
        groups.setdefault(find(i), []).append(i)
    clusters = list(groups.values())
    near = [geodesic(c) for c in clusters]
    merged = list(range(len(clusters)))
    for i in range(len(clusters)):
        for j in range(i + 1, len(clusters)):
            if near[i][clusters[j]].min() < 0.3 * height:
                old, new = merged[j], merged[i]
                merged = [new if m == old else m for m in merged]
    clusters = [np.array(sum((clusters[i] for i in range(len(clusters)) if merged[i] == m), [])) for m in sorted(set(merged))]
    for c in clusters:
        print('ground contact at', np.round(wco[c, :2].mean(axis=0), 3), len(c), 'vertices')
    feet = sorted(clusters, key=lambda c: -np.linalg.norm(wco[c, :2].mean(axis=0) - centre))[:NLEGS]
assert len(feet) == NLEGS, 'found %d feet, expected %d (name them with --feet)' % (len(feet), NLEGS)

# --- Each leg: rings of equal geodesic distance from the foot, up to where they spread.
STEP = 0.015 * height
legs = []
for foot in feet:
    dist = geodesic(list(foot))
    rings = []
    for k in range(1, 90):
        ring = np.nonzero((dist >= (k - 1) * STEP) & (dist < k * STEP))[0]
        if not len(ring):
            continue
        c = wco[ring].mean(axis=0)
        r = float(np.median(np.linalg.norm(wco[ring] - c, axis=1)))
        rings.append((k * STEP, c, r))
        if k > 4 and r > SPREAD * height:
            break
    reach, root, _ = rings[-2]
    tip = wco[foot].mean(axis=0)
    tip[2] = wco[foot, 2].min()
    line = (root - tip) / np.linalg.norm(root - tip)
    off = [np.linalg.norm((c - tip) - np.dot(c - tip, line) * line) for _, c, _ in rings[:-1]]
    bend = int(np.argmax(off))
    if off[bend] < 0.03 * height:
        bend = len(off) // 2
    knee_at, knee, _ = rings[bend]
    legs.append({'dist': dist, 'reach': reach, 'root': root, 'knee': knee, 'tip': tip, 'knee_at': knee_at})
    print('leg foot', np.round(tip, 3), 'knee', np.round(knee, 3), 'root', np.round(root, 3), 'reach', round(reach, 3))

# Sides (+X is one) and order front (-Y) to back; neighbours along a side and across step apart.
for side in (1, -1):
    mine = sorted([l for l in legs if np.sign(l['tip'][0] - centre[0]) == side], key=lambda l: l['tip'][1])
    assert len(mine) == NLEGS // 2, 'expected %d legs a side, got %d on %+d' % (NLEGS // 2, len(mine), side)
    for i, l in enumerate(mine):
        l['side'], l['pair'] = side, i
        l['phase'] = 0.5 if (i % 2 == 0) == (side > 0) else 0.0
legs.sort(key=lambda l: (l['pair'], -l['side']))

# --- Landmarks.
hub = np.mean([l['root'] for l in legs], axis=0)
leg_any = np.zeros(n, bool)
for l in legs:
    leg_any |= l['dist'] < l['reach']
rest = ~leg_any
top_cut = ground + 0.75 * height
back = point('--back') if '--back' in ARGS else wco[rest & (wco[:, 2] > top_cut)].mean(axis=0)
if '--head' in ARGS:
    head = point('--head')
else:
    lowish = rest & (wco[:, 2] < hub[2] + 0.15 * height)
    front = wco[lowish, 1].min()
    head = wco[lowish & (wco[:, 1] < front + 0.12 * height)].mean(axis=0)
upper = rest & (wco[:, 2] > hub[2] + 0.2 * height)
mouth = point('--mouth') if '--mouth' in ARGS else wco[upper][np.argmin(wco[upper, 1])]
print('hub', np.round(hub, 3), 'head', np.round(head, 3), 'back', np.round(back, 3), 'mouth', np.round(mouth, 3),
      'height', round(height, 3))

# --- Bones.
arm_data = bpy.data.armatures.new('Armature')
rig = bpy.data.objects.new('Armature', arm_data)
scene.collection.objects.link(rig)
bpy.context.view_layer.objects.active = rig
bpy.ops.object.mode_set(mode='EDIT')
eb = arm_data.edit_bones


def bone(name, a, b, parent=None, connect=False):
    e = eb.new(name)
    e.head, e.tail = mathutils.Vector(a), mathutils.Vector(b)
    e.parent, e.use_connect = parent, connect
    return e


root_b = bone('Root', (centre[0], centre[1], ground), (centre[0], centre[1], ground + 0.1 * height))
body_b = bone('Body', hub, hub + np.array([0, 0, 0.1 * height]), root_b)
bone('Head', hub + (head - hub) * 0.4, head, body_b)
bone('Back', hub + np.array([0, 0, 0.05 * height]), back, body_b)
for i, l in enumerate(legs):
    up = bone('Leg%d_Upper' % i, l['root'], l['knee'], body_b)
    bone('Leg%d_Lower' % i, l['knee'], l['tip'], up, True)
    l['upper'], l['lower'] = 'Leg%d_Upper' % i, 'Leg%d_Lower' % i
bpy.ops.object.mode_set(mode='OBJECT')
sock = bpy.data.objects.new('socket_projectile', None)
scene.collection.objects.link(sock)
sock.parent, sock.parent_type, sock.parent_bone = rig, 'BONE', 'Back'
sock.matrix_world = mathutils.Matrix.Translation(mathutils.Vector(mouth))

# --- Skin.
BAND = 0.025 * height
leg_w = []
for l in legs:
    d = l['dist']
    fade = np.clip((l['reach'] + 0.02 * height - d) / (0.06 * height), 0.0, 1.0)
    lower = np.clip((l['knee_at'] + BAND - d) / (2 * BAND), 0.0, 1.0)
    leg_w.append((fade * (1 - lower), fade * lower))
total = sum(u + w for u, w in leg_w)
scale = np.where(total > 1, 1 / np.maximum(total, 1e-9), 1.0)
total = np.minimum(total, 1.0)
near_bone = {}
for name in ('Body', 'Head', 'Back'):
    b = arm_data.bones[name]
    h, t = np.array(b.head_local), np.array(b.tail_local)
    seg = t - h
    u = np.clip(((wco - h) @ seg) / max(seg @ seg, 1e-9), 0, 1)
    d = np.linalg.norm(wco - (h + u[:, None] * seg), axis=1)
    seeds = np.argsort(d)[:max(8, int((d < 0.04 * height).sum()))]
    near_bone[name] = geodesic(list(seeds))
names = list(near_bone)
dist = np.stack([near_bone[k] for k in names])
SIGMA = 0.03 * height
core = np.exp(-(dist - dist.min(axis=0)) / SIGMA)
core[dist - dist.min(axis=0) > 4 * SIGMA] = 0
core /= core.sum(axis=0)
vg = {k: body_mesh.vertex_groups.new(name=k) for k in names}
lg = [(body_mesh.vertex_groups.new(name=l['upper']), body_mesh.vertex_groups.new(name=l['lower'])) for l in legs]
for v in body_mesh.data.vertices:
    i = weld[v.index]
    t = float(total[i])
    for k, name in enumerate(names):
        w = (1 - t) * float(core[k, i])
        if w > 1e-4:
            vg[name].add([v.index], w, 'REPLACE')
    for (gu, gw), (u, w) in zip(lg, leg_w):
        if u[i] * scale[i] > 1e-4:
            gu.add([v.index], float(u[i] * scale[i]), 'REPLACE')
        if w[i] * scale[i] > 1e-4:
            gw.add([v.index], float(w[i] * scale[i]), 'REPLACE')
body_mesh.parent = rig
mod = body_mesh.modifiers.new('Armature', 'ARMATURE')
mod.object = rig
print('skinned', int((total > 0.5).sum()), 'of', n, 'welded vertices to the legs')

# --- Clips.
bones = rig.pose.bones
to_arm = rig.matrix_world.inverted()


def local_rot(pb, axis_world, degrees):
    axis = (to_arm.to_3x3() @ axis_world).normalized()
    q = mathutils.Quaternion(axis, math.radians(degrees))
    rest_q = pb.bone.matrix_local.to_3x3().to_quaternion()
    return rest_q.inverted() @ q @ rest_q


def leg_axes(l):
    out = mathutils.Vector(l['knee'] - l['root'])
    out.z = 0
    out.normalize()
    return out, out.cross(Z)


def ease(t):
    return 0.5 - 0.5 * math.cos(math.pi * max(0.0, min(1.0, t)))


def key(action, frame, pose):
    rig.animation_data_create()
    rig.animation_data.action = action
    for pb in bones:
        pb.rotation_mode = 'QUATERNION'
        q = mathutils.Quaternion()
        for axis, deg in pose.get(pb.name, []):
            q = local_rot(pb, axis, deg) @ q
        pb.rotation_quaternion = q
        pb.keyframe_insert('rotation_quaternion', frame=frame, group=pb.name)
        off = pose.get(('loc', pb.name))
        pb.location = (pb.bone.matrix_local.to_3x3().inverted() @ off) if off else mathutils.Vector()
        pb.keyframe_insert('location', frame=frame, group=pb.name)


def clip(name, length, pose_at, step=1):
    action = bpy.data.actions.new(name)
    for f in list(range(0, length, step)) + [length]:
        key(action, f, pose_at(f / length))
    action.use_fake_user = True
    return action


def legs_at(p, swing, lift, fold):
    out = {}
    for l in legs:
        q = (p + l['phase']) % 1.0
        if q < 0.5:
            fwd, up = swing * (1 - 4 * q), 0.0
        else:
            s = (q - 0.5) / 0.5
            fwd, up = -swing + 2 * swing * ease(s), math.sin(math.pi * s)
        _, lift_axis = leg_axes(l)
        out[l['upper']] = [(Z, -fwd * l['side']), (lift_axis, lift * up)]
        out[l['lower']] = [(lift_axis, -fold * up)]
    return out


def walking(swing, lift, fold, bob, sway):
    def pose(t):
        p = legs_at(2 * t, swing, lift, fold)
        w = math.sin(4 * math.pi * t)
        p[('loc', 'Body')] = Z * (bob * height * abs(w))
        p['Back'] = [(Y, sway * math.sin(2 * math.pi * t)), (X, 0.5 * sway * w)]
        p['Head'] = [(Z, -0.6 * sway * math.sin(2 * math.pi * t))]
        return p
    return pose


def idle(t):
    s = math.sin(2 * math.pi * t)
    c = math.sin(2 * math.pi * (t + 0.25))
    return {'Back': [(X, 2.5 * s), (Y, 1.5 * c)], 'Head': [(Z, 5 * c), (X, 2 * s)],
            ('loc', 'Body'): Z * (0.006 * height * s)}


def attack(t):
    """Wind up to 40%, strike at 45%, recover."""
    wind = ease(t / 0.4) * (1 - ease((t - 0.4) / 0.08))
    strike = ease((t - 0.4) / 0.08) * (1 - ease((t - 0.55) / 0.45))
    p = {}
    if ATTACK == 'spit':  # the back rears, then thrusts its mouth at the target
        p['Back'] = [(X, 18 * wind - 22 * strike)]
        p['Head'] = [(X, 8 * wind - 6 * strike)]
        p[('loc', 'Body')] = -Z * (0.03 * height * wind) + Y * (0.04 * height * wind) - Y * (0.05 * height * strike)
    else:  # the head lunges and bites
        p['Head'] = [(X, 12 * wind - 25 * strike)]
        p['Back'] = [(X, 4 * wind - 5 * strike)]
        p[('loc', 'Body')] = Y * (0.04 * height * wind) - Y * (0.08 * height * strike)
    for l in legs:  # brace: the legs bend a little under the thrust
        _, lift_axis = leg_axes(l)
        p[l['lower']] = [(lift_axis, 6 * (wind + strike))]
    return p


def hit(t):
    k = math.sin(math.pi * min(1.0, t / 0.6)) if t < 0.6 else 0.0
    return {'Body': [(X, -5 * k)], 'Back': [(X, -8 * k)], ('loc', 'Body'): Y * (0.03 * height * k)}


def cast(t):
    k = ease(t / 0.4) * (1 - ease((t - 0.75) / 0.25))
    p = {'Body': [(X, -14 * k)], 'Back': [(X, -10 * k)], 'Head': [(X, -10 * k)], ('loc', 'Body'): Z * (0.04 * height * k)}
    for l in legs:
        if l['pair'] == 0:
            _, lift_axis = leg_axes(l)
            p[l['upper']] = [(lift_axis, 25 * k)]
    return p


def death(t):
    k = ease(t / 0.7)
    curl = ease((t - 0.1) / 0.6)
    p = {'Body': [(Y, 70 * k)], ('loc', 'Body'): -Z * (0.6 * float(hub[2] - ground) * k), 'Back': [(X, 10 * k)]}
    for l in legs:
        _, lift_axis = leg_axes(l)
        p[l['upper']] = [(lift_axis, 35 * curl)]
        p[l['lower']] = [(lift_axis, -75 * curl)]
    return p


clip('walk', int(40 / SPEED), walking(14, 18, 14, 0.008, 3))
clip('run', int(24 / SPEED), walking(20, 26, 20, 0.014, 5))
clip('idle', 60, idle, step=2)
clip('attack', 36, attack)
clip('hit', 18, hit, step=2)
clip('cast', 45, cast, step=2)
clip('death', 40, death, step=2)
rig.animation_data.action = bpy.data.actions['idle']
for pb in bones:
    pb.matrix_basis = mathutils.Matrix()
print('clips', [(a.name, tuple(round(v) for v in a.frame_range)) for a in bpy.data.actions])
bpy.ops.export_scene.gltf(filepath=OUT, export_format='GLB', export_animations=True, export_animation_mode='ACTIONS',
                          export_force_sampling=True, export_yup=True, export_apply=False)
print('wrote', OUT)
