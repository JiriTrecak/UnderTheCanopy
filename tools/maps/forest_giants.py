"""Authors Forest Giants, a showcase of the forest floor's scale (docs/art-direction.md, What
big means), through the editor's MCP tools.

A clean, flat clearing in the middle holds an ant colony and its army. Around it the forest
floor is dressed with the giants in bands:
- the rim, just outside the clearing: moss cushions, pebbles, dew drops, needles, seeds and
  nuts, button mushrooms, fiddleheads, clover and sorrel, fallen leaves and twigs;
- the deep frame: fern crowns, boletes, ink caps, morels, glowing bonnets, burrs and cones;
- the giants: parasol mushrooms, beech trunks, the branch ridge, the feather, roots.
Height grows with distance from the clearing, and the tallest stand on the far (north)
side, where the camera looks over them into the canopy. Inside the clearing there are only
a few low landmarks (a snail shell, an acorn cup with its acorn, a fallen leaf, pebbles).
The camera never leaves the playable area, so the frame is inside it. It stays open ground
(the giants' blockers keep units out of most of it): closed blocks would bring the biome's
own frame scatter, and this map shows the giants alone.

Run with the server up (`canopy mcp --data <repo>`): `python3 tools/maps/forest_giants.py`.
It creates the map, so remove the package first to rebuild it. Placement is seeded, so a
rebuild gives the same map. `--scenery` (no server needed) rewrites only the existing map's
scenery.json, for trying placements."""
import json, math, os, random, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'forest-giants'
FOLDER = 'showcase'
SIZE = 256
BORDER = 16
C = SIZE / 2
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
rng = random.Random(23)


def radius(a):
    """The clearing's edge, metres from the middle, at angle `a`: a wobbling circle."""
    return 62 + 5 * math.sin(3 * a + 0.4) + 3 * math.cos(5 * a)


def outside(x, z):
    """Metres outside the clearing's edge (negative inside)."""
    dx, dz = x - C, z - C
    return math.hypot(dx, dz) - radius(math.atan2(dz, dx))


def measure(name):
    """Half the larger footprint side and the height of a giants model, in its metres."""
    data = open(os.path.join(ROOT, 'base', 'assets', 'environment', name, 'geometry.glb'), 'rb').read()
    n = struct.unpack('<I', data[12:16])[0]
    gltf = json.loads(data[20:20 + n])
    lo, hi = [1e9] * 3, [-1e9] * 3
    for mesh in gltf['meshes']:
        for prim in mesh['primitives']:
            acc = gltf['accessors'][prim['attributes']['POSITION']]
            lo = [min(a, b) for a, b in zip(lo, acc['min'])]
            hi = [max(a, b) for a, b in zip(hi, acc['max'])]
    return max(hi[0] - lo[0], hi[2] - lo[2]) / 2, hi[1] - lo[1]


placed = []  # (asset, x, z, yaw, scale, radius)


# Flat litter (leaves, needles, twigs) lies under everything, so it never blocks a placement.
LITTER = {'leaf-oak', 'leaf-beech', 'leaf-maple', 'bracken', 'pine-needles-a', 'pine-needles-b', 'twig', 'maple-seed'}


def free(x, z, r, overlap, litter):
    return all(math.hypot(px - x, pz - z) >= (r + pr) * overlap
               for (a, px, pz, _, _, pr) in placed if (a.removeprefix('scenery.giants-') in LITTER) == litter)


def put(piece, x, z, scale=1.0, yaw=None, force=False, overlap=0.7):
    r, height = measure('giants-' + piece)
    r, height = r * scale, height * scale
    if not force:
        d = outside(x, z)
        # Height grows with distance from the clearing, more steeply on the far (north)
        # side: the camera looks over the near edge and up into the far one.
        limit = 3 + d * (2.4 if z < C else 1.3 if z < C + 30 else 0.8)
        if not (0 < x < SIZE and 0 < z < SIZE) or height > limit or d < r * 0.4:
            return False
        if not free(x, z, r, overlap, piece in LITTER):
            return False
    placed.append(('scenery.giants-' + piece, x, z, rng.uniform(0, math.tau) if yaw is None else yaw, scale, r))
    return True


def spot(near, far):
    """A random point between `near` and `far` metres outside the clearing."""
    while True:
        x, z = rng.uniform(2, SIZE - 2), rng.uniform(2, SIZE - 2)
        if near <= outside(x, z) <= far:
            return x, z


def clusters(pieces, count, near, far, size=(3, 6), spread=1.6, scale=(0.75, 1.2), overlap=0.75):
    """`count` clusters, each of one piece from `pieces`, a few together as they grow."""
    for _ in range(count):
        piece = rng.choice(pieces)
        r = measure('giants-' + piece)[0]
        for _ in range(20):
            x, z = spot(near, far)
            n = rng.randint(*size)
            done = 0
            for _ in range(n * 6):
                a, d = rng.uniform(0, math.tau), rng.uniform(0, r * spread * math.sqrt(n))
                if put(piece, x + d * math.cos(a), z + d * math.sin(a), rng.uniform(*scale), overlap=overlap):
                    done += 1
                    if done >= n:
                        break
            if done:
                break


def scatter(pieces, count, near, far, scale=(0.8, 1.2), overlap=0.7):
    """`count` single pieces from `pieces` between `near` and `far` metres out."""
    done = 0
    for _ in range(count * 40):
        x, z = spot(near, far)
        if put(rng.choice(pieces), x, z, rng.uniform(*scale), overlap=overlap):
            done += 1
            if done >= count:
                return


class Scenery:
    """`--scenery`: no server; only the scenery file is written, at the end."""

    def call(self, _name, **args):
        if _name == 'edit' and args['commands'][0]['document'].endswith('scenery.json'):
            path = os.path.join(ROOT, 'base', args['commands'][0]['document'])
            with open(path, 'w') as f:
                json.dump({'objects': args['commands'][0]['value']}, f, indent=2)
                f.write('\n')
            print(len(args['commands'][0]['value']), 'scenery objects written')
            sys.exit(0)
        return {}


c = Scenery() if '--scenery' in sys.argv else mcp.Client()
c.call('create_map', id=M, name='Showcase: Forest Giants', size=SIZE, biome='frozen-dream', folder=FOLDER)
D = f'maps/{FOLDER}/{M}.utcm/map.json'
c.call('edit', label='The playable area, an afternoon that stays', commands=[
    {'op': 'set', 'document': D, 'pointer': '/playable',
     'value': {'x': BORDER, 'z': BORDER, 'width': SIZE - 2 * BORDER, 'height': SIZE - 2 * BORDER}},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
    {'op': 'set', 'document': f'maps/{FOLDER}/{M}.utcm/package.json', 'pointer': '/description',
     'value': 'The forest floor at insect scale: an ant colony in a clearing among giant mushrooms, ferns, nuts, '
              'leaves and trunks.'},
    {'op': 'set', 'document': D, 'pointer': '/landscape',
     'value': {'environment': {'hour': 15.5, 'playing': False, 'weather': {'kind': 'clear'}}}},
])
c.call('map_blocks', map=M, op='flat')

# --- The giants, placed by hand: most on the far (north) side, a few on the flanks. ---
put('beech-trunk', 206, 30, 1.0, force=True)
put('beech-trunk', 26, 40, 0.8, yaw=2.0, force=True)
put('parasol', 76, 22, 1.1, yaw=0.6, force=True)
put('parasol', 150, 16, 0.9, yaw=1.1, force=True)
put('parasol', 238, 104, 0.85, yaw=2.2, force=True)
put('parasol', 16, 118, 0.75, yaw=4.0, force=True)
put('branch', 116, 34, 0.9, yaw=0.12, force=True)
put('feather', 232, 168, 0.8, yaw=1.6, force=True)
put('root', 174, 40, 1.0, yaw=2.4, force=True)
put('root', 232, 62, 0.9, yaw=1.2, force=True)
put('root', 46, 60, 0.8, yaw=5.2, force=True)
put('fern-crown', 40, 92, 0.9, force=True)
put('fern-crown', 222, 212, 0.9, force=True)
put('fern-crown', 30, 196, 0.8, force=True)
put('porcini', 196, 70, 0.9, force=True)
put('grass', 104, 12, 1.0, force=True)
put('shelf-fungus', 196, 46, 0.9, yaw=3.6, force=True)
put('mycena', 214, 140, 0.9, force=True)

# --- Layers, from the giants inwards. ---
# Deep frame: fern crowns, grass, big boletes and the odd parasol, in groups.
clusters(['fern-crown', 'grass', 'harts-tongue'], 14, 24, 110, size=(2, 4), spread=1.2, scale=(0.75, 1.1))
clusters(['porcini', 'parasol', 'inkcap'], 6, 30, 110, size=(2, 3), spread=1.4, scale=(0.7, 1.0))
# Mid: mushroom clusters, plant groves, cones, burrs and fronds.
clusters(['inkcap', 'puffballs', 'withered', 'mycena', 'buttons', 'buttons', 'inkcap', 'mycena'], 20, 6, 40, size=(3, 5), spread=1.3)
clusters(['morel', 'porcini'], 6, 10, 50, size=(2, 3), spread=1.5, scale=(0.6, 0.95))
clusters(['clover', 'sorrel', 'fiddleheads', 'fern-frond', 'harts-tongue'], 22, 5, 45, size=(3, 6), spread=1.1)
scatter(['pine-cone', 'chestnut-burr', 'bark-slab', 'mossy-stone', 'porcini'], 16, 8, 60, scale=(0.7, 1.1))
# Rim: low ground cover just outside the clearing; seeds in twos and threes.
clusters(['moss', 'moss', 'buttons', 'bilberries'], 18, 0, 10, size=(2, 5), spread=1.4, scale=(0.6, 1.1))
clusters(['pebbles', 'mossy-stone'], 5, 2, 30, size=(1, 3), spread=1.5, scale=(0.5, 0.9))
clusters(['acorn', 'hazelnut', 'beechnut'], 10, 1, 16, size=(2, 3), spread=1.5, scale=(0.7, 1.0))
clusters(['dew-drops', 'dew-drop'], 14, 0, 20, size=(1, 3), spread=2.0)
# Litter under everything: fallen leaves, bracken, seeds, needles and twigs.
scatter(['leaf-oak', 'leaf-beech', 'leaf-maple', 'leaf-oak', 'bracken', 'maple-seed', 'pine-needles-a',
         'pine-needles-b', 'twig', 'leaf-beech'], 260, 0, 110, scale=(0.55, 1.05), overlap=0.3)
# Fill: moss and small plants between the groups.
scatter(['moss', 'moss', 'clover', 'sorrel', 'fiddleheads', 'buttons'], 140, 2, 110, scale=(0.7, 1.2), overlap=0.6)

# --- Inside the clearing: a few low landmarks, clean ground between them. ---
put('snail-shell', 94, 98, 0.9, yaw=0.7, force=True)
put('acorn-cup', 162, 160, 1.0, yaw=2.0, force=True)
put('acorn', 172, 150, 0.9, yaw=0.4, force=True)
put('leaf-oak', 94, 162, 0.8, yaw=2.6, force=True)
put('pebbles', 164, 96, 0.7, yaw=1.0, force=True)

objects = [{'id': f'giants.{i}', 'asset': a, 'x': round(x, 2), 'z': round(z, 2), 'yaw': round(y, 3),
            'scale': round(s, 3)} for i, (a, x, z, y, s, _) in enumerate(placed)]
c.call('edit', label='The forest giants', commands=[
    {'op': 'set', 'document': f'maps/{FOLDER}/{M}.utcm/scenery.json', 'pointer': '/objects', 'value': objects}])

# --- The colony and its army, for scale. ---
FORT = (121.5, 129.5)
print(c.call('map_place', map=M, kind='start', x=FORT[0], z=FORT[1]))
entities = [
    {'definition': 'building.ants.fort', 'id': 'colony.hall', 'owner': 'player.1',
     'initialState': {'construction': 'complete'}, 'position': {'x': FORT[0], 'y': FORT[1]}, 'rotation': 0.0},
    {'definition': 'building.ants.barracks', 'id': 'colony.burrow', 'owner': 'player.1',
     'initialState': {'construction': 'complete'}, 'position': {'x': 145.5, 'y': 113.5}, 'rotation': 0.0},
    {'definition': 'building.ants.house', 'id': 'colony.mound', 'owner': 'player.1',
     'initialState': {'construction': 'complete'}, 'position': {'x': 99.5, 'y': 143.5}, 'rotation': 0.0},
]
army = ['unit.ants.warrior'] * 8 + ['unit.ants.archer'] * 4 + ['unit.ants.queensguard'] * 2
for i, d in enumerate(army):
    entities.append({'definition': d, 'id': f'colony.army.{i}', 'owner': 'player.1',
                     'position': {'x': 134.0 + 5.0 * (i % 5), 'y': 140.0 + 5.0 * (i // 5)}, 'rotation': 200.0})
for i, d in enumerate(['unit.ants.queen', 'unit.ants.marshal']):
    entities.append({'definition': d, 'id': f'colony.hero.{i}', 'owner': 'player.1',
                     'position': {'x': 140.0 + 9.0 * i, 'y': 160.0}, 'rotation': 200.0})
for i in range(5):
    entities.append({'definition': 'unit.ants.civilian', 'id': f'colony.worker.{i}', 'owner': 'player.1',
                     'position': {'x': 104.0 + 4.0 * i, 'y': 116.0}, 'rotation': 90.0})
c.call('edit', label='The colony', commands=[
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': entities},
    {'op': 'set', 'document': D, 'pointer': '/sandbox', 'value': True},
])
print(len(objects), 'scenery objects')
print(c.call('problems'))
print(c.call('save', force=True))
