"""Makes a Tripo GLB game-ready in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/import_tripo.py -- <spec.json>

spec.json: {"out": "<base/assets/environment>", "items": [{"name", "glb", "height" | "width"
(metres after scaling), "triangles" (budget), "texture" (pixels, square), "tone", "saturation",
"kind": "prop"|"foliage"|"tree", "tags": [...]}]}.

For each item: joins the meshes, puts the pivot at the centre of the base (lowest point at 0),
scales to the requested size, decimates to the triangle budget, shrinks every texture,
tones the base colour (darker / less saturated, as the art direction's frame asks), and writes
`<out>/<name>/` with geometry.glb (textures embedded), source.blend and asset.json."""
import bpy, json, os, sys
import numpy as np

argv = sys.argv[sys.argv.index('--') + 1:]
spec = json.load(open(argv[0]))
OUT = spec['out']


def tone_image(img, tone, saturation):
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    rgb = px[:, :, :3]
    grey = rgb.mean(axis=2, keepdims=True)
    px[:, :, :3] = np.clip((grey + (rgb - grey) * saturation) * tone, 0.0, 1.0)
    img.pixels[:] = px.ravel()


def process(item):
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.ops.import_scene.gltf(filepath=item['glb'])
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
                    tone_image(img, item.get('tone', 1.0), item.get('saturation', 1.0))
                    if item.get('emissive'):
                        # Self-lit material (amber): the base colour also drives emission.
                        bsdf = [l.to_node for l in node.outputs['Color'].links if l.to_socket.name == 'Base Color'][0]
                        mat.node_tree.links.new(node.outputs['Color'], bsdf.inputs['Emission Color'])
                        bsdf.inputs['Emission Strength'].default_value = item['emissive']
                img.pack()

    folder = os.path.join(OUT, item['name'])
    os.makedirs(folder, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True, export_image_format='JPEG',
                              export_jpeg_quality=90)
    final = sum(len(p.vertices) - 2 for p in obj.data.polygons)
    asset = {
        'id': 'asset.models.environment.' + item['name'], 'name': item['name'].replace('-', ' '),
        'kind': item.get('kind', 'prop'), 'tags': ['lookdev', 'tripo'] + item.get('tags', []),
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'groundContact': {'mode': 'pivot'}},
        'license': 'Generated with Tripo (text to 3D) for Under the Canopy; decimated and toned by tools/props/import_tripo.py.',
    }
    asset['capabilities'].update(item.get('capabilities', {}))
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump(asset, f, indent=2, ensure_ascii=False)
        f.write('\n')
    dims = (size * scale).round(2).tolist()
    print('built', item['name'], tris, '->', final, 'triangles, size', dims)


for item in spec['items']:
    process(item)
