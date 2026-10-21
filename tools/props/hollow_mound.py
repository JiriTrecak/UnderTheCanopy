"""The collapsed ant mound of the Hollow Brood (Underleaf biome, docs/art-direction.md),
procedural in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/hollow_mound.py

A neighbouring colony's nest after the substance took it: a broad mound of pale sand-clay
(the ants' own building material, D2 sand-clay spires), its spire snapped and slumped, a
second spire stump leaning, tunnel mouths gaping dark in its flanks, clay rubble spilled
round its foot, and thin cracks glowing faintly blue-violet where substance seeped up
through it. Big readable forms, no noise: the lumps are a few octaves of smooth noise.

About 22 m across and 14 m tall: a landmark the Hollow is built round, which armies walk
round (an elliptical blocker). Pivot at the centre on the ground.
Writes base/assets/environment/underleaf/hollow-collapsed-mound/."""
import math, os, random, sys

import bpy, bmesh
import mathutils
from mathutils import noise
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
NAME = 'hollow-collapsed-mound'
OUT = os.path.join(ROOT, 'base', 'assets', 'environment', 'underleaf', NAME)
VIOLET = np.array((0.55, 0.32, 1.0), np.float32)


def clay_textures(size=512, seed=7):
    """Sand-clay albedo with soft darker strata, and a glow map of a few thin cracks."""
    rng = np.random.default_rng(seed)
    u = np.linspace(0, 1, size, endpoint=False)
    x, y = np.meshgrid(u, u)
    f = np.zeros((size, size), np.float32)
    for k in (2, 3, 5, 8):
        p = rng.uniform(0, 2 * np.pi, 3)
        f += np.sin(2 * np.pi * (x * k + 0.3 * np.sin(2 * np.pi * y * k + p[0])) + p[1]) / k
        f += np.sin(2 * np.pi * (y * k * 1.7 + 0.2 * np.sin(2 * np.pi * x * k + p[2]))) / (k * 1.3)
    f = (f - f.min()) / (f.max() - f.min())
    strata = 0.5 + 0.5 * np.sin(2 * np.pi * y * 3 + 2.5 * f)
    base = np.array((0.64, 0.54, 0.4), np.float32)
    dark = np.array((0.44, 0.36, 0.27), np.float32)
    t = np.clip(0.35 + 0.45 * f + 0.1 * strata, 0, 1)[..., None]
    albedo = dark * (1 - t) + base * t
    cracks = np.exp(-((f - 0.52) / 0.008) ** 2) * (strata > 0.92) * (x > 0.6) * (y < 0.4)
    glow = (cracks[..., None] * VIOLET).astype(np.float32)
    return albedo.astype(np.float32), glow


def blob(name, radius, height, seed, segments=56, rings=22, squash=1.0, crown=None):
    """A lumpy mound: a squashed dome displaced by smooth noise, its top cut jagged."""
    bm = bmesh.new()
    bmesh.ops.create_uvsphere(bm, u_segments=segments, v_segments=rings * 2, radius=1.0, calc_uvs=True)
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if v.co.z < -0.05], context='VERTS')
    off = mathutils.Vector((seed * 3.1, seed * 1.7, seed * 0.9))
    for v in bm.verts:
        d = v.co.copy()
        z = max(0.0, d.z)
        # A mound: wide foot, steepening flanks.
        r = radius * (1.0 - 0.15 * z) * (1 + 0.18 * noise.noise(d * 1.6 + off) + 0.07 * noise.noise(d * 4.0 + off))
        h = height * (z ** 0.8) * (1 + 0.12 * noise.noise(d * 2.2 + off))
        v.co = mathutils.Vector((d.x * r, d.y * r * squash, h))
        if crown is not None and v.co.z > crown[0]:
            # The snapped top: flattened and torn.
            v.co.z = crown[0] + (v.co.z - crown[0]) * 0.25 + crown[1] * noise.noise(d * 6 + off)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    o = bpy.data.objects.new(name, me)
    return kit.link(o)


def spire(name, base_r, height, lean, seed, snapped=0.0):
    bm = bmesh.new()
    rng = random.Random(seed)
    rings, seg = 10, 14
    verts = []
    for j in range(rings + 1):
        t = j / rings
        if snapped and t > 1 - snapped:
            break
        r = base_r * (1 - t) ** 1.25 + 0.15
        row = []
        for i in range(seg):
            a = 2 * math.pi * i / seg
            w = 1 + 0.15 * noise.noise(mathutils.Vector((math.cos(a) * 2, math.sin(a) * 2, t * 5 + seed)))
            row.append(bm.verts.new((math.cos(a) * r * w + lean[0] * t * t * height,
                                     math.sin(a) * r * w + lean[1] * t * t * height, t * height)))
        verts.append(row)
    for j in range(len(verts) - 1):
        for i in range(seg):
            bm.faces.new([verts[j][i], verts[j][(i + 1) % seg], verts[j + 1][(i + 1) % seg], verts[j + 1][i]])
    top = verts[-1]
    if snapped:
        # A torn, jagged break.
        c = bm.verts.new(sum((v.co for v in top), mathutils.Vector()) / len(top) + mathutils.Vector((0, 0, -0.4)))
        for i in range(seg):
            top[i].co.z += rng.uniform(-0.6, 0.9)
            bm.faces.new([top[i], top[(i + 1) % seg], c])
    else:
        c = bm.verts.new(sum((v.co for v in top), mathutils.Vector()) / len(top) + mathutils.Vector((0, 0, 0.6)))
        for i in range(seg):
            bm.faces.new([top[i], top[(i + 1) % seg], c])
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    return kit.link(bpy.data.objects.new(name, me))


def build():
    kit.reset()
    albedo, glow = clay_textures()
    mat = kit.material(NAME, base=kit._array_image(NAME + '-clay', albedo), roughness=0.9, normal=2.5,
                       emissive=kit._array_image(NAME + '-seep', glow), emission=1.6)
    tunnel = kit.material(NAME + '-tunnel', color=(0.035, 0.03, 0.03), roughness=1.0)
    parts = []
    mound = blob('mound', 10.5, 5.0, 1, squash=0.9, crown=(4.2, 0.6))
    parts.append(mound)
    shoulder = blob('shoulder', 5.0, 4.0, 2)
    shoulder.location = (-6.5, 3.0, 0)
    parts.append(shoulder)
    s1 = spire('spire-snapped', 3.6, 17.0, (0.1, -0.04), 3, snapped=0.3)
    s1.location = (0.5, 0.5, 2.5)
    s2 = spire('spire-leaning', 2.0, 11.0, (-0.4, 0.22), 4, snapped=0.2)
    s2.location = (-5.5, 3.2, 2.8)
    s3 = spire('spire-small', 1.0, 5.5, (0.3, 0.3), 5)
    s3.location = (5.5, 4.0, 2.0)
    parts += [s1, s2, s3]
    rng = random.Random(9)
    # The fallen top of the spire, lying broken at the foot.
    fallen = spire('spire-fallen', 1.9, 8.0, (0.0, 0.0), 6, snapped=0.3)
    fallen.rotation_euler = (math.radians(84), 0, math.radians(35))
    fallen.location = (8.0, -6.5, 1.3)
    parts.append(fallen)
    # Clay rubble round the foot.
    for k in range(26):
        a = rng.uniform(0, 2 * math.pi)
        r = rng.uniform(9.0, 13.0)
        bm = bmesh.new()
        bmesh.ops.create_icosphere(bm, subdivisions=2, radius=rng.uniform(0.35, 1.1))
        for v in bm.verts:
            v.co *= 1 + 0.25 * noise.noise(v.co * 2 + mathutils.Vector((k, 0, 0)))
            v.co.z *= 0.6
        me = bpy.data.meshes.new(f'rubble-{k}')
        bm.to_mesh(me)
        bm.free()
        o = kit.link(bpy.data.objects.new(f'rubble-{k}', me))
        o.location = (math.cos(a) * r, math.sin(a) * r * 0.9, 0.1)
        o.rotation_euler = (0, 0, rng.uniform(0, 6.3))
        parts.append(o)
    for o in parts:
        kit.finish(o, mat, uv=5.0)
    # Tunnel mouths: dark ovals pressed into the flanks, facing out.
    holes = []
    for k, (a, h, s) in enumerate([(-1.9, 1.2, 1.5), (-0.9, 2.0, 1.1), (0.4, 0.9, 1.7), (2.2, 1.6, 1.2), (2.9, 2.6, 0.9),
                                   (-2.6, 2.4, 0.7), (1.3, 2.8, 0.8), (-0.2, 3.2, 0.6)]):
        out = mathutils.Vector((math.cos(a), math.sin(a), 0))
        r = 10.5 * (1 - 0.15 * (h / 5.0)) * (1 - (h / 5.0) ** 1.25 * 0.55) * 0.93 - 0.2
        o = kit.cylinder(f'hole-{k}', s, 0.6, verts=16)
        o.rotation_mode = 'QUATERNION'
        o.rotation_quaternion = mathutils.Vector((0, 0, 1)).rotation_difference(out + mathutils.Vector((0, 0, 0.35)))
        o.location = out * r + mathutils.Vector((0, 0, h))
        o.scale = (1.0, 0.75, 1.0)
        kit.finish(o, tunnel, uv=1.0)
        holes.append(o)
    bpy.ops.object.select_all(action='DESELECT')
    for o in parts:
        o.select_set(True)
    bpy.context.view_layer.objects.active = parts[0]
    bpy.ops.object.join()
    bpy.context.view_layer.objects.active.name = NAME
    bpy.ops.object.select_all(action='DESELECT')
    for o in holes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = holes[0]
    bpy.ops.object.join()
    bpy.context.view_layer.objects.active.name = NAME + '-tunnels'
    asset = {
        'id': 'asset.models.environment.' + NAME,
        'name': 'Collapsed ant mound',
        'kind': 'prop',
        'tags': ['underleaf', 'hollow', 'ants', 'landmark', 'procedural'],
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'vegetationClearance': 3, 'groundContact': {'mode': 'pivot'}},
        'license': 'Original procedural model for Under the Canopy (tools/props/hollow_mound.py).',
    }
    kit.export(OUT, NAME, asset)


build()
