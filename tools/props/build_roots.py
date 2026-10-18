"""Builds procedural surface roots of giant trees in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/build_roots.py -- <spec.json>

spec.json: {"out": "<base/assets/environment>", "bark": "<seamless bark albedo>",
"items": [{"name", "length", "radius", "seed", "fork": bool}]}.

A root is a tube that leaves the trunk at the origin heading +Y (glTF -Z after export) and
snakes away along the ground: it tapers from `radius` to a tenth of it, rises out of the soil
and dives back in a few times, and its cross-section is gnarled by low-frequency noise.
Half of the tube sits below the ground, so it reads as a root, not a log. The bark texture
tiles around and along it; a normal map is derived from the bark's luminance."""
import bpy, bmesh, json, math, os, random, sys
import mathutils
import numpy as np

argv = sys.argv[sys.argv.index('--') + 1:]
spec = json.load(open(argv[0]))
OUT = spec['out']
SIDES, SEGMENTS = 10, 56


def path(length, rng):
    """Centre line: points, heading wanders smoothly."""
    points, heading, at = [], 0.0, mathutils.Vector((0.0, 0.0, 0.0))
    phases = [rng.uniform(0, 6.28) for _ in range(3)]
    step = length / SEGMENTS
    for i in range(SEGMENTS + 1):
        t = i / SEGMENTS
        points.append((at.copy(), t))
        heading += 0.07 * math.sin(t * 9.0 + phases[0]) + 0.04 * math.sin(t * 23.0 + phases[1])
        at += mathutils.Vector((math.sin(heading), math.cos(heading), 0.0)) * step
    return points


def tube(bm, uv_layer, points, radius, rng, start_v=0.0):
    phases = [rng.uniform(0, 6.28) for _ in range(4)]
    rings = []
    for i, (c, t) in enumerate(points):
        nxt = points[min(i + 1, len(points) - 1)][0]
        prv = points[max(i - 1, 0)][0]
        tangent = (nxt - prv).normalized()
        side = tangent.cross(mathutils.Vector((0, 0, 1))).normalized()
        up = side.cross(tangent).normalized()
        r = radius * (1.0 - 0.9 * t ** 0.8)
        # Rises out of the soil and dives back in: the centre's height above the ground.
        hump = 0.4 + 0.4 * max(0.0, math.sin(t * 7.0 + phases[0])) - 0.55 * t
        centre = c + mathutils.Vector((0, 0, r * hump))
        ring = []
        for k in range(SIDES + 1):
            a = 2 * math.pi * k / SIDES
            gnarl = 1.0 + 0.16 * math.sin(a * 3 + t * 13 + phases[1]) + 0.1 * math.sin(a * 5 - t * 29 + phases[2])
            p = centre + (side * math.cos(a) + up * math.sin(a)) * r * gnarl
            # Flatten the underside a little where it would show above the ground.
            p.z = max(p.z, c.z - r * 0.6)
            v = bm.verts.new(p)
            u_coord = k / SIDES * 2.0
            v_coord = start_v + t * (points[-1][0] - points[0][0]).length / (2 * math.pi * radius) * 2.0
            ring.append((v, (u_coord, v_coord)))
        rings.append(ring)
    for i in range(len(rings) - 1):
        for k in range(SIDES):
            quad = [rings[i][k], rings[i][k + 1], rings[i + 1][k + 1], rings[i + 1][k]]
            f = bm.faces.new([q[0] for q in quad])
            for loop, (_, uv) in zip(f.loops, quad):
                loop[uv_layer].uv = uv


def bark_images(path_):
    albedo = bpy.data.images.load(path_)
    w, h = albedo.size
    px = np.array(albedo.pixels[:], dtype=np.float32).reshape(h, w, 4)
    lum = px[:, :, :3] @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    dx = (np.roll(lum, -1, axis=1) - np.roll(lum, 1, axis=1)) * 4.0
    dy = (np.roll(lum, -1, axis=0) - np.roll(lum, 1, axis=0)) * 4.0
    n = np.stack([-dx, -dy, np.ones_like(lum)], axis=-1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    normal = bpy.data.images.new('bark_normal', w, h, alpha=False)
    normal.colorspace_settings.name = 'Non-Color'
    normal.pixels[:] = np.concatenate([n * 0.5 + 0.5, np.ones((h, w, 1), np.float32)], axis=2).ravel()
    normal.pack()
    albedo.pack()
    return albedo, normal


def material(albedo, normal):
    mat = bpy.data.materials.new('bark')
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes['Principled BSDF']
    a = nt.nodes.new('ShaderNodeTexImage'); a.image = albedo
    nt.links.new(a.outputs['Color'], bsdf.inputs['Base Color'])
    nmap = nt.nodes.new('ShaderNodeNormalMap')
    b = nt.nodes.new('ShaderNodeTexImage'); b.image = normal
    nt.links.new(b.outputs['Color'], nmap.inputs['Color'])
    nt.links.new(nmap.outputs['Normal'], bsdf.inputs['Normal'])
    bsdf.inputs['Roughness'].default_value = 0.85
    return mat


def build(item):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    albedo, normal = bark_images(spec['bark'])
    mat = material(albedo, normal)
    rng = random.Random(item['seed'])
    mesh = bpy.data.meshes.new(item['name'])
    obj = bpy.data.objects.new(item['name'], mesh)
    bpy.context.scene.collection.objects.link(obj)
    bm = bmesh.new()
    uv_layer = bm.loops.layers.uv.verify()
    points = path(item['length'], rng)
    tube(bm, uv_layer, points, item['radius'], rng)
    if item.get('fork'):
        # A smaller root branching off a third of the way along.
        base, _ = points[SEGMENTS // 3]
        heading = rng.choice([-1, 1]) * rng.uniform(0.5, 0.9)
        sub = path(item['length'] * 0.5, rng)
        rot = mathutils.Matrix.Rotation(heading, 3, 'Z')
        sub = [(base + rot @ p, t) for p, t in sub]
        tube(bm, uv_layer, sub, item['radius'] * 0.45, rng)
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mat)
    for p in mesh.polygons:
        p.use_smooth = True
    folder = os.path.join(OUT, item['name'])
    os.makedirs(folder, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True, export_image_format='JPEG',
                              export_jpeg_quality=90)
    tris = sum(len(p.vertices) - 2 for p in mesh.polygons)
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump({
            'id': 'asset.models.environment.' + item['name'], 'name': item['name'].replace('-', ' '),
            'kind': 'prop', 'tags': ['lookdev', 'autumn', 'procedural'],
            'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                          {'role': 'source', 'index': 1, 'format': 'blend'}],
            'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
            'capabilities': {'groundContact': {'mode': 'pivot'}},
            'license': 'Procedural Blender geometry (tools/props/build_roots.py) with an ImageGen (gpt-image-2) bark texture.',
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', item['name'], tris, 'triangles')


for item in spec['items']:
    build(item)
