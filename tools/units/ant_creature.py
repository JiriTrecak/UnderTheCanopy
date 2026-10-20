"""The clips a six-legged ant creature needs, added in Blender to a Tripo "Other" rig:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/ant_creature.py -- \\
        <ants-unit-<name>-walk.glb> <out.glb> [--speed 1.0] [--attack bite|sting|spit|slam] [--flyer]

Tripo rigs any creature ("Other": Root, Spine_n, Head_n, <pair>_<Left|Right>_Limb_<segment>
and unnamed extras such as mandibles, fins and antennae) but animates it only with its octopod
`Walk` preset; its text-to-motion is humanoid. So the walk comes from Tripo and everything else
is posed here from the bones' roles, in world space (front is -Y, up +Z), and keyed on the
rig: `run` (the walk, faster), `idle` (a breathing bob, extras swaying), `attack` (a lunge that
bites, stings or spits; contact at a third of the clip), `hit` (a recoil), `cast` (rearing
up), `death` (rolling over, legs curling; held at the end). The output keeps the skeleton and
skin and goes on to tools/units/tripo_unit.py like any Tripo character (scale, ground, team
mask, in place, export)."""
import math, sys

import bpy
import mathutils

ARGS = sys.argv[sys.argv.index('--') + 1:]
SRC, OUT = ARGS[0], ARGS[1]
SPEED = float(ARGS[ARGS.index('--speed') + 1]) if '--speed' in ARGS else 1.0
ATTACK = ARGS[ARGS.index('--attack') + 1] if '--attack' in ARGS else 'bite'
FLYER = '--flyer' in ARGS
FPS = 30

bpy.ops.wm.read_factory_settings(use_empty=True)
bpy.context.scene.render.fps = FPS
bpy.ops.import_scene.gltf(filepath=SRC)
rig = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
walk = bpy.data.actions[0]
walk.name = 'walk'
to_arm = rig.matrix_world.to_3x3().inverted()
bones = rig.pose.bones


def role(name):
    n = name.split('::')[-1]
    if n == 'Root':
        return 'root'
    if n.startswith('Spine'):
        return 'spine'
    if n.startswith('Head'):
        return 'head'
    if '_Limb_' in n:
        return 'limb'
    return 'extra'


def limb(name):
    """(pair, side, segment) of a limb bone: pair 0 is the front, side +1 left, -1 right."""
    n = name.split('::')[-1].split('_')
    return int(n[0]), (1 if n[1] == 'Left' else -1), int(n[3])


pairs = sorted({limb(b.name)[0] for b in bones if role(b.name) == 'limb'})
FRONT, BACK = pairs[0], pairs[-1]
extras = [b for b in bones if role(b.name) == 'extra']
heads = [b for b in bones if role(b.name) == 'head']
size = max((rig.matrix_world @ b.bone.tail_local - rig.matrix_world @ b.bone.head_local).length for b in bones) * 4
print('bones', len(bones), 'pairs', pairs, 'extras', [b.name for b in extras], 'size', round(size, 2))

X, Y, Z = mathutils.Vector((1, 0, 0)), mathutils.Vector((0, 1, 0)), mathutils.Vector((0, 0, 1))


def local_rot(pb, axis_world, degrees):
    """A rotation about a world axis, in the bone's own rest frame."""
    axis = (to_arm @ axis_world).normalized()
    q = mathutils.Quaternion(axis, math.radians(degrees))
    rest = pb.bone.matrix_local.to_3x3().to_quaternion()
    return rest.inverted() @ q @ rest


def key_pose(action, frame, pose):
    """pose: {bone: [(axis, degrees)], ...} plus optional ('loc', bone): world offset."""
    rig.animation_data.action = action
    for pb in bones:
        pb.rotation_mode = 'QUATERNION'
        q = mathutils.Quaternion()
        for axis, deg in pose.get(pb.name, []):
            q = local_rot(pb, axis, deg) @ q
        pb.rotation_quaternion = q
        pb.keyframe_insert('rotation_quaternion', frame=frame)
        off = pose.get(('loc', pb.name))
        pb.location = (pb.bone.matrix_local.to_3x3().inverted() @ (to_arm @ off)) if off else mathutils.Vector()
        pb.keyframe_insert('location', frame=frame)


def clip(name, frames, pose_at, step=2):
    action = bpy.data.actions.new(name)
    rig.animation_data_create()
    for f in list(range(0, frames, step)) + [frames]:
        key_pose(action, f, pose_at(f / frames))
    action.use_fake_user = True
    return action


def ease(t):
    return 0.5 - 0.5 * math.cos(math.pi * max(0.0, min(1.0, t)))


def bump(t, a, b, c):
    """0 before a, eases up to 1 at b, back to 0 at c."""
    if t <= a or t >= c:
        return 0.0
    return ease((t - a) / (b - a)) if t < b else ease((c - t) / (c - b))


def named(role_name):
    return [b.name for b in bones if role(b.name) == role_name]


ROOT = named('root')[0]
SPINE = named('spine') or [ROOT]
HEAD = [b.name for b in heads]
if not HEAD:  # Tripo found no head: the frontmost bone that isn't a leg stands in for it
    front = min((b for b in bones if role(b.name) in ('spine', 'extra')),
                key=lambda b: (rig.matrix_world @ b.bone.head_local).y)
    HEAD = [front.name]
    print('head (frontmost)', front.name)


def legs(pair=None, segment=None):
    out = []
    for b in bones:
        if role(b.name) != 'limb':
            continue
        p, s, g = limb(b.name)
        if (pair is None or p == pair) and (segment is None or g == segment):
            out.append((b.name, s))
    return out


def merge(*poses):
    out = {}
    for p in poses:
        for k, v in p.items():
            if isinstance(k, tuple):
                out[k] = out.get(k, mathutils.Vector()) + v
            else:
                out.setdefault(k, []).extend(v)
    return out


# --- idle: a slow breath, the head looking about, extras (fins, antennae, mandibles) swaying.
def idle(t):
    s = math.sin(2 * math.pi * t)
    c = math.sin(2 * math.pi * (t + 0.25))
    pose = {SPINE[0]: [(X, 1.5 * s)], HEAD[0]: [(Z, 4 * c), (X, 2 * s)], (('loc', ROOT)): Z * (0.012 * size * s)}
    for i, e in enumerate(extras):
        pose.setdefault(e.name, []).append((Y if i % 2 else X, 5 * math.sin(2 * math.pi * (t + i * 0.13))))
    if FLYER:
        pose[('loc', ROOT)] = Z * (0.05 * size * s)
    return pose


# --- attack: wind up, lunge and strike at a third, recover.
def attack(t):
    wind = bump(t, 0.0, 0.18, 0.33)
    strike = bump(t, 0.18, 0.33, 0.75)
    pose = {('loc', ROOT): -Y * (0.18 * size * strike) + Y * (0.05 * size * wind)}
    if ATTACK == 'sting':  # the abdomen and back legs thrust forward under the body
        pose[ROOT] = [(X, -10 * wind + 14 * strike)]
    elif ATTACK == 'spit':  # rear back, then throw the head forward
        pose[SPINE[0]] = [(X, -14 * wind + 8 * strike)]
    elif ATTACK == 'slam':
        pose[SPINE[0]] = [(X, -18 * wind + 22 * strike)]
    else:  # bite
        pose[SPINE[0]] = [(X, -6 * wind + 10 * strike)]
    pose.setdefault(HEAD[0], []).append((X, -10 * wind + 16 * strike))
    for e in extras + [b for b in heads[2:]]:  # mandibles open on the wind-up, snap on the strike
        pose.setdefault(e.name, []).append((Z, 14 * wind - 6 * strike))
    for name, side in legs(pair=FRONT, segment=0):
        pose.setdefault(name, []).append((X, -20 * wind + 10 * strike))
    return pose


# --- hit: a quick recoil back and up.
def hit(t):
    k = bump(t, 0.0, 0.25, 1.0)
    return {('loc', ROOT): Y * (0.06 * size * k), SPINE[0]: [(X, -8 * k)], HEAD[0]: [(X, -10 * k)]}


# --- cast: rear up on the back legs, front legs raised, extras flared.
def cast(t):
    k = bump(t, 0.0, 0.35, 1.0)
    pose = {ROOT: [(X, -18 * k)], ('loc', ROOT): Z * (0.08 * size * k), HEAD[0]: [(X, -8 * k)]}
    for name, side in legs(pair=FRONT, segment=0):
        pose[name] = [(X, -35 * k), (Y, 10 * k * side)]
    for i, e in enumerate(extras):
        pose[e.name] = [(Y, 14 * k * (1 if i % 2 else -1))]
    return pose


# --- death: sag, roll onto the side, legs curl up; held at the end.
def death(t):
    k = ease(min(1.0, t / 0.6))
    curl = ease(min(1.0, max(0.0, (t - 0.15) / 0.6)))
    pose = {ROOT: [(Y, 100 * k)], ('loc', ROOT): -Z * (0.1 * size * k) + X * (0.12 * size * k), HEAD[0]: [(X, 20 * curl)]}
    for name, side in legs():
        p, s, g = limb(name)
        pose[name] = [(Y, -side * (25 if g == 0 else 35) * curl)]
    return pose


# --- run: the walk, faster (Tripo's walk is one loop).
start, end = walk.frame_range
run = walk.copy()
run.name = 'run'
for fc in [fc for layer in run.layers for strip in layer.strips for bag in strip.channelbags for fc in bag.fcurves] \
        if hasattr(run, 'layers') and run.layers else run.fcurves:
    for kp in fc.keyframe_points:
        for p in (kp.co, kp.handle_left, kp.handle_right):
            p.x = start + (p.x - start) / 1.6
walk_frames = int(end - start)
clip('idle', 60, idle)
clip('attack', max(18, int(24 / SPEED)), attack)
clip('hit', 12, hit)
clip('cast', 30, cast)
clip('death', 36, death)
for a in (walk, run):
    a.use_fake_user = True
rig.animation_data.action = None
for pb in bones:
    pb.rotation_quaternion = mathutils.Quaternion()
    pb.location = mathutils.Vector()
print('clips', [(a.name, tuple(round(v) for v in a.frame_range)) for a in bpy.data.actions])
bpy.ops.export_scene.gltf(filepath=OUT, export_format='GLB', export_animations=True, export_animation_mode='ACTIONS',
                          export_force_sampling=True, export_yup=True, export_apply=False)
print('wrote', OUT)
