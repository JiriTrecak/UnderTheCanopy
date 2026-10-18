"""The beetles' War Lodge (the barracks) from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/war_lodge.py -- \\
        <beetle-war-lodge-segmented.glb>

The lodge is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-war-lodge-idle.png), split by Tripo's segmentation into parts that share
one texture atlas. It has no substance glass of its own: it reads through motion (D65).
- the atlas gets the team mask (the royal-blue shield face, banners and bands) in its alpha;
- the two crossed war hammers over the gate are their own nodes, `hammer-l` and `hammer-r`,
  pivoted where their handles cross behind the shield (the handle butts below the shield
  stay on the wall). In `work` they drill: both lift and swing in over the shield, and
  their heads clash above it like an anvil beat (a `clang` event per strike);
- the portcullis is its own node: it creeps up and down while the lodge trains, and lifts
  fully in `spawn` as a soldier marches out (a `gate` event for the dust at its foot);
- the six banners on the rack before the gate hang in three pairs that stir while it works;
- sockets: `socket_clang` where the heads meet, `socket_gate` at the portcullis foot,
  `socket_tower_l` / `socket_tower_r` on the horned domes (steam vents).
At rest nothing moves."""
import math, os, sys

import bmesh
import bpy
import mathutils
import mathutils.bvhtree
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
LODGE = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-war-lodge'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 16.0  # fills its 16-cell footprint: buildings have no walkable perimeter
S = WIDTH / 16.0  # offsets below are in metres at the 16-metre build

# Segmented parts by role (tripo_part_<n>, this export only).
HAMMER_L, HAMMER_R = 'tripo_part_8', 'tripo_part_9'  # head, upper handle and a butt stub each
PORTCULLIS = ['tripo_part_5', 'tripo_part_12', 'tripo_part_21']  # the gate grid
STEP = 'tripo_part_3'  # the front step with the banner rack; its banners are split out
BANNER_BIT = 'tripo_part_17'  # a sliver of the rightmost banner pair
DOMES = {'l': 'tripo_part_7', 'r': 'tripo_part_10'}  # the horned tower caps
# Static: 11 the coat-of-arms shield (team-colour face), 0/1 towers, 2 keep, 4 battlements, 6 courtyard floor, 13-20 small trims; 15 is
# the dark gate passage behind the portcullis, 16 the banner rack's backing bars.

kit.reset()
bpy.ops.import_scene.gltf(filepath=LODGE)
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        for c in o.children:
            m = c.matrix_world.copy()
            c.parent = None
            c.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']
# Normalise: footprint centred, ground at 0, WIDTH across, transforms applied. Tripo's base
# is the lodge's own stone platform; there is no dirt to strip.
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
by_name = {o.name: o for o in parts}
ATLAS = parts[0].data.materials[0]  # every part shares one material and atlas


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


def split(obj, name, keep):
    """Moves the faces of `obj` whose centre passes `keep(centre)` into a new object."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    out = obj.copy()
    out.data = obj.data.copy()
    out.name = out.data.name = name
    bpy.context.scene.collection.objects.link(out)
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if keep(f.calc_center_median())], context='FACES')
    bm.to_mesh(obj.data)
    bm.free()
    bm = bmesh.new()
    bm.from_mesh(out.data)
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if not keep(f.calc_center_median())], context='FACES')
    bm.to_mesh(out.data)
    bm.free()
    return out


def join(objs, name):
    bpy.ops.object.select_all(action='DESELECT')
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.join()
    out = bpy.context.view_layer.objects.active
    out.name = out.data.name = name
    return out


def set_origin(obj, at):
    """Moves the node origin to `at` (world), keeping the mesh where it is."""
    at = mathutils.Vector(at)
    obj.data.transform(mathutils.Matrix.Translation(-at))
    obj.location = at
    obj.rotation_mode = 'XYZ'  # glTF imports use quaternions; the clips key Euler angles


def verts(obj):
    return np.array([(obj.matrix_world @ v.co)[:] for v in obj.data.vertices], np.float32)


# --- Team colour: the royal-blue plates become the mask in the atlas's alpha (one material).
node = texture_of(ATLAS)
px = pixels(node.image)
rgb = px[:, :, :3]
blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08)
team = bpy.data.images.new('war-lodge-team', node.image.size[0], node.image.size[1], alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
ATLAS.name = 'TC_war_lodge'
ATLAS['teamColorMask'] = 'baseColorAlpha'
print('team mask mean', round(float(mask.mean()), 3))

# --- The hammers: Tripo modelled each as a head with its upper handle, which meets a butt
# stub (the other hammer's handle end, as the crossing reads) in a V behind the shield.
# The V's apex is the pivot; the stubs stay on the wall.
APEX_Z = 6.5 * S
hammers = {}
for side, part in (('l', HAMMER_L), ('r', HAMMER_R)):
    whole = by_name[part]
    arm = split(whole, 'hammer-' + side, lambda c: c.z >= APEX_Z)
    hv = verts(arm)
    low = hv[hv[:, 2] < APEX_Z + 0.35 * S]
    pivot = mathutils.Vector((float(low[:, 0].mean()), float(low[:, 1].mean()), APEX_Z + 0.1 * S))
    set_origin(arm, pivot)
    hammers[side] = arm
    print('hammer', side, 'pivot', [round(v, 2) for v in pivot], 'faces', len(arm.data.polygons))

# The drill: both hammers lift by LIFT and swing their heads in by ANGLE about their pivots
# (Y axis: + turns the left head clockwise as seen from the front, towards the middle) until
# the heads clash above the shield. Swinging alone would drive the heads through the
# shield's rim; LIFT carries them over it (checked by eye at every frame of the sweep; at
# the top the handles' cut ends stay behind the shield). ANGLE is where the heads touch.
LIFT = 1.8 * S


def posed(side, angle, lift):
    """The head's vertices (world) with the hammer turned by `angle` and lifted by `lift`."""
    arm = hammers[side]
    pv = np.array(arm.location[:], np.float32)
    v = np.array([v.co[:] for v in arm.data.vertices], np.float32)
    a = angle if side == 'l' else -angle
    c, s = math.cos(a), math.sin(a)
    out = np.stack([v[:, 0] * c + v[:, 2] * s, v[:, 1], -v[:, 0] * s + v[:, 2] * c], 1) + pv
    out[:, 2] += lift
    far = np.linalg.norm(v[:, [0, 2]], axis=1) > 1.6 * S  # the head, not the handle
    return out[far]


def gap(angle):
    """Smallest x gap between the heads where they share height (negative: overlapping)."""
    a, b = posed('l', angle, LIFT), posed('r', angle, LIFT)
    best = 9.0
    for p in a:
        near = b[np.abs(b[:, 2] - p[2]) < 0.3 * S]
        if len(near):
            best = min(best, float(near[:, 0].min() - p[0]))
    return best


ANGLE = 0.0
while ANGLE < 1.2 and gap(ANGLE) > 0.02 * S:
    ANGLE += 0.005
print('drill lift', round(LIFT, 2), 'angle', round(math.degrees(ANGLE), 1))
hl, hr = posed('l', ANGLE, LIFT), posed('r', ANGLE, LIFT)
meet = mathutils.Vector(((float(hl[:, 0].max()) + float(hr[:, 0].min())) / 2,
                         float(min(hl[:, 1].min(), hr[:, 1].min())) - 0.1 * S,
                         float((hl[:, 2].mean() + hr[:, 2].mean()) / 2)))

# --- The portcullis: the gate grid, raised along Z (it slides up behind the keep's face).
gate = join([by_name[n] for n in PORTCULLIS], 'portcullis')
gv = verts(gate)
GATE_FOOT = mathutils.Vector((float((gv[:, 0].min() + gv[:, 0].max()) / 2), float(gv[:, 1].mean()), float(gv[:, 2].min())))
GATE_H = float(gv[:, 2].max() - gv[:, 2].min())
set_origin(gate, GATE_FOOT)
print('portcullis', [round(v, 2) for v in GATE_FOOT], 'height', round(GATE_H, 2))

# --- The banners on the rack before the gate: three pairs, each hung from the rail.
step = by_name[STEP]
RACK = dict(x=(-1.95 * S, 2.5 * S), y=(-5.6 * S, -4.3 * S), z=(1.45 * S, 3.22 * S))


def in_rack(c):
    return all(RACK[k][0] < getattr(c, k) < RACK[k][1] for k in 'xyz')


rack = split(step, 'banners', in_rack)
rack = join([rack, by_name[BANNER_BIT]], 'banners')
rv = verts(rack)
print('banner faces', len(rack.data.polygons), 'x', rv[:, 0].min().round(2), rv[:, 0].max().round(2),
      'y', rv[:, 1].min().round(2), rv[:, 1].max().round(2), 'z', rv[:, 2].min().round(2), rv[:, 2].max().round(2))
RAIL = mathutils.Vector((0, float(np.median(rv[:, 1])), float(rv[:, 2].max()) + 0.08 * S))
EDGES = [-0.48 * S, 1.02 * S]  # between the pairs
banners = []
for i, (a, b) in enumerate(zip([-9.0] + EDGES, EDGES + [9.0])):
    pair = split(rack, 'banners-%d' % (i + 1), lambda c, a=a, b=b: a <= c.x < b)
    pv = verts(pair)
    set_origin(pair, (float((pv[:, 0].min() + pv[:, 0].max()) / 2), RAIL.y, RAIL.z))
    banners.append(pair)
bpy.data.objects.remove(rack, do_unlink=True)

# The passage behind the portcullis: Tripo left ragged shards there, which show once the
# gate lifts. A plain dark backing just behind the grid makes the opening a clean, deep
# doorway; its UVs all sit on one dark, unmasked texel of the atlas (no new material).
lum_map = rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32)
rm = pixels(texture_of(ATLAS, 'Roughness').image)  # glTF: roughness in G, metallic in B
k, edge = 9, 32
bad = lum_map + mask * 4.0 + (rm[:, :, 1] < 0.5) * 4.0 + (rm[:, :, 2] > 0.08) * 4.0
dark = np.lib.stride_tricks.sliding_window_view(bad, (k, k)).max(axis=(2, 3))
dark[:edge], dark[-edge:], dark[:, :edge], dark[:, -edge:] = 9, 9, 9, 9  # no atlas padding
iy, ix = np.unravel_index(int(np.argmin(dark)), dark.shape)
th, tw = lum_map.shape
dark_uv = ((ix + k / 2) / tw, (iy + k / 2) / th)
print('dark texel lum', round(float(dark[iy, ix]), 3), 'uv', [round(v, 3) for v in dark_uv])
x0, x1 = float(gv[:, 0].min()) - 0.1 * S, float(gv[:, 0].max()) + 0.1 * S
back = kit.box('gate-backing', (x1 - x0, 0.05 * S, GATE_H + 0.1 * S),
               at=((x0 + x1) / 2, GATE_FOOT.y + 0.2 * S, GATE_FOOT.z + GATE_H / 2))
back.data.materials.append(ATLAS)
back.data.uv_layers.new(name='UVMap')
for d in back.data.uv_layers[0].data:
    d.uv = dark_uv

# --- Everything else is one static node.
moving = set(hammers.values()) | {gate} | set(banners)
static = [o for o in bpy.context.scene.objects if o.type == 'MESH' and o not in moving]
domes = {k: verts(by_name[n]) for k, n in DOMES.items()}
lodge = join(static, 'war-lodge')

# --- Sockets.
surface = mathutils.bvhtree.BVHTree.FromObject(lodge, bpy.context.evaluated_depsgraph_get())
for side, dv in domes.items():
    # The steam vents on the crown of each horned dome (found by a ray straight down: the
    # dome's top is a few large faces, so its vertices don't say where its surface is).
    cx, cy = float((dv[:, 0].min() + dv[:, 0].max()) / 2), float((dv[:, 1].min() + dv[:, 1].max()) / 2)
    hit = surface.ray_cast(mathutils.Vector((cx, cy, 30.0)), mathutils.Vector((0, 0, -1)))[0]
    kit.socket('socket_tower_' + side, (cx, cy, (hit.z if hit else float(dv[:, 2].max())) + 0.1 * S))
kit.socket('socket_clang', meet)
kit.socket('socket_gate', GATE_FOOT + mathutils.Vector((0, -0.9 * S, 0.05 * S)))


# --- Clips: one action per clip keying every moving node (one slot each), so blends between
# clips stay clean.
def clip(name, tracks):
    """Tracks are (node, path, index, [(frame, value)], {frame: interpolation}); keys not
    named there are Bézier. A key's interpolation shapes the segment after it."""
    action = bpy.data.actions.new(name)
    for obj, path, index, keys, interp in tracks:
        obj.animation_data_create()
        obj.animation_data.action = action
        for f, value in keys:
            getattr(obj, path)[index] = value
            obj.keyframe_insert(data_path=path, index=index, frame=f)
        for fc in slot_curves(action, obj):
            if fc.data_path == path:
                for kp in fc.keyframe_points:
                    kp.interpolation = (interp or {}).get(round(kp.co.x), 'BEZIER')
        obj.animation_data.action = None
    for obj in dict.fromkeys(t[0] for t in tracks):
        track = obj.animation_data.nla_tracks.new()
        track.name = name
        track.strips.new(name, 0, action)
    return action


def slot_curves(action, obj):
    slot = obj.animation_data.action_slot
    out = []
    for layer in action.layers:
        for strip in layer.strips:
            bag = strip.channelbag(slot)
            if bag:
                out.extend(bag.fcurves)
    return out


def tracks(frames, swing=None, lift=None, gate_up=None, stir=None, strike=None):
    """Every moving node's channels over `frames`; None holds the rest pose. `strike`
    names the hammer keys whose segment runs straight into the clash (no ease-in)."""
    rest = [(0, 0.0), (frames, 0.0)]
    out = []
    for side, arm in hammers.items():
        z0 = arm.location.z
        sign = 1.0 if side == 'l' else -1.0
        fast = {f: 'LINEAR' for f in (strike or [])}
        out.append((arm, 'rotation_euler', 1, [(f, sign * v) for f, v in (swing or rest)], fast))
        out.append((arm, 'location', 2, [(f, z0 + v) for f, v in (lift or rest)], fast))
    out.append((gate, 'location', 2, [(f, GATE_FOOT.z + v) for f, v in (gate_up or rest)], None))
    for i, pair in enumerate(banners):
        out.append((pair, 'rotation_euler', 0, (stir[i] if stir else rest), None))
    return out


clip('idle', tracks(2))

# work (4 s): five drill beats, each 24 frames: the hammers lift and swing in (fast), clash,
# recoil a little and fall back to rest. The portcullis creeps up, hangs, and comes down;
# the banner pairs stir out of step.
W, BEAT, HIT = 120, 24, 9
swing, lift, clangs, strike = [(0, 0.0)], [(0, 0.0)], [], []
for beat in range(W // BEAT):
    f = beat * BEAT
    swing += [(f + 3, -0.06), (f + 6, ANGLE * 0.4), (f + HIT, ANGLE), (f + HIT + 3, ANGLE * 0.8), (f + BEAT, 0.0)]
    lift += [(f + 3, LIFT * 0.1), (f + 6, LIFT * 0.6), (f + HIT, LIFT), (f + HIT + 3, LIFT * 0.97), (f + BEAT, 0.0)]
    clangs.append(f + HIT)
    strike.append(f + 6)
GATE_WORK = 0.42 * GATE_H
gate_work = [(0, 0.0), (45, GATE_WORK), (80, GATE_WORK), (W, 0.0)]


def stir_keys(frames, amp, phase):
    keys = [(0, 0.0)]
    for f in range(10, frames - 5, 15):
        keys.append((f, amp * math.sin(2 * math.pi * (f / frames) * 2 + phase) * math.sin(math.pi * f / frames) ** 0.5))
    return keys + [(frames, 0.0)]


clip('work', tracks(W, swing, lift, gate_work, [stir_keys(W, 0.16, p) for p in (0.0, 2.1, 4.2)], strike))

# spawn (2 s): the portcullis lifts fully (dust at its foot on `gate`), holds while the
# soldier marches out, then drops; the banners swing in the draught.
SP, GATE_EVENT = 60, 3
GATE_FULL = 0.92 * GATE_H
clip('spawn', tracks(SP, gate_up=[(0, 0.0), (GATE_EVENT, 0.05 * S), (16, GATE_FULL), (42, GATE_FULL), (SP, 0.0)],
                     stir=[[(0, 0.0), (12 + 2 * i, 0.2), (26 + 2 * i, -0.1), (40 + 2 * i, 0.04), (SP, 0.0)]
                           for i in range(3)]))

all_pts = [o.matrix_world @ v.co for o in bpy.context.scene.objects if o.type == 'MESH' for v in o.data.vertices]
height = max(max(p.z for p in all_pts), float(hl[:, 2].max()), float(hr[:, 2].max()))  # heads at the clash
tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
print('triangles', tris, 'height', round(height, 2), 'clangs', clangs)

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'War Lodge', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': [ATLAS.name]},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            # Event times are seconds into the clip.
            {'semantic': 'work', 'clip': 'work', 'loop': True,
             'events': [{'name': 'clang', 'time': round(f / kit.FPS, 3)} for f in clangs]},
            {'semantic': 'spawn', 'clip': 'spawn', 'loop': False,
             'events': [{'name': 'gate', 'time': round(GATE_EVENT / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [16, 16], 'offset': [0, 0]},
        'vegetationClearance': 8,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/buildings/war_lodge.py.',
}
for o in bpy.data.objects:
    if o.name.startswith('socket_'):
        print('socket', o.name, [round(v, 2) for v in o.location])
kit.export(OUT, NAME, asset)
