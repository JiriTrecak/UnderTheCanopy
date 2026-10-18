"""The beetles' Great Bastion (the main hall, tier 2) from Tripo, made game-ready and animated in
Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/great_bastion.py -- \\
        <beetle-great-bastion-segmented.glb>

The Great Bastion is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-hall-t2-idle.png): the Bastion's colossal armoured beetle crouched on a
heavier stepped platform, its royal-blue domed shell crowned with three brass smokestacks, its
black head and brass-tipped claws at the front the gate, four squat boiler towers with big
substance tanks round it and three banners before it. Tripo's segmentation split it into parts
that share one texture atlas. Here (D65; the same states as the tier-1 Bastion,
tools/buildings/bastion.py):
- the royal-blue shell plates, tower bands, platform bands and banners become the team mask in
  the atlas's alpha; the black chitin and steel stay out of it;
- the four tanks' dark steel glows blue-violet while it works (`activeGlow` on the atlas,
  limited to the texels under the towers' faces);
- moving nodes: the two great claws (`mandible-l`, `mandible-r`), hinged where they leave the
  head, which part sideways like a gate; the two outer claws beside them (`claw-l`, `claw-r`),
  which lift and spread with them; the head with its inner fangs (`head`), which lifts its chin;
  the shell with its smokestack crown (`shell`), which swells a little as if breathing; and the
  four tanks (`boiler-fl`, `-bl`, `-br`, `-fr`), which swell with pressure one after another
  round the hall (a `vent-<k>` event each for the steam and violet flash on that tower).
  In `work` (training, researching or upgrading) the claws part slowly, the head lifts, the
  shell breathes and the boilers pump in turn twice round; `rivet` / `rivet-2` events put sparks
  round the platform rim. In `spawn` the claws snap wide and the head rears (a `gate` event for
  the steam burst) as a Tinker comes out;
- sockets: the three chimney tops (smoke), the tower caps (steam), the tanks (motes), the gate
  (steam burst), the core over the shell (the violet light) and four points on the platform
  rim (rivet sparks).
At rest nothing moves and nothing glows (D65)."""
import math, os, sys

import bmesh
import bpy
import mathutils
import mathutils.bvhtree
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SOURCE = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-great-bastion'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 20.0  # fills its 20-cell footprint: buildings have no walkable perimeter
S = WIDTH / 20.0  # offsets below are in metres at the 20-metre build
VIOLET = (0.55, 0.32, 1.0)

# Segmented parts by role (tripo_part_<n>, this export only).
PLATFORM = 'tripo_part_0'  # the stepped platform with its stairs
SHELL = 'tripo_part_1'  # the domed shell and the thorax under it
CROWN = 'tripo_part_14'  # the three brass smokestacks on the shell's back
HEAD = 'tripo_part_6'  # head, inner fangs and the great left claw
MANDIBLE_R = 'tripo_part_12'  # the great right claw (the left one is cut from the head)
CLAWS = {'l': 'tripo_part_11', 'r': 'tripo_part_13'}  # the outer front claws
TOWERS = {'fl': 'tripo_part_5', 'bl': 'tripo_part_2', 'br': 'tripo_part_4', 'fr': 'tripo_part_3'}
HIDDEN = ['tripo_part_16']  # a stray sheet under the shell, inside the platform
# Static: 7, 8, 10 the banners; 9, 15 legs.

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
for n in HIDDEN:
    bpy.data.objects.remove(bpy.data.objects[n], do_unlink=True)
parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']
by_name = {o.name: o for o in parts}
ATLAS = parts[0].data.materials[0]
assert all(o.data.materials[0] == ATLAS for o in parts), 'expected one shared atlas'


def verts(obj):
    return np.array([(obj.matrix_world @ v.co)[:] for v in obj.data.vertices], np.float32)


# Normalise on the platform: centred, ground at 0, WIDTH across; transforms applied. Tripo's
# base is the hall's own stepped steel platform; there is no dirt to strip.
for o in parts:
    o.data.transform(o.matrix_world)
    o.matrix_world = mathutils.Matrix.Identity(4)
pv = verts(by_name[PLATFORM])
all_v = np.concatenate([verts(o) for o in parts])
lo, hi = pv.min(axis=0), pv.max(axis=0)
scale = WIDTH / float(max(hi[0] - lo[0], hi[1] - lo[1]))
shift = mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation(
    (-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -float(all_v[:, 2].min())))
for o in parts:
    o.data.transform(shift)
    for p in o.data.polygons:
        p.use_smooth = True


def split(obj, name, keep):
    """Moves the faces of `obj` whose centre passes `keep(centre)` into a new object."""
    out = obj.copy()
    out.data = obj.data.copy()
    out.name = out.data.name = name
    bpy.context.scene.collection.objects.link(out)
    for me, inside in ((obj.data, True), (out.data, False)):
        bm = bmesh.new()
        bm.from_mesh(me)
        bmesh.ops.delete(bm, geom=[f for f in bm.faces if keep(f.calc_center_median()) == inside], context='FACES')
        bm.to_mesh(me)
        bm.free()
    return out


def split_islands(obj, name, keep):
    """Moves the connected pieces of `obj` whose centroid passes `keep(centroid)` into a new
    object (Tripo's mesh is many loose pieces; cutting by piece leaves no torn faces)."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    bm.faces.ensure_lookup_table()
    seen, chosen = set(), set()
    for f in bm.faces:
        if f.index in seen:
            continue
        stack, piece = [f], []
        seen.add(f.index)
        while stack:
            g = stack.pop()
            piece.append(g.index)
            for e in g.edges:
                for h in e.link_faces:
                    if h.index not in seen:
                        seen.add(h.index)
                        stack.append(h)
        centre = sum((bm.faces[i].calc_center_median() for i in piece), mathutils.Vector()) / len(piece)
        if keep(centre):
            chosen.update(piece)
    bm.free()
    centres = {i: p.center.copy() for i, p in enumerate(obj.data.polygons)}
    keys = {tuple(round(c, 5) for c in centres[i]) for i in chosen}
    return split(obj, name, lambda c: tuple(round(v, 5) for v in c) in keys)


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


# --- Team colour: the royal-blue plates become the mask in the atlas's alpha (one material).
# The black chitin and steel carry a faint blue sheen; the brightness floor keeps them out.
node = next(n for n in ATLAS.node_tree.nodes if n.type == 'TEX_IMAGE' and 'basecolor' in n.image.name.lower())
TW, TH = node.image.size
px = np.array(node.image.pixels[:], dtype=np.float32).reshape(TH, TW, 4)
rgb = px[:, :, :3].copy()
LUM = np.array([0.2126, 0.7152, 0.0722], np.float32)
lum = rgb @ LUM
blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * np.clip((rgb[:, :, 2] - 0.1) / 0.06, 0.0, 1.0)
team = bpy.data.images.new('great-bastion-team', TW, TH, alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
ATLAS.name = 'TC_great_bastion'
ATLAS['teamColorMask'] = 'baseColorAlpha'
print('team mask mean', round(float(mask.mean()), 3))


def raster(obj, mark, faces=None):
    """Sets `mark` true on the texels under every UV triangle of `obj` (or of the polygons
    for which `faces(polygon)` holds)."""
    me = obj.data
    uvl = me.uv_layers.active.data
    for poly in me.polygons:
        if faces is not None and not faces(poly):
            continue
        idx = list(poly.loop_indices)
        for k in range(1, len(idx) - 1):
            tri = [idx[0], idx[k], idx[k + 1]]
            (u0, v0), (u1, v1), (u2, v2) = [(uvl[i].uv[0] * TW, uvl[i].uv[1] * TH) for i in tri]
            x0, x1 = int(max(0, min(u0, u1, u2) - 1)), int(min(TW - 1, max(u0, u1, u2) + 1))
            y0, y1 = int(max(0, min(v0, v1, v2) - 1)), int(min(TH - 1, max(v0, v1, v2) + 1))
            xs, ys = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
            d = (v1 - v2) * (u0 - u2) + (u2 - u1) * (v0 - v2)
            if abs(d) < 1e-9:
                continue
            a = ((v1 - v2) * (xs - u2) + (u2 - u1) * (ys - v2)) / d
            b = ((v2 - v0) * (xs - u2) + (u0 - u2) * (ys - v2)) / d
            inside = (a > -0.05) & (b > -0.05) & (1 - a - b > -0.05)
            mark[y0:y1 + 1, x0:x1 + 1] |= inside


# --- The boiler towers' axes: the centre of each cap, on the platform.
AXES = {}
for k, n in TOWERS.items():
    tv = verts(by_name[n])
    top = tv[tv[:, 2] > tv[:, 2].max() - 1.2 * S]
    AXES[k] = (mathutils.Vector((float(top[:, 0].mean()), float(top[:, 1].mean()), float(tv[:, 2].min()))),
               float(tv[:, 2].max()))


def in_tank(k, c, lo=0.0, hi=1.0, r=2.05):
    """Whether point `c` lies inside tower `k`'s tank: within `r` m of its axis, between the
    shares `lo` and `hi` of the tower's height."""
    axis, top = AXES[k]
    h = (c.z - axis.z) / (top - axis.z)
    return math.hypot(c.x - axis.x, c.y - axis.y) < r * S and lo <= h <= hi


# --- Active glow (D65): the substance tanks, the dark steel drums between each tower's banded
# base and its brass-ringed dome, glow violet while it works; the bases, brackets, rings, domes
# and feed pipes stay as they are. Only texels under those drums' faces light up.
TANK_BAND = (0.42, 0.72)  # the drum, as shares of the tower's height (measured on the model)
towers_px = np.zeros((TH, TW), bool)
others_px = np.zeros((TH, TW), bool)
for k, n in TOWERS.items():
    drum = lambda p, k=k: in_tank(k, p.center, *TANK_BAND, r=1.62)  # noqa: E731
    raster(by_name[n], towers_px, drum)
    raster(by_name[n], others_px, lambda p, drum=drum: not drum(p))
for o in parts:
    if o.name not in TOWERS.values():
        raster(o, others_px)
lit = towers_px & ~others_px
print('tank texels', int(towers_px.sum()), 'shared with other faces', int((towers_px & others_px).sum()))
for _ in range(2):  # a little bleed so mips keep the edges lit
    lit = lit | np.roll(lit, 1, 0) | np.roll(lit, -1, 0) | np.roll(lit, 1, 1) | np.roll(lit, -1, 1)
lit &= ~(others_px & ~towers_px)
weight = lit * np.clip((0.22 - lum) / 0.12, 0.0, 1.0) * (1.0 - mask)
glow = weight[..., None] * np.array(VIOLET, np.float32)
img = bpy.data.images.new('great-bastion-glow', TW, TH, alpha=False)
img.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
img.pack()
tex = ATLAS.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = img
bsdf = ATLAS.node_tree.nodes['Principled BSDF']
ATLAS.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 5.0
ATLAS['activeGlow'] = {'pulse': 0.9, 'flow': 0.0}

# --- The great left claw is part of Tripo's head piece: its loose pieces left of the face.
head = by_name[HEAD]
mandible_l = split_islands(head, 'mandible-l', lambda c: c.x < -1.6 * S and c.z < 5.3 * S)
mandible_r = by_name[MANDIBLE_R]
mandible_r.name = mandible_r.data.name = 'mandible-r'
mandibles = {'l': mandible_l, 'r': mandible_r}

# --- The head with its inner fangs hinges at the top of its neck, under the shell's front rim.
head.name = head.data.name = 'head'
hv = verts(head)
HEAD_X = float((hv[:, 0].min() + hv[:, 0].max()) / 2)
HINGE = mathutils.Vector((HEAD_X, float(hv[:, 1].max()) - 0.3 * S, float(hv[:, 2].max()) - 0.4 * S))
set_origin(head, HINGE)
print('head faces', len(head.data.polygons), 'hinge', [round(v, 2) for v in HINGE],
      'y', hv[:, 1].min().round(2), hv[:, 1].max().round(2), 'z', hv[:, 2].min().round(2), hv[:, 2].max().round(2))


def root_of(mv):
    """A claw's hinge: the mean of its highest, rearmost vertices."""
    root = mv[mv[:, 2] > mv[:, 2].max() - 0.6 * S]
    root = root[root[:, 1] > np.percentile(root[:, 1], 50)]
    return float(root[:, 0].mean()), float(root[:, 1].mean()), float(root[:, 2].mean())


# --- The great claws part sideways about Z, so the gate opens across the RTS camera's view.
for side, m in mandibles.items():
    mv = verts(m)
    set_origin(m, root_of(mv))
    tip = mv[np.argmin(mv[:, 2])]
    print('mandible', side, 'faces', len(m.data.polygons), 'pivot', [round(v, 2) for v in m.location], 'tip', tip.round(2))
claws = {}
for side, n in CLAWS.items():
    m = by_name[n]
    m.name = m.data.name = 'claw-' + side
    set_origin(m, root_of(verts(m)))
    claws[side] = m
    print('claw', side, 'pivot', [round(v, 2) for v in m.location])

# --- The shell carries its smokestack crown, breathing about its own foot.
crown_v = verts(by_name[CROWN])
shell = join([by_name[SHELL], by_name[CROWN]], 'shell')
sv = verts(shell)
SHELL_FOOT = mathutils.Vector((float(np.median(sv[:, 0])), float(np.median(sv[:, 1])), float(np.percentile(sv[:, 2], 3))))
SHELL_TOP = float(sv[:, 2].max())
set_origin(shell, SHELL_FOOT)

# --- The boiler tanks: each swells about the foot of its axis. Its feed pipe to the shell and
# the brackets reaching out from its base stay on the platform, so the joints never move.
boilers, tank_axes = {}, {}
for k, n in TOWERS.items():
    axis = AXES[k][0]
    tank = split(by_name[n], 'boiler-' + k, lambda c, k=k: in_tank(k, c, lo=0.18))
    tank_axes[k] = (axis, verts(tank))
    set_origin(tank, axis)
    boilers[k] = tank
    print('boiler', k, 'axis', [round(v, 2) for v in axis], 'faces', len(tank.data.polygons), 'top', round(float(tank_axes[k][1][:, 2].max()), 2))

# --- Everything else is one static node.
moving = {head, shell} | set(mandibles.values()) | set(claws.values()) | set(boilers.values())
static = [o for o in bpy.context.scene.objects if o.type == 'MESH' and o not in moving]
bastion = join(static, 'great-bastion')

# --- Sockets.
surface = mathutils.bvhtree.BVHTree.FromObject(bastion, bpy.context.evaluated_depsgraph_get())


def ground(x, y, top=30.0):
    hit = surface.ray_cast(mathutils.Vector((x, y, top)), mathutils.Vector((0, 0, -1)))[0]
    return hit.z if hit else 0.0


# The three smokestacks: the crown's highest points, left to right.
tops = crown_v[crown_v[:, 2] > crown_v[:, 2].max() - 1.0 * S]
for key, (a, b) in zip('abc', ((-9, -0.7 * S), (-0.7 * S, 0.5 * S), (0.5 * S, 9))):
    t = tops[(tops[:, 0] > a) & (tops[:, 0] < b)]
    t = t[t[:, 2] > t[:, 2].max() - 0.3 * S]
    kit.socket('socket_chimney_' + key, (float(t[:, 0].mean()), float(t[:, 1].mean()), float(t[:, 2].max()) + 0.3 * S))
for k, (axis, tv) in tank_axes.items():
    kit.socket('socket_tower_' + k, (axis.x, axis.y, float(tv[:, 2].max()) + 0.15 * S))
    # The motes rise off the tank's face towards the camera, at mid height.
    kit.socket('socket_tank_' + k, (axis.x, float(tv[:, 1].min()) + 0.3 * S, axis.z + 0.55 * (float(tv[:, 2].max()) - axis.z)))
fang_tip = float(hv[:, 1].min())
kit.socket('socket_gate', (HEAD_X, fang_tip - 0.4 * S, ground(HEAD_X, fang_tip - 0.4 * S) + 0.1 * S))
kit.socket('socket_glow', (HEAD_X, fang_tip + 0.6 * S, 3.8 * S))
# The hall's violet light hangs over the shell's middle, between the four tanks.
kit.socket('socket_core', (0.0, 0.0, 9.0 * S))
RIM = {'a': (-6.0, -9.6), 'b': (9.6, 2.5), 'c': (5.5, -9.6), 'd': (-9.6, 4.5)}
for k, (x, y) in RIM.items():
    kit.socket('socket_rivet_' + k, (x * S, y * S, ground(x * S, y * S) + 0.05 * S))


# --- Clips: one action per clip keying every moving node (one slot each), so blends between
# clips stay clean.
def slot_curves(action, obj):
    slot = obj.animation_data.action_slot
    out = []
    for layer in action.layers:
        for strip in layer.strips:
            bag = strip.channelbag(slot)
            if bag:
                out.extend(bag.fcurves)
    return out


def clip(name, tracks):
    """Tracks are (node, path, index, [(frame, value)], interpolation or None)."""
    action = bpy.data.actions.new(name)
    for obj, path, index, keys, interp in tracks:
        obj.animation_data_create()
        obj.animation_data.action = action
        for f, value in keys:
            getattr(obj, path)[index] = value
            obj.keyframe_insert(data_path=path, index=index, frame=f)
        for fc in slot_curves(action, obj):
            if fc.data_path == path and fc.array_index == index:
                for kp in fc.keyframe_points:
                    kp.interpolation = interp or 'BEZIER'
        obj.animation_data.action = None
    for obj in dict.fromkeys(t[0] for t in tracks):
        track = obj.animation_data.nla_tracks.new()
        track.name = name
        track.strips.new(name, 0, action)
    return action


CLAW_SHARE = 1.25  # the outer claws spread a little further than the great ones, so they clear


def tracks(frames, part=None, lift=None, breathe=None, swell=None, snap=False):
    """Every moving node's channels over `frames`; None holds the rest pose. `part` opens the
    claws (radians, outward), `lift` raises the head's chin (radians), `breathe` swells the
    shell (share of its height), `swell` maps a boiler to its swelling keys (share)."""
    rest = [(0, 0.0), (frames, 0.0)]
    interp = 'LINEAR' if snap else None
    out = []
    for group, share in ((mandibles, 1.0), (claws, CLAW_SHARE)):
        for side, m in group.items():
            sign = -1.0 if side == 'l' else 1.0  # about Z: the left claw turns clockwise to open
            out.append((m, 'rotation_euler', 2, [(f, sign * share * v) for f, v in (part or rest)], None))
            # The claws tip up a little as they part, so their points clear the floor.
            out.append((m, 'rotation_euler', 0, [(f, 0.25 * share * v) for f, v in (part or rest)], None))
    out.append((head, 'rotation_euler', 0, [(f, -v) for f, v in (lift or rest)], interp))
    out.append((shell, 'scale', 2, [(f, 1.0 + v) for f, v in (breathe or rest)], None))
    out.append((shell, 'scale', 0, [(f, 1.0 + 0.4 * v) for f, v in (breathe or rest)], None))
    out.append((shell, 'scale', 1, [(f, 1.0 + 0.4 * v) for f, v in (breathe or rest)], None))
    for k, b in boilers.items():
        keys = (swell or {}).get(k, rest)
        out.append((b, 'scale', 2, [(f, 1.0 + v) for f, v in keys], None))
        out.append((b, 'scale', 0, [(f, 1.0 + 0.6 * v) for f, v in keys], None))
        out.append((b, 'scale', 1, [(f, 1.0 + 0.6 * v) for f, v in keys], None))
    return out


clip('idle', tracks(2))

# work (6 s): the claws part slowly, stand open working a little, and close; the head lifts its
# chin with them; the shell breathes three times; the boilers pump in turn round the hall
# (front left, back left, back right, front right) twice, each venting at its peak. Rivet
# sparks round the rim, alternating between two pairs of sockets.
W = 180
OPEN = 0.42
part = [(0, 0.0), (45, OPEN), (70, OPEN * 0.85), (95, OPEN), (120, OPEN * 0.85), (140, OPEN), (W, 0.0)]
lift = [(0, 0.0), (45, 0.14), (95, 0.11), (140, 0.14), (W, 0.0)]
BREATH = 0.025
breathe = [(0, 0.0)]
for i in range(3):
    breathe += [(i * 60 + 30, BREATH), (i * 60 + 60, 0.0)]
ORDER = ['fl', 'bl', 'br', 'fr']
STEP, RISE, SWELL = W // 8, 7, 0.11
swell, vents = {k: [(0, 0.0)] for k in ORDER}, []
for i in range(8):
    k = ORDER[i % 4]
    f = 4 + i * STEP
    swell[k] += [(f, 0.0), (f + RISE, SWELL), (f + RISE + 4, SWELL * 0.8), (f + RISE + 16, 0.0)]
    vents.append(('vent-' + k, f + RISE))
for k in ORDER:
    swell[k].append((W, 0.0))
RIVETS = [12, 72, 132]
RIVETS_2 = [42, 102, 162]
clip('work', tracks(W, part, lift, breathe, swell))

# spawn (2 s): the claws snap open wide and the head rears (steam bursts at the gate on `gate`)
# as a Tinker comes out; then they settle shut.
SP, GATE = 60, 2
WIDE = 0.75
clip('spawn', tracks(SP, part=[(0, 0.0), (6, WIDE), (12, WIDE * 0.93), (38, WIDE), (SP, 0.0)],
                     lift=[(0, 0.0), (6, 0.24), (38, 0.22), (SP, 0.0)],
                     breathe=[(0, 0.0), (8, BREATH * 1.3), (SP, 0.0)]))

all_pts = np.concatenate([verts(o) for o in bpy.context.scene.objects if o.type == 'MESH'])
height = max(float(all_pts[:, 2].max()), SHELL_FOOT.z + (SHELL_TOP - SHELL_FOOT.z) * (1 + BREATH * 1.3))
tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
print('triangles', tris, 'height', round(height, 2), 'extent x', all_pts[:, 0].min().round(2), all_pts[:, 0].max().round(2),
      'y', all_pts[:, 1].min().round(2), all_pts[:, 1].max().round(2))

events = ([{'name': 'rivet', 'time': round(f / kit.FPS, 3)} for f in RIVETS]
          + [{'name': 'rivet-2', 'time': round(f / kit.FPS, 3)} for f in RIVETS_2]
          + [{'name': n, 'time': round(f / kit.FPS, 3)} for n, f in vents])
asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Great Bastion', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': [ATLAS.name]},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            # Event times are seconds into the clip.
            {'semantic': 'work', 'clip': 'work', 'loop': True, 'events': sorted(events, key=lambda e: e['time'])},
            {'semantic': 'spawn', 'clip': 'spawn', 'loop': False,
             'events': [{'name': 'gate', 'time': round(GATE / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [20, 20], 'offset': [0, 0]},
        'vegetationClearance': 10,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/buildings/great_bastion.py.',
}
for o in bpy.data.objects:
    if o.name.startswith('socket_'):
        print('socket', o.name, [round(v, 2) for v in o.location])
kit.export(OUT, NAME, asset)
