"""Our own FX texture primitives for effect graphs (D113; skill canopy-effect-design,
reference/textures.md): greyscale glows, glints, streaks, beams, rings, swirls, embers, a
crack web, and noise-built flame and smoke flipbooks, each an image asset
`asset.effects.fx.<name>` with `image.png` (RGBA: brightness in RGB for additive layers,
coverage in alpha for alpha-blended ones).

Runs on Blender's bundled Python (numpy and OpenImageIO):

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" \\
        tools/effects/fx_textures.py <package dir> [name ...]

writes `<package dir>/assets/effects/fx/<name>/` (`base` for the game, or the lab's
`target/wc3-lab/base` while a look is being tried). Deterministic: the same seed every run.
"""
import json
import math
import os
import sys

import numpy as np
import OpenImageIO as oiio

TAU = math.tau


def grid(n):
    """Pixel centres in -1..1 (x right, y up) for an n² image."""
    c = (np.arange(n) + 0.5) / n * 2.0 - 1.0
    return np.meshgrid(c, -c)


def smooth(e0, e1, x):
    t = np.clip((x - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3.0 - 2.0 * t)


def periodic_noise(n, cells, rng):
    """Smooth value noise over an n² image, `cells` lattice cells across, tiling."""
    lattice = rng.random((cells, cells))
    u = np.arange(n) / n * cells
    i0 = np.floor(u).astype(int)
    f = u - i0
    f = f * f * (3 - 2 * f)
    i1 = (i0 + 1) % cells
    i0 %= cells
    a = lattice[np.ix_(i0, i0)]
    b = lattice[np.ix_(i0, i1)]
    c = lattice[np.ix_(i1, i0)]
    d = lattice[np.ix_(i1, i1)]
    fy, fx = f[:, None], f[None, :]
    return (a * (1 - fx) + b * fx) * (1 - fy) + (c * (1 - fx) + d * fx) * fy


def fbm(n, rng, base=4, octaves=5):
    out = np.zeros((n, n))
    amp, total = 1.0, 0.0
    for o in range(octaves):
        out += periodic_noise(n, base * 2 ** o, rng) * amp
        total += amp
        amp *= 0.5
    return out / total


def sample(img, x, y):
    """Bilinear lookup of `img` at -1..1 coordinates (wrapping)."""
    n = img.shape[0]
    u = (x * 0.5 + 0.5) * n - 0.5
    v = (-y * 0.5 + 0.5) * n - 0.5
    x0 = np.floor(u).astype(int)
    y0 = np.floor(v).astype(int)
    fx, fy = u - x0, v - y0
    x0 %= n
    y0 %= n
    x1, y1 = (x0 + 1) % n, (y0 + 1) % n
    return ((img[y0, x0] * (1 - fx) + img[y0, x1] * fx) * (1 - fy)
            + (img[y1, x0] * (1 - fx) + img[y1, x1] * fx) * fy)


def edge_fade(x, y, start=0.82):
    """1 inside, 0 at the image edge: nothing may touch the border."""
    return 1.0 - smooth(start, 1.0, np.maximum(np.abs(x), np.abs(y)))


# --- Glows and points -------------------------------------------------------------------

def soft_dot(n, width):
    x, y = grid(n)
    r = np.hypot(x, y)
    g = np.exp(-(r / width) ** 2 * 3.0) * (1.0 - smooth(0.85, 1.0, r))
    return g + 0.25 * np.exp(-(r / (width * 0.25)) ** 2 * 3.0) * (1 - smooth(0.85, 1, r))


def pin_dot(n):
    x, y = grid(n)
    r = np.hypot(x, y)
    return (np.exp(-(r / 0.07) ** 2) + 0.35 * np.exp(-(r / 0.45) ** 2 * 2)) * (1 - smooth(0.8, 1, r))


def soft_disc(n):
    x, y = grid(n)
    r = np.hypot(x, y)
    return 1.0 - smooth(0.35, 0.95, r)


def rays(x, y, count, sharpness, phase=0.0, jitter=None):
    a = np.arctan2(y, x) + phase
    v = np.abs(np.cos(a * count / 2.0)) ** sharpness
    if jitter is not None:
        v = v * jitter(a)
    return v


def star_glint(n, points=6):
    x, y = grid(n)
    r = np.hypot(x, y)
    spikes = rays(x, y, points, 220.0) * np.exp(-r * 2.6)
    core = np.exp(-(r / 0.06) ** 2) + 0.3 * np.exp(-(r / 0.3) ** 2 * 2)
    return np.clip(core + spikes * 0.9, 0, None) * (1 - smooth(0.85, 1, r))


def cross_glint(n):
    x, y = grid(n)
    r = np.hypot(x, y)
    arm = np.exp(-(np.minimum(np.abs(x), np.abs(y)) / 0.012) ** 2) * np.exp(-r * 2.2)
    core = np.exp(-(r / 0.05) ** 2)
    return (arm + core) * (1 - smooth(0.85, 1, r))


def burst_flash(n, rng, streak=False):
    x, y = grid(n)
    r = np.hypot(x, y)
    a = np.arctan2(y, x)
    lengths = rng.uniform(0.45, 1.0, 23)
    idx = ((a + math.pi) / TAU * 23).astype(int) % 23
    frac = (a + math.pi) / TAU * 23 - np.floor((a + math.pi) / TAU * 23)
    reach = lengths[idx] * (1 - frac) + lengths[(idx + 1) % 23] * frac
    body = np.exp(-(r / (reach * 0.55)) ** 2 * 2.5)
    spikes = rays(x, y, 14, 40.0) * np.exp(-(r / reach) * 3)
    out = body * 0.9 + spikes * 0.5 + np.exp(-(r / 0.12) ** 2)
    if streak:
        out += np.exp(-(y / 0.025) ** 2) * np.exp(-(np.abs(x) / 0.7) ** 2) * 0.8
    return out * (1 - smooth(0.82, 1, r))


def line_flare(n):
    x, y = grid(n)
    return np.exp(-(y / 0.03) ** 2) * np.exp(-(np.abs(x) / 0.55) ** 2) * edge_fade(x, y)


def comet(n):
    """Head at the top, tail fading down: stretched along an axis, the head leads."""
    x, y = grid(n)
    head = np.exp(-((x / 0.32) ** 2 + ((y - 0.55) / 0.32) ** 2) * 2.5)
    t = np.clip((0.75 - y) / 1.6, 0, 1)
    width = 0.28 * (1 - t) + 0.04
    tail = np.exp(-(x / width) ** 2 * 2) * (1 - t) ** 1.5 * (y < 0.75)
    return np.maximum(head, tail * 0.8) * edge_fade(x, y, 0.88)


def soft_beam(n, width):
    """Bright at the bottom centre, fading up, soft sides: a pillar along an axis."""
    x, y = grid(n)
    t = (y + 1) / 2
    across = np.exp(-(x / (width * (1 - 0.4 * t))) ** 2 * 2)
    along = (1 - t) ** 1.2 * smooth(-1.0, -0.85, y)
    return across * along * edge_fade(x, y, 0.9)


def streak_cone(n):
    x, y = grid(n)
    t = (y + 1) / 2
    return np.exp(-(x / (0.05 + 0.3 * t)) ** 2 * 2) * t ** 1.5 * (1 - smooth(0.8, 1.0, y)) * edge_fade(x, y, 0.9)


# --- Rings and swirls -------------------------------------------------------------------

def ring(n, radius, inner, outer):
    x, y = grid(n)
    r = np.hypot(x, y)
    d = r - radius
    return np.where(d < 0, np.exp(-(d / inner) ** 2), np.exp(-(d / outer) ** 2)) * (1 - smooth(0.9, 1, r))


def ring_hard(n):
    x, y = grid(n)
    r = np.hypot(x, y)
    inside = smooth(0.62, 0.66, r)
    return inside * np.exp(-((r - 0.66) / 0.18) ** 2) * (1 - smooth(0.9, 1, r))


def ring_shock(n, rng):
    x, y = grid(n)
    r = np.hypot(x, y)
    noise = fbm(n, rng, base=6)
    wobble = sample(noise, x * 0.5, y * 0.5) - 0.5
    d = r - 0.68 - wobble * 0.12
    body = np.exp(-(d / 0.14) ** 2) * (0.55 + 0.9 * noise)
    return np.clip(body, 0, None) * (1 - smooth(0.9, 1, r))


def ring_rays(n):
    x, y = grid(n)
    r = np.hypot(x, y)
    streaks = 0.45 + 0.55 * rays(x, y, 48, 6.0)
    return smooth(0.25, 0.65, r) * (1 - smooth(0.7, 0.98, r)) * streaks


def ring_wispy(n, rng):
    x, y = grid(n)
    r = np.hypot(x, y)
    a = np.arctan2(y, x)
    noise = fbm(n, rng, base=5)
    broken = smooth(0.35, 0.7, sample(noise, np.cos(a) * 0.6, np.sin(a) * 0.6))
    return np.exp(-((r - 0.7) / 0.07) ** 2) * broken * (1 - smooth(0.9, 1, r))


def swirl(n, arms=3, blur=False):
    x, y = grid(n)
    r = np.hypot(x, y)
    a = np.arctan2(y, x)
    phase = a * arms + np.log(np.maximum(r, 1e-3)) * 5.0
    band = (0.5 + 0.5 * np.cos(phase)) ** (2.0 if blur else 6.0)
    return band * smooth(0.08, 0.35, r) * (1 - smooth(0.6, 0.95, r))


# --- Atlases and flipbooks --------------------------------------------------------------

def atlas(frames, cols, rows):
    n = frames[0].shape[0]
    out = np.zeros((rows * n, cols * n))
    for i, f in enumerate(frames):
        r, c = divmod(i, cols)
        out[r * n:(r + 1) * n, c * n:(c + 1) * n] = f
    return out


def ember(n, rng):
    x, y = grid(n)
    a, b = rng.uniform(0.25, 0.5), rng.uniform(0.15, 0.4)
    rot = rng.uniform(0, TAU)
    xr = x * math.cos(rot) - y * math.sin(rot)
    yr = x * math.sin(rot) + y * math.cos(rot)
    blob = np.exp(-((xr / a) ** 2 + (yr / b) ** 2) * 2)
    bite = np.exp(-(((xr - a * 0.5) / (a * 0.8)) ** 2 + ((yr - b * 0.3) / (b * 0.8)) ** 2) * 2)
    return np.clip(blob - bite * rng.uniform(0.6, 1.0), 0, None) * (1 - smooth(0.8, 1, np.hypot(x, y)))


def flame(n, rng, frames=64):
    """A lick of flame that forms, curls up and dissipates; additive."""
    base = fbm(256, rng, base=3)
    detail = fbm(256, rng, base=6, octaves=4)
    out = []
    for i in range(frames):
        t = i / (frames - 1)
        x, y = grid(n)
        scroll = t * 1.6
        # Sideways curl, stronger higher up and later on.
        curl = (sample(base, x * 0.5, y * 0.5 - scroll * 0.5) - 0.5) * (0.35 + 0.7 * t) * smooth(-1.0, 0.8, y)
        xs = x + curl
        # The flame's foot rises a little as it burns out.
        ys = y + 0.7 - t * 0.35
        top = 1.1 + 0.4 * smooth(0.0, 0.4, t)
        width = 0.62 * np.clip(1.0 - np.maximum(ys, 0) / top, 0.0, 1.0) ** 0.8 + 0.02
        across = np.exp(-(xs / np.maximum(width, 0.03)) ** 2 * 1.4)
        along = np.exp(-(np.minimum(ys, 0.0) / 0.3) ** 2 * 2.0) * (1.0 - smooth(top * 0.55, top, ys))
        mask = across * along
        n2 = sample(detail, x * 0.8 + curl * 0.6, y * 0.8 - scroll)
        grow = smooth(0.0, 0.18, t)
        threshold = 0.2 + 0.55 * smooth(0.35, 1.0, t)
        density = mask * (0.35 + 1.1 * n2) * grow
        f = np.clip((density - threshold) / (1.0 - threshold), 0.0, 1.0) ** 0.75
        out.append(f * (1.0 - 0.45 * smooth(0.6, 1.0, t)) * edge_fade(x, y, 0.86))
    return out


def smoke(n, rng, frames=64):
    """A puff that swells and thins; coverage in alpha, lit from the upper left."""
    base = fbm(256, rng, base=3)
    detail = fbm(256, rng, base=7, octaves=4)
    covers, shades = [], []
    for i in range(frames):
        t = i / (frames - 1)
        x, y = grid(n)
        scale = 0.35 + 0.5 * smooth(0.0, 0.7, t)
        warp = (sample(base, x * 0.6 + t * 0.2, y * 0.6 - t * 0.3) - 0.5) * 0.45
        px, py = x + warp, y + warp * 0.6
        r = np.hypot(px, py) / scale
        body = np.exp(-r ** 2 * 1.8)
        n2 = sample(detail, x * 0.9 + t * 0.15, y * 0.9 - t * 0.3)
        threshold = 0.15 + 0.6 * smooth(0.25, 1.0, t)
        density = body * (0.45 + 0.9 * n2)
        cover = np.clip((density - threshold) / (1.0 - threshold) * 1.6, 0.0, 1.0) * edge_fade(x, y, 0.86)
        light = np.clip(0.7 + 0.3 * (-px + py) / scale * 0.6 + 0.25 * (n2 - 0.5), 0.3, 1.0)
        covers.append(cover)
        shades.append(light)
    return covers, shades


def crack_web(n, rng):
    """Radial cracks with forks, bright at the centre."""
    x, y = grid(n)
    field = np.zeros((n, n))
    for k in range(11):
        a = k / 11 * TAU + rng.uniform(-0.2, 0.2)
        px, py, w = 0.0, 0.0, 0.035
        steps = rng.integers(7, 11)
        for s in range(steps):
            a += rng.uniform(-0.45, 0.45)
            length = rng.uniform(0.07, 0.12)
            qx, qy = px + math.cos(a) * length, py + math.sin(a) * length
            # Distance to the segment p→q.
            dx, dy = qx - px, qy - py
            t = np.clip(((x - px) * dx + (y - py) * dy) / (dx * dx + dy * dy), 0, 1)
            d = np.hypot(x - (px + t * dx), y - (py + t * dy))
            field = np.maximum(field, np.exp(-(d / w) ** 2) * (1 - s / steps * 0.6))
            if rng.random() < 0.25:
                fa = a + rng.choice([-1, 1]) * rng.uniform(0.5, 0.9)
                fx, fy = qx + math.cos(fa) * length * 0.8, qy + math.sin(fa) * length * 0.8
                ex, ey = fx - qx, fy - qy
                t2 = np.clip(((x - qx) * ex + (y - qy) * ey) / (ex * ex + ey * ey), 0, 1)
                d2 = np.hypot(x - (qx + t2 * ex), y - (qy + t2 * ey))
                field = np.maximum(field, np.exp(-(d2 / (w * 0.7)) ** 2) * 0.7)
            px, py, w = qx, qy, w * 0.88
    r = np.hypot(x, y)
    return np.clip(field + 0.5 * np.exp(-(r / 0.18) ** 2), 0, None) * (1 - smooth(0.85, 1, r))


# --- Output -----------------------------------------------------------------------------

def normalise(img):
    return np.clip(img / max(1e-6, img.max()), 0.0, 1.0)


def write(package, name, title, grey, alpha=None, flipbook=None):
    folder = os.path.join(package, 'assets', 'effects', 'fx', name)
    os.makedirs(folder, exist_ok=True)
    grey = normalise(grey)
    # Additive textures carry their brightness in RGB with alpha 1 (additive layers are
    # weighted by alpha too); covering ones (smoke) their coverage in alpha.
    alpha = np.ones_like(grey) if alpha is None else np.clip(alpha, 0, 1)
    h, w = grey.shape
    rgba = np.stack([grey, grey, grey, alpha], axis=-1).astype(np.float32)
    out = oiio.ImageOutput.create(os.path.join(folder, 'image.png'))
    spec = oiio.ImageSpec(w, h, 4, oiio.UINT8)
    # Our RGB is not premultiplied: without this OpenImageIO divides it by alpha on write.
    spec.attribute('oiio:UnassociatedAlpha', 1)
    out.open(os.path.join(folder, 'image.png'), spec)
    out.write_image((rgba * 255.0 + 0.5).astype(np.uint8))
    out.close()
    asset = {
        'id': 'asset.effects.fx.' + name,
        'name': 'FX · ' + title,
        'kind': 'texture',
        'tags': ['effect', 'fx'] + (['flipbook-%dx%d' % flipbook] if flipbook else []),
        'resources': [{'role': 'image', 'index': 1, 'format': 'png'}],
        'license': 'Original for Under the Canopy, generated by tools/effects/fx_textures.py.',
    }
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump(asset, f, indent=2, ensure_ascii=False)
        f.write('\n')


def library():
    rng = lambda seed: np.random.default_rng(seed)  # noqa: E731
    items = {
        'soft-dot': ('Soft dot', lambda: soft_dot(256, 0.55)),
        'soft-dot-tight': ('Soft dot, tight', lambda: soft_dot(256, 0.3)),
        'soft-dot-wide': ('Soft dot, wide', lambda: soft_dot(256, 0.85)),
        'pin-dot': ('Pin dot with halo', lambda: pin_dot(256)),
        'soft-disc': ('Soft disc', lambda: soft_disc(256)),
        'star-glint': ('Star glint', lambda: star_glint(256)),
        'cross-glint': ('Cross glint', lambda: cross_glint(256)),
        'burst-flash': ('Burst flash', lambda: burst_flash(256, rng(3))),
        'burst-flash-streak': ('Burst flash with a streak', lambda: burst_flash(256, rng(4), streak=True)),
        'line-flare': ('Line flare', lambda: line_flare(256)),
        'comet': ('Comet streak', lambda: comet(256)),
        'soft-beam': ('Soft beam', lambda: soft_beam(256, 0.45)),
        'soft-beam-thin': ('Soft beam, thin', lambda: soft_beam(256, 0.2)),
        'streak-cone': ('Streak cone', lambda: streak_cone(256)),
        'ring-clean': ('Clean ring', lambda: ring(512, 0.72, 0.03, 0.03)),
        'ring-thick': ('Thick glowing ring', lambda: ring(512, 0.66, 0.12, 0.1)),
        'ring-hard': ('Ring with a hard inner edge', lambda: ring_hard(512)),
        'ring-shock': ('Noisy shockwave ring', lambda: ring_shock(512, rng(7))),
        'ring-rays': ('Radial streak ring', lambda: ring_rays(512)),
        'ring-wispy': ('Wispy broken ring', lambda: ring_wispy(512, rng(9))),
        'swirl': ('Spiral swirl', lambda: swirl(512)),
        'swirl-blur': ('Spiral swirl, blurred', lambda: swirl(512, 2, blur=True)),
        'embers': ('Embers 4×2', lambda: (atlas([ember(128, rng(20 + i)) for i in range(8)], 4, 2), None, (4, 2))),
        'crack-web': ('Ground crack web', lambda: crack_web(512, rng(11))),
        'flame': ('Flame lick 8×8', lambda: (atlas(flame(128, rng(13)), 8, 8), None, (8, 8))),
        'smoke': ('Smoke puff 8×8', 'smoke'),
    }
    return items, rng


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    package = sys.argv[1]
    only = set(sys.argv[2:])
    items, rng = library()
    for name, (title, make) in items.items():
        if only and name not in only:
            continue
        if make == 'smoke':
            covers, shades = smoke(128, rng(17))
            write(package, name, title, atlas(shades, 8, 8), atlas(covers, 8, 8), (8, 8))
        else:
            made = make()
            if isinstance(made, tuple):
                grey, alpha, flip = made
                write(package, name, title, grey, alpha, flip)
            else:
                write(package, name, title, made)
        print('wrote', name)


if __name__ == '__main__':
    main()
