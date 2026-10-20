"""Builds a decal atlas (D80, `canopy_render::dressing`): every separate shape of one or
more sheets photographed on white (leaves, clover) cut out and packed into a square grid
of cells, premultiplied RGBA, as a texture asset a biome's `blocks.decals` names.

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \
        tools/terrain/build_decals.py <asset name> <sheet.png> [<sheet.png> …] [--tone 1] [--saturation 1]

Prints the grid size (`cells`) and how many cells hold a shape (`count`)."""
import argparse, json, math, os, sys
import numpy as np
import OpenImageIO as oiio

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from build_overlay import leaves_of  # noqa: E402

SIZE = 1024


def main():
    p = argparse.ArgumentParser()
    p.add_argument('name')
    p.add_argument('sheets', nargs='+')
    p.add_argument('--tone', type=float, default=1.0)
    p.add_argument('--saturation', type=float, default=1.0)
    a = p.parse_args()
    shapes = [s for path in a.sheets for s in leaves_of(path)]
    cells = max(1, math.ceil(math.sqrt(len(shapes))))
    cell = SIZE // cells
    atlas = np.zeros((SIZE, SIZE, 4), np.float32)
    for i, shape in enumerate(shapes):
        h, w = shape.shape[:2]
        s = cell * 0.94 / max(h, w)
        nw, nh = max(1, int(w * s)), max(1, int(h * s))
        buf = oiio.ImageBuf(oiio.ImageSpec(w, h, 4, oiio.FLOAT))
        buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 4), shape)
        small = oiio.ImageBufAlgo.resize(buf, roi=oiio.ROI(0, nw, 0, nh, 0, 1, 0, 4)).get_pixels(oiio.FLOAT)
        rgb = small[..., :3]
        alpha = np.clip(small[..., 3:4], 0.0, 1.0)
        colour = rgb / np.maximum(alpha, 1e-4)
        grey = colour.mean(axis=2, keepdims=True)
        colour = np.clip((grey + (colour - grey) * a.saturation) * a.tone, 0.0, 1.0)
        # No pale fringe: soft edges darken towards the shape's shadowed rim, and the edge
        # itself is pulled in a little.
        colour = colour * (0.55 + 0.45 * np.clip(alpha / 0.9, 0.0, 1.0))
        alpha = alpha ** 1.4
        small = np.concatenate([colour * alpha, alpha], -1)
        cx, cz = (i % cells) * cell + (cell - nw) // 2, (i // cells) * cell + (cell - nh) // 2
        atlas[cz:cz + nh, cx:cx + nw] = small
    folder = os.path.join(ROOT, 'base', 'assets', 'textures', a.name)
    os.makedirs(folder, exist_ok=True)
    path = os.path.join(folder, 'image.png')
    out = oiio.ImageOutput.create(path)
    out.open(path, oiio.ImageSpec(SIZE, SIZE, 4, oiio.UINT8))
    out.write_image(np.clip(atlas * 255 + 0.5, 0, 255).astype(np.uint8))
    out.close()
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump({
            'id': 'asset.texture.' + a.name, 'name': a.name.replace('-', ' '), 'kind': 'texture',
            'tags': ['decals', 'imagegen'],
            'resources': [{'role': 'image', 'index': 1, 'format': 'png'}],
            'license': 'Original ImageGen (gpt-image-2) cut-outs for Under the Canopy, packed by tools/terrain/build_decals.py.',
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', a.name, 'cells', cells, 'count', len(shapes))


if __name__ == '__main__':
    main()
