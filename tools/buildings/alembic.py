"""The beetles' Alembic from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/alembic.py -- \\
        <beetle-alembic-segmented.glb>

The Alembic is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-alembic-idle.png), split by Tripo's segmentation into parts that share one
texture atlas: a tall glass distillation column under a royal-blue dome, a stacked coil spire on
top, four condenser flasks linked to the column by brass tubes, two banners, a stepped stone
base. Beetle specialists learn to handle substance here. Here (D65):
- Tripo made the base 1.6 times wider than deep; the 12-cell footprint is square, so the
  stone platform's front and back bands are stretched outward to fill it while the round
  machinery (column, dome, flasks, spire) keeps its true shape and the banners move rigidly;
- the royal-blue dome, plates and banners become the team mask in the atlas's alpha;
- the column's glass is its own node with its own UVs: dark glass at rest; at work it fills
  with swirling violet substance streaming upward (`flow` scrolls V, which the atlas cannot);
- the condensers' glass glows and pulses in the atlas's active emission;
- the coil spire comes apart: its four rings and the crown are their own nodes; at work
  they rise apart like a drawn spring, turning against each other, and a violet core inside
  shows through the gaps; arcs crackle out from the crown;
- `spawn` (a specialist comes out): the spire jolts up, every arc fires, `discharge` at its
  peak (the discharge from the spire and the flash at the door hang on it);
- sockets for the state effects: the spire's crown, the column, two tube elbows, the door."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ALEMBIC = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-alembic'
OUT = os.environ.get('ALEMBIC_OUT') or os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 12.0  # fills its 12-cell footprint across X: buildings have no walkable perimeter
S = WIDTH / 12.0  # the offsets below were set on the 12-metre build
VIOLET = (0.55, 0.32, 1.0)

# Segmented parts by role (tripo_part_<n>; stable only for this one export).
DOME = 'tripo_part_0'      # the dome and the column's upper body, with the column's glass
COLUMN_FOOT = 'tripo_part_1'  # the column's foot ring and the front steps
SPIRE = 'tripo_part_6'     # the coil spire: collar, four coil rings, crown
CONDENSERS = ['tripo_part_7', 'tripo_part_3', 'tripo_part_11', 'tripo_part_5']  # LF, LB (and its tube), RF, RB
TUBE_L = 'tripo_part_3'    # its top is the left tube's elbow
TUBE_R = 'tripo_part_16'   # the right front condenser's tube elbow
BANNERS = ['tripo_part_9', 'tripo_part_12', 'tripo_part_20']  # right banner and its foot, left banner
# Stone base 2, 4, 8, 10, 13; small trim and rings the rest.

# The footprint stretch (metres at the 12-metre build, before stretching): the machinery
# stands within FRONT of the centre to the front and BACK to the back; the platform beyond
# is stretched so its edges reach EDGE.
FRONT, BACK, EDGE = 2.45 * S, 2.7 * S, 5.4 * S

kit.reset()
bpy.ops.import_scene.gltf(filepath=ALEMBIC)
parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']
# Normalise: footprint centred, ground at 0, WIDTH across; transforms applied.
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        for c in o.children:
            m = c.matrix_world.copy()
            c.parent = None
            c.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
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

# --- Fill the square footprint: stretch the platform's front and back bands outward.
ys = [v.co.y for o in parts for v in o.data.vertices]
gain_front = (EDGE - FRONT) / (-min(ys) - FRONT)
gain_back = (EDGE - BACK) / (max(ys) - BACK)


def stretch(y):
    if y < -FRONT:
        return -FRONT + (y + FRONT) * gain_front
    if y > BACK:
        return BACK + (y - BACK) * gain_back
    return y


for o in parts:
    if o.name in BANNERS:  # thin flags and poles move whole, never thicken
        cy = sum(v.co.y for v in o.data.vertices) / len(o.data.vertices)
        d = stretch(cy) - cy
        for v in o.data.vertices:
            v.co.y += d
    else:
        for v in o.data.vertices:
            v.co.y = stretch(v.co.y)
    o.data.update()
print('stretch gains', round(gain_front, 2), round(gain_back, 2))

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


# --- Team colour: the blue dome, plates and banners become the mask in the atlas's alpha
# (once: every part shares the one material).
node = texture_of(ATLAS)
px = pixels(node.image)
RGB = px[:, :, :3].copy()
TH, TW = RGB.shape[:2]
blue = RGB[:, :, 2] - np.maximum(RGB[:, :, 0], RGB[:, :, 1])
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (RGB[:, :, 2] > 0.08)
team = bpy.data.images.new('alembic-team', TW, TH, alpha=True)
team.pixels[:] = np.concatenate([RGB, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
ATLAS.name = 'TC_alembic'
ATLAS['teamColorMask'] = 'baseColorAlpha'
team_slots = [ATLAS.name]
print('team mask', round(float(mask.mean()), 4))

lum = lambda rgb: rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32)  # noqa: E731


def face_colour(me, poly):
    """The atlas colour under a face: its corners and centre, averaged."""
    uv = me.uv_layers[0].data
    uvs = [uv[li].uv for li in poly.loop_indices]
    uvs.append(sum(uvs, mathutils.Vector((0, 0))) / len(uvs))
    return np.mean([RGB[min(TH - 1, int(t[1] * TH)), min(TW - 1, int(t[0] * TW))] for t in uvs], axis=0)


def dark_glass(me, poly, lum_max=0.3):
    """Dark, colourless texels: glass (and the dark recess behind it), never brass or blue."""
    c = face_colour(me, poly)
    return float(lum(c)) < lum_max and float(c.max() - c.min()) < 0.06


def split(obj, name, keep):
    """Moves the faces of `obj` for which `keep(mesh, poly)` holds into a new object."""
    me = obj.data
    chosen = {p.index for p in me.polygons if keep(me, p)}
    out = obj.copy()
    out.data = me.copy()
    out.name = out.data.name = name
    bpy.context.scene.collection.objects.link(out)
    for target, drop in ((obj, chosen), (out, None)):
        bm = bmesh.new()
        bm.from_mesh(target.data)
        bm.faces.ensure_lookup_table()
        gone = [f for f in bm.faces if (f.index in chosen) == (drop is not None)]
        bmesh.ops.delete(bm, geom=gone, context='FACES')
        bm.to_mesh(target.data)
        bm.free()
    return out


def set_origin(obj, at):
    """Moves the node origin to `at` (world), keeping the mesh where it is."""
    at = mathutils.Vector(at)
    obj.data.transform(mathutils.Matrix.Translation(-at))
    obj.location = at
    obj.rotation_mode = 'XYZ'  # glTF imports use quaternions; the clips key Euler angles


def substance_texture(name, w=256, h=512, seed=5, base=0.32):
    """Swirling violet substance: bright veins over a fuller glow, tiling in U and V (so
    `flow` can stream it up the glass)."""
    rng = np.random.default_rng(seed)
    uu, vv = np.meshgrid(np.linspace(0, 1, w, endpoint=False), np.linspace(0, 1, h, endpoint=False))

    def waves(n, top, u, v):
        """A tiling field: random waves with whole frequencies in U and V."""
        out = np.zeros_like(u)
        for _ in range(n):
            ku, kv = int(rng.integers(-top, top + 1)), int(rng.integers(1, top + 1))
            out += np.sin(2 * np.pi * (ku * u + kv * v) + rng.uniform(0, 2 * np.pi)) / math.hypot(ku, kv)
        return out / n

    # Domain-warped (warp keeps whole periods: offsets from tiling fields), so it swirls.
    du, dv = waves(10, 3, uu, vv), waves(10, 3, uu, vv)
    field = waves(16, 4, uu + 0.6 * du, vv + 0.6 * dv)
    field = (field - field.min()) / (field.max() - field.min())
    veins = np.exp(-((field - 0.5) / 0.045) ** 2) + 0.6 * np.exp(-((field - 0.25) / 0.035) ** 2)
    swell = 0.7 + 0.3 * np.sin(2 * np.pi * (vv * 2 + 0.5 * du))  # denser and thinner bands rise
    glow = np.clip((base + 0.9 * veins) * swell, 0, 1.3)[..., None]
    rgb = glow * np.array(VIOLET, np.float32) + np.clip(veins - 0.6, 0, 1)[..., None] * 0.35  # hot cores whiten
    img = bpy.data.images.new(name, w, h, alpha=False)
    img.pixels[:] = np.concatenate([rgb, np.ones((h, w, 1), np.float32)], axis=2).ravel()
    img.pack()
    return img


def glass_material(name, emission, pulse, flow, tex):
    """Dark smooth glass at rest; substance light only while the Alembic works."""
    mat = kit.material(name, color=(0.03, 0.035, 0.05), roughness=0.12, metallic=0.0)
    nt = mat.node_tree
    t = nt.nodes.new('ShaderNodeTexImage')
    t.image = tex
    bsdf = nt.nodes['Principled BSDF']
    nt.links.new(t.outputs['Color'], bsdf.inputs['Emission Color'])
    bsdf.inputs['Emission Strength'].default_value = emission
    mat['activeGlow'] = {'pulse': pulse, 'flow': flow}
    return mat


# --- The column's glass: its own node, flat dark glass with its own planar UVs (U across the
# window, V down its height, so a positive `flow` streams the substance upward: glTF flips V).
def in_window(me, p):
    c, n = p.center, p.normal
    return abs(c.x) < 0.95 * S and 3.2 * S < c.z < 7.6 * S and n.y < -0.05 and dark_glass(me, p)


glass = split(by_name[DOME], 'column-glass', in_window)
gv = np.array([v.co[:] for v in glass.data.vertices])
g_lo, g_hi = gv.min(0), gv.max(0)
uvl = glass.data.uv_layers[0].data
V_TILES = 2.0
for poly in glass.data.polygons:
    for li in poly.loop_indices:
        co = glass.data.vertices[glass.data.loops[li].vertex_index].co
        uvl[li].uv = ((co.x - g_lo[0]) / (g_hi[0] - g_lo[0]),
                      V_TILES * (1.0 - (co.z - g_lo[2]) / (g_hi[2] - g_lo[2])))
glass.data.materials.clear()
glass.data.materials.append(glass_material('column-substance', 4.5, 0.6, 0.22, substance_texture('column-substance-glow')))
print('column glass faces', len(glass.data.polygons), 'x', g_lo[0].round(2), g_hi[0].round(2),
      'z', g_lo[2].round(2), g_hi[2].round(2), 'front', g_lo[1].round(2))

# --- The condensers' glass: alive in the atlas's emission (no flow there), pulsing.
GLOW = np.zeros((TH, TW), np.float32)


def glow_faces(obj, faces, weight):
    """Adds the texels under `faces` of `obj`, weighted by `weight(rgb)`, to the atlas's
    active emission."""
    me = obj.data
    uv = me.uv_layers[0].data
    hit = np.zeros((TH, TW), bool)
    for poly in faces:
        tri = np.array([uv[li].uv[:] for li in poly.loop_indices]) * (TW, TH)
        for k in range(1, len(tri) - 1):
            a, b, c = tri[0], tri[k], tri[k + 1]
            x0, y0 = np.floor(np.minimum(np.minimum(a, b), c)).astype(int) - 2
            x1, y1 = np.ceil(np.maximum(np.maximum(a, b), c)).astype(int) + 2
            x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, TW - 1), min(y1, TH - 1)
            if x1 < x0 or y1 < y0:
                continue
            xs, ys_ = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
            d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
            if abs(d) < 1e-9:
                continue
            l1 = ((b[1] - c[1]) * (xs - c[0]) + (c[0] - b[0]) * (ys_ - c[1])) / d
            l2 = ((c[1] - a[1]) * (xs - c[0]) + (a[0] - c[0]) * (ys_ - c[1])) / d
            e = 2.0 / max(1.0, np.hypot(*(b - a)))  # a texel or two of margin against mip seams
            inside = (l1 >= -e) & (l2 >= -e) & (1 - l1 - l2 >= -e)
            hit[y0:y1 + 1, x0:x1 + 1] |= inside
    GLOW[:] = np.maximum(GLOW, hit * weight(RGB))


glass_weight = lambda c: np.clip((0.34 - lum(c)) / 0.12, 0.0, 1.0) * (c.max(axis=2) - c.min(axis=2) < 0.1)  # noqa: E731
for name in CONDENSERS:
    o = by_name[name]
    faces = [p for p in o.data.polygons if abs(p.normal.z) < 0.4 and dark_glass(o.data, p) and p.center.z < 5.2 * S]
    glow_faces(o, faces, glass_weight)
    print('condenser glass', name, len(faces))
glow_rgb = GLOW[..., None] * np.array(VIOLET, np.float32)
glow_img = bpy.data.images.new('alembic-glow', TW, TH, alpha=False)
glow_img.pixels[:] = np.concatenate([glow_rgb, np.ones_like(glow_rgb[:, :, :1])], axis=2).ravel()
glow_img.scale(TW // 2, TH // 2)
glow_img.pack()
glow_node = ATLAS.node_tree.nodes.new('ShaderNodeTexImage')
glow_node.image = glow_img
atlas_bsdf = ATLAS.node_tree.nodes['Principled BSDF']
ATLAS.node_tree.links.new(glow_node.outputs['Color'], atlas_bsdf.inputs['Emission Color'])
atlas_bsdf.inputs['Emission Strength'].default_value = 6.0
ATLAS['activeGlow'] = {'pulse': 1.1, 'flow': 0.0}

# --- The coil spire: the collar stays on the dome; four coil rings and the crown become
# their own nodes, turning about the spire's axis.
spire = by_name[SPIRE]
sv = np.array([v.co[:] for v in spire.data.vertices])
AXIS = mathutils.Vector(((sv[:, 0].min() + sv[:, 0].max()) / 2, (sv[:, 1].min() + sv[:, 1].max()) / 2, 0))
CUTS = [z * S for z in (9.25, 9.55, 9.85, 10.15, 10.55)]  # collar | ring 0..3 | crown
rings = []
for k in range(4):
    z0, z1 = CUTS[k], CUTS[k + 1]
    ring = split(spire, f'coil-ring-{k}', lambda me, p, z0=z0, z1=z1: z0 <= p.center.z < z1)
    set_origin(ring, (AXIS.x, AXIS.y, z0))
    rings.append(ring)
crown = split(spire, 'coil-crown', lambda me, p: p.center.z >= CUTS[-1])
set_origin(crown, (AXIS.x, AXIS.y, CUTS[-1]))
CROWN_TOP = max(v.co.z for v in crown.data.vertices) + CUTS[-1]
print('spire axis', [round(v, 2) for v in AXIS], 'rings', [len(r.data.polygons) for r in rings],
      'crown', len(crown.data.polygons), 'top', round(CROWN_TOP, 2))

# The violet core inside the coil: hidden by the rings at rest, showing through the gaps as
# they draw apart (its height follows the crown).
CORE_R, CORE_Z0 = 0.36 * S, 9.0 * S
core_h = CUTS[-1] - CORE_Z0
core = kit.cylinder('coil-core', CORE_R, core_h, at=(AXIS.x, AXIS.y, CORE_Z0), verts=20)
core.rotation_mode = 'XYZ'
kit.cube_uv(core, 0.7 * S)  # V runs up its sides, so `flow` streams the light
core.data.materials.append(glass_material('coil-core', 7.0, 2.2, 0.9, substance_texture('coil-core-glow', 128, 128, seed=9, base=0.55)))
for p in core.data.polygons:
    p.use_smooth = True

# --- Arcs: jagged violet bolts out from the crown, three directions, two shapes each, so the
# flicker never repeats; they ride the crown (same lift and turn).
arc_mat = kit.material('alembic-arc', color=(0.75, 0.62, 1.0), roughness=0.4)
arc_bsdf = arc_mat.node_tree.nodes['Principled BSDF']
arc_bsdf.inputs['Emission Color'].default_value = (0.62, 0.42, 1.0, 1.0)
arc_bsdf.inputs['Emission Strength'].default_value = 9.0
arc_mat['activeGlow'] = {'pulse': 3.0, 'flow': 0.0}
arc_mat['noShadow'] = True
CROWN_BULB = 11.15 * S - CUTS[-1]  # the crown's bulb, above its foot


def bolt(name, angle, seed, reach, drop, radius=0.07 * S, steps=8):
    """A jagged arc from the crown's bulb out and down at `angle` (radians about the axis)."""
    rng = np.random.default_rng(seed)
    out = mathutils.Vector((math.cos(angle), math.sin(angle), 0))
    side = mathutils.Vector((-out.y, out.x, 0))
    start = out * 0.38 * S + mathutils.Vector((0, 0, CROWN_BULB))
    end = out * reach + mathutils.Vector((0, 0, CROWN_BULB - drop))
    ctrl = (start + end) / 2 + mathutils.Vector((0, 0, 0.45 * S)) + side * rng.uniform(-0.3, 0.3) * S
    pts = []
    for i in range(steps + 1):
        t = i / steps
        p = (1 - t) ** 2 * start + 2 * (1 - t) * t * ctrl + t ** 2 * end
        if 0 < i < steps:
            p += (side * rng.uniform(-1, 1) * 0.16 + mathutils.Vector((0, 0, rng.uniform(-1, 1) * 0.14))) * S
        pts.append(p)
    bm = bmesh.new()
    loops = []
    for i, p in enumerate(pts):
        t = (pts[min(i + 1, steps)] - pts[max(i - 1, 0)]).normalized()
        n = t.cross(mathutils.Vector((0, 0, 1)))
        n = (n if n.length > 1e-4 else t.cross(mathutils.Vector((1, 0, 0)))).normalized()
        b = t.cross(n).normalized()
        r = radius * (1.0 - 0.75 * i / steps)
        loops.append([bm.verts.new(p + (n * math.cos(a) + b * math.sin(a)) * r)
                      for a in (k * 2 * math.pi / 3 for k in range(3))])
    for r0, r1 in zip(loops, loops[1:]):
        for j in range(3):
            bm.faces.new([r0[j], r0[(j + 1) % 3], r1[(j + 1) % 3], r1[j]])
    bm.faces.new(list(reversed(loops[0])))
    bm.faces.new(loops[-1])
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = kit.link(bpy.data.objects.new(name, me))
    obj.data.materials.append(arc_mat)
    obj.location = (AXIS.x, AXIS.y, CUTS[-1])
    obj.rotation_mode = 'XYZ'
    return obj


arcs = []
for i, base_angle in enumerate((-math.pi / 2 - 0.9, -math.pi / 2 + 0.9, math.pi / 2)):  # front-left, front-right, back
    for variant in range(2):
        a = base_angle + (0.35 if variant else -0.2)
        arcs.append(bolt(f'arc-{i}-{variant}', a, 17 + 10 * i + variant, (1.5 + 0.3 * variant) * S, (0.7 + 0.35 * variant) * S))

# --- Sockets.
SPIRE_LIFT = 1.0 * S  # how far the crown rises at work
kit.socket('socket_spire', (AXIS.x, AXIS.y, CROWN_TOP + SPIRE_LIFT * 0.6))
kit.socket('socket_column', ((g_lo[0] + g_hi[0]) / 2, g_lo[1] - 0.25 * S, g_lo[2] + 0.3 * (g_hi[2] - g_lo[2])))


def top_of(obj):
    vs = [v.co for v in obj.data.vertices]
    zmax = max(v.z for v in vs)
    ring = [v for v in vs if v.z > zmax - 0.2 * S]
    return mathutils.Vector((sum(v.x for v in ring) / len(ring), sum(v.y for v in ring) / len(ring), zmax))


kit.socket('socket_tube_l', top_of(by_name[TUBE_L]) + mathutils.Vector((0, 0, 0.15 * S)))
kit.socket('socket_tube_r', top_of(by_name[TUBE_R]) + mathutils.Vector((0, 0, 0.15 * S)))
foot = [v.co for v in by_name[COLUMN_FOOT].data.vertices if abs(v.co.x) < 0.8 * S]
# The door: the head of the steps, at the foot of the glass column (specialists come down
# the steps from it).
step_top = max(v.z for v in foot if v.y < -3.0 * S)
kit.socket('socket_door', (0.0, -3.0 * S, step_top + 0.4 * S))
print('door', round(step_top, 2))
bpy.context.view_layer.update()
all_pts = [o.matrix_world @ v.co for o in bpy.context.scene.objects if o.type == 'MESH' for v in o.data.vertices]
kit.socket('socket_spawn', (0.0, min(p.y for p in all_pts) + 0.8 * S, 0.1))

# --- Clips: every moving node keys every clip, so blends between clips stay clean.
OFF = 0.001  # arcs collapsed into the crown


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
    """One action `name` keying several nodes (one slot each): tracks are
    (node, path, indices, [(frame, value)], interpolation or None for Bezier)."""
    action = bpy.data.actions.new(name)
    for obj, path, indices, keys, interp in tracks:
        obj.animation_data_create()
        obj.animation_data.action = action
        for f, value in keys:
            for i in indices:
                getattr(obj, path)[i] = value
                obj.keyframe_insert(data_path=path, index=i, frame=f)
        for fc in slot_curves(action, obj):
            if fc.data_path == path:
                for kp in fc.keyframe_points:
                    kp.interpolation = interp or 'BEZIER'
        obj.animation_data.action = None
    for obj in dict.fromkeys(t[0] for t in tracks):
        track = obj.animation_data.nla_tracks.new()
        track.name = name
        track.strips.new(name, 0, action)
    return action


RING_Z = [r.location.z for r in rings]
CROWN_Z = crown.location.z
RING_STEP = SPIRE_LIFT / 5  # ring k rises (k+1)/5 of the crown's lift


TWIST = 0.6  # radians: the rings' widest turn (see below)


def spire_tracks(lift, twist):
    """The spire drawn apart by `lift` keys (frame, share of SPIRE_LIFT); the rings turn
    against each other and the crown by `twist` keys (frame, share of TWIST). The game
    blends clips (work into spawn or idle, at any moment) by mixing joint matrices
    linearly, which shrinks a node by cos(angle / 2) midway: turns stay small."""
    tracks = []
    for k, ring in enumerate(rings):
        tracks.append((ring, 'location', [2], [(f, RING_Z[k] + RING_STEP * (k + 1) * s) for f, s in lift], None))
        sign = 1 if k % 2 == 0 else -1
        tracks.append((ring, 'rotation_euler', [2], [(f, sign * TWIST * s) for f, s in twist], None))
    crown_keys = [(f, CROWN_Z + SPIRE_LIFT * s) for f, s in lift]
    crown_turn = [(f, -TWIST * s) for f, s in twist]
    tracks.append((crown, 'location', [2], crown_keys, None))
    tracks.append((crown, 'rotation_euler', [2], crown_turn, None))
    tracks.append((core, 'scale', [2], [(f, 1.0 + SPIRE_LIFT * s / core_h) for f, s in lift], None))
    for a in arcs:
        tracks.append((a, 'location', [2], crown_keys, None))
        tracks.append((a, 'rotation_euler', [2], crown_turn, None))
    return tracks


def flicker(frames, seed, on_share, lo=3, hi=7):
    """Constant on/off keys for an arc: short bursts at pseudo-random frames."""
    rng = np.random.default_rng(seed)
    keys, f = [(0, OFF)], int(rng.integers(14, 24))
    while f < frames - 26:
        length = int(rng.integers(lo, hi))
        keys += [(f, float(rng.uniform(0.85, 1.0))), (f + length, OFF)]
        f += length + int(rng.integers(2, max(3, int(length / on_share))))
    keys.append((frames, OFF))
    return keys


clip('idle', spire_tracks([(0, 0.0), (2, 0.0)], [(0, 0.0), (2, 0.0)])
     + [(a, 'scale', [0, 1, 2], [(0, OFF), (2, OFF)], 'CONSTANT') for a in arcs])
W = 120  # 4 s: the spire draws apart, hums open with a slow breath, and closes again
clip('work', spire_tracks([(0, 0.0), (24, 1.0), (44, 0.82), (62, 1.0), (82, 0.84), (98, 1.0), (W, 0.0)],
                          [(0, 0.0), (30, 1.0), (60, 0.1), (90, 1.0), (W, 0.0)])
     + [(a, 'scale', [0, 1, 2], flicker(W, 41 + i, 0.5), 'CONSTANT') for i, a in enumerate(arcs)])

SP = 45  # 1.5 s: a specialist is done; the spire jolts open and discharges
DISCHARGE = 9


def spawn_arc(i):
    """Every arc fires through the discharge: each direction alternates its two shapes."""
    keys = [(0, OFF)]
    for k, f in enumerate(range(5, 29, 4)):
        keys.append((f, 1.0 if (k + i) % 2 == 0 else OFF))
    return keys + [(30, OFF), (SP, OFF)]


clip('spawn', spire_tracks([(0, 0.0), (DISCHARGE, 1.25), (16, 0.95), (26, 1.05), (SP, 0.0)],
                           [(0, 0.0), (DISCHARGE, 1.0), (24, 0.5), (SP, 0.0)])
     + [(a, 'scale', [0, 1, 2], spawn_arc(i), 'CONSTANT') for i, a in enumerate(arcs)])

tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
height = max(p.z for p in all_pts)
print('triangles', tris, 'height', round(height, 2),
      'footprint', round(min(p.x for p in all_pts), 2), round(max(p.x for p in all_pts), 2),
      round(min(p.y for p in all_pts), 2), round(max(p.y for p in all_pts), 2))

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Alembic', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': team_slots},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            {'semantic': 'work', 'clip': 'work', 'loop': True, 'events': []},
            # Event times are seconds into the clip.
            {'semantic': 'spawn', 'clip': 'spawn', 'loop': False,
             'events': [{'name': 'discharge', 'time': round(DISCHARGE / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [12, 12], 'offset': [0, 0]},
        'vegetationClearance': 6,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + SPIRE_LIFT + 0.6, 1)},  # over the opened spire
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; stretched to its footprint and animated by '
               'tools/buildings/alembic.py.',
}
kit.export(OUT, NAME, asset)
