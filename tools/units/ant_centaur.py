"""Six walking legs for a Tripo ant centaur (an upright torso with arms on an ant body):

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/ant_centaur.py -- \\
        <tripo.glb> <out.glb> [--ignore x,y;...] [--feet x,y;...] [--root-spread 0.07]
        [--upper hip;neck;crown;right shoulder;right hand;left shoulder;left hand] [--report]

Tripo rigs a centaur as a biped whatever rig type is chosen: two of its six legs (and often the
abdomen, or a leg taken for an arm) hang on the humanoid's thighs and calves. Its humanoid rig
and Mixamo presets (walk, run, slash, hit, cast) still suit the torso, so they stay; this
script replaces the lower body:
- the feet are where the mesh touches the ground: clusters of the lowest vertices, merged when
  they are near along the mesh (one foot's claws), the six farthest from the body's centre.
  `--ignore` drops a contact near x,y (a staff's foot); `--feet` names the six outright, each
  the lowest vertices near its point, so a foot may hang above the ground;
- each leg is followed up from its foot by geodesic distance over the welded mesh: the rings of
  equal distance stay narrow along the leg and spread where it joins the body, which marks the
  leg's root (`--root-spread`, the ring radius that counts as body). The rings' centres give the
  root, the knee (the bend farthest from the root-foot line) and the foot;
- new bones: `Body` (under Tripo's root, at the legs' roots) and per leg `Leg<n>_Upper`,
  `Leg<n>_Lower`. The leg's vertices are skinned to them by geodesic distance, blended at the
  knee and faded into the body at the root; every other lower-body weight (hips, thighs,
  calves, waist) moves to `Body`, so only the torso, arms and head keep Tripo's skin;
- the clips: the root's turn moves into the hips, the biped hips are damped to a third and lose
  their bob, the arms swing half as far in walk and run (a staff in hand would sweep the legs).
  Legs and body are keyed in every clip: `walk` and `run` get a tripod gait (front and back
  legs of one side step with the middle leg of the other), the rest stand still. `idle`
  (breathing torso, head looking about) is added, and so is `death` (legs curl, the body sinks
  and tips, the torso slumps; held at the end); `hit` is added when Tripo's export has none,
  and `fall` goes.
When Tripo's skeleton misses the torso altogether (the Gardener's sat inside its basket),
`--upper` replaces it by `Torso`, `Head` and two-bone arms at the given points (x,y,z each),
skinned by geodesic nearness (an arm keeps only what is clearly its own: sleeve, hand, what it
holds), and every clip is keyed here: the gait with a swaying torso, a staff drawn back and
flung (`slash`), a staff raised (`cast_a_spell`), a recoil (`hit_to_body_01`).
Front is -Y, up +Z, as Tripo exports. The output goes on to tools/units/tripo_unit.py like any
Tripo character (scale, ground, team mask, in place; clips idle, walk, run, attack: slash,
hit: hit_to_body_01, death, cast: cast_a_spell; height 1, `keepViolet`).

The ants' centaurs were made with (Tripo exports of walk, run, slash, hit_to_body_01, cast_a_spell):
    herald.glb --ignore "-0.271,-0.148"   (the staff's foot)
    royal-alchemist.glb   (Oleander as reworked on 2026-10-22, one flask and a short robe: the
        six feet are found unaided; the reference must show six separate legs, or Tripo makes four)
    gardener.glb --feet "-0.40,0.30;0.41,0.30;-0.386,-0.061;0.376,0.022;-0.166,-0.147;0.153,-0.141"
        --upper "0,0.0,0.40;0,-0.07,0.55;0,-0.06,0.72;-0.13,-0.06,0.52;-0.23,-0.15,0.44;0.13,-0.06,0.52;0.17,-0.08,0.31"
"""
import heapq, math, sys

import bpy
import mathutils
import numpy as np

ARGS = sys.argv[sys.argv.index('--') + 1:]
SRC, OUT = ARGS[0], ARGS[1]


def points(flag):
    if flag not in ARGS:
        return []
    return [tuple(float(c) for c in p.split(',')) for p in ARGS[ARGS.index(flag) + 1].split(';') if p]


IGNORE = points('--ignore')
FEET = points('--feet')
UPPER = points('--upper')
SPREAD = float(ARGS[ARGS.index('--root-spread') + 1]) if '--root-spread' in ARGS else 0.07
REPORT = '--report' in ARGS
FPS = 30
X, Y, Z = mathutils.Vector((1, 0, 0)), mathutils.Vector((0, 1, 0)), mathutils.Vector((0, 0, 1))

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.context.scene.render.fps = FPS
bpy.ops.import_scene.gltf(filepath=SRC)
rig = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
body_mesh = next(o for o in bpy.data.objects if o.type == 'MESH' and o.parent is rig)
rig.animation_data.action = None
for pb in rig.pose.bones:
    pb.matrix_basis = mathutils.Matrix()
bpy.context.view_layer.update()
to_arm = rig.matrix_world.inverted()

# --- The welded mesh: Tripo splits vertices along UV seams; geodesics need them joined.
mw = body_mesh.matrix_world
co = np.array([mw @ v.co for v in body_mesh.data.vertices])
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


# --- Feet: clusters of the lowest vertices, the six farthest from the centre.
ground = wco[:, 2].min()
height = wco[:, 2].max() - ground
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
clusters = {}
for i in low:
    clusters.setdefault(find(i), []).append(i)
# The claws of one foot are near each other along the mesh; two legs only meet through the body.
clusters = list(clusters.values())
near = [geodesic(c) for c in clusters]
merged = list(range(len(clusters)))
for i in range(len(clusters)):
    for j in range(i + 1, len(clusters)):
        if near[i][clusters[j]].min() < 0.3 * height:
            old, new = merged[j], merged[i]
            merged = [new if m == old else m for m in merged]
clusters = [np.array(sum((clusters[i] for i in range(len(clusters)) if merged[i] == m), [])) for m in sorted(set(merged))]
centre = wco[:, :2].mean(axis=0)


def xy(c):
    return wco[c, :2].mean(axis=0)


for c in sorted(clusters, key=lambda c: -np.linalg.norm(xy(c) - centre)):
    print('ground contact at', np.round(xy(c), 3), len(c), 'vertices')
clusters = [c for c in clusters if all(np.linalg.norm(xy(c) - p) > 0.05 for p in IGNORE)]
if FEET:  # the lowest vertices near each named point; a foot may hang above the ground
    feet = []
    for p in FEET:
        near_p = np.nonzero(np.linalg.norm(wco[:, :2] - np.array(p), axis=1) < 0.06 * height)[0]
        floor = wco[near_p, 2].min()
        feet.append(near_p[wco[near_p, 2] < floor + 0.03 * height])
else:
    feet = sorted(clusters, key=lambda c: -np.linalg.norm(xy(c) - centre))[:6]
assert len(feet) == 6, 'found %d feet' % len(feet)

# --- Each leg: rings of equal geodesic distance from the foot, up to where they spread.
STEP = 0.015 * height
legs = []
for foot in feet:
    dist = geodesic(list(foot))
    rings = []
    for k in range(1, 80):
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
    if off[bend] < 0.03 * height:  # a straight leg bends halfway
        bend = len(off) // 2
    knee_at, knee, _ = rings[bend]
    legs.append({'dist': dist, 'reach': reach, 'root': root, 'knee': knee, 'tip': tip, 'knee_at': knee_at})
    print('leg foot', np.round(tip, 3), 'knee', np.round(knee, 3), 'root', np.round(root, 3),
          'reach', round(reach, 3), 'rings', [round(r, 3) for _, _, r in rings])

# Sides and order: +X is one side; front is -Y. Tripods: front and back of one side with the
# middle of the other.
for side in (1, -1):
    mine = sorted([l for l in legs if np.sign(l['root'][0] - centre[0]) == side], key=lambda l: l['root'][1])
    assert len(mine) == 3, 'expected three legs a side, got %d on %+d' % (len(mine), side)
    for i, l in enumerate(mine):
        l['side'], l['pair'] = side, i
        l['tripod'] = (i % 2 == 0) == (side > 0)
legs.sort(key=lambda l: (l['pair'], -l['side']))

# --- Bones.
bpy.context.view_layer.objects.active = rig
bpy.ops.object.mode_set(mode='EDIT')
eb = rig.data.edit_bones
top = next(b for b in eb if b.parent is None)
top_name = top.name
hub = np.mean([l['root'] for l in legs], axis=0)
body = eb.new('Body')
body.head = to_arm @ mathutils.Vector(hub)
body.tail = to_arm @ mathutils.Vector(hub + np.array([0, 0, 0.1 * height]))
body.parent = top
if UPPER:  # Tripo's skeleton missed the torso: an own one, from landmarks, replaces it
    for b in [b for b in eb if b.name not in (top.name, body.name)]:
        eb.remove(b)
    hip, neck, crown, rs, rh, ls, lh = (to_arm @ mathutils.Vector(p) for p in UPPER)

    def bone(name, a, b, parent):
        e = eb.new(name)
        e.head, e.tail, e.parent = a, b, parent
        return e

    torso = bone('Torso', hip, neck, body)
    bone('Head', neck, crown, torso)
    for side, shoulder, hand in (('R', rs, rh), ('L', ls, lh)):
        elbow = (shoulder + hand) / 2 + (shoulder - hip).normalized() * 0.15 * (hand - shoulder).length
        up = bone('Arm_%s_Upper' % side, shoulder, elbow, torso)
        lo = bone('Arm_%s_Lower' % side, elbow, hand, up)
        lo.use_connect = True
for i, l in enumerate(legs):
    up = eb.new('Leg%d_Upper' % i)
    up.head, up.tail = to_arm @ mathutils.Vector(l['root']), to_arm @ mathutils.Vector(l['knee'])
    up.parent = body
    lo = eb.new('Leg%d_Lower' % i)
    lo.head, lo.tail = to_arm @ mathutils.Vector(l['knee']), to_arm @ mathutils.Vector(l['tip'])
    lo.parent, lo.use_connect = up, True
    l['upper'], l['lower'] = up.name, lo.name
bpy.ops.object.mode_set(mode='OBJECT')

# --- Skin: legs by geodesic distance, blended at the knee and faded into the body at the root.
BAND = 0.025 * height
leg_w = []
for l in legs:
    d = l['dist']
    fade = np.clip((l['reach'] + 0.02 * height - d) / (0.06 * height), 0.0, 1.0)
    lower = np.clip((l['knee_at'] + BAND - d) / (2 * BAND), 0.0, 1.0)
    leg_w.append((fade * (1 - lower), fade * lower))
total = sum(u + w for u, w in leg_w)
scale = np.where(total > 1, 1 / np.maximum(total, 1e-9), 1.0)
total = total * scale

LOWER = ('Root', 'Hip', 'Pelvis', 'Waist', 'Thigh', 'Calf', 'Foot', 'Toe', 'Knee')
groups = body_mesh.vertex_groups
if UPPER:  # the new torso, head, arms and body skin by geodesic nearness to each bone
    for g in list(groups):
        groups.remove(g)
    near_bone = {}
    for bone in rig.data.bones:
        if bone.parent is None or bone.name.startswith('Leg'):
            continue
        h, t = np.array(rig.matrix_world @ bone.head_local), np.array(rig.matrix_world @ bone.tail_local)
        seg = t - h
        u = np.clip(((wco - h) @ seg) / max(seg @ seg, 1e-9), 0, 1)
        d = np.linalg.norm(wco - (h + u[:, None] * seg), axis=1)
        seeds = np.argsort(d)[:max(8, int((d < 0.04 * height).sum()))]
        # an arm only takes what is clearly its own (sleeve, hand, what it holds), not the cloak beside it
        near_bone[bone.name] = geodesic(list(seeds)) + (0.08 * height if bone.name.startswith('Arm') else 0.0)
    names = list(near_bone)
    dist = np.stack([near_bone[k] for k in names])
    SIGMA = 0.025 * height
    w = np.exp(-(dist - dist.min(axis=0)) / SIGMA)
    w[dist - dist.min(axis=0) > 4 * SIGMA] = 0
    w /= w.sum(axis=0)
    created = {k: groups.new(name=k) for k in names}
    for v in body_mesh.data.vertices:
        i = weld[v.index]
        for k, name in enumerate(names):
            if w[k, i] > 1e-3:
                created[name].add([v.index], float(w[k, i]), 'REPLACE')
lower_groups = {g.index for g in groups if any(k in g.name for k in LOWER)}
gb = groups.get('Body') or groups.new(name='Body')
gl = [(groups.new(name=l['upper']), groups.new(name=l['lower'])) for l in legs]
for v in body_mesh.data.vertices:
    i = weld[v.index]
    t = float(total[i])
    old = [(g.group, g.weight) for g in v.groups]
    keep = [] if t > 0 else [(g, w) for g, w in old if g not in lower_groups]
    kept = sum(w for _, w in keep)
    moved = sum(w for _, w in old) - kept
    norm = kept + moved
    final = {}
    for g, w in keep:
        final[g] = (1 - t) * w / norm
    final[gb.index] = final.get(gb.index, 0.0) + (1 - t) * (moved / norm if norm > 0 else 1.0)
    for (gu, gw), (u, w) in zip(gl, leg_w):
        final[gu.index] = float(u[i] * scale[i])
        final[gw.index] = float(w[i] * scale[i])
    for g, _ in old:
        groups[g].remove([v.index])
    for g, w in final.items():
        if w > 1e-4:
            groups[g].add([v.index], w, 'REPLACE')
print('skinned', int((total > 0.5).sum()), 'of', n, 'welded vertices to the legs')

if REPORT:
    bpy.ops.wm.save_as_mainfile(filepath=OUT.replace('.glb', '.blend'))

# --- Clips.
bones = rig.pose.bones


def named(*names):
    return [pb for pb in bones if pb.name.split(':')[-1] in names]


def local_rot(pb, axis_world, degrees):
    """A rotation about a world axis, in the bone's own rest frame."""
    axis = (to_arm.to_3x3() @ axis_world).normalized()
    q = mathutils.Quaternion(axis, math.radians(degrees))
    rest = pb.bone.matrix_local.to_3x3().to_quaternion()
    return rest.inverted() @ q @ rest


def leg_axes(l):
    out = mathutils.Vector(l['knee'] - l['root'])
    out.z = 0
    out.normalize()
    return out, out.cross(Z)  # outward, and the axis that lifts the leg


HIPS = named('Hip', 'Pelvis', 'Waist', 'Hips')
SPINE = named('Spine01', 'Spine02', 'Spine', 'Spine1', 'Spine2', 'Torso')
HEAD = named('Head')
ARMS = named('Arm_R_Upper', 'Arm_R_Lower', 'Arm_L_Upper', 'Arm_L_Lower')
BODY = bones['Body']
LEGS = [(bones[l['upper']], bones[l['lower']], l) for l in legs]
OWN = [BODY] + [pb for u, w, _ in LEGS for pb in (u, w)] + (SPINE + HEAD + ARMS if UPPER else [])


def key(action, frame, pose, extra=()):
    """Keys the body and legs (and `extra` bones) from pose: {bone name: [(axis, deg)], ('loc', name): offset}."""
    rig.animation_data.action = action
    for pb in OWN + list(extra):
        pb.rotation_mode = 'QUATERNION'
        q = mathutils.Quaternion()
        for axis, deg in pose.get(pb.name, []):
            q = local_rot(pb, axis, deg) @ q
        pb.rotation_quaternion = q
        pb.keyframe_insert('rotation_quaternion', frame=frame, group=pb.name)
        off = pose.get(('loc', pb.name))
        pb.location = (pb.bone.matrix_local.to_3x3().inverted() @ (to_arm.to_3x3() @ off)) if off else mathutils.Vector()
        pb.keyframe_insert('location', frame=frame, group=pb.name)


def fcurves(action):
    if getattr(action, 'layers', None):
        return [fc for layer in action.layers for strip in layer.strips for bag in strip.channelbags for fc in bag.fcurves]
    return list(action.fcurves)


def fold_root(action):
    """The root carries the ant body now: its turn moves into the hips (the torso still turns), its travel goes."""
    root, hip = bones[top_name], next((pb for pb in HIPS if pb.parent and pb.parent.name == top_name), None)
    curves = [fc for fc in fcurves(action) if fc.data_path.startswith('pose.bones["%s"]' % top_name)]
    if hip is None or not curves:
        return
    rig.animation_data.action = action
    a, b = frames(action)
    r_rest = root.bone.matrix_local.to_quaternion()
    h_rest = hip.bone.matrix_local.to_quaternion()
    poses = []
    for f in range(a, b + 1):
        bpy.context.scene.frame_set(f)
        turn = r_rest @ root.rotation_quaternion @ r_rest.inverted()
        poses.append((f, h_rest.inverted() @ turn @ h_rest @ hip.rotation_quaternion))
    for fc in curves:
        while len(fc.keyframe_points):
            fc.keyframe_points.remove(fc.keyframe_points[0])
    for f, q in poses:
        hip.rotation_quaternion = q
        hip.keyframe_insert('rotation_quaternion', frame=f, group=hip.name)
        root.rotation_quaternion = mathutils.Quaternion()
        root.location = mathutils.Vector()
        root.keyframe_insert('rotation_quaternion', frame=f, group=root.name)
        root.keyframe_insert('location', frame=f, group=root.name)


def damp_hips(action, k=0.33, arms=1.0):
    """Damps the biped hips to `k` (and their bob away); `arms` damps the arms too: a walking
    biped pumps them, and a staff in hand swings through the legs."""
    hips = [pb.name for pb in HIPS]
    limbs = [pb.name for pb in bones if any(k in pb.name for k in ('Clavicle', 'Upperarm', 'Forearm', 'Hand'))]
    for fc in fcurves(action):
        name = fc.data_path.split('"')[1] if '"' in fc.data_path else ''
        if name in limbs and arms < 1.0 and fc.data_path.endswith('rotation_quaternion') and fc.array_index > 0:
            for kp in fc.keyframe_points:
                for p in (kp.co, kp.handle_left, kp.handle_right):
                    p.y *= arms
            continue
        if name not in hips:
            continue
        if fc.data_path.endswith('location'):
            for kp in fc.keyframe_points:
                for p in (kp.co, kp.handle_left, kp.handle_right):
                    p.y = 0.0
        elif fc.data_path.endswith('rotation_quaternion') and fc.array_index > 0:
            for kp in fc.keyframe_points:
                for p in (kp.co, kp.handle_left, kp.handle_right):
                    p.y *= k


def ease(t):
    return 0.5 - 0.5 * math.cos(math.pi * max(0.0, min(1.0, t)))


def gait(swing, lift, fold):
    """A tripod step at gait phase p: the legs on the ground push back, the others lift and swing forward."""
    def pose(p):
        out = {}
        for up, lo, l in LEGS:
            q = (p + (0.5 if l['tripod'] else 0.0)) % 1.0
            if q < 0.5:
                fwd, up_deg = swing * (1 - 4 * q), 0.0
            else:
                s = (q - 0.5) / 0.5
                fwd, up_deg = -swing + 2 * swing * ease(s), math.sin(math.pi * s)
            outward, lift_axis = leg_axes(l)
            # forward is -Y: a leg on +X swings forward turning clockwise about +Z
            out[up.name] = [(Z, -fwd * l['side']), (lift_axis, lift * up_deg)]
            out[lo.name] = [(lift_axis, -fold * up_deg)]
        return out
    return pose


def frames(action):
    a, b = action.frame_range
    return int(a), int(b)


actions = {a.name: a for a in bpy.data.actions}
if UPPER:  # Tripo's clips drove the skeleton that was replaced
    for a in actions.values():
        bpy.data.actions.remove(a)
    actions = {}
if 'fall' in actions:
    bpy.data.actions.remove(actions.pop('fall'))
for name, action in actions.items():
    fold_root(action)
    damp_hips(action, arms=0.5 if name in ('walk', 'run') else 1.0)
    a, b = frames(action)
    if name in ('walk', 'run'):
        cycles = 2
        pose = gait(*((16, 20, 14) if name == 'walk' else (22, 28, 20)))
        for f in range(a, b + 1):
            key(action, f, pose(cycles * (f - a) / (b - a)))
    else:
        for f in (a, b):
            key(action, f, {})
    action.use_fake_user = True


def clip(name, length, pose_at, extra=(), step=2):
    action = bpy.data.actions.new(name)
    for f in list(range(0, length, step)) + [length]:
        key(action, f, pose_at(f / length), extra)
    action.use_fake_user = True
    return action


def idle(t):
    s = math.sin(2 * math.pi * t)
    c = math.sin(2 * math.pi * (t + 0.25))
    pose = {pb.name: [(X, 1.2 * s)] for pb in SPINE}
    for pb in HEAD:
        pose[pb.name] = [(Z, 4 * c), (X, 1.5 * s)]
    pose[('loc', 'Body')] = Z * (0.004 * height * s)
    return pose


def death(t):
    k = ease(t / 0.7)
    curl = ease((t - 0.1) / 0.6)
    pose = {'Body': [(Y, 18 * k)], ('loc', 'Body'): -Z * (0.55 * float(hub[2] - ground) * k)}
    for pb in SPINE:
        pose[pb.name] = [(X, 22 * k)]
    for pb in HEAD:
        pose[pb.name] = [(X, 25 * curl)]
    arms(pose, 15 * k, 15 * k)
    for up, lo, l in LEGS:
        _, lift_axis = leg_axes(l)
        pose[up.name] = [(lift_axis, 30 * curl)]
        pose[lo.name] = [(lift_axis, -70 * curl)]
    return pose


def hit(t):
    k = math.sin(math.pi * min(1.0, t / 0.6)) if t < 0.6 else 0.0
    pose = {pb.name: [(X, -8 * k)] for pb in SPINE}
    pose['Body'] = [(X, -4 * k)]
    return pose


def arms(pose, right, left=0.0, fold=0.0):
    """Swings the arms forward (negative) or back about X, the forearms folding by `fold`."""
    for side, deg in (('R', right), ('L', left)):
        if 'Arm_%s_Upper' % side in bones:
            pose.setdefault('Arm_%s_Upper' % side, []).append((X, deg))
            pose.setdefault('Arm_%s_Lower' % side, []).append((X, fold if side == 'R' else 0.0))
    return pose


def walking(swing, lift, fold, sway):
    legs_at = gait(swing, lift, fold)

    def pose(t):
        p = legs_at(2 * t)
        w = math.sin(4 * math.pi * t)
        for pb in SPINE:
            p[pb.name] = [(Y, sway * w), (X, 0.5 * sway)]
        for pb in HEAD:
            p[pb.name] = [(Y, -0.6 * sway * w)]
        return arms(p, 2.5 * sway * w, -2.5 * sway * w)
    return pose


def attack(t):
    """Draw the staff back, then fling forward; the release at 45%."""
    wind, strike = ease(t / 0.35) * (1 - ease((t - 0.35) / 0.12)), ease((t - 0.35) / 0.12) * (1 - ease((t - 0.6) / 0.4))
    p = {pb.name: [(X, -7 * wind + 10 * strike)] for pb in SPINE}
    for pb in HEAD:
        p[pb.name] = [(X, -4 * wind + 6 * strike)]
    return arms(p, -55 * wind - 35 * strike, 6 * wind, -25 * wind)


def cast(t):
    """Raise the staff high and hold it; the spell at the top."""
    k = ease(t / 0.4) * (1 - ease((t - 0.75) / 0.25))
    p = {pb.name: [(X, -8 * k)] for pb in SPINE}
    for pb in HEAD:
        p[pb.name] = [(X, -10 * k)]
    p[('loc', 'Body')] = Z * (0.02 * height * k)
    if 'Arm_R_Upper' in bones:  # the arm lifts out to the side, the forearm keeps the staff upright
        p['Arm_R_Upper'] = [(Y, 55 * k), (X, -20 * k)]
        p['Arm_R_Lower'] = [(Y, -45 * k)]
    return arms(p, 0.0, -15 * k)


if UPPER:
    clip('walk', 56, walking(16, 20, 14, 2.5), step=1)
    clip('run', 30, walking(22, 28, 20, 4), step=1)
    clip('slash', 30, attack)
    clip('cast_a_spell', 45, cast)
clip('idle', 60, idle, SPINE + HEAD)
clip('death', 36, death, SPINE + HEAD)
if 'hit_to_body_01' not in actions:
    clip('hit_to_body_01', 18, hit, SPINE)
rig.animation_data.action = None
for pb in bones:
    pb.matrix_basis = mathutils.Matrix()
print('clips', [(a.name, tuple(round(v) for v in a.frame_range)) for a in bpy.data.actions])
bpy.ops.export_scene.gltf(filepath=OUT, export_format='GLB', export_animations=True, export_animation_mode='ACTIONS',
                          export_force_sampling=True, export_yup=True, export_apply=False)
print('wrote', OUT)
