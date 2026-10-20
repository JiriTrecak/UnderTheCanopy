"""Compares a render with its target picture, region by region (look-dev, D80): how close
the colour, the contrast and the detail are, and where the two differ most.

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \
        tools/look/compare.py <regions.json> <render.png> [--out <folder>]

`regions.json` names the target and its regions:

    {"target": "art/styles/target-frost-cliff.jpg",
     "regions": {"ice": [0.66, 0.35, 0.9, 0.95], ...}}

each region a box in fractions of the frame (x0, y0, x1, y1), or a polygon of [x, y]
fractions. Both pictures are compared at the same size, softened a little first so the
comparison is of what the eye reads rather than of individual pixels.

For every region it prints the mean colour of each (sRGB 0–255), the difference in
perceptual lightness and colour (CIELAB ΔL, Δa, Δb and ΔE), the contrast (the spread of
lightness) and the detail (mean lightness gradient), with the render's share of the
target's. The total is the area-weighted mean ΔE over the frame. It writes
`<out>/compare.jpg`: target, render and a ΔE heat map (black: alike, white: ΔE 30 or
more), with the regions outlined."""
import argparse, json, os, sys
import numpy as np
import OpenImageIO as oiio

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
W, H = 960, 540


def load(path):
    buf = oiio.ImageBuf(path)
    buf = oiio.ImageBufAlgo.resize(buf, roi=oiio.ROI(0, W, 0, H, 0, 1, 0, buf.spec().nchannels))
    return np.ascontiguousarray(buf.get_pixels(oiio.FLOAT)[:, :, :3])


def soften(a, r=2):
    for _ in range(2):
        for axis in (0, 1):
            acc = np.zeros_like(a)
            for d in range(-r, r + 1):
                acc += np.roll(a, d, axis=axis)
            a = acc / (2 * r + 1)
    return a


def lab(srgb):
    c = np.where(srgb <= 0.04045, srgb / 12.92, ((srgb + 0.055) / 1.055) ** 2.4)
    m = np.array([[0.4124, 0.3576, 0.1805], [0.2126, 0.7152, 0.0722], [0.0193, 0.1192, 0.9505]])
    xyz = c @ m.T / np.array([0.9505, 1.0, 1.089])
    f = np.where(xyz > 0.008856, np.cbrt(xyz), 7.787 * xyz + 16 / 116)
    return np.stack([116 * f[..., 1] - 16, 500 * (f[..., 0] - f[..., 1]), 200 * (f[..., 1] - f[..., 2])], -1)


def mask_of(shape):
    yy, xx = np.mgrid[0:H, 0:W]
    fx, fy = (xx + 0.5) / W, (yy + 0.5) / H
    if len(shape) == 4 and not isinstance(shape[0], list):
        x0, y0, x1, y1 = shape
        return (fx >= x0) & (fx < x1) & (fy >= y0) & (fy < y1)
    inside = np.zeros((H, W), bool)
    n = len(shape)
    for i in range(n):
        (ax, ay), (bx, by) = shape[i], shape[(i + n - 1) % n]
        cross = ((ay > fy) != (by > fy)) & (fx < (bx - ax) * (fy - ay) / (by - ay + 1e-9) + ax)
        inside ^= cross
    return inside


def gradient(l):
    gx = np.abs(np.diff(l, axis=1, append=l[:, -1:]))
    gy = np.abs(np.diff(l, axis=0, append=l[-1:, :]))
    return gx + gy


def main():
    p = argparse.ArgumentParser()
    p.add_argument('regions')
    p.add_argument('render')
    p.add_argument('--out', default=None)
    a = p.parse_args()
    spec = json.load(open(a.regions))
    target = load(os.path.join(ROOT, spec['target']))
    render = load(a.render)
    # Detail from the sharp pictures, everything else from softened ones.
    lt_sharp, lr_sharp = lab(target)[..., 0], lab(render)[..., 0]
    st, sr = soften(target), soften(render)
    lt, lr = lab(st), lab(sr)
    de = np.sqrt(((lt - lr) ** 2).sum(-1))
    gt, gr = gradient(lt_sharp), gradient(lr_sharp)
    rows = []
    print(f"{'region':14s} {'target rgb':>15s} {'render rgb':>15s}   ΔL    Δa    Δb    ΔE  contrast  detail")
    for name, shape in spec['regions'].items():
        m = mask_of(shape)
        if not m.any():
            continue
        ct = tuple(int(v) for v in (st[m].mean(0) * 255).round())
        cr = tuple(int(v) for v in (sr[m].mean(0) * 255).round())
        d = lr[m].mean(0) - lt[m].mean(0)
        contrast = lr[m][:, 0].std() / max(lt[m][:, 0].std(), 1e-3)
        detail = gr[m].mean() / max(gt[m].mean(), 1e-3)
        e = de[m].mean()
        rows.append((name, e))
        print(f"{name:14s} {str(ct):>15s} {str(cr):>15s} {d[0]:+5.1f} {d[1]:+5.1f} {d[2]:+5.1f} {e:5.1f}   {contrast:5.2f}   {detail:5.2f}")
    print(f"{'frame':14s} {'':>15s} {'':>15s} {'':17s} {de.mean():5.1f}")
    out = a.out or os.path.dirname(os.path.abspath(a.render))
    heat = np.clip(de / 30.0, 0, 1)[..., None].repeat(3, -1)
    sheet = np.concatenate([target, render, heat], 1)
    # Region outlines on all three panels.
    for name, shape in spec['regions'].items():
        m = mask_of(shape)
        edge = m ^ np.roll(m, 1, 0) | m ^ np.roll(m, 1, 1)
        for k in range(3):
            sheet[:, k * W:(k + 1) * W][edge] = [1.0, 0.85, 0.2]
    path = os.path.join(out, 'compare.jpg')
    o = oiio.ImageOutput.create(path)
    o.open(path, oiio.ImageSpec(W * 3, H, 3, oiio.UINT8))
    o.write_image(np.clip(sheet * 255 + 0.5, 0, 255).astype(np.uint8))
    o.close()
    print('wrote', path)


if __name__ == '__main__':
    main()
