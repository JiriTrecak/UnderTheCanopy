"""Writes the fall look-dev map (`base/maps/showcase/lookdev-fall.utcm`): a render-only
testbed that rebuilds the target frame `art/styles/target-fall-wide-map.jpg` in the engine
(docs/art-direction.md, D61).

The layout is traced on the target image in its pixel coordinates (2000 × 1125) and projected
onto the ground through the game camera (56° pitch, the game's field of view) at the framing
`FOCUS`, `ZOOM`, so `just shot --map lookdev-fall --camera 240,240,2.1` sees the same
composition:
- the open, calm soil (one C-shaped clearing with the main base, the corridor and the expansion);
- a shallow ford across it and a deep pool at the bottom right;
- the forest frame around it: leaf-litter ground, giant trunks and roots, mushrooms, ferns.

Runs on Blender's bundled Python (numpy):
    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" tools/maps/lookdev_fall.py
The package is rewritten from scratch on every run; seeded, so runs are identical."""
import json, math, os, shutil, struct, zlib
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'maps', 'showcase', 'lookdev-fall.utcm')
SIZE = 480
HALO = 16
SPAN = SIZE + 2 * HALO
SIDE = SPAN + 1

# ------------------------------------------------------------------ camera (crates/render/src/camera.rs)
FOCUS = (240.0, 240.0)
ZOOM = 2.1
W, H = 2000.0, 1125.0
PITCH = math.radians(56.0)
ASPECT = 16.0 / 9.0
FOV_Y = math.radians(70.0 / math.hypot(1.0, ASPECT))
DISTANCE = ZOOM * 128.0 / (2.0 * math.tan(FOV_Y / 2.0) * ASPECT)


def to_world(px, py):
    """Target-image pixel → ground point (x, z) the game camera sees there."""
    fx, fz = FOCUS
    eye = np.array([fx, math.sin(PITCH) * DISTANCE, fz + math.cos(PITCH) * DISTANCE])
    forward = np.array([0.0, -math.sin(PITCH), -math.cos(PITCH)])
    right = np.array([1.0, 0.0, 0.0])
    up = np.cross(right, forward)
    t = math.tan(FOV_Y / 2.0)
    nx, ny = px / W * 2.0 - 1.0, 1.0 - py / H * 2.0
    d = forward + right * nx * t * ASPECT + up * ny * t
    p = eye + d * (-eye[1] / d[1])
    return (float(p[0]), float(p[2]))


def poly(points):
    return np.array([to_world(x, y) for x, y in points])

# ------------------------------------------------------------------ layout (target pixels)
OPEN = poly([
    (380, 120), (600, 150), (660, 280), (800, 320), (940, 280), (1000, 160), (1100, 110),
    (1450, 110), (1500, 230), (1560, 330), (1650, 420), (1680, 600), (1640, 760), (1560, 795),
    (1250, 805), (1050, 825), (900, 865), (850, 955), (800, 1060), (700, 1115), (400, 1115),
    (300, 1000), (260, 850), (245, 650), (230, 450), (260, 300), (300, 190),
])
FORD = poly([(1440, 320), (1380, 300), (1200, 330), (1050, 420), (900, 520), (760, 560),
             (700, 620), (720, 690), (830, 690), (950, 610), (1100, 520), (1250, 420), (1400, 370)])
POOL = poly([(1050, 1150), (1100, 960), (1250, 930), (1450, 860), (1600, 815), (1800, 795),
             (2100, 795), (2100, 1200)])
# Giant pieces: (scenery id, pixel x, pixel y, scale, yaw degrees)
GIANTS = [
    ('scenery.lookdev-trunk', 800, 175, 0.85, 15),
    ('scenery.lookdev-trunk-mushrooms', 1720, 150, 0.85, 200),
    ('scenery.lookdev-trunk', 2150, 760, 0.8, 300),
    ('scenery.lookdev-trunk', -150, 380, 0.75, 110),
    ('scenery.lookdev-stump', 200, 740, 1.5, 40),
    ('scenery.lookdev-puffball', 1015, 1000, 1.6, 0),
    ('scenery.lookdev-root-arch', 1370, 860, 1.1, 10),
]

# ------------------------------------------------------------------ fields

def signed_distance(px, pz, polygon):
    """Signed distance from points to a polygon's outline: negative inside."""
    x, z = px[..., None], pz[..., None]
    a, b = polygon, np.roll(polygon, -1, axis=0)
    ax, az, bx, bz = a[:, 0], a[:, 1], b[:, 0], b[:, 1]
    dx, dz = bx - ax, bz - az
    t = np.clip(((x - ax) * dx + (z - az) * dz) / (dx * dx + dz * dz), 0.0, 1.0)
    dist = np.sqrt((x - ax - dx * t) ** 2 + (z - az - dz * t) ** 2).min(axis=-1)
    cond = ((az <= z) & (bz > z)) | ((bz <= z) & (az > z))
    xint = ax + (z - az) / np.where(bz - az == 0, 1e-9, bz - az) * (bx - ax)
    inside = (cond & (x < xint)).sum(axis=-1) % 2 == 1
    return np.where(inside, -dist, dist)


def value_noise(x, z, scale, seed):
    lattice = np.random.default_rng(seed).random((64, 64))
    gx, gz = x / scale, z / scale
    ix, iz = np.floor(gx).astype(int), np.floor(gz).astype(int)
    fx, fz = gx - ix, gz - iz
    sx, sz = fx * fx * (3 - 2 * fx), fz * fz * (3 - 2 * fz)
    at = lambda i, k: lattice[i % 64, k % 64]
    top = at(ix, iz) + (at(ix + 1, iz) - at(ix, iz)) * sx
    bottom = at(ix, iz + 1) + (at(ix + 1, iz + 1) - at(ix, iz + 1)) * sx
    return top + (bottom - top) * sz


def fbm(x, z, scale, seed, octaves=3):
    total, amp, norm = 0.0, 1.0, 0.0
    for o in range(octaves):
        total = total + value_noise(x, z, scale / 2 ** o, seed + o) * amp
        norm += amp
        amp *= 0.5
    return total / norm


def smoothstep(e0, e1, v):
    t = np.clip((v - e0) / (e1 - e0), 0.0, 1.0)
    return t * t * (3 - 2 * t)


def fields(x, z):
    """Openness (1 on playable soil), ford and pool masks at points."""
    wobble = (fbm(x, z, 14, 11) - 0.5) * 5.0
    open_ = 1.0 - smoothstep(-3.0, 2.0, signed_distance(x, z, OPEN) + wobble)
    ford = 1.0 - smoothstep(-5.0, 2.0, signed_distance(x, z, FORD) + (fbm(x, z, 9, 23) - 0.5) * 5.0)
    pool = 1.0 - smoothstep(-14.0, 1.0, signed_distance(x, z, POOL) + (fbm(x, z, 16, 31) - 0.5) * 6.0)
    return open_, ford * open_, pool

# ------------------------------------------------------------------ grids

def write_grid(path, data, code, side):
    raw = data.tobytes()
    size = 4 if code in (1, 3) else 1
    if size > 1:
        raw = b''.join(raw[plane::size] for plane in range(size))
    header = b'UTCG' + bytes([1, code, 1, 1 if size > 1 else 0]) + struct.pack('<II', side, side)
    with open(path, 'wb') as f:
        f.write(header + zlib.compress(raw, 9))


def build_grids():
    v = np.arange(SIDE, dtype=np.float64) - HALO
    x, z = np.meshgrid(v, v)
    open_, ford, pool = fields(x, z)
    frame = 1.0 - open_
    heights = frame * 0.45 + (fbm(x, z, 30, 5) - 0.5) * 0.5 * frame
    heights -= ford * 0.6
    # Amber craters: a bowl with a low raised rim of thrown-out soil.
    for px, py in AMBER:
        cx, cz = to_world(px, py)
        r = np.sqrt((x - cx) ** 2 + (z - cz) ** 2)
        heights -= 2.6 * (1.0 - smoothstep(2.0, 9.0, r))
        heights += 1.1 * np.exp(-((r - 9.8) / 1.8) ** 2)
    heights -= pool * 3.5
    water = np.full_like(heights, -17.0)
    water[pool > 0.01] = -0.4
    water[(ford > 0.02) & (pool <= 0.02)] = -0.16
    edge = fbm(x, z, 6, 41)
    litter = smoothstep(0.1, 0.75, frame + (edge - 0.5) * 0.7) * (1.0 - pool)
    # Water flow per cell: still everywhere; the ford takes its own clear profile (slot 1)
    # and drifts gently along its length.
    c = np.arange(SPAN, dtype=np.float64) - HALO + 0.5
    cx, cz = np.meshgrid(c, c)
    _, cford, cpool = fields(cx, cz)
    rim = signed_distance(x, z, OPEN)
    sparse = smoothstep(-14.0, -2.0, rim + (fbm(x, z, 7, 53) - 0.5) * 8.0) * (1.0 - pool) * (1.0 - ford)
    # Wet, darker soil in a ragged band around the ford.
    ford_edge = signed_distance(x, z, FORD)
    wet = 0.25 * (1.0 - smoothstep(-0.5, 3.0, ford_edge + (fbm(x, z, 5, 61) - 0.5) * 4.0)) * open_
    flow = np.zeros((SPAN, SPAN, 4), dtype=np.uint8)
    flow[..., 0] = 128
    flow[..., 1] = 128
    in_ford = (cford > 0.02) & (cpool <= 0.02)
    flow[in_ford, 0] = 150
    flow[in_ford, 1] = 116
    flow[in_ford, 3] = 1
    # Frame shade: nothing on the playable ground, deepening into the frame, so the frame
    # sits below the clearing in value as in the target.
    shade = 0.7 * smoothstep(6.0, 32.0, rim + (fbm(x, z, 20, 71) - 0.5) * 10.0)
    return (heights.astype('<f4'), water.astype('<f4'), litter.astype('<f4'), np.zeros_like(heights, dtype='<f4'),
            flow, sparse.astype('<f4'), wet.astype('<f4'), shade.astype('<f4'))

# ------------------------------------------------------------------ scenery

# (scenery id, weight, scale range, zone): 'edge' only at the clearing's rim, 'deep' only
# further into the frame, 'any' both.
FRAME = [
    ('scenery.lookdev-leaf-oak', 10, (1.3, 2.0), 'any'),
    ('scenery.lookdev-leaf-maple', 9, (1.3, 2.0), 'any'),
    ('scenery.lookdev-leaf-beech', 8, (1.2, 1.8), 'any'),
    ('scenery.lookdev-fern', 12, (1.0, 1.7), 'any'),
    ('scenery.lookdev-bracken', 1.2, (0.6, 1.1), 'any'),
    ('scenery.lookdev-bracken', 5, (0.6, 1.2), 'edge'),
    ('scenery.lookdev-leaf-big', 1.0, (1.0, 2.2), 'any'),
    ('scenery.lookdev-leaf-big-oak', 1.0, (1.0, 2.2), 'any'),
    ('scenery.lookdev-leaf-big', 1.2, (1.1, 2.0), 'edge'),
    ('scenery.lookdev-leaf-big-oak', 1.2, (1.1, 2.0), 'edge'),
    ('scenery.lookdev-mushrooms', 0.6, (0.7, 1.2), 'edge'),
    ('scenery.lookdev-acorn', 1.2, (0.8, 1.2), 'edge'),
    ('scenery.lookdev-mushrooms', 0.3, (0.6, 1.1), 'any'),
    ('scenery.lookdev-acorn', 0.5, (0.7, 1.1), 'any'),
    ('scenery.lookdev-puffball', 0.3, (0.5, 0.9), 'deep'),
]
LEAVES = ['scenery.lookdev-leaf-big', 'scenery.lookdev-leaf-big-oak']
# Amber deposits (target pixels); each sits in a crater.
AMBER = [(430, 370), (1465, 685)]
# Mushroom clusters around the giants' roots, as in the target: (pixel x, pixel y, count).
CLUSTERS = [(760, 225, 9), (1640, 270, 10), (1760, 300, 6), (250, 290, 6), (1720, 680, 7), (140, 960, 7), (800, 1010, 5)]


def build_scenery():
    rng = np.random.default_rng(20261018)
    objects = []

    def add(asset, x, z, scale, yaw, mode='terrain', elevation=0.0):
        objects.append({
            'asset': asset, 'elevation': round(float(elevation), 3), 'heightMode': mode,
            'id': 'lookdev.%d' % len(objects), 'locked': False, 'scale': round(float(scale), 4),
            'visible': True, 'x': round(float(x), 2), 'yaw': round(float(yaw), 4), 'z': round(float(z), 2),
        })

    roots = ['scenery.lookdev-root-a', 'scenery.lookdev-root-b', 'scenery.lookdev-root-c', 'scenery.lookdev-root-d']
    for asset, px, py, scale, yaw in GIANTS:
        x, z = to_world(px, py)
        add(asset, x, z, scale, math.radians(yaw))
        if 'trunk' in asset:
            # Surface roots snake out from the trunk's base in every direction. A root
            # model heads along -Z, so its yaw is atan2(-dx, -dz) for an outward (dx, dz).
            count = 8
            for k in range(count):
                angle = 2 * math.pi * k / count + rng.uniform(-0.3, 0.3)
                dx, dz = math.cos(angle), math.sin(angle)
                start = 11.0 * scale
                tip = (x + dx * (start + 40 * scale), z + dz * (start + 40 * scale))
                if signed_distance(np.array(tip[0]), np.array(tip[1]), OPEN) < 6.0:
                    continue  # roots stay in the frame, out of the playable ground
                add(roots[rng.integers(4)], x + dx * start, z + dz * start,
                    scale * rng.uniform(0.8, 1.2), math.atan2(-dx, -dz))
    # Amber crystals stand in their craters (render-only: no mine entity, whose foundation
    # would flatten the crater).
    for i, (px, py) in enumerate(AMBER):
        x, z = to_world(px, py)
        add('scenery.lookdev-amber', x, z, 1.45, math.radians(30 + 70 * i), 'absolute', -2.2)

    # Candidates on a jittered grid over the framed area plus a margin, so the instance
    # count follows what the camera can see.
    xs = np.arange(FOCUS[0] - 175, FOCUS[0] + 175, 2.8)
    zs = np.arange(FOCUS[1] - 150, FOCUS[1] + 140, 2.8)
    gx, gz = np.meshgrid(xs, zs)
    gx = gx + rng.uniform(-1.0, 1.0, gx.shape)
    gz = gz + rng.uniform(-1.0, 1.0, gz.shape)
    open_, _, pool = fields(gx, gz)
    frame = 1.0 - open_
    density = smoothstep(0.25, 0.9, frame) * (0.35 + 0.45 * fbm(gx, gz, 14, 77))
    dist = signed_distance(gx, gz, OPEN)
    deep = smoothstep(4.0, 12.0, dist) > 0.5
    edge = (dist > -1.0) & (dist < 7.0)
    weights = np.array([w for _, w, _, _ in FRAME], dtype=float)
    rolls = rng.random(gx.shape)
    for idx in np.ndindex(gx.shape):
        if pool[idx] > 0.05 or rolls[idx] >= density[idx]:
            continue
        allowed = np.array([1.0 if zone == 'any' or (zone == 'deep' and deep[idx]) or (zone == 'edge' and edge[idx]) else 0.0
                            for _, _, _, zone in FRAME])
        w = weights * allowed
        i = rng.choice(len(FRAME), p=w / w.sum())
        asset, _, (s0, s1), _ = FRAME[i]
        add(asset, gx[idx], gz[idx], rng.uniform(s0, s1), rng.uniform(0, 2 * math.pi))

    # A fine layer of small fallen leaves under everything in the frame, so the floor has
    # depth: overlapping, curled, some tipped on an edge, each casting a small shadow.
    xs = np.arange(FOCUS[0] - 175, FOCUS[0] + 175, 1.25)
    zs = np.arange(FOCUS[1] - 150, FOCUS[1] + 140, 1.25)
    gx, gz = np.meshgrid(xs, zs)
    gx = gx + rng.uniform(-0.6, 0.6, gx.shape)
    gz = gz + rng.uniform(-0.6, 0.6, gz.shape)
    open_, _, pool_ = fields(gx, gz)
    rim = signed_distance(gx, gz, OPEN)
    # Densest along the clearing's rim, where the eye lands; thinner deep in the frame,
    # where the litter texture and the larger clusters carry it.
    fine = smoothstep(0.15, 0.75, 1.0 - open_) * (0.15 + 0.5 * (1.0 - smoothstep(6.0, 22.0, rim)))
    rolls = rng.random(gx.shape)
    small = ['scenery.lookdev-litter-oak', 'scenery.lookdev-litter-maple', 'scenery.lookdev-litter-beech']
    for idx in np.ndindex(gx.shape):
        if pool_[idx] > 0.05 or rolls[idx] >= fine[idx]:
            continue
        r = rng.random()
        if r < 0.03:
            add('scenery.lookdev-acorn', gx[idx], gz[idx], rng.uniform(0.35, 0.6), rng.uniform(0, 2 * math.pi))
        elif r < 0.05:
            add('scenery.lookdev-mushrooms', gx[idx], gz[idx], rng.uniform(0.3, 0.55), rng.uniform(0, 2 * math.pi))
        else:
            add(small[rng.integers(3)], gx[idx], gz[idx], rng.uniform(0.8, 1.6), rng.uniform(0, 2 * math.pi))

    # Fallen leaves floating on the deep pool and drifting in the ford.
    for _ in range(45):
        x, z = to_world(rng.uniform(1250, 2000), rng.uniform(880, 1125))
        if fields(np.array(x), np.array(z))[2] > 0.6:
            add(LEAVES[rng.integers(2)], x, z, rng.uniform(0.18, 0.32), rng.uniform(0, 2 * math.pi), 'absolute', -0.38)
    for _ in range(140):
        t = rng.uniform(0, 1)
        x, z = to_world(720 + 720 * t + rng.uniform(-80, 80), 650 - 320 * t + rng.uniform(-60, 60))
        if fields(np.array(x), np.array(z))[1] > 0.5:
            add(LEAVES[rng.integers(2)], x, z, rng.uniform(0.14, 0.3), rng.uniform(0, 2 * math.pi), 'absolute', -0.14)
    # Gnarled surface roots all through the frame, pointing away from the clearing, as the
    # giants' root networks that cover the target's forest floor.
    xs = np.arange(FOCUS[0] - 175, FOCUS[0] + 175, 13.0)
    zs = np.arange(FOCUS[1] - 150, FOCUS[1] + 140, 13.0)
    for zc in zs:
        for xc in xs:
            x, z = xc + rng.uniform(-5, 5), zc + rng.uniform(-5, 5)
            d = float(signed_distance(np.array(x), np.array(z), OPEN))
            if d < 6.0 or float(fields(np.array(x), np.array(z))[2]) > 0.05 or rng.random() > 0.55:
                continue
            # Away from the clearing: the gradient of the distance, by finite differences.
            gx_ = float(signed_distance(np.array(x + 1.0), np.array(z), OPEN)) - d
            gz_ = float(signed_distance(np.array(x), np.array(z + 1.0), OPEN)) - d
            angle = math.atan2(gz_, gx_) + rng.uniform(-0.8, 0.8)
            dx, dz = math.cos(angle), math.sin(angle)
            add(roots[rng.integers(4)], x, z, rng.uniform(0.35, 0.7), math.atan2(-dx, -dz))
    for px, py, count in CLUSTERS:
        cx, cz = to_world(px, py)
        for _ in range(count):
            a, r = rng.uniform(0, 2 * math.pi), rng.uniform(0, 7)
            add('scenery.lookdev-mushrooms', cx + math.cos(a) * r, cz + math.sin(a) * r,
                rng.uniform(0.45, 0.9), rng.uniform(0, 2 * math.pi))
    # Single fallen leaves blown onto the soil near the clearing's rim.
    xs = np.arange(FOCUS[0] - 175, FOCUS[0] + 175, 3.0)
    zs = np.arange(FOCUS[1] - 150, FOCUS[1] + 140, 3.0)
    gx, gz = np.meshgrid(xs, zs)
    gx = gx + rng.uniform(-1.4, 1.4, gx.shape)
    gz = gz + rng.uniform(-1.4, 1.4, gz.shape)
    dist = signed_distance(gx, gz, OPEN)
    _, ford_, pool_ = fields(gx, gz)
    chance = smoothstep(-12.0, -1.0, dist) * (dist < 0) * 0.16
    rolls = rng.random(gx.shape)
    for idx in np.ndindex(gx.shape):
        if rolls[idx] < chance[idx] and ford_[idx] < 0.05 and pool_[idx] < 0.05:
            add(LEAVES[rng.integers(2)], gx[idx], gz[idx], rng.uniform(0.12, 0.24), rng.uniform(0, 2 * math.pi))
    return objects

# ------------------------------------------------------------------ map

def snap(value):
    """Building grid: centres at 1.5 mod 4."""
    return math.floor((value - 1.5) / 4.0 + 0.5) * 4.0 + 1.5


def build_map():
    def at(px, py):
        x, z = to_world(px, py)
        return {'x': snap(x), 'y': snap(z)}

    def amber(id_, px, py):
        return {'definition': 'building.neutral.amber-mine', 'id': id_,
                'initialState': {'amount': 12500, 'construction': 'complete'}, 'owner': 'none',
                'position': at(px, py), 'rotation': 0.0,
                'appearance': {'asset': 'asset.models.resources.lookdev-amber'}}

    def hall(id_, px, py):
        return {'definition': 'building.beetles.bastion', 'id': id_,
                'initialState': {'construction': 'complete'}, 'owner': 'player.1',
                'position': at(px, py), 'rotation': 0.0}

    start = at(560, 640)
    return {
        'biome': 'autumn-forest',
        'sandbox': True,
        'camps': [],
        'size': SIZE,
        'playerStarts': [{'mainFort': 'start.player.1/main-fort', 'player': 1, 'setup': 'setup.beetles',
                          'x': start['x'], 'z': start['y']}],
        'entities': [
            {'definition': 'unit.ants.warrior', 'id': 'lookdev.hero', 'owner': 'player.1',
             'position': {'x': float(math.floor(to_world(560, 860)[0])), 'y': float(math.floor(to_world(560, 860)[1]))},
             'rotation': 200.0},
        ],
        'terrain': {'paint': [{'material': 'asset.terrain.lookdev-litter-sparse', 'variants': False},
                              {'material': 'asset.terrain.lookdev-wet-soil', 'variants': False}], 'textureGrid': {'cellSize': 5, 'origin': [-0.5, -0.5]}, 'waterProfiles': ['water.fall-ford']},
    }


def main():
    if os.path.exists(OUT):
        shutil.rmtree(OUT)
    os.makedirs(os.path.join(OUT, 'terrain'))
    with open(os.path.join(OUT, 'package.json'), 'w') as f:
        json.dump({'format': 1, 'id': 'lookdev-fall', 'kind': 'map', 'name': 'Look-dev: Fall',
                   'version': '1.0.0', 'dependencies': {'base': '>=0.3.5'}}, f, indent=2)
        f.write('\n')
    with open(os.path.join(OUT, 'map.json'), 'w') as f:
        json.dump(build_map(), f, indent=2)
        f.write('\n')
    heights, water, litter, zero, flow, sparse, wet, shade = build_grids()
    t = os.path.join(OUT, 'terrain')
    write_grid(os.path.join(t, 'heights.grid'), heights, 1, SIDE)
    write_grid(os.path.join(t, 'water-heights.grid'), water, 1, SIDE)
    write_grid(os.path.join(t, 'water-flow.grid'), flow, 3, SPAN)
    write_grid(os.path.join(t, 'grass.grid'), litter, 1, SIDE)
    write_grid(os.path.join(t, 'rock.grid'), zero, 1, SIDE)
    write_grid(os.path.join(t, 'paint-1-weights.grid'), sparse, 1, SIDE)
    write_grid(os.path.join(t, 'paint-2-weights.grid'), wet, 1, SIDE)
    write_grid(os.path.join(t, 'shade.grid'), shade, 1, SIDE)
    scenery = build_scenery()
    with open(os.path.join(OUT, 'scenery.json'), 'w') as f:
        json.dump({'objects': scenery}, f, indent=2, ensure_ascii=False)
        f.write('\n')
    counts = {}
    for o in scenery:
        counts[o['asset']] = counts.get(o['asset'], 0) + 1
    print('wrote', OUT, len(scenery), 'objects', counts)


if __name__ == '__main__':
    main()
