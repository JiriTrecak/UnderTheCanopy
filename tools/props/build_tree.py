"""Dresses a bare tree (a Tripo trunk and limbs, made game-ready by import_tripo.py) with a
crown of leaf-spray cards in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/build_tree.py -- <spec.json>

spec.json: {"out": "<base/assets/environment>", "items": [{"name", "bare": "<source.blend of the
bare tree>", "sprays": ["<cut-out on white>", …], "cards": count, "size": spray metres,
"crown": [lowest, highest] share of the height where the crown grows, "seed",
"wind": {"amplitude", "speed", "strength", "stiffness"}, "tone", "saturation"}]}.

The crown follows the limbs: cards are scattered over the bark's surface in the crown band,
weighted by area and by how far the surface reaches out from the trunk (limbs and twigs, not
the bole), each pushed off the bark along its normal, facing outwards and up, gently curved.
One material per spray image, alpha-masked and marked `foliage`, so the cooker gives it leaf
flutter; every card's vertices carry `_LEAF`, their offset from the card's centre, which the
statics shader turns about in the wind (shaders/lib/wind.wgsl). The node carries
`foliageWind` and `plantHeight` for the trunk sway."""
import bpy, bmesh, json, math, os, random, sys
import mathutils
import numpy as np

argv = sys.argv[sys.argv.index('--') + 1:]
spec = json.load(open(argv[0]))
OUT = spec['out']
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))


def key_image(path, name, tone, saturation, size=512):
    """White background → alpha, edge colour bled outwards (as build_cards.py)."""
    img = bpy.data.images.load(path)
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    rgb = px[:, :, :3]
    d = (1.0 - rgb).max(axis=2)
    alpha = np.clip((d - 0.05) / 0.12, 0.0, 1.0)
    ys, xs = np.nonzero(alpha > 0.5)
    y0, y1, x0, x1 = max(0, ys.min() - 4), min(h - 1, ys.max() + 4), max(0, xs.min() - 4), min(w - 1, xs.max() + 4)
    rgb, alpha = rgb[y0:y1 + 1, x0:x1 + 1], alpha[y0:y1 + 1, x0:x1 + 1]
    solid = alpha > 0.5
    fill = np.where(solid[..., None], rgb, 0.0)
    weight = solid.astype(np.float32)
    for _ in range(24):
        acc, wacc = np.zeros_like(fill), np.zeros_like(weight)
        for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)):
            acc += np.roll(np.roll(fill, dy, 0), dx, 1)
            wacc += np.roll(np.roll(weight, dy, 0), dx, 1)
        grow = (weight == 0) & (wacc > 0)
        fill[grow] = acc[grow] / wacc[grow][:, None]
        weight[grow] = 1.0
    rgb = np.where(solid[..., None], rgb, fill)
    grey = rgb.mean(axis=2, keepdims=True)
    rgb = np.clip((grey + (rgb - grey) * saturation) * tone, 0.0, 1.0)
    hh, ww = rgb.shape[:2]
    out = bpy.data.images.new(name, ww, hh, alpha=True)
    out.pixels[:] = np.concatenate([rgb, alpha[..., None]], axis=2).ravel()
    out.scale(size, size)
    out.pack()
    return out, ww / hh


def leaf_material(name, image):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes['Principled BSDF']
    tex = nt.nodes.new('ShaderNodeTexImage')
    tex.image = image
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    cut = nt.nodes.new('ShaderNodeMath')
    cut.operation = 'GREATER_THAN'
    cut.inputs[1].default_value = 0.5
    nt.links.new(tex.outputs['Alpha'], cut.inputs[0])
    nt.links.new(cut.outputs['Value'], bsdf.inputs['Alpha'])
    bsdf.inputs['Roughness'].default_value = 0.6
    mat.use_backface_culling = False
    mat.surface_render_method = 'DITHERED'
    mat['foliage'] = True
    return mat


def surface_samples(obj, lo, hi, count, rng):
    """Area- and reach-weighted points on the bark between heights `lo` and `hi`."""
    mesh = obj.data
    mesh.calc_loop_triangles()
    tris = mesh.loop_triangles
    co = np.array([v.co[:] for v in mesh.vertices])
    idx = np.array([t.vertices[:] for t in tris])
    a, b, c = co[idx[:, 0]], co[idx[:, 1]], co[idx[:, 2]]
    centre = (a + b + c) / 3.0
    normal = np.cross(b - a, c - a)
    area = np.linalg.norm(normal, axis=1) * 0.5
    normal /= np.maximum(np.linalg.norm(normal, axis=1, keepdims=True), 1e-9)
    reach = np.linalg.norm(centre[:, :2], axis=1)
    band = (centre[:, 2] > lo) & (centre[:, 2] < hi)
    # The bole's radius in the band: surfaces near it are trunk, not limbs.
    core = np.percentile(reach[band], 30) if band.any() else 0.0
    weight = area * band * np.clip((reach - core) / max(core, 1e-3), 0.05, 4.0) ** 1.5
    weight *= 0.4 + 0.6 * np.clip((centre[:, 2] - lo) / max(hi - lo, 1e-3), 0.0, 1.0)
    weight /= weight.sum()
    picks = np.random.default_rng(rng.randrange(1 << 30)).choice(len(tris), size=count, p=weight)
    out = []
    for i in picks:
        u, v = rng.random(), rng.random()
        if u + v > 1.0:
            u, v = 1.0 - u, 1.0 - v
        p = a[i] + (b[i] - a[i]) * u + (c[i] - a[i]) * v
        out.append((mathutils.Vector(p), mathutils.Vector(normal[i]), float(reach[i])))
    return out


def card(bm, uv_layer, leaf_layer, centre, facing, length, aspect, rng):
    """A gently cupped 2×2 card around `centre`, its face towards `facing`."""
    width = length * aspect
    z = facing.normalized()
    x = z.cross(mathutils.Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), 0.3))).normalized()
    y = z.cross(x).normalized()
    roll = rng.uniform(0, 2 * math.pi)
    x, y = x * math.cos(roll) + y * math.sin(roll), y * math.cos(roll) - x * math.sin(roll)
    cup = rng.uniform(0.05, 0.16) * length
    # `_LEAF` holds offsets up to 4 m per axis; a larger card turns about a shortened arm,
    # which only softens its flutter.
    fit = min(1.0, 3.9 / max(width * 0.5 + cup, length * 0.5 + cup))
    grid = []
    for j in range(3):
        row = []
        for i in range(3):
            u, v = i / 2, j / 2
            du, dv = (u - 0.5) * 2, (v - 0.5) * 2
            p = centre + x * (du * width * 0.5) + y * (dv * length * 0.5) + z * (cup * (du * du + dv * dv) * 0.5)
            vert = bm.verts.new(p)
            offset = (p - centre) * fit / 4.0 * 0.5 + mathutils.Vector((0.5, 0.5, 0.5))
            vert[leaf_layer] = (offset.x, offset.y, offset.z, 1.0)
            row.append((vert, (u, v)))
        grid.append(row)
    for j in range(2):
        for i in range(2):
            quad = [grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]]
            f = bm.faces.new([q[0] for q in quad])
            for loop, (_, uv) in zip(f.loops, quad):
                loop[uv_layer].uv = uv
    return


def build(item):
    bpy.ops.wm.open_mainfile(filepath=item['bare'])
    bark = [o for o in bpy.context.scene.objects if o.type == 'MESH'][0]
    bark.name = item['name'] + '-bark'
    co = np.array([v.co[:] for v in bark.data.vertices])
    height = float(co[:, 2].max())
    rng = random.Random(item.get('seed', 1))
    crown = item.get('crown', [0.45, 1.0])
    samples = surface_samples(bark, crown[0] * height, crown[1] * height + 1.0, item['cards'], rng)
    images = [key_image(p, '%s-spray-%d' % (item['name'], k), item.get('tone', 1.0), item.get('saturation', 1.0))
              for k, p in enumerate(item['sprays'])]
    mats = [leaf_material('%s-leaves-%d' % (item['name'], k), img) for k, (img, _) in enumerate(images)]
    size = item['size']
    leaves = []
    for k, mat in enumerate(mats):
        mesh = bpy.data.meshes.new('%s-leaves-%d' % (item['name'], k))
        obj = bpy.data.objects.new(mesh.name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        mesh.materials.append(mat)
        leaves.append((obj, bmesh.new()))
    up = mathutils.Vector((0, 0, 1))
    for n, (p, normal, reach) in enumerate(samples):
        k = rng.randrange(len(mats))
        obj, bm = leaves[k]
        uv_layer = bm.loops.layers.uv.verify()
        leaf_layer = bm.verts.layers.float_color.get('_LEAF') or bm.verts.layers.float_color.new('_LEAF')
        length = size * rng.uniform(0.7, 1.25)
        outward = mathutils.Vector((p.x, p.y, 0.0))
        outward = outward.normalized() if outward.length > 1e-3 else normal
        centre = p + normal * rng.uniform(0.15, 0.6) * length + outward * rng.uniform(0.0, 0.5) * length \
            + mathutils.Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-0.6, 0.8))) * length * 0.35
        facing = (normal * 0.5 + outward * 0.4 + up * rng.uniform(0.3, 1.0)
                  + mathutils.Vector((rng.uniform(-1, 1), rng.uniform(-1, 1), rng.uniform(-1, 1))) * 0.45)
        card(bm, uv_layer, leaf_layer, centre, facing, length, images[k][1], rng)
    for obj, bm in leaves:
        bm.to_mesh(obj.data)
        bm.free()
        for poly in obj.data.polygons:
            poly.use_smooth = True
    wind = item.get('wind', {})
    for obj in [bark] + [o for o, _ in leaves]:
        obj['foliageWind'] = {'amplitude': wind.get('amplitude', 0.5), 'speed': wind.get('speed', 1.0)}
        obj['plantHeight'] = height
    folder = os.path.join(OUT, item['name'])
    os.makedirs(folder, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True, export_image_format='AUTO',
                              export_extras=True, export_attributes=True)
    cards = sum(len(o.data.polygons) for o, _ in leaves) * 2
    tris = sum(len(p.vertices) - 2 for p in bark.data.polygons)
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump({
            'id': 'asset.models.environment.' + item['name'], 'name': item['name'].replace('-', ' '),
            'kind': 'tree', 'tags': ['lookdev', 'autumn', 'tripo', 'imagegen'] + item.get('tags', []),
            'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                          {'role': 'source', 'index': 1, 'format': 'blend'}],
            'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
            'capabilities': {
                'wind': {'mode': 'tree', 'strength': wind.get('strength', 0.05), 'speed': wind.get('speed', 0.2),
                         'stiffness': wind.get('stiffness', 0.9)},
                'groundContact': {'mode': 'pivot'},
                'canopy': True,
            },
            'license': 'Tripo (text to 3D) trunk and limbs with ImageGen (gpt-image-2) leaf sprays, '
                       'dressed by tools/props/build_tree.py.',
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', item['name'], 'height', round(height, 1), 'bark', tris, 'leaves', cards, 'triangles')


for item in spec['items']:
    build(item)
