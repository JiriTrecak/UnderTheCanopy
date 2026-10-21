"""Still pools of raw substance for the Underleaf biome's maps (the Hollow Brood: a colony
sickened by the forest's liquid magic), procedural in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/substance_pool.py

Substance is the forest's own magic: liquid, blue-violet, and it ALWAYS glows (as on the
deposit, docs/art-direction.md: never green). A pool is three parts:
- the liquid: a perfectly still, glossy sheet just above the ground, its emission a
  violet field of slow swirls and brighter veins, dimmer towards its rim, over-bright so
  the bloom picks it up;
- a crust round it: a low, pale, crystalline rim of mineral the substance left as it
  seeped, faintly lit from inside at its inner edge;
- crystal shards in clusters along the rim, tilted outwards, pale lilac and softly lit.

Outlines are seeded blobs, so the sizes differ in shape too. Pivot at the centre on the
ground; nothing blocks (decoration on the ground, units walk round or over it).

Writes base/assets/environment/underleaf/substance-pool-<size>/ (geometry.glb,
source.blend, asset.json)."""
import math, os, random, sys

import bpy, bmesh
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'assets', 'environment', 'underleaf')
VIOLET = np.array((0.55, 0.32, 1.0), np.float32)
LILAC = np.array((0.86, 0.78, 1.0), np.float32)

POOLS = [
    # name, radius (m), lobes seed, crystals, title
    ('substance-pool-small', 1.6, 11, 14, 'Substance pool, small'),
    ('substance-pool', 2.8, 23, 26, 'Substance pool'),
    ('substance-pool-large', 4.4, 37, 40, 'Substance pool, large'),
]


def liquid_texture(size=512, seed=3):
    """Slow clouds of violet with thin, soft lilac veins, brightest in the middle."""
    rng = np.random.default_rng(seed)
    u = np.linspace(-1, 1, size, endpoint=False)
    x, y = np.meshgrid(u, u)
    field = np.zeros((size, size), np.float32)
    for k in (1.2, 2.1, 3.7, 6.3, 9.8):
        p = rng.uniform(0, 2 * np.pi, 4)
        field += np.sin(k * x * 2.6 + np.sin(k * y * 2.0 + p[0]) * 1.2 + p[1]) / k ** 1.2
        field += np.sin(k * y * 2.4 + np.sin(k * x * 1.8 + p[2]) * 1.2 + p[3]) / k ** 1.2
    field = (field - field.min()) / (field.max() - field.min())
    veins = np.exp(-((field - 0.55) / 0.025) ** 2) * 0.5 + np.exp(-((field - 0.3) / 0.02) ** 2) * 0.3
    r = np.sqrt(x * x + y * y)
    core = np.clip(1.1 - r * 0.8, 0.3, 1.0)
    glow = (0.35 + 0.5 * field)[..., None] * VIOLET + veins[..., None] * LILAC * 0.45
    return (glow * core[..., None]).astype(np.float32)


def outline(radius, seed, n=96):
    """A seeded blob: a few soft lobes and smaller wanders, never a star."""
    rng = random.Random(seed)
    waves = [(k, rng.uniform(0.04, 0.11) / (1 + 0.35 * (k - 2)), rng.uniform(0, 6.3)) for k in range(2, 8)]
    stretch = rng.uniform(1.0, 1.35)
    pts = []
    for i in range(n):
        a = 2 * math.pi * i / n
        r = radius * (1 + sum(amp * math.sin(k * a + ph) for k, amp, ph in waves))
        pts.append((r * math.cos(a) * stretch, r * math.sin(a) / stretch ** 0.5))
    return pts


def build(name, radius, seed, crystals, title):
    kit.reset()
    pts = outline(radius, seed)
    rng = random.Random(seed * 7)
    # --- Liquid: a fan over the outline, UVs planar over the pool.
    liquid_img = kit._array_image(name + '-liquid', liquid_texture(seed=seed))
    mat_liquid = kit.material(name + '-liquid', color=(0.08, 0.04, 0.2), roughness=0.08,
                              emissive=liquid_img, emission=4.0)
    bm = bmesh.new()
    uv = bm.loops.layers.uv.verify()
    level = 0.1
    centre = bm.verts.new((0, 0, level))
    ring = [bm.verts.new((x * 1.02, y * 1.02, level)) for x, y in pts]
    extent = radius * 1.4
    for i in range(len(ring)):
        f = bm.faces.new([centre, ring[i], ring[(i + 1) % len(ring)]])
        for loop in f.loops:
            loop[uv].uv = (0.5 + loop.vert.co.x / (2 * extent), 0.5 + loop.vert.co.y / (2 * extent))
    me = bpy.data.meshes.new(name + '-liquid')
    bm.to_mesh(me)
    bm.free()
    liquid = kit.link(bpy.data.objects.new(name + '-liquid', me))
    kit.finish(liquid, mat_liquid, smooth=False)
    # --- Crust: a low rim along the outline, its profile from the ground up to a crest and
    # down to the liquid; the inner lip's UV v is 1, where the crust texture glows.
    crust_rgb = np.ones((64, 64, 3), np.float32)
    v = np.linspace(0, 1, 64)[:, None]
    crust_rgb *= (0.62 + 0.1 * np.sin(np.linspace(0, 40, 64))[None, :, None] * 0.3)
    crust_rgb = crust_rgb * np.array((0.8, 0.76, 0.9), np.float32)
    crust_img = kit._array_image(name + '-crust', crust_rgb)
    glow = (np.clip((v - 0.55) / 0.45, 0, 1) ** 2)[..., None] * np.ones((64, 64, 1), np.float32) * VIOLET
    glow_img = kit._array_image(name + '-crust-glow', glow.astype(np.float32))
    mat_crust = kit.material(name + '-crust', base=crust_img, roughness=0.55,
                             emissive=glow_img, emission=2.2)
    profile = [  # (outward offset, height, v)
        (0.55 * min(1.0, radius / 2.0), 0.0, 0.0),
        (0.32, 0.12, 0.3),
        (0.14, 0.24, 0.6),
        (0.0, 0.2, 0.8),
        (-0.12, level + 0.02, 1.0),
    ]
    bm = bmesh.new()
    uv = bm.loops.layers.uv.verify()
    rows = []
    n = len(pts)
    for i, (x, y) in enumerate(pts):
        d = mathutils.Vector((x, y)).normalized()
        jitter = rng.uniform(0.85, 1.25)
        row = []
        for off, h, vv in profile:
            p = mathutils.Vector((x, y)) + d * off * (jitter if off > 0 else 1.0)
            row.append((bm.verts.new((p.x, p.y, h * (jitter if 0 < h < 0.3 else 1.0))), (i / n * radius * 2, vv)))
        rows.append(row)
    for i in range(n):
        a, b = rows[i], rows[(i + 1) % n]
        for k in range(len(profile) - 1):
            quad = [a[k], b[k], b[k + 1], a[k + 1]]
            f = bm.faces.new([q[0] for q in quad])
            for loop, (_, t) in zip(f.loops, quad):
                loop[uv].uv = t
    me = bpy.data.meshes.new(name + '-crust')
    bm.to_mesh(me)
    bm.free()
    crust = kit.link(bpy.data.objects.new(name + '-crust', me))
    kit.finish(crust, mat_crust)
    # --- Crystals: clusters of hexagonal shards on the rim, tilted outwards.
    shard_rgb = np.ones((8, 8, 3), np.float32) * np.array((0.8, 0.74, 0.95), np.float32)
    mat_shard = kit.material(name + '-shard', base=kit._array_image(name + '-shard', shard_rgb),
                             roughness=0.2, emissive=kit._array_image(name + '-shard-glow', np.ones((8, 8, 3), np.float32) * VIOLET),
                             emission=1.4)
    perimeter = sum((mathutils.Vector(pts[i]) - mathutils.Vector(pts[i - 1])).length for i in range(n))
    shards = []
    s = 0
    for k in range(int(perimeter / 0.3)):
        i = rng.randrange(n)
        x, y = pts[i]
        out = mathutils.Vector((x, y, 0)).normalized()
        base = mathutils.Vector((x, y, 0.14)) + out * rng.uniform(-0.05, 0.25)
        big = rng.random() < 0.12
        for _ in range(rng.randint(1, 3) + (2 if big else 0)):
            h = rng.uniform(0.5, 1.1) if big else rng.uniform(0.12, 0.4)
            h *= min(1.3, 0.55 + radius / 5.0)
            r = h * rng.uniform(0.17, 0.26)
            ob = kit.cylinder(f'{name}-shard-{s}', r, h, verts=6, r_top=r * 0.12)
            s += 1
            lean = rng.uniform(0.2, 0.9)
            tilt = mathutils.Vector((out.x * lean + rng.uniform(-0.3, 0.3), out.y * lean + rng.uniform(-0.3, 0.3), 1.0)).normalized()
            ob.rotation_mode = 'QUATERNION'
            ob.rotation_quaternion = mathutils.Vector((0, 0, 1)).rotation_difference(tilt) @ mathutils.Quaternion((0, 0, 1), rng.uniform(0, 6.3))
            ob.location = base + mathutils.Vector((rng.uniform(-0.18, 0.18), rng.uniform(-0.18, 0.18), -0.06))
            kit.finish(ob, mat_shard, smooth=False, uv=0.5)
            shards.append(ob)
    # One object per material: join the shards.
    bpy.ops.object.select_all(action='DESELECT')
    for ob in shards:
        ob.select_set(True)
    bpy.context.view_layer.objects.active = shards[0]
    bpy.ops.object.join()
    bpy.context.view_layer.objects.active.name = name + '-shards'
    for ob in bpy.data.objects:
        ob.select_set(False)
    tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
    asset = {
        'id': 'asset.models.environment.' + name,
        'name': title,
        'kind': 'prop',
        'tags': ['underleaf', 'hollow', 'substance', 'procedural'],
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'vegetationClearance': 0, 'groundContact': {'mode': 'terrain'}},
        'license': 'Original procedural model for Under the Canopy (tools/props/substance_pool.py).',
    }
    kit.export(os.path.join(OUT, name), name, asset)
    print(name, tris, 'triangles')


for pool in POOLS:
    build(*pool)
