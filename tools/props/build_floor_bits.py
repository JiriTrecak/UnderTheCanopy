"""Builds the forest floor's simple pieces that Tripo can't make well, in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/build_floor_bits.py -- <out>

`<out>` is the environment folder (base/assets/environment). It writes:
- `giants-pine-needles-a`, `-b`: fallen pine needles at insect scale, beams 12–18 m long
  (a pair joined at their sheath, and a loose crossing of three), dry rust-brown, untextured;
- `giants-dew-drop`, `giants-dew-drops`: dew drops as glassy, slightly flattened orbs,
  1–2.4 m across, alpha-blended so the ground shows through.

Each asset folder gets geometry.glb, source.blend and asset.json. Shapes are seeded, so a
rebuild gives the same pieces."""
import bpy, bmesh, json, math, os, random, sys
import mathutils

OUT = sys.argv[sys.argv.index('--') + 1:][0]


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def material(name, color, roughness, alpha=1.0, specular=0.5):
    m = bpy.data.materials.new(name)
    m.use_nodes = True
    b = m.node_tree.nodes['Principled BSDF']
    b.inputs['Base Color'].default_value = (*color, 1.0)
    b.inputs['Roughness'].default_value = roughness
    b.inputs['Specular IOR Level'].default_value = specular
    if alpha < 1.0:
        b.inputs['Alpha'].default_value = alpha
        m.surface_render_method = 'BLENDED'
    return m


def needle(bm, start, direction, length, radius, bend, rng):
    """A tapered, flattened, gently bent tube from `start` along `direction` (on the ground)."""
    sides, segs = 6, 14
    side = mathutils.Vector((-direction.y, direction.x, 0.0))
    rings = []
    for i in range(segs + 1):
        t = i / segs
        c = start + direction * (t * length) + side * (bend * math.sin(t * math.pi) * length)
        c.z = radius * 0.8 + 0.25 * math.sin(t * math.pi) * radius * 3
        r = radius * (1.0 - 0.85 * t ** 3)
        ring = []
        for s in range(sides):
            a = s / sides * math.tau
            # A needle is half-round in section: flat on one side.
            off = side * (math.cos(a) * r) + mathutils.Vector((0, 0, max(math.sin(a), -0.35) * r * 0.75))
            ring.append(bm.verts.new(c + off))
        rings.append(ring)
    for i in range(segs):
        for s in range(sides):
            a, b = rings[i][s], rings[i][(s + 1) % sides]
            c, d = rings[i + 1][(s + 1) % sides], rings[i + 1][s]
            bm.faces.new((a, b, c, d))
    bm.faces.new(list(reversed(rings[0])))
    tip = bm.verts.new(start + direction * (length * 1.03) + mathutils.Vector((0, 0, radius * 0.6)))
    for s in range(sides):
        bm.faces.new((rings[-1][s], rings[-1][(s + 1) % sides], tip))


def save(name, kind, tags, note):
    folder = os.path.join(OUT, name)
    os.makedirs(folder, exist_ok=True)
    for o in bpy.context.scene.objects:
        o.select_set(True)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True)
    blend1 = os.path.join(folder, 'source.blend1')
    if os.path.exists(blend1):
        os.remove(blend1)
    asset = {
        'id': 'asset.models.environment.' + name, 'name': name.replace('-', ' '), 'kind': kind,
        'tags': ['forest-floor', 'giants'] + tags,
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'groundContact': {'mode': 'pivot'}},
        'license': note,
    }
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump(asset, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', name)


def needles(name, count, seed):
    reset()
    rng = random.Random(seed)
    bm = bmesh.new()
    if count == 2:
        # A pine's pair: two needles from one sheath, opening in a narrow V.
        base = mathutils.Vector((0, 0, 0))
        for k in (-1, 1):
            a = k * rng.uniform(0.12, 0.2)
            needle(bm, base, mathutils.Vector((math.cos(a), math.sin(a), 0)), rng.uniform(15, 18), 0.32, k * 0.03, rng)
        sheath = bmesh.ops.create_cone(bm, cap_ends=True, segments=8, radius1=0.5, radius2=0.42, depth=1.4)
        bmesh.ops.rotate(bm, verts=sheath['verts'], cent=(0, 0, 0), matrix=mathutils.Matrix.Rotation(math.pi / 2, 3, 'Y'))
        bmesh.ops.translate(bm, verts=sheath['verts'], vec=(-0.5, 0, 0.45))
    else:
        for k in range(count):
            a = rng.uniform(0, math.pi)
            d = mathutils.Vector((math.cos(a), math.sin(a), 0))
            start = mathutils.Vector((rng.uniform(-3, 3), rng.uniform(-3, 3), 0)) - d * 7
            needle(bm, start, d, rng.uniform(12, 17), rng.uniform(0.26, 0.34), rng.uniform(-0.05, 0.05), rng)
    mesh = bpy.data.meshes.new(name)
    bm.to_mesh(mesh)
    bm.free()
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    for p in mesh.polygons:
        p.use_smooth = True
    # Stack the crossing needles: lift each later one onto the one under it a little.
    mesh.materials.append(material('needle', (0.30, 0.15, 0.075), 0.55))
    lo = min(v.co.z for v in mesh.vertices)
    for v in mesh.vertices:
        v.co.z -= lo
    save(name, 'prop', ['pine'], 'Original artwork for Under the Canopy, built by tools/props/build_floor_bits.py.')


def drops(name, sizes, seed):
    reset()
    rng = random.Random(seed)
    glass = material('dew', (0.78, 0.88, 0.95), 0.03, alpha=0.32, specular=1.0)
    for k, r in enumerate(sizes):
        bpy.ops.mesh.primitive_uv_sphere_add(segments=24, ring_count=14, radius=r)
        o = bpy.context.active_object
        o.scale = (1.0, 1.0, 0.78)
        if k:
            a = rng.uniform(0, math.tau)
            d = sizes[0] + r + rng.uniform(0.2, 1.2)
            o.location = (math.cos(a) * d, math.sin(a) * d, 0)
        o.location.z = r * 0.78 - r * 0.08
        bpy.ops.object.transform_apply(location=True, rotation=False, scale=True)
        bpy.ops.object.shade_smooth()
        o.data.materials.append(glass)
    save(name, 'prop', ['dew', 'water'], 'Original artwork for Under the Canopy, built by tools/props/build_floor_bits.py.')


needles('giants-pine-needles-a', 2, 3)
needles('giants-pine-needles-b', 3, 7)
drops('giants-dew-drop', [1.2], 1)
drops('giants-dew-drops', [1.1, 0.6, 0.45, 0.3], 5)
