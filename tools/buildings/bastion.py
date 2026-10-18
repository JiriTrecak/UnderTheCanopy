"""The beetles' Bastion (the main hall, tier 1) from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/bastion.py -- \\
        <beetle-bastion-segmented.glb>

The Bastion is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-hall-t1-idle.png): a colossal armoured beetle crouched on a square riveted
steel platform, its royal-blue domed shell the roof, its black head and brass-tipped claws
at the front the gate, two round substance tanks on brass-capped towers at its sides and two
banners before them. Tripo's segmentation split it into parts that share one texture atlas.
Here:
- the platform is stretched front to back so it fills the square 20-cell footprint (Tripo
  made it 20 x 17.6 m); the beetle, towers and banners on it keep their proportions;
- the royal-blue shell plates, banners and the front plaque become the team mask in the
  atlas's alpha; the black chitin and steel stay out of it;
- the two tanks' dark steel and their feed pipes glow blue-violet while it works
  (`activeGlow` on the atlas, limited to the texels under the towers' faces);
- moving nodes: the two great outer claws (`mandible-l`, `mandible-r`), hinged where they
  leave the head, which part sideways like a gate; the head with its inner fangs (`head`),
  which lifts its chin; and the shell (`shell`), which swells a little as if breathing.
  In `work` (training or upgrading) the claws part slowly, the head lifts and the shell
  breathes; `rivet` / `rivet-2` events put sparks round the platform rim. In `spawn` the claws
  snap wide and the head rears (a `gate` event for the steam burst) as a Tinker comes out;
- sockets: the tower caps (steam), the tanks (motes), the gate (light, steam burst) and four
  points on the platform rim (rivet sparks).
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
NAME = 'beetles-bastion'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 20.0  # fills its 20-cell footprint: buildings have no walkable perimeter
S = WIDTH / 20.0  # offsets below are in metres at the 20-metre build
VIOLET = (0.55, 0.32, 1.0)

# Segmented parts by role (tripo_part_<n>, this export only).
BODY = 'tripo_part_0'  # shell, head, inner fangs and thorax in one part
MANDIBLES = {'l': 'tripo_part_10', 'r': 'tripo_part_14'}  # the great outer claws
TOWERS = {'l': 'tripo_part_5', 'r': 'tripo_part_4'}  # tank, brass bands and cap, feed pipe
PLATFORM = ['tripo_part_1', 'tripo_part_2', 'tripo_part_3', 'tripo_part_11', 'tripo_part_18']
RIGHT_BANNER = 'tripo_part_8'
LEFT_BANNER_IN = 'tripo_part_3'  # the left banner shares its part with the platform's left side
HIDDEN = ['tripo_part_23']  # a stray sheet inside the platform block
# Static: 6, 7, 9, 12, 13, 15, 16, 17, 19, 20 legs; 21, 22 floor pipes.

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


# Normalise on the platform: centred, ground at 0, WIDTH across; transforms applied.
for o in parts:
    o.data.transform(o.matrix_world)
    o.matrix_world = mathutils.Matrix.Identity(4)
pv = np.concatenate([verts(by_name[n]) for n in PLATFORM])
all_v = np.concatenate([verts(o) for o in parts])
lo, hi = pv.min(axis=0), pv.max(axis=0)
scale = WIDTH / float(hi[0] - lo[0])
shift = mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation(
    (-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -float(all_v[:, 2].min())))
for o in parts:
    o.data.transform(shift)
    for p in o.data.polygons:
        p.use_smooth = True


def split(obj, name, keep):
    """Moves the faces of `obj` whose centre passes `keep(centre, normal)` into a new object."""
    out = obj.copy()
    out.data = obj.data.copy()
    out.name = out.data.name = name
    bpy.context.scene.collection.objects.link(out)
    for me, inside in ((obj.data, True), (out.data, False)):
        bm = bmesh.new()
        bm.from_mesh(me)
        bmesh.ops.delete(bm, geom=[f for f in bm.faces if keep(f.calc_center_median(), f.normal) == inside], context='FACES')
        bm.to_mesh(me)
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


# --- The left banner leaves the platform part, then the platform alone is stretched front to
# back to the square footprint (its floor is flat, so what stands on it stays on it).
left_banner = split(by_name[LEFT_BANNER_IN], 'banner-l',
                    lambda c, n: c.x < -5.5 * S and -7.05 * S < c.y < -6.1 * S
                    and (c.z > 2.25 * S or (-8.65 * S < c.x < -7.7 * S and c.z > 1.76 * S and n.z < 0.8)))
pv = np.concatenate([verts(by_name[n]) for n in PLATFORM])
depth = float(pv[:, 1].max() - pv[:, 1].min())
STRETCH = WIDTH / depth
for n in PLATFORM:
    by_name[n].data.transform(mathutils.Matrix.Diagonal((1.0, STRETCH, 1.0, 1.0)))
# The banners ride with the floor under them (moved, not stretched), keeping their place
# behind the front rim as in the concept.
for banner in (left_banner, by_name[RIGHT_BANNER]):
    bv = verts(banner)
    banner.data.transform(mathutils.Matrix.Translation((0, float(bv[:, 1].mean()) * (STRETCH - 1.0), 0)))
print('platform depth', round(depth, 2), 'stretch', round(STRETCH, 3))

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
team = bpy.data.images.new('bastion-team', TW, TH, alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
ATLAS.name = 'TC_bastion'
ATLAS['teamColorMask'] = 'baseColorAlpha'
print('team mask mean', round(float(mask.mean()), 3))


def raster(obj, mark):
    """Sets `mark` true on the texels under every UV triangle of `obj`."""
    me = obj.data
    uvl = me.uv_layers.active.data
    for poly in me.polygons:
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


# --- Active glow (D65): the tanks' dark steel and feed pipes glow violet while it works;
# their brass bands and caps stay metal. Only texels under the towers' faces light up.
towers_px = np.zeros((TH, TW), bool)
for n in TOWERS.values():
    raster(by_name[n], towers_px)
others_px = np.zeros((TH, TW), bool)
for o in parts:
    if o.name not in TOWERS.values():
        raster(o, others_px)
lit = towers_px & ~others_px
print('tank texels', int(towers_px.sum()), 'shared with other parts', int((towers_px & others_px).sum()))
for _ in range(2):  # a little bleed so mips keep the edges lit
    lit = lit | np.roll(lit, 1, 0) | np.roll(lit, -1, 0) | np.roll(lit, 1, 1) | np.roll(lit, -1, 1)
lit &= ~(others_px & ~towers_px)
weight = lit * np.clip((0.22 - lum) / 0.12, 0.0, 1.0) * (1.0 - mask)
glow = weight[..., None] * np.array(VIOLET, np.float32)
img = bpy.data.images.new('bastion-glow', TW, TH, alpha=False)
img.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
img.pack()
tex = ATLAS.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = img
bsdf = ATLAS.node_tree.nodes['Principled BSDF']
ATLAS.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 3.5  # big tanks: below the forge's 5, so they stay violet, not white
ATLAS['activeGlow'] = {'pulse': 0.9, 'flow': 0.0}

# --- The head with its inner fangs: everything of the body before the thorax and below the
# shell's front rim. It hinges at the top of its neck, under the rim, so the cut stays hidden.
HEAD_Y, HEAD_Z, HEAD_X = -5.3 * S, 5.5 * S, 2.2 * S
head = split(by_name[BODY], 'head', lambda c, n: c.y < HEAD_Y and c.z < HEAD_Z and abs(c.x) < HEAD_X)
hv = verts(head)
HINGE = mathutils.Vector((0.0, float(hv[:, 1].max()) - 0.3 * S, float(hv[:, 2].max()) - 0.3 * S))
set_origin(head, HINGE)
print('head faces', len(head.data.polygons), 'hinge', [round(v, 2) for v in HINGE],
      'y', hv[:, 1].min().round(2), hv[:, 1].max().round(2), 'z', hv[:, 2].min().round(2), hv[:, 2].max().round(2))

# --- The great outer claws: hinged where they leave the head (their highest, rearmost end),
# they part sideways about Z, so the gate opens across the RTS camera's view.
mandibles = {}
for side, n in MANDIBLES.items():
    m = by_name[n]
    m.name = m.data.name = 'mandible-' + side
    mv = verts(m)
    root = mv[mv[:, 2] > mv[:, 2].max() - 0.6 * S]
    root = root[root[:, 1] > np.percentile(root[:, 1], 50)]
    set_origin(m, (float(root[:, 0].mean()), float(root[:, 1].mean()), float(root[:, 2].mean())))
    mandibles[side] = m
    tip = mv[np.argmin(mv[:, 2])]
    print('mandible', side, 'pivot', [round(v, 2) for v in m.location], 'tip', tip.round(2))

# --- The shell: the rest of the body, breathing about its own foot.
shell = by_name[BODY]
shell.name = shell.data.name = 'shell'
sv = verts(shell)
SHELL_FOOT = mathutils.Vector((0.0, float(np.median(sv[:, 1])), float(np.percentile(sv[:, 2], 3))))
SHELL_TOP = float(sv[:, 2].max())
set_origin(shell, SHELL_FOOT)

# --- Everything else is one static node.
moving = {head, shell} | set(mandibles.values())
tower_v = {k: verts(by_name[n]) for k, n in TOWERS.items()}
static = [o for o in bpy.context.scene.objects if o.type == 'MESH' and o not in moving]
bastion = join(static, 'bastion')

# --- Sockets.
surface = mathutils.bvhtree.BVHTree.FromObject(bastion, bpy.context.evaluated_depsgraph_get())


def ground(x, y, top=30.0):
    hit = surface.ray_cast(mathutils.Vector((x, y, top)), mathutils.Vector((0, 0, -1)))[0]
    return hit.z if hit else 0.0


for side, tv in tower_v.items():
    cx, cy = float((tv[:, 0].min() + tv[:, 0].max()) / 2), float((tv[:, 1].min() + tv[:, 1].max()) / 2)
    # The towers' feed pipes reach in towards the shell; the tank itself is the outer bulk.
    outer = tv[np.abs(tv[:, 0]) > np.abs(cx) - 0.3 * S]
    cx = float((outer[:, 0].min() + outer[:, 0].max()) / 2)
    cy = float((outer[:, 1].min() + outer[:, 1].max()) / 2)
    kit.socket('socket_tower_' + side, (cx, cy, ground(cx, cy) + 0.15 * S))
    # The motes rise off the tank's face towards the camera, at mid height.
    front = float(outer[:, 1].min())
    kit.socket('socket_tank_' + side, (cx, front + 0.2 * S, 3.6 * S))
fang_tip = float(hv[:, 1].min())
kit.socket('socket_gate', (0.0, fang_tip - 0.4 * S, ground(0.0, fang_tip - 0.4 * S) + 0.1 * S))
kit.socket('socket_glow', (0.0, fang_tip + 0.6 * S, 3.2 * S))
RIM = {'a': (-6.0, -9.0), 'b': (9.15, 2.5), 'c': (5.5, -9.0), 'd': (-9.15, 4.5)}  # on the rim's top
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


def tracks(frames, part=None, lift=None, breathe=None, snap=False):
    """Every moving node's channels over `frames`; None holds the rest pose. `part` opens the
    claws (radians, outward), `lift` raises the head's chin (radians), `breathe` swells the
    shell (share of its height)."""
    rest = [(0, 0.0), (frames, 0.0)]
    interp = 'LINEAR' if snap else None
    out = []
    for side, m in mandibles.items():
        sign = -1.0 if side == 'l' else 1.0  # about Z: the left claw turns clockwise to open
        out.append((m, 'rotation_euler', 2, [(f, sign * v) for f, v in (part or rest)], None))
        # The claws lift as they part (about X: negative raises the points, which lie ahead of
        # and below the hinge), so they rise off the floor like a gate swinging up and out.
        out.append((m, 'rotation_euler', 0, [(f, -0.45 * v) for f, v in (part or rest)], None))
    out.append((head, 'rotation_euler', 0, [(f, -v) for f, v in (lift or rest)], interp))
    out.append((shell, 'scale', 2, [(f, 1.0 + v) for f, v in (breathe or rest)], None))
    out.append((shell, 'scale', 0, [(f, 1.0 + 0.4 * v) for f, v in (breathe or rest)], None))
    out.append((shell, 'scale', 1, [(f, 1.0 + 0.4 * v) for f, v in (breathe or rest)], None))
    return out


clip('idle', tracks(2))

# work (6 s): the claws part slowly, stand open working a little, and close; the head lifts
# its chin with them; the shell breathes three times. Rivet sparks round the rim, alternating
# between two pairs of sockets.
W = 180
OPEN = 0.6
part = [(0, 0.0), (45, OPEN), (70, OPEN * 0.85), (95, OPEN), (120, OPEN * 0.85), (140, OPEN), (W, 0.0)]
lift = [(0, 0.0), (45, 0.22), (95, 0.17), (140, 0.22), (W, 0.0)]
BREATH = 0.03
breathe = [(0, 0.0)]
for i in range(3):
    breathe += [(i * 60 + 30, BREATH), (i * 60 + 60, 0.0)]
RIVETS = [12, 72, 132]
RIVETS_2 = [42, 102, 162]
clip('work', tracks(W, part, lift, breathe))

# spawn (2 s): the claws snap open wide and the head rears (steam bursts at the gate on `gate`)
# as a Tinker comes out; then they settle shut.
SP, GATE = 60, 2
WIDE = 1.0
clip('spawn', tracks(SP, part=[(0, 0.0), (6, WIDE), (12, WIDE * 0.93), (38, WIDE), (SP, 0.0)],
                     lift=[(0, 0.0), (6, 0.4), (38, 0.36), (SP, 0.0)],
                     breathe=[(0, 0.0), (8, BREATH * 1.3), (SP, 0.0)]))

all_pts = np.concatenate([verts(o) for o in bpy.context.scene.objects if o.type == 'MESH'])
height = max(float(all_pts[:, 2].max()), SHELL_FOOT.z + (SHELL_TOP - SHELL_FOOT.z) * (1 + BREATH * 1.3))
tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
print('triangles', tris, 'height', round(height, 2), 'extent x', all_pts[:, 0].min().round(2), all_pts[:, 0].max().round(2),
      'y', all_pts[:, 1].min().round(2), all_pts[:, 1].max().round(2))

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Bastion', 'kind': 'building',
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
             'events': sorted([{'name': 'rivet', 'time': round(f / kit.FPS, 3)} for f in RIVETS]
                              + [{'name': 'rivet-2', 'time': round(f / kit.FPS, 3)} for f in RIVETS_2],
                              key=lambda e: e['time'])},
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
               'concept for Under the Canopy; animated by tools/buildings/bastion.py.',
}
for o in bpy.data.objects:
    if o.name.startswith('socket_'):
        print('socket', o.name, [round(v, 2) for v in o.location])
kit.export(OUT, NAME, asset)
