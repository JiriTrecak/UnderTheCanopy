"""Draws a map's blocks from above as a picture, to check a layout: ground shaded by
level (dark low to pale high), ramps amber, closed cells (forest floor, rock) dark green,
water blue, ice pale blue, cliffs dark, starts as red rings; with `--reach`, where an army
reaches from the first start tinted green and the margin it keeps from cliffs red.

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \\
        tools/maps/blocks_map.py <map package> <out.png> [--scale 4] [--reach]"""
import argparse, json, os, struct, zlib
import numpy as np
import OpenImageIO as oiio

HALO = 16
BLOCK = 4


def grid(path):
    b = open(path, 'rb').read()
    _magic, _fmt, sample, codec, shuffle, w, h = struct.unpack('<4sBBBBII', b[:16])
    raw = zlib.decompress(b[16:]) if codec else b[16:]
    size = {0: 1, 1: 4, 2: 4}.get(sample, 4)
    if shuffle and size > 1:
        n = len(raw) // size
        raw = np.frombuffer(raw, np.uint8).reshape(size, n).T.copy().tobytes()
    dt = np.float32 if sample == 1 else np.uint8
    return w, h, np.frombuffer(raw, dt)


def main():
    p = argparse.ArgumentParser()
    p.add_argument('map')
    p.add_argument('out')
    p.add_argument('--scale', type=int, default=4)
    p.add_argument('--reach', action='store_true', help='tint where an army reaches from the first start')
    a = p.parse_args()
    t = os.path.join(a.map, 'terrain')
    side, _, blocks = grid(os.path.join(t, 'blocks.grid'))
    blocks = blocks.reshape(side, side, 4)
    span = side * BLOCK
    paint = None
    if os.path.exists(os.path.join(t, 'block-paint.grid')):
        paint = grid(os.path.join(t, 'block-paint.grid'))[2].reshape(span, span)
    blocked = grid(os.path.join(t, 'blocked.grid'))[2].reshape(span, span)
    vs, _, heights = grid(os.path.join(t, 'heights.grid'))
    heights = heights.reshape(vs, vs)
    img = np.zeros((span, span, 3), np.float32)
    for cz in range(span):
        for cx in range(span):
            b = blocks[cz // BLOCK, cx // BLOCK]
            kind = b[1]
            if paint is not None and paint[cz, cx] and kind != 1:
                kind = paint[cz, cx] - 1
            h = heights[cz:cz + 2, cx:cx + 2].mean() / 3.0
            ground = 0.18 + 0.2 * h
            colour = {
                0: (ground * 0.9, ground, ground * 0.85),
                1: (0.85, 0.6, 0.25),
                2: (0.08, 0.16, 0.1),
                3: (0.3, 0.3, 0.32),
                4: (0.35, 0.6, 0.8),
                5: (0.1, 0.25, 0.55),
                6: (0.75, 0.88, 0.95),
            }[int(kind)]
            if blocked[cz, cx] and kind not in (2, 3):
                colour = (0.08, 0.16, 0.1)
            img[cz, cx] = colour
    # Cliffs: steep cells dark.
    hz = heights[:-1, :-1]
    steep = (np.abs(heights[1:, :-1] - hz) > 1.2) | (np.abs(heights[:-1, 1:] - hz) > 1.2)
    img[steep[:span, :span]] *= 0.35
    m = json.load(open(os.path.join(a.map, 'map.json')))
    if a.reach and m.get('playerStarts'):
        # Where an army reaches from the first start: cells, not closed, more than three
        # cells from a step over 0.9 m (the simulation's army radius), joined.
        h = heights[:span, :span]
        steep = np.zeros((span, span), bool)
        for dz in (-1, 0, 1):
            for dx in (-1, 0, 1):
                if dx or dz:
                    other = np.roll(np.roll(h, dz, 0), dx, 1)
                    steep |= np.abs(h - other) > 0.9
        stop = steep | (blocked != 0)
        for _ in range(3):
            grown = stop.copy()
            for dz in (-1, 0, 1):
                for dx in (-1, 0, 1):
                    grown |= np.roll(np.roll(stop, dz, 0), dx, 1)
            stop = grown
        s0 = m['playerStarts'][0]
        sx, sz = int(s0['x'] + HALO), int(s0['z'] + HALO)
        seen = np.zeros_like(stop)
        # From the nearest open cell to the start.
        best = None
        for r in range(0, 40):
            ys, xs = np.nonzero(~stop[max(0, sz - r):sz + r + 1, max(0, sx - r):sx + r + 1])
            if len(ys):
                best = (max(0, sz - r) + ys[0], max(0, sx - r) + xs[0])
                break
        stack = [best] if best else []
        while stack:
            z, x = stack.pop()
            if seen[z, x] or stop[z, x]:
                continue
            seen[z, x] = True
            for dz, dx in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nz, nx = z + dz, x + dx
                if 0 <= nz < span and 0 <= nx < span and not seen[nz, nx] and not stop[nz, nx]:
                    stack.append((nz, nx))
        img[seen] = img[seen] * 0.5 + np.array([0.15, 0.45, 0.2])
        img[stop & (blocked == 0) & ~steep] = img[stop & (blocked == 0) & ~steep] * 0.6 + np.array([0.3, 0.05, 0.05])
    big = np.repeat(np.repeat(img, a.scale, 0), a.scale, 1)
    for s in m.get('playerStarts', []):
        x, z = (s['x'] + HALO) * a.scale, (s['z'] + HALO) * a.scale
        yy, xx = np.mgrid[0:big.shape[0], 0:big.shape[1]]
        ring = np.abs(np.hypot(xx - x, yy - z) - 26 * a.scale) < a.scale * 0.8
        big[ring] = (0.9, 0.2, 0.2)
    o = oiio.ImageOutput.create(a.out)
    o.open(a.out, oiio.ImageSpec(big.shape[1], big.shape[0], 3, oiio.UINT8))
    o.write_image(np.clip(big * 255 + 0.5, 0, 255).astype(np.uint8))
    o.close()
    print('wrote', a.out)


if __name__ == '__main__':
    main()
