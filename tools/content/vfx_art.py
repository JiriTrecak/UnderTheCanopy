"""Shared effect textures, drawn procedurally so they stay crisp and reproducible:

    python3 tools/content/vfx_art.py

Writes asset folders under base/assets/effects/vfx/ (pure Python, no dependencies). Every
sprite is grey: its brightness is what a layer's `ramp` colours (or its colour tints) and
its alpha is its shape, built so erosion (`erosionOverLife`) eats it from its thin edges
to its dense centre:

- `puff`: four billowing cloud puffs (a 2 × 2 atlas): fire, smoke and dust;
- `flame-strip`: a strip of licking flame tongues, repeating along its length (ribbons);
- `ash`: four ragged ash flakes (2 × 2);
- `chunk`: four lumps of rock or slag with a lit top (2 × 2);
- `blast-ring`: a ground shock ring smeared outwards, for blast waves;
- `mote`: a soft round mote with a dense heart: sparkles, droplets, embers, glowing heads;
- `wake`: a soft band repeating along its length, faintly streaked: the air wake of a
  thorn or a bolt (ribbons).
"""
import json
import math
import os
import struct
import zlib

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'assets', 'effects', 'vfx')
LICENSE = 'Original procedural effect art for Under the Canopy (tools/content/vfx_art.py).'


def png(path, width, height, pixel):
    """Writes an RGBA PNG; `pixel(i, j)` returns (r, g, b, a) floats in 0–1."""
    rows = bytearray()
    for j in range(height):
        rows.append(0)
        for i in range(width):
            rows.extend(max(0, min(255, int(round(c * 255)))) for c in pixel(i, j))

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
    n = (x * 374761393 + y * 668265263 + seed * 1442695041) & 0xffffffff
    n = ((n ^ (n >> 13)) * 1274126177) & 0xffffffff
    return ((n ^ (n >> 16)) & 0xffff) / 65535.0


def noise(x, y, seed, period=None):
    """Smooth value noise; with `period` it repeats every `period` along x."""
    x0, y0 = math.floor(x), math.floor(y)
    fx, fy = x - x0, y - y0
    ux, uy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)

    def h(i, j):
        if period:
            i %= period
        return hash2(i, j, seed)

    a, b = h(x0, y0), h(x0 + 1, y0)
    c, d = h(x0, y0 + 1), h(x0 + 1, y0 + 1)
    return (a + (b - a) * ux) * (1 - uy) + (c + (d - c) * ux) * uy


def fbm(x, y, seed, octaves=4, period=None):
    total, amp, norm = 0.0, 1.0, 0.0
    for o in range(octaves):
        p = period * (2 ** o) if period else None
        total += noise(x * 2 ** o, y * 2 ** o, seed + o * 31, p) * amp
        norm += amp
        amp *= 0.5
    return total / norm


def atlas(size, cells, cell_fn):
    """A `cells` × `cells` atlas of `size` px; `cell_fn(k, u, v)` draws cell `k` at
    −1…1 coordinates (y up)."""
    step = size // cells

    def pixel(i, j):
        k = (j // step) * cells + i // step
        u = ((i % step) + 0.5) / step * 2 - 1
        v = 1 - ((j % step) + 0.5) / step * 2
        return cell_fn(k, u, v)

    return pixel


def puff(k, u, v):
    seed = 101 + k * 17
    # A lumpy outline: the radius wobbles with angle, the inside billows.
    warp_u = u + (fbm(u * 1.6 + 3, v * 1.6, seed + 5, 3) - 0.5) * 0.55
    warp_v = v + (fbm(u * 1.6, v * 1.6 + 7, seed + 9, 3) - 0.5) * 0.55
    r = math.hypot(warp_u, warp_v) / 0.95
    body = 1 - smooth(0.05, 1.0, r)
    billow = fbm(u * 2.4 + k * 3, v * 2.4, seed, 5)
    # Alpha keeps a gradient all the way in (thin wisps at the rim, lumps of billow
    # inside, densest at the heart), so erosion opens holes before it reaches the centre.
    density = max(0.0, body * (0.35 + 1.1 * billow) - 0.08)
    alpha = min(1.0, density ** 0.85)
    # Lit from above, densest at the heart: bright core, darker rim and underside.
    light = 0.55 + 0.45 * smooth(-0.9, 0.7, v + (billow - 0.5) * 0.6)
    value = min(1.0, (0.18 + 0.82 * density ** 0.75) * light)
    return value, value, value, alpha


def flame_strip(i, j, w, h):
    # u repeats over the width; v is across the strip (0 at the centre line).
    u = i / w
    v = (j + 0.5) / h * 2 - 1
    # Contrasty noise: tongues reach far out between narrow necks.
    tongues = smooth(0.3, 0.7, fbm(u * 6, v * 1.5 + 2, 701, 4, period=6))
    licks = fbm(u * 14, v * 3 + 9, 709, 3, period=14)
    edge = 0.35 + 0.8 * tongues + 0.35 * (licks - 0.5)
    across = abs(v) * 1.05
    shape = 1 - smooth(edge * 0.35, edge, across)
    core = (1 - smooth(0.0, 0.55, across)) * (0.75 + 0.25 * licks)
    # Bright throughout (a layer's colour tints it; additive strips on bright ground need
    # the light), a little hotter along the core; the shape is in the alpha.
    value = min(1.0, 0.72 + 0.28 * core + 0.1 * (tongues - 0.5))
    return value, value, value, min(1.0, shape * (0.7 + 0.5 * tongues))


def ash(k, u, v):
    seed = 401 + k * 13
    angle = math.atan2(v, u)
    r = math.hypot(u, v)
    # A ragged flake: a jagged polygon with a few notches.
    rim = 0.8 + 0.25 * (fbm(math.cos(angle) * 1.4 + 2, math.sin(angle) * 1.4 + k, seed, 3) - 0.5) * 2
    rim *= 0.85 + 0.15 * math.cos(angle * (3 + k) + k)
    alpha = 1 - smooth(rim - 0.06, rim, r)
    value = 0.3 + 0.5 * fbm(u * 5 + k, v * 5, seed + 3, 3) + 0.2 * smooth(-0.6, 0.6, v)
    return value, value, value, alpha


def chunk_cell(k, u, v):
    seed = 503 + k * 19
    angle = math.atan2(v, u)
    r = math.hypot(u, v)
    rim = 0.82 + 0.16 * (fbm(math.cos(angle) + 5, math.sin(angle) + k, seed, 3) - 0.5) * 2
    alpha = 1 - smooth(rim - 0.05, rim, r)
    # A faceted lump lit from the upper left: brighter facets on top, dark underside.
    facet = math.floor((angle / math.tau + 0.5) * (5 + k)) / (5 + k)
    shade = 0.35 + 0.45 * smooth(-0.8, 0.8, v * 0.8 - u * 0.4) + 0.12 * (hash2(int(facet * 97), k, seed) - 0.5)
    grain = 0.85 + 0.15 * fbm(u * 9, v * 9, seed + 1, 2)
    value = max(0.0, min(1.0, shade * grain * (1 - 0.35 * smooth(0.3, 1.0, r / max(rim, 1e-3)))))
    return value, value, value, alpha


def blast_ring(i, j, w, h):
    u = (i + 0.5) / w * 2 - 1
    v = 1 - (j + 0.5) / h * 2
    r = math.hypot(u, v)
    angle = math.atan2(v, u)
    # Radial smears: the ring's outer edge is sharp, its inside streaks back to the centre.
    streak = fbm(math.cos(angle) * 6 + 3, math.sin(angle) * 6 + 1, 811, 4)
    front = 0.86 + (streak - 0.5) * 0.08
    lead = 1 - smooth(front - 0.02, front + 0.03, r)
    tail = smooth(front - 0.45 - 0.25 * streak, front - 0.04, r)
    alpha = lead * tail * (0.6 + 0.5 * streak)
    value = 0.4 + 0.6 * smooth(front - 0.3, front - 0.02, r)
    return value, value, value, min(1.0, alpha)


def mote(i, j, w, h):
    u = (i + 0.5) / w * 2 - 1
    v = 1 - (j + 0.5) / h * 2
    r = math.hypot(u, v)
    alpha = (1 - smooth(0.0, 1.0, r)) ** 1.6
    value = 0.55 + 0.45 * (1 - smooth(0.0, 0.45, r))
    return value, value, value, alpha


def wake(i, j, w, h):
    u = i / w
    v = (j + 0.5) / h * 2 - 1
    streak = fbm(u * 8, v * 2 + 3, 911, 3, period=8)
    across = math.exp(-(v / (0.38 + 0.12 * streak)) ** 2)
    value = 0.88 + 0.12 * streak
    return value, value, value, min(1.0, across * (0.75 + 0.35 * streak))


TEXTURES = {
    'puff': ('Cloud puff', 256, 256, atlas(256, 2, puff),
             'Four billowing cloud puffs (2 × 2): fire, smoke and dust, bright and dense at the heart.'),
    'flame-strip': ('Flame strip', 256, 64, lambda i, j: flame_strip(i, j, 256, 64),
                    'Licking flame tongues along a strip, repeating along its length: fire ribbons.'),
    'ash': ('Ash flakes', 128, 128, atlas(128, 2, ash), 'Four ragged ash flakes (2 × 2).'),
    'chunk': ('Debris chunks', 128, 128, atlas(128, 2, chunk_cell),
              'Four lumps of rock or slag lit from above (2 × 2): debris.'),
    'blast-ring': ('Blast ring', 256, 256, lambda i, j: blast_ring(i, j, 256, 256),
                   'A ground shock ring with a sharp front smeared back towards the centre.'),
    'mote': ('Soft mote', 64, 64, lambda i, j: mote(i, j, 64, 64),
             'A soft round mote with a dense heart: sparkles, droplets, embers and glowing heads.'),
    'wake': ('Air wake', 128, 32, lambda i, j: wake(i, j, 128, 32),
             'A soft band repeating along its length, faintly streaked: the air wake behind a thorn or bolt.'),
}


def main():
    for name, (title, w, h, fn, about) in TEXTURES.items():
        folder = os.path.join(OUT, name)
        os.makedirs(folder, exist_ok=True)
        doc = {'id': 'asset.effects.vfx.' + name, 'name': title, 'kind': 'texture',
               'tags': ['effect', 'vfx', 'particle'],
               'resources': [{'role': 'image', 'index': 1, 'format': 'png'}],
               'license': LICENSE + ' ' + about}
        with open(os.path.join(folder, 'asset.json'), 'w') as f:
            f.write(json.dumps(doc, indent=2, ensure_ascii=False) + '\n')
        png(os.path.join(folder, 'image.png'), w, h, fn)
        print('wrote', name)


if __name__ == '__main__':
    main()
