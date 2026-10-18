"""The beetles' Ancestor Shrine from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/ancestor_shrine.py -- \\
        <beetle-ancestor-shrine-segmented.glb>

The shrine is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-ancestor-shrine-idle.png), split by Tripo's segmentation into parts that
share one texture atlas. Heroes are trained and reconstituted here by machine. Here (D65):
- the royal-blue banners and plates become the team mask in the atlas's alpha;
- the ancestor's shell is its own node: at rest it sits dark on the dais; at work it lifts
  off and turns a little while a violet energy shimmer streams up over it. The atlas cannot
  stream (`flow` scrolls V), so the shell gets its own cylindrical UVs, its textures are
  baked onto them, and its emission is a tiling vein pattern that rises along V;
- the orb at the top of the arch (sphere and segmented brass ring) is its own node: it glows
  and its ring twists to and fro while the shrine works (within ±0.55 rad: clips blend by
  lerping matrices, so a full turn would shrink mid-fade); the shoulder emitters glow too;
- violet arcs crackle from the orb down to the shoulder emitters: separate emissive nodes,
  collapsed into the orb at rest, flickering out to full size at work;
- `spawn` (a hero comes out): the shell heaves, every arc fires, `flash` at its peak;
- sockets for the state effects: the orb, the shell, the dais, the spawn point at the steps."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SHRINE = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-ancestor-shrine'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 12.0  # fills its 12-cell footprint: buildings have no walkable perimeter
S = WIDTH / 12.0  # the offsets below were set on the 12-metre build
VIOLET = (0.55, 0.32, 1.0)

# Segmented parts by role (tripo_part_<n>; stable only for this one export).
ARCH = 'tripo_part_0'      # the arch frame round the shell, with its shoulder emitters
SHELL = 'tripo_part_2'     # the ancestor's shell on the dais
ORB = 'tripo_part_14'      # the orb in its brass ring at the top of the arch, and its keystone
DAIS_RING = 'tripo_part_15'  # the brass half-ring at the foot of the shell
# Banner pillars 1 (left) and 3/9 (right); dais and steps 4, 5, 6, 7, 8; small trim the rest.

kit.reset()
bpy.ops.import_scene.gltf(filepath=SHRINE)
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
    """Moves the faces of `obj` whose centre passes `keep(centre)` into a new object."""
    bm = bmesh.new()
    bm.from_mesh(obj.data)
    out = obj.copy()
    out.data = obj.data.copy()
    out.name = out.data.name = name
    bpy.context.scene.collection.objects.link(out)
    gone = [f for f in bm.faces if keep(f.calc_center_median())]
    bmesh.ops.delete(bm, geom=gone, context='FACES')
    bm.to_mesh(obj.data)
    bm.free()
    bm = bmesh.new()
    bm.from_mesh(out.data)
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if not keep(f.calc_center_median())], context='FACES')
    bm.to_mesh(out.data)
    bm.free()
    return out


def set_origin(obj, at):
    """Moves the node origin to `at` (world), keeping the mesh where it is."""
    at = mathutils.Vector(at)
    obj.data.transform(mathutils.Matrix.Translation(-at))
    obj.location = at
    obj.rotation_mode = 'XYZ'  # glTF imports use quaternions; the clips key Euler angles


# --- The shell: own UVs (cylindrical, V from the top down: glTF flips V and the shader
# streams the glow towards higher glTF V, so `flow` > 0 rises), its atlas textures baked onto them, so the
# energy shimmer can stream up over it.
shell = by_name[SHELL]
sv = np.array([v.co[:] for v in shell.data.vertices])
s_lo, s_hi = sv.min(0), sv.max(0)
s_centre = mathutils.Vector(((s_lo[0] + s_hi[0]) / 2, (s_lo[1] + s_hi[1]) / 2, s_lo[2]))
U_SPAN = 0.94  # the cylinder's seam is at the back, against the arch


def cylinder_uv(obj, centre, z0, z1):
    me = obj.data
    old = me.uv_layers[0].name
    layer = me.uv_layers.new(name='shell')
    for poly in me.polygons:
        us, vs = [], []
        for li in poly.loop_indices:
            co = me.vertices[me.loops[li].vertex_index].co
            a = math.atan2(co.y - centre.y, co.x - centre.x)
            us.append(((a - math.pi / 2) % (2 * math.pi)) / (2 * math.pi))  # 0 at the back (+Y)
            vs.append((z1 - co.z) / (z1 - z0))
        if max(us) - min(us) > 0.5:  # straddles the seam: keep the face whole
            us = [u + 1 if u < 0.5 else u for u in us]
        for li, u, v in zip(poly.loop_indices, us, vs):
            layer.data[li].uv = (0.01 + u * U_SPAN, 0.01 + v * 0.98)
    return old, layer


old_uv, shell_uv = cylinder_uv(shell, s_centre, s_lo[2], s_hi[2])
shell_mat = ATLAS.copy()
shell_mat.name = 'shell'
shell.data.materials[0] = shell_mat


def bake_shell(obj, mat, old):
    """Bakes the atlas's base colour, roughness/metal and normal onto the shell's own UVs."""
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
    obj.data.uv_layers.active = obj.data.uv_layers['shell']
    for o in bpy.context.scene.objects:
        o.select_set(o is obj)
    bpy.context.view_layer.objects.active = obj
    base_src = texture_of(mat)
    rm_src = [n for n in nt.nodes if n.type == 'TEX_IMAGE' and n.image and n.image.name.endswith('_rm')][0]
    out = {}
    for key, w, h, data in (('base', 2048, 1024, False), ('rm', 1024, 512, True), ('normal', 2048, 1024, True)):
        img = bpy.data.images.new('shell-' + key, w, h, alpha=False)
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


baked = bake_shell(shell, shell_mat, old_uv)
shell.data.uv_layers.remove(shell.data.uv_layers[old_uv])
shell.data.uv_layers.active = shell.data.uv_layers['shell']
shell.data.uv_layers['shell'].active_render = True


def shimmer_texture(name, w=512, h=256, seed=7):
    """Violet energy veins over a faint glow, tiling in U and V (so `flow` can stream it)."""
    rng = np.random.default_rng(seed)
    uu, vv = np.meshgrid(np.linspace(0, 1, w, endpoint=False), np.linspace(0, 1, h, endpoint=False))
    field = np.zeros((h, w), np.float32)
    for k, ku in ((1, 3), (2, 5), (3, 8), (5, 11)):
        p = rng.uniform(0, 2 * np.pi, 3)
        field += np.sin(2 * np.pi * (uu * ku + vv * k) + p[0] + 0.9 * np.sin(2 * np.pi * vv * k + p[1])) / k
    field = (field - field.min()) / (field.max() - field.min())
    veins = np.exp(-((field - 0.5) / 0.035) ** 2) + 0.5 * np.exp(-((field - 0.22) / 0.03) ** 2)
    glow = np.clip(0.04 + veins, 0, 1.2)[..., None] * np.array(VIOLET, np.float32)
    img = bpy.data.images.new(name, w, h, alpha=False)
    img.pixels[:] = np.concatenate([glow, np.ones((h, w, 1), np.float32)], axis=2).ravel()
    img.pack()
    return img


# The shell's own material on its baked textures.
nt = shell_mat.node_tree
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
glow_tex = nt.nodes.new('ShaderNodeTexImage')
glow_tex.image = shimmer_texture('shell-shimmer')
nt.links.new(glow_tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 3.5
shell_mat['activeGlow'] = {'pulse': 0.5, 'flow': 0.12}

# --- Team colour: the blue banners and plates become the mask in the atlas's alpha (once:
# every other part shares the one material).
node = texture_of(ATLAS)
px = pixels(node.image)
rgb = px[:, :, :3]
blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08)
team = bpy.data.images.new('shrine-team', node.image.size[0], node.image.size[1], alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
ATLAS.name = 'TC_shrine'
ATLAS['teamColorMask'] = 'baseColorAlpha'
team_slots = [ATLAS.name]
print('team mask', round(float(mask.mean()), 4))

lum = lambda rgb: rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32)  # noqa: E731


GLOW = np.zeros(rgb.shape[:2], np.float32)  # the atlas's active emission, filled per part


def glow_islands(obj, weight):
    """Adds the texels under `obj`'s faces (its UV islands in the shared atlas), weighted by
    `weight(rgb)`, to the atlas's active emission: the one material serves every part, and
    only these texels come alive while the shrine works."""
    h, w = GLOW.shape
    me = obj.data
    uv = me.uv_layers[0].data
    hit = np.zeros((h, w), bool)
    for poly in me.polygons:
        tri = np.array([uv[li].uv[:] for li in poly.loop_indices]) * (w, h)
        for k in range(1, len(tri) - 1):
            a, b, c = tri[0], tri[k], tri[k + 1]
            x0, y0 = np.floor(np.minimum(np.minimum(a, b), c)).astype(int) - 2
            x1, y1 = np.ceil(np.maximum(np.maximum(a, b), c)).astype(int) + 2
            x0, y0, x1, y1 = max(x0, 0), max(y0, 0), min(x1, w - 1), min(y1, h - 1)
            if x1 < x0 or y1 < y0:
                continue
            xs, ys = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
            d = (b[1] - c[1]) * (a[0] - c[0]) + (c[0] - b[0]) * (a[1] - c[1])
            if abs(d) < 1e-9:
                continue
            l1 = ((b[1] - c[1]) * (xs - c[0]) + (c[0] - b[0]) * (ys - c[1])) / d
            l2 = ((c[1] - a[1]) * (xs - c[0]) + (a[0] - c[0]) * (ys - c[1])) / d
            e = 2.0 / max(1.0, np.hypot(*(b - a)))  # a texel or two of margin against mip seams
            inside = (l1 >= -e) & (l2 >= -e) & (1 - l1 - l2 >= -e)
            hit[y0:y1 + 1, x0:x1 + 1] |= inside
    GLOW[:] = np.maximum(GLOW, hit * weight(rgb))


# --- The orb: its sphere and brass ring (in front of the keystone) become one node that
# turns about its own axis, facing the camera's view; its dark steel glows.
orb_part = by_name[ORB]
ov = np.array([v.co[:] for v in orb_part.data.vertices])
oc = (ov.min(0) + ov.max(0)) / 2
front_y = ov[:, 1].min() + 0.6 * S
for _ in range(3):
    near = ov[(ov[:, 1] < front_y) & (np.hypot(ov[:, 0] - oc[0], ov[:, 2] - oc[2]) < 0.85 * S)]
    oc[0], oc[2] = (near[:, 0].min() + near[:, 0].max()) / 2, (near[:, 2].min() + near[:, 2].max()) / 2
ORB_C = mathutils.Vector((oc[0], near[:, 1].max(), oc[2]))
ORB_R = max(near[:, 0].max() - near[:, 0].min(), near[:, 2].max() - near[:, 2].min()) / 2
orb = split(orb_part, 'orb', lambda c: c.y < front_y and math.hypot(c.x - ORB_C.x, c.z - ORB_C.z) < 0.85 * S)
glow_islands(orb, lambda c: np.clip((0.2 - lum(c)) / 0.12, 0.0, 1.0))
set_origin(orb, ORB_C)
orb_front = ORB_C.y - (ORB_C.y - near[:, 1].min())
print('orb centre', [round(v, 2) for v in ORB_C], 'radius', round(ORB_R, 2), 'faces', len(orb.data.polygons))

# --- The shoulder emitters on the arch frame: their dark steel glows while it works.
emit = split(by_name[ARCH], 'emitters', lambda c: 2.2 * S < abs(c.x) < 3.6 * S and 7.7 * S < c.z < 9.3 * S)
glow_islands(emit, lambda c: np.clip((0.16 - lum(c)) / 0.1, 0.0, 1.0) * 0.7)
ev = np.array([v.co[:] for v in emit.data.vertices])
EMIT_L = mathutils.Vector((ev[ev[:, 0] < 0][:, 0].mean(), ev[:, 1].min(), ev[ev[:, 0] < 0][:, 2].max()))
EMIT_R = mathutils.Vector((ev[ev[:, 0] > 0][:, 0].mean(), ev[:, 1].min(), ev[ev[:, 0] > 0][:, 2].max()))
print('emitters', [round(v, 2) for v in EMIT_L], [round(v, 2) for v in EMIT_R], 'faces', len(emit.data.polygons))

# The atlas's emission: the orb and the emitters, dark at rest, alive only while it works.
glow_rgb = GLOW[..., None] * np.array(VIOLET, np.float32)
glow_img = bpy.data.images.new('shrine-glow', GLOW.shape[1], GLOW.shape[0], alpha=False)
glow_img.pixels[:] = np.concatenate([glow_rgb, np.ones_like(glow_rgb[:, :, :1])], axis=2).ravel()
glow_img.scale(GLOW.shape[1] // 2, GLOW.shape[0] // 2)
glow_img.pack()
glow_node = ATLAS.node_tree.nodes.new('ShaderNodeTexImage')
glow_node.image = glow_img
atlas_bsdf = ATLAS.node_tree.nodes['Principled BSDF']
ATLAS.node_tree.links.new(glow_node.outputs['Color'], atlas_bsdf.inputs['Emission Color'])
atlas_bsdf.inputs['Emission Strength'].default_value = 7.0
ATLAS['activeGlow'] = {'pulse': 0.8, 'flow': 0.0}

# --- The shell node: pivot on the dais under its centre.
set_origin(shell, s_centre)
shell.name = 'shell'

# --- Arcs: jagged violet bolts from the orb's rim down to each shoulder emitter, in front of
# the frame; two variants a side so the flicker never repeats a shape.
arc_mat = kit.material('arc', color=(0.72, 0.6, 1.0), roughness=0.4)
arc_mat.node_tree.nodes['Principled BSDF'].inputs['Emission Color'].default_value = (0.55, 0.35, 1.0, 1.0)
arc_mat.node_tree.nodes['Principled BSDF'].inputs['Emission Strength'].default_value = 6.0
arc_mat['activeGlow'] = {'pulse': 3.0, 'flow': 0.0}
arc_mat['noShadow'] = True


def bolt(name, start, end, bulge, seed, radius=0.08 * S, steps=12):
    rng = np.random.default_rng(seed)
    start, end = mathutils.Vector(start), mathutils.Vector(end)
    ctrl = (start + end) / 2 + mathutils.Vector(bulge)
    pts = []
    for i in range(steps + 1):
        t = i / steps
        p = (1 - t) ** 2 * start + 2 * (1 - t) * t * ctrl + t ** 2 * end
        if 0 < i < steps:
            p += mathutils.Vector((rng.uniform(-1, 1) * 0.15, rng.uniform(-1, 1) * 0.06, rng.uniform(-1, 1) * 0.15)) * S
        pts.append(p)
    bm = bmesh.new()
    rings = []
    for i, p in enumerate(pts):
        t = (pts[min(i + 1, steps)] - pts[max(i - 1, 0)]).normalized()
        n = t.cross(mathutils.Vector((0, 1, 0))).normalized()
        b = t.cross(n).normalized()
        r = radius * (0.55 + 0.45 * math.sin(math.pi * i / steps))
        rings.append([bm.verts.new(p + (n * math.cos(a) + b * math.sin(a)) * r)
                      for a in (k * math.pi / 2 for k in range(4))])
    for r0, r1 in zip(rings, rings[1:]):
        for j in range(4):
            bm.faces.new([r0[j], r0[(j + 1) % 4], r1[(j + 1) % 4], r1[j]])
    bm.faces.new(list(reversed(rings[0])))
    bm.faces.new(rings[-1])
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = kit.link(bpy.data.objects.new(name, me))
    obj.data.materials.append(arc_mat)
    set_origin(obj, ORB_C)
    return obj


ARC_Y = min(orb_front, EMIT_L.y) - 0.15 * S
arcs = []
for side, target in (('l', EMIT_L), ('r', EMIT_R)):
    sx = -1 if side == 'l' else 1
    start = (ORB_C.x + sx * ORB_R * 0.9, ARC_Y, ORB_C.z - 0.15 * S)
    end = (target.x, ARC_Y, target.z - 0.25 * S)  # onto the emitter's top
    for variant, seed in (('a', 11), ('b', 23)):
        bulge = (sx * 0.3 * S, 0, 0.6 * S if variant == 'a' else 0.35 * S)
        arcs.append(bolt(f'arc-{side}-{variant}', start, end, bulge, seed + (0 if side == 'l' else 100)))

# --- Sockets.
kit.socket('socket_orb', (ORB_C.x, orb_front - 0.3 * S, ORB_C.z))
kit.socket('socket_shell', (s_centre.x, s_lo[1] - 0.2 * S, s_centre.z + 3.0 * S))
kit.socket('socket_dais', (s_centre.x, s_lo[1] - 0.5 * S, s_centre.z + 0.2 * S))
kit.socket('socket_emitter_l', EMIT_L + mathutils.Vector((0, -0.2 * S, 0.1 * S)))
kit.socket('socket_emitter_r', EMIT_R + mathutils.Vector((0, -0.2 * S, 0.1 * S)))
all_pts = [o.matrix_world @ v.co for o in bpy.context.scene.objects if o.type == 'MESH' for v in o.data.vertices]
kit.socket('socket_spawn', (0.0, min(p.y for p in all_pts) + 0.8 * S, 0.1))

# --- Clips: every moving node keys every clip, so blends between clips stay clean.
OFF = 0.001  # arcs collapsed into the orb


def clip(name, tracks):
    """One action `name` keying several nodes (one slot each): tracks are
    (node, path, indices, [(frame, value)], interpolation or None for Bézier)."""
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


def rest_tracks(frames):
    return [(shell, 'location', [2], [(0, s_centre.z), (frames, s_centre.z)], None),
            (shell, 'rotation_euler', [2], [(0, 0.0), (frames, 0.0)], None),
            (orb, 'rotation_euler', [1], [(0, 0.0), (frames, 0.0)], None)] + \
           [(a, 'scale', [0, 1, 2], [(0, OFF), (frames, OFF)], None) for a in arcs]


def flicker(frames, seed, on_share, lo=4, hi=9):
    """Constant on/off keys for an arc: short bursts at pseudo-random frames."""
    rng = np.random.default_rng(seed)
    keys, f = [(0, OFF)], int(rng.integers(2, 12))
    while f < frames - 6:
        length = int(rng.integers(lo, hi))
        keys += [(f, float(rng.uniform(0.92, 1.0))), (min(f + length, frames - 2), OFF)]
        f += length + int(rng.integers(2, int(length / on_share)))
    keys.append((frames, OFF))
    return keys


Z0 = s_centre.z
RISE = 0.38 * S  # the shell's tip stays just clear of the orb's ring
clip('idle', rest_tracks(2))
W = 180  # 6 s: the shell lifts, hovers turning, settles; the orb's ring twists to and fro
clip('work', [
    (shell, 'location', [2], [(0, Z0), (40, Z0 + RISE), (75, Z0 + RISE * 0.8), (110, Z0 + RISE),
                              (145, Z0 + RISE * 0.85), (W, Z0)], None),
    (shell, 'rotation_euler', [2], [(0, 0.0), (60, 0.26), (120, -0.22), (W, 0.0)], None),
    (orb, 'rotation_euler', [1], [(0, 0.0), (22, -0.55), (67, 0.55), (112, -0.55), (157, 0.55), (W, 0.0)], None),
] + [(a, 'scale', [0, 1, 2], flicker(W, 31 + i, 0.45), 'CONSTANT') for i, a in enumerate(arcs)])


def spawn_arc(i):
    """Every arc fires through the flash: each side alternates its two shapes."""
    first = i % 2 == 0  # variant a of each side
    keys = [(0, OFF)]
    for k, f in enumerate(range(3, 27, 5)):
        keys.append((f, 1.0 if (k % 2 == 0) == first else OFF))
    return keys + [(28, OFF), (SP, OFF)]


SP = 45  # 1.5 s: the hero is reconstituted; every arc fires, the shell heaves
FLASH = 10
clip('spawn', [
    (shell, 'location', [2], [(0, Z0), (FLASH, Z0 + RISE * 1.1), (22, Z0 + RISE * 0.8), (SP, Z0)], None),
    (shell, 'rotation_euler', [2], [(0, 0.0), (FLASH, 0.12), (SP, 0.0)], None),
    (orb, 'rotation_euler', [1], [(0, 0.0), (FLASH, -0.55), (26, 0.35), (SP, 0.0)], None),
] + [(a, 'scale', [0, 1, 2], spawn_arc(i), 'CONSTANT') for i, a in enumerate(arcs)])

tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
height = max(p.z for p in all_pts)
print('triangles', tris, 'height', round(height, 2))

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Ancestor Shrine', 'kind': 'building',
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
             'events': [{'name': 'flash', 'time': round(FLASH / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [12, 12], 'offset': [0, 0]},
        'vegetationClearance': 6,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/buildings/ancestor_shrine.py.',
}
kit.export(OUT, NAME, asset)
