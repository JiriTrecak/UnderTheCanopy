"""Cuts one shape out of a cut-out sheet (a transparent gpt-image sheet, or one on white)
as its own small texture asset: straight alpha, colour bled into the transparent texels,
for a biome's falling-leaf motes (`motes.leafTexture`) or any single sprite.

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \\
        tools/terrain/build_sprite.py <asset name> <sheet.png> <shape index> [--size 256]
        [--tone 1] [--saturation 1]

Shapes are numbered as tools/terrain/build_decals.py finds them (row by row of their
labels); `--list` prints each shape's mean colour and size to pick one. Writes
base/assets/textures/<asset name>/ (image.png, asset.json), id asset.texture.<name>."""
import argparse, json, os, sys
import numpy as np
import OpenImageIO as oiio

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from build_decals import leaves_of, ROOT  # noqa: E402


def main():
    p = argparse.ArgumentParser()
    p.add_argument('name')
    p.add_argument('sheet')
    p.add_argument('index', type=int, nargs='?', default=0)
    p.add_argument('--size', type=int, default=256)
    p.add_argument('--tone', type=float, default=1.0)
    p.add_argument('--saturation', type=float, default=1.0)
    p.add_argument('--list', action='store_true')
    a = p.parse_args()
    shapes = leaves_of(a.sheet)
    if a.list:
        for i, s in enumerate(shapes):
            al = s[..., 3:4]
            mean = (s[..., :3].sum(axis=(0, 1)) / max(1e-4, al.sum())).round(2)
            print(i, s.shape[:2], mean)
        return
    shape = shapes[a.index]
    alpha = shape[..., 3:4]
    rgb = shape[..., :3] / np.maximum(alpha, 1e-4)
    grey = rgb.mean(axis=2, keepdims=True)
    rgb = np.clip((grey + (rgb - grey) * a.saturation) * a.tone, 0, 1)
    solid = alpha[..., 0] > 0.5
    fill, weight = np.where(solid[..., None], rgb, 0.0), solid.astype(np.float32)
    for _ in range(16):
        acc, wacc = np.zeros_like(fill), np.zeros_like(weight)
        for dy, dx in ((0, 1), (0, -1), (1, 0), (-1, 0)):
            acc += np.roll(np.roll(fill, dy, 0), dx, 1)
            wacc += np.roll(np.roll(weight, dy, 0), dx, 1)
        grow = (weight == 0) & (wacc > 0)
        fill[grow] = acc[grow] / wacc[grow][:, None]
        weight[grow] = 1.0
    rgb = np.where(solid[..., None], rgb, fill)
    h, w = alpha.shape[:2]
    side = max(h, w)
    canvas = np.zeros((side, side, 4), np.float32)
    y0, x0 = (side - h) // 2, (side - w) // 2
    canvas[y0:y0 + h, x0:x0 + w] = np.concatenate([rgb, alpha], -1)
    buf = oiio.ImageBuf(oiio.ImageSpec(side, side, 4, oiio.FLOAT))
    buf.set_pixels(oiio.ROI(0, side, 0, side, 0, 1, 0, 4), canvas)
    small = oiio.ImageBufAlgo.resize(buf, roi=oiio.ROI(0, a.size, 0, a.size, 0, 1, 0, 4)).get_pixels(oiio.FLOAT)
    folder = os.path.join(ROOT, 'base', 'assets', 'textures', a.name)
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(folder, 'image.png')
    out = oiio.ImageOutput.create(path)
    out.open(path, oiio.ImageSpec(a.size, a.size, 4, oiio.UINT8))
    out.write_image(np.clip(small * 255 + 0.5, 0, 255).astype(np.uint8))
    out.close()
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump({
            'id': 'asset.texture.' + a.name, 'name': a.name.replace('-', ' '), 'kind': 'texture',
            'tags': ['sprite', 'imagegen'],
            'resources': [{'role': 'image', 'index': 1, 'format': 'png'}],
            'license': 'Original ImageGen (gpt-image-2.5) cut-out for Under the Canopy, cut by tools/terrain/build_sprite.py.',
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', a.name)


if __name__ == '__main__':
    main()
