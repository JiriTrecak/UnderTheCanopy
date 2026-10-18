"""The beetles' Brood Den (supply, and a pressure node of the substance network, D66) from
Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/brood_den.py -- \\
        <beetle-brood-den-segmented.glb>

The den is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-brood-den-idle.png): a ring of low sleeping bunks under royal-blue
wing-case roofs around a domed boiler-house crowned with a brass pressure gauge, a valve
wheel and a short vent stack. Tripo's segmentation split it into parts that share one
texture atlas, so it stays one material. Here (D65):
- the atlas gets the team mask (the blue wing-case roofs and the front plaque) in its alpha;
- the slab is stretched front to back to fill the square footprint (Tripo's is 7 % short);
- the gauge's dial gets an active violet back-light: dark at rest, lit while it works;
- the painted needle is wiped from the dial and replaced by a real one, `gauge-needle`, that
  turns about the dial's own axis; the valve wheel is its own node, `valve-wheel`, turning
  about its own axis where it sits on the dome;
- `idle` is still. A den never trains or researches, so `work` only plays if the network
  drives the den's state (pressurising): the wheel is turned open, the needle swings up to
  the top of the dial, quivers there while the stack vents (`vent` event), then the wheel
  is closed and the needle falls back;
- sockets: `socket_vent` over the stack, `socket_gauge` before the dial.
At rest nothing moves and nothing glows."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
DEN = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-brood-den'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 8.0  # fills its 8-cell footprint: buildings have no walkable perimeter
S = WIDTH / 8.0  # offsets below are in metres at the 8-metre build

# Segmented parts by role (tripo_part_<n>, this export only).
GAUGE = 'tripo_part_8'  # the pressure gauge on its post, dial facing the front (-Y)
WHEEL = 'tripo_part_11'  # the valve wheel, tilted on the dome's front slope
STACK = 'tripo_part_12'  # the short vent stack behind the gauge
RIGID = {GAUGE, WHEEL}  # round things: moved by the stretch, never deformed
# Static: 0 the slab, 1 the dome, 2-7/9/10/13-15 the bunks (13 and 15 one bunk), 17 the
# block behind the gauge, 16/18-20 small trims.

kit.reset()
bpy.ops.import_scene.gltf(filepath=DEN)
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
# is the den's own iron slab; there is no dirt to strip.
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
height = (hi.z - lo.z) * scale
# Fill the square footprint: the slab is 7 % shorter front to back than across; the whole
# den stretches along Y (too little to see), the round gauge and wheel only move.
gain = WIDTH / ((hi.y - lo.y) * scale)
for o in parts:
    if o.name in RIGID:
        cy = sum(v.co.y for v in o.data.vertices) / len(o.data.vertices)
        for v in o.data.vertices:
            v.co.y += cy * (gain - 1)
    else:
        for v in o.data.vertices:
            v.co.y *= gain
    o.data.update()
print('stretch', round(gain, 3))

ATLAS = parts[0].data.materials[0]  # every part shares one material and atlas
assert all(o.data.materials[0] == ATLAS for o in parts), 'expected one shared atlas'


def texture_of(mat, socket='Base Color'):
    bsdf = mat.node_tree.nodes['Principled BSDF']
    link = bsdf.inputs[socket].links
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


node = texture_of(ATLAS)
H, W = node.image.size[1], node.image.size[0]
px = np.array(node.image.pixels[:], dtype=np.float32).reshape(H, W, 4)
rgb = px[:, :, :3].copy()
LUM = np.array([0.2126, 0.7152, 0.0722], np.float32)


def plane_frame(points):
    """Centre, normal and in-plane axes of a flat set of points (least squares)."""
    p = np.array(points)
    c = p.mean(axis=0)
    _, _, vt = np.linalg.svd(p - c)
    return c, vt[2], vt[0], vt[1]


def tilt_euler(n):
    """Euler (YXZ) angles that turn local +Y onto `n`: rotation (a, spin, c) then spins a
    node about `n` by keying its Y angle alone."""
    n = mathutils.Vector(n).normalized()
    a = math.asin(max(-1.0, min(1.0, n.z)))
    c = math.atan2(-n.x, n.y)
    return a, c


def set_tilted_origin(obj, origin, n):
    """Moves `obj`'s node origin to `origin`, its local +Y along `n` (mesh unmoved)."""
    a, c = tilt_euler(n)
    rot = mathutils.Euler((a, 0.0, c), 'YXZ').to_matrix().to_4x4()
    obj.data.transform(rot.inverted() @ mathutils.Matrix.Translation(-mathutils.Vector(origin)))
    obj.rotation_mode = 'YXZ'  # glTF imports use quaternions; the clips key Euler angles
    obj.location = origin
    obj.rotation_euler = (a, 0.0, c)
    return a, c


# --- The gauge dial: the front faces of the gauge in its dial plane.
gauge = by_name[GAUGE]
uvs = gauge.data.uv_layers.active.data


def face_rgb(me, poly):
    uvl = me.uv_layers.active.data
    uv = np.mean([tuple(uvl[i].uv) for i in poly.loop_indices], axis=0)
    return rgb[min(H - 1, int(uv[1] * H)), min(W - 1, int(uv[0] * W))]


cream = [p for p in gauge.data.polygons if p.normal.y < -0.8 and float(face_rgb(gauge.data, p) @ LUM) > 0.3
         and float(np.ptp(face_rgb(gauge.data, p))) < 0.18]
dial_c, dial_n, _, _ = plane_frame([tuple(gauge.data.vertices[i].co) for p in cream for i in p.vertices])
if dial_n[1] < 0:
    dial_n = -dial_n  # into the dial, away from the viewer: positive spin turns clockwise
dial_n = mathutils.Vector(dial_n)
plane_d = lambda co: (mathutils.Vector(co) - mathutils.Vector(dial_c)).dot(dial_n)  # noqa: E731
dial_faces = [p for p in gauge.data.polygons if p.normal.dot(-dial_n) > 0.8 and abs(plane_d(p.center)) < 0.05 * S]
dial_pts = [gauge.data.vertices[i].co for p in dial_faces for i in p.vertices]
# The dial's centre: the middle of its extent in its own plane.
a_, c_ = tilt_euler(dial_n)
to_local = mathutils.Euler((a_, 0.0, c_), 'YXZ').to_matrix().inverted()
loc = [to_local @ (co - mathutils.Vector(dial_c)) for co in dial_pts]
mid = mathutils.Vector(((min(v.x for v in loc) + max(v.x for v in loc)) / 2, 0,
                        (min(v.z for v in loc) + max(v.z for v in loc)) / 2))
DIAL = mathutils.Vector(dial_c) + to_local.inverted() @ mid
DIAL_R = min(max(v.x for v in loc) - min(v.x for v in loc), max(v.z for v in loc) - min(v.z for v in loc)) / 2
print('dial', [round(v, 2) for v in DIAL], 'r', round(DIAL_R, 2), 'normal', [round(v, 2) for v in dial_n],
      'faces', len(dial_faces))

# Tripo modelled the painted needle as a low ridge on the dial: press it flat into the dial.
flat = 0
for v in gauge.data.vertices:
    rel = v.co - DIAL
    d = rel.dot(dial_n)
    r = (rel - dial_n * d).length
    if (r < 0.8 * DIAL_R and -0.16 * S < d < -0.004 * S) or (r < 0.93 * DIAL_R and -0.1 * S < d < -0.004 * S):
        v.co -= dial_n * d
        flat += 1
gauge.data.update()
print('needle ridge vertices flattened', flat)


def raster(me, polys, fn):
    """Calls fn(ys, xs, inside, world points) for the texels under each face's UV triangles."""
    uvl = me.uv_layers.active.data
    for poly in polys:
        idx = list(poly.loop_indices)
        for k in range(1, len(idx) - 1):
            tri = [idx[0], idx[k], idx[k + 1]]
            (u0, v0), (u1, v1), (u2, v2) = [(uvl[i].uv[0] * W, uvl[i].uv[1] * H) for i in tri]
            p0, p1, p2 = [np.array(me.vertices[me.loops[i].vertex_index].co) for i in tri]
            x0, x1 = int(max(0, min(u0, u1, u2) - 1)), int(min(W - 1, max(u0, u1, u2) + 1))
            y0, y1 = int(max(0, min(v0, v1, v2) - 1)), int(min(H - 1, max(v0, v1, v2) + 1))
            xs, ys = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
            d = (v1 - v2) * (u0 - u2) + (u2 - u1) * (v0 - v2)
            if abs(d) < 1e-9:
                continue
            a = ((v1 - v2) * (xs - u2) + (u2 - u1) * (ys - v2)) / d
            b = ((v2 - v0) * (xs - u2) + (u0 - u2) * (ys - v2)) / d
            g = 1 - a - b
            inside = (a > -0.03) & (b > -0.03) & (g > -0.03)
            world = a[..., None] * p0 + b[..., None] * p1 + g[..., None] * p2
            fn(slice(y0, y1 + 1), slice(x0, x1 + 1), inside, world)


# Wipe the painted needle (and the ragged shadow Tripo painted round the rim): dark texels
# of the dial become its plain cream, so the real needle is the only one.
dial_mask = np.zeros((H, W), bool)
near_centre = np.zeros((H, W), bool)
centre = np.array(DIAL)


def mark(ys, xs, inside, world):
    dial_mask[ys, xs] |= inside
    off = world - centre
    radial = np.linalg.norm(off - np.outer(off.reshape(-1, 3) @ np.array(dial_n), np.array(dial_n)).reshape(off.shape),
                            axis=-1)
    near_centre[ys, xs] |= inside & (radial < 0.97 * DIAL_R)


# Every face before the dial, near its plane and inside its rim (the painted needle spans
# faces whose own texel is dark, so they are found by place, not by colour).
dial_faces = [p for p in gauge.data.polygons if p.normal.dot(-dial_n) > 0.6 and abs(plane_d(p.center)) < 0.12 * S
              and ((p.center - DIAL) - dial_n * (p.center - DIAL).dot(dial_n)).length < 1.05 * DIAL_R]
raster(gauge.data, dial_faces, mark)
lum = rgb @ LUM
cream_rgb = np.median(rgb[dial_mask & (lum > 0.35)], axis=0)
needle_px = near_centre & (lum < float(cream_rgb @ LUM) * 0.93)
rgb[needle_px] = cream_rgb
print('dial texels', int(dial_mask.sum()), 'needle texels wiped', int(needle_px.sum()), 'cream', cream_rgb.round(2))
# The flattened ridge's faces take a plain cream texel deep inside the dial's texels.
plain = dial_mask & (np.abs(rgb - cream_rgb).max(axis=2) < 0.02)
while True:
    inner = plain & np.roll(plain, 1, 0) & np.roll(plain, -1, 0) & np.roll(plain, 1, 1) & np.roll(plain, -1, 1)
    if not inner.any():
        break
    plain = inner
cy, cx = [int(np.median(a)) for a in np.nonzero(plain)]
cy, cx = min(zip(*np.nonzero(plain)), key=lambda t: (t[0] - cy) ** 2 + (t[1] - cx) ** 2)
CREAM_UV = ((cx + 0.5) / W, (cy + 0.5) / H)
uvl = gauge.data.uv_layers.active.data
ridge = 0
for p in gauge.data.polygons:
    rel = [gauge.data.vertices[i].co - DIAL for i in p.vertices]
    if all(abs(r.dot(dial_n)) < 0.003 * S and (r - dial_n * r.dot(dial_n)).length < 0.93 * DIAL_R for r in rel) \
            and float(face_rgb(gauge.data, p) @ LUM) < float(cream_rgb @ LUM) * 0.93:
        for i in p.loop_indices:
            uvl[i].uv = CREAM_UV
        ridge += 1
print('ridge faces re-mapped to cream', ridge)

# --- Team colour: one shared atlas, so its alpha takes the mask once for every part.
blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08)
team = bpy.data.images.new('brood-den-team', W, H, alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
ATLAS.name = 'TC_brood_den'
ATLAS['teamColorMask'] = 'baseColorAlpha'
print('team mask mean', round(float(mask.mean()), 3))

# --- Active glow on the one material: the dial is back-lit violet while the den works
# (a few texels of bleed so mips keep its edge lit); everything else stays dark.
lit = dial_mask.copy()
for _ in range(2):
    lit = lit | np.roll(lit, 1, 0) | np.roll(lit, -1, 0) | np.roll(lit, 1, 1) | np.roll(lit, -1, 1)
weight = lit * np.clip((rgb @ LUM - 0.2) / 0.2, 0.0, 1.0)
glow = weight[..., None] * np.array((0.55, 0.32, 1.0), np.float32)
img = bpy.data.images.new('brood-den-glow', W, H, alpha=False)
img.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
img.pack()
tex = ATLAS.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = img
bsdf = ATLAS.node_tree.nodes['Principled BSDF']
ATLAS.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 3.0
ATLAS['activeGlow'] = {'pulse': 0.5, 'flow': 0.0}

# --- The needle: a bold dark-iron pointer with a brass-capped hub, in the atlas (its UVs
# sit on a flat dark texel, outside the dial and the team mask).
blur = lum.copy()
for _ in range(4):
    blur = (blur + np.roll(blur, 2, 0) + np.roll(blur, -2, 0) + np.roll(blur, 2, 1) + np.roll(blur, -2, 1)) / 5
var = np.abs(lum - blur)
score = np.abs(blur - 0.05) + var * 4 + mask * 10 + lit * 10
iy, ix = np.unravel_index(np.argmin(score[8:-8, 8:-8]), score[8:-8, 8:-8].shape)
DARK_UV = ((ix + 8.5) / W, (iy + 8.5) / H)
print('needle texel', rgb[iy + 8, ix + 8].round(3))

L, WID, T, GAP = 0.85 * DIAL_R, 0.2 * DIAL_R, 0.035 * S, 0.06 * S  # length, width, thickness, off the dial
bm = bmesh.new()
outline = [(0, L), (WID / 2, 0.08 * L), (WID * 0.6, -0.12 * L), (0, -0.28 * L), (-WID * 0.6, -0.12 * L),
           (-WID / 2, 0.08 * L)]
front = [bm.verts.new((x, -GAP - T, z)) for x, z in outline]
back = [bm.verts.new((x, -GAP, z)) for x, z in outline]
bm.faces.new(front)
bm.faces.new(list(reversed(back)))
for i in range(len(outline)):
    j = (i + 1) % len(outline)
    bm.faces.new([front[i], back[i], back[j], front[j]])
hub_r = 0.13 * DIAL_R
ring = 12
hf = [bm.verts.new((math.cos(2 * math.pi * k / ring) * hub_r, -GAP - T * 1.8, math.sin(2 * math.pi * k / ring) * hub_r))
      for k in range(ring)]
hb = [bm.verts.new((v.co.x, -GAP * 0.5, v.co.z)) for v in hf]
bm.faces.new(list(reversed(hf)))
for k in range(ring):
    j = (k + 1) % ring
    bm.faces.new([hf[k], hf[j], hb[j], hb[k]])
bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
uv_layer = bm.loops.layers.uv.verify()
for f in bm.faces:
    for lp in f.loops:
        lp[uv_layer].uv = DARK_UV
me = bpy.data.meshes.new('gauge-needle')
bm.to_mesh(me)
bm.free()
needle = kit.link(bpy.data.objects.new('gauge-needle', me))
needle.data.materials.append(ATLAS)
a_n, c_n = tilt_euler(dial_n)
needle.rotation_mode = 'YXZ'
needle.location = DIAL
needle.rotation_euler = (a_n, 0.0, c_n)

# --- The valve wheel: its own node, turning about its own axis on the dome.
wheel = by_name[WHEEL]
wc, wn, e1, e2 = plane_frame([tuple(v.co) for v in wheel.data.vertices])
wv = np.array([tuple(v.co) for v in wheel.data.vertices]) - wc
w1, w2 = wv @ e1, wv @ e2
wc = wc + e1 * (w1.min() + w1.max()) / 2 + e2 * (w2.min() + w2.max()) / 2
if wn[2] < 0:
    wn = -wn  # out of the dome
wheel.name = wheel.data.name = 'valve-wheel'
a_w, c_w = set_tilted_origin(wheel, mathutils.Vector(wc), mathutils.Vector(wn))
print('wheel', [round(v, 2) for v in wc], 'axis', [round(v, 2) for v in wn],
      'diameter', round(float(max(np.ptp(w1), np.ptp(w2))), 2))

# --- Sockets.
stack = by_name[STACK]
sv = [v.co for v in stack.data.vertices]
stop = max(v.z for v in sv)
srim = [v for v in sv if v.z > stop - 0.12 * S]
kit.socket('socket_vent', (sum(v.x for v in srim) / len(srim), sum(v.y for v in srim) / len(srim), stop + 0.25 * S))
kit.socket('socket_gauge', DIAL - dial_n * 0.6 * S)


# --- Clips: every moving node keys every clip (one slot each), so blends stay clean.
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
    """One action `name` keying several nodes: tracks are (node, path, index, [(frame, value)])."""
    action = bpy.data.actions.new(name)
    for obj, path, index, keys in tracks:
        obj.animation_data_create()
        obj.animation_data.action = action
        for f, value in keys:
            getattr(obj, path)[index] = value
            obj.keyframe_insert(data_path=path, index=index, frame=f)
        for fc in slot_curves(action, obj):
            for kp in fc.keyframe_points:
                kp.interpolation = 'BEZIER'
        obj.animation_data.action = None
    for obj in dict.fromkeys(t[0] for t in tracks):
        track = obj.animation_data.nla_tracks.new()
        track.name = name
        track.strips.new(name, 0, action)
    return action


# Angles about each node's own axis (local Y). The needle: positive turns it clockwise as
# seen from the front; it rests at ten o'clock and is driven past three o'clock.
NEEDLE_REST, NEEDLE_PEAK = math.radians(-60), math.radians(105)
WHEEL_REST = 0.0
F = 120  # 4 s loop
VENT = 50  # the needle at the top: the stack vents


def held(obj, frames):
    return [(obj, 'rotation_euler', 1, [(0, 0.0 if obj is wheel else NEEDLE_REST),
                                        (frames, 0.0 if obj is wheel else NEEDLE_REST)])]


clip('idle', held(needle, 2) + held(wheel, 2))
q = math.radians(5)
clip('work', [
    # The wheel is spun open (one full turn), held, and spun shut again.
    (wheel, 'rotation_euler', 1, [(0, WHEEL_REST), (8, WHEEL_REST), (40, WHEEL_REST + 2 * math.pi),
                                  (78, WHEEL_REST + 2 * math.pi), (110, WHEEL_REST), (F, WHEEL_REST)]),
    # The needle follows the wheel up, overshoots, quivers at the top, then falls back.
    (needle, 'rotation_euler', 1, [(0, NEEDLE_REST), (14, NEEDLE_REST), (44, NEEDLE_PEAK + q), (VENT, NEEDLE_PEAK - q),
                                   (56, NEEDLE_PEAK + q * 0.6), (62, NEEDLE_PEAK - q * 0.6),
                                   (68, NEEDLE_PEAK + q * 0.4), (74, NEEDLE_PEAK - q * 0.3), (80, NEEDLE_PEAK),
                                   (108, NEEDLE_REST - q * 0.6), (114, NEEDLE_REST), (F, NEEDLE_REST)]),
])
# The tilt angles are constant: key them in every clip too, so sampled export keeps them.
for name in ('idle', 'work'):
    action = bpy.data.actions[name]
    end = 2 if name == 'idle' else F
    for obj, (a, c) in ((needle, (a_n, c_n)), (wheel, (a_w, c_w))):
        obj.animation_data.action = action
        for f in (0, end):
            obj.rotation_euler[0], obj.rotation_euler[2] = a, c
            obj.keyframe_insert(data_path='rotation_euler', index=0, frame=f)
            obj.keyframe_insert(data_path='rotation_euler', index=2, frame=f)
        obj.animation_data.action = None
needle.rotation_euler = (a_n, NEEDLE_REST, c_n)
wheel.rotation_euler = (a_w, WHEEL_REST, c_w)

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Brood Den', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask', 'supply', 'network-node'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': ['TC_brood_den']},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            # Event times are seconds into the clip.
            {'semantic': 'work', 'clip': 'work', 'loop': True,
             'events': [{'name': 'vent', 'time': round(VENT / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [8, 8], 'offset': [0, 0]},
        'vegetationClearance': 4,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/buildings/brood_den.py.',
}
kit.export(OUT, NAME, asset)
