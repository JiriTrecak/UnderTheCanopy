"""Grades a unit asset's base colour, keeping its team mask, and exports it again:

    /Applications/Blender.app/Contents/MacOS/Blender -b <asset>/source.blend -P tools/units/grade_unit.py -- \\
        '{"saturation": 0.7, "gamma": 0.85, "hue": 0.0, "warm": 0.0}'

`objects` limits it to those objects' materials ("objects": ["sword"]). Optional `bands` grade one hue range each (see below), e.g. muting gold and browning red chitin:
    '{"bands": [{"from": 0.06, "to": 0.17, "saturation": 0.8, "value": 0.8},
                {"from": 0.94, "to": 0.06, "shift": 0.015, "saturation": 0.88, "value": 0.88}]}'

Per texel outside the team plates (the base colour's alpha): saturation is scaled, value is
lifted by `gamma` (below 1 brightens the darks most), and reds/oranges (the chitin) turn by
`hue` (a share of the colour wheel; 0.03 pushes red towards amber). Gold keeps its hue. The
team plates are left as they are: the game paints them in the player's colour. The asset's
geometry.glb, source.blend and asset.json are rewritten in place."""
import colorsys, json, os, sys

import bpy
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

GRADE = json.loads(sys.argv[sys.argv.index('--') + 1])
OUT = os.path.dirname(bpy.data.filepath)
NAME = os.path.basename(OUT)
ASSET = json.load(open(os.path.join(OUT, 'asset.json')))


def rgb_to_hsv(rgb):
    r, g, b = rgb[..., 0], rgb[..., 1], rgb[..., 2]
    mx, mn = rgb.max(-1), rgb.min(-1)
    d = mx - mn
    h = np.zeros_like(mx)
    m = d > 1e-6
    rm = m & (mx == r)
    gm = m & (mx == g) & ~rm
    bm = m & ~rm & ~gm
    h[rm] = ((g - b)[rm] / d[rm]) % 6
    h[gm] = (b - r)[gm] / d[gm] + 2
    h[bm] = (r - g)[bm] / d[bm] + 4
    h = h / 6
    s = np.where(mx > 1e-6, d / np.maximum(mx, 1e-6), 0)
    return h, s, mx


def hsv_to_rgb(h, s, v):
    i = np.floor(h * 6).astype(int) % 6
    f = h * 6 - np.floor(h * 6)
    p, q, t = v * (1 - s), v * (1 - f * s), v * (1 - (1 - f) * s)
    out = np.stack([np.choose(i, [v, q, p, p, t, v]), np.choose(i, [t, v, v, q, p, p]), np.choose(i, [p, p, t, v, v, q])], -1)
    return out


# `objects` limits the grade to the materials of those objects (a held sword, say).
only = {m for o in bpy.data.objects if o.name in GRADE.get('objects', []) for m in o.data.materials} if GRADE.get('objects') else None
graded = set()
for mat in bpy.data.materials:
    if not mat.get('teamColorMask') or (only is not None and mat not in only):
        continue
    bsdf = next((n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
    node = bsdf.inputs['Base Color'].links[0].from_node if bsdf and bsdf.inputs['Base Color'].links else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    if node is None or node.image.name in graded:
        continue
    img = node.image
    graded.add(img.name)
    w, h = img.size
    px = np.array(img.pixels[:], np.float32).reshape(h, w, 4)
    team = px[..., 3]
    hh, ss, vv = rgb_to_hsv(px[..., :3])
    red = np.clip(1 - np.minimum(hh, 1 - hh) / 0.07, 0, 1)  # reds and red-oranges, not gold
    hh = (hh + GRADE.get('hue', 0.0) * red) % 1.0
    ss = ss * GRADE.get('saturation', 1.0)
    vv = np.power(np.clip(vv, 0, 1), GRADE.get('gamma', 1.0))
    # Hue bands: {"from", "to", "shift", "saturation", "value"} change only the texels whose hue
    # lies in [from, to) (wrapping past 1), faded in over 0.02 at each end; grey texels are spared.
    for band in GRADE.get('bands', []):
        a, b = band['from'], band['to']
        span = (b - a) % 1.0
        x = (hh - a) % 1.0
        w = np.clip(np.minimum(x, span - x) / 0.02, 0, 1) * (x < span) * np.clip((ss - 0.15) / 0.1, 0, 1)
        hh = (hh + band.get('shift', 0.0) * w) % 1.0
        ss = ss * (1 + (band.get('saturation', 1.0) - 1) * w)
        vv = vv * (1 + (band.get('value', 1.0) - 1) * w)
    rgb = hsv_to_rgb(hh, np.clip(ss, 0, 1), vv)
    keep = team[..., None] > 0.5
    px[..., :3] = np.where(keep, px[..., :3], rgb)
    img.pixels[:] = px.ravel()
    img.pack()
    print('graded', img.name, GRADE)

kit.export(OUT, NAME, ASSET)
