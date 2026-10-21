"""The Hollow's sick plants and the Glade's mushroom ring for the Underleaf biome (docs/
art-direction.md), made in Blender from the base game's own plant models, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/hollow_props.py

The Hollow Brood is a colony sickened by raw substance: its plants are drained, not rotten.
- **Withered ferns**: the bushy and sword ferns, their fronds dried to ash-grey and pale
  ochre, slumped lower and wider (scaled down in height), a faint violet bruise at the
  heart where the substance rose through them.
- **Bleached mushrooms**: mushroom clusters drained to bone and ash, one set standing and
  one toppled on its side.
- **Mushroom ring** (the Glade): a fairy ring of small cream mushrooms, about 11 m across,
  which armies walk through.

Each model keeps its source's geometry and UVs; only its textures are regraded (in linear
light) and, for the slumped and toppled ones, its transform. Writes
base/assets/environment/underleaf/<name>/ (geometry.glb, source.blend, asset.json)."""
import math, os, random, sys

import bpy
import mathutils
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ENV = os.path.join(ROOT, 'base', 'assets', 'environment')
OUT = os.path.join(ENV, 'underleaf')
LICENSE = ('Original artwork for Under the Canopy: regraded and re-posed from the base game\'s '
           'own plant models by tools/props/hollow_props.py.')


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def load(name):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=os.path.join(ENV, name, 'geometry.glb'))
    return [o for o in bpy.data.objects if o not in before]


def regrade(images, grade):
    """`grade(rgb, a) -> rgb` over every image (straight colour, 0–1)."""
    done = set()
    for img in images:
        if img is None or img.name in done or img.size[0] == 0:
            continue
        done.add(img.name)
        w, h = img.size
        px = np.array(img.pixels[:], np.float32).reshape(h, w, 4)
        px[:, :, :3] = np.clip(grade(px[:, :, :3], px[:, :, 3:4]), 0, 1)
        img.pixels[:] = px.ravel()
        img.pack()


def regrade_factors(objs, grade):
    """The same grade on untextured materials' base colour factors (linear in Blender)."""
    for m in materials(objs):
        bsdf = next((n for n in m.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
        if bsdf is None or bsdf.inputs['Base Color'].is_linked:
            continue
        c = np.array(bsdf.inputs['Base Color'].default_value[:3], np.float32)
        srgb = np.where(c <= 0.0031308, c * 12.92, 1.055 * np.power(c, 1 / 2.4) - 0.055)
        g = np.clip(grade(srgb[None, None, :], None)[0, 0], 0, 1)
        lin = np.where(g <= 0.04045, g / 12.92, ((g + 0.055) / 1.055) ** 2.4)
        bsdf.inputs['Base Color'].default_value = (*lin.tolist(), 1.0)


def material_images(objs):
    out = []
    for o in objs:
        if o.type != 'MESH':
            continue
        for slot in o.material_slots:
            m = slot.material
            if m and m.use_nodes:
                for n in m.node_tree.nodes:
                    if n.type == 'TEX_IMAGE' and n.image is not None:
                        # Only colour textures: normal maps are non-colour data.
                        if n.image.colorspace_settings.name == 'sRGB':
                            out.append(n.image)
    return out


def materials(objs):
    return {s.material for o in objs if o.type == 'MESH' for s in o.material_slots if s.material}


def withered(rgb, a):
    grey = rgb @ np.array([0.3, 0.55, 0.15], np.float32)
    g = grey[..., None]
    ash = np.array([0.62, 0.58, 0.52], np.float32)
    ochre = np.array([0.72, 0.6, 0.4], np.float32)
    # Darker parts to ash, lighter to dry ochre; a little of the old hue left.
    tone = np.clip(g * 1.25, 0, 1)
    out = (ash * (1 - tone) + ochre * tone) * (0.45 + 0.75 * g) * 1.05
    return out * 0.92 + rgb * 0.08


def bleached(rgb, a):
    grey = rgb @ np.array([0.3, 0.55, 0.15], np.float32)
    g = grey[..., None]
    bone = np.array([0.86, 0.83, 0.76], np.float32)
    ash = np.array([0.55, 0.53, 0.52], np.float32)
    t = np.clip(g * 1.6, 0, 1)
    return (ash * (1 - t) + bone * t) * (0.6 + 0.5 * g)


def cream(rgb, a):
    grey = rgb @ np.array([0.3, 0.55, 0.15], np.float32)
    g = grey[..., None]
    cap = np.array([0.82, 0.72, 0.55], np.float32)
    return cap * (0.45 + 0.7 * g) * 0.95 + rgb * 0.05


def bruise(mats, strength=0.6):
    """A faint violet glow at the plant's heart: its material's emission, low."""
    for m in mats:
        bsdf = next((n for n in m.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
        if bsdf is not None:
            bsdf.inputs['Emission Color'].default_value = (0.55, 0.32, 1.0, 1.0)
            bsdf.inputs['Emission Strength'].default_value = strength * 0.08


def ground(objs):
    """Lowest point on the ground, footprint centred."""
    bpy.context.view_layer.update()
    lo = mathutils.Vector((1e9,) * 3)
    hi = -lo
    for o in objs:
        if o.type == 'MESH':
            for v in o.data.vertices:
                w = o.matrix_world @ v.co
                lo = mathutils.Vector(map(min, lo, w))
                hi = mathutils.Vector(map(max, hi, w))
    for o in objs:
        if o.parent is None:
            o.location -= mathutils.Vector(((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, lo.z))
    return hi - lo


def export(name, title, kind='foliage', tags=(), wind=False):
    folder = os.path.join(OUT, name)
    os.makedirs(folder, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True, export_image_format='AUTO',
                              export_extras=True)
    caps = {'vegetationClearance': 0, 'groundContact': {'mode': 'pivot'}}
    import json
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump({
            'id': 'asset.models.environment.' + name, 'name': title, 'kind': kind,
            'tags': ['underleaf', *tags],
            'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                          {'role': 'source', 'index': 1, 'format': 'blend'}],
            'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
            'capabilities': caps, 'license': LICENSE,
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', name)


def withered_fern(name, source, title):
    reset()
    objs = load(source)
    regrade(material_images(objs), withered)
    regrade_factors(objs, withered)
    bruise(materials(objs), 0.3)
    for o in objs:
        if o.parent is None:
            o.scale = (o.scale.x * 1.15, o.scale.y * 1.15, o.scale.z * 0.62)
    ground(objs)
    export(name, title, tags=('hollow', 'withered'))


def bleached_mushrooms(name, source, title, toppled=False):
    reset()
    objs = load(source)
    regrade(material_images(objs), bleached)
    regrade_factors(objs, bleached)
    if toppled:
        root = bpy.data.objects.new('toppled', None)
        bpy.context.scene.collection.objects.link(root)
        for o in objs:
            if o.parent is None:
                o.parent = root
        root.rotation_euler = (math.radians(78), 0, math.radians(20))
        objs.append(root)
    ground(objs)
    export(name, title, kind='foliage', tags=('hollow', 'withered'))


def mushroom_ring(name, title, radius=5.5, count=17, seed=4):
    reset()
    rng = random.Random(seed)
    parts = []
    for k in range(count):
        objs = load('canopy-mushrooms-ochre')
        root = bpy.data.objects.new(f'm{k}', None)
        bpy.context.scene.collection.objects.link(root)
        for o in objs:
            if o.parent is None:
                o.parent = root
        a = 2 * math.pi * k / count + rng.uniform(-0.12, 0.12)
        r = radius * rng.uniform(0.92, 1.08)
        root.location = (r * math.cos(a), r * math.sin(a), 0)
        root.rotation_euler = (0, 0, rng.uniform(0, 6.28))
        s = rng.uniform(0.7, 1.25)
        root.scale = (s, s, s)
        parts += objs + [root]
    regrade(material_images(parts), cream)
    regrade_factors(parts, cream)
    export(name, title, kind='foliage', tags=('glade',))


withered_fern('hollow-withered-fern', 'pl-fern-bushy-b', 'Withered fern')
withered_fern('hollow-withered-fern-tall', 'pl-fern-sword-b', 'Withered fern, tall')
bleached_mushrooms('hollow-bleached-mushrooms', 'woodland-mushroom-cluster', 'Bleached mushrooms')
bleached_mushrooms('hollow-toppled-mushrooms', 'canopy-mushrooms-ochre', 'Bleached mushrooms, toppled', toppled=True)
mushroom_ring('glade-mushroom-ring', 'Mushroom ring')
