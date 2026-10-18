"""Builds a Warcraft III-style transition atlas ground from one seamless image: 16 corner
tiles (a cell's NW, NE, SW and SE corners covered or not) with drawn, ragged edges, plus
full-tile variants, packed as the renderer's atlas tiles (canopy-assets skill,
terrain-material; scene/src/terrain.rs `atlas_codes`, shaders/terrain.wgsl KIND_ATLAS).

Runs on Blender's bundled Python, which has numpy and OpenImageIO:

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \
        tools/terrain/build_atlas.py <seamless image> <asset name> [--title "…"] [--roughness 0.9]
        [--normal 4] [--edge 0.18] [--tags autumn]

How the tiles stay seamless:
- every tile is `TILE` pixels square and shows one cell of ground;
- each tile's content is a crop of the image whose border is cross-faded into one shared,
  self-tiling border tile, so any two tiles (full or transition) meet without a seam;
- a transition tile's coverage is the bilinear field of its four corners, offset by noise
  that is periodic over one tile, thresholded with a soft, ragged edge. Along an edge
  between two cells the field depends only on that edge's two corners and the noise is
  periodic, so neighbouring tiles agree.

Atlas layout (8 × 8 tiles of 128 px in a 1024² image): tiles 0–15 are the corner tiles in
mask order (bit 0 NW, 1 NE, 2 SW, 3 SE; tile 0 unused), tiles 16–63 full variants."""
import argparse, gzip, json, os, shutil
import numpy as np
import OpenImageIO as oiio

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
COLS = ROWS = 8
TILE = 128
SIZE = COLS * TILE


def read_rgb(path):
    buf = oiio.ImageBuf(path)
    return np.ascontiguousarray(buf.get_pixels(oiio.FLOAT)[:, :, :3], dtype=np.float32)


def periodic_noise(rng, size, octaves=((2, 0.55), (4, 0.3), (8, 0.15))):
    """Smooth noise periodic over `size` pixels: a sum of random Fourier waves."""
    y, x = np.mgrid[0:size, 0:size] / size
    out = np.zeros((size, size), np.float32)
    for freq, amp in octaves:
        # Many integer wave vectors around the ring |k| ≈ freq keep it periodic and even
        # in every direction.
        waves = 10
        for i in range(waves):
            angle = np.pi * i / waves + rng.uniform(0, np.pi / waves)
            kx, ky = int(round(freq * np.cos(angle))), int(round(freq * np.sin(angle)))
            if kx == 0 and ky == 0:
                kx = freq
            phase = rng.uniform(0, 2 * np.pi)
            out += amp * np.cos(2 * np.pi * (kx * x + ky * y) + phase) / np.sqrt(waves)
    return out


def edge_ramp(size, width):
    """0 at the tile's border, 1 inside, over `width` of the tile."""
    u = (np.arange(size) + 0.5) / size
    e = np.minimum(u, 1.0 - u)
    r = np.clip(e / width, 0.0, 1.0)
    r = r * r * (3 - 2 * r)
    return np.minimum(r[:, None], r[None, :])


def self_tiling(crop):
    """Cross-fades a crop with itself shifted by half, so it wraps."""
    n = crop.shape[0]
    rolled = np.roll(np.roll(crop, n // 2, axis=0), n // 2, axis=1)
    mask = edge_ramp(n, 0.3)[:, :, None]
    return crop * mask + rolled * (1.0 - mask)


def to_linear(c):
    return np.where(c <= 0.04045, c / 12.92, ((c + 0.055) / 1.055) ** 2.4)


def pack(rgba):
    return gzip.compress(np.clip(rgba * 255.0 + 0.5, 0, 255).astype(np.uint8).tobytes(), 9)


def write_png(path, data):
    data = np.clip(data * 255.0 + 0.5, 0, 255).astype(np.uint8)
    h, w = data.shape[:2]
    c = 1 if data.ndim == 2 else data.shape[2]
    out = oiio.ImageOutput.create(path)
    out.open(path, oiio.ImageSpec(w, h, c, oiio.UINT8))
    out.write_image(data)
    out.close()


def main():
    p = argparse.ArgumentParser()
    p.add_argument('image')
    p.add_argument('name')
    p.add_argument('--title', default=None)
    p.add_argument('--roughness', type=float, default=0.9)
    p.add_argument('--normal', type=float, default=4.0)
    p.add_argument('--edge', type=float, default=0.18, help='softness of transition edges')
    p.add_argument('--scale', type=float, default=0.35, help='image pixels → tile pixels')
    p.add_argument('--tone', type=float, default=1.0, help='brightness multiplier')
    p.add_argument('--tags', default='autumn')
    a = p.parse_args()
    rng = np.random.default_rng(7)

    src = read_rgb(a.image)
    # Resample so one tile shows the right amount of ground.
    h, w = src.shape[:2]
    small_w = max(TILE * 2, int(w * a.scale))
    buf = oiio.ImageBufAlgo.resize(oiio.ImageBuf(a.image), roi=oiio.ROI(0, small_w, 0, small_w, 0, 1, 0, 3))
    src = np.ascontiguousarray(buf.get_pixels(oiio.FLOAT)[:, :, :3], dtype=np.float32) * a.tone
    n = src.shape[0]

    def crop(i):
        x, y = rng.integers(0, n - TILE, 2)
        return src[y:y + TILE, x:x + TILE]

    border = self_tiling(crop(0))
    interior = edge_ramp(TILE, 0.22)[:, :, None]

    def content():
        return border * (1.0 - interior) + crop(0) * interior

    albedo = np.zeros((SIZE, SIZE, 3), np.float32)
    alpha = np.zeros((SIZE, SIZE), np.float32)
    noise = periodic_noise(rng, TILE)
    noise2 = periodic_noise(rng, TILE, ((6, 0.5), (12, 0.35), (24, 0.15)))
    # Fine speckle, periodic over a tile: per-texel random values, lightly smoothed.
    speckle = rng.random((TILE, TILE)).astype(np.float32)
    speckle = (speckle + np.roll(speckle, 1, 0) + np.roll(speckle, 1, 1)) / 3.0
    speckle = (speckle - speckle.min()) / (speckle.max() - speckle.min())
    u = (np.arange(TILE) + 0.5) / TILE
    gx, gy = np.meshgrid(u, u)
    for t in range(COLS * ROWS):
        cx, cy = (t % COLS) * TILE, (t // COLS) * TILE
        albedo[cy:cy + TILE, cx:cx + TILE] = content()
        if t >= 16:
            alpha[cy:cy + TILE, cx:cx + TILE] = 1.0
            continue
        nw, ne, sw, se = (float((t >> b) & 1) for b in range(4))
        field = (nw * (1 - gx) * (1 - gy) + ne * gx * (1 - gy) + sw * (1 - gx) * gy + se * gx * gy)
        # Ragged edge: large wobble plus fine debris speckle; a corner tile's coverage
        # still reaches exactly its covered corners.
        # The noise only moves the edge: it fades where the field is fully 0 or 1, so a
        # tile is empty away from its covered corners and full near them.
        middle = np.clip(4.0 * field * (1.0 - field), 0.0, 1.0) ** 0.6
        f = field + (0.55 * noise + 0.2 * noise2) * middle
        cover = np.clip((f - 0.5) / a.edge + 0.5, 0.0, 1.0)
        # Debris thins out as scattered specks rather than ending on a line: in the
        # transition, fine periodic speckle decides which texels keep their debris.
        edge_zone = np.clip(4.0 * cover * (1.0 - cover), 0.0, 1.0)
        dithered = np.clip((cover - 0.5 + (speckle - 0.5) * 1.1) * 3.0 + 0.5, 0.0, 1.0)
        alpha[cy:cy + TILE, cx:cx + TILE] = cover * (1.0 - edge_zone) + dithered * edge_zone

    lum = to_linear(albedo) @ np.array([0.2126, 0.7152, 0.0722], np.float32)
    # Height per tile; blur lightly inside each tile.
    height = lum.copy()
    for _ in range(2):
        height = (height + np.roll(height, 1, 0) + np.roll(height, -1, 0) + np.roll(height, 1, 1) + np.roll(height, -1, 1)) / 5.0
    height = (height - height.min()) / max(1e-6, height.max() - height.min())
    dx = (np.roll(height, -1, 1) - np.roll(height, 1, 1)) * a.normal
    dy = (np.roll(height, -1, 0) - np.roll(height, 1, 0)) * a.normal
    nrm = np.stack([-dx, -dy, np.ones_like(height)], -1)
    nrm /= np.linalg.norm(nrm, axis=-1, keepdims=True)

    folder = os.path.join(ROOT, 'base', 'assets', 'terrain', a.name)
    os.makedirs(folder, exist_ok=True)
    rough = np.full((SIZE, SIZE, 1), a.roughness, np.float32)
    with open(os.path.join(folder, 'data.bin'), 'wb') as f:
        f.write(pack(np.concatenate([albedo, rough], -1)))
    with open(os.path.join(folder, 'data_2.bin'), 'wb') as f:
        f.write(pack(np.concatenate([nrm * 0.5 + 0.5, alpha[:, :, None]], -1)))
    om = np.concatenate([np.ones((SIZE, SIZE, 1)), np.zeros((SIZE, SIZE, 2)), np.ones((SIZE, SIZE, 1))], -1)
    with open(os.path.join(folder, 'data_3.bin'), 'wb') as f:
        f.write(pack(om))
    write_png(os.path.join(folder, 'albedo.png'), np.concatenate([albedo, alpha[:, :, None]], -1))
    write_png(os.path.join(folder, 'normal.png'), nrm * 0.5 + 0.5)
    write_png(os.path.join(folder, 'occlusion.png'), np.ones((SIZE, SIZE)))
    write_png(os.path.join(folder, 'roughness.png'), np.full((SIZE, SIZE), a.roughness))
    write_png(os.path.join(folder, 'metalness.png'), np.zeros((SIZE, SIZE)))
    thumb = oiio.ImageBufAlgo.resize(oiio.ImageBuf(a.image), roi=oiio.ROI(0, 128, 0, 128, 0, 1, 0, 3))
    write_png(os.path.join(folder, 'image.png'), thumb.get_pixels(oiio.FLOAT)[:, :, :3])
    shutil.copyfile(a.image, os.path.join(folder, 'source.png'))

    res = lambda role, i, fmt, cs=None: {k: v for k, v in [('role', role), ('index', i), ('format', fmt), ('colorSpace', cs)] if v is not None}
    asset = {
        'id': 'asset.terrain.' + a.name,
        'name': a.title or a.name.replace('-', ' '),
        'kind': 'terrain-material',
        'tags': ['lookdev', 'imagegen', 'atlas'] + [t for t in a.tags.split(',') if t],
        'resources': [
            res('data', 1, 'bin'), res('data', 2, 'bin'), res('data', 3, 'bin'),
            res('albedo', 1, 'png', 'srgb'), res('normal', 1, 'png', 'none'),
            res('occlusion', 1, 'png', 'none'), res('roughness', 1, 'png', 'none'),
            res('metalness', 1, 'png', 'none'), res('image', 1, 'png', 'srgb'),
            res('source', 1, 'png'),
        ],
        'terrain': {
            'tileWorldSize': 4,
            'reflectance': 1,
            'shading': 'physical',
            'sourceIds': [],
            'atlas': {
                'columns': COLS,
                'rows': ROWS,
                'fullTiles': list(range(16, COLS * ROWS)),
                'corners': [None] + list(range(1, 16)),
            },
            'albedo': {'role': 'albedo', 'index': 1},
            'normal': {'role': 'normal', 'index': 1},
            'occlusion': {'role': 'occlusion', 'index': 1},
            'roughness': {'role': 'roughness', 'index': 1},
            'metalness': {'role': 'metalness', 'index': 1},
            'thumbnail': {'role': 'image', 'index': 1},
            'runtime': {
                'albedoRoughness': {'role': 'data', 'index': 1},
                'normalOpacity': {'role': 'data', 'index': 2},
                'occlusionMetalness': {'role': 'data', 'index': 3},
            },
        },
        'license': 'Original ImageGen terrain texture (gpt-image-2) for Under the Canopy, built into a corner-transition atlas by tools/terrain/build_atlas.py.',
    }
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump(asset, f, indent=2, ensure_ascii=False)
        f.write('\n')
    print('built', folder)


if __name__ == '__main__':
    main()
