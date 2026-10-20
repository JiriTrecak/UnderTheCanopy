"""Builds a map's painted ground (terrain.overlay, D63): a large unique image cut to the
map's non-playable frame by its own layout, so the painting never needs to match the
map's shapes, with fallen leaves scattered over its edge.

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \
        tools/terrain/build_overlay.py <painting> <asset name> <x> <z> <width> <depth>
        [--inner 3] [--outer 12] [--leaves <sheet.png> …] [--leaf-size 2.4 4.6]

The painting fades in smoothly from `inner` to `outer` metres outside the clearing. Over
that edge go single fallen leaves cut from leaf sheets (separate leaves photographed on
white): every leaf is one connected shape of a sheet, placed whole, with a random size and
turn and a soft shadow. They lie densely along the rim, thin out into single leaves on the
soil, and grow sparse under the painting deeper in, so the frame's edge is made of leaf
silhouettes rather than a cut-out line.

Reads the layout from tools/maps/lookdev_fall.py (`OPEN`, `fields`), so it is that map's
tool; another map passes its own module the same way."""
import argparse, json, math, os, sys
import numpy as np
import OpenImageIO as oiio

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools', 'maps'))


def leaves_of(path, min_pixels=900):
    """The separate leaves of a sheet on white, or of a transparent sheet: (premultiplied
    RGBA crops)."""
    pixels = oiio.ImageBuf(path).get_pixels(oiio.FLOAT)
    rgb = pixels[:, :, :3]
    if pixels.shape[2] == 4 and pixels[:, :, 3].min() < 0.5:
        # OpenImageIO reads PNG alpha associated; the crops below premultiply again.
        alpha = pixels[:, :, 3]
        rgb = rgb / np.maximum(alpha, 1e-4)[..., None]
    else:
        # Faint grey shadows on the sheet stay out: only clearly coloured pixels are leaf.
        alpha = np.clip(((1.0 - rgb).max(axis=2) - 0.13) / 0.1, 0.0, 1.0)
    solid = alpha > 0.5
    # Connected shapes by repeated min-label propagation on a half-size mask.
    small = solid[::2, ::2]
    h, w = small.shape
    big = h * w + 1
    labels = np.where(small, np.arange(h * w).reshape(h, w), big)
    for _ in range(4000):
        prev = labels
        p = np.pad(labels, 1, constant_values=big)
        n = np.minimum.reduce([p[1:-1, 1:-1], p[:-2, 1:-1], p[2:, 1:-1], p[1:-1, :-2], p[1:-1, 2:]])
        labels = np.where(small, n, big)
        if np.array_equal(labels, prev):
            break
    out = []
    for label in np.unique(labels[small]):
        ys, xs = np.nonzero(labels == label)
        if len(ys) * 4 < min_pixels:
            continue
        y0, y1 = max(0, ys.min() * 2 - 4), min(rgb.shape[0], ys.max() * 2 + 6)
        x0, x1 = max(0, xs.min() * 2 - 4), min(rgb.shape[1], xs.max() * 2 + 6)
        mask = np.zeros(rgb.shape[:2], bool)
        mask[y0:y1, x0:x1] = np.repeat(np.repeat(labels == label, 2, 0), 2, 1)[:rgb.shape[0], :rgb.shape[1]][y0:y1, x0:x1]
        a = alpha[y0:y1, x0:x1] * mask[y0:y1, x0:x1]
        out.append(np.concatenate([rgb[y0:y1, x0:x1] * a[..., None], a[..., None]], -1).astype(np.float32))
    return out


def stamp(leaf, size_px, angle_deg):
    """A leaf scaled so its longer side is `size_px`, turned by `angle_deg`."""
    h, w = leaf.shape[:2]
    s = size_px / max(h, w)
    nw, nh = max(2, int(w * s)), max(2, int(h * s))
    buf = oiio.ImageBuf(oiio.ImageSpec(w, h, 4, oiio.FLOAT))
    buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 4), leaf)
    buf = oiio.ImageBufAlgo.resize(buf, roi=oiio.ROI(0, nw, 0, nh, 0, 1, 0, 4))
    side = int(math.ceil(math.hypot(nw, nh))) + 2
    pad = oiio.ImageBuf(oiio.ImageSpec(side, side, 4, oiio.FLOAT))
    oiio.ImageBufAlgo.zero(pad)
    oiio.ImageBufAlgo.paste(pad, (side - nw) // 2, (side - nh) // 2, 0, 0, buf)
    turned = oiio.ImageBufAlgo.rotate(pad, math.radians(angle_deg), roi=pad.roi)
    return turned.get_pixels(oiio.FLOAT).reshape(side, side, 4)


def over(dst, src, x, y):
    """Premultiplied `src` over `dst` with its top-left at (x, y), clipped."""
    h, w = src.shape[:2]
    H, W = dst.shape[:2]
    x0, y0, x1, y1 = max(0, x), max(0, y), min(W, x + w), min(H, y + h)
    if x0 >= x1 or y0 >= y1:
        return
    s = src[y0 - y:y1 - y, x0 - x:x1 - x]
    d = dst[y0:y1, x0:x1]
    d[...] = s + d * (1.0 - s[..., 3:4])


def blur(a, r):
    for axis in (0, 1):
        acc = np.zeros_like(a)
        for k in range(-r, r + 1):
            acc += np.roll(a, k, axis)
        a = acc / (2 * r + 1)
    return a


def main():
    p = argparse.ArgumentParser()
    p.add_argument('painting')
    p.add_argument('name')
    p.add_argument('x', type=float)
    p.add_argument('z', type=float)
    p.add_argument('width', type=float)
    p.add_argument('depth', type=float)
    p.add_argument('--map', default='lookdev_fall')
    p.add_argument('--inner', type=float, default=3.0, help='painting starts this far outside the clearing (m)')
    p.add_argument('--outer', type=float, default=12.0, help='painting fully covers from here outwards (m)')
    p.add_argument('--tone', type=float, default=1.0)
    p.add_argument('--leaves', nargs='*', default=[])
    p.add_argument('--leaf-size', type=float, nargs=2, default=(2.4, 4.6), help='metres')
    p.add_argument('--leaf-tone', type=float, default=1.0)
    p.add_argument('--leaf-saturation', type=float, default=1.0)
    p.add_argument('--leaf-density', type=float, default=1.0, help='scales how many whole leaves lie on the edge')
    p.add_argument('--dry', type=float, default=0.35, help='share of leaves that are dry and dark')
    p.add_argument('--seed', type=int, default=5)
    a = p.parse_args()
    m = __import__(a.map)
    rng = np.random.default_rng(a.seed)

    img = oiio.ImageBuf(a.painting).get_pixels(oiio.FLOAT)[:, :, :3] * a.tone
    h, w = img.shape[:2]
    per_metre = w / a.width
    xs = a.x + (np.arange(w) + 0.5) / w * a.width
    zs = a.z + (np.arange(h) + 0.5) / h * a.depth
    gx, gz = np.meshgrid(xs, zs)
    d = m.signed_distance(gx, gz, m.OPEN) + (m.fbm(gx, gz, 7, 91) - 0.5) * 6.0
    _, _, pool = m.fields(gx, gz)
    cover = np.clip((d - a.inner) / (a.outer - a.inner), 0.0, 1.0)
    cover = cover * cover * (3.0 - 2.0 * cover)
    cover *= 1.0 - np.clip((pool - 0.3) / 0.4, 0.0, 1.0)
    # Premultiplied throughout: colour is zero wherever the ground shows through, so
    # filtering and mip maps never bleed the painting into the clearing.
    rgba = np.concatenate([img * cover[:, :, None], cover[:, :, None]], -1).astype(np.float32)

    leaves = [leaf for path in a.leaves for leaf in leaves_of(path)]
    if leaves:
        for leaf in leaves:
            grey = leaf[..., :3].mean(axis=2, keepdims=True)
            leaf[..., :3] = (grey + (leaf[..., :3] - grey) * a.leaf_saturation) * a.leaf_tone
        print('leaves cut from sheets:', len(leaves))
        # Candidates over the rectangle; kept by distance from the clearing: single leaves
        # blown onto the soil, a dense band along the rim, sparse under the painting.
        count = int(a.width * a.depth * 0.55)
        cx = rng.uniform(a.x, a.x + a.width, count)
        cz = rng.uniform(a.z, a.z + a.depth, count)
        cd = m.signed_distance(cx, cz, m.OPEN) + (m.fbm(cx, cz, 7, 91) - 0.5) * 6.0 + rng.normal(0, 1.5, count)
        _, ford, cpool = m.fields(cx, cz)
        keep = np.interp(cd, [-14, -8, -3, 1, 8, 14, 20, 28], [0.0, 0.06, 0.3, 0.9, 0.9, 0.3, 0.06, 0.0])
        keep = np.minimum(keep * a.leaf_density, 1.0)
        keep *= (cpool < 0.2) & (ford < 0.3)
        chosen = rng.random(count) < keep
        order = np.argsort(-cd[chosen])  # deepest first, so the rim's leaves lie on top
        px = ((cx[chosen] - a.x) / a.width * w)[order]
        pz = ((cz[chosen] - a.z) / a.depth * h)[order]
        depth = cd[chosen][order]
        lo, hi = a.leaf_size
        shadow_offset = (int(round(0.35 * per_metre)), int(round(0.45 * per_metre)))

        # Under the leaves: fine debris (leaf fragments, dry and dark) whose density makes
        # the soft gradient from litter to bare soil, and thin twigs.
        count = int(a.width * a.depth * 2.2)
        fx = rng.uniform(a.x, a.x + a.width, count)
        fz = rng.uniform(a.z, a.z + a.depth, count)
        fd = m.signed_distance(fx, fz, m.OPEN) + (m.fbm(fx, fz, 7, 91) - 0.5) * 6.0 + rng.normal(0, 2.0, count)
        _, ford_f, pool_f = m.fields(fx, fz)
        keep_f = np.interp(fd, [-13, -7, -2, 3, 12, 22], [0.0, 0.12, 0.55, 1.0, 0.8, 0.0])
        keep_f *= (pool_f < 0.2) & (ford_f < 0.3)
        picked = np.nonzero(rng.random(count) < keep_f)[0]
        for i in picked:
            leaf = leaves[rng.integers(len(leaves))]
            s = stamp(leaf, rng.uniform(0.35, 1.0) * per_metre, rng.uniform(0, 360))
            s[..., :3] *= rng.uniform(0.25, 0.55)
            over(rgba, s, int((fx[i] - a.x) / a.width * w - s.shape[1] / 2), int((fz[i] - a.z) / a.depth * h - s.shape[0] / 2))
        twig_colour = np.array([0.11, 0.075, 0.05, 1.0], np.float32)
        count = int(a.width * a.depth * 0.02)
        tx = rng.uniform(a.x, a.x + a.width, count)
        tz = rng.uniform(a.z, a.z + a.depth, count)
        td = m.signed_distance(tx, tz, m.OPEN)
        twigs = 0
        for i in np.nonzero((td > -6) & (td < 14) & (m.fields(tx, tz)[2] < 0.2))[0]:
            length = rng.uniform(2.0, 6.0) * per_metre
            heading = rng.uniform(0, 2 * math.pi)
            bend = rng.uniform(-0.6, 0.6)
            width = max(1, int(rng.uniform(0.08, 0.16) * per_metre))
            dot = np.zeros((2 * width + 1, 2 * width + 1, 4), np.float32)
            yy, xx = np.mgrid[-width:width + 1, -width:width + 1]
            disc = np.clip(width + 0.5 - np.hypot(xx, yy), 0.0, 1.0) * 0.85
            dot[...] = twig_colour * disc[..., None]
            x, y = (tx[i] - a.x) / a.width * w, (tz[i] - a.z) / a.depth * h
            steps = int(length / max(1, width * 0.7))
            for k in range(steps):
                heading += bend / steps
                x += math.cos(heading) * length / steps
                y += math.sin(heading) * length / steps
                over(rgba, dot, int(x) - width, int(y) - width)
            twigs += 1
        print('fragments:', len(picked), 'twigs:', twigs)
        for i in range(len(px)):
            leaf = leaves[rng.integers(len(leaves))]
            # Blown out onto the soil, the smaller leaves travel furthest; most leaves are
            # small, a few large.
            size = (lo + (hi - lo) * rng.random() ** 2) * per_metre * float(np.interp(depth[i], [-10, 0], [0.6, 1.0]))
            s = stamp(leaf, size, rng.uniform(0, 360))
            # Leaves on the soil are a touch lighter than those in the litter.
            # Deeper in, the leaves sink into the frame's shade.
            # A third of the leaves are dry and dark brown.
            dry = rng.uniform(0.32, 0.55) if rng.random() < a.dry else rng.uniform(0.85, 1.1)
            s[..., :3] *= dry * float(np.interp(depth[i], [2, 20], [1.0, 0.55]))
            sh = blur(s[..., 3], max(1, int(0.25 * per_metre)))
            shadow = np.zeros_like(s)
            shadow[..., 3] = sh * 0.45
            x0, y0 = int(px[i] - s.shape[1] / 2), int(pz[i] - s.shape[0] / 2)
            over(rgba, shadow, x0 + shadow_offset[0], y0 + shadow_offset[1])
            over(rgba, s, x0, y0)
        print('leaves placed:', len(px))

    folder = os.path.join(ROOT, 'base', 'assets', 'textures', a.name)
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(folder, 'image.png')
    out = oiio.ImageOutput.create(path)
    out.open(path, oiio.ImageSpec(w, h, 4, oiio.UINT8))
    out.write_image(np.clip(rgba * 255 + 0.5, 0, 255).astype(np.uint8))
    out.close()
    src = os.path.join(folder, 'source.png')
    oiio.ImageBuf(a.painting).write(src)
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump({
            'id': 'asset.texture.' + a.name, 'name': a.name.replace('-', ' '), 'kind': 'texture',
            'tags': ['lookdev', 'imagegen', 'overlay'],
            'resources': [{'role': 'image', 'index': 1, 'format': 'png'}, {'role': 'source', 'index': 1, 'format': 'png'}],
            'license': 'Original ImageGen (gpt-image-2) painting and leaf sheets for Under the Canopy, '
                       'cut and scattered by tools/terrain/build_overlay.py.',
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', folder, w, h)


if __name__ == '__main__':
    main()
