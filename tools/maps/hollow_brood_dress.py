"""The Hollow Brood's set dressing (tools/maps/hollow_brood.py `dress`). The Underleaf biome
dresses the frame with the forest-floor giants itself (`blocks.scatter`); this places what
belongs to the mission: the giants that mark its corners (beech trunks, parasols, the
branch ridge, the feather), the landmarks in play (the webs' snail shell, the beetle husk,
the broodkeepers' shrine), the Underleaf mission pieces at the places, the stream's reeds,
and the forest going wrong along the road: from the sick pool on, stands of withered
plants, dissolving ink caps and glowing bonnets, thickest round the Hollow (`wrong`).

A piece stands only where it fits: deep enough in the forest for its height (faster where
the camera looks over play at it than where it stands between the camera and play), its
blocker clear of open ground, out of the saved cameras' lines of sight. Seeded, so a
rebuild gives the same map. The mission's own props (webs, the lost patrol's cocoons, the
Hollow's dead) come last, so the pieces before them keep their ids."""
import json, math, os, random, struct
from collections import deque

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def read_grid(path):
    """A `.grid` file's width and samples (floats, or bytes)."""
    import zlib
    data = open(path, 'rb').read()
    element, codec, shuffle = data[5], data[6], data[7]
    width, height = struct.unpack('<II', data[8:16])
    raw = zlib.decompress(data[16:]) if codec == 1 else data[16:]
    size = {1: 4, 2: 1, 3: 4}[element]
    if size > 1 and shuffle & 1:
        n = len(raw) // size
        raw = bytes(raw[p * n + i] for i in range(n) for p in range(size))
    if element == 1:
        return width, struct.unpack('<%df' % (width * height), raw)
    return width, raw


_sizes = {}


def measure(piece):
    """A piece's half footprint, height and blocker reach (from its centre), in its metres."""
    if piece not in _sizes:
        group, name = piece.split('/')
        sc = json.load(open(os.path.join(ROOT, 'base', 'scenery', group, name.removeprefix('giants-') + '.json')))
        model = sc['model'].removeprefix('asset.models.environment.')
        path = os.path.join(ROOT, 'base', 'assets', 'environment', model, 'geometry.glb')
        if not os.path.exists(path):
            path = os.path.join(ROOT, 'base', 'assets', 'environment', 'underleaf', model, 'geometry.glb')
        data = open(path, 'rb').read()
        n = struct.unpack('<I', data[12:16])[0]
        gltf = json.loads(data[20:20 + n])
        lo, hi = [1e9] * 3, [-1e9] * 3
        for mesh in gltf['meshes']:
            for prim in mesh['primitives']:
                acc = gltf['accessors'][prim['attributes']['POSITION']]
                lo = [min(a, b) for a, b in zip(lo, acc['min'])]
                hi = [max(a, b) for a, b in zip(hi, acc['max'])]
        blockers = sc.get('blockers') or ([sc['blocker']] if sc.get('blocker') else [])
        reach = max([max(b['width'], b['depth']) / 2 + math.hypot(b.get('x', 0), b.get('z', 0))
                     for b in blockers] or [0])
        _sizes[piece] = (max(hi[0] - lo[0], hi[2] - lo[2]) / 2, hi[1] - lo[1], reach)
    return _sizes[piece]


G = 'giants/giants-'
U = 'underleaf/'
L = 'landmark/'
# Flat litter lies under everything, so it never stops another piece.
LITTER = {G + p for p in ('leaf-oak', 'leaf-beech', 'leaf-maple', 'bracken', 'pine-needles-a',
                          'pine-needles-b', 'twig', 'maple-seed')}


def wrong(x, z):
    """How far the forest has gone wrong here: 0 by the start, 1 at the Hollow."""
    return max(0.0, min(1.0, (x - 0.6 * z + 60) / 260))


def g(*names):
    return [G + n for n in names]


class Dresser:
    def __init__(self, package, size, seed=29):
        self.rng = random.Random(seed)
        self.size = size
        self.side, closed = read_grid(os.path.join(package, 'terrain', 'blocked.grid'))
        self.vside, self.ground = read_grid(os.path.join(package, 'terrain', 'heights.grid'))
        _, self.water = read_grid(os.path.join(package, 'terrain', 'water-heights.grid'))
        side = self.side
        # Distance into the forest from open ground, and the open cell nearest.
        INF = 10 ** 9
        self.dist = [INF] * (side * side)
        self.near = [0] * (side * side)
        q = deque()
        for i in range(side * side):
            if not closed[i]:
                self.dist[i] = 0
                self.near[i] = i
                q.append(i)
        while q:
            i = q.popleft()
            x, z = i % side, i // side
            for dx, dz in ((1, 0), (-1, 0), (0, 1), (0, -1)):
                nx, nz = x + dx, z + dz
                if 0 <= nx < side and 0 <= nz < side:
                    j = nz * side + nx
                    if self.dist[j] == INF:
                        self.dist[j] = self.dist[i] + 1
                        self.near[j] = self.near[i]
                        q.append(j)
        self.placed = []  # (piece, x, z, yaw, scale, r)
        self.sightlines = []  # (eye x, eye z, target x, target z) of close saved cameras
        self.least_wrong = 0.0  # spots only where the forest has gone at least this wrong

    def cell(self, x, z):
        return int(math.floor(x)) + 16, int(math.floor(z)) + 16

    def wet(self, x, z):
        cx, cz = self.cell(x, z)
        if not (0 <= cx < self.vside and 0 <= cz < self.vside):
            return False
        i = cz * self.vside + cx
        return self.water[i] > -16 and self.water[i] > self.ground[i] - 0.05

    def depth(self, x, z):
        """Metres into the forest, and which side of play the piece stands on: +1 when the
        open ground lies south of it (the camera looks over play at it), -1 north."""
        cx, cz = self.cell(x, z)
        side = self.side
        if not (0 <= cx < side and 0 <= cz < side):
            return 60.0, 0.0
        i = cz * side + cx
        if self.dist[i] == 0:
            return 0.0, 0.0
        ox, oz = self.near[i] % side, self.near[i] // side
        d = math.hypot(ox - cx, oz - cz)
        return d, (oz - cz) / max(d, 1.0)

    def free(self, x, z, r, overlap, litter):
        for (p, px, pz, _, _, pr) in self.placed:
            if (p in LITTER) == litter and (px - x) ** 2 + (pz - z) ** 2 < ((r + pr) * overlap) ** 2:
                return False
        return True

    def put(self, piece, x, z, scale=1.0, yaw=None, force=False, overlap=0.7):
        r, height, reach = measure(piece)
        r, height, reach = r * scale, height * scale, reach * scale
        if not force:
            if not (-12 < x < self.size + 12 and -12 < z < self.size + 12):
                return False
            d, s = self.depth(x, z)
            rise = max(0.6, 1.5 + 1.1 * s)
            if height > 3 + d * rise or d < max(r * 0.35, reach + 1.0) or self.wet(x, z):
                return False
            if not self.free(x, z, r, overlap, piece in LITTER):
                return False
            # Nothing taller than a pebble in a saved camera's line of sight.
            if height > 1.5:
                for (ex, ez, tx, tz) in self.sightlines:
                    vx, vz = tx - ex, tz - ez
                    t = max(0.0, min(1.0, ((x - ex) * vx + (z - ez) * vz) / (vx * vx + vz * vz)))
                    if math.hypot(ex + vx * t - x, ez + vz * t - z) < r + 3:
                        return False
        yaw = self.rng.uniform(0, math.tau) if yaw is None else yaw
        self.placed.append((piece, x, z, yaw, scale, r))
        return True

    def spot(self, near_d, far_d, box=None):
        for _ in range(4000):
            if box:
                x, z = self.rng.uniform(box[0], box[2]), self.rng.uniform(box[1], box[3])
            else:
                x, z = self.rng.uniform(-10, self.size + 10), self.rng.uniform(-10, self.size + 10)
            if near_d <= self.depth(x, z)[0] <= far_d and wrong(x, z) >= self.least_wrong:
                return x, z
        return None

    def pick(self, sets, x, z):
        return sets[1] if self.rng.random() < wrong(x, z) ** 1.6 else sets[0]

    def clusters(self, sets, count, near_d, far_d, size=(3, 6), spread=1.5, scale=(0.75, 1.2), overlap=0.75, box=None):
        """`count` groups, each of one piece from the healthy or withered set."""
        for _ in range(count):
            at = self.spot(near_d, far_d, box)
            if not at:
                continue
            piece = self.rng.choice(self.pick(sets, *at))
            r = measure(piece)[0]
            n = self.rng.randint(*size)
            done = 0
            for _ in range(n * 8):
                a, d = self.rng.uniform(0, math.tau), self.rng.uniform(0, r * spread * math.sqrt(n))
                if self.put(piece, at[0] + d * math.cos(a), at[1] + d * math.sin(a), self.rng.uniform(*scale),
                            overlap=overlap):
                    done += 1
                    if done >= n:
                        break

    def scatter(self, sets, count, near_d, far_d, scale=(0.8, 1.2), overlap=0.7, box=None):
        done = 0
        for _ in range(count * 30):
            at = self.spot(near_d, far_d, box)
            if at and self.put(self.rng.choice(self.pick(sets, *at)), at[0], at[1], self.rng.uniform(*scale),
                               overlap=overlap):
                done += 1
                if done >= count:
                    return

    def objects(self):
        return [{'id': f'dress.{i}', 'asset': 'scenery.' + p.split('/')[1], 'x': round(x, 2), 'z': round(z, 2),
                 'yaw': round(y, 3), 'scale': round(s, 3)} for i, (p, x, z, y, s, _) in enumerate(self.placed)]


def dress(package, size, stream, sightlines=()):
    """Every scenery object of the map, in order; `stream` is the stream's line, and
    `sightlines` the low saved cameras' (eye x, eye z, target x, target z), kept clear."""
    d = Dresser(package, size)
    d.sightlines = list(sightlines)
    put = d.put
    rng = d.rng

    # --- Set pieces, placed by hand -------------------------------------------------------
    # The giants: three dark beech trunks at the corners (the start's south-west, over the
    # Hollow, the far north-west), roots and shelf fungus beside them, parasols along the
    # north and east, the branch ridge in the west, the feather in the south-east.
    put(G + 'beech-trunk', -4, 300, 1.0, 0.6, force=True)
    put(G + 'beech-trunk', 384, 0, 1.1, 2.2, force=True)
    put(G + 'beech-trunk', 20, 30, 0.8, 4.0, force=True)
    put(G + 'root', 40, 330, 0.9, 5.9, force=True)
    put(G + 'root', 34, 262, 0.8, 0.4, force=True)
    put(G + 'root', 336, 22, 0.9, 1.1, force=True)
    put(G + 'root', 360, 52, 0.8, 2.6, force=True)
    put(G + 'shelf-fungus', 26, 290, 1.0, 1.3, force=True)
    put(G + 'shelf-fungus', 352, 30, 1.1, 3.9, force=True)
    put(G + 'parasol', 150, 30, 1.0, 0.7, force=True)
    put(G + 'parasol', 262, 16, 0.9, 1.6, force=True)
    put(G + 'parasol', 370, 140, 0.85, 2.4, force=True)
    put(G + 'parasol', 40, 150, 0.8, 4.4, force=True)
    put(G + 'parasol', 150, 236, 0.75, 3.1, force=True)
    put(G + 'parasol', 300, 214, 0.8, 5.0, force=True)
    put(G + 'branch', 14, 214, 1.0, 1.35, force=True)
    put(G + 'feather', 328, 340, 0.9, 2.6, force=True)
    put(G + 'fern-crown', 236, 352, 0.9, force=True)
    put(G + 'fern-crown', 120, 352, 0.85, force=True)
    # Landmarks in play: the webs' snail shell, the beetle wreck, the broodkeepers' shrine at
    # the Moonlit Hollow's north end, facing the way in (its lantern lies in front of it).
    put(G + 'snail-shell', 60, 204, 1.0, 2.2, force=True)
    put(G + 'beetle-husk', 316, 302, 1.0, 0.9, force=True)
    put(U + 'hollow-broodkeepers-shrine', 206, 45, 1.0, 0.0, force=True)
    put(G + 'pebbles', 196, 52, 0.5, 1.0, force=True)
    # The hornets' paper nest: a stack of shelf fungus and bark at the ledge's back (a real
    # paper nest is a prop still to make).
    put(G + 'shelf-fungus', 284, 166, 0.8, 2.0, force=True)
    put(G + 'bark-slab', 278, 174, 0.7, 0.5, force=True)
    # The Hollow: the collapsed mound where the nest still glows, the pools, dead plants.
    put(U + 'hollow-collapsed-mound', 322, 52, 1.0, 0.4, force=True)
    put(U + 'substance-pool-large', 290, 58, 0.9, 0.3, force=True)
    put(U + 'substance-pool', 326, 84, 0.8, 1.7, force=True)
    put(U + 'substance-pool-small', 284, 84, 0.9, 2.6, force=True)
    # The sick pool.
    put(U + 'substance-pool', 112, 164, 0.9, 0.8, force=True)
    put(U + 'substance-pool-small', 120, 158, 0.8, 2.0, force=True)
    for x, z, s, a in [(300, 80, 1.4, 0.4), (320, 70, 1.3, 2.1), (280, 70, 1.2, 4.0), (296, 46, 1.1, 5.1),
                       (124, 176, 1.3, 1.0), (106, 172, 1.1, 2.8), (340, 70, 1.0, 3.3), (196, 238, 0.8, 1.4)]:
        put(U + ('underleaf-patch-withered-a' if a < 3 else 'underleaf-patch-withered-b'), x, z, s, a, force=True)
    for i, (x, z, s) in enumerate([(276, 56, 3.2), (334, 92, 3.0), (310, 40, 2.8), (100, 158, 2.8), (132, 160, 2.4)]):
        put(U + 'hollow-bleached-mushrooms', x, z, s, i * 1.7, force=True)
    for i, (x, z, s) in enumerate([(292, 92, 3.0), (338, 58, 2.6), (118, 182, 2.6)]):
        put(U + 'hollow-toppled-mushrooms', x, z, s, i * 2.3 + 0.5, force=True)
    for i, (x, z, s) in enumerate([(272, 76, 3.4), (346, 76, 3.2), (300, 34, 3.6), (318, 100, 3.0), (104, 152, 3.0),
                                   (140, 176, 2.8), (96, 180, 3.2)]):
        put(U + ('hollow-withered-fern' if i % 2 else 'hollow-withered-fern-tall'), x, z, s, i * 2.1, force=True)
    # The webbed hollow: the lost patrol's cocoons, standing, hanging from a dead stalk, lying.
    for piece, x, z, a in [('standing', 82, 188, 0.3), ('hanging', 90, 194, 1.9), ('lying', 76, 200, 3.0)]:
        put(U + 'hollow-cocoon-' + piece, x, z, 1.0, a, force=True)
    # The mushroom circle on the road, the ford's stones and reeds, the lily pool.
    put(U + 'glade-mushroom-ring', 134, 298, 1.7, 0.0, force=True)
    for x, z, s in [(184, 260, 2.4), (202, 276, 2.2), (204, 260, 2.0)]:
        put(L + 'woodland-river-stones', x, z, s, rng.uniform(0, 6), force=True)
    for x, z, s in [(104, 284, 1.6), (118, 286, 1.4), (110, 292, 1.2), (98, 282, 1.3)]:
        put(L + ('shore-lily-white' if s > 1.3 else 'shore-lily-pink'), x, z, s, rng.uniform(0, 6), force=True)
    for x, z in [(122, 284), (96, 288), (176, 284), (206, 270)]:
        put(L + 'shore-cattails', x, z, 2.2, rng.uniform(0, 6), force=True)
    # The Moonlit Hollow: glowing bonnets round the shrine, dew on the moss.
    for x, z, s, a in [(196, 40, 0.7, 0.5), (218, 44, 0.65, 2.5), (214, 62, 0.6, 4.1), (194, 60, 0.55, 5.5)]:
        put(G + 'mycena', x, z, s, a, force=True)
    for x, z in [(202, 58), (212, 52), (200, 48)]:
        put(G + 'dew-drops', x, z, 1.0, rng.uniform(0, 6), force=True)
    put(U + 'underleaf-patch-moss-a', 206, 56, 1.0, 0.4, force=True)
    # Calm ground patches: moss and blossom by the start, moss at the spring.
    put(U + 'underleaf-patch-moss-b', 72, 326, 1.1, 0.8, force=True)
    put(U + 'underleaf-patch-blossoms', 96, 308, 0.9, 2.0, force=True)
    put(U + 'underleaf-patch-moss-a', 262, 240, 1.0, 0.2, force=True)
    # The stream's banks in the forest: reeds and mossy stones break its edges.
    for (ax, az), (bx, bz) in zip(stream, stream[1:]):
        length = math.hypot(bx - ax, bz - az)
        nx, nz = -(bz - az) / length, (bx - ax) / length
        for k in range(int(length // 6)):
            t = (k + rng.random()) * 6 / length
            for sign in (-1, 1):
                off = sign * rng.uniform(5.5, 8.0)
                x, z = ax + (bx - ax) * t + nx * off, az + (bz - az) * t + nz * off
                if d.depth(x, z)[0] >= 1 and not d.wet(x, z):
                    piece = L + 'shore-cattails' if rng.random() < 0.6 else G + 'mossy-stone'
                    put(piece, x, z, rng.uniform(2.0, 3.2) if 'cattails' in piece else rng.uniform(0.35, 0.6),
                        force=True)

    # --- The dead ground round the Hollow: withered ferns, bleached and toppled mushrooms,
    # glowing bonnets, dry bracken; nothing green.
    dead = (230, -12, 396, 150)
    d.clusters(([U + 'hollow-withered-fern-tall', U + 'hollow-withered-fern'],) * 2, 46, 1, 40, size=(3, 6), spread=1.2,
               scale=(3.0, 4.2), box=dead)
    d.clusters(([U + 'hollow-bleached-mushrooms', U + 'hollow-toppled-mushrooms'],) * 2, 26, 1, 30, size=(2, 4),
               spread=1.4, scale=(2.6, 3.8), box=dead)
    d.clusters((g('mycena', 'withered', 'inkcap', 'withered'),) * 2, 22, 3, 50, size=(2, 4), spread=1.3, box=dead)
    d.scatter((g('bracken', 'leaf-oak', 'twig'),) * 2, 90, 1, 60, scale=(0.6, 1.0), overlap=0.3, box=dead)

    # --- The forest going wrong -----------------------------------------------------------
    # The biome dresses the whole frame with the giants (healthy ones); from the sick pool
    # and the terrace on, stands of withered plants, dissolving ink caps and glowing
    # bonnets grow among them, thicker towards the Hollow.
    d.least_wrong = 0.4
    dying = ([U + 'hollow-withered-fern-tall', U + 'hollow-withered-fern'],) * 2
    d.clusters(dying, 30, 1, 30, size=(3, 5), spread=1.2, scale=(2.8, 4.0))
    d.clusters((g('withered', 'inkcap', 'withered'),) * 2, 18, 2, 40, size=(2, 4), spread=1.3, scale=(0.75, 1.1))
    d.least_wrong = 0.6
    d.clusters((g('mycena'),) * 2, 12, 2, 40, size=(2, 3), spread=1.4, scale=(0.7, 1.0))
    d.least_wrong = 0.0
    # The thornbacks' berry bushes round their clearing on the terrace.
    d.clusters((g('bilberries'), g('bilberries')), 8, 0, 8, size=(3, 5), spread=1.2, box=(176, 86, 226, 130))

    # --- The mission's props, last so the pieces above keep their ids ----------------------
    # The webbed hollow: old webs along its edges, their high sides to the trees (sheets
    # west and north-west, curtains south and north-east), tangles over the floor, and more
    # of the patrol's cocoons.
    for piece, x, z, s, a in [('sheet', 54, 196, 1.0, 1.571), ('sheet', 60, 182, 0.8, 2.4),
                              ('drape', 76, 210, 1.0, 3.142), ('drape', 95, 186, 0.8, 5.5),
                              ('tangle', 64, 204, 0.9, 0.7), ('tangle', 92, 203, 0.7, 2.3),
                              ('tangle', 70, 186, 0.6, 4.1)]:
        put(U + 'hollow-web-' + piece, x, z, s, a, force=True)
    for piece, x, z, s, a in [('standing', 60, 200, 0.9, 2.0), ('lying', 66, 191, 0.9, 1.1),
                              ('hanging', 88, 206, 0.9, 4.0)]:
        put(U + 'hollow-cocoon-' + piece, x, z, s, a, force=True)
    # The Hollow's dead (fallen units sink, so these stay), on the verges, out of the lanes: two
    # of the watch by the road where the dead sentry falls, more on the rim and down in the
    # Hollow, bleached by the substance.
    for piece, x, z, a in [('husk', 183, 237, 1.0), ('corpse', 176, 224, 2.2), ('corpse', 254, 120, 2.6),
                           ('husk', 256, 106, 0.4), ('husk', 281, 84, 2.0), ('corpse', 341, 62, 5.0),
                           ('husk', 274, 52, 3.3), ('husk', 300, 45, 4.4)]:
        put(U + 'hollow-ant-' + piece, x, z, 1.0, a, force=True)
    return d.objects()
