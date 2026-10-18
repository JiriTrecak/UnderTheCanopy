"""The beetles' Elder Bastion (the main hall, tier 3, its final form) from Tripo, made game-ready
and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/elder_bastion.py -- \\
        <beetle-elder-bastion-segmented.glb>

The Elder Bastion is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-hall-t3-idle.png): the Bastion's colossal beetle crouched inside a fortified
wall-platform with four boiler towers and three banners, its royal-blue plated shell carrying
one giant sealed substance reactor (a tall glass column in brass bars under a blue dome cap and
spike), flanked by three great curved brass horn buttresses like a crown; at the front the
black head with its inner fangs and the two great gate mandibles. Tripo's segmentation split
it into parts that share one texture atlas. Here (D65; the same states, node names, events and
team mask as the tier-1 and tier-2 halls, tools/buildings/bastion.py and great_bastion.py):
- the royal-blue shell plates, dome cap, wall panels and banners become the team mask in the
  atlas's alpha; the black chitin and steel stay out of it (the same brightness floor);
- the reactor's glass is its own material with its own cylindrical UVs (V from the top of the
  glass to its foot in Blender, so `flow` streams upward): dark glass at rest; at work it fills
  with a swirling violet storm streaming up;
- the horn buttresses' tips glow violet while it works (`activeGlow` on the atlas, limited to
  the texels under the horns' upper faces), and jagged violet arcs crackle between each horn
  tip and the reactor's cap (their own nodes, flickered by scale, collapsed at rest);
- moving nodes: the two great gate claws (`mandible-l`, `mandible-r`), hinged where they leave
  the head, which part sideways like a gate; the head with its inner fangs (`head`), which
  lifts its chin; the shell with the reactor column and the horns (`shell`), which swells a
  little as if breathing; the reactor's dome cap and spike (`cap`), which turns like a valve
  crown and kicks up on each surge; and the four boiler towers' drums (`boiler-fl`, `-bl`,
  `-br`, `-fr`), which swell with pressure one after another round the wall (a `vent-<k>` event
  each for the steam on that tower). In `work` (training, researching or upgrading) the claws
  part slowly, the head lifts, the shell breathes three times, the cap turns and kicks on each
  breath (`surge`: a violet surge at the cap), the boilers pump twice round, the arcs crackle;
  `rivet` / `rivet-2` events put sparks round the wall rim. In `spawn` the claws snap wide, the
  head rears, the cap jolts up and every arc fires (a `gate` event for the steam burst at the
  gate and the violet flash from the reactor) as a unit comes out;
- sockets: the reactor (light, foot for rising motes, cap for surges), the three horn tips
  (arcs and sparks), the tower caps (steam), the gate and four points on the wall rim.
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
NAME = 'beetles-elder-bastion'
OUT = os.environ.get('ELDER_OUT') or os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 20.0  # fills its 20-cell footprint: buildings have no walkable perimeter
S = WIDTH / 20.0  # offsets below are in metres at the 20-metre build
VIOLET = (0.55, 0.32, 1.0)

# Segmented parts by role (tripo_part_<n>, this export only).
PLATFORM = 'tripo_part_0'  # the floor, the fortified wall with its pillars and the small towers
SHELL = 'tripo_part_1'  # the domed shell, thorax, reactor column, dome cap and spike
HEAD = 'tripo_part_4'  # head, crest, inner fangs and the great right claw
HEAD_EXTRA = ['tripo_part_19']  # the left inner fang's tip
MANDIBLE_L = 'tripo_part_15'  # the great left claw
HORNS = {'l': 'tripo_part_10', 'm': 'tripo_part_11', 'r': 'tripo_part_9'}  # big left, small left, right
TOWERS = {'fl': 'tripo_part_12', 'bl': 'tripo_part_8', 'br': 'tripo_part_3', 'fr': 'tripo_part_7'}
# Static: 2, 13, 14 the banners; 5, 6, 17 legs; 16, 18, 20 trim behind the shell.

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
by_name = {o.name: o for o in parts}
ATLAS = parts[0].data.materials[0]
assert all(o.data.materials[0] == ATLAS for o in parts), 'expected one shared atlas'


def verts(obj):
    return np.array([(obj.matrix_world @ v.co)[:] for v in obj.data.vertices], np.float32)


# Normalise: centred, ground at 0, WIDTH across; transforms applied. Tripo's base is the hall's
# own walled steel platform; there is no dirt to strip.
for o in parts:
    o.data.transform(o.matrix_world)
    o.matrix_world = mathutils.Matrix.Identity(4)
all_v = np.concatenate([verts(o) for o in parts])
lo, hi = all_v.min(axis=0), all_v.max(axis=0)
scale = WIDTH / float(max(hi[0] - lo[0], hi[1] - lo[1]))
shift = mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation(
    (-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -float(lo[2])))
for o in parts:
    o.data.transform(shift)
    for p in o.data.polygons:
        p.use_smooth = True


def split(obj, name, keep):
    """Moves the faces of `obj` whose polygon passes `keep(polygon, mesh)` into a new object."""
    out = obj.copy()
    out.data = obj.data.copy()
    out.name = out.data.name = name
    bpy.context.scene.collection.objects.link(out)
    for me, inside in ((obj.data, True), (out.data, False)):
        chosen = {p.index for p in me.polygons if keep(p, me) == inside}
        bm = bmesh.new()
        bm.from_mesh(me)
        bm.faces.ensure_lookup_table()
        bmesh.ops.delete(bm, geom=[bm.faces[i] for i in chosen], context='FACES')
        bm.to_mesh(me)
        bm.free()
    return out


def islands_where(obj, keep):
    """The face indices of the connected pieces of `obj` whose centroid passes `keep`."""
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
    return chosen


def join(objs, name):
    bpy.ops.object.select_all(action='DESELECT')
    for o in objs:
        o.select_set(True)
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


# --- The atlas.
node = next(n for n in ATLAS.node_tree.nodes if n.type == 'TEX_IMAGE' and 'basecolor' in n.image.name.lower())
TW, TH = node.image.size
px = np.array(node.image.pixels[:], dtype=np.float32).reshape(TH, TW, 4)
rgb = px[:, :, :3].copy()
LUM = np.array([0.2126, 0.7152, 0.0722], np.float32)
lum = rgb @ LUM


def texel(uv):
    return rgb[min(TH - 1, int(uv[1] * TH)) % TH, min(TW - 1, int(uv[0] * TW)) % TW]


def face_colour(me, poly):
    uvl = me.uv_layers[0].data
    cs = [texel(uvl[li].uv) for li in poly.loop_indices]
    cs.append(texel(np.mean([uvl[li].uv[:] for li in poly.loop_indices], axis=0)))
    return np.mean(cs, axis=0)


def raster(obj, mark, faces=None):
    """Sets `mark` true on the texels under every UV triangle of `obj` (or of the polygons
    for which `faces(polygon)` holds)."""
    me = obj.data
    uvl = me.uv_layers[0].data
    for poly in me.polygons:
        if faces is not None and not faces(poly):
            continue
        idx = list(poly.loop_indices)
        for k in range(1, len(idx) - 1):
            tri = [idx[0], idx[k], idx[k + 1]]
            (u0, v0), (u1, v1), (u2, v2) = [(uvl[i].uv[0] * TW, uvl[i].uv[1] * TH) for i in tri]
            x0, x1 = int(max(0, min(u0, u1, u2) - 1)), int(min(TW - 1, max(u0, u1, u2) + 1))
            y0, y1 = int(max(0, min(v0, v1, v2) - 1)), int(min(TH - 1, max(v0, v1, v2) + 1))
            if x1 < x0 or y1 < y0:
                continue
            xs, ys = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
            d = (v1 - v2) * (u0 - u2) + (u2 - u1) * (v0 - v2)
            if abs(d) < 1e-9:
                continue
            a = ((v1 - v2) * (xs - u2) + (u2 - u1) * (ys - v2)) / d
            b = ((v2 - v0) * (xs - u2) + (u0 - u2) * (ys - v2)) / d
            inside = (a > -0.05) & (b > -0.05) & (1 - a - b > -0.05)
            mark[y0:y1 + 1, x0:x1 + 1] |= inside


# --- The reactor's axis: the middle of the column above the shell.
shell_src = by_name[SHELL]
sv0 = verts(shell_src)
col_v = sv0[(sv0[:, 2] > 9.0 * S) & (sv0[:, 2] < 13.0 * S)]
AXIS = mathutils.Vector((float((col_v[:, 0].min() + col_v[:, 0].max()) / 2),
                         float((col_v[:, 1].min() + col_v[:, 1].max()) / 2), 0.0))
COL_R = float(np.percentile(np.hypot(col_v[:, 0] - AXIS.x, col_v[:, 1] - AXIS.y), 50))
print('reactor axis', [round(v, 2) for v in AXIS], 'radius', round(COL_R, 2))


def radius(c):
    return math.hypot(c.x - AXIS.x, c.y - AXIS.y)


# --- The reactor's glass. Tripo's panes are a ragged ring of small faces behind the column's
# posts and bands; a clean glass cylinder just outside them takes their place (the posts and
# bands stand proud of it), and the panes inside it go. Its own material and UVs, so the storm
# can stream up it.
GLASS_Z = (8.45 * S, 13.2 * S)
GLASS_R = float(os.environ.get('ELDER_GLASS_R', 2.1)) * S


def hidden_pane(poly, me):
    vs = [me.vertices[i].co for i in poly.vertices]
    return all(GLASS_Z[0] + 0.05 * S < v.z < GLASS_Z[1] - 0.05 * S and radius(v) < GLASS_R - 0.02 * S for v in vs)


panes = split(shell_src, 'panes', hidden_pane)
print('panes hidden', len(panes.data.polygons))
bpy.data.objects.remove(panes, do_unlink=True)
G_Z0, G_Z1 = GLASS_Z
glass = kit.cylinder('reactor-glass', GLASS_R, G_Z1 - G_Z0, at=(AXIS.x, AXIS.y, G_Z0), verts=28)
glass.data.transform(mathutils.Matrix.Translation(glass.location))
glass.location = (0, 0, 0)
bm = bmesh.new()
bm.from_mesh(glass.data)
bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.normal.z < -0.5], context='FACES')  # no floor
bm.to_mesh(glass.data)
bm.free()
glass.data.uv_layers.new(name='UVMap')
for p in glass.data.polygons:
    p.use_smooth = abs(p.normal.z) < 0.5
V_SPAN = 1.0  # storm texture lengths over the glass's height


def cylinder_uv(obj):
    """Cylindrical UVs: U round the axis (three storm repeats), V from the top of the glass
    (0) to its foot (V_SPAN). The engine streams emission towards +V in glTF's flipped V, so
    the storm rises."""
    me = obj.data
    layer = me.uv_layers[0]
    for poly in me.polygons:
        us, vs = [], []
        for li in poly.loop_indices:
            co = me.vertices[me.loops[li].vertex_index].co
            a = math.atan2(co.y - AXIS.y, co.x - AXIS.x)
            us.append(3.0 * (a % (2 * math.pi)) / (2 * math.pi))
            if abs(poly.normal.z) > 0.5:  # the lid: the storm's top row, spread from the centre
                vs.append(0.15 * radius(co) / GLASS_R)
            else:
                vs.append((G_Z1 - co.z) / (G_Z1 - G_Z0) * V_SPAN)
        if max(us) - min(us) > 1.5:  # straddles the seam: keep the face whole
            us = [u + 3.0 if u < 1.5 else u for u in us]
        for li, u, v in zip(poly.loop_indices, us, vs):
            layer.data[li].uv = (u, v)


cylinder_uv(glass)


def storm_texture(name, w=512, h=256, seed=11):
    """A violet storm in the glass, tiling in U and V (so `flow` can stream it up): bright
    filaments that writhe upwards with a twist, over a deep violet haze."""
    rng = np.random.default_rng(seed)
    uu, vv = np.meshgrid(np.linspace(0, 1, w, endpoint=False), np.linspace(0, 1, h, endpoint=False))
    veins = np.zeros((h, w), np.float32)
    for k, (ku, kv, width, amp) in enumerate(((4, 1, 0.026, 1.0), (6, -1, 0.02, 0.85), (3, 2, 0.032, 0.7),
                                              (9, 1, 0.016, 0.55), (5, -2, 0.02, 0.5))):
        p = rng.uniform(0, 2 * np.pi, 4)
        wobble = 0.24 * np.sin(2 * np.pi * (vv * 2 + uu * (k % 2)) + p[1]) \
            + 0.12 * np.sin(2 * np.pi * (vv * 3 - uu * 1) + p[2])
        phase = (uu * ku + vv * kv + wobble * ku / 5 + p[0] / (2 * np.pi)) % 1.0
        d = np.minimum(phase, 1.0 - phase)
        along = 0.5 + 0.5 * np.sin(2 * np.pi * (vv * 2 + uu * ku * 0.5) + p[3])
        veins += amp * np.exp(-(d / width) ** 2) * (0.25 + 0.75 * along ** 2)
    haze = 0.16 + 0.12 * (0.5 + 0.5 * np.sin(2 * np.pi * (uu * 2 + vv * 1) + 0.7))
    glow = haze[..., None] * np.array(VIOLET, np.float32) \
        + np.clip(veins, 0, 1.4)[..., None] * np.array((0.8, 0.62, 1.0), np.float32)
    img = bpy.data.images.new(name, w, h, alpha=False)
    img.pixels[:] = np.concatenate([glow, np.ones((h, w, 1), np.float32)], axis=2).ravel()
    img.pack()
    return img


glass_mat = kit.material('reactor-glass', color=(0.012, 0.012, 0.028), roughness=0.22, metallic=0.0)
storm = glass_mat.node_tree.nodes.new('ShaderNodeTexImage')
storm.image = storm_texture('reactor-storm')
gbsdf = glass_mat.node_tree.nodes['Principled BSDF']
glass_mat.node_tree.links.new(storm.outputs['Color'], gbsdf.inputs['Emission Color'])
gbsdf.inputs['Emission Strength'].default_value = 6.0
glass_mat['activeGlow'] = {'pulse': 0.5, 'flow': 0.4}  # 0.4 heights a second, rising
glass.data.materials.clear()
glass.data.materials.append(glass_mat)

# --- Team colour: the royal-blue plates become the mask in the atlas's alpha (one material).
# The black chitin and steel carry a faint blue sheen; the brightness floor keeps them out.
mask = np.clip((rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1]) - 0.04) / 0.1, 0.0, 1.0) \
    * np.clip((rgb[:, :, 2] - 0.1) / 0.06, 0.0, 1.0)
team = bpy.data.images.new('elder-bastion-team', TW, TH, alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
ATLAS.name = 'TC_elder_bastion'
ATLAS['teamColorMask'] = 'baseColorAlpha'
print('team mask mean', round(float(mask.mean()), 3))

# --- Active glow (D65): the horns' upper reaches glow violet while it works, strongest at
# the tips; only texels under those faces light up, never texels other faces share.
horn_z = {}
for k, n in HORNS.items():
    hv = verts(by_name[n])
    horn_z[k] = (float(hv[:, 2].min()), float(hv[:, 2].max()))
GLOW = np.zeros((TH, TW), np.float32)
others_px = np.zeros((TH, TW), bool)
for o in parts:
    if o.name not in HORNS.values():
        raster(o, others_px)
for k, n in HORNS.items():
    z0, z1 = horn_z[k]
    for share, weight in ((0.66, 0.15), (0.74, 0.4), (0.81, 0.7), (0.88, 1.0)):
        band = np.zeros((TH, TW), bool)
        raster(by_name[n], band, lambda p, z0=z0, z1=z1, share=share: (p.center.z - z0) / (z1 - z0) >= share)
        GLOW = np.maximum(GLOW, band * weight)
GLOW *= ~others_px
glow_rgb = GLOW[..., None] * np.array(VIOLET, np.float32)
glow_img = bpy.data.images.new('elder-bastion-glow', TW, TH, alpha=False)
glow_img.pixels[:] = np.concatenate([glow_rgb, np.ones_like(glow_rgb[:, :, :1])], axis=2).ravel()
glow_img.scale(TW // 2, TH // 2)
glow_img.pack()
gtex = ATLAS.node_tree.nodes.new('ShaderNodeTexImage')
gtex.image = glow_img
bsdf = ATLAS.node_tree.nodes['Principled BSDF']
ATLAS.node_tree.links.new(gtex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 5.0
ATLAS['activeGlow'] = {'pulse': 1.4, 'flow': 0.0}
print('horn glow texels', int((GLOW > 0.1).sum()))

# --- The head: Tripo's head piece also carries the great right claw (its loose pieces right
# of the face, low and forward) and its fangs; the left fang's tip is its own part.
head_src = by_name[HEAD]
right_claw = islands_where(head_src, lambda c: c.x > 1.7 * S and c.y < -6.0 * S and c.z < 3.2 * S)
mandible_r = split(head_src, 'mandible-r', lambda p, me: p.index in right_claw)
head = join([head_src] + [by_name[n] for n in HEAD_EXTRA], 'head')
mandible_l = by_name[MANDIBLE_L]
mandible_l.name = mandible_l.data.name = 'mandible-l'
mandibles = {'l': mandible_l, 'r': mandible_r}
hv = verts(head)
HEAD_X = float((hv[:, 0].min() + hv[:, 0].max()) / 2)
HINGE = mathutils.Vector((HEAD_X, float(hv[:, 1].max()) - 0.3 * S, float(np.percentile(hv[:, 2], 92))))
set_origin(head, HINGE)
print('head faces', len(head.data.polygons), 'hinge', [round(v, 2) for v in HINGE])


def root_of(mv):
    """A claw's hinge: the mean of its highest, rearmost vertices."""
    root = mv[mv[:, 2] > mv[:, 2].max() - 0.6 * S]
    root = root[root[:, 1] > np.percentile(root[:, 1], 50)]
    return float(root[:, 0].mean()), float(root[:, 1].mean()), float(root[:, 2].mean())


for side, m in mandibles.items():
    mv = verts(m)
    set_origin(m, root_of(mv))
    print('mandible', side, 'faces', len(m.data.polygons), 'pivot', [round(v, 2) for v in m.location],
          'tip', mv[np.argmin(mv[:, 1])].round(2))

# --- The reactor's dome cap and spike: everything of the shell above the glass's top band,
# within the column.
CAP_Z = 13.45 * S
cap = split(shell_src, 'cap', lambda p, me: p.center.z > CAP_Z and radius(p.center) < COL_R + 0.6 * S)
cv = verts(cap)
CAP_TOP = float(cv[:, 2].max())
CAP_FOOT = mathutils.Vector((AXIS.x, AXIS.y, float(cv[:, 2].min())))
set_origin(cap, CAP_FOOT)
print('cap faces', len(cap.data.polygons), 'z', round(CAP_FOOT.z, 2), round(CAP_TOP, 2))

# --- The horns' tips, before they join the shell.
TIPS = {}
for k, n in HORNS.items():
    hv_ = verts(by_name[n])
    top = hv_[hv_[:, 2] > hv_[:, 2].max() - 0.35 * S]
    TIPS[k] = mathutils.Vector(top.mean(axis=0).tolist())

# --- The shell carries the reactor's glass and the horns, breathing about its own foot.
shell = join([shell_src, glass] + [by_name[n] for n in HORNS.values()], 'shell')
sv = verts(shell)
SHELL_FOOT = mathutils.Vector((AXIS.x, AXIS.y, float(np.percentile(sv[:, 2], 3))))
SHELL_TOP = float(sv[:, 2].max())
set_origin(shell, SHELL_FOOT)

# --- The boiler towers: each drum swells about the foot of its axis; the tower's base, the
# wall it stands on and anything outside the drum's radius stay put.
AXES = {}
for k, n in TOWERS.items():
    tv = verts(by_name[n])
    top = tv[tv[:, 2] > tv[:, 2].max() - 1.0 * S]
    ax = mathutils.Vector((float(top[:, 0].mean()), float(top[:, 1].mean()), 0.0))
    upper = tv[tv[:, 2] > tv[:, 2].max() - 3.0 * S]
    r = float(np.percentile(np.hypot(upper[:, 0] - ax.x, upper[:, 1] - ax.y), 92)) + 0.1 * S
    AXES[k] = (ax, r, float(tv[:, 2].max()))
boilers, tower_tops = {}, {}
for k, n in TOWERS.items():
    ax, r, top = AXES[k]
    src = by_name[n]
    drum = split(src, 'boiler-' + k, lambda p, me, ax=ax, r=r: math.hypot(p.center.x - ax.x, p.center.y - ax.y) < r
                 and p.center.z > 2.6 * S)
    dv = verts(drum)
    foot = float(dv[:, 2].min())
    set_origin(drum, (ax.x, ax.y, foot))
    boilers[k] = drum
    tower_tops[k] = mathutils.Vector((ax.x, ax.y, float(dv[:, 2].max())))
    print('boiler', k, 'axis', [round(v, 2) for v in ax], 'r', round(r, 2), 'faces', len(drum.data.polygons),
          'z', round(foot, 2), round(float(dv[:, 2].max()), 2), 'left', len(src.data.polygons))

# --- Arcs: jagged violet bolts from each horn tip to the reactor's cap, two shapes per horn,
# so the flicker never repeats. Their own material: bright violet, alive only at work.
arc_mat = kit.material('elder-bastion-arc', color=(0.75, 0.62, 1.0), roughness=0.4)
arc_bsdf = arc_mat.node_tree.nodes['Principled BSDF']
arc_bsdf.inputs['Emission Color'].default_value = (0.62, 0.42, 1.0, 1.0)
arc_bsdf.inputs['Emission Strength'].default_value = 9.0
arc_mat['activeGlow'] = {'pulse': 3.0, 'flow': 0.0}
arc_mat['noShadow'] = True


def bolt(name, start, end, seed, radius=0.13 * S, steps=10, bow=0.5):
    """A jagged arc from `start` (its node origin) to `end`, bowed upward."""
    rng = np.random.default_rng(seed)
    start, end = mathutils.Vector(start), mathutils.Vector(end)
    along = (end - start).normalized()
    side = along.cross(mathutils.Vector((0, 0, 1)))
    side = side.normalized() if side.length > 1e-4 else mathutils.Vector((1, 0, 0))
    ctrl = (start + end) / 2 + mathutils.Vector((0, 0, bow * S)) + side * rng.uniform(-0.4, 0.4) * S
    pts = []
    for i in range(steps + 1):
        t = i / steps
        p = (1 - t) ** 2 * start + 2 * (1 - t) * t * ctrl + t ** 2 * end
        if 0 < i < steps:
            p += (side * rng.uniform(-1, 1) * 0.24 + mathutils.Vector((0, 0, rng.uniform(-1, 1) * 0.2))) * S
        pts.append(p - start)
    bm = bmesh.new()
    loops = []
    for i, p in enumerate(pts):
        t = (pts[min(i + 1, steps)] - pts[max(i - 1, 0)]).normalized()
        n = t.cross(mathutils.Vector((0, 0, 1)))
        n = (n if n.length > 1e-4 else t.cross(mathutils.Vector((1, 0, 0)))).normalized()
        b = t.cross(n).normalized()
        r = radius * (1.0 - 0.55 * abs(2 * i / steps - 1))  # thickest mid-arc
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
    obj.location = start
    obj.rotation_mode = 'XYZ'
    return obj


arcs = []
CAP_HIT_Z = CAP_FOOT.z + 0.55 * (CAP_TOP - CAP_FOOT.z) * 0.5  # on the dome, under the spike
for i, (k, tip) in enumerate(TIPS.items()):
    out = mathutils.Vector((tip.x - AXIS.x, tip.y - AXIS.y, 0)).normalized()
    for variant in range(2):
        end = mathutils.Vector((AXIS.x, AXIS.y, CAP_HIT_Z + (0.35 if variant else 0.0) * S)) \
            + out * (0.9 - 0.25 * variant) * S
        start = tip - out * 0.12 * S - mathutils.Vector((0, 0, (0.15 + 0.5 * variant) * S))
        arcs.append(bolt(f'arc-{k}-{variant}', start, end, 31 + 7 * i + variant, bow=0.6 - 0.3 * variant))
for o in [o for o in arcs]:
    kit.cube_uv(o, 1.0)

# --- Everything else is one static node.
moving = {head, shell, cap} | set(mandibles.values()) | set(boilers.values()) | set(arcs)
static = [o for o in bpy.context.scene.objects if o.type == 'MESH' and o not in moving]
hall = join(static, 'elder-bastion')

# --- Sockets.
surface = mathutils.bvhtree.BVHTree.FromObject(hall, bpy.context.evaluated_depsgraph_get())


def ground(x, y, top=30.0):
    hit = surface.ray_cast(mathutils.Vector((x, y, top)), mathutils.Vector((0, 0, -1)))[0]
    return hit.z if hit else 0.0


for k, t in TIPS.items():
    kit.socket('socket_horn_' + k, (t.x, t.y, t.z - 0.1 * S))
for k, t in tower_tops.items():
    kit.socket('socket_tower_' + k, (t.x, t.y, t.z + 0.15 * S))
# The reactor: its light hangs before the glass, its motes rise round the glass from its foot,
# the surges flare at the cap.
kit.socket('socket_core', (AXIS.x, AXIS.y - COL_R - 1.2 * S, (G_Z0 + G_Z1) / 2))
kit.socket('socket_reactor', (AXIS.x, AXIS.y, G_Z0))
kit.socket('socket_cap', (AXIS.x, AXIS.y, CAP_FOOT.z + 0.4 * S))
kit.socket('socket_crown', (AXIS.x, AXIS.y, (CAP_TOP + CAP_FOOT.z) / 2))
fang_tip = float(hv[:, 1].min())
kit.socket('socket_gate', (HEAD_X, fang_tip - 0.4 * S, ground(HEAD_X, fang_tip - 0.4 * S) + 0.1 * S))
# Four points on the wall's top, two at the front, one on each side (searched inward from the
# edge, so they sit on the stone and not in a gap between merlons).
RIM = {'a': ((-6.0, -9.5), (0, 1)), 'b': ((9.0, 2.5), (-1, 0)), 'c': ((5.5, -9.5), (0, 1)), 'd': ((-9.0, 4.5), (1, 0))}
for k, ((x, y), (dx, dy)) in RIM.items():
    for step in range(12):
        px_, py_ = (x + dx * 0.15 * step) * S, (y + dy * 0.15 * step) * S
        z = ground(px_, py_)
        if z > 1.0 * S:
            break
    kit.socket('socket_rivet_' + k, (px_, py_, z + 0.05 * S))


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


OFF = 0.001  # arcs collapsed into their horn tips
ARC_ORIGINS = {a: a.location.copy() for a in arcs}
CAP_REST = cap.location.copy()


def ease(keys, f):
    """`keys` (frame, value) eased (cosine) between neighbours, at frame `f`."""
    ks = sorted(keys)
    if f <= ks[0][0]:
        return ks[0][1]
    for (f0, v0), (f1, v1) in zip(ks, ks[1:]):
        if f0 <= f <= f1:
            t = (f - f0) / max(f1 - f0, 1e-6)
            return v0 + (v1 - v0) * (0.5 - 0.5 * math.cos(math.pi * t))
    return ks[-1][1]


def dense(frames, fn):
    """Keys every other frame (and on the last) from `fn(frame)`: the shell and everything
    riding it are sampled from one function, so they never drift apart."""
    fs = list(range(0, frames, 2)) + [frames]
    return [(f, fn(f)) for f in fs]


def tracks(frames, part=None, lift=None, breathe=None, swell=None, turn=None, kick=None, arcs_on=None):
    """Every moving node's channels over `frames`; None holds the rest pose. `part` opens the
    claws (radians, outward), `lift` raises the head's chin (radians), `breathe` swells the
    shell (share of its height), `swell` maps a boiler to its swelling keys (share), `turn`
    turns the cap (radians), `kick` lifts it (metres), `arcs_on` maps an arc to its scale keys."""
    rest = [(0, 0.0), (frames, 0.0)]
    part, lift, b = part or rest, lift or rest, breathe or rest
    turn, kick = turn or rest, kick or rest
    out = []
    lin = 'LINEAR'
    for side, m in mandibles.items():
        sign = -1.0 if side == 'l' else 1.0  # about Z: the left claw turns clockwise to open
        out.append((m, 'rotation_euler', 2, dense(frames, lambda f: sign * ease(part, f)), lin))
        # The claws tip up a little as they part (their points reach forward and down, so a
        # negative turn about X lifts them), so their points clear the floor.
        out.append((m, 'rotation_euler', 0, dense(frames, lambda f: -0.25 * ease(part, f)), lin))
    out.append((head, 'rotation_euler', 0, dense(frames, lambda f: -ease(lift, f)), lin))
    for i, share in ((0, 0.4), (1, 0.4), (2, 1.0)):
        grow = lambda f, share=share: 1.0 + share * ease(b, f)  # noqa: E731
        out.append((shell, 'scale', i, dense(frames, grow), lin))

        def ride(point, extra=None, i=i, grow=grow):
            """A node at `point` riding the breathing shell (plus `extra(f)` on this axis)."""
            return dense(frames, lambda f: SHELL_FOOT[i] + (point[i] - SHELL_FOOT[i]) * grow(f) + (extra(f) if extra else 0.0))
        # The cap rides the shell (its foot on the breathing column), turns and kicks up.
        out.append((cap, 'location', i, ride(CAP_REST, (lambda f: ease(kick, f)) if i == 2 else None), lin))
        out.append((cap, 'scale', i, dense(frames, grow), lin))
        for a in arcs:
            out.append((a, 'location', i, ride(ARC_ORIGINS[a]), lin))
    out.append((cap, 'rotation_euler', 2, dense(frames, lambda f: ease(turn, f)), lin))
    for k, bo in boilers.items():
        keys = (swell or {}).get(k, rest)
        out.append((bo, 'scale', 2, dense(frames, lambda f, keys=keys: 1.0 + ease(keys, f)), lin))
        out.append((bo, 'scale', 0, dense(frames, lambda f, keys=keys: 1.0 + 0.6 * ease(keys, f)), lin))
        out.append((bo, 'scale', 1, dense(frames, lambda f, keys=keys: 1.0 + 0.6 * ease(keys, f)), lin))
    for a in arcs:
        keys = (arcs_on or {}).get(a, [(0, OFF), (frames, OFF)])
        for i in range(3):
            out.append((a, 'scale', i, keys, 'CONSTANT'))
    return out


def flicker(frames, seed, on_share, lo=3, hi=7, start=10, end=None):
    """Constant on/off keys for an arc: short bursts at pseudo-random frames."""
    rng = np.random.default_rng(seed)
    end = frames - 8 if end is None else end
    keys, f = [(0, OFF)], start + int(rng.integers(0, 10))
    while f < end:
        length = int(rng.integers(lo, hi))
        keys += [(f, float(rng.uniform(0.85, 1.0))), (f + length, OFF)]
        f += length + int(rng.integers(2, max(3, int(length / on_share))))
    keys.append((frames, OFF))
    return keys


clip('idle', tracks(2))

# work (6 s): the claws part slowly, stand open working a little, and close; the head lifts its
# chin with them; the shell breathes three times and the cap kicks up on each breath (`surge`)
# while it turns one way and back; the boilers pump in turn round the wall (front left, back
# left, back right, front right) twice, each venting at its peak; the arcs crackle throughout.
# Rivet sparks round the rim, alternating between two pairs of sockets.
W = 180
OPEN = 0.42
part = [(0, 0.0), (45, OPEN), (70, OPEN * 0.85), (95, OPEN), (120, OPEN * 0.85), (140, OPEN), (W, 0.0)]
lift = [(0, 0.0), (45, 0.14), (95, 0.11), (140, 0.14), (W, 0.0)]
BREATH = 0.025
breathe = [(0, 0.0)]
for i in range(3):
    breathe += [(i * 60 + 30, BREATH), (i * 60 + 60, 0.0)]
SURGES = [26, 86, 146]
KICK = 0.32 * S
kick = [(0, 0.0)]
for f in SURGES:
    kick += [(f - 6, 0.0), (f, KICK), (f + 5, KICK * 0.8), (f + 22, 0.0)]
kick.append((W, 0.0))
TURN = 0.45
turn = [(0, 0.0), (60, TURN), (120, -TURN * 0.6), (W, 0.0)]
ORDER = ['fl', 'bl', 'br', 'fr']
STEP, RISE, SWELL = W // 8, 7, 0.1
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
work_arcs = {a: flicker(W, 101 + i, 0.55) for i, a in enumerate(arcs)}
clip('work', tracks(W, part, lift, breathe, swell, turn, kick, work_arcs))

# spawn (2 s): the claws snap open wide and the head rears, the cap jolts up and every arc fires
# (steam bursts at the gate and the reactor flashes on `gate`) as a unit comes out; then they
# settle shut.
SP, GATE = 60, 2
WIDE = 0.58  # with the tip-up, the claws stay within 0.6 rad of rest (clip blends lerp matrices)


def spawn_arc(i):
    keys = [(0, OFF)]
    for n, f in enumerate(range(2, 34, 4)):
        keys.append((f, 1.0 if (n + i) % 2 == 0 else OFF))
    return keys + [(36, OFF), (SP, OFF)]


clip('spawn', tracks(SP, part=[(0, 0.0), (6, WIDE), (12, WIDE * 0.93), (38, WIDE), (SP, 0.0)],
                     lift=[(0, 0.0), (6, 0.24), (38, 0.22), (SP, 0.0)],
                     breathe=[(0, 0.0), (8, BREATH * 1.3), (SP, 0.0)],
                     kick=[(0, 0.0), (5, KICK * 1.4), (12, KICK * 1.1), (34, KICK * 1.2), (SP, 0.0)],
                     turn=[(0, 0.0), (8, 0.3), (SP, 0.0)],
                     arcs_on={a: spawn_arc(i) for i, a in enumerate(arcs)}))

all_pts = np.concatenate([verts(o) for o in bpy.context.scene.objects if o.type == 'MESH' and o not in arcs])
height = max(float(all_pts[:, 2].max()) + KICK * 1.4, SHELL_FOOT.z + (SHELL_TOP - SHELL_FOOT.z) * (1 + BREATH * 1.3))
tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
print('triangles', tris, 'height', round(height, 2), 'extent x', all_pts[:, 0].min().round(2), all_pts[:, 0].max().round(2),
      'y', all_pts[:, 1].min().round(2), all_pts[:, 1].max().round(2))

events = ([{'name': 'rivet', 'time': round(f / kit.FPS, 3)} for f in RIVETS]
          + [{'name': 'rivet-2', 'time': round(f / kit.FPS, 3)} for f in RIVETS_2]
          + [{'name': 'surge', 'time': round(f / kit.FPS, 3)} for f in SURGES]
          + [{'name': n, 'time': round(f / kit.FPS, 3)} for n, f in vents])
asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Elder Bastion', 'kind': 'building',
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
               'concept for Under the Canopy; animated by tools/buildings/elder_bastion.py.',
}
for o in bpy.data.objects:
    if o.name.startswith('socket_'):
        print('socket', o.name, [round(v, 2) for v in o.location])
kit.export(OUT, NAME, asset)
