"""The Hollow Brood's old spider webs, in Blender, headless (Tripo cannot make thin, see-through
silk; these are cards like the litter):

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/build_webs.py

The silk is one gpt-image-2.5 painting with real transparency (`art/styles/hollow-web.png`: a
torn orb web, thick radial threads, sagging spirals and matted patches), laid on subdivided
cards. The material is alpha-blended and a little self-lit, so the webs read silver and
see-through even in the Hollow's blue hour. Every piece is anchored: guy threads run from its
high points down to the ground, so nothing hangs from the air.
- `hollow-web-sheet`: a torn sheet web, about 10 m across, held up 5 m high at its back by
  two guy threads and drawn down to the ground at its front; to stand against stems and
  ferns, its high side towards them.
- `hollow-web-drape`: draped strands, a torn curtain of web 12 m wide slung from a sagging
  silk rope between two high points and drawn down to the ground in front, a looser strand
  and tatters on it, each end tied down by two guy threads; to hang between two stems.
- `hollow-web-tangle`: a low tangle of web over the ground, about 9 m across and 1.8 m high,
  like silk blown over the litter.

Pivot at the centre of the footprint on the ground; seeded, so a rebuild gives the same
pieces. Writes base/assets/environment/underleaf/<name>/ (geometry.glb, albedo.png,
source.blend, asset.json)."""
import json, math, os, random

import bpy, bmesh
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'assets', 'environment', 'underleaf')
SOURCE = os.path.join(ROOT, 'art', 'styles', 'hollow-web.png')
SILK = np.array((0.84, 0.86, 0.9), np.float32)  # cool silver
OPACITY = 0.85
GLOW = 0.3
LICENSE = ('Original procedural model for Under the Canopy (tools/props/build_webs.py), '
           'silk painted with gpt-image-2.5.')

# Texture regions (u0, v0, u1, v1) of the painting.
WHOLE = (0.0, 0.0, 1.0, 1.0)
STRAND = (0.0, 0.47, 1.0, 0.53)  # the thick thread across the middle
BAND = (0.0, 0.3, 1.0, 0.45)  # a band of spiral threads
TATTER = (0.75, 0.55, 0.95, 0.95)  # a matted corner with hanging threads


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)


def silk_image():
    img = bpy.data.images.load(SOURCE)
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    lum = px[:, :, :3].mean(axis=2, keepdims=True)
    alpha = px[:, :, 3:4]
    # The silk's own shading, tinted silver; transparent texels carry the silk colour too, so
    # mips and blending keep no dark fringe.
    shade = np.where(alpha > 0.05, np.clip(lum * 1.1, 0.55, 1.0), 0.85)
    out = bpy.data.images.new('hollow-web', w, h, alpha=True)
    out.pixels[:] = np.concatenate([shade * SILK, alpha * OPACITY], axis=2).ravel()
    out.scale(512, 512)
    return out


def material(image):
    mat = bpy.data.materials.new('silk')
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes['Principled BSDF']
    tex = nt.nodes.new('ShaderNodeTexImage')
    tex.image = image
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
    nt.links.new(tex.outputs['Alpha'], bsdf.inputs['Alpha'])
    nt.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
    bsdf.inputs['Emission Strength'].default_value = GLOW
    bsdf.inputs['Roughness'].default_value = 0.55
    mat.use_backface_culling = False
    mat.surface_render_method = 'BLENDED'
    return mat


class Mesh:
    def __init__(self, seed):
        self.bm = bmesh.new()
        self.uv = self.bm.loops.layers.uv.new('UVMap')
        self.rng = random.Random(seed)

    def face(self, verts, uvs):
        f = self.bm.faces.new(verts)
        for loop, uv in zip(f.loops, uvs):
            loop[self.uv].uv = uv

    def grid(self, at, nu, nv, rect):
        """A card of nu × nv quads; `at(u, v)` gives each point, `rect` its texture region."""
        u0, v0, u1, v1 = rect
        verts = [[self.bm.verts.new(at(i / nu, j / nv)) for j in range(nv + 1)] for i in range(nu + 1)]
        for i in range(nu):
            for j in range(nv):
                quad = [verts[i][j], verts[i + 1][j], verts[i + 1][j + 1], verts[i][j + 1]]
                uvs = [(u0 + (u1 - u0) * (i + a) / nu, v0 + (v1 - v0) * (j + b) / nv)
                       for a, b in ((0, 0), (1, 0), (1, 1), (0, 1))]
                self.face(quad, uvs)

    def sheet(self, p00, p10, p01, p11, sag, rect=WHOLE, n=(10, 8), ragged=0.15):
        """A slack sheet between four corners (u along p00→p10), sagging in the middle."""
        p00, p10, p01, p11 = (np.array(p, float) for p in (p00, p10, p01, p11))
        jitter = {}

        def at(u, v):
            p = (1 - u) * (1 - v) * p00 + u * (1 - v) * p10 + (1 - u) * v * p01 + u * v * p11
            p[2] -= sag * math.sin(math.pi * u) * math.sin(math.pi * v)
            key = (round(u, 4), round(v, 4))
            if key not in jitter:
                edge = min(u, 1 - u, v, 1 - v)
                jitter[key] = np.array([self.rng.uniform(-1, 1) for _ in range(3)]) * ragged * (edge > 0)
            return tuple(p + jitter[key])

        self.grid(at, *n, rect)

    def rope(self, a, b, sag, width, rect=STRAND, n=10, up=(0.0, 0.0, 1.0)):
        """A sagging silk rope (a ribbon) from a to b, its flat side turned to `up` and across."""
        a, b = np.array(a, float), np.array(b, float)
        along = b - a
        side = np.cross(along, np.array(up))
        if np.linalg.norm(side) < 1e-6:
            side = np.array([1.0, 0.0, 0.0])
        side = side / np.linalg.norm(side) * width / 2

        def at(u, v):
            p = a + along * u
            p[2] -= sag * 4 * u * (1 - u)
            return tuple(p + side * (2 * v - 1))

        self.grid(at, n, 1, rect)

    def dome(self, centre, radius, height, rect=WHOLE, rings=6, sectors=16, lumps=0.25):
        """A low, lumpy dome of silk over the ground, its rim on the ground."""
        cx, cy = centre
        reach = [radius * (1 + self.rng.uniform(-0.2, 0.15)) for _ in range(sectors)]
        lift = [[self.rng.uniform(-lumps, lumps) for _ in range(sectors)] for _ in range(rings + 1)]
        u0, v0, u1, v1 = rect

        def point(r, s):
            t = r / rings
            a = s * math.tau / sectors
            rr = reach[s % sectors] * t
            z = height * (1 - t * t) ** 0.8 * (1 + lift[r][s % sectors]) if r < rings else 0.0
            return (cx + rr * math.cos(a), cy + rr * math.sin(a), max(0.0, z))

        def uv(p):
            return (u0 + (u1 - u0) * (0.5 + (p[0] - cx) / (2.4 * radius)),
                    v0 + (v1 - v0) * (0.5 + (p[1] - cy) / (2.4 * radius)))

        top = self.bm.verts.new(point(0, 0))
        ring = [[self.bm.verts.new(point(r, s)) for s in range(sectors)] for r in range(1, rings + 1)]
        for s in range(sectors):
            q = [top, ring[0][s], ring[0][(s + 1) % sectors]]
            self.face(q, [uv(v.co) for v in q])
        for r in range(rings - 1):
            for s in range(sectors):
                q = [ring[r][s], ring[r + 1][s], ring[r + 1][(s + 1) % sectors], ring[r][(s + 1) % sectors]]
                self.face(q, [uv(v.co) for v in q])

    def finish(self, name, image):
        mesh = bpy.data.meshes.new(name)
        self.bm.to_mesh(mesh)
        self.bm.free()
        obj = bpy.data.objects.new(name, mesh)
        bpy.context.scene.collection.objects.link(obj)
        mesh.materials.append(material(image))
        # Pivot: the footprint's centre on the ground (the lowest point).
        co = np.array([v.co[:] for v in mesh.vertices])
        lo, hi = co.min(axis=0), co.max(axis=0)
        for v in mesh.vertices:
            v.co.x -= (lo[0] + hi[0]) / 2
            v.co.y -= (lo[1] + hi[1]) / 2
            v.co.z -= lo[2]
        return obj, hi - lo


def save(name, title, image, size):
    folder = os.path.join(OUT, name)
    os.makedirs(folder, exist_ok=True)
    image.filepath_raw = os.path.join(folder, 'albedo.png')
    image.file_format = 'PNG'
    image.save()
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    blend1 = os.path.join(folder, 'source.blend1')
    if os.path.exists(blend1):
        os.remove(blend1)
    bpy.ops.export_scene.gltf(filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB',
                              export_yup=True, export_apply=True, export_image_format='AUTO')
    asset = {
        'id': 'asset.models.environment.' + name, 'name': title, 'kind': 'prop',
        'tags': ['underleaf', 'hollow', 'webs', 'procedural'],
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'albedo', 'index': 1, 'format': 'png'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'groundContact': {'mode': 'pivot'}},
        'license': LICENSE,
    }
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump(asset, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', name, 'size', [round(float(s), 2) for s in size])


def web_sheet():
    reset()
    image = silk_image()
    m = Mesh(11)
    # The sheet: high at the back (+Y), drawn down to the ground at the front.
    back_l, back_r = (-5.0, 2.0, 5.2), (4.8, 2.6, 4.4)
    m.sheet((-4.2, -3.2, 0.1), (4.4, -3.6, 0.05), back_l, back_r, sag=1.1, n=(12, 10))
    # A second, smaller torn panel under it, the old web it was spun over.
    m.sheet((-2.5, -1.0, 0.0), (2.8, -1.4, 0.0), (-2.0, 1.4, 2.6), (2.6, 1.6, 2.2), sag=0.5,
            rect=(0.1, 0.55, 0.6, 1.0), n=(6, 5))
    # Guy threads from the high corners back down to the ground, a third to the middle.
    m.rope(back_l, (-6.4, 5.2, 0.0), 0.25, 0.5)
    m.rope(back_r, (6.2, 5.6, 0.0), 0.25, 0.5)
    m.rope((0.0, 2.4, 4.0), (0.6, 5.0, 0.0), 0.2, 0.4)
    obj, size = m.finish('hollow-web-sheet', image)
    save('hollow-web-sheet', 'Torn sheet web', image, size)


def web_drape():
    reset()
    image = silk_image()
    m = Mesh(23)
    left, right = (-6.0, 0.0, 5.0), (6.0, 0.4, 4.4)
    # A torn curtain of web hanging between two high points, its top a sagging rope.
    m.sheet((-5.2, -2.6, 1.0), (5.0, -2.2, 0.7), left, right, sag=0.8, n=(14, 6), ragged=0.12)
    m.rope(left, right, 0.9, 1.0, rect=BAND, n=14, up=(0.0, 1.0, 0.0))
    # A second, looser strand below it, and tatters hanging from the curtain's foot.
    m.rope((-5.6, -1.2, 3.0), (5.4, -0.8, 2.6), 1.6, 0.8, n=14, up=(0.0, 1.0, 0.0))
    for x, length in ((-3.4, 0.7), (-1.2, 0.9), (1.0, 0.6), (3.0, 0.8)):
        u = (x + 5.2) / 10.2
        z = 1.0 + (0.7 - 1.0) * u + 0.2
        y = -2.6 + 0.4 * u
        m.rope((x, y, z), (x + 0.1, y - 0.2, max(0.12, z - length)), 0.0, 0.8, rect=TATTER, n=3,
               up=(0.0, 1.0, 0.0))
    # Each end tied down by two long guy threads running out to the ground.
    for end, dx in ((left, -3.2), (right, 3.2)):
        for dy in (-1.0, 2.6):
            m.rope(end, (end[0] + dx, end[1] + dy, 0.0), 0.3, 0.8, rect=BAND)
    obj, size = m.finish('hollow-web-drape', image)
    save('hollow-web-drape', 'Draped web strands', image, size)


def web_tangle():
    reset()
    image = silk_image()
    m = Mesh(37)
    m.dome((0.0, 0.0), 4.2, 1.8)
    m.dome((1.8, 1.2), 2.4, 2.4, rect=(0.2, 0.2, 0.8, 0.8))
    m.dome((-2.0, -1.4), 2.0, 1.4, rect=(0.5, 0.0, 1.0, 0.5))
    obj, size = m.finish('hollow-web-tangle', image)
    save('hollow-web-tangle', 'Web tangle', image, size)


web_sheet()
web_drape()
web_tangle()
