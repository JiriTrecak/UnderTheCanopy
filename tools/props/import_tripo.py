"""Makes a Tripo GLB game-ready in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/import_tripo.py -- <spec.json> [<glb folder> [<name>...]]

spec.json: {"out": "<base/assets/environment>", "tags": [...] (optional, replaces the default
["lookdev", "tripo"]), "license": "..." (optional), "items": [{"name", "glb", "height" | "width"
(metres after scaling), "title" (optional display name), "triangles" (budget), "texture" (pixels, square), "tone", "saturation", "tint" ([r, g, b] multiplier),
"kind": "prop"|"foliage"|"tree", "tags": [...]}]}.

Optional per item, applied in this order before sizing:
- "flat": true turns a flat piece (leaf, feather) that Tripo stood up at the concept's camera
  angle so it lies on the ground, its shown side up;
- "upright": true turns a tall piece Tripo leaned over so its longest axis stands vertical;
- "lay": [x, y, z] degrees, turns a standing piece over; "squash": scales its height;
- "axes": [x, y, z] scales each axis (Blender's, Z up): lengthens a branch or widens a cap;
- "stretch": {"above": share of the height, "by": factor}: lengthens only what lies above that
  height, so a trunk grows taller while its buttress roots keep their shape;
- "sink": share of the height pushed below the ground;
- "sway": metres the top of a piece with `capabilities.wind` sways (default 0.5);
- "glow": {"color": [r, g, b], "strength", "hue": "violet"}: makes the texels of that hue
  self-lit (bioluminescent gills), the rest of the material stays unlit.

For each item: joins the meshes, puts the pivot at the centre of the base (lowest point at 0),
scales to the requested size, decimates to the triangle budget, shrinks every texture,
tones the base colour (darker / less saturated, as the art direction's frame asks), and writes
`<out>/<name>/` with geometry.glb (textures embedded), source.blend and asset.json."""
import bpy, json, math, os, sys
import numpy as np

argv = sys.argv[sys.argv.index('--') + 1:]
spec = json.load(open(argv[0]))
OUT = spec['out']
# Optional: the folder relative `glb` paths are in (Tripo exports stay out of the repository),
# and the names of the items to build (default: all).
SOURCE = argv[1] if len(argv) > 1 else '.'
ONLY = set(argv[2:])


def tone_image(img, tone, saturation, tint=(1.0, 1.0, 1.0)):
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    rgb = px[:, :, :3]
    grey = rgb.mean(axis=2, keepdims=True)
    px[:, :, :3] = np.clip((grey + (rgb - grey) * saturation) * tone * np.array(tint, np.float32), 0.0, 1.0)
    img.pixels[:] = px.ravel()


def glow_image(mat, node, img, glow):
    """An emission texture from the base colour's texels of the glow's hue (before toning)."""
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    r, g, b = px[:, :, 0], px[:, :, 1], px[:, :, 2]
    # Violet to blue: blue leads, red at least near green; weighted by brightness.
    # Only strongly coloured texels glow (gills, rims), not pale lilac caps.
    hue = np.clip((b - np.maximum(r, g) - glow.get('threshold', 0.16)) / 0.18, 0.0, 1.0) * np.clip((b - 0.3) / 0.3, 0.0, 1.0)
    out = np.zeros_like(px)
    out[:, :, :3] = hue[:, :, None] * np.array(glow['color'], np.float32)
    out[:, :, 3] = 1.0
    em = bpy.data.images.new(img.name + '-glow', w, h)
    em.pixels[:] = out.ravel()
    em.pack()
    tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
    tex.image = em
    tex.interpolation = node.interpolation
    bsdf = [l.to_node for l in node.outputs['Color'].links if l.to_socket.name == 'Base Color'][0]
    uv = node.inputs['Vector'].links[0].from_socket if node.inputs['Vector'].links else None
    if uv:
        mat.node_tree.links.new(uv, tex.inputs['Vector'])
    mat.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
    bsdf.inputs['Emission Strength'].default_value = glow.get('strength', 2.0)


def process(item):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=os.path.join(SOURCE, item['glb']))
    meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH']
    bpy.ops.object.select_all(action='DESELECT')
    for o in meshes:
        o.select_set(True)
    bpy.context.view_layer.objects.active = meshes[0]
    if len(meshes) > 1:
        bpy.ops.object.join()
    obj = bpy.context.view_layer.objects.active
    # Clear parents (glTF import adds empties) keeping the world transform, then apply it.
    bpy.ops.object.parent_clear(type='CLEAR_KEEP_TRANSFORM')
    for o in list(bpy.context.scene.objects):
        if o != obj:
            bpy.data.objects.remove(o, do_unlink=True)
    bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
    import mathutils
    if 'lay' in item:
        # Lay a standing piece down (degrees about X, Y, Z), then squash its height.
        rx, ry, rz = (math.radians(a) for a in item['lay'])
        obj.data.transform(mathutils.Euler((rx, ry, rz)).to_matrix().to_4x4())
    if item.get('flat'):
        # A flat piece (leaf, feather, seed wing) that Tripo stood up at the concept's camera
        # angle: turn its thinnest principal axis to vertical, keeping the side the concept
        # showed (facing the camera, front-left and above) on top.
        co = np.array([v.co[:] for v in obj.data.vertices])
        co = co - co.mean(axis=0)
        _, vecs = np.linalg.eigh(co.T @ co)
        normal = vecs[:, 0]
        if normal @ np.array([0.0, -0.57, 0.82]) < 0:
            normal = -normal
        rot = mathutils.Vector(normal.tolist()).rotation_difference(mathutils.Vector((0, 0, 1)))
        obj.data.transform(rot.to_matrix().to_4x4())
    if item.get('upright'):
        # A tall piece Tripo leaned over: turn its longest principal axis to vertical.
        co = np.array([v.co[:] for v in obj.data.vertices])
        co = co - co.mean(axis=0)
        _, vecs = np.linalg.eigh(co.T @ co)
        axis = vecs[:, 2] if vecs[2, 2] >= 0 else -vecs[:, 2]
        rot = mathutils.Vector(axis.tolist()).rotation_difference(mathutils.Vector((0, 0, 1)))
        obj.data.transform(rot.to_matrix().to_4x4())
    if 'squash' in item:
        obj.data.transform(mathutils.Matrix.Diagonal((1.0, 1.0, item['squash'], 1.0)))
    if 'axes' in item:
        obj.data.transform(mathutils.Matrix.Diagonal((*item['axes'], 1.0)))
    if 'stretch' in item:
        zs = [v.co.z for v in obj.data.vertices]
        z0, z1 = min(zs), max(zs)
        cut = z0 + (z1 - z0) * item['stretch']['above']
        by = item['stretch']['by']
        for v in obj.data.vertices:
            if v.co.z > cut:
                v.co.z = cut + (v.co.z - cut) * by

    # Pivot: centre of the footprint, lowest point on the ground (Blender is Z-up).
    co = np.array([v.co[:] for v in obj.data.vertices])
    lo, hi = co.min(axis=0), co.max(axis=0)
    centre = np.array([(lo[0] + hi[0]) / 2, (lo[1] + hi[1]) / 2, lo[2]])
    obj.data.transform(__import__('mathutils').Matrix.Translation(-__import__('mathutils').Vector(centre)))
    size = hi - lo
    if 'height' in item:
        scale = item['height'] / size[2]
    else:
        scale = item['width'] / max(size[0], size[1])
    obj.data.transform(__import__('mathutils').Matrix.Scale(scale, 4))
    sink = item.get('sink', 0.0)
    if sink:
        obj.data.transform(__import__('mathutils').Matrix.Translation((0, 0, -sink * size[2] * scale)))

    if item.get('weld', False):
        # Tripo splits vertices along UV seams; welding them (UVs stay per corner) keeps
        # decimation from tearing the surface into shards.
        import bmesh
        bm = bmesh.new()
        bm.from_mesh(obj.data)
        bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=float(max(size) * scale) * 1e-5)
        bm.to_mesh(obj.data)
        bm.free()
    tris = sum(len(p.vertices) - 2 for p in obj.data.polygons)
    budget = item['triangles']
    if tris > budget:
        mod = obj.modifiers.new('decimate', 'DECIMATE')
        mod.ratio = budget / tris
        mod.use_collapse_triangulate = True
        bpy.ops.object.modifier_apply(modifier=mod.name)
    for p in obj.data.polygons:
        p.use_smooth = True

    px = item.get('texture', 1024)
    for mat in obj.data.materials:
        if not mat or not mat.use_nodes:
            continue
        mat.use_backface_culling = False
        for node in mat.node_tree.nodes:
            if node.type == 'TEX_IMAGE' and node.image:
                img = node.image
                if img.size[0] > px:
                    img.scale(px, px)
                linked = [l.to_socket.name for l in node.outputs['Color'].links]
                if 'Base Color' in linked:
                    if item.get('glow'):
                        glow_image(mat, node, img, item['glow'])
                    tone_image(img, item.get('tone', 1.0), item.get('saturation', 1.0), item.get('tint', (1.0, 1.0, 1.0)))
                    if item.get('emissive'):
                        # Self-lit material (amber): the base colour also drives emission.
                        bsdf = [l.to_node for l in node.outputs['Color'].links if l.to_socket.name == 'Base Color'][0]
                        mat.node_tree.links.new(node.outputs['Color'], bsdf.inputs['Emission Color'])
                        bsdf.inputs['Emission Strength'].default_value = item['emissive']
                img.pack()

    wind = item.get('capabilities', {}).get('wind')
    if wind:
        # The cooker sways a model whose node carries `foliageWind` (amplitude in metres at
        # its top, before placement scale) and `plantHeight`.
        obj['foliageWind'] = {'amplitude': item.get('sway', 0.5), 'speed': wind.get('speed', 1.0)}
        obj['plantHeight'] = max(v.co.z for v in obj.data.vertices)
    folder = os.path.join(OUT, item['name'])
    os.makedirs(folder, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    if os.path.exists(os.path.join(folder, 'source.blend1')):
        os.remove(os.path.join(folder, 'source.blend1'))
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True, export_image_format='JPEG',
                              export_jpeg_quality=90, export_extras=bool(wind))
    final = sum(len(p.vertices) - 2 for p in obj.data.polygons)
    asset = {
        'id': 'asset.models.environment.' + item['name'], 'name': item.get('title', item['name'].replace('-', ' ')),
        'kind': item.get('kind', 'prop'), 'tags': spec.get('tags', ['lookdev', 'tripo']) + item.get('tags', []),
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'groundContact': {'mode': 'pivot'}},
        'license': spec.get('license', 'Generated with Tripo (text to 3D) for Under the Canopy; decimated and toned by tools/props/import_tripo.py.'),
    }
    asset['capabilities'].update(item.get('capabilities', {}))
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump(asset, f, indent=2, ensure_ascii=False)
        f.write('\n')
    dims = (size * scale).round(2).tolist()
    print('built', item['name'], tris, '->', final, 'triangles, size', dims)


for item in spec['items']:
    if not ONLY or item['name'] in ONLY:
        process(item)
