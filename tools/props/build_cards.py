"""Builds leaf-litter and fern props from generated cut-out images (on a white background) in
Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/build_cards.py -- <spec.json>

spec.json: {"out": "<base/assets/environment>", "items": [{"name", "image", "kind": "leaves"|"fern",
"size": metres, "count": leaves per cluster, "spread": metres, "seed"}]}.

Each item becomes an asset folder `<out>/<name>/` with `geometry.glb` (texture embedded),
`albedo.png`, `source.blend` and `asset.json`:
- the white background is keyed to alpha and the colour bled into the transparent texels, so
  mip maps keep no white fringe;
- every leaf is a gently curled, subdivided card lying on the ground (real geometry, fine for
  the third-person camera); a prop is a cluster of `count` overlapping leaves, so one instance
  covers more ground;
- ferns are fronds that arch off the ground."""
import bpy, bmesh, json, math, os, random, sys
import numpy as np

argv = sys.argv[sys.argv.index('--') + 1:]
spec = json.load(open(argv[0]))
OUT = spec['out']


TONE, SATURATION = 1.0, 1.0


def key_image(path, name, size=512):
    img = bpy.data.images.load(path)
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    rgb = px[:, :, :3]
    # Distance from white decides alpha; a soft ramp keeps edges clean.
    d = (1.0 - rgb).max(axis=2)
    alpha = np.clip((d - 0.05) / 0.12, 0.0, 1.0)
    ys, xs = np.nonzero(alpha > 0.5)
    y0, y1, x0, x1 = ys.min(), ys.max(), xs.min(), xs.max()
    pad = 4
    y0, x0 = max(0, y0 - pad), max(0, x0 - pad)
    y1, x1 = min(h - 1, y1 + pad), min(w - 1, x1 + pad)
    rgb, alpha = rgb[y0:y1 + 1, x0:x1 + 1], alpha[y0:y1 + 1, x0:x1 + 1]
    # Bleed colour outwards so transparent texels carry the edge colour (no white halos).
    solid = alpha > 0.5
    fill = np.where(solid[..., None], rgb, 0.0)
    weight = solid.astype(np.float32)
    for _ in range(24):
        acc = np.zeros_like(fill)
        wacc = np.zeros_like(weight)
        for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)):
            acc += np.roll(np.roll(fill, dy, 0), dx, 1)
            wacc += np.roll(np.roll(weight, dy, 0), dx, 1)
        grow = (weight == 0) & (wacc > 0)
        fill[grow] = acc[grow] / wacc[grow][:, None]
        weight[grow] = 1.0
    rgb = np.where(solid[..., None], rgb, fill)
    # Tone towards the target's value range: a little darker and less saturated.
    grey = rgb.mean(axis=2, keepdims=True)
    rgb = (grey + (rgb - grey) * SATURATION) * TONE
    hh, ww = rgb.shape[:2]
    out = bpy.data.images.new(name, ww, hh, alpha=True)
    out.pixels[:] = np.concatenate([rgb, alpha[..., None]], axis=2).ravel()
    out.scale(size, max(16, int(size * hh / ww)) if ww >= hh else size)
    if ww < hh:
        out.scale(max(16, int(size * ww / hh)), size)
    return out, ww / hh


def material(name, image):
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes['Principled BSDF']
    tex = nt.nodes.new('ShaderNodeTexImage')
    tex.image = image
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    # A "greater than" on alpha is how the glTF exporter writes alphaMode MASK with a cutoff.
    cut = nt.nodes.new('ShaderNodeMath')
    cut.operation = 'GREATER_THAN'
    cut.inputs[1].default_value = 0.5
    nt.links.new(tex.outputs['Alpha'], cut.inputs[0])
    nt.links.new(cut.outputs['Value'], bsdf.inputs['Alpha'])
    bsdf.inputs['Roughness'].default_value = 0.85
    mat.use_backface_culling = False
    mat.surface_render_method = 'DITHERED'
    return mat


def card(bm, aspect, length, curl, lift, rng, transform):
    """One subdivided card: length along +Z, width by aspect; curled across, lifted at the tip."""
    nx, nz = 3, 4
    width = length * aspect
    verts = []
    uv_layer = bm.loops.layers.uv.verify()
    for j in range(nz + 1):
        row = []
        for i in range(nx + 1):
            u, v = i / nx, j / nz
            x = (u - 0.5) * width
            z = (v - 0.5) * length
            y = curl * (2 * (u - 0.5)) ** 2 * width + lift * v * v * length + 0.01
            # Blender is Z-up: the card lies in X/Y, its height above the ground is Z.
            p = transform @ __import__('mathutils').Vector((x, z, y))
            row.append((bm.verts.new(p), (u, v)))
        verts.append(row)
    for j in range(nz):
        for i in range(nx):
            quad = [verts[j][i], verts[j][i + 1], verts[j + 1][i + 1], verts[j + 1][i]]
            f = bm.faces.new([q[0] for q in quad])
            for loop, (_, uv) in zip(f.loops, quad):
                loop[uv_layer].uv = uv


def build(item):
    import mathutils
    global TONE, SATURATION
    name = item['name']
    TONE, SATURATION = item.get('tone', 1.0), item.get('saturation', 1.0)
    bpy.ops.wm.read_factory_settings(use_empty=True)
    image, aspect = key_image(item['image'], name + '_albedo')
    mat = material(name, image)
    mesh = bpy.data.meshes.new(name)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    bm = bmesh.new()
    rng = random.Random(item.get('seed', 1))
    count = item.get('count', 1)
    size = item['size']
    spread = item.get('spread', 0.0)
    for k in range(count):
        length = size * rng.uniform(0.75, 1.15)
        if item['kind'] == 'fern':
            curl, lift = rng.uniform(0.03, 0.07), rng.uniform(0.2, 0.32)
            tilt = 0.0
        else:
            curl, lift = rng.uniform(0.02, 0.07), rng.uniform(0.0, 0.03)
            tilt = rng.uniform(-0.05, 0.05)
        ang = rng.uniform(0, 2 * math.pi)
        r = spread * math.sqrt(rng.random()) if k else 0.0
        offset = mathutils.Vector((math.cos(ang) * r, math.sin(ang) * r, k * 0.012))
        rot = mathutils.Matrix.Rotation(rng.uniform(0, 2 * math.pi), 4, 'Z') @ mathutils.Matrix.Rotation(tilt, 4, 'X')
        card(bm, aspect, length, curl, lift, rng, mathutils.Matrix.Translation(offset) @ rot)
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mat)
    for p in mesh.polygons:
        p.use_smooth = True
    folder = os.path.join(OUT, name)
    os.makedirs(folder, exist_ok=True)
    image.filepath_raw = os.path.join(folder, 'albedo.png')
    image.file_format = 'PNG'
    image.save()
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True, export_image_format='AUTO')
    tris = sum(len(p.vertices) - 2 for p in mesh.polygons)
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump({
            'id': 'asset.models.environment.' + name, 'name': name.replace('-', ' '),
            'kind': 'foliage', 'tags': ['lookdev', 'autumn', 'imagegen'],
            'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                          {'role': 'albedo', 'index': 1, 'format': 'png'},
                          {'role': 'source', 'index': 1, 'format': 'blend'}],
            'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
            'capabilities': {'vegetationClearance': 0, 'groundContact': {'mode': 'terrain'}},
            'license': 'Original ImageGen (gpt-image-2) albedo on procedural Blender geometry (tools/props/build_cards.py).',
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', name, tris, 'triangles')


for item in spec['items']:
    build(item)
