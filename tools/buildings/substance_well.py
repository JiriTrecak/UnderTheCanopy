"""The beetles' Substance Well from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/substance_well.py -- \\
        <beetle-substance-well-segmented.glb>

The well is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-substance-well-idle.png): a "tap crown" of riveted steel clamps with brass
spikes and a team-coloured ring, driven into a Substance root knot, with a tall glass column
in brass bands under a copper coil cap. Tripo's segmentation split it into parts that share
one texture atlas, though the knot, the ring and some clamps share parts. It stands exactly
on a Substance deposit (8 x 8 cells) and covers it: 12 x 12 cells. Here:
- the royal-blue ring and plates become the team mask in the atlas's alpha. The knot is
  violet, not blue (hue about 256 against the plates' 220), so the mask keeps to blue hues;
- the knot's cracks glow violet ALWAYS: raw substance is the forest's own wild magic, the
  one exception to D65 (docs/beetles.md). That is a plain emission on the atlas material
  (no `activeGlow`), limited to the texels under the knot's faces;
- the glass of the column is its own node and material. It rests dark; while workers
  gather it fills with a violet storm streaming up (`activeGlow` with `flow`): the atlas
  cannot stream, so the glass gets cylindrical UVs, its atlas textures baked onto them, and
  a tiling storm texture for its emission;
- the three copper rings of the coil cap are their own nodes: in `work` they turn (in
  alternating directions) and spring up and down with each pump stroke;
- the ten clamps are their own nodes: in `work` they bite down into the knot once per
  stroke, a ripple running round the crown;
- sockets for the state effects: the knot, the column's foot and middle, the coil cap."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
WELL = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-substance-well'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 12.0  # covers the 8-cell deposit within a 12-cell footprint; no walkable perimeter
S = WIDTH / 12.0  # the offsets below were set on the 12-metre build
VIOLET = (0.55, 0.32, 1.0)

# Segmented parts by role (tripo_part_<n>; stable only for this one export).
COLUMN = 'tripo_part_0'  # the glass column, its steel stanchions and brass bands
COLLAR = 'tripo_part_10'  # the cogged collar at the column's foot
CAP = 'tripo_part_7'  # the cap over the glass, under the coil
COIL_LOW = 'tripo_part_1'  # the lower two copper rings
COIL_TOP = 'tripo_part_23'  # the top copper ring
SPINDLE = 'tripo_part_24'  # the spindle and finial through the rings
# The knot and the blue ring are mixed through 2, 3, 6, 13, 27, 28 (and small bits); the
# square steel base frame is 4, 5, 8, 26, 16, 21, 32. The clamps, by compass direction
# (the building faces -Y, south): the parts that rise above the knot.
CLAMPS = {
    'e': ['tripo_part_29', 'tripo_part_15'], 'ne': ['tripo_part_12'], 'n': ['tripo_part_22', 'tripo_part_17'],
    'nnw': ['tripo_part_18'], 'wnw': ['tripo_part_20'], 'w': ['tripo_part_25'],
    'sw': ['tripo_part_11'], 's': [], 'sse': ['tripo_part_14'], 'se': ['tripo_part_19'],
}
S_CLAMP_FROM = 'tripo_part_9'  # the south clamp and the SSE clamp's wedge share their part
# with the ring's south arc

kit.reset()
bpy.ops.import_scene.gltf(filepath=WELL)
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
    """Moves the faces of `obj` that pass `keep(bmesh face, uv layer)` into a new object."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    out = obj.copy()
    out.data = obj.data.copy()
    out.name = out.data.name = name
    bpy.context.scene.collection.objects.link(out)
    uvl = bm.loops.layers.uv.active
    gone = [f for f in bm.faces if keep(f, uvl)]
    bmesh.ops.delete(bm, geom=gone, context='FACES')
    bm.to_mesh(obj.data)
    bm.free()
    bm = bmesh.new()
    bm.from_mesh(out.data)
    uvl = bm.loops.layers.uv.active
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if not keep(f, uvl)], context='FACES')
    bm.to_mesh(out.data)
    bm.free()
    return out


def join(objs, name):
    """Joins meshes into the first one, renamed."""
    for o in bpy.context.scene.objects:
        o.select_set(o in objs)
    bpy.context.view_layer.objects.active = objs[0]
    if len(objs) > 1:
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


node = texture_of(ATLAS)
px = pixels(node.image)
rgb = px[:, :, :3].copy()
H, W = rgb.shape[:2]
r_, g_, b_ = rgb[..., 0], rgb[..., 1], rgb[..., 2]
blueness = b_ - np.maximum(r_, g_)
hue = 240 + 60 * (r_ - g_) / np.maximum(rgb.max(-1) - rgb.min(-1), 1e-6)  # for blue-dominant texels
lum = lambda c: c @ np.array([0.2126, 0.7152, 0.0722], np.float32)  # noqa: E731


def raster(objs, uv_layer=0):
    """The atlas texels under the faces of `objs` (with a texel or two of margin)."""
    hit = np.zeros((H, W), bool)
    for obj in objs:
        me = obj.data
        uv = me.uv_layers[uv_layer].data
        for poly in me.polygons:
            tri = np.array([uv[li].uv[:] for li in poly.loop_indices]) * (W, H)
            for k in range(1, len(tri) - 1):
                a, b, c = tri[0], tri[k], tri[k + 1]
                x0, y0 = np.floor(np.minimum(np.minimum(a, b), c)).astype(int) - 2
                x1, y1 = np.ceil(np.maximum(np.maximum(a, b), c)).astype(int) + 2
                x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, W - 1), min(y1, H - 1)
                if x1 < x0 or y1 < y0:
                    continue
                xs, ys = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
                d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
                if abs(d) < 1e-9:
                    continue
                l1 = ((b[1] - c[1]) * (xs - c[0]) + (c[0] - b[0]) * (ys - c[1])) / d
                l2 = ((c[1] - a[1]) * (xs - c[0]) + (a[0] - c[0]) * (ys - c[1])) / d
                e = 2.0 / max(1.0, np.hypot(*(b - a)))
                hit[y0:y1 + 1, x0:x1 + 1] |= (l1 >= -e) & (l2 >= -e) & (1 - l1 - l2 >= -e)
    return hit


# --- The column's glass: the faces of the glass between the stanchions and bands (radius
# about 1.05-1.10 m against the stanchions' 1.1-1.5 m), not brass.
col = by_name[COLUMN]
cv = np.array([v.co[:] for v in col.data.vertices])
AXIS = mathutils.Vector((float((cv[:, 0].min() + cv[:, 0].max()) / 2), float((cv[:, 1].min() + cv[:, 1].max()) / 2), 0))


def texel_mean(f, uvl):
    cs = [rgb[int(l[uvl].uv[1] * H) % H, int(l[uvl].uv[0] * W) % W] for l in f.loops]
    return np.mean(cs, axis=0)


def is_glass(f, uvl):
    c = f.calc_center_median()
    if not (3.75 * S < c.z < 8.3 * S):
        return False
    r = math.hypot(c.x - AXIS.x, c.y - AXIS.y)
    if r > 1.2 * S:
        return False
    t = texel_mean(f, uvl)
    brass = t[0] > t[2] * 1.25 and t[0] > 0.12  # warm and bright: a band, not glass
    violet = t[2] - max(t[0], t[1]) > 0.02 and t[0] > t[1]
    # Glass sits at about 1.05-1.1 m; the stanchions stand out to 1.2 m in grey steel.
    return not brass and (r < 1.115 * S or violet)


glass = split(col, 'glass', is_glass)
gv = np.array([v.co[:] for v in glass.data.vertices])
G_Z0, G_Z1 = float(gv[:, 2].min()), float(gv[:, 2].max())
print('glass faces', len(glass.data.polygons), 'z', round(G_Z0, 2), round(G_Z1, 2))
V_SPAN = 1.0  # storm texture lengths over the glass's height


def cylinder_uv(obj, centre, z0, z1):
    """Cylindrical UVs: U round the axis, V from the top (0) to the foot (V_SPAN). The shader
    streams emission towards +V in glTF's flipped V, so features rise in Blender's terms."""
    me = obj.data
    old = me.uv_layers[0].name
    layer = me.uv_layers.new(name='glass')
    for poly in me.polygons:
        us, vs = [], []
        for li in poly.loop_indices:
            co = me.vertices[me.loops[li].vertex_index].co
            a = math.atan2(co.y - centre.y, co.x - centre.x)
            us.append((a % (2 * math.pi)) / (2 * math.pi))
            vs.append((z1 - co.z) / (z1 - z0) * V_SPAN)
        if max(us) - min(us) > 0.5:  # straddles the seam: keep the face whole
            us = [u + 1 if u < 0.5 else u for u in us]
        for li, u, v in zip(poly.loop_indices, us, vs):
            layer.data[li].uv = (u, v)
    return old, layer


old_uv, glass_uv = cylinder_uv(glass, AXIS, G_Z0, G_Z1)
glass_mat = ATLAS.copy()
glass_mat.name = 'glass'
glass.data.materials[0] = glass_mat


def bake(obj, mat, old, target_uv):
    """Bakes the atlas's base colour, roughness/metal and normal onto `obj`'s own UVs."""
    sc = bpy.context.scene
    sc.render.engine = 'CYCLES'
    sc.cycles.samples = 1
    sc.cycles.device = 'CPU'
    nt = mat.node_tree
    bsdf = nt.nodes['Principled BSDF']
    for n in list(nt.nodes):
        if n.type == 'TEX_IMAGE':
            uvn = nt.nodes.new('ShaderNodeUVMap')
            uvn.uv_map = old
            nt.links.new(uvn.outputs['UV'], n.inputs['Vector'])
        if n.type == 'NORMAL_MAP':
            n.uv_map = old
    obj.data.uv_layers.active = obj.data.uv_layers[target_uv]
    for o in bpy.context.scene.objects:
        o.select_set(o is obj)
    bpy.context.view_layer.objects.active = obj
    base_src = texture_of(mat)
    rm_src = [n for n in nt.nodes if n.type == 'TEX_IMAGE' and n.image and n.image.name.endswith('_rm')][0]
    out = {}
    for key, w, h, data in (('base', 1024, 512, False), ('rm', 512, 256, True), ('normal', 1024, 512, True)):
        img = bpy.data.images.new('glass-' + key, w, h, alpha=False)
        if data:
            img.colorspace_settings.name = 'Non-Color'
        target = nt.nodes.new('ShaderNodeTexImage')
        target.image = img
        for n in nt.nodes:
            n.select = False
        target.select = True
        nt.nodes.active = target
        if key == 'normal':
            bsdf.inputs['Emission Strength'].default_value = 0.0
            bpy.ops.object.bake(type='NORMAL', normal_space='TANGENT', margin=16, use_clear=True)
        else:
            src = base_src if key == 'base' else rm_src
            link = nt.links.new(src.outputs['Color'], bsdf.inputs['Emission Color'])
            bsdf.inputs['Emission Strength'].default_value = 1.0
            bpy.ops.object.bake(type='EMIT', margin=16, use_clear=True)
            nt.links.remove(link)
        img.pack()
        nt.nodes.remove(target)
        out[key] = img
    sc.render.engine = 'BLENDER_EEVEE'
    return out


baked = bake(glass, glass_mat, old_uv, 'glass')
glass.data.uv_layers.remove(glass.data.uv_layers[old_uv])
glass.data.uv_layers.active = glass.data.uv_layers['glass']
glass.data.uv_layers['glass'].active_render = True


def storm_texture(name, w=512, h=256, seed=5):
    """A violet storm in the glass, tiling in U and V (so `flow` can stream it up): thin
    bright filaments that writhe upwards with a twist, over a deep violet haze."""
    rng = np.random.default_rng(seed)
    uu, vv = np.meshgrid(np.linspace(0, 1, w, endpoint=False), np.linspace(0, 1, h, endpoint=False))
    veins = np.zeros((h, w), np.float32)
    for k, (ku, kv, width, amp) in enumerate(((5, 1, 0.022, 1.0), (7, -1, 0.018, 0.8), (3, 2, 0.03, 0.6),
                                              (11, 1, 0.014, 0.5))):
        p = rng.uniform(0, 2 * np.pi, 4)
        wobble = 0.22 * np.sin(2 * np.pi * (vv * 2 + uu * (k % 2)) + p[1]) \
            + 0.12 * np.sin(2 * np.pi * (vv * 3 - uu * 1) + p[2])
        phase = (uu * ku + vv * kv + wobble * ku / 5 + p[0] / (2 * np.pi)) % 1.0
        d = np.minimum(phase, 1.0 - phase)  # distance to the filament's centre line
        # Each filament brightens and fades along its length (so it reads as wisps).
        along = 0.5 + 0.5 * np.sin(2 * np.pi * (vv * 2 + uu * ku * 0.5) + p[3])
        veins += amp * np.exp(-(d / width) ** 2) * (0.25 + 0.75 * along ** 2)
    haze = 0.10 + 0.10 * (0.5 + 0.5 * np.sin(2 * np.pi * (uu * 2 + vv * 1) + 0.7))
    glow = haze[..., None] * np.array(VIOLET, np.float32) \
        + np.clip(veins, 0, 1.3)[..., None] * np.array((0.78, 0.6, 1.0), np.float32)
    img = bpy.data.images.new(name, w, h, alpha=False)
    img.pixels[:] = np.concatenate([glow, np.ones((h, w, 1), np.float32)], axis=2).ravel()
    img.pack()
    return img


nt = glass_mat.node_tree
for n in list(nt.nodes):
    if n.type in ('TEX_IMAGE', 'UVMAP', 'SEPARATE_COLOR', 'NORMAL_MAP'):
        nt.nodes.remove(n)
bsdf = nt.nodes['Principled BSDF']
base_tex = nt.nodes.new('ShaderNodeTexImage')
base_tex.image = baked['base']
nt.links.new(base_tex.outputs['Color'], bsdf.inputs['Base Color'])
rm_tex = nt.nodes.new('ShaderNodeTexImage')
rm_tex.image = baked['rm']
sep = nt.nodes.new('ShaderNodeSeparateColor')
nt.links.new(rm_tex.outputs['Color'], sep.inputs['Color'])
nt.links.new(sep.outputs['Green'], bsdf.inputs['Roughness'])
nt.links.new(sep.outputs['Blue'], bsdf.inputs['Metallic'])
nrm_tex = nt.nodes.new('ShaderNodeTexImage')
nrm_tex.image = baked['normal']
nm = nt.nodes.new('ShaderNodeNormalMap')
nt.links.new(nrm_tex.outputs['Color'], nm.inputs['Color'])
nt.links.new(nm.outputs['Normal'], bsdf.inputs['Normal'])
storm_tex = nt.nodes.new('ShaderNodeTexImage')
storm_tex.image = storm_texture('glass-storm')
nt.links.new(storm_tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 5.0
glass_mat['activeGlow'] = {'pulse': 0.5, 'flow': 0.45}  # 0.45 heights a second, rising
for k in [k for k in glass_mat.keys() if k != 'activeGlow']:
    del glass_mat[k]

# --- Team colour: the royal-blue ring and plates become the mask in the atlas's alpha.
hue_w = np.clip((242.0 - hue) / 8.0, 0.0, 1.0)  # 234 and below: blue plate; 242 and up: violet knot
mask = np.clip((blueness - 0.04) / 0.1, 0.0, 1.0) * (b_ > 0.08) * hue_w
team = bpy.data.images.new('well-team', W, H, alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
ATLAS.name = 'TC_well'
ATLAS['teamColorMask'] = 'baseColorAlpha'
team_slots = [ATLAS.name]
print('team mask', round(float(mask.mean()), 4))

# --- The knot: its violet cracks glow always (raw substance, docs/beetles.md). Plain emission
# on the atlas (no activeGlow), only on the texels under the knot's own faces: everything
# inside the ring, below the column, that is not the column.
NOT_KNOT = {COLUMN, COLLAR, CAP, COIL_LOW, COIL_TOP, SPINDLE}


def knot_faces(obj):
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    keep = []
    for f in bm.faces:
        c = f.calc_center_median()
        r = math.hypot(c.x - AXIS.x, c.y - AXIS.y)
        if 1.5 * S < r < 4.7 * S and c.z < 3.2 * S:
            keep.append(f.index)
    bm.free()
    return set(keep)


knot_hit = np.zeros((H, W), bool)
for o in parts:
    if o.name in NOT_KNOT or o is glass:
        continue
    idx = knot_faces(o)
    if not idx:
        continue
    tmp = o.copy()
    tmp.data = o.data.copy()
    bm = bmesh.new()
    bm.from_mesh(tmp.data)
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.index not in idx], context='FACES')
    bm.to_mesh(tmp.data)
    bm.free()
    knot_hit |= raster([tmp])
    bpy.data.meshes.remove(tmp.data)
violet_w = np.clip((hue - 244.0) / 6.0, 0.0, 1.0) * np.clip((blueness - 0.03) / 0.06, 0.0, 1.0)
bright = np.clip((b_ - 0.16) / 0.26, 0.0, 1.0)  # the cracks' bright veins over the dark knot
KNOT = knot_hit * violet_w * (0.04 + 0.96 * bright ** 1.6)
knot_rgb = KNOT[..., None] * np.array(VIOLET, np.float32)
knot_img = bpy.data.images.new('well-knot-glow', W, H, alpha=False)
knot_img.pixels[:] = np.concatenate([knot_rgb, np.ones_like(knot_rgb[:, :, :1])], axis=2).ravel()
knot_img.scale(W // 2, H // 2)
knot_img.pack()
knot_node = ATLAS.node_tree.nodes.new('ShaderNodeTexImage')
knot_node.image = knot_img
atlas_bsdf = ATLAS.node_tree.nodes['Principled BSDF']
ATLAS.node_tree.links.new(knot_node.outputs['Color'], atlas_bsdf.inputs['Emission Color'])
atlas_bsdf.inputs['Emission Strength'].default_value = 3.0
print('knot glow texels', int((KNOT > 0.05).sum()))

# --- The coil cap: three copper rings, each its own node turning on the column's axis.
low = by_name[COIL_LOW]
lz = np.array([v.co.z for v in low.data.vertices])
ring_split = float((lz.min() + lz.max()) / 2)
ring_a = split(low, 'coil-ring-a', lambda f, _: f.calc_center_median().z < ring_split)
low.name = low.data.name = 'coil-ring-b'
ring_b = low
ring_c = by_name[COIL_TOP]
ring_c.name = ring_c.data.name = 'coil-ring-c'
RINGS = [ring_a, ring_b, ring_c]
ring_rest = []
for ring in RINGS:
    rv = np.array([v.co[:] for v in ring.data.vertices])
    centre = ((rv.min(0) + rv.max(0)) / 2).tolist()
    set_origin(ring, (AXIS.x, AXIS.y, centre[2]))
    ring_rest.append(centre[2])
    print(ring.name, 'faces', len(ring.data.polygons), 'z', round(centre[2], 2))

# --- The clamps: each its own node, origin at its foot on the knot.
s_src = by_name[S_CLAMP_FROM]


def is_s_clamp(f, _):
    c = f.calc_center_median()
    return -1.3 * S < c.x < 0.0 and c.z > 2.15 * S and (c.y > -4.3 * S or c.z > 2.75 * S)


clamps = []
for key, names in CLAMPS.items():
    objs = [by_name[n] for n in names]
    if key == 's':
        objs = [split(s_src, 'clamp-s-src', is_s_clamp)]
    if key == 'sse':  # its head is part 14; its wedge is the east end of part 9
        objs.append(split(s_src, 'clamp-sse-src', lambda f, _: f.calc_center_median().x > 0.4 * S))
    clamp = join(objs, 'clamp-' + key)
    v = np.array([p.co[:] for p in clamp.data.vertices])
    foot = ((v[:, 0].min() + v[:, 0].max()) / 2, (v[:, 1].min() + v[:, 1].max()) / 2, float(v[:, 2].min()))
    set_origin(clamp, foot)
    angle = math.atan2(foot[1] - AXIS.y, foot[0] - AXIS.x)
    clamps.append((angle, clamp))
    print(clamp.name, 'faces', len(clamp.data.polygons), 'foot', [round(x, 2) for x in foot])
clamps.sort(key=lambda t: -t[0])  # the ripple runs clockwise seen from above

# --- Sockets.
all_pts = [o.matrix_world @ v.co for o in bpy.context.scene.objects if o.type == 'MESH' for v in o.data.vertices]
height = max(p.z for p in all_pts)
kit.socket('socket_knot', (AXIS.x, AXIS.y, 2.5 * S))
kit.socket('socket_column_foot', (AXIS.x, AXIS.y, G_Z0))
kit.socket('socket_column', (AXIS.x, AXIS.y - 1.4 * S, (G_Z0 + G_Z1) / 2))
kit.socket('socket_coil', (AXIS.x, AXIS.y, ring_rest[1]))


# --- Clips: every moving node keys every clip, so blends between clips stay clean.
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


def slot_curves(action, obj):
    slot = obj.animation_data.action_slot
    out = []
    for layer in action.layers:
        for strip in layer.strips:
            bag = strip.channelbag(slot)
            if bag:
                out.extend(bag.fcurves)
    return out


def rest(frames):
    return [(r, 'location', [2], [(0, z), (frames, z)], None) for r, z in zip(RINGS, ring_rest)] + \
           [(r, 'rotation_euler', [2], [(0, 0.0), (frames, 0.0)], None) for r in RINGS] + \
           [(c, 'location', [2], [(0, c.location.z), (frames, c.location.z)], None) for _, c in clamps]


clip('idle', rest(2))
WK = 60  # 2 s: one pump stroke. The clamps bite in a ripple round the crown; the coil's
# rings spring apart as the stroke drives substance up, and turn all the while.
BITE = 0.16 * S
STAGGER = 1.6  # frames between neighbouring clamps
clamp_tracks = []
for i, (_, c) in enumerate(clamps):
    z0, d = c.location.z, round(i * STAGGER)
    clamp_tracks.append((c, 'location', [2], [(0, z0), (2 + d, z0), (9 + d, z0 - BITE), (13 + d, z0 - BITE),
                                              (30 + d, z0), (WK, z0)], None))
SPRING = 0.2 * S  # the middle ring's lift; the top ring lifts twice that
ring_tracks = []
for k, (ring, z) in enumerate(zip(RINGS, ring_rest)):
    lift = SPRING * k
    turn = 2 * math.pi * (1 if k % 2 == 0 else -1)
    ring_tracks.append((ring, 'location', [2], [(0, z), (8, z), (18, z + lift), (26, z + lift * 0.85), (44, z), (WK, z)], None))
    ring_tracks.append((ring, 'rotation_euler', [2], [(0, 0.0), (WK, turn)], 'LINEAR'))
clip('work', clamp_tracks + ring_tracks)

tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
print('triangles', tris, 'height', round(height, 2))

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Substance Well', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': team_slots},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            # Event times are seconds into the clip: the clamps' bite, the rings' peak.
            {'semantic': 'work', 'clip': 'work', 'loop': True,
             'events': [{'name': 'pump', 'time': round(9 / kit.FPS, 3)},
                        {'name': 'surge', 'time': round(18 / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [12, 12], 'offset': [0, 0]},
        'vegetationClearance': 6,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/buildings/substance_well.py.',
}
kit.export(OUT, NAME, asset)
