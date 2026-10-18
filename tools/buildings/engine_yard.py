"""The beetles' Engine Yard from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/engine_yard.py -- \\
        <beetle-engine-yard-segmented.glb>

The yard is Tripo's image-to-3D model of the approved concept (art/styles/beetle-engine-yard-idle.png),
split by Tripo's segmentation into parts that share one texture atlas. Here (D65):
- the royal-blue plates (hangar shell, gantry banners, base insets) become the team mask in
  the atlas's alpha, once for the whole shared material;
- the substance tank's glass gets active emission (dark at rest, blue-violet while working),
  mapped in UV space from the glass band of the tank so its brass straps and rims stay dark;
- the gantry trolley slides along its beam, the crane claw (its own node under the trolley)
  drops onto the assembly cradle, clamps and rises (`weld` events for the sparks), and the big
  cog on the right turns; `spawn` opens the claw and spins the cog when a machine rolls out;
- the beam the trolley leaves is filled with a copy of the beam's own next stretch, and the
  claw hangs from a hoist rod that hides in the trolley at rest;
- the back-left corner Tripo left empty is filled with the base's right edge, mirrored, so
  the yard fills its footprint;
- sockets for the state effects: the tank glass, the tank's steam nozzle, the claw's jaws
  (animated, so the sparks follow it) and the hangar mouth."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
YARD = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-engine-yard'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 16.0  # fills its 16-cell footprint: buildings have no walkable perimeter
S = WIDTH / 16.0  # the offsets below were set at 16 metres

# Segmented parts by role (tripo_part_<n>; numbers hold for this one export).
HANGAR, CRADLE, BASE_RIGHT = 'tripo_part_0', 'tripo_part_1', 'tripo_part_3'
GANTRY_LEFT, GANTRY_RIGHT = 'tripo_part_2', 'tripo_part_6'  # each a post and half the beam;
# the left half also carries the trolley block (blue chevron pendant) and the hanging claw
COG, TANK = 'tripo_part_4', 'tripo_part_5'  # cog mount tripo_part_7, base blocks 8, 9, 10
BURIED = ['tripo_part_11', 'tripo_part_12', 'tripo_part_14']  # floor chips at z 0 under the base

kit.reset()
bpy.ops.import_scene.gltf(filepath=YARD)
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        for c in o.children:
            m = c.matrix_world.copy()
            c.parent = None
            c.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
for name in BURIED:
    bpy.data.objects.remove(bpy.data.objects[name], do_unlink=True)
parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']
# Normalise: footprint centred, ground at 0, WIDTH across; transforms applied.
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
HEIGHT = (hi.z - lo.z) * scale
by_name = {o.name: o for o in parts}
MAT = parts[0].data.materials[0]  # the one shared atlas material


# --- Mesh helpers ----------------------------------------------------------------------

def split(obj, keep, name):
    """Moves the faces of `obj` whose (centre, bmesh face) satisfy `keep` into a new object."""
    new = obj.copy()
    new.data = obj.data.copy()
    new.name = new.data.name = name
    bpy.context.scene.collection.objects.link(new)
    for target, wanted in ((new, False), (obj, True)):
        bm = bmesh.new()
        bm.from_mesh(target.data)
        doomed = [f for f in bm.faces if keep(f.calc_center_median(), f) == wanted]
        bmesh.ops.delete(bm, geom=doomed, context='FACES')
        bm.to_mesh(target.data)
        bm.free()
    return new


def bisect(obj, x, only=lambda c: True):
    """Cuts the faces of `obj` that `only` selects along the plane x = const."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    faces = [f for f in bm.faces if only(f.calc_center_median())]
    geom = list({e for f in faces for e in f.edges}) + faces + list({v for f in faces for v in f.verts})
    bmesh.ops.bisect_plane(bm, geom=geom, plane_co=(x, 0, 0), plane_no=(1, 0, 0))
    bm.to_mesh(obj.data)
    bm.free()


def clip_box(obj, lo_, hi_):
    """Keeps only the part of `obj` inside the axis box, cutting faces at its walls."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    for axis in range(3):
        for value, sign in ((lo_[axis], -1), (hi_[axis], 1)):
            if value is None:
                continue
            co, no = [0, 0, 0], [0, 0, 0]
            co[axis], no[axis] = value, sign
            geom = bm.verts[:] + bm.edges[:] + bm.faces[:]
            bmesh.ops.bisect_plane(bm, geom=geom, plane_co=co, plane_no=no, clear_outer=True)
    bm.to_mesh(obj.data)
    bm.free()


def bounds(obj):
    vs = [v.co for v in obj.data.vertices]
    return (mathutils.Vector([min(v[i] for v in vs) for i in range(3)]),
            mathutils.Vector([max(v[i] for v in vs) for i in range(3)]))


def set_pivot(obj, pivot, parent=None):
    """Moves the node origin to `pivot` (world) without moving the mesh; optional parent."""
    obj.data.transform(mathutils.Matrix.Translation(-pivot))
    obj.rotation_mode = 'XYZ'  # glTF imports use quaternions; the clips key Euler angles
    if parent is None:
        obj.location = pivot
    else:
        obj.parent = parent
        obj.matrix_parent_inverse = mathutils.Matrix.Identity(4)
        obj.location = pivot - parent_world(parent)
    return obj


def parent_world(obj):
    return obj.location if obj.parent is None else obj.location + parent_world(obj.parent)


def uv_at(obj, point):
    """The UV of the face of `obj` nearest `point`: a texel of that surface."""
    me = obj.data
    best = min(me.polygons, key=lambda p: (p.center - point).length)
    uvl = me.uv_layers.active.data
    return mathutils.Vector(np.mean([uvl[i].uv[:] for i in best.loop_indices], axis=0))


# --- The back-left corner: Tripo left x < -6, y > 1 empty. The base's right edge, mirrored,
# fills it with the same stone blocks and brass caps.
fill = by_name[BASE_RIGHT].copy()
fill.data = by_name[BASE_RIGHT].data.copy()
fill.name = fill.data.name = 'base-back-left'
bpy.context.scene.collection.objects.link(fill)
fill.data.transform(mathutils.Matrix.Scale(-1, 4, (1, 0, 0)))
bm = bmesh.new()
bm.from_mesh(fill.data)
bmesh.ops.reverse_faces(bm, faces=bm.faces[:])
bm.to_mesh(fill.data)
bm.free()
clip_box(fill, (None, 1.0 * S, None), (-5.95 * S, None, None))
parts.append(fill)

# --- Team colour: blue plates become the mask in the shared atlas's alpha, once.
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


node = texture_of(MAT)
px = pixels(node.image)
rgb = px[:, :, :3]
blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08)
team = bpy.data.images.new('engine-yard-team', node.image.size[0], node.image.size[1], alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
MAT.name = 'TC_engine_yard'
MAT['teamColorMask'] = 'baseColorAlpha'
team_slots = [MAT.name]
print('team mask mean', round(float(mask.mean()), 3))

# --- The tank's glass: the shared material gets an emission map that lights only the glass:
# atlas texels whose surface lies in the glass band of the tank (found by rasterising the
# tank's triangles in UV space) and that are dark (glass, not brass straps). Every surface has
# its own atlas island, so the rest of the yard samples black and one material stays enough.
lum = lambda c: c @ np.array([0.2126, 0.7152, 0.0722], np.float32)  # noqa: E731
tank = by_name[TANK]
GLASS = (3.8 * S, 6.4 * S)  # its height on the tank, between the brass rings
EDGE = 0.15 * S  # the glow fades in over this at the glass's top and bottom
band = [p for p in tank.data.polygons if GLASS[0] < p.center.z < GLASS[1]]
axis = mathutils.Vector((np.mean([p.center.x for p in band]), np.mean([p.center.y for p in band]), 0))
radius = max(math.hypot(p.center.x - axis.x, p.center.y - axis.y) for p in band)
G = 512  # the emission map's size: a soft glow needs no more


def uv_mask(obj, size, inside):
    """Per texel, `inside` (0..1) of its surface point, interpolated over the triangle."""
    out = np.zeros((size, size), np.float32)
    me = obj.data
    me.calc_loop_triangles()
    uvl = me.uv_layers.active.data
    for tri in me.loop_triangles:
        uv = np.array([uvl[i].uv[:] for i in tri.loops]) * size - 0.5
        pos = np.array([me.vertices[v].co[:] for v in tri.vertices])
        (x0, y0), (x1, y1) = np.floor(uv.min(0)).astype(int), np.ceil(uv.max(0)).astype(int)
        xs, ys = np.meshgrid(np.arange(max(x0, 0), min(x1, size - 1) + 1), np.arange(max(y0, 0), min(y1, size - 1) + 1))
        (ax, ay), (bx, by), (cx, cy) = uv
        den = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
        if abs(den) < 1e-12:
            continue
        l0 = ((by - cy) * (xs - cx) + (cx - bx) * (ys - cy)) / den
        l1 = ((cy - ay) * (xs - cx) + (ax - cx) * (ys - cy)) / den
        l2 = 1 - l0 - l1
        on = (l0 > -0.05) & (l1 > -0.05) & (l2 > -0.05)  # a little over the edge: no seams
        p = l0[..., None] * pos[0] + l1[..., None] * pos[1] + l2[..., None] * pos[2]
        w = np.where(on, inside(p), 0.0)
        out[ys, xs] = np.maximum(out[ys, xs], w)
    return out


band_mask = uv_mask(tank, G, lambda p: np.clip((p[..., 2] - GLASS[0]) / EDGE, 0, 1)
                    * np.clip((GLASS[1] - p[..., 2]) / EDGE, 0, 1))
small = rgb.reshape(G, rgb.shape[0] // G, G, rgb.shape[1] // G, 3).mean(axis=(1, 3))
weight = band_mask * np.clip((0.16 - lum(small)) / 0.08, 0.0, 1.0)
# Texels any other surface samples stay dark, so no glow bleeds onto a neighbouring island.
others = np.zeros_like(weight)
for o in parts:
    if o is not tank:
        others = np.maximum(others, uv_mask(o, G, lambda p: np.ones(p.shape[:-1])))
print('glass texels shared with other surfaces', int(((weight > 0.05) & (others > 0)).sum()))
weight *= 1.0 - others
print('glass texels', int((weight > 0.5).sum()), 'axis', [round(v, 2) for v in axis], 'radius', round(radius, 2))
glow = weight[..., None] * np.array((0.55, 0.32, 1.0), np.float32)
gimg = bpy.data.images.new('tank-glow', G, G, alpha=False)
gimg.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
gimg.pack()
tex = MAT.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = gimg
bsdf = MAT.node_tree.nodes['Principled BSDF']
MAT.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 5.0
MAT['activeGlow'] = {'pulse': 0.9, 'flow': 0.0}

# --- The trolley: the block on the beam with its chevron pendant, cut clean from the beam.
left = by_name[GANTRY_LEFT]
T0, T1 = -0.25 * S, 1.8 * S
high = lambda c: c.z > 7.25 * S  # noqa: E731
bisect(left, T0, high)
trolley = split(left, lambda c, f: high(c) and T0 < c.x < T1 and c.y < -2.2 * S, 'trolley')
claw = split(left, lambda c, f: not high(c) and c.x > -0.4 * S and -4.25 * S < c.y < -3.35 * S and c.z > 4.0 * S,
             'claw')
tlo, thi = bounds(trolley)
clo, chi = bounds(claw)
print('trolley', [round(v, 2) for v in tlo], [round(v, 2) for v in thi], 'claw', [round(v, 2) for v in clo],
      [round(v, 2) for v in chi])

# The beam behind the trolley: the next stretch of the right half's beam, moved over.
right = by_name[GANTRY_RIGHT]
rlo, _ = bounds(right)
beam_x0 = rlo.x + 0.02 * S
span = beam_x0 - T0
beam = right.copy()
beam.data = right.data.copy()
beam.name = beam.data.name = 'beam-fill'
bpy.context.scene.collection.objects.link(beam)
clip_box(beam, (beam_x0, -4.4 * S, 8.5 * S), (beam_x0 + span, -3.2 * S, 9.8 * S))
beam.data.transform(mathutils.Matrix.Translation((-span, 0, 0)))
print('beam fill', len(beam.data.polygons), 'faces over', round(T0, 2), round(beam_x0, 2))

# Pivots: the trolley at its own centre on the beam, the claw at its hook (top), the jaws at
# their hinges on the hub, the cog at its axle.
CX, CY = (clo.x + chi.x) / 2, (clo.y + chi.y) / 2
HUB = 5.3 * S
jaw_l = split(claw, lambda c, f: c.z < HUB and c.x < CX, 'jaw-left')
jaw_r = split(claw, lambda c, f: c.z < HUB and c.x >= CX, 'jaw-right')
set_pivot(trolley, mathutils.Vector((CX, CY, 8.9 * S)))
set_pivot(claw, mathutils.Vector((CX, CY, chi.z)), trolley)
set_pivot(jaw_l, mathutils.Vector((CX - 0.12 * S, CY, HUB - 0.1 * S)), claw)
set_pivot(jaw_r, mathutils.Vector((CX + 0.12 * S, CY, HUB - 0.1 * S)), claw)
# The hoist rod: from the hook up into the trolley, so the claw never hangs on nothing.
ROD = 2.0 * S
steel = uv_at(right, mathutils.Vector((3.0 * S, -4.3 * S, 9.15 * S)))
rod = kit.cylinder('hoist-rod', 0.07 * S, ROD, verts=12)
rod.data.materials.append(MAT)
rod.data.uv_layers.new()
for d in rod.data.uv_layers.active.data:
    d.uv = steel
rod.parent = claw
rod.matrix_parent_inverse = mathutils.Matrix.Identity(4)
rod.location = (0, 0, -0.1 * S)
for p in rod.data.polygons:
    p.use_smooth = True

cog = by_name[COG]
glo, ghi = bounds(cog)
set_pivot(cog, mathutils.Vector(((glo.x + ghi.x) / 2, (glo.y + ghi.y) / 2, (glo.z + ghi.z) / 2)))
cog.name = 'cog'
print('cog', [round(v, 2) for v in cog.location], 'diameter', round(ghi.z - glo.z, 2))

# --- Sockets.
def top_of(obj):
    vs = [v.co for v in obj.data.vertices]
    zmax = max(v.z for v in vs)
    ring = [v for v in vs if v.z > zmax - 0.25 * S]
    return mathutils.Vector((sum(v.x for v in ring) / len(ring), sum(v.y for v in ring) / len(ring), zmax))


kit.socket('socket_steam', top_of(tank) + mathutils.Vector((0, 0, 0.3 * S)))
kit.socket('socket_tank', (axis.x, axis.y - radius - 0.4 * S, 5.0 * S))
hlo, hhi = bounds(by_name[HANGAR])
kit.socket('socket_hangar', ((hlo.x + hhi.x) / 2, hlo.y - 0.3 * S, 3.0 * S))
weld = kit.socket('socket_weld', (0, 0, 0))
weld.parent = claw
weld.matrix_parent_inverse = mathutils.Matrix.Identity(4)
weld.location = (0, 0, clo.z - chi.z + 0.2 * S)  # between the jaw tips

# --- Clips: one action per clip, a slot per moving node (merged into one glTF animation).
def clip(obj, name, keys, path, index, interpolation='BEZIER'):
    action = bpy.data.actions.get(name) or bpy.data.actions.new(name)
    ad = obj.animation_data_create()
    ad.action = action
    for frame, value in keys:
        getattr(obj, path)[index] = value
        obj.keyframe_insert(data_path=path, index=index, frame=frame)
    for fc in kit._fcurves(action):
        if fc.data_path == path and fc.array_index == index:
            for kp in fc.keyframe_points:
                kp.interpolation = interpolation
    track = ad.nla_tracks.new()
    track.name = name
    track.strips.new(name, int(keys[0][0]), action)
    ad.action = None


TX = trolley.location.x
CZ = claw.location.z
OPEN, SHUT = 0.0, 0.28  # jaw swing (left jaw negative, right positive, closes the tips)
F, FS = 120, 45
# The work cycle (4 s): over the cradle's right side, drop, clamp, weld twice, open and rise;
# across to its left side, drop, clamp, weld twice, open, rise and home.
RIGHT, LEFT = 1.3 * S, -0.6 * S
DROP_R, DROP_L = 1.6 * S, 1.35 * S
trolley_keys = [(0, TX), (16, TX + RIGHT), (60, TX + RIGHT), (80, TX + LEFT), (106, TX + LEFT), (F, TX)]
claw_keys = [(0, CZ), (14, CZ), (30, CZ - DROP_R), (50, CZ - DROP_R), (64, CZ), (78, CZ), (90, CZ - DROP_L),
             (102, CZ - DROP_L), (114, CZ), (F, CZ)]
jaw_keys = [(0, OPEN), (30, OPEN), (35, SHUT), (50, SHUT), (55, OPEN), (90, OPEN), (94, SHUT), (102, SHUT),
            (106, OPEN), (F, OPEN)]
WELDS = [37, 46, 95, 101]
# Spawn (1.5 s): the claw lifts and throws its jaws wide, the cog spins a fast full turn.
spawn_claw = [(0, CZ), (10, CZ + 0.45 * S), (30, CZ + 0.45 * S), (FS, CZ)]
spawn_jaw = [(0, OPEN), (8, OPEN - 0.35), (32, OPEN - 0.35), (FS, OPEN)]
clip(trolley, 'idle', [(0, TX), (2, TX)], 'location', 0)
clip(trolley, 'work', trolley_keys, 'location', 0)
clip(trolley, 'spawn', [(0, TX), (FS, TX)], 'location', 0)
clip(claw, 'idle', [(0, CZ), (2, CZ)], 'location', 2)
clip(claw, 'work', claw_keys, 'location', 2)
clip(claw, 'spawn', spawn_claw, 'location', 2)
for jaw, sign in ((jaw_l, -1), (jaw_r, 1)):
    clip(jaw, 'idle', [(0, OPEN), (2, OPEN)], 'rotation_euler', 1)
    clip(jaw, 'work', [(f, sign * v) for f, v in jaw_keys], 'rotation_euler', 1)
    clip(jaw, 'spawn', [(f, sign * v) for f, v in spawn_jaw], 'rotation_euler', 1)
clip(cog, 'idle', [(0, 0.0), (2, 0.0)], 'rotation_euler', 0)
clip(cog, 'work', [(0, 0.0), (F, -2 * math.pi)], 'rotation_euler', 0, 'LINEAR')
clip(cog, 'spawn', [(0, 0.0), (FS, -2 * math.pi)], 'rotation_euler', 0)

tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
print('triangles', tris)
asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Engine Yard', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': team_slots},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            {'semantic': 'work', 'clip': 'work', 'loop': True,
             # Event times are seconds into the clip.
             'events': [{'name': 'weld', 'time': round(f / kit.FPS, 3)} for f in WELDS]},
            {'semantic': 'spawn', 'clip': 'spawn', 'loop': False,
             'events': [{'name': 'release', 'time': round(3 / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [16, 16], 'offset': [0, 0]},
        'vegetationClearance': 8,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(HEIGHT + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/buildings/engine_yard.py.',
}
kit.export(OUT, NAME, asset)
