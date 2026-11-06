"""Fire flipbooks from a small 2D fluid simulation (buoyant, swirling hot gas): the
flames of skill canopy-effect-design. Each is an image asset `asset.effects.fx.<name>`
written into a package folder (the lab, `target/wc3-lab/base`, until Jiri agrees the
look). Run with Blender's Python (numpy and OpenImageIO):

    "$PY" -I tools/effects/fire_textures.py <package dir> [name ...]

- `fire-loop`: a whole fire burning from a source at the foot, 8×8 frames that loop
  seamlessly; the body of a torch or a patch of burning ground.
- `fire-smoke`: a puff of smoke rolling up and thinning over 8×8 frames, its coverage
  in alpha and lit from above.
- `ground-scorch`: burnt ground under a fire, dark char with a ragged soft edge, its
  coverage in alpha (one image).

Flames carry their colour (a temperature ramp: white-yellow core, orange, deep red
edges) because a single tint over a grey flame looks flat; layers multiply it by a near
white colour over life and push its brightness.
"""
import json
import math
import os
import sys

import numpy as np
import OpenImageIO as oiio

# Pixels per frame: 8 × 8 frames make a 1024 px sheet, which fits the base pack's effect
# image budget and holds up at game zoom.
FRAME = 128


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


class Noise:
    """Smooth value noise tiling in x, scrolling in time."""

    def __init__(self, rng, cells=8):
        self.grid = rng.random((cells + 1, cells + 1, 64))
        self.cells = cells

    def __call__(self, x, y, t):
        c = self.cells
        x = (x % 1.0) * c
        y = (y % 1.0) * c
        t = (t % 1.0) * 63
        x0, y0, t0 = np.floor(x).astype(int), np.floor(y).astype(int), int(t)
        fx, fy, ft = x - x0, y - y0, t - t0
        fx, fy = fx * fx * (3 - 2 * fx), fy * fy * (3 - 2 * fy)
        g = self.grid

        def at(tt):
            a = g[y0 % c, x0 % c, tt % 64]
            b = g[y0 % c, (x0 + 1) % c, tt % 64]
            d = g[(y0 + 1) % c, x0 % c, tt % 64]
            e = g[(y0 + 1) % c, (x0 + 1) % c, tt % 64]
            return (a * (1 - fx) + b * fx) * (1 - fy) + (d * (1 - fx) + e * fx) * fy

        return at(t0) * (1 - ft) + at(t0 + 1) * ft


def sample(f, x, y):
    """Bilinear sample of `f` (rows y, columns x) at grid coordinates, clamped."""
    h, w = f.shape
    x = np.clip(x, 0, w - 1.001)
    y = np.clip(y, 0, h - 1.001)
    x0, y0 = x.astype(int), y.astype(int)
    fx, fy = x - x0, y - y0
    return ((f[y0, x0] * (1 - fx) + f[y0, x0 + 1] * fx) * (1 - fy)
            + (f[y0 + 1, x0] * (1 - fx) + f[y0 + 1, x0 + 1] * fx) * fy)


class Fluid:
    """Stable fluids on a W × H grid (row 0 at the bottom): velocity, temperature and smoke."""

    def __init__(self, w, h, seed):
        self.w, self.h = w, h
        self.u = np.zeros((h, w))
        self.v = np.zeros((h, w))
        self.temp = np.zeros((h, w))
        self.smoke = np.zeros((h, w))
        self.ys, self.xs = np.mgrid[0:h, 0:w].astype(float)
        self.rng = np.random.default_rng(seed)
        self.noise = Noise(self.rng, 6)
        self.time = 0.0

    def advect(self, f, dt):
        return sample(f, self.xs - self.u * dt, self.ys - self.v * dt)

    def advect_sharp(self, f):
        """BFECC advection: less smearing, so flames keep crisp tongues."""
        there = self.advect(f, 1.0)
        back = sample(there, self.xs + self.u, self.ys + self.v)
        out = self.advect(f + (f - back) * 0.5, 1.0)
        # Never hotter than the plain step's neighbourhood: no stray hot specks.
        top = there.copy()
        for dy in (-1, 0, 1):
            for dx in (-1, 0, 1):
                top = np.maximum(top, np.roll(np.roll(there, dy, 0), dx, 1))
        return np.clip(out, 0.0, top)

    def project(self, iterations=40):
        u, v = self.u, self.v
        div = np.zeros_like(u)
        div[1:-1, 1:-1] = 0.5 * (u[1:-1, 2:] - u[1:-1, :-2] + v[2:, 1:-1] - v[:-2, 1:-1])
        p = np.zeros_like(u)
        for _ in range(iterations):
            p[1:-1, 1:-1] = (p[1:-1, 2:] + p[1:-1, :-2] + p[2:, 1:-1] + p[:-2, 1:-1] - div[1:-1, 1:-1]) / 4
            p[:, 0], p[:, -1], p[0, :] = p[:, 1], p[:, -2], p[1, :]
            p[-1, :] = 0  # open top
        u[1:-1, 1:-1] -= 0.5 * (p[1:-1, 2:] - p[1:-1, :-2])
        v[1:-1, 1:-1] -= 0.5 * (p[2:, 1:-1] - p[:-2, 1:-1])
        u[:, 0] = u[:, -1] = 0
        v[0, :] = np.maximum(v[0, :], 0)

    def vorticity(self, strength, dt):
        u, v = self.u, self.v
        w = np.zeros_like(u)
        w[1:-1, 1:-1] = 0.5 * (v[1:-1, 2:] - v[1:-1, :-2] - u[2:, 1:-1] + u[:-2, 1:-1])
        a = np.abs(w)
        gx = np.zeros_like(u)
        gy = np.zeros_like(u)
        gx[1:-1, 1:-1] = 0.5 * (a[1:-1, 2:] - a[1:-1, :-2])
        gy[1:-1, 1:-1] = 0.5 * (a[2:, 1:-1] - a[:-2, 1:-1])
        n = np.hypot(gx, gy) + 1e-5
        u += dt * strength * (gy / n) * w
        v += dt * strength * (-gx / n) * w

    def step(self, source, lift=0.2, cool=0.06, curl=0.9, wobble=0.12, fade=0.01):
        """One step (velocities in cells a step): `source` (H × W, 0–1) burns; hot gas
        rises by `lift`, cools by `cool` (more when hotter), swirls by `curl`."""
        self.time += 1.0
        t = self.time
        self.temp = np.maximum(self.temp, source)
        self.smoke = np.maximum(self.smoke, source)
        x = self.xs / self.w
        y = self.ys / self.h
        # Buoyancy, a wandering sideways push and the swirl that tears flames into licks.
        self.v += lift * self.temp
        self.u += wobble * (self.noise(x * 0.8, y * 0.6 - t * 0.004, t * 0.003) - 0.5) * 2 * (self.temp + 0.05)
        self.vorticity(curl, 1.0)
        # Velocity eases out where it is cold (air settles).
        self.u *= 0.985
        self.v *= 0.985
        self.project()
        self.u, self.v = self.advect(self.u, 1.0), self.advect(self.v, 1.0)
        self.project()
        self.temp = self.advect_sharp(self.temp)
        self.smoke = self.advect(self.smoke, 1.0)
        # Hot gas cools (radiates, more when hotter); smoke thins slowly.
        self.temp *= np.exp(-cool * (0.5 + self.temp))
        self.smoke *= np.exp(-fade)


def ramp(t):
    """Temperature (0–1) to flame colour (linear RGB): deep red edges, orange, yellow, a
    white-yellow core."""
    keys = np.array([
        [0.00, 0.00, 0.00, 0.00],
        [0.18, 0.35, 0.03, 0.00],
        [0.40, 0.95, 0.22, 0.02],
        [0.62, 1.00, 0.52, 0.10],
        [0.82, 1.00, 0.80, 0.35],
        [1.00, 1.00, 0.95, 0.75],
    ])
    out = np.zeros(t.shape + (3,))
    for c in range(3):
        out[..., c] = np.interp(t, keys[:, 0], keys[:, c + 1])
    return out


def resize(img, n):
    """`img` (h × w [× c]) to n × n, bilinear."""
    h, w = img.shape[:2]
    ys, xs = np.mgrid[0:n, 0:n].astype(float)
    x = (xs + 0.5) * w / n - 0.5
    y = (ys + 0.5) * h / n - 0.5
    if img.ndim == 2:
        return sample(img, x, y)
    return np.stack([sample(img[..., c], x, y) for c in range(img.shape[2])], axis=-1)


def edge_fade(n, start=0.8):
    ys, xs = np.mgrid[0:n, 0:n].astype(float)
    x = (xs + 0.5) / n * 2 - 1
    y = (ys + 0.5) / n * 2 - 1
    return (1 - smooth(start, 0.98, np.abs(x))) * (1 - smooth(start, 0.98, np.abs(y)))


def flame_pixels(temp, n, glow=0.55, foot=0.1):
    """A frame of flame (n × n RGBA, row 0 at the top) from a temperature field (row 0 at
    the bottom): the flame's colour by temperature, its brightness as the hot gas, and a
    soft glow around it."""
    temp = resize(temp[::-1], n)
    t = np.clip(temp, 0.0, 1.0)
    body = smooth(0.16, 0.6, t) ** 1.3
    colour = ramp(np.clip(0.28 + t * 1.05, 0, 1)) * body[..., None]
    # A faint glow so the edges never cut hard (bloom does the rest).
    halo = resize(resize(t, n // 8), n)
    colour += ramp(np.full_like(t, 0.3)) * (glow * 0.25 * smooth(0.02, 0.5, halo))[..., None]
    colour *= edge_fade(n)[..., None]
    if foot > 0:
        # The fuel itself is not drawn: the flame's foot fades in gradually above it, so
        # sprites standing in each other never show a straight lower edge.
        ys = (np.arange(n)[:, None] + 0.5) / n
        colour *= (smooth(1.0 - foot * 0.5, 1.0 - foot * 2.0, ys) ** 1.6)[..., None]
    return np.concatenate([colour, np.ones((n, n, 1))], axis=-1)


def fire_loop(seed=3, frames=64):
    """A fire burning from a bed at the foot, looping over `frames`."""
    w = h = 128
    fluid = Fluid(w, h, seed)
    noise = Noise(np.random.default_rng(seed + 1), 5)
    x = fluid.xs / w
    y = fluid.ys / h
    def source(t):
        # A bed of fuel at the foot, burning unevenly: hot spots wander along it.
        bed = np.exp(-((x - 0.5) / 0.2) ** 4 - ((y - 0.06) / 0.03) ** 2) * 0.85
        spots = noise(x * 2.5 - t * 0.003, 0.0, t * 0.05)
        pulse = noise(x * 1.2 + 0.37, 0.5, t * 0.11)
        return np.clip(bed * smooth(0.25, 0.75, spots) * (0.6 + 0.8 * pulse) * 1.2, 0, 1)

    def run(steps):
        for _ in range(steps):
            fluid.step(source(fluid.time), cool=0.032, lift=0.18)

    run(160)
    captured = []
    for _ in range(frames * 2):
        run(2)
        captured.append(fluid.temp.copy())
    out = []
    for i in range(frames):
        # A seamless loop: each frame crossfades from the second run into the first, so the
        # last frame flows into frame 0.
        k = i / frames
        temp = captured[frames + i] * (1 - k) + captured[i] * k
        # The fuel bed is flat and white-hot: hide it, so flames start at their tongues.
        temp = temp * smooth(0.07, 0.17, y)
        out.append(flame_pixels(temp, FRAME, foot=0.0))
    return out


def fire_smoke(seed=11, frames=64):
    """A puff of smoke rolling up and thinning: coverage in alpha, lit from above."""
    w = h = 112
    fluid = Fluid(w, h, seed)
    x = fluid.xs / w
    y = fluid.ys / h
    out = []
    total = 120
    every = total / frames
    step = 0
    for i in range(frames):
        while step < (i + 1) * every:
            on = 1.0 - smooth(14, 30, step)
            src = np.exp(-((x - 0.5) / 0.2) ** 2 - ((y - 0.24) / 0.13) ** 2) * on
            fluid.step(src * 0.7, lift=0.05, cool=0.004, curl=1.1, wobble=0.15, fade=0.004)
            step += 1
        d = resize(fluid.smoke[::-1], FRAME)
        k = i / (frames - 1)
        cover = np.clip(smooth(0.01, 0.3, d) * (1.0 - 0.7 * smooth(0.35, 1.0, k)), 0, 1)
        # Lit from above: thinner below its top edge, darker underneath.
        above = np.roll(d, 6, axis=0)
        light = np.clip(0.75 + 1.4 * (d - above), 0.35, 1.0)
        cover *= edge_fade(FRAME)
        rgba = np.stack([light, light, light, cover], axis=-1)
        out.append(rgba)
    return out


def ground_scorch(seed=5, n=512):
    """Char: near-black with ashy streaks, covering most at the centre, its edge ragged."""
    rng = np.random.default_rng(seed)
    noise = Noise(rng, 8)
    detail = Noise(rng, 24)
    ys, xs = np.mgrid[0:n, 0:n].astype(float)
    x = (xs + 0.5) / n * 2 - 1
    y = (ys + 0.5) / n * 2 - 1
    r = np.hypot(x, y)
    angle = (np.arctan2(y, x) / (2 * math.pi)) % 1.0
    ragged = 0.62 + 0.22 * noise(angle, 0.3, 0.0) + 0.08 * detail(angle * 3, 0.6, 0.0)
    cover = (1 - smooth(ragged * 0.55, ragged, r)) * (0.75 + 0.25 * detail(x * 0.5 + 0.5, y * 0.5 + 0.5, 0.2))
    ash = 0.06 + 0.1 * smooth(0.55, 0.8, detail(x * 0.5 + 0.5, y * 0.5 + 0.5, 0.7))
    rgb = np.stack([ash * 1.1, ash * 0.95, ash * 0.85], axis=-1)
    return np.concatenate([rgb, cover[..., None]], axis=-1)


def atlas(frames, cols, rows):
    n = frames[0].shape[0]
    sheet = np.zeros((n * rows, n * cols, 4))
    for k, f in enumerate(frames):
        r, c = divmod(k, cols)
        sheet[r * n:(r + 1) * n, c * n:(c + 1) * n] = f
    return sheet


def write(package, name, title, sheet, flipbook=True):
    folder = os.path.join(package, 'assets', 'effects', 'fx', name)
    os.makedirs(folder, exist_ok=True)
    rgba = np.clip(sheet, 0, 1).astype(np.float32)
    h, w = rgba.shape[:2]
    path = os.path.join(folder, 'image.png')
    out = oiio.ImageOutput.create(path)
    spec = oiio.ImageSpec(w, h, 4, oiio.UINT8)
    # Our RGB is not premultiplied: without this OpenImageIO divides it by alpha on write.
    spec.attribute('oiio:UnassociatedAlpha', 1)
    out.open(path, spec)
    out.write_image((rgba * 255.0 + 0.5).astype(np.uint8))
    out.close()
    asset = {
        'id': 'asset.effects.fx.' + name,
        'name': 'FX · ' + title,
        'kind': 'texture',
        'tags': ['effect', 'fx'] + (['flipbook-8x8'] if flipbook else []),
        'resources': [{'role': 'image', 'index': 1, 'format': 'png'}],
        'license': 'Original for Under the Canopy, simulated by tools/effects/fire_textures.py.',
    }
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump(asset, f, indent=2, ensure_ascii=False)
        f.write('\n')


ITEMS = {
    'fire-loop': ('Fire loop 8×8', fire_loop),
    'fire-smoke': ('Fire smoke 8×8', fire_smoke),
    'ground-scorch': ('Ground scorch', ground_scorch),
}


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    package = sys.argv[1]
    only = set(sys.argv[2:])
    for name, (title, make) in ITEMS.items():
        if only and name not in only:
            continue
        made = make()
        if isinstance(made, list):
            write(package, name, title, atlas(made, 8, 8))
        else:
            write(package, name, title, made, flipbook=False)
        print('wrote', name)


if __name__ == '__main__':
    main()
