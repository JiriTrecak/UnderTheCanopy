"""The ants' nest-cast buildings (an ivory nest cast crowned with royal blue, 2026-11-02) from
their Tripo models, as look tests:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/nest_cast.py -- \\
        <tripo model.glb> <asset folder name, e.g. ants-baobab-nest> "<asset name>" \\
        [--footprint <cells>] [--fit base] [--height <m>]

The model is stretched to fill its whole square footprint (`--footprint` metres both ways, 20 by
default, its height scaled with the width), centred, with the ground at 0 and the gate facing -Y.
`--fit base` fills the square with its foot (the lowest tenth of its height) and lets a crown
overhang it; `--height` sets its height instead of following the width. The royal
blue goes into the base colour's alpha as the team mask. No moving parts yet: `idle` and `work`
hold the rest pose (D65), `work` carries the `bloom` event the hall's effects use, and the
sockets are the highest point (`socket_crown`) and the gate (`socket_gate`)."""
import os, sys

import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ARGS = sys.argv[sys.argv.index('--') + 1:]
SRC, NAME, TITLE = ARGS[:3]
FIT_BASE = '--fit' in ARGS and ARGS[ARGS.index('--fit') + 1] == 'base'
TARGET_HEIGHT = float(ARGS[ARGS.index('--height') + 1]) if '--height' in ARGS else None
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
FOOTPRINT = float(ARGS[ARGS.index('--footprint') + 1]) if '--footprint' in ARGS else 20.0

kit.reset()
bpy.ops.import_scene.gltf(filepath=SRC)
meshes = [o for o in bpy.data.objects if o.type == 'MESH']
for o in meshes:
    o.data.transform(o.matrix_world)
    o.parent = None
    o.matrix_world = mathutils.Matrix.Identity(4)
for o in [o for o in bpy.data.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
if len(meshes) > 1:
    with bpy.context.temp_override(active_object=meshes[0], selected_editable_objects=meshes):
        bpy.ops.object.join()
body = meshes[0]
body.name = body.data.name = 'body'

# --- Fill the square: each horizontal axis to the footprint, height with the width.
pts = [v.co for v in body.data.vertices]
lo = mathutils.Vector([min(p[i] for p in pts) for i in range(3)])
hi = mathutils.Vector([max(p[i] for p in pts) for i in range(3)])
if FIT_BASE:
    foot = [p for p in pts if p.z < lo.z + 0.1 * (hi.z - lo.z)]
    lo.x, lo.y = min(p.x for p in foot), min(p.y for p in foot)
    hi.x, hi.y = max(p.x for p in foot), max(p.y for p in foot)
sx, sy = FOOTPRINT / (hi.x - lo.x), FOOTPRINT / (hi.y - lo.y)
sz = TARGET_HEIGHT / (hi.z - lo.z) if TARGET_HEIGHT else sx
body.data.transform(mathutils.Matrix.Diagonal((sx, sy, sz, 1)) @ mathutils.Matrix.Translation(
    (-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z)))
for p in body.data.polygons:
    p.use_smooth = True
HEIGHT = (hi.z - lo.z) * sz
print('stretched %.2f x %.2f to %.0f x %.0f m (height x%.2f of the width), height %.2f m' % (
    (hi.x - lo.x) * sx, (hi.y - lo.y) * sx, FOOTPRINT, FOOTPRINT, sz / sx, HEIGHT))

# --- Team mask: the caps' royal blue into the base colour's alpha.
mat = body.data.materials[0]
mat.name = 'TC_%s_body' % NAME.replace('-', '_')
link = mat.node_tree.nodes['Principled BSDF'].inputs['Base Color'].links
node = link[0].from_node
while node.type != 'TEX_IMAGE':
    node = node.inputs[0].links[0].from_node
W, H = node.image.size
rgb = np.array(node.image.pixels[:], np.float32).reshape(H, W, 4)[:, :, :3].copy()
blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
team = np.clip((blue - 0.04) / 0.1, 0.0, 1.0)

# The player colour is shaded by the square root of the texel's peak channel, which is nearly
# flat across a royal-blue leaf: stretch the crown's own darks and lights (veins, folds) so they
# shade every player colour from half to full brightness, as a painted leaf would.
crown = team > 0.5

# `--ivory <linear>`: the brightest tenth of the nest cast's albedo (outside the crown) brought down
# to this, so near-white bone does not glow against a dark ground by night (2026-11-02, a test).
if '--ivory' in ARGS:
    target = float(ARGS[ARGS.index('--ivory') + 1])
    lin = np.where(rgb <= 0.04045, rgb / 12.92, ((rgb + 0.055) / 1.055) ** 2.4)
    lum = lin @ np.array([0.2126, 0.7152, 0.0722], np.float32)
    k = min(1.0, target / float(np.percentile(lum[~crown], 90)))
    lin = lin * (1 + (k - 1) * (1 - team))[..., None]
    rgb = np.where(lin <= 0.0031308, lin * 12.92, 1.055 * np.maximum(lin, 0) ** (1 / 2.4) - 0.055).astype(np.float32)
    print('ivory x%.2f (linear)' % k)
peak = rgb.max(axis=2)
dark, light = np.percentile(peak[crown], [2, 98])
shade = 0.5 + 0.5 * np.clip((peak - dark) / max(light - dark, 1e-4), 0.0, 1.0)


# --- Leaf veins (Jiri, 2026-11-02: the flat blue read as plastic, and every ant leaf gets them).
# Each separate blue leaf is found on the mesh. Its veins are painted into the shading in its own
# frame: a round leaf (the Nest's umbrella) gets primaries radiating from where its stalk meets
# it; a long one (a crest blade, a mound's leaf) a midrib from base to tip with side veins. All
# veins scale with the leaf, over a fine net of cells, mixed over Tripo's own folds.
def leaves():
    """The crown's leaves: lists of polygon indices, connected through shared vertices, of the
    polygons whose UV centre lies on the team mask."""
    me = body.data
    uv = me.uv_layers.active.data
    on = []
    for p in me.polygons:
        u, v = np.mean([uv[i].uv[:] for i in p.loop_indices], axis=0)
        on.append(team[min(H - 1, int(v * H)), min(W - 1, int(u * W))] > 0.5)
    parent = list(range(len(me.vertices)))

    def find(i):
        while parent[i] != i:
            parent[i] = parent[parent[i]]
            i = parent[i]
        return i
    # glTF splits vertices along UV seams: rejoin those standing at one place.
    first = {}
    for v in me.vertices:
        key = tuple(np.round(v.co[:], 4))
        if key in first:
            parent[find(v.index)] = find(first[key])
        else:
            first[key] = v.index
    for p in me.polygons:
        if on[p.index]:
            for a, b in zip(p.vertices, p.vertices[1:]):
                parent[find(a)] = find(b)
    groups = {}
    for p in me.polygons:
        if on[p.index]:
            groups.setdefault(find(p.vertices[0]), []).append(p.index)
    return on, [g for g in groups.values() if len(g) >= 6]


def frame(polys, on):
    """A leaf's base (where it meets the rest), tip (furthest from the base), length and shape."""
    me = body.data
    mine = {i for p in polys for i in me.polygons[p].vertices}
    pts = np.array([me.vertices[i].co[:] for i in mine])
    joined = set()
    for p in me.polygons:
        if not on[p.index]:
            joined.update(i for i in p.vertices if i in mine)
    if joined:
        base = np.mean([me.vertices[i].co[:] for i in joined], axis=0)
    else:
        base = pts[np.argmin(pts[:, 2])]
    tip = pts[np.argmax(np.linalg.norm(pts - base, axis=1))]
    centred = pts - pts.mean(0)
    spread = np.sqrt(np.linalg.eigvalsh(np.cov(centred.T)))[::-1]
    return base, tip, float(np.linalg.norm(tip - base)), spread[0] / max(spread[1], 1e-4) < 1.6


def raster(polys, out_xyz, out_id, leaf):
    """Writes each texel of the leaf's polygons its model position and the leaf's number."""
    me = body.data
    uv = me.uv_layers.active.data
    me.calc_loop_triangles()
    wanted = set(polys)
    for tri in me.loop_triangles:
        if tri.polygon_index not in wanted:
            continue
        t = np.array([uv[i].uv[:] for i in tri.loops]) * (W, H) - 0.5
        p = np.array([me.vertices[i].co[:] for i in tri.vertices], np.float32)
        x0, y0 = np.maximum(np.floor(t.min(0)).astype(int), 0)
        x1, y1 = np.minimum(np.ceil(t.max(0)).astype(int), (W - 1, H - 1))
        if x1 < x0 or y1 < y0:
            continue
        gx, gy = np.meshgrid(np.arange(x0, x1 + 1), np.arange(y0, y1 + 1))
        (ax, ay), (bx, by), (cx, cy) = t
        d = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
        if abs(d) < 1e-9:
            continue
        w0 = ((by - cy) * (gx - cx) + (cx - bx) * (gy - cy)) / d
        w1 = ((cy - ay) * (gx - cx) + (ax - cx) * (gy - cy)) / d
        w2 = 1 - w0 - w1
        inside = (w0 > -0.02) & (w1 > -0.02) & (w2 > -0.02)
        xyz = w0[..., None] * p[0] + w1[..., None] * p[1] + w2[..., None] * p[2]
        out_xyz[gy[inside], gx[inside]] = xyz[inside]
        out_id[gy[inside], gx[inside]] = leaf


def areoles(q, cell):
    """The fine net between veins: the edges of a jittered cell grid over points q (n x 3)."""
    u, v = (q[:, 0] + 0.37 * q[:, 2]) / cell, (q[:, 1] - 0.29 * q[:, 2]) / cell
    iu, iv = np.floor(u), np.floor(v)
    f1 = np.full(len(q), 9.0, np.float32)
    f2 = np.full(len(q), 9.0, np.float32)
    for du in (-1, 0, 1):
        for dv in (-1, 0, 1):
            cu, cv = iu + du, iv + dv
            hx = np.mod(np.sin(cu * 127.1 + cv * 311.7) * 43758.5453, 1.0)
            hy = np.mod(np.sin(cu * 269.5 + cv * 183.3) * 43758.5453, 1.0)
            dist = np.hypot(cu + 0.15 + 0.7 * hx - u, cv + 0.15 + 0.7 * hy - v)
            f2 = np.where(dist < f1, f1, np.minimum(f2, dist))
            f1 = np.minimum(f1, dist)
    return 1 - np.clip((f2 - f1 - 0.04) / 0.1, 0.0, 1.0)


def veins(q, base, tip, length, round_leaf):
    """A leaf's brightness, 0 to 1, and its relief (1 on the leaf, lower in its vein grooves)
    at points q (n x 3)."""
    rel = q - base
    if round_leaf:
        # Primaries radiating from the stalk, measured around the leaf's own up axis.
        r = np.linalg.norm(rel, axis=1)
        sector = 2 * np.pi / 11
        delta = np.mod(np.arctan2(rel[:, 1], rel[:, 0]) + 0.3, sector) - sector / 2
        along, across = r * np.cos(delta), np.abs(r * np.sin(delta))
    else:
        axis = (tip - base) / max(length, 1e-4)
        along = rel @ axis
        across = np.linalg.norm(rel - along[:, None] * axis, axis=1)
    fall = np.clip(along / max(length, 1e-4), 0.0, 1.0)
    width = length * (0.016 + 0.03 * (1 - fall))
    primary = 1 - np.clip((across - width * 0.5) / (width * 0.5), 0.0, 1.0)
    spacing = length / (6.0 if round_leaf else 7.0)
    t = (along - across * 1.1) / spacing
    side = 1 - np.clip((np.abs(t - np.round(t)) * spacing - length * 0.004) / (length * 0.008), 0.0, 1.0)
    side *= np.clip((along - length * 0.08) / (length * 0.08), 0.0, 1.0)
    net = areoles(q, max(0.18, length / 16))
    lamina = 0.3 - 0.12 * fall
    bright = np.clip(lamina + 0.12 * net + 0.42 * side + 0.7 * primary, 0.0, 1.0)
    return bright, 1 - np.clip(0.9 * primary + 0.55 * side + 0.12 * net, 0.0, 1.0)


on, found = leaves()
at = np.full((H, W, 3), np.nan, np.float32)
which = np.full((H, W), -1, np.int16)
relief = np.ones((H, W), np.float32)
frames = []
for n, polys in enumerate(found):
    frames.append(frame(polys, on))
    raster(polys, at, which, n)
for n, (base, tip, length, round_leaf) in enumerate(frames):
    sel = crown & (which == n)
    pattern, relief[sel] = veins(at[sel], base, tip, length, round_leaf)
    shade[sel] = 0.2 + 0.8 * (0.2 * (shade[sel] - 0.5) * 2 + 0.8 * pattern)
    print('leaf %d: %s, %.1f m, %d texels' % (n, 'round' if round_leaf else 'long', length, sel.sum()))
stored = (shade ** 2) ** (1 / 2.2)  # the sRGB peak whose linear square root is `shade`
rgb = rgb + (rgb * (stored / np.maximum(peak, 1e-4))[..., None] - rgb) * team[..., None]

# A soft leaf sheen: Tripo's PBR map leaves the leaf glossy, which reads as plastic, while a
# fully matte one hides its vein relief.
rough = mat.node_tree.nodes['Principled BSDF'].inputs['Roughness'].links[0].from_node
while rough.type != 'TEX_IMAGE':
    rough = rough.inputs[0].links[0].from_node
RW, RH = rough.image.size
mr = np.array(rough.image.pixels[:], np.float32).reshape(RH, RW, 4)
t = team[(np.arange(RH) * H // RH)[:, None], (np.arange(RW) * W // RW)[None, :]]
mr[:, :, 1] += (0.62 - mr[:, :, 1]) * t
mr[:, :, 2] *= 1.0 - t
rough.image.pixels[:] = mr.ravel()
rough.image.pack()

# Crown normals from the vein relief: Tripo's normal bake after Retopo breaks a smooth leaf into
# rectangular facets, so the crown's normals come from the painted veins alone (tangent space
# follows the UVs: x along u, y along v).
bump = mat.node_tree.nodes['Principled BSDF'].inputs['Normal'].links[0].from_node
while bump.type != 'TEX_IMAGE':
    bump = bump.inputs['Color'].links[0].from_node
NW, NH = bump.image.size
nm = np.array(bump.image.pixels[:], np.float32).reshape(NH, NW, 4)
t = team[(np.arange(NH) * H // NH)[:, None], (np.arange(NW) * W // NW)[None, :]][..., None]
f = H // NH
r = relief[:NH * f, :NW * f].reshape(NH, f, NW, f).mean(axis=(1, 3))
r = sum(np.roll(np.roll(r, dy, 0), dx, 1) for dy in (-1, 0, 1) for dx in (-1, 0, 1)) / 9
dv, du = np.gradient(r)
n = np.stack([-2.2 * du, -2.2 * dv, np.ones_like(r)], axis=-1)
n /= np.linalg.norm(n, axis=-1, keepdims=True)
nm[:, :, :3] += (0.5 + 0.5 * n - nm[:, :, :3]) * t
bump.image.pixels[:] = nm.ravel()
bump.image.pack()

img = bpy.data.images.new('%s-team' % NAME, W, H, alpha=True)
img.pixels[:] = np.concatenate([rgb, team[..., None]], axis=2).ravel()
img.alpha_mode = 'CHANNEL_PACKED'
img.pack()
node.image = img
mat['teamColorMask'] = 'baseColorAlpha'
print('team %.3f of the atlas' % team.mean())
# Tripo exports 4K maps; the game's buildings use 2K.
for im in bpy.data.images:
    if im.size[0] > 2048:
        im.scale(2048, 2048)
        im.pack()

# --- Sockets and still clips.
top = max(body.data.vertices, key=lambda v: v.co.z).co
kit.socket('socket_crown', (top.x, top.y, top.z + 0.4))
kit.socket('socket_gate', (0.0, -FOOTPRINT / 2 + 0.5, 1.5))
kit.clip(body, 'idle', [(0, 0.0), (2, 0.0)])
kit.clip(body, 'work', [(0, 0.0), (120, 0.0)])

asset = {
    'id': 'asset.models.buildings.%s' % NAME,
    'name': TITLE,
    'kind': 'building',
    'tags': ['ants', 'tripo', 'team-mask', 'nest-cast'],
    'race': 'ants',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'}, {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': [mat.name]},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            {'semantic': 'work', 'clip': 'work', 'loop': True, 'events': [{'name': 'bloom', 'time': 1.0}]},
        ],
        'blocker': {'shape': 'box', 'size': [FOOTPRINT, FOOTPRINT], 'offset': [0, 0]},
        'vegetationClearance': FOOTPRINT / 2,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(HEIGHT + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, retopology) from an ImageGen (gpt-image-2.5) concept for '
               'Under the Canopy; made game-ready by tools/buildings/nest_cast.py.',
}
kit.export(OUT, NAME, asset)
