"""Adds scene poses to a skinned unit, in its saved Blender file, for `mission.animate`:

    /Applications/Blender.app/Contents/MacOS/Blender -b base/assets/units/<name>/source.blend \\
        -P tools/units/scene_poses.py -- kneel [sleep]

- `kneel` (a one-shot the game holds at its end when a scene asks for it with `loop`): from
  the idle stance down in about a second. A humanoid (Mixamo rig) goes down on its right knee,
  bowing its back and head; a creature (Tripo's own rig) lowers its body between its planted
  feet and bows its head to the ground, the insect's kneel.
- `sleep` (a loop): a humanoid sits slumped on the ground, legs drawn up, head on its chest; a
  creature rests its belly on the ground, legs folded, head down. Either breathes slowly.

Poses are solved with IK (feet and knees placed on the ground in the idle's frame) and baked
into ordinary bone keys; the constraints go again. The clips start and end in place (the cook
refuses clips whose root travels). The asset is re-exported and the semantic registered in its
`asset.json` (2-space JSON); a clip that exists is replaced.

Blender is Z-up; units face -Y (glTF +Z)."""
import json, math, os, sys

import bpy
import mathutils

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

V = mathutils.Vector
FPS = 30
CLIPS = sys.argv[sys.argv.index('--') + 1:] if '--' in sys.argv else ['kneel']
OUT = os.path.dirname(bpy.data.filepath)
NAME = os.path.basename(OUT)
scene = bpy.context.scene
scene.render.fps = FPS
rig = next(o for o in bpy.data.objects if o.type == 'ARMATURE')
rig.animation_data_create()
IDLE = bpy.data.actions['idle']
pose = rig.pose.bones
MIXAMO = any(b.name.startswith('mixamorig:') for b in rig.data.bones)
W = rig.matrix_world


def update():
    bpy.context.view_layer.update()


def bone(short):
    """A Mixamo bone by its short name."""
    return pose.get('mixamorig:' + short)


def world_head(pb):
    return W @ pb.head


def world_tail(pb):
    return W @ pb.tail


def stance():
    """The idle's first frame, as the pose to start from."""
    rig.animation_data.action = IDLE
    scene.frame_set(0)
    update()
    keys = {pb.name: (pb.location.copy(), pb.rotation_quaternion.copy(), pb.scale.copy()) for pb in pose}
    rig.animation_data.action = None
    for pb in pose:
        pb.rotation_mode = 'QUATERNION'
        pb.location, pb.rotation_quaternion, pb.scale = (v.copy() for v in keys[pb.name])
    update()
    return keys


def height():
    meshes = [o for o in bpy.data.objects if o.type == 'MESH']
    dg = bpy.context.evaluated_depsgraph_get()
    top = 0.0
    for m in meshes:
        e = m.evaluated_get(dg)
        top = max(top, max((m.matrix_world @ v.co).z for v in e.data.vertices))
    return top


def turn(pb, angle, axis='X', pivot=None):
    """Rotates a bone (and what hangs from it) in world space round `axis` through its head."""
    m = pb.matrix.copy()
    centre = pb.head.copy() if pivot is None else pivot
    r = mathutils.Matrix.Rotation(angle, 4, axis)
    pb.matrix = mathutils.Matrix.Translation(centre) @ r @ mathutils.Matrix.Translation(-centre) @ m
    update()


def shift(pb, offset):
    m = pb.matrix.copy()
    m.translation = m.translation + offset
    pb.matrix = m
    update()


def empty(name, at):
    e = bpy.data.objects.new(name, None)
    scene.collection.objects.link(e)
    e.location = at
    return e


def ik(pb, target, chain, pole=None):
    c = pb.constraints.new('IK')
    c.target = target
    c.chain_count = chain
    c.use_tail = True
    if pole is not None:
        c.pole_target = pole
        c.pole_angle = 0.0
    return pb, c


def solve(constraints, bones):
    """Bakes what the IK made of `bones` into their own channels and removes the IK."""
    update()
    solved = {pb.name: pb.matrix.copy() for pb in bones}
    for owner, c in constraints:
        owner.constraints.remove(c)
    for o in [o for o in bpy.data.objects if o.type == 'EMPTY' and o.name.startswith('ik_')]:
        bpy.data.objects.remove(o, do_unlink=True)
    update()
    # Parents before children.
    for pb in sorted(bones, key=lambda b: len(b.parent_recursive)):
        pb.matrix = solved[pb.name]
        update()


def snapshot():
    return {pb.name: (pb.location.copy(), pb.rotation_quaternion.copy(), pb.scale.copy()) for pb in pose}


def apply(keys):
    for pb in pose:
        pb.location, pb.rotation_quaternion, pb.scale = (v.copy() for v in keys[pb.name])
    update()


def chain_of(pb):
    """The bones of `pb`'s chain up to a ground-standing limb's root."""
    chain = [pb]
    while chain[-1].parent is not None and len(chain[-1].parent.children) == 1 and chain[-1].parent.parent is not None:
        chain.append(chain[-1].parent)
    return chain


# --- Humanoids (Mixamo) ---------------------------------------------------------------------

def humanoid_kneel(h):
    hips = bone('Hips')
    thigh = (world_head(bone('RightLeg')) - world_head(bone('RightUpLeg'))).length
    shin = (world_head(bone('RightFoot')) - world_head(bone('RightLeg'))).length
    ankle = world_head(bone('LeftFoot')).z
    # Down on the right knee: the hips sink to the kneeling thigh's length over the knee.
    drop = world_head(hips).z - (thigh * 0.95 + 0.02 * h)
    shift(hips, V((0, 0.12 * thigh, -drop)))
    turn(hips, math.radians(6))
    for name, angle in (('Spine', 8), ('Spine1', 8), ('Spine2', 6), ('Neck', 10), ('Head', 18)):
        if bone(name):
            turn(bone(name), math.radians(angle))
    targets = []
    r_hip, l_hip = world_head(bone('RightUpLeg')), world_head(bone('LeftUpLeg'))
    # Right: the knee on the ground under the hip, the shin lying back.
    knee = V((r_hip.x, r_hip.y + 0.05 * thigh, 0.03 * h))
    t = empty('ik_r', knee + V((0, shin * 0.98, 0.04 * h)))
    p = empty('ik_r_pole', knee + V((0, -thigh, 0)))
    targets.append(ik(bone('RightLeg'), t, 2, p))
    # Left: the foot planted ahead, the shin upright, the knee forward.
    t = empty('ik_l', V((l_hip.x, l_hip.y - thigh * 0.95, ankle)))
    p = empty('ik_l_pole', V((l_hip.x, l_hip.y - thigh * 2, l_hip.z + thigh)))
    targets.append(ik(bone('LeftLeg'), t, 2, p))
    solve(targets, [bone(n) for n in ('RightUpLeg', 'RightLeg', 'LeftUpLeg', 'LeftLeg')])
    # The kneeling foot rests on its toes.
    turn(bone('RightFoot'), math.radians(-35))
    # Arms hang a little forward of the knee.
    for side in ('Left', 'Right'):
        if bone(side + 'Arm'):
            turn(bone(side + 'Arm'), math.radians(15))


def humanoid_sleep(h):
    hips = bone('Hips')
    thigh = (world_head(bone('RightLeg')) - world_head(bone('RightUpLeg'))).length
    shin = (world_head(bone('RightFoot')) - world_head(bone('RightLeg'))).length
    # Sitting on the ground, leaning back a little, legs drawn up in front.
    drop = world_head(hips).z - 0.11 * h
    shift(hips, V((0, 0.25 * thigh, -drop)))
    turn(hips, math.radians(-12))
    for name, angle in (('Spine', 14), ('Spine1', 14), ('Spine2', 12), ('Neck', 18), ('Head', 30)):
        if bone(name):
            turn(bone(name), math.radians(angle))
    targets = []
    for side, out in (('Right', -1), ('Left', 1)):
        hip = world_head(bone(side + 'UpLeg'))
        t = empty('ik_' + side, V((hip.x + out * 0.2 * thigh, hip.y - (thigh + shin) * 0.72, 0.06 * h)))
        p = empty('ik_' + side + '_pole', V((hip.x + out * 0.6 * thigh, hip.y - thigh, hip.z + 2 * thigh)))
        targets.append(ik(bone(side + 'Leg'), t, 2, p))
    solve(targets, [bone(n) for n in ('RightUpLeg', 'RightLeg', 'LeftUpLeg', 'LeftLeg')])
    # Arms limp in the lap.
    for side in ('Left', 'Right'):
        if bone(side + 'Arm'):
            turn(bone(side + 'Arm'), math.radians(25))
        if bone(side + 'ForeArm'):
            turn(bone(side + 'ForeArm'), math.radians(20))


def humanoid_breath(phase):
    for name in ('Spine1', 'Spine2'):
        if bone(name):
            turn(bone(name), math.radians(2.5) * phase)
    if bone('Head'):
        turn(bone('Head'), math.radians(2) * phase)


# --- Creatures (Tripo's own rig) ------------------------------------------------------------

def root():
    return next(pb for pb in pose if pb.parent is None)


def head_bone():
    for name in ('tripo::Head_0', 'tripo::Spine_4', 'tripo::Head_1'):
        if name in pose:
            return pose[name]
    return None


def feet(h):
    """Bones whose tails stand on the ground: the tips of the legs."""
    return [pb for pb in pose if not pb.children and world_tail(pb).z < 0.15 * h and pb.parent is not None]


def creature_lower(h, share, bow, nod):
    """Lowers the body by `share` of its height over the ground between planted feet."""
    tips = feet(h)
    planted = {pb.name: world_tail(pb).copy() for pb in tips}
    body = sum(world_head(chain_of(pb)[-1]).z for pb in tips) / max(len(tips), 1)
    r = root()
    shift(r, V((0, 0, -share * body)))
    # Bow forward (the front faces -Y): a turn round X through the body's middle.
    turn(r, math.radians(bow), pivot=r.head + V((0, 0, 0.3 * h)))
    head = head_bone()
    if head is not None:
        turn(head, math.radians(nod))
    targets, bones = [], []
    for pb in tips:
        chain = chain_of(pb)
        t = empty('ik_' + pb.name, planted[pb.name])
        targets.append(ik(pb, t, len(chain)))
        bones += chain
    solve(targets, bones)


def creature_breath(phase, h):
    r = root()
    shift(r, V((0, 0, 0.012 * h * phase)))
    head = head_bone()
    if head is not None:
        turn(head, math.radians(2) * phase)


# --- Clips ----------------------------------------------------------------------------------

def key_all(frame):
    for pb in pose:
        pb.keyframe_insert('location', frame=frame, group=pb.name)
        pb.keyframe_insert('rotation_quaternion', frame=frame, group=pb.name)
        pb.keyframe_insert('scale', frame=frame, group=pb.name)


def new_action(name):
    old = bpy.data.actions.get(name)
    if old is not None:
        bpy.data.actions.remove(old)
    action = bpy.data.actions.new(name)
    action.use_fake_user = True
    rig.animation_data.action = action
    return action


def make_kneel(start, h):
    new_action('kneel')
    apply(start)
    key_all(0)
    if MIXAMO:
        humanoid_kneel(h)
    else:
        creature_lower(h, share=0.4, bow=12, nod=16)
    key_all(28)
    key_all(36)


def make_sleep(start, h):
    new_action('sleep')
    apply(start)
    if MIXAMO:
        humanoid_sleep(h)
    else:
        creature_lower(h, share=0.72, bow=4, nod=16)
    rest = snapshot()
    # A slow breath: in over 1.4 s, out over 1.6 s, the loop closing on itself.
    for frame, phase in ((0, 0.0), (42, 1.0), (90, 0.0)):
        apply(rest)
        if MIXAMO:
            humanoid_breath(phase)
        else:
            creature_breath(phase, h)
        key_all(frame)


start = stance()
h = height()
print('model height', round(h, 3), 'mixamo' if MIXAMO else 'creature')
for clip in CLIPS:
    apply(start)
    {'kneel': make_kneel, 'sleep': make_sleep}[clip](start, h)
rig.animation_data.action = IDLE
apply(start)

asset = json.load(open(os.path.join(OUT, 'asset.json')))
animations = asset['capabilities']['animations']
for clip in CLIPS:
    animations[:] = [a for a in animations if a['semantic'] != clip]
    animations.append({'semantic': clip, 'clip': clip, 'loop': clip == 'sleep', 'events': []})
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)
