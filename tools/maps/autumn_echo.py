"""Re-dresses Echo Isles in the autumn forest (D61, docs/art-direction.md) without changing how
it plays: every start, camp, amber site, deposit, height, cliff, water level and path stays.

Runs on Blender's bundled Python (numpy):
    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" tools/maps/autumn_echo.py

It reads the summer original from git (`git show <SOURCE>:...`), so reruns are identical, and
rewrites `base/maps/skirmish/echo-isles.utcm` in place:
- **Tree walls become the forest frame, as solid as it looks.** The ground the pines stood on
  turns to dark leaf litter that sinks into shade with depth (`shade.grid`, D63), and the same
  mask closes it (`blocked.grid`): what reads as frame is never walked or built on. Each pine
  becomes a low frame piece (fern, bracken, a shrub, mushrooms); ferns, roots, leaves,
  and saplings the old pines' height dress the woods. Leaves and acorns stay on the closed side;
  only a few single leaves blow onto the soil.
- **Ground.** Summer Warcraft paint becomes the autumn biome's soil and litter, with two
  corner-transition atlases breaking up the open ground (`asset.terrain.autumn-moss` where the
  summer grass was thickest, `asset.terrain.autumn-needles` on the old dirt), a debris band
  along the frame and wet soil along the shores; cliffs keep their rock.
- **Water.** Water a unit can wade (60 cm, the simulation's wading depth) takes the pale,
  calm shallows profile; deeper water is the biome's dark pool. Fallen leaves float on both."""
import json, math, os, shutil, struct, subprocess, zlib
import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
MAP = 'base/maps/skirmish/echo-isles.utcm'
OUT = os.path.join(ROOT, MAP)
SOURCE = '8335a30'  # the last summer Echo Isles, with its Substance deposits (D62)
HALO = 16
WADE = 0.6  # metres: crates/sim/src/terrain/mod.rs WADING_DEPTH_CM
SEED = 20261019
CLOSE_GAP = 4.0  # cells: gaps between trees narrower than this become frame

# ------------------------------------------------------------------ grids


def git_bytes(rel):
    return subprocess.run(['git', 'show', '%s:%s' % (SOURCE, rel)], cwd=ROOT, check=True,
                          capture_output=True).stdout


def read_grid(data):
    assert data[:4] == b'UTCG'
    _, code, codec, shuffled = data[4:8]
    w, h = struct.unpack('<II', data[8:16])
    raw = zlib.decompress(data[16:]) if codec else data[16:]
    size = 4 if code in (1, 3) else 1
    if shuffled and size > 1:
        planes = np.frombuffer(raw, np.uint8).reshape(size, -1)
        raw = planes.T.copy().tobytes()
    if code == 1:
        return np.frombuffer(raw, '<f4').reshape(h, w).astype(np.float64)
    if code == 3:
        return np.frombuffer(raw, np.uint8).reshape(h, w, 4).copy()
    return np.frombuffer(raw, np.uint8).reshape(h, w).copy()


def write_grid(path, data, code):
    side = data.shape[0]
    raw = data.tobytes()
    size = 4 if code in (1, 3) else 1
    if size > 1:
        raw = b''.join(raw[plane::size] for plane in range(size))
    header = b'UTCG' + bytes([1, code, 1, 1 if size > 1 else 0]) + struct.pack('<II', data.shape[1], side)
    with open(path, 'wb') as f:
        f.write(header + zlib.compress(raw, 9))


def grid(name):
    return read_grid(git_bytes('%s/terrain/%s' % (MAP, name)))

# ------------------------------------------------------------------ noise and fields


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


def distance_to(mask, steps):
    """Octagonal distance (cells) from every point to the nearest True of `mask`, up to `steps`."""
    inf = float(steps + 1)
    d = np.where(mask, 0.0, inf)
    diag = math.sqrt(2.0)
    for _ in range(steps):
        p = np.pad(d, 1, constant_values=inf)
        n = np.minimum.reduce([
            p[1:-1, 1:-1], p[:-2, 1:-1] + 1, p[2:, 1:-1] + 1, p[1:-1, :-2] + 1, p[1:-1, 2:] + 1,
            p[:-2, :-2] + diag, p[:-2, 2:] + diag, p[2:, :-2] + diag, p[2:, 2:] + diag])
        if np.array_equal(n, d):
            break
        d = n
    return d

# ------------------------------------------------------------------ pieces

# Frame pieces standing where the pines stood: (scenery id, weight, model scale per pine
# scale). They block nothing themselves: the blocked grid closes the frame.
WALL = [
    ('scenery.lookdev-fern', 0.50, 1.0),
    ('scenery.lookdev-bracken', 0.30, 0.9),
    ('scenery.autumn-shrub-copper', 0.07, 1.7),
    ('scenery.autumn-shrub-gold', 0.06, 1.7),
    ('scenery.lookdev-mushrooms', 0.07, 0.6),
]
PINE_BLOCKER = 3.3  # scenery.woodland-pine-*: a 3.3-cell ellipse at scale 1

# Dressing in the frame, never blocking: (scenery id, weight, scale range, zone): 'edge' only
# near the frame's rim, 'deep' only well inside, 'any' both.
DRESS = [
    ('scenery.lookdev-fern', 10, (0.9, 1.6), 'any'),
    ('scenery.lookdev-bracken', 2.5, (0.6, 1.1), 'any'),
    ('scenery.lookdev-leaf-big', 1.0, (1.0, 2.0), 'any'),
    ('scenery.lookdev-leaf-big-oak', 1.0, (1.0, 2.0), 'any'),
    ('scenery.lookdev-leaf-oak', 1.2, (1.2, 1.9), 'edge'),
    ('scenery.lookdev-leaf-maple', 1.2, (1.2, 1.9), 'edge'),
    ('scenery.lookdev-leaf-beech', 1.2, (1.1, 1.7), 'edge'),
    ('scenery.lookdev-mushrooms', 0.5, (0.35, 0.65), 'any'),
    ('scenery.lookdev-acorn', 0.8, (0.7, 1.2), 'edge'),
    ('scenery.lookdev-puffball', 0.25, (0.3, 0.5), 'deep'),
    ('scenery.autumn-shrub-copper', 0.5, (1.2, 1.8), 'any'),
    ('scenery.autumn-shrub-gold', 0.4, (1.2, 1.8), 'any'),
]
LITTER = ['scenery.lookdev-litter-oak', 'scenery.lookdev-litter-maple', 'scenery.lookdev-litter-beech']
LEAVES = ['scenery.lookdev-leaf-big', 'scenery.lookdev-leaf-big-oak']
ROOTS = ['scenery.lookdev-root-a', 'scenery.lookdev-root-b', 'scenery.lookdev-root-c', 'scenery.lookdev-root-d']
ROOT_LENGTHS = [48.0, 38.0, 58.0, 28.0]  # tools/props/build_roots.py


# ------------------------------------------------------------------ build


def main():
    rng = np.random.default_rng(SEED)
    heights = grid('heights.grid')
    water = grid('water-heights.grid')
    side = heights.shape[0]
    v = np.arange(side, dtype=np.float64) - HALO
    x, z = np.meshgrid(v, v)
    wet_vertex = water > -16.5
    depth = np.where(wet_vertex, water - heights, -1.0)

    # Original paint, as coverage after later layers paint over earlier ones.
    weights = [grid('paint-%d-weights.grid' % i) for i in range(1, 9)]
    cover = []
    for i, w in enumerate(weights):
        c = w.copy()
        for later in weights[i + 1:]:
            c *= 1.0 - later
        cover.append(c)
    ldrt, ldro, ldrg, lrok, lgrs, lgrd, zsan, cliff = cover

    pines = json.loads(git_bytes(MAP + '/scenery.json'))['objects']
    tree = np.zeros_like(heights, bool)
    for o in pines:
        r = PINE_BLOCKER / 2 * o['scale']
        i0, i1 = int(o['z'] + HALO - r - 1), int(o['z'] + HALO + r + 2)
        j0, j1 = int(o['x'] + HALO - r - 1), int(o['x'] + HALO + r + 2)
        sub = (x[i0:i1, j0:j1] - o['x']) ** 2 + (z[i0:i1, j0:j1] - o['z']) ** 2 <= r ** 2
        tree[i0:i1, j0:j1] |= sub
    # Warcraft's woods are trees on a grid with gaps no army fits through; the frame closes
    # them into one solid wood (a morphological closing: grow, then shrink back), so the
    # frame has no checkerboard of holes, while gaps wider than CLOSE_GAP stay open.
    pine = tree
    grown = distance_to(tree, 8) <= CLOSE_GAP / 2
    tree = distance_to(~grown, 8) > CLOSE_GAP / 2
    # Keep the closing inside the woods: only ground hemmed in by trees on two sides fills
    # (a narrow gap), never the woods' outer edge, so the open ground keeps its size.
    tree &= pine | (distance_to(pine, 8) <= CLOSE_GAP / 2 + 0.5)
    walk = ~tree & (depth <= WADE)
    deep = depth > WADE
    inside = distance_to(walk | deep, 40)  # how far into the frame (tree ground)
    outside = distance_to(tree, 24)  # how far from the frame
    # Signed distance to the frame's edge (> 0 in the frame), its outline roughened a little
    # inwards only, so no gap between woods ever narrows.
    wobble = -fbm(x, z, 9, 11) * 1.5
    signed = np.where(tree, inside, -outside) + wobble
    frame = signed > 0.0

    # Ground: litter in the frame and, faintly, where summer grass was; rock where rock was.
    litter = smoothstep(-1.5, 1.0, signed)
    # Rock where the summer had rock or cliff faces (the biome's rock, not Warcraft's tiles).
    rock = np.clip(np.maximum(lrok * 0.85, cliff) * (0.8 + (fbm(x, z, 4, 51) - 0.5) * 0.8), 0, 0.75)
    # The debris band: the frame's rim breaking up onto the soil (atlas corners pick the
    # tiles, so the masks below are near-binary).
    band = (signed + (fbm(x, z, 7, 17) - 0.5) * 3.0 > -3.0) & (signed < 2.0)
    debris = (band & ~deep).astype(np.float64)
    # The summer's structure carries over: its worn dirt paths stay bare soil; its grass
    # becomes a floor of pine needles with broad patches of moss, so the open ground is
    # never one surface but never blotchy either.
    open_ground = ~frame & ~deep
    covered = np.clip(lgrs + lgrd, 0, 1) + (fbm(x, z, 11, 47) - 0.5) * 0.5 > 0.5
    needles = (covered & open_ground).astype(np.float64)
    moss = (covered & open_ground & (fbm(x, z, 40, 41) + (fbm(x, z, 8, 49) - 0.5) * 0.25 > 0.56)).astype(np.float64)
    # Wet, darker soil along every shore and where the summer sand was.
    above = heights - np.where(wet_vertex, water, -17.0)
    shore = (1.0 - smoothstep(0.0, 0.9, above + (fbm(x, z, 5, 19) - 0.5) * 0.6)) * wet_vertex
    damp = np.clip(np.maximum(shore * 0.8, zsan), 0, 1)
    # The frame is closed: one value per cell, closed when its centre's corners are mostly frame.
    cell_frame = (frame[:-1, :-1].astype(int) + frame[1:, :-1] + frame[:-1, 1:] + frame[1:, 1:]) >= 3
    blocked = cell_frame.astype(np.uint8)
    # Frame shade: none on the playable ground, sinking with depth into the woods.
    # Dark from the frame's very edge (Echo's woods are narrow bands), deeper further in.
    shade = (0.55 + 0.2 * smoothstep(1.0, 8.0, signed + (fbm(x, z, 16, 23) - 0.5) * 4.0)) * smoothstep(-0.8, 0.3, signed)

    # Water profile per cell: wadeable water is the ford (slot 1), the rest the biome's pool.
    cell_depth = (depth[:-1, :-1] + depth[1:, :-1] + depth[:-1, 1:] + depth[1:, 1:]) / 4.0
    cell_wet = wet_vertex[:-1, :-1] & wet_vertex[1:, 1:]
    flow = grid('water-flow.grid')
    flow[..., 0] = 128
    flow[..., 1] = 128
    flow[..., 2] = 0
    flow[..., 3] = 0
    ford = cell_wet & (cell_depth > 0.0) & (cell_depth <= WADE + 0.1)
    flow[ford, 3] = 1  # water.fall-shallows

    # ---------------------------------------------------------------- scenery
    mp = json.loads(git_bytes(MAP + '/map.json'))
    keep_clear = [(e['position']['x'], e['position']['y']) for e in mp['entities']]
    keep_clear += [(s['x'], s['z']) for s in mp['playerStarts']]
    keep_clear = np.array(keep_clear)
    objects = []

    def add(asset, px, pz, scale, yaw, mode='terrain', elevation=0.0):
        objects.append({'asset': asset, 'elevation': round(float(elevation), 3), 'heightMode': mode,
                        'id': 'autumn.%d' % len(objects), 'locked': False, 'scale': round(float(scale), 4),
                        'visible': True, 'x': round(float(px), 2), 'yaw': round(float(yaw), 4),
                        'z': round(float(pz), 2)})

    def at(field, px, pz):
        i = np.clip(np.round(np.asarray(pz) + HALO).astype(int), 0, side - 1)
        j = np.clip(np.round(np.asarray(px) + HALO).astype(int), 0, side - 1)
        return field[i, j]

    # The walls: one frame piece per pine, its blocker the pine's.
    wall_w = np.array([w for _, w, _ in WALL])
    for o in pines:
        sid, _, factor = WALL[rng.choice(len(WALL), p=wall_w / wall_w.sum())]
        add(sid, o['x'], o['z'], o['scale'] * factor * rng.uniform(0.95, 1.1), rng.uniform(0, 2 * math.pi),
            o.get('heightMode', 'terrain'), o.get('elevation', 0.0))

    def near_entity(px, pz, r):
        return (np.hypot(keep_clear[:, 0] - px, keep_clear[:, 1] - pz) < r).any()

    # Dressing on a jittered grid over the frame.
    xs = np.arange(-HALO, side - HALO, 2.6)
    gx, gz = np.meshgrid(xs, xs)
    gx = gx + rng.uniform(-1.0, 1.0, gx.shape)
    gz = gz + rng.uniform(-1.0, 1.0, gz.shape)
    s = at(signed, gx, gz)
    t = at(frame.astype(float), gx, gz) > 0.5
    density = np.where(t, 0.22 + 0.25 * fbm(gx, gz, 14, 29), 0.0)
    edge = t & (s < 3.5)
    deep_in = t & (s > 6.0)
    dw = np.array([w for _, w, _, _ in DRESS])
    rolls = rng.random(gx.shape)
    for idx in zip(*np.nonzero(rolls < density)):
        allowed = np.array([1.0 if zone == 'any' or (zone == 'edge' and edge[idx]) or (zone == 'deep' and deep_in[idx])
                            else 0.0 for _, _, _, zone in DRESS])
        w = dw * allowed
        sid, _, (s0, s1), _ = DRESS[rng.choice(len(DRESS), p=w / w.sum())]
        add(sid, gx[idx], gz[idx], rng.uniform(s0, s1), rng.uniform(0, 2 * math.pi))

    # Fine fallen leaves along the frame's rim, on the closed side; only a few single leaves
    # blow onto the soil, so nothing walkable looks like frame.
    xs = np.arange(-HALO, side - HALO, 1.4)
    gx, gz = np.meshgrid(xs, xs)
    gx = gx + rng.uniform(-0.6, 0.6, gx.shape)
    gz = gz + rng.uniform(-0.6, 0.6, gz.shape)
    s = at(signed, gx, gz)
    dry = at(depth, gx, gz) <= 0.05
    chance = np.where(dry, 0.16 * smoothstep(-0.5, 0.8, s) * (1 - smoothstep(2.5, 5.0, s))
                      + 0.025 * smoothstep(-6.0, -1.5, s) * (s < -1.0), 0.0)
    rolls = rng.random(gx.shape)
    for idx in zip(*np.nonzero(rolls < chance)):
        px, pz = gx[idx], gz[idx]
        if s[idx] < 0 and near_entity(px, pz, 14.0):
            continue
        if s[idx] < -1.0:
            add(LEAVES[rng.integers(2)], px, pz, rng.uniform(0.12, 0.22), rng.uniform(0, 2 * math.pi))
        else:
            add(LITTER[rng.integers(3)], px, pz, rng.uniform(0.8, 1.5), rng.uniform(0, 2 * math.pi))

    # Surface roots deep in the larger woods, pointing away from the playable ground, kept in
    # the frame along their whole length.
    for zc in np.arange(-HALO, side - HALO, 11.0):
        for xc in np.arange(-HALO, side - HALO, 11.0):
            px, pz = xc + rng.uniform(-4, 4), zc + rng.uniform(-4, 4)
            d0 = float(at(inside, px, pz))
            if not at(frame, px, pz) or d0 < 5.0 or rng.random() > 0.6:
                continue
            gxd = float(at(inside, px + 1, pz)) - d0
            gzd = float(at(inside, px, pz + 1)) - d0
            angle = math.atan2(gzd, gxd) + rng.uniform(-0.7, 0.7)
            dx, dz = math.cos(angle), math.sin(angle)
            kind, scale = int(rng.integers(4)), rng.uniform(0.18, 0.32)
            along = np.linspace(0.0, 1.0, 8) * ROOT_LENGTHS[kind] * scale
            if (at(inside, px + dx * along, pz + dz * along) < 2.5).any() or not at(frame, px + dx * along, pz + dz * along).all():
                continue
            add(ROOTS[kind], px, pz, scale, math.atan2(-dx, -dz))

    # Saplings where the woods are deep enough, and giant trees in the deepest ones: their
    # crowns rise past the RTS view and dissolve there (D64); their boles stay in the frame.
    def scatter(kind, minimum, spacing, count, scales):
        placed = []
        cand = np.argwhere(frame & (signed >= minimum))
        rng.shuffle(cand)
        for i, j in cand:
            px, pz = j - HALO + rng.uniform(-0.5, 0.5), i - HALO + rng.uniform(-0.5, 0.5)
            if any(math.hypot(px - a, pz - b) < spacing for a, b in placed):
                continue
            placed.append((px, pz))
            asset = kind[rng.integers(len(kind))] if isinstance(kind, list) else kind
            add(asset, px, pz, rng.uniform(*scales), rng.uniform(0, 2 * math.pi))
            if len(placed) >= count:
                break
        return placed

    # Saplings about the old pines' height (the lookdev sapling is 21 m: about 10 m here),
    # only where the wood is deep enough that their crowns stay over it. No giant trees: in
    # woods this narrow their trunks and roots would cover the playable ground.
    giants = []
    saplings = scatter('scenery.lookdev-sapling', 6.0, 14.0, 160, (0.4, 0.55))

    # Fallen leaves afloat: drifting in the fords, scattered on the dark pools near the shores.
    for _ in range(9000):
        px, pz = rng.uniform(-HALO, side - HALO), rng.uniform(-HALO, side - HALO)
        dpt = float(at(depth, px, pz))
        if dpt <= 0.05:
            continue
        if dpt <= WADE and rng.random() < 0.5:
            add(LEAVES[rng.integers(2)], px, pz, rng.uniform(0.18, 0.36), rng.uniform(0, 2 * math.pi),
                'absolute', float(at(water, px, pz)) + 0.02)
        elif dpt > WADE and float(at(outside, px, pz)) < 18.0 and rng.random() < 0.12:
            add(LEAVES[rng.integers(2)], px, pz, rng.uniform(0.18, 0.32), rng.uniform(0, 2 * math.pi),
                'absolute', float(at(water, px, pz)) + 0.02)

    # ---------------------------------------------------------------- write
    t = os.path.join(OUT, 'terrain')
    for f in os.listdir(t):
        os.remove(os.path.join(t, f))
    write_grid(os.path.join(t, 'heights.grid'), heights.astype('<f4'), 1)
    write_grid(os.path.join(t, 'water-heights.grid'), water.astype('<f4'), 1)
    write_grid(os.path.join(t, 'water-flow.grid'), flow, 3)
    write_grid(os.path.join(t, 'grass.grid'), litter.astype('<f4'), 1)
    write_grid(os.path.join(t, 'rock.grid'), rock.astype('<f4'), 1)
    # The renderer draws at most four ground layers a cell (the biome's soil, litter, rock and
    # waterbed count): the paint layers never overlap one another, in priority debris, wet
    # soil, moss, needles, and the open-ground ones stay off rock and the frame's litter.
    bare = (rock < 0.05) & (litter < 0.05)
    debris = np.where(above < 0.6, 0.0, debris)  # the shore is the wet soil's: no fifth layer there
    damp = np.where(debris > 0, 0.0, damp)
    moss = np.where((debris > 0) | (damp > 0.05) | ~bare, 0.0, moss)
    needles = np.where((debris > 0) | (damp > 0.05) | (moss > 0) | ~bare, 0.0, needles)
    layers = [needles, moss, debris, damp]
    for n, layer in enumerate(layers, 1):
        write_grid(os.path.join(t, 'paint-%d-weights.grid' % n), layer.astype('<f4'), 1)
    write_grid(os.path.join(t, 'shade.grid'), shade.astype('<f4'), 1)
    write_grid(os.path.join(t, 'blocked.grid'), blocked, 2)

    mp['biome'] = 'autumn-forest'
    mp['terrain']['paint'] = [
        {'material': 'asset.terrain.autumn-needles', 'variants': False},
        {'material': 'asset.terrain.autumn-moss', 'variants': False},
        {'material': 'asset.terrain.lookdev-debris', 'variants': False},
        {'material': 'asset.terrain.lookdev-wet-soil', 'variants': False},
    ]
    mp['terrain']['waterProfiles'] = ['water.fall-shallows']
    # The autumn forest's afternoon: a low warm sun throwing long shadows (D61).
    mp['landscape'] = {'environment': {'hour': 15.5, 'playing': False}}
    with open(os.path.join(OUT, 'map.json'), 'w') as f:
        json.dump(mp, f, indent=2, ensure_ascii=False)
        f.write('\n')
    with open(os.path.join(OUT, 'scenery.json'), 'w') as f:
        json.dump({'objects': objects}, f, indent=2, ensure_ascii=False)
        f.write('\n')
    counts = {}
    for o in objects:
        counts[o['asset']] = counts.get(o['asset'], 0) + 1
    print('wrote', OUT, len(objects), 'objects; giants', len(giants), 'saplings', len(saplings))
    print(sorted(counts.items(), key=lambda kv: -kv[1]))
    print('closed cells', int(blocked.sum()), 'ford cells', int(ford.sum()),
          'moss', round(float(moss.mean()), 3), 'needles', round(float(needles.mean()), 3))


if __name__ == '__main__':
    main()
