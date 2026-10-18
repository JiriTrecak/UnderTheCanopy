"""Builds a map's painted ground (terrain.overlay, D63): a large unique image cut to the
map's non-playable frame by its own layout, so the painting never needs to match the
map's shapes. The edge breaks up into speckle over the debris transition.

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \
        tools/terrain/build_overlay.py <painting> <asset name> <x> <z> <width> <depth> [--inner -1] [--outer 5]

Reads the layout from tools/maps/lookdev_fall.py (`OPEN`, `fields`), so it is that map's
tool; another map passes its own module the same way."""
import argparse, json, os, sys
import numpy as np
import OpenImageIO as oiio

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
sys.path.insert(0, os.path.join(ROOT, 'tools', 'maps'))


def main():
    p = argparse.ArgumentParser()
    p.add_argument('painting')
    p.add_argument('name')
    p.add_argument('x', type=float)
    p.add_argument('z', type=float)
    p.add_argument('width', type=float)
    p.add_argument('depth', type=float)
    p.add_argument('--map', default='lookdev_fall')
    p.add_argument('--inner', type=float, default=-1.0, help='edge start, metres from the clearing (negative: inside)')
    p.add_argument('--outer', type=float, default=6.0, help='fully covered from here outwards')
    p.add_argument('--tone', type=float, default=1.0)
    a = p.parse_args()
    m = __import__(a.map)

    img = oiio.ImageBuf(a.painting).get_pixels(oiio.FLOAT)[:, :, :3] * a.tone
    h, w = img.shape[:2]
    xs = a.x + (np.arange(w) + 0.5) / w * a.width
    zs = a.z + (np.arange(h) + 0.5) / h * a.depth
    gx, gz = np.meshgrid(xs, zs)
    d = m.signed_distance(gx, gz, m.OPEN) + (m.fbm(gx, gz, 7, 91) - 0.5) * 6.0
    _, _, pool = m.fields(gx, gz)
    cover = np.clip((d - a.inner) / (a.outer - a.inner), 0.0, 1.0)
    # Speckle in the transition so the painted floor thins out as debris.
    rng = np.random.default_rng(3)
    speckle = rng.random((h, w)).astype(np.float32)
    zone = np.clip(4.0 * cover * (1.0 - cover), 0.0, 1.0)
    dithered = np.clip((cover - 0.5 + (speckle - 0.5) * 1.2) * 3.0 + 0.5, 0.0, 1.0)
    alpha = cover * (1.0 - zone) + dithered * zone
    alpha *= 1.0 - np.clip((pool - 0.3) / 0.4, 0.0, 1.0)
    # Stored premultiplied: colour is zero wherever the ground shows through, so filtering
    # and mip maps never bleed the painting into the clearing.
    rgba = np.concatenate([img * alpha[:, :, None], alpha[:, :, None]], -1)

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
            'license': 'Original ImageGen (gpt-image-2) painting for Under the Canopy, cut to the map by tools/terrain/build_overlay.py.',
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', folder, w, h)


if __name__ == '__main__':
    main()
