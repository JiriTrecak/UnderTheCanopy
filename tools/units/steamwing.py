"""The beetles' Steamwing from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/steamwing.py -- \\
        <unit-steamwing-segmented.glb>

The Steamwing is the beetles' tier-2 flyer: a beetle too heavy to fly, lifted by a boiler wing
rig on its back, with a substance beam emitter under its chin that fires violet bolts. The
model is Tripo's image-to-3D model of the chosen concept (`unit-steamwing-1.png` in the old
repository's `.asset-work/concepts/buildings/`), split by Tripo's segmentation into ~100 parts
that share one texture atlas. It is a rigid machine, animated by nodes like the buildings
(the cook turns animated nodes into palette joints), not skinned. Here:
- the parts become three nodes: the body (beetle, boiler, stacks, legs, emitter) and the two
  wings (arm, hub, steel frame and panels), each wing hinged at its arm's root on the boiler;
- Tripo stood the wings upright in a V, edge-on from above; they are laid out flat (a little
  dihedral, swept back), so the wingspan reads from the RTS camera and from straight above, and
  they beat about the body's long axis;
- the slate wing panels and shell plates become the team mask in the atlas's alpha (Tripo
  painted the concept's royal-blue plates slate grey), the boiler's blue glass glows violet
  with substance (glass only, docs/beetles.md);
- the model's lowest point is at z 0 under its pivot, like every unit; the game lifts flyers to
  their definition's `flightHeight` and drops the corpse to the floor when one dies;
- clips: `idle` (hovering: a steady beat, the body bobs), `walk` (flying forward, also `run`:
  a faster beat, nose down), `attack` (the chin emitter fires, the body jolts back; `release`),
  `hit`, `death` (the wings fold, it noses over and lies crashed);
- sockets: `socket_projectile` at the emitter's mouth, `socket_exhaust_a..c` on the stacks;
- `geometry_2.glb` is the violet bolt the emitter fires (the weapon missile's model): a white-
  violet core in a violet glow with a tapering streak behind it, along +Y (its flight)."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOURCE = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-steamwing'
OUT = os.path.join(ROOT, 'base', 'assets', 'units', NAME)
DEBUG_MASK = os.environ.get('STEAMWING_DEBUG_MASK') == '1'
BODY = 1.55  # metres head (emitter) to tail; display.scale 2 makes it 3.1 in game, a bit over a soldier
DISPLAY_SCALE = 2.0

# Parts by role in Tripo's normalisation (4 m across): the wings lie off the body's sides at the
# boiler (y 0.1..0.6, above z 0.5); the three exhaust stacks top the boiler; the emitter tube
# runs under the chin. Numbers hold for this one export.
STACKS = {'a': 'tripo_part_31', 'b': 'tripo_part_25', 'c': 'tripo_part_32'}
EMITTER = 'tripo_part_2'

kit.reset()
bpy.ops.import_scene.gltf(filepath=SOURCE)
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        for c in o.children:
            m = c.matrix_world.copy()
            c.parent = None
            c.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']
MAT = parts[0].data.materials[0]  # the one shared atlas material
for o in parts:
    o.data.transform(o.matrix_world)
    o.matrix_world = mathutils.Matrix.Identity(4)


def bounds(objs):
    vs = np.array([v.co[:] for o in objs for v in o.data.vertices])
    return mathutils.Vector(vs.min(0)), mathutils.Vector(vs.max(0))


# Normalise: centred over its pivot, lowest point at z 0, BODY long (the wings add no length).
lo, hi = bounds(parts)
s4 = 4.0 / max(hi.x - lo.x, hi.y - lo.y)
shift = mathutils.Matrix.Translation((-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z))
for o in parts:
    o.data.transform(shift)
side = {}
for o in parts:
    a, b = bounds([o])
    c = (a + b) / 2 * s4
    if abs(c.x) > 0.45 and c.z > 0.5 and 0.1 < c.y < 0.6:
        side.setdefault('r' if c.x > 0 else 'l', []).append(o)
wing_parts = side['r'] + side['l']
body_parts = [o for o in parts if o not in wing_parts]
blo, bhi = bounds(body_parts)
scale = BODY / (bhi.y - blo.y)
for o in parts:
    o.data.transform(mathutils.Matrix.Scale(scale, 4))
    for p in o.data.polygons:
        p.use_smooth = True
print('wing parts', len(side['r']), len(side['l']), 'scale', round(scale, 4))
by_name = {o.name: o for o in parts}
stack_tops = {}
for key, name in STACKS.items():
    o = by_name[name]
    vs = [v.co for v in o.data.vertices]
    zmax = max(v.z for v in vs)
    ring = [v for v in vs if v.z > zmax - 0.03]
    stack_tops[key] = mathutils.Vector((np.mean([v.x for v in ring]), np.mean([v.y for v in ring]), zmax))
ev = [v.co for v in by_name[EMITTER].data.vertices]
ey = min(v.y for v in ev)
mouth_ring = [v for v in ev if v.y < ey + 0.03]
MOUTH = mathutils.Vector((np.mean([v.x for v in mouth_ring]), ey, np.mean([v.z for v in mouth_ring])))


# --- Texture: the shared atlas.
def texture_of(mat, socket='Base Color'):
    bsdf = mat.node_tree.nodes['Principled BSDF']
    link = bsdf.inputs[socket].links
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


def uv_mask(objs, size, inside):
    """Per texel, `inside(position, normal)` (0..1) of its surface point (world metres)."""
    out = np.zeros((size, size), np.float32)
    for obj in objs:
        me = obj.data
        me.calc_loop_triangles()
        uvl = me.uv_layers.active.data
        for tri in me.loop_triangles:
            uv = np.array([uvl[i].uv[:] for i in tri.loops]) * size - 0.5
            pos = np.array([me.vertices[v].co[:] for v in tri.vertices])
            nrm = np.array(tri.normal[:])
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
            p = l0[..., None] * pos[0] + l1[..., None] * pos[1] + l2[..., None] * pos[2]
            w = np.where(on, inside(p, nrm), 0.0)
            out[ys, xs] = np.maximum(out[ys, xs], w)
    return out


node = texture_of(MAT)
W, H = node.image.size
px = np.array(node.image.pixels[:], np.float32).reshape(H, W, 4)
rgb = px[:, :, :3]
lum = rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32)
G = 512  # surface maps at a quarter of the atlas, upsampled
up = lambda m: np.kron(m, np.ones((H // G, W // G), np.float32))  # noqa: E731

# Team colour: the slate plates. Tripo painted the concept's royal-blue wing panels and shell
# plates slate grey (a cool, unsaturated mid grey); the brass trim, bright steel and black chitin
# keep their colour. Only the wing panels and the upper shell take it: the slate of the legs
# and the boiler's underside stays steel.
spread = rgb.max(2) - rgb.min(2)
slate = (np.clip((rgb[:, :, 2] - rgb[:, :, 0] + 0.005) / 0.03, 0, 1)
         * np.clip((0.16 - spread) / 0.05, 0, 1)
         * np.clip((lum - 0.13) / 0.04, 0, 1) * np.clip((0.40 - lum) / 0.06, 0, 1))
wings_uv = up(uv_mask(wing_parts, G, lambda p, n: np.ones(p.shape[:-1])))
shell_uv = up(uv_mask(body_parts, G, lambda p, n: np.clip((p[..., 2] - 0.42 * BODY / 1.55) / 0.06, 0, 1)
                      * float(n[2] > 0.25)))
glass = np.clip(((rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])) - 0.18) / 0.1, 0, 1)
mask = np.clip(slate * np.maximum(wings_uv, shell_uv) * (1 - glass), 0, 1)
print('team mask mean', round(float(mask.mean()), 4), 'glass', round(float(glass.mean()), 4))
linear = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
source_max = float(np.percentile(linear.max(2)[mask > 0.5], 90)) if (mask > 0.5).any() else 1.0
print('team source max (linear)', round(source_max, 4))
shown = rgb.copy()
if DEBUG_MASK:
    shown = shown * (1 - mask[..., None]) + mask[..., None] * np.array((1.0, 0.1, 0.1), np.float32)
team = bpy.data.images.new('steamwing-team', W, H, alpha=True)
team.pixels[:] = np.concatenate([shown, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
MAT.name = 'TC_' + NAME
MAT['teamColorMask'] = 'baseColorAlpha'
MAT['teamColorSourceMax'] = round(source_max, 4)

# Substance glass: the boiler's blue window glows violet (constant: the rig is always running).
gl = (glass[..., None] * np.array((0.55, 0.32, 1.0), np.float32)).astype(np.float32)
gimg = bpy.data.images.new('steamwing-glass', W, H, alpha=False)
gimg.pixels[:] = np.concatenate([gl, np.ones_like(gl[:, :, :1])], axis=2).ravel()
gimg.scale(1024, 1024)
gimg.pack()
tex = MAT.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = gimg
bsdf = MAT.node_tree.nodes['Principled BSDF']
MAT.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 3.0


# --- Nodes: the body and two wings.
def join(objs, name):
    bpy.ops.object.select_all(action='DESELECT')
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    out = bpy.context.view_layer.objects.active
    out.name = out.data.name = name
    return out


body = join(body_parts, 'body')
blo, bhi = bounds([body])
PIVOT = mathutils.Vector((0.0, (blo.y + bhi.y) / 2, (blo.z + bhi.z) / 2))
body.data.transform(mathutils.Matrix.Translation(-PIVOT))
body.rotation_mode = 'XYZ'
body.location = PIVOT
wings = {}
for key in ('r', 'l'):
    w = join(side[key], 'wing-' + ('right' if key == 'r' else 'left'))
    vs = np.array([v.co[:] for v in w.data.vertices])
    inner = vs[np.abs(vs[:, 0]) < np.abs(vs[:, 0]).min() + 0.04 * BODY / 1.55]
    hinge = mathutils.Vector((inner[:, 0].mean(), inner[:, 1].mean(), inner[:, 2].mean()))
    sign = 1.0 if key == 'r' else -1.0
    # Laid flat: the upright V turns forward and down (top to the front), then sweeps back
    # and keeps a little dihedral; the panels trail behind the spar.
    lay = (mathutils.Matrix.Rotation(-sign * 0.22, 4, 'Y')       # dihedral: tip up
           @ mathutils.Matrix.Rotation(sign * 0.55, 4, 'Z')      # swept back
           @ mathutils.Matrix.Rotation(math.radians(80), 4, 'X'))  # upright -> flat
    w.data.transform(lay @ mathutils.Matrix.Translation(-hinge))
    w.rotation_mode = 'XYZ'
    w.parent = body
    w.matrix_parent_inverse = mathutils.Matrix.Identity(4)
    w.location = hinge - PIVOT
    wings[key] = w
    wlo, whi = bounds([w])
    print('wing', key, 'hinge', [round(v, 2) for v in hinge], 'extent', [round(v, 2) for v in wlo], [round(v, 2) for v in whi])

# Sockets ride the body.
def body_socket(name, at):
    e = kit.socket(name, (0, 0, 0))
    e.parent = body
    e.matrix_parent_inverse = mathutils.Matrix.Identity(4)
    e.location = at - PIVOT
    return e


body_socket('socket_projectile', MOUTH + mathutils.Vector((0, -0.04, 0)))
for key, top in stack_tops.items():
    body_socket('socket_exhaust_' + key, top + mathutils.Vector((0, 0, 0.04)))


# --- Clips: sampled every frame (linear), one action per clip with a slot per node.
def keyclip(obj, name, frames, pose):
    """`pose(frame)` -> {(data path, index): value} for `obj`, keyed on every frame."""
    action = bpy.data.actions.get(name) or bpy.data.actions.new(name)
    ad = obj.animation_data_create()
    ad.action = action
    for f in frames:
        for (path, index), value in pose(f).items():
            getattr(obj, path)[index] = value
            obj.keyframe_insert(data_path=path, index=index, frame=f)
    for fc in kit._fcurves(action):
        for kp in fc.keyframe_points:
            kp.interpolation = 'LINEAR'
    track = ad.nla_tracks.new()
    track.name = name
    track.strips.new(name, int(frames[0]), action)
    ad.action = None


REST_LOC = body.location.copy()
U = BODY / 1.55  # motion offsets were set at 1.55 m


def body_pose(dz=0.0, dy=0.0, pitch=0.0, roll=0.0, yaw=0.0):
    return {('location', 0): REST_LOC.x, ('location', 1): REST_LOC.y + dy * U, ('location', 2): REST_LOC.z + dz * U,
            ('rotation_euler', 0): pitch, ('rotation_euler', 1): roll, ('rotation_euler', 2): yaw}


def wing_pose(sign, beat=0.0, sweep=0.0):
    """`beat` > 0 raises the tip (about the long axis); `sweep` > 0 folds it back."""
    return {('rotation_euler', 0): 0.0, ('rotation_euler', 1): -sign * beat, ('rotation_euler', 2): sign * sweep}


def stroke(phase):
    """A wing stroke over one beat (phase 0..1): a quick, powerful downstroke and a slower
    recovery, starting and ending at mid (0) going up."""
    return math.sin(2 * math.pi * phase + 0.35 * math.sin(2 * math.pi * phase))


def animate(name, frames, body_at, beat_at, sweep_at=lambda f: 0.0):
    keyclip(body, name, frames, body_at)
    for key, sign in (('r', 1.0), ('l', -1.0)):
        keyclip(wings[key], name, frames, lambda f, s=sign: wing_pose(s, beat_at(f), sweep_at(f)))


BEAT = 0.55
# idle: hovering, one beat per 0.8 s of clip (the game plays clips at 1.5x).
FI = 24
animate('idle', range(0, FI + 1),
        lambda f: body_pose(dz=-0.035 * math.sin(2 * math.pi * f / FI + 0.6), pitch=0.03 * math.sin(2 * math.pi * f / FI)),
        lambda f: BEAT * stroke(f / FI))
# walk (and run): flying forward, nose down, two beats a 0.55 s cycle.
FW = 16
animate('walk', range(0, FW + 1),
        lambda f: body_pose(dz=-0.025 * math.sin(2 * math.pi * f / FW + 0.6), pitch=0.16 + 0.02 * math.sin(2 * math.pi * f / FW)),
        lambda f: BEAT * stroke(f / FW))
# attack: aims (nose dips), fires at FIRE: the body jolts back and up, then settles.
FA, FIRE = 30, 12


def recoil(f):
    if f <= FIRE:
        k = f / FIRE
        return body_pose(dz=0.01 * k, dy=-0.02 * k, pitch=0.07 * k)
    t = (f - FIRE) / (FA - FIRE)
    kick = math.exp(-5.0 * t) * min(1.0, (f - FIRE) / 2.0)
    settle = 1 - t
    return body_pose(dz=0.06 * kick, dy=0.22 * kick - 0.02 * settle * (1 - kick), pitch=0.07 * settle - 0.32 * kick)


animate('attack', range(0, FA + 1), recoil, lambda f: BEAT * stroke(f / 15.0))
# hit: a flinch, a roll and a drop; the wings flare.
FH = 12
animate('hit', range(0, FH + 1),
        lambda f: body_pose(dz=-0.08 * math.sin(math.pi * f / FH), roll=0.28 * math.sin(math.pi * f / FH),
                            pitch=-0.1 * math.sin(math.pi * f / FH)),
        lambda f: 0.35 * math.sin(math.pi * f / FH))
# death: the rig chokes, the wings fold back and droop, it noses over and lies crashed (the
# game drops a flyer's corpse to the floor in about 0.7 s of clip time).
FD = 48
PITCH_D, ROLL_D, BEAT_D, SWEEP_D = 0.45, 0.5, -0.22, 0.5


def ease(t):
    t = min(max(t, 0.0), 1.0)
    return t * t * (3 - 2 * t)


def crashed_lift():
    """How high the body must sit in its crashed pose so nothing sinks into the ground."""
    rot = mathutils.Euler((PITCH_D, ROLL_D, 0.0), 'XYZ').to_matrix().to_4x4()
    mb = mathutils.Matrix.Translation(REST_LOC) @ rot
    zs = [(mb @ v.co).z for v in body.data.vertices]
    for key, sign in (('r', 1.0), ('l', -1.0)):
        w = wings[key]
        mw = mb @ mathutils.Matrix.Translation(w.location) @ mathutils.Euler(
            (0.0, -sign * BEAT_D, sign * SWEEP_D), 'XYZ').to_matrix().to_4x4()
        zs += [(mw @ v.co).z for v in w.data.vertices]
    return -min(zs) / U - 0.04  # a little dug in: it crashed


LIFT = crashed_lift()
print('crashed lift', round(LIFT * U, 3))


def death_body(f):
    if f <= 6:  # the choke: a jerk up and a twist
        k = math.sin(math.pi * f / 12)
        return body_pose(dz=0.05 * k, pitch=-0.12 * k, roll=-0.1 * k)
    t = ease((f - 6) / 16)  # nose over while it falls
    pose = body_pose(dz=LIFT * t, pitch=-0.12 * (1 - t) + PITCH_D * t, roll=-0.1 * (1 - t) + ROLL_D * t)
    if f > 22:  # the crash: a small bounce, then still
        b = math.exp(-(f - 22) / 4.0) * math.sin(math.pi * min(1.0, (f - 22) / 8.0))
        pose[('location', 2)] += 0.05 * U * b
        pose[('rotation_euler', 0)] -= 0.06 * b
    return pose


def death_beat(f):
    if f <= 8:  # a last spasm
        return 0.4 * math.sin(math.pi * f / 8)
    return BEAT_D * ease((f - 8) / 14)


animate('death', range(0, FD + 1), death_body, death_beat, lambda f: SWEEP_D * ease((f - 6) / 18))

# Back to rest for the export (the rest pose is the nodes' own transform).
body.location = REST_LOC
body.rotation_euler = (0, 0, 0)
for w in wings.values():
    w.rotation_euler = (0, 0, 0)

tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
lo, hi = bounds([body])
wl, wh = bounds([wings['l']])
wr, whr = bounds([wings['r']])
span = (wings['r'].location.x + whr.x) - (wings['l'].location.x + wl.x)
height = hi.z - lo.z
print('triangles', tris, 'body', round(hi.y - lo.y, 2), 'span', round(span, 2), 'height', round(height, 2))

E = lambda f: round(f / kit.FPS, 3)  # noqa: E731  events are seconds into the clip
asset = {
    'id': 'asset.models.units.' + NAME, 'name': 'Steamwing', 'kind': 'unit',
    'tags': ['beetles', 'tripo', 'machine', 'flyer', 'animated', 'team-mask'], 'race': 'beetles',
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
            {'semantic': 'attack', 'clip': 'attack', 'loop': False, 'events': [{'name': 'release', 'time': E(FIRE)}]},
            {'semantic': 'hit', 'clip': 'hit', 'loop': False, 'events': []},
            {'semantic': 'death', 'clip': 'death', 'loop': False, 'events': []},
        ],
        'sockets': [{'name': 'projectile', 'node': 'socket_projectile', 'offset': [0, 0, 0]}]
        + [{'name': 'exhaust_' + k, 'node': 'socket_exhaust_' + k, 'offset': [0, 0, 0]} for k in STACKS],
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': DISPLAY_SCALE, 'character': 'base', 'healthHeight': round((height + 0.35) * DISPLAY_SCALE, 1)},
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) concept for '
               'Under the Canopy; prepared and animated by tools/units/steamwing.py (the bolt is modelled there).',
}
kit.export(OUT, NAME, asset)


# --- The bolt (geometry_2.glb): flies along glTF +Y (Blender +Z), its head at the origin.
kit.reset()


def glow(name, base, emission, strength, alpha=1.0):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    b = mat.node_tree.nodes['Principled BSDF']
    b.inputs['Base Color'].default_value = (*base, 1.0)
    b.inputs['Emission Color'].default_value = (*emission, 1.0)
    b.inputs['Emission Strength'].default_value = strength
    b.inputs['Roughness'].default_value = 1.0
    if alpha < 1.0:
        b.inputs['Alpha'].default_value = alpha
        mat.surface_render_method = 'BLENDED'
    return mat


def spindle(name, radius, front, back, segments=12, rings=6):
    """A teardrop along Z: a round nose `front` ahead of the widest point, a long taper
    `back` behind it."""
    bm = bmesh.new()
    prof = []
    for i in range(rings + 1):  # nose: quarter circle
        a = math.pi / 2 * i / rings
        prof.append((radius * math.sin(a), front * math.cos(a)))
    for i in range(1, rings + 1):  # tail: taper to a point
        t = i / rings
        prof.append((radius * (1 - t) ** 1.6, -back * t))
    verts = []
    for r, z in prof:
        ring = []
        for j in range(segments):
            a = 2 * math.pi * j / segments
            ring.append(bm.verts.new((r * math.cos(a), r * math.sin(a), z)))
        verts.append(ring)
    for k in range(len(verts) - 1):
        for j in range(segments):
            bm.faces.new([verts[k][j], verts[k][(j + 1) % segments], verts[k + 1][(j + 1) % segments], verts[k + 1][j]])
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = bpy.data.objects.new(name, me)
    kit.link(obj)
    for p in me.polygons:
        p.use_smooth = True
    return obj


core = spindle('bolt-core', 0.11, 0.18, 0.55)
core.data.materials.append(glow('bolt-core', (0.85, 0.78, 1.0), (0.88, 0.8, 1.0), 10.0))
halo = spindle('bolt-glow', 0.22, 0.3, 1.0)
halo.data.materials.append(glow('bolt-glow', (0.4, 0.16, 0.95), (0.45, 0.2, 1.0), 4.0, alpha=0.55))
streak = spindle("bolt-streak", 0.14, 0.05, 2.3, rings=8)
streak.location = (0, 0, -0.3)
streak.data.materials.append(glow('bolt-streak', (0.36, 0.12, 0.9), (0.4, 0.16, 1.0), 3.0, alpha=0.4))
for o in (core, halo, streak):
    o.data.uv_layers.new()
bpy.ops.export_scene.gltf(filepath=os.path.join(OUT, 'geometry_2.glb'), export_format='GLB', export_yup=True,
                          export_apply=True, export_extras=True, export_animations=False)
print('bolt', sum(len(o.data.polygons) for o in (core, halo, streak)), 'faces')
