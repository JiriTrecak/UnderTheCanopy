"""The beetles' effect art, drawn procedurally so it stays crisp and reproducible:

    python3 tools/content/beetles/gen_art.py

Writes asset folders under base/assets/effects/beetles/ (pure Python, no dependencies):
- white sprites whose alpha is the shape, so a layer's colour tints them and its `core`
  burns their dense centre white: `spark` (a tight hot dot that streaks along its
  velocity), `glint` (a four-point star), `flash` (a ragged muzzle flash), `shard` (a
  glass sliver), `ring` (a thin shock ring with a faint fill);
- ground decals: `molten` (a cooling pool of metal: hot veins in a dark crust, its colour
  in the texture), `scorch` (a soot mark, for normal blending);
- `bolt`: a brass repeater bolt with a glowing substance head (the Bolter's projectile);
- `crucible`: the Forgemaster's falling crucible, a riveted iron pot brimming with molten
  metal (a small glTF model; its metal glows through an emissive material);
- `anvil`: the Forgemaster's falling anvil, red-hot iron with molten metal in its cracks.
"""
import json
import math
import os
import struct
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__)))))
OUT = os.path.join(ROOT, 'base', 'assets', 'effects', 'beetles')
LICENSE = 'Original procedural effect art for Under the Canopy (tools/content/beetles/gen_art.py).'


# ----- images ------------------------------------------------------------------------------

def png(path, width, height, pixel):
    """Writes an RGBA PNG; `pixel(x, y)` returns (r, g, b, a) floats in 0–1 at the
    texel centre in −1…1 coordinates (y up)."""
    rows = bytearray()
    for j in range(height):
        rows.append(0)
        for i in range(width):
            u = (i + 0.5) / width * 2 - 1
            v = 1 - (j + 0.5) / height * 2
            r, g, b, a = pixel(u, v)
            rows.extend(max(0, min(255, int(round(c * 255)))) for c in (r, g, b, a))

    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data) & 0xffffffff)

    header = struct.pack('>IIBBBBB', width, height, 8, 6, 0, 0, 0)
    with open(path, 'wb') as f:
        f.write(b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', zlib.compress(bytes(rows), 9))
                + chunk(b'IEND', b''))


def smooth(e0, e1, x):
    t = max(0.0, min(1.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


def hash2(x, y, seed):
    n = (x * 374761393 + y * 668265263 + seed * 2147483647) & 0xffffffff
    n = ((n ^ (n >> 13)) * 1274126177) & 0xffffffff
    return ((n ^ (n >> 16)) & 0xffff) / 65535.0


def value_noise(x, y, seed):
    xi, yi = math.floor(x), math.floor(y)
    fx, fy = x - xi, y - yi
    ux, uy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
    a, b = hash2(xi, yi, seed), hash2(xi + 1, yi, seed)
    c, d = hash2(xi, yi + 1, seed), hash2(xi + 1, yi + 1, seed)
    return (a + (b - a) * ux) * (1 - uy) + (c + (d - c) * ux) * uy


def fbm(x, y, seed, octaves=4):
    total, amp, norm = 0.0, 1.0, 0.0
    for o in range(octaves):
        total += value_noise(x, y, seed + o * 17) * amp
        norm += amp
        x, y, amp = x * 2.03, y * 2.03, amp * 0.5
    return total / norm


def white(a):
    return (1.0, 1.0, 1.0, a)


def spark(u, v):
    r = math.hypot(u, v)
    return white(max(0.0, 1 - r) ** 2.4 + 0.6 * max(0.0, 1 - r * 3.2) ** 2)


def glint(u, v):
    r = math.hypot(u, v)
    rays = 0.0
    for k in range(4):
        angle = k * math.pi / 4
        ca, sa = math.cos(angle), math.sin(angle)
        along, across = u * ca + v * sa, -u * sa + v * ca
        reach = 1.0 if k % 2 == 0 else 0.5
        width = 0.035 if k % 2 == 0 else 0.025
        rays = max(rays, math.exp(-(across / width) ** 2) * max(0.0, 1 - abs(along) / reach) ** 1.6)
    core = max(0.0, 1 - r / 0.32) ** 2
    return white(min(1.0, rays + core))


FLASH_SPIKES = [(0.0, 1.0), (0.9, 0.62), (1.75, 0.85), (2.6, 0.55), (3.3, 0.9), (4.15, 0.6), (5.0, 0.8), (5.7, 0.5)]


def flash(u, v):
    r = math.hypot(u, v)
    theta = math.atan2(v, u)
    spikes = 0.0
    for angle, reach in FLASH_SPIKES:
        d = math.atan2(math.sin(theta - angle), math.cos(theta - angle))
        width = 0.22 * (1 - r / reach) + 0.02
        if r < reach:
            spikes = max(spikes, math.exp(-(d * max(r, 0.08) / (width * 0.35)) ** 2) * (1 - r / reach) ** 1.2)
    core = max(0.0, 1 - r / 0.42) ** 1.6
    return white(min(1.0, spikes + core))


SHARD = [(0.02, 1.0), (0.42, 0.25), (0.18, -1.0), (-0.38, -0.1)]


def inside(poly, x, y):
    hit = False
    for (x0, y0), (x1, y1) in zip(poly, poly[1:] + poly[:1]):
        if (y0 > y) != (y1 > y) and x < (x1 - x0) * (y - y0) / (y1 - y0) + x0:
            hit = not hit
    return hit


def edge_distance(poly, x, y):
    best = 9.0
    for (x0, y0), (x1, y1) in zip(poly, poly[1:] + poly[:1]):
        dx, dy = x1 - x0, y1 - y0
        t = max(0.0, min(1.0, ((x - x0) * dx + (y - y0) * dy) / (dx * dx + dy * dy)))
        best = min(best, math.hypot(x - x0 - t * dx, y - y0 - t * dy))
    return best


def shard(u, v):
    d = edge_distance(SHARD, u, v)
    if not inside(SHARD, u, v):
        return white(max(0.0, 1 - d / 0.025) * 0.6)
    # Glass: bright at the edges and along a facet line, clearer in between.
    facet = math.exp(-((u - 0.12 * v - 0.05) / 0.05) ** 2)
    return white(min(1.0, 0.42 + 0.58 * max(0.0, 1 - d / 0.1) + 0.35 * facet))


def ring(u, v):
    r = math.hypot(u, v)
    rim = math.exp(-((r - 0.93) / 0.025) ** 2)
    fill = smooth(0.55, 0.92, r) ** 2 * (1 - smooth(0.92, 0.97, r)) * 0.12
    return white(min(1.0, rim + fill))


def molten(u, v):
    r = math.hypot(u, v)
    warp = fbm(u * 3 + 11, v * 3 + 5, 3)
    edge = 0.82 + (warp - 0.5) * 0.3
    if r > edge + 0.03:
        return (0, 0, 0, 0)
    alpha = 1 - smooth(edge - 0.06, edge + 0.03, r)
    # Veins: ridges of a noise field glow where the crust cracks; the middle stays hotter.
    n = fbm(u * 5.5 + 3, v * 5.5 + 9, 7, 5)
    vein = math.exp(-((n - 0.5) / 0.045) ** 2)
    heat = min(1.0, vein * 0.95 + (1 - r / edge) ** 2 * 0.55 + 0.05)
    crust = 0.07 + 0.05 * fbm(u * 14, v * 14, 2, 2)
    # Dark crust → deep red → orange → yellow-white.
    stops = [(0.0, (crust, crust * 0.8, crust * 0.7)), (0.35, (0.55, 0.08, 0.02)), (0.7, (1.0, 0.45, 0.08)),
             (1.0, (1.0, 0.92, 0.6))]
    for (h0, c0), (h1, c1) in zip(stops, stops[1:]):
        if heat <= h1:
            t = (heat - h0) / (h1 - h0)
            col = tuple(a + (b - a) * t for a, b in zip(c0, c1))
            break
    return (*col, alpha)


def scorch(u, v):
    r = math.hypot(u, v)
    warp = fbm(u * 2.5 + 4, v * 2.5 + 1, 9)
    edge = 0.78 + (warp - 0.5) * 0.4
    soot = (1 - smooth(edge * 0.35, edge, r)) * (0.75 + 0.25 * fbm(u * 9, v * 9, 4, 3))
    streaks = 0.5 + 0.5 * math.cos(math.atan2(v, u) * 13 + warp * 6)
    return (0.05, 0.045, 0.04, min(1.0, soot * (0.75 + 0.25 * streaks)))


TEXTURES = {
    'spark': ('Beetle spark', 64, 64, spark, 'A tight hot dot: streaks along its velocity when stretched.'),
    'glint': ('Beetle glint', 128, 128, glint, 'A four-point star glint for flashes and impacts.'),
    'flash': ('Beetle muzzle flash', 128, 128, flash, 'A ragged many-spiked muzzle flash.'),
    'shard': ('Beetle glass shard', 64, 128, shard, 'A sliver of lantern glass, bright at its edges.'),
    'ring': ('Beetle shock ring', 256, 256, ring, 'A thin shock ring with a faint fill.'),
    'molten': ('Beetle molten pool', 256, 256, molten, 'Molten metal cooling in a dark crust, hot veins glowing.'),
    'scorch': ('Beetle scorch mark', 256, 256, scorch, 'A soot scorch mark for the ground (normal blending).'),
}


def asset(folder, doc):
    os.makedirs(folder, exist_ok=True)
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        f.write(json.dumps(doc, indent=2, ensure_ascii=False) + '\n')


def textures():
    for name, (title, w, h, fn, about) in TEXTURES.items():
        folder = os.path.join(OUT, name)
        asset(folder, {'id': 'asset.effects.beetles.' + name, 'name': title, 'kind': 'texture',
                       'tags': ['effect', 'beetles', 'particle'],
                       'resources': [{'role': 'image', 'index': 1, 'format': 'png'}],
                       'license': LICENSE + ' ' + about})
        png(os.path.join(folder, 'image.png'), w, h, fn)


# ----- the crucible ------------------------------------------------------------------------

def lathe(profile, sides, y_scale=1.0):
    """Vertices and triangles of a surface of revolution through `profile` [(radius, y)]."""
    positions, normals, indices = [], [], []
    for k in range(sides + 1):
        a = k / sides * math.tau
        ca, sa = math.cos(a), math.sin(a)
        for j, (r, y) in enumerate(profile):
            # The normal from the profile's slope.
            r0, y0 = profile[max(j - 1, 0)]
            r1, y1 = profile[min(j + 1, len(profile) - 1)]
            dr, dy = r1 - r0, y1 - y0
            nr, ny = dy, -dr
            n = math.hypot(nr, ny) or 1.0
            positions.append((r * ca, y * y_scale, r * sa))
            normals.append((nr / n * ca, ny / n, nr / n * sa))
    rows = len(profile)
    for k in range(sides):
        for j in range(rows - 1):
            a, b = k * rows + j, (k + 1) * rows + j
            indices += [a, a + 1, b, b, a + 1, b + 1]
    return positions, normals, indices


def disc(radius, y, sides):
    positions = [(0.0, y, 0.0)] + [(radius * math.cos(k / sides * math.tau), y, radius * math.sin(k / sides * math.tau))
                                   for k in range(sides + 1)]
    normals = [(0.0, 1.0, 0.0)] * len(positions)
    indices = []
    for k in range(sides):
        indices += [0, k + 2, k + 1]
    return positions, normals, indices


def boxes(centres, size):
    positions, normals, indices = [], [], []
    for cx, cy, cz in centres:
        for axis in range(3):
            for sign in (-1, 1):
                n = [0.0, 0.0, 0.0]
                n[axis] = sign
                u, w = [(a) for a in range(3) if a != axis]
                base = len(positions)
                for du, dw in ((-1, -1), (1, -1), (1, 1), (-1, 1)):
                    p = [cx, cy, cz]
                    p[axis] += sign * size[axis]
                    p[u] += du * size[u]
                    p[w] += dw * size[w]
                    positions.append(tuple(p))
                    normals.append(tuple(n))
                quad = [base, base + 1, base + 2, base, base + 2, base + 3]
                if (sign > 0) != (axis == 1):
                    quad = [base, base + 2, base + 1, base, base + 3, base + 2]
                indices += quad
    return positions, normals, indices


def merge(*parts):
    positions, normals, indices = [], [], []
    for p, n, i in parts:
        base = len(positions)
        positions += p
        normals += n
        indices += [base + x for x in i]
    return positions, normals, indices


def glb(path, primitives, materials=None, name='crucible'):
    """A minimal glTF binary: one node, one mesh, a primitive per (positions, normals,
    indices, material)."""
    blob = bytearray()
    views, accessors, prims = [], [], []

    def view(data, target):
        while len(blob) % 4:
            blob.append(0)
        views.append({'buffer': 0, 'byteOffset': len(blob), 'byteLength': len(data), 'target': target})
        blob.extend(data)
        return len(views) - 1

    for (positions, normals, indices), material in primitives:
        pos = b''.join(struct.pack('<3f', *p) for p in positions)
        nor = b''.join(struct.pack('<3f', *n) for n in normals)
        idx = b''.join(struct.pack('<I', i) for i in indices)
        lo = [min(p[k] for p in positions) for k in range(3)]
        hi = [max(p[k] for p in positions) for k in range(3)]
        accessors.append({'bufferView': view(pos, 34962), 'componentType': 5126, 'count': len(positions),
                          'type': 'VEC3', 'min': lo, 'max': hi})
        accessors.append({'bufferView': view(nor, 34962), 'componentType': 5126, 'count': len(normals),
                          'type': 'VEC3'})
        accessors.append({'bufferView': view(idx, 34963), 'componentType': 5125, 'count': len(indices),
                          'type': 'SCALAR'})
        n = len(accessors)
        prims.append({'attributes': {'POSITION': n - 3, 'NORMAL': n - 2}, 'indices': n - 1, 'material': material})
    doc = {
        'asset': {'version': '2.0', 'generator': 'tools/content/beetles/gen_art.py'},
        'extensionsUsed': ['KHR_materials_emissive_strength'],
        'scene': 0, 'scenes': [{'nodes': [0]}],
        'nodes': [{'name': name, 'mesh': 0}],
        'meshes': [{'name': name, 'primitives': prims}],
        'materials': materials or [
            {'name': 'iron', 'doubleSided': True, 'pbrMetallicRoughness': {'baseColorFactor': [0.08, 0.075, 0.07, 1], 'metallicFactor': 0.7,
                                                      'roughnessFactor': 0.45}},
            {'name': 'molten', 'doubleSided': True, 'pbrMetallicRoughness': {'baseColorFactor': [1.0, 0.5, 0.12, 1], 'metallicFactor': 0.0,
                                                        'roughnessFactor': 0.6},
             'emissiveFactor': [1.0, 0.42, 0.08], 'extensions': {'KHR_materials_emissive_strength': {'emissiveStrength': 9.0}}},
            {'name': 'glow', 'doubleSided': True, 'pbrMetallicRoughness': {'baseColorFactor': [0.3, 0.1, 0.04, 1], 'metallicFactor': 0.6,
                                                      'roughnessFactor': 0.5},
             'emissiveFactor': [1.0, 0.3, 0.05], 'extensions': {'KHR_materials_emissive_strength': {'emissiveStrength': 2.5}}},
        ],
        'accessors': accessors,
        'bufferViews': views,
        'buffers': [{'byteLength': len(blob)}],
    }
    text = json.dumps(doc, separators=(',', ':')).encode()
    while len(text) % 4:
        text += b' '
    while len(blob) % 4:
        blob.append(0)
    body = (struct.pack('<II', len(text), 0x4E4F534A) + text + struct.pack('<II', len(blob), 0x004E4942) + bytes(blob))
    with open(path, 'wb') as f:
        f.write(struct.pack('<III', 0x46546C67, 2, 12 + len(body)) + body)


def crucible():
    """A squat iron crucible (1 m tall, its foot at the origin) brimming with molten metal:
    a flared pot, a thick lip, a glowing band where the metal shows through a seam, two
    trunnions and a row of rivets."""
    sides = 28
    shell = lathe([(0.0, 0.0), (0.42, 0.0), (0.5, 0.06), (0.6, 0.45), (0.7, 0.86), (0.78, 0.9), (0.8, 0.98),
                   (0.72, 1.0), (0.66, 0.94)], sides)
    seam = lathe([(0.585, 0.38), (0.6, 0.42), (0.6, 0.47), (0.585, 0.5)], sides)
    metal = disc(0.67, 0.93, sides)
    lugs = boxes([(0.74, 0.62, 0.0), (-0.74, 0.62, 0.0)], (0.12, 0.08, 0.08))
    rivets = boxes([(0.66 * math.cos(a), 0.74, 0.66 * math.sin(a)) for a in
                    [k / 12 * math.tau + 0.2 for k in range(12)]], (0.03, 0.03, 0.03))
    return [(merge(shell, lugs, rivets), 0), (metal, 1), (seam, 2)]


ANVIL_MATERIALS = [
    {'name': 'hot iron', 'doubleSided': True,
     'pbrMetallicRoughness': {'baseColorFactor': [0.1, 0.045, 0.03, 1], 'metallicFactor': 0.6, 'roughnessFactor': 0.45},
     'emissiveFactor': [1.0, 0.28, 0.06], 'extensions': {'KHR_materials_emissive_strength': {'emissiveStrength': 0.45}}},
    {'name': 'molten', 'doubleSided': True,
     'pbrMetallicRoughness': {'baseColorFactor': [1.0, 0.6, 0.2, 1], 'metallicFactor': 0.0, 'roughnessFactor': 0.6},
     'emissiveFactor': [1.0, 0.55, 0.15], 'extensions': {'KHR_materials_emissive_strength': {'emissiveStrength': 10.0}}},
]


def along_x(part, y, tilt=0.0):
    """Turns a part built along +Y to lie along +X at height `y`."""
    positions, normals, indices = part
    c, s = math.cos(tilt), math.sin(tilt)

    def turn(v, lift):
        x, yy, z = v[1], -v[0], v[2]
        return (x * c - yy * s, x * s + yy * c + lift, z)

    return [turn(p, y) for p in positions], [turn(n, 0.0) for n in normals], indices


def anvil():
    """A smith's anvil (about 0.95 m tall, its foot at the origin, the horn towards +X):
    a splayed foot, a narrow waist, a heavy face with a heel and a tapering horn, molten
    metal glowing in cracks across the face and down the sides."""
    foot = boxes([(0.0, 0.08, 0.0)], (0.44, 0.08, 0.3))
    plinth = boxes([(0.0, 0.2, 0.0)], (0.32, 0.05, 0.22))
    waist = boxes([(0.0, 0.38, 0.0)], (0.2, 0.14, 0.15))
    face = boxes([(-0.02, 0.7, 0.0)], (0.48, 0.18, 0.24))
    heel = boxes([(-0.6, 0.78, 0.0)], (0.12, 0.1, 0.17))
    horn = along_x(lathe([(0.0, -0.0), (0.21, 0.0), (0.17, 0.2), (0.1, 0.42), (0.0, 0.62)], 14), 0.72)
    horn = ([(x + 0.46, y, z) for x, y, z in horn[0]], horn[1], horn[2])
    cracks = boxes([(0.0, 0.885, 0.0), (-0.22, 0.885, 0.08), (0.2, 0.885, -0.07), (0.0, 0.7, 0.245),
                    (0.0, 0.7, -0.245), (0.0, 0.42, 0.152)],
                   (0.4, 0.006, 0.022))
    cross = boxes([(-0.1, 0.886, 0.0), (0.26, 0.886, 0.0)], (0.02, 0.006, 0.2))
    return [(merge(foot, plinth, waist, face, heel, horn), 0), (merge(cracks, cross), 1)]


BOLT_MATERIALS = [
    {'name': 'brass', 'doubleSided': True,
     'pbrMetallicRoughness': {'baseColorFactor': [0.78, 0.56, 0.24, 1], 'metallicFactor': 0.9, 'roughnessFactor': 0.35}},
    {'name': 'substance', 'doubleSided': True,
     'pbrMetallicRoughness': {'baseColorFactor': [0.55, 0.4, 1.0, 1], 'metallicFactor': 0.0, 'roughnessFactor': 0.5},
     'emissiveFactor': [0.6, 0.4, 1.0], 'extensions': {'KHR_materials_emissive_strength': {'emissiveStrength': 8.0}}},
]


def bolt():
    """A repeater bolt along +Y (its flight): a brass shaft with four fins and a glowing
    substance head, 0.86 m long."""
    shaft = lathe([(0.0, -0.4), (0.035, -0.4), (0.035, 0.2), (0.0, 0.2)], 10)
    fins = boxes([(0.06, -0.3, 0.0), (-0.06, -0.3, 0.0), (0.0, -0.3, 0.06), (0.0, -0.3, -0.06)],
                 (0.035, 0.09, 0.008))
    fins2 = boxes([(0.0, -0.3, 0.06), (0.0, -0.3, -0.06)], (0.008, 0.09, 0.035))
    head = lathe([(0.0, 0.17), (0.07, 0.2), (0.05, 0.3), (0.0, 0.46)], 10)
    return [(merge(shaft, fins, fins2), 0), (head, 1)]


def model():
    folder = os.path.join(OUT, 'bolt')
    asset(folder, {'id': 'asset.models.effects.beetles-bolt', 'name': 'Repeater bolt', 'kind': 'prop',
                   'tags': ['effect', 'beetles', 'projectile'],
                   'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'}],
                   'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
                   'license': LICENSE + ' A brass repeater bolt with a substance head.'})
    glb(os.path.join(folder, 'geometry.glb'), bolt(), BOLT_MATERIALS, 'bolt')

    folder = os.path.join(OUT, 'crucible')
    asset(folder, {'id': 'asset.models.effects.beetles-crucible', 'name': 'Forgefall crucible', 'kind': 'prop',
                   'tags': ['effect', 'beetles', 'forgemaster'],
                   'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'}],
                   'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
                   'license': LICENSE + ' The Forgemaster\'s falling crucible.'})
    glb(os.path.join(folder, 'geometry.glb'), crucible())

    folder = os.path.join(OUT, 'anvil')
    asset(folder, {'id': 'asset.models.effects.beetles-anvil', 'name': 'Forgefall anvil', 'kind': 'prop',
                   'tags': ['effect', 'beetles', 'forgemaster'],
                   'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'}],
                   'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
                   'license': LICENSE + ' The Forgemaster\'s falling anvil, red-hot, molten metal in its cracks.'})
    glb(os.path.join(folder, 'geometry.glb'), anvil(), ANVIL_MATERIALS, 'anvil')


if __name__ == '__main__':
    textures()
    model()
    print('wrote', len(TEXTURES), 'textures, the bolt, the crucible and the anvil to', OUT)
