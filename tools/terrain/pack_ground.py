"""Packs a generated ground image into the renderer's terrain tiles: a seamless 1024² albedo
tile (sRGB RGB, roughness in alpha) and a normal tile (tangent-space RGB, height in alpha),
each a gzip of raw RGBA (canopy-assets skill, terrain-material).

Runs on Blender's bundled Python, which has numpy and OpenImageIO:

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \
        tools/terrain/pack_ground.py <image> <asset name> \
        [--roughness 0.92] [--normal 2.5] [--blur 2] [--saturation 1] [--value 1] [--tags autumn] [--name "…"]

writes `base/assets/terrain/<asset name>/` (albedo.png, data.bin, source.png, asset.json) and
`base/assets/terrain/<asset name>-normal/` (data.bin, asset.json), ids
`asset.terrain.<asset name>` and `asset.terrain.<asset name>-normal`."""
import argparse, gzip, json, os, shutil
import numpy as np
import OpenImageIO as oiio

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SIZE = 1024


def read_rgb(path):
    buf = oiio.ImageBuf(path)
    spec = buf.spec()
    if (spec.width, spec.height) != (SIZE, SIZE):
        buf = oiio.ImageBufAlgo.resize(buf, roi=oiio.ROI(0, SIZE, 0, SIZE, 0, 1, 0, spec.nchannels))
    pixels = buf.get_pixels(oiio.FLOAT)  # sRGB-encoded values in 0..1
    return np.ascontiguousarray(pixels[:, :, :3], dtype=np.float32)


def seamless(img):
    """Cross-fades the image with a copy rolled by half a tile, so the edges wrap."""
    rolled = np.roll(np.roll(img, SIZE // 2, axis=0), SIZE // 2, axis=1)
    u = (np.arange(SIZE) + 0.5) / SIZE
    edge = np.minimum(u, 1.0 - u)  # 0 at the border, 0.5 in the middle
    w = np.clip((edge - 0.06) / 0.16, 0.0, 1.0)
    w = w * w * (3 - 2 * w)
    mask = np.minimum(w[:, None], w[None, :])[:, :, None]
    return img * mask + rolled * (1.0 - mask)


def blur(a, radius):
    """Wrapping box blur, three passes (close to a Gaussian)."""
    if radius <= 0:
        return a
    for _ in range(3):
        for axis in (0, 1):
            acc = np.zeros_like(a)
            for d in range(-radius, radius + 1):
                acc += np.roll(a, d, axis=axis)
            a = acc / (2 * radius + 1)
    return a


def to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def pack(rgba):
    return gzip.compress(np.clip(rgba * 255.0 + 0.5, 0, 255).astype(np.uint8).tobytes(), 9)


def write_png(path, rgb):
    data = np.clip(rgb * 255.0 + 0.5, 0, 255).astype(np.uint8)
    out = oiio.ImageOutput.create(path)
    out.open(path, oiio.ImageSpec(SIZE, SIZE, 3, oiio.UINT8))
    out.write_image(data)
    out.close()


def asset_json(path, asset_id, name, tags, files, license_):
    with open(path, 'w') as f:
        json.dump({
            'id': asset_id, 'name': name, 'kind': 'terrain-material', 'tags': tags,
            'resources': [{'role': r, 'index': 1, 'format': fmt} for r, fmt in files],
            'license': license_,
        }, f, indent=2, ensure_ascii=False)
        f.write('\n')


def main():
    p = argparse.ArgumentParser()
    p.add_argument('image')
    p.add_argument('name')
    p.add_argument('--roughness', type=float, default=0.92)
    p.add_argument('--normal', type=float, default=2.5, help='normal strength')
    p.add_argument('--blur', type=int, default=2, help='height blur radius, texels')
    p.add_argument('--saturation', type=float, default=1.0, help='scales the colour around its grey')
    p.add_argument('--value', type=float, default=1.0, help='scales the brightness (sRGB)')
    p.add_argument('--tags', default='autumn')
    p.add_argument('--title', default=None)
    a = p.parse_args()

    albedo = seamless(read_rgb(a.image))
    grey = albedo @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    albedo = np.clip((grey[..., None] + (albedo - grey[..., None]) * a.saturation) * a.value, 0.0, 1.0)
    lum = to_linear(albedo) @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    height = blur(lum, a.blur)
    height = (height - height.min()) / max(1e-6, height.max() - height.min())
    dx = (np.roll(height, -1, axis=1) - np.roll(height, 1, axis=1)) * a.normal
    dz = (np.roll(height, -1, axis=0) - np.roll(height, 1, axis=0)) * a.normal
    n = np.stack([-dx, -dz, np.ones_like(height)], axis=-1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)

    tags = ['lookdev', 'imagegen'] + [t for t in a.tags.split(',') if t]
    title = a.title or a.name.replace('-', ' ')
    license_ = 'Original ImageGen terrain texture for Under the Canopy (gpt-image-2), made seamless and packed by tools/terrain/pack_ground.py.'
    base = os.path.join(ROOT, 'base', 'assets', 'terrain')
    adir, ndir = os.path.join(base, a.name), os.path.join(base, a.name + '-normal')
    for d in (adir, ndir):
        os.makedirs(d, exist_ok=True)

    rough = np.full((SIZE, SIZE, 1), a.roughness, dtype=np.float32)
    with open(os.path.join(adir, 'data.bin'), 'wb') as f:
        f.write(pack(np.concatenate([albedo, rough], axis=-1)))
    write_png(os.path.join(adir, 'albedo.png'), albedo)
    shutil.copyfile(a.image, os.path.join(adir, 'source.png'))
    asset_json(os.path.join(adir, 'asset.json'), 'asset.terrain.' + a.name, title + ' · albedo/roughness',
               tags, [('albedo', 'png'), ('data', 'bin'), ('source', 'png')], license_)

    with open(os.path.join(ndir, 'data.bin'), 'wb') as f:
        f.write(pack(np.concatenate([n * 0.5 + 0.5, height[:, :, None]], axis=-1)))
    asset_json(os.path.join(ndir, 'asset.json'), 'asset.terrain.' + a.name + '-normal', title + ' · normal/height',
               tags, [('data', 'bin')], license_)
    print('packed', adir, ndir)


if __name__ == '__main__':
    main()
