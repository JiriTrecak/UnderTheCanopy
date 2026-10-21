"""Builds ground patches: one cut-out image (a transparent gpt-image sheet of a single patch of
moss, leaves, flowers or withered ground) laid flat on the ground as a soft-edged card, in
Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/build_patches.py -- <spec.json>

spec.json: {"items": [{"name", "image", "size": metres across its widest side, "title",
"tags": [...], "tone": 1, "saturation": 1, "folder": "underleaf"}]}.

Each item becomes `base/assets/environment/<folder>/<name>/` (geometry.glb with the texture
embedded, albedo.png, source.blend, asset.json):
- the card is the image's own shape trimmed to its opaque part, subdivided and lifted a few
  centimetres in a soft dome with a gentle wobble, so it lies on the ground without being
  perfectly flat and never sinks into it;
- alpha is blended (soft edges, no shimmering cut-out edge in motion), the colour bled into
  the transparent texels so mip maps keep no dark or white fringe;
- placed by maps as scenery (`category: terrain`, the editor's Ground group): the map
  author paints a withered stain, a moss carpet or a drift of leaves where the story needs
  it, since the biome generates the rest of the ground from the blocks."""
import bpy, bmesh, json, math, os, random, sys
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
argv = sys.argv[sys.argv.index('--') + 1:]
spec = json.load(open(argv[0]))
TEXTURE = 1024


def load(path, tone, saturation):
    img = bpy.data.images.load(path)
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    rgb, alpha = px[:, :, :3], px[:, :, 3]
    ys, xs = np.nonzero(alpha > 0.04)
    pad = 6
    y0, y1 = max(0, ys.min() - pad), min(h, ys.max() + pad)
    x0, x1 = max(0, xs.min() - pad), min(w, xs.max() + pad)
    rgb, alpha = rgb[y0:y1, x0:x1].copy(), alpha[y0:y1, x0:x1].copy()
    grey = rgb.mean(axis=2, keepdims=True)
    rgb = np.clip((grey + (rgb - grey) * saturation) * tone, 0, 1)
    # Bleed colour outwards so mips of the transparent texels carry the edge colour.
    solid = alpha > 0.5
    fill = np.where(solid[..., None], rgb, 0.0)
    weight = solid.astype(np.float32)
    for _ in range(32):
        acc = np.zeros_like(fill)
        wacc = np.zeros_like(weight)
        for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)):
            acc += np.roll(np.roll(fill, dy, 0), dx, 1)
            wacc += np.roll(np.roll(weight, dy, 0), dx, 1)
        grow = (weight == 0) & (wacc > 0)
        fill[grow] = acc[grow] / wacc[grow][:, None]
        weight[grow] = 1.0
    rgb = np.where(solid[..., None], rgb, fill)
    hh, ww = rgb.shape[:2]
    out = bpy.data.images.new('albedo', ww, hh, alpha=True)
    out.pixels[:] = np.concatenate([rgb, alpha[..., None]], axis=2).ravel()
    s = TEXTURE / max(ww, hh)
    out.scale(max(16, int(ww * s)), max(16, int(hh * s)))
    return out, ww / hh, alpha


def build(item):
    import mathutils
    name = item['name']
    bpy.ops.wm.read_factory_settings(use_empty=True)
    image, aspect, alpha = load(item['image'], item.get('tone', 1.0), item.get('saturation', 1.0))
    size = item['size']
    width, depth = (size, size / aspect) if aspect >= 1 else (size * aspect, size)
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes['Principled BSDF']
    tex = nt.nodes.new('ShaderNodeTexImage')
    tex.image = image
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    nt.links.new(tex.outputs['Alpha'], bsdf.inputs['Alpha'])
    bsdf.inputs['Roughness'].default_value = item.get('roughness', 0.9)
    mat.surface_render_method = 'BLENDED'
    mat.use_backface_culling = True
    mesh = bpy.data.meshes.new(name)
    obj = bpy.data.objects.new(name, mesh)
    bpy.context.scene.collection.objects.link(obj)
    bm = bmesh.new()
    uv = bm.loops.layers.uv.verify()
    n = 14
    rng = random.Random(item.get('seed', 7))
    phase = [rng.uniform(0, 6.28) for _ in range(4)]
    grid = []
    for j in range(n + 1):
        row = []
        for i in range(n + 1):
            u, v = i / n, j / n
            x, y = (u - 0.5) * width, (v - 0.5) * depth
            r = min(1.0, math.hypot(2 * u - 1, 2 * v - 1))
            dome = item.get('dome', 0.05) * (1 - r * r)
            wobble = 0.012 * (math.sin(x * 1.3 + phase[0]) + math.sin(y * 1.7 + phase[1]))
            row.append((bm.verts.new((x, y, 0.03 + max(0.0, dome + wobble))), (u, 1 - v)))
        grid.append(row)
    for j in range(n):
        for i in range(n):
            quad = [grid[j][i], grid[j][i + 1], grid[j + 1][i + 1], grid[j + 1][i]]
            # Skip quads the image leaves fully transparent.
            ys = [int(q[1][1] * (alpha.shape[0] - 1)) for q in quad]
            xs = [int(q[1][0] * (alpha.shape[1] - 1)) for q in quad]
            if alpha[min(ys):max(ys) + 1, min(xs):max(xs) + 1].max() < 0.02:
                continue
            f = bm.faces.new([q[0] for q in quad])
            for loop, (_, t) in zip(f.loops, quad):
                loop[uv].uv = t
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)
    loose = [v for v in bm.verts if not v.link_faces]
    bmesh.ops.delete(bm, geom=loose, context='VERTS')
    bm.to_mesh(mesh)
    bm.free()
    mesh.materials.append(mat)
    for p in mesh.polygons:
        p.use_smooth = True
    folder = os.path.join(ROOT, 'base', 'assets', 'environment', item.get('folder', 'underleaf'), name)
    os.makedirs(folder, exist_ok=True)
    image.filepath_raw = os.path.join(folder, 'albedo.png')
    image.file_format = 'PNG'
    image.save()
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True, export_image_format='AUTO')
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump({
            'id': 'asset.models.environment.' + name, 'name': item.get('title', name.replace('-', ' ')),
            'kind': 'foliage', 'tags': item.get('tags', ['underleaf', 'imagegen']),
            'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                          {'role': 'albedo', 'index': 1, 'format': 'png'},
                          {'role': 'source', 'index': 1, 'format': 'blend'}],
            'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
            'capabilities': {'vegetationClearance': 0, 'groundContact': {'mode': 'terrain'}},
            'license': 'Original ImageGen (gpt-image-2.5) cut-out on procedural Blender geometry (tools/props/build_patches.py).',
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    bpy.data.batch_remove([o for o in bpy.data.objects])
    print('built', name, len(mesh.polygons), 'quads')


for item in spec['items']:
    build(item)
