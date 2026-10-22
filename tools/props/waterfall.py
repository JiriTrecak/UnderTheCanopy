"""A forest-floor waterfall for the Underleaf biome's maps (Founding: where the glade's
stream drops into the ravine), procedural in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/waterfall.py

The block terrain makes still water at whole levels, so a stream meeting a lower pool shows
no fall; this piece is the fall. Three parts, all soft-alpha (blended) and static (scenery
never animates):
- the sheet: a band of falling water the stream's width, pouring over the lip (local +Z is
  downstream), bowing out and dropping almost straight; vertical streaks of white water
  over tea-dark glassy water, thinning to nothing at the sides;
- the foam: a ragged disc of white water and bubbles where it lands, brightest under the
  sheet;
- the froth: a few low, pale mounds of churned water at the foot of the sheet.

Sizes in metres; the pivot is at the foot, on the lower water: place it with its height
mode absolute at the lower water's surface (Underleaf deep water at level 0: -0.4) and the
lip stands DROP above (6.2: a stream at level 2). Nothing blocks.

Writes base/assets/environment/underleaf/glade-waterfall/ (geometry.glb, source.blend,
asset.json)."""
import json, math, os, random

import bpy, bmesh
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
NAME = 'glade-waterfall'
OUT = os.path.join(ROOT, 'base', 'assets', 'environment', 'underleaf', NAME)
WIDTH = 11.0
DROP = 6.2
rng = np.random.default_rng(31)


def image(name, rgba):
    h, w = rgba.shape[:2]
    img = bpy.data.images.new(name, w, h, alpha=True)
    img.pixels[:] = rgba.astype(np.float32).ravel()
    img.pack()
    return img


def blended(name, img, roughness=0.15, emission=0.0):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes['Principled BSDF']
    tex = nt.nodes.new('ShaderNodeTexImage')
    tex.image = img
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    nt.links.new(tex.outputs['Alpha'], bsdf.inputs['Alpha'])
    bsdf.inputs['Roughness'].default_value = roughness
    if emission > 0:
        nt.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
        bsdf.inputs['Emission Strength'].default_value = emission
    mat.surface_render_method = 'BLENDED'
    mat.use_backface_culling = False
    return mat


def sheet_texture(w=256, h=512):
    """Streaks of white water falling (V down the sheet) over dark glassy water."""
    u = np.linspace(0, 1, w, endpoint=False)
    v = np.linspace(0, 1, h, endpoint=False)[:, None]
    streak = np.zeros((h, w), np.float32)
    for k in range(70):
        x0 = rng.uniform(0, 1)
        width = rng.uniform(0.004, 0.02)
        start, length = rng.uniform(-0.3, 0.6), rng.uniform(0.3, 1.2)
        wob = rng.uniform(0, 6.3)
        cx = x0 + 0.006 * np.sin(v * 9 + wob)
        line = np.exp(-((u[None, :] - cx) / width) ** 2)
        along = np.clip((v - start) / 0.15, 0, 1) * np.clip((start + length - v) / 0.2, 0, 1)
        streak = np.maximum(streak, line * along * rng.uniform(0.5, 1.0))
    # More white water lower down, where the fall breaks up.
    streak = np.clip(streak * (0.7 + 0.6 * v) + 0.25 * v ** 3, 0, 1)
    side = np.clip(np.minimum(u, 1 - u) / 0.12, 0, 1)[None, :] ** 0.8
    water = np.array((0.16, 0.2, 0.16), np.float32)
    white = np.array((0.86, 0.92, 0.9), np.float32)
    rgb = water * (1 - streak[..., None]) + white * streak[..., None]
    alpha = (0.45 + 0.5 * streak) * side
    return np.concatenate([rgb, alpha[..., None]], axis=2)


def foam_texture(size=256):
    """A ragged ring of foam and bubbles, densest in the middle, fading out."""
    u = np.linspace(-1, 1, size)
    x, y = np.meshgrid(u, u)
    r = np.sqrt(x * x + y * y)
    noise = np.zeros((size, size), np.float32)
    for k in (3, 7, 13, 25):
        p = rng.uniform(0, 6.3, 4)
        noise += (np.sin(x * k + p[0] + np.sin(y * k * 0.7 + p[1])) * np.sin(y * k + p[2] + np.sin(x * k * 0.6 + p[3]))) / k ** 0.6
    noise = (noise - noise.min()) / (noise.max() - noise.min())
    dots = np.zeros((size, size), np.float32)
    for _ in range(260):
        cx, cy, rr = rng.uniform(-0.9, 0.9), rng.uniform(-0.9, 0.9), rng.uniform(0.01, 0.035)
        dots = np.maximum(dots, np.clip(1 - np.abs(np.sqrt((x - cx) ** 2 + (y - cy) ** 2) - rr) / 0.008, 0, 1))
    body = np.clip(1.15 - r * 1.15, 0, 1) ** 1.3
    foam = np.clip(body * (0.45 + 0.75 * noise) + dots * body * 0.6, 0, 1)
    rgb = np.ones((size, size, 3), np.float32) * np.array((0.84, 0.9, 0.88), np.float32)
    return np.concatenate([rgb, np.clip(foam * 1.2, 0, 0.97)[..., None]], axis=2)


bpy.ops.wm.read_factory_settings(use_empty=True)
# --- The sheet: rows down the fall, columns across it; it bows out over the lip.
mat_sheet = blended(NAME + '-sheet', image(NAME + '-sheet', sheet_texture()), 0.12, 0.25)
bm = bmesh.new()
uv = bm.loops.layers.uv.verify()
rows, cols = 18, 12
grid = []
for j in range(rows + 1):
    t = j / rows
    # Over the lip (a quarter turn of 1.2 m), then down a steep cascade that spreads as it
    # falls: seen from the game's camera above, a ribbon of white water, not an edge.
    if t < 0.12:
        a = t / 0.12 * math.pi / 3
        z, y = 1.2 * math.sin(a), DROP + 1.2 * (math.cos(a) - 1)
    else:
        s = (t - 0.12) / 0.88
        step = 0.25 * math.sin(s * math.pi * 3) ** 2
        z, y = 1.04 + 6.0 * s ** 0.85 + step, (DROP - 0.6) * (1 - s) ** 1.15
    row = []
    for i in range(cols + 1):
        u = i / cols
        x = (u - 0.5) * WIDTH * (1 + 0.3 * t)
        ripple = 0.08 * math.sin(i * 1.7 + j * 0.9)
        row.append((bm.verts.new((x, y, z + ripple)), (u, 1 - t)))
    grid.append(row)
for j in range(rows):
    for i in range(cols):
        quad = [grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]]
        f = bm.faces.new([q[0] for q in quad])
        for loop, (_, tt) in zip(f.loops, quad):
            loop[uv].uv = tt
me = bpy.data.meshes.new(NAME + '-sheet')
bm.to_mesh(me)
bm.free()
sheet = bpy.data.objects.new(NAME + '-sheet', me)
bpy.context.scene.collection.objects.link(sheet)
me.materials.append(mat_sheet)
for p in me.polygons:
    p.use_smooth = True

# --- The foam: a disc on the lower water, centred a little downstream of the sheet's foot.
mat_foam = blended(NAME + '-foam', image(NAME + '-foam', foam_texture()), 0.5, 0.15)
bm = bmesh.new()
uv = bm.loops.layers.uv.verify()
radius, n = 8.0, 40
centre = bm.verts.new((0, 0.3, 7.6))
ring = []
for k in range(n):
    a = 2 * math.pi * k / n
    rr = radius * (1 + 0.12 * math.sin(3 * a + 1.0) + 0.06 * math.sin(7 * a))
    ring.append(bm.verts.new((rr * math.cos(a) * 1.15, 0.3, 7.6 + rr * math.sin(a) * 0.75)))
for k in range(n):
    f = bm.faces.new([centre, ring[(k + 1) % n], ring[k]])
    for loop in f.loops:
        co = loop.vert.co
        loop[uv].uv = (0.5 + co.x / (2 * radius * 1.35), 0.5 + (co.z - 7.6) / (2 * radius * 1.15))
me = bpy.data.meshes.new(NAME + '-foam')
bm.to_mesh(me)
bm.free()
foam = bpy.data.objects.new(NAME + '-foam', me)
bpy.context.scene.collection.objects.link(foam)
me.materials.append(mat_foam)

# --- The froth: low pale mounds of churned water along the sheet's foot.
froth_rgba = np.ones((16, 16, 4), np.float32) * np.array((0.86, 0.92, 0.9, 0.7), np.float32)
mat_froth = blended(NAME + '-froth', image(NAME + '-froth', froth_rgba), 0.6, 0.2)
pr = random.Random(5)
parts = []
for k in range(9):
    bpy.ops.mesh.primitive_uv_sphere_add(segments=12, ring_count=6, radius=1.0)
    ob = bpy.context.active_object
    s = pr.uniform(0.7, 1.4)
    ob.scale = (s * 1.4, s * 0.45, s)
    ob.location = (pr.uniform(-WIDTH * 0.6, WIDTH * 0.6), 0.1, pr.uniform(6.4, 8.0))
    parts.append(ob)
bpy.ops.object.select_all(action='DESELECT')
for ob in parts:
    ob.select_set(True)
bpy.context.view_layer.objects.active = parts[0]
bpy.ops.object.join()
froth = bpy.context.view_layer.objects.active
froth.name = NAME + '-froth'
bpy.ops.object.transform_apply(location=False, rotation=False, scale=True)
froth.data.materials.clear()
froth.data.materials.append(mat_froth)
for p in froth.data.polygons:
    p.use_smooth = True

os.makedirs(OUT, exist_ok=True)
bpy.ops.wm.save_as_mainfile(filepath=os.path.join(OUT, 'source.blend'))
bpy.ops.export_scene.gltf(filepath=os.path.join(OUT, 'geometry.glb'), export_format='GLB',
                          export_yup=True, export_apply=True, export_image_format='AUTO')
with open(os.path.join(OUT, 'asset.json'), 'w') as f:
    json.dump({
        'id': 'asset.models.environment.' + NAME, 'name': 'Glade waterfall',
        'kind': 'prop', 'tags': ['underleaf', 'glade', 'water', 'procedural'],
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'vegetationClearance': 0, 'groundContact': {'mode': 'terrain'}},
        'license': 'Original procedural model for Under the Canopy (tools/props/waterfall.py).',
    }, f, indent=2, ensure_ascii=False)
    f.write('\n')
print('built', NAME)
