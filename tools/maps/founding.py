"""Authors Founding, the ant campaign's second mission (docs/story.md, "Mission 2: Founding"),
through the editor's MCP tools: the blocks (terrain), the set dressing, the entities, the
camps and the mission (players, regions, objectives). The Lua script is `founding.lua`
beside this file; it is copied in as the package's `mission.lua`.

The map, north (z = 0) up, playable 352 × 352 inside a 32-cell border. The ground is the
Underleaf biome's moss at level 2; hollows sink to soil at level 1, ledges rise to 3:

- **The glade** (centre-south, around 204, 238): the calm, open build area where the Queen
  founds her colony; two amber sites, the mossy rise where she plants her standard (236,
  272), the substance spring at its east edge (260, 232), the sleeping Alate in a flower at
  its west edge.
- **The south road**: the column's arrival from the south edge, with the start's crates;
  off it the fern hollow with the sap spring (146, 346, sunk to level 1) and the mushroom
  circle (130, 300).
- **The south-east crossroads** (306, 316): the Pill-Bug Peddler; a road north to the
  Rootbound Guardians' amber pocket (344, 218); a gap east closed by rubble, behind it a
  dew spring in a nook (356, 334).
- **The west road** to the west edge, where Oleander arrives (40, 238); the bog pond's
  hollow below it (78, 286, level 1) with its toads and the fourth amber.
- **The stream** from the east edge along the glade's north, shallow and wadable, falls
  6 m into **the ravine** at (166, 120); a narrow ramp hidden in the ferns leads down to
  **the waterfall's ledge** (level 1, 154, 134) and its sealed cache: the secret.
- **The north bank** beyond the stream: the hornets' cliff (196, 104, level 3), and the
  **ridge** (level 3, 320, 84) with the feral brood's main burrows, up a ramp at (292, 134).
- **The den** (86, 96) sunk to level 1 across the ravine, reached over the narrow root
  bridge (96, 152–184) and down a ramp.

Run with your own server up (`CANOPY_MCP_PORT=7452 ./target/release/canopy mcp --data
<repo>`; the client reads the same variable): `CANOPY_MCP_PORT=7452 python3
tools/maps/founding.py`. It builds the map in `maps/showcase/`, saves it, then moves the
package into the campaign (`campaigns/vanguard.utcc/missions/founding.utcm`), adds the
stream's flow and foam and the script, and so rebuilds from scratch each run (restart the
server between runs: it still holds the moved map). Placement is seeded: a rebuild gives
the same map. `--terrain` stops after the blocks."""
import json, math, os, random, shutil, struct, sys, time, zlib
from collections import deque
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'founding'
BORDER = 32
PLAY = 352
SIZE = PLAY + 2 * BORDER  # 416
SPAN = SIZE + 32          # the terrain covers a 16-cell halo
LO, HI = BORDER, BORDER + PLAY
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
SHOWCASE = os.path.join(ROOT, 'base', 'maps', 'showcase', M + '.utcm')
MISSION = os.path.join(ROOT, 'base', 'campaigns', 'vanguard.utcc', 'missions', M + '.utcm')
D = f'maps/showcase/{M}.utcm/map.json'
SC = f'maps/showcase/{M}.utcm/scenery.json'
rng = random.Random(1902)
TERRAIN_ONLY = '--terrain' in sys.argv

for path in (SHOWCASE, MISSION):
    if os.path.exists(path):
        shutil.rmtree(path)
c = mcp.Client()


def blocks(op, points, **kw):
    c.call('map_blocks', map=M, op=op, points=points, fill=True, **kw)


def line(op, points, radius, **kw):
    c.call('map_blocks', map=M, op=op, points=points, radius=radius, **kw)


def cells(kind, points, radius):
    c.call('map_blocks', map=M, op='cells', points=points, radius=radius, kind=kind)


def fill(kind, points):
    c.call('map_blocks', map=M, op='fill', points=points, kind=kind)


def box(op, a, b, **kw):
    c.call('map_blocks', map=M, op=op, **{'from': list(a)}, to=list(b), **kw)


def ramp(top, foot, level, radius=1.0):
    c.call('map_blocks', map=M, op='ramp', **{'from': list(top)}, to=list(foot), radius=radius, level=level)


def ring(cx, cz, r, n=28, squash=1.0, wobble=0.0, seed=0.0):
    return [[cx + r * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.cos(a),
             cz + r * squash * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.sin(a)]
            for a in (k * 2 * math.pi / n for k in range(n))]


def open_area(outline, level=None):
    """Opens ground inside an outline, block by block and then cell by cell (soft edges)."""
    blocks('paint', outline, kind='ground')
    fill('ground', outline)
    if level is not None:
        blocks('level', outline, level=level)


def trail(points, rmin, rmax, step=9.0, seed=0):
    """A path of overlapping wobbly clearings, wider and narrower as it goes."""
    r = random.Random(seed)
    for (ax, az), (bx, bz) in zip(points, points[1:]):
        n = max(1, int(math.hypot(bx - ax, bz - az) / step))
        for k in range(n + 1):
            t = k / n
            x, z = ax + (bx - ax) * t, az + (bz - az) * t
            open_area(ring(x, z, r.uniform(rmin, rmax), 14, wobble=0.12, seed=r.uniform(0, 6)))


# ======================================================================== the terrain
for attempt in range(60):  # a fresh server checks the package first
    try:
        c.call('create_map', id=M, name='Founding', size=SIZE, biome='underleaf', folder='showcase')
        break
    except RuntimeError as error:
        if 'check' not in str(error):
            raise
        time.sleep(2)
c.call('edit', label='Founding: the playable area, a clear morning', commands=[
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/playable',
     'value': {'x': BORDER, 'z': BORDER, 'width': PLAY, 'height': PLAY}},
    {'op': 'set', 'document': D, 'pointer': '/landscape',
     'value': {'environment': {'hour': 9.0, 'playing': False, 'weather': {'kind': 'clear'}}}},
    {'op': 'set', 'document': f'maps/showcase/{M}.utcm/package.json', 'pointer': '/description',
     'value': 'Prologue, Mission Two. A new colony in a quiet morning glade, under attack from the feral brood.'},
])
c.call('map_blocks', map=M, op='flat')
WORLD = ((-15.9, -15.9), (SIZE + 15.9, SIZE + 15.9))
box('level', *WORLD, level=2)
box('paint', *WORLD, kind='wall')

# The glade and the places around it, all at level 2 (moss).
GLADE = ring(204, 236, 64, 40, squash=0.86, wobble=0.07, seed=0.7)
open_area(GLADE)
open_area(ring(258, 230, 21, 18, wobble=0.1, seed=1.1))   # the substance spring's bay
open_area(ring(234, 270, 18, 16, wobble=0.1))             # around the mossy rise
open_area(ring(186, 290, 18, 16, wobble=0.1, seed=2.2))   # the glade's south mouth
open_area(ring(130, 300, 15, 18, wobble=0.12, seed=4))    # the mushroom circle's clearing
open_area(ring(306, 316, 20, 22, wobble=0.1, seed=1))     # the crossroads
open_area(ring(344, 218, 24, 22, wobble=0.12, seed=3))    # the Rootbound pocket
open_area(ring(357, 334, 13, 18, wobble=0.12, seed=5))    # the dew-spring nook
open_area(ring(150, 214, 14, 16, wobble=0.1, seed=2.6))   # the Alate's corner
# The west: between the ravine's south bank and the west road.
open_area([[70, 190], [100, 184], [126, 194], [146, 208], [148, 246], [116, 252], [62, 252],
           [40, 244], [40, 226], [54, 204]])
for x, z, r, seed in ((104, 212, 12, 0.3), (64, 214, 9, 1.7), (120, 248, 6, 2.9)):
    fill('wall', ring(x, z, r, 16, wobble=0.15, seed=seed))
    blocks('paint', ring(x, z, r - 3, 12), kind='wall')
# The north bank: the stream's valley from the ravine to the east edge.
open_area([[170, 112], [184, 92], [212, 88], [236, 100], [256, 118], [278, 134], [312, 142],
           [352, 144], [400, 146], [400, 182], [352, 188], [318, 192], [288, 190], [258, 184],
           [230, 182], [200, 178], [180, 162], [168, 136]])
trail([[206, 396], [198, 360], [206, 326], [200, 300]], 10, 15, seed=1)  # the south road
trail([[252, 278], [278, 300], [300, 314]], 9, 13, seed=2)               # glade to crossroads
trail([[294, 324], [258, 336], [214, 336]], 7, 10, seed=3)              # crossroads to the road
trail([[312, 300], [326, 272], [338, 244]], 8, 12, seed=4)              # north to the pocket
trail([[146, 238], [112, 238], [76, 236], [28, 234]], 9, 13, seed=5)    # the west road
trail([[170, 344], [196, 340]], 5, 7, seed=6)                           # to the fern hollow
trail([[140, 292], [156, 280]], 6, 8, seed=7)                           # to the mushrooms
trail([[150, 196], [168, 182]], 7, 10, seed=8)                          # the west to the north
line('paint', [[278, 230], [322, 220]], 1.0, kind='ground')             # a gap, spring to pocket
box('paint', (322, 318), (346, 326), kind='ground')                     # the nook's gap (rubble)

# The fern hollow and the bog pond sink to soil (level 1); a ramp down into each.
HOLLOW = ring(146, 346, 18, 22, wobble=0.12, seed=2)
open_area(HOLLOW, level=1)
ramp((171, 344), (165, 344), level=2, radius=0.6)
BOG = ring(78, 286, 28, 26, wobble=0.1, seed=6)
open_area(BOG, level=1)
ramp((86, 252), (86, 260), level=2, radius=1.5)

# The ridge (level 3) and its ramp, falling south to the north bank.
RIDGE = [[268, 58], [292, 38], [332, 34], [366, 46], [378, 84], [370, 116], [344, 132],
         [304, 134], [278, 122], [266, 92]]
open_area(RIDGE, level=3)
ramp((292, 133), (292, 140), level=3, radius=1.5)
# The hornets' cliff (level 3), one narrow ramp up from the east.
HORNETS = ring(196, 102, 15, 18, wobble=0.12, seed=1.3)
open_area(HORNETS, level=3)
ramp((211, 110), (217, 110), level=3, radius=0.6)

# The den across the ravine, sunk to level 1; the bridge lands at level 2 and ramps down.
DEN = [[46, 56], [88, 40], [134, 50], [150, 84], [140, 116], [118, 136], [86, 140], [56, 138],
       [40, 110]]
open_area(DEN, level=1)
open_area(ring(96, 144, 9, 12), level=2)
ramp((96, 138), (96, 132), level=2, radius=1.0)

# The ravine: level 0, deep water, its banks faced with roots.
RAVINE = [[182, 12], [176, 56], [168, 96], [158, 120], [138, 144], [118, 160], [92, 168],
          [76, 166], [62, 172], [46, 168], [30, 176], [14, 172]]
line('level', RAVINE, 2.6, level=0)
line('paint', RAVINE, 2.6, kind='deep')
# The root bridge: two blocks wide, north to south across the ravine at x 92..100.
box('level', (92.5, 150), (99.5, 186), level=2)
box('paint', (92.5, 150), (99.5, 186), kind='ground')
box('face', (92.5, 152), (99.5, 184), style=1)                  # root banks: a root bridge
# The waterfall's ledge: level 1 below the fall, a narrow ramp down to it from the south.
LEDGE = [[148, 128], [158, 126], [162, 134], [160, 142], [150, 142], [146, 136]]
box('level', (148.5, 126.5), (159.5, 141.5), level=1)
box('paint', (148.5, 126.5), (159.5, 141.5), kind='ground')
ramp((156, 152), (156, 145), level=2, radius=0.4)
line('paint', [[170, 182], [164, 168], [158, 156]], 0.6, kind='ground')  # hidden in the ferns
line('face', RAVINE, 3.8, style=1)

# The stream: shallow and wadable, over the moss at level 2.
STREAM = [[408, 160], [372, 164], [340, 158], [312, 162], [284, 158], [258, 154], [234, 150],
          [210, 140], [188, 128], [168, 120]]
cells('shallow', STREAM, 6.0)
POND = ring(88, 292, 14, 18, wobble=0.18, seed=0.4)
fill('shallow', POND)

box('round', *WORLD)
print('terrain', c.call('problems'))
c.call('save', force=True)
if TERRAIN_ONLY:
    sys.exit(0)


# ======================================================================== reading the terrain
def read_grid(path):
    raw = open(path, 'rb').read()
    w, h = struct.unpack('<II', raw[8:16])
    data = zlib.decompress(raw[16:])
    size = len(data) // (w * h)
    if raw[7] and size > 1:
        n = w * h
        out = bytearray(len(data))
        for i in range(size):
            out[i::size] = data[i * n:(i + 1) * n]
        data = bytes(out)
    return raw[:16], w, h, size, data


TERRAIN = os.path.join(SHOWCASE, 'terrain')
_, bw, _, _, bdata = read_grid(os.path.join(TERRAIN, 'blocked.grid'))
_, hw, _, _, hdata = read_grid(os.path.join(TERRAIN, 'heights.grid'))
_, _, _, _, wdata = read_grid(os.path.join(TERRAIN, 'water-heights.grid'))
HEIGHTS = struct.unpack(f'<{len(hdata) // 4}f', hdata)
WATERS = struct.unpack(f'<{len(wdata) // 4}f', wdata)


def closed(x, z):
    cx, cz = int(math.floor(x)) + 16, int(math.floor(z)) + 16
    if not (0 <= cx < SPAN and 0 <= cz < SPAN):
        return True
    return bdata[cz * bw + cx] != 0


def height(x, z):
    vx, vz = min(max(int(round(x)) + 16, 0), hw - 1), min(max(int(round(z)) + 16, 0), hw - 1)
    return HEIGHTS[vz * hw + vx]


def wet(x, z):
    vx, vz = min(max(int(round(x)) + 16, 0), hw - 1), min(max(int(round(z)) + 16, 0), hw - 1)
    return WATERS[vz * hw + vx] > HEIGHTS[vz * hw + vx]


# Distance (cells, on a 2-cell lattice) from every closed sample to the nearest open,
# walkable-looking sample inside the playable square, and where that open sample is.
G = 2
N = SPAN // G
dist = [[-1.0] * N for _ in range(N)]
near = [[None] * N for _ in range(N)]
queue = deque()
for j in range(N):
    for i in range(N):
        x, z = i * G - 16 + 1, j * G - 16 + 1
        if not closed(x, z) and LO <= x <= HI and LO <= z <= HI:
            dist[j][i] = 0.0
            near[j][i] = (x, z)
            queue.append((i, j))
while queue:
    i, j = queue.popleft()
    for di, dj in ((1, 0), (-1, 0), (0, 1), (0, -1), (1, 1), (1, -1), (-1, 1), (-1, -1)):
        a, b = i + di, j + dj
        if 0 <= a < N and 0 <= b < N and dist[b][a] < 0:
            nx, nz = near[j][i]
            x, z = a * G - 16 + 1, b * G - 16 + 1
            dist[b][a] = math.hypot(x - nx, z - nz)
            near[b][a] = near[j][i]
            queue.append((a, b))


def frame_depth(x, z):
    if not closed(x, z):
        return 0.0
    i, j = int((x + 16) // G), int((z + 16) // G)
    if not (0 <= i < N and 0 <= j < N):
        return 999.0
    return dist[j][i]


def nearest_open(x, z):
    i, j = int((x + 16) // G), int((z + 16) // G)
    i, j = min(max(i, 0), N - 1), min(max(j, 0), N - 1)
    return near[j][i]


# ======================================================================== the dressing
ASSETS = os.path.join(ROOT, 'base', 'assets', 'environment')
SIZES = {}


def measure(piece):
    """Half the larger footprint side and the height of a giants model, in metres."""
    if piece in SIZES:
        return SIZES[piece]
    data = open(os.path.join(ASSETS, 'giants-' + piece, 'geometry.glb'), 'rb').read()
    n = struct.unpack('<I', data[12:16])[0]
    gltf = json.loads(data[20:20 + n])
    lo, hi = [1e9] * 3, [-1e9] * 3
    for mesh in gltf['meshes']:
        for prim in mesh['primitives']:
            acc = gltf['accessors'][prim['attributes']['POSITION']]
            lo = [min(a, b) for a, b in zip(lo, acc['min'])]
            hi = [max(a, b) for a, b in zip(hi, acc['max'])]
    SIZES[piece] = (max(hi[0] - lo[0], hi[2] - lo[2]) / 2, hi[1] - lo[1])
    return SIZES[piece]


BLOCKER = {}
for path in os.listdir(os.path.join(ROOT, 'base', 'scenery', 'giants')):
    doc = json.load(open(os.path.join(ROOT, 'base', 'scenery', 'giants', path)))
    bl = doc.get('blockers') or []
    BLOCKER[doc['id'].removeprefix('scenery.giants-')] = max(
        [math.hypot(b.get('x', 0), b.get('z', 0)) + max(b['width'], b['depth']) / 2 for b in bl] or [0.0])

placed = []  # (asset, x, z, yaw, scale, radius, litter)
ELEVATION = {}  # index in placed -> elevation
LITTER = {'leaf-oak', 'leaf-beech', 'leaf-maple', 'bracken', 'pine-needles-a', 'pine-needles-b',
          'twig', 'maple-seed'}


def free(x, z, r, overlap, litter):
    return all(math.hypot(px - x, pz - z) >= (r + pr) * overlap
               for (_, px, pz, _, _, pr, lit) in placed if lit == litter)


def put(piece, x, z, scale=1.0, yaw=None, force=False, overlap=0.7, rim=False):
    """A giants piece: its blocker inside the frame (or wholly on open ground when `rim`
    allows a blocker-free piece there), tall pieces only deep in the frame, taller on the
    far (north) side of the play the camera looks over."""
    r, tall = measure(piece)
    r, tall = r * scale, tall * scale
    litter = piece in LITTER
    if not force:
        if not (2 <= x <= SIZE - 2 and 2 <= z <= SIZE - 2):
            return False
        d = frame_depth(x, z)
        block = BLOCKER.get(piece, 0.0) * scale
        if block > 0 and d < block + 1.5:
            return False
        if block == 0 and d <= 0 and not rim:
            return False
        if wet(x, z):
            return False
        o = nearest_open(x, z)
        far = o is not None and o[1] > z + 4  # play lies south: the piece stands on its far side
        slope = 2.6 if far else 1.0
        if tall > 4 + max(d, 0) * slope:
            return False
        if not free(x, z, r, overlap, litter):
            return False
    placed.append(('scenery.giants-' + piece, x, z, rng.uniform(0, math.tau) if yaw is None else yaw,
                   scale, r, litter))
    return True


def prop(asset, x, z, yaw=0.0, scale=1.0, elevation=None):
    """Any other catalogue piece, placed by hand."""
    if elevation is not None:
        ELEVATION[len(placed)] = elevation
    placed.append((asset, x, z, yaw, scale, 0.0, True))


# --- The giants, by hand: trunks in two corners, parasols on the north and east sides,
# the branch ridge and the feather in the deep frame.
put('beech-trunk', 412, 6, 1.0, yaw=0.6, force=True)
put('beech-trunk', 2, 414, 0.9, yaw=2.2, force=True)
put('root', 372, 20, 1.0, yaw=2.6, force=True)
put('root', 404, 52, 0.9, yaw=1.9, force=True)
put('root', 30, 386, 0.8, yaw=0.4, force=True)
put('shelf-fungus', 386, 34, 1.0, yaw=3.4, force=True)
put('shelf-fungus', 22, 372, 0.9, yaw=0.2, force=True)
for x, z, s, y in ((226, 46, 1.0, 0.9), (396, 116, 0.9, 2.3), (130, 18, 0.85, 0.2),
                   (402, 262, 0.8, 1.6), (20, 128, 0.75, 4.0)):
    put('parasol', x, z, s, yaw=y, force=True)
put('branch', 284, 262, 0.85, yaw=0.35, force=True)
put('feather', 262, 380, 0.75, yaw=1.2, force=True)

# The layers between (ferns, sorrel and clover, boletes, moss, seeds, litter) are the
# Underleaf biome's own frame scatter (`blocks.scatter`): only the giants that set the
# map's skyline and the places' own pieces are placed by hand, so nothing doubles up.

# --- The glade and its places, by hand: calm ground, a few low landmarks with clean
# blockers, patches of moss and blossom.
prop('scenery.glade-mushroom-ring', 130, 300, 0.4, 1.3)            # the mushroom circle
for x, z, s, y in ((160, 270, 1.1, 0.4), (246, 210, 1.0, 1.9), (176, 200, 0.9, 3.1),
                   (222, 292, 1.0, 2.5), (128, 226, 1.0, 0.9), (330, 300, 1.0, 1.2),
                   (60, 228, 1.1, 2.0), (210, 168, 1.0, 0.3)):
    prop('scenery.underleaf-patch-moss-' + ('a' if y > 1.5 else 'b'), x, z, y, s)
for x, z, s, y in ((238, 272, 0.7, 0.2), (150, 216, 0.8, 1.0), (186, 296, 0.9, 2.2),
                   (148, 350, 0.9, 0.5), (356, 330, 0.8, 1.4), (300, 334, 0.8, 2.9)):
    prop('scenery.underleaf-patch-blossoms', x, z, y, s)
for x, z, s, y in ((270, 214, 0.9, 1.0), (186, 318, 1.0, 2.4), (112, 244, 0.9, 0.2),
                   (330, 240, 0.9, 3.0), (226, 160, 0.9, 1.7)):
    prop('scenery.underleaf-patch-leaves-' + ('a' if y > 1.5 else 'b'), x, z, y, s)
# The feral grounds: a sick brood's bedding, withered.
for x, z, s, y in ((300, 74, 1.3, 0.3), (336, 104, 1.2, 1.7), (352, 64, 1.1, 2.9),
                   (78, 82, 1.3, 0.9), (104, 108, 1.1, 2.2), (60, 116, 1.0, 4.0)):
    prop('scenery.underleaf-patch-withered-' + ('a' if y > 1.5 else 'b'), x, z, y, s)
put('beetle-husk', 64, 72, 1.1, yaw=0.8, force=True)               # an old ruin in the den
prop('scenery.hollow-collapsed-mound', 110, 70, 0.4, 0.9)
prop('scenery.hollow-collapsed-mound', 356, 116, 2.0, 0.8)
prop('scenery.substance-pool-small', 70, 124, 0.7, 1.0)              # where the ferals drink
prop('scenery.substance-pool-small', 332, 56, 2.1, 1.1)
for x, z, s, y in ((52, 96, 2.6, 0.4), (124, 100, 2.2, 1.9), (284, 72, 2.8, 2.6), (360, 96, 2.4, 0.9)):
    prop('scenery.hollow-bleached-mushrooms', x, z, y, s)
for x, z, s, y in ((96, 64, 2.4, 1.1), (290, 104, 2.2, 3.3)):
    prop('scenery.hollow-toppled-mushrooms', x, z, y, s)
put('withered', 120, 80, 1.0, yaw=1.0, force=True)
put('withered', 126, 86, 0.8, yaw=2.4, force=True)
put('withered', 360, 76, 1.0, yaw=0.3, force=True)
# A familiar thing near the colony: an acorn in its cup at the glade's south-west rim.
put('acorn-cup', 158, 286, 1.0, yaw=2.0, force=True)
put('acorn', 166, 292, 0.9, yaw=0.4, force=True)
# The hornets' paper nest on its cliff.
put('hornet-nest', 186, 94, 1.0, yaw=0.5, force=True)
# The waterfall: mossy stones, shelf fungus and dew round the ledge.
for x, z, piece, s in ((146, 130, 'mossy-stone', 0.7), (161, 128, 'shelf-fungus', 0.6),
                       (147, 141, 'dew-drops', 1.0), (160, 140, 'moss', 0.8), (152, 126, 'dew-drop', 1.0)):
    put(piece, x, z, s, force=True)
# The bog pond: lilies, cattails and reeds.
for x, z, asset, s, y in ((84, 288, 'shore-lily-white', 4.4, 0.3), (92, 296, 'woodland-lily-a', 4.8, 1.1),
                          (80, 298, 'shore-lily-pink', 4.0, 2.2), (96, 286, 'woodland-lily-b', 4.4, 4.0),
                          (88, 302, 'shore-lily-white', 3.6, 5.1)):
    prop('scenery.' + asset, x, z, y, s)
for x, z, s, y in ((74, 300, 3.0, 0.2), (100, 300, 2.6, 1.4), (102, 282, 2.8, 2.6), (76, 282, 2.4, 3.8)):
    prop('scenery.shore-cattails' if y < 2 else 'scenery.woodland-reed-tuft', x, z, y, s)
# The mossy rise where the Queen plants her standard: moss cushions round a bed of blossom.
for k in range(7):
    a = 3.49 + k * 0.62
    put('moss', 236 + 9.5 * math.cos(a), 271 + 8.5 * math.sin(a), 0.9 + 0.1 * (k % 3), force=True)
put('bilberries', 244, 262, 1.1, force=True)
put('dew-drops', 228, 278, 1.0, force=True)
# The root bridge: a giant root under the earthen span, lying across the ravine.
prop('scenery.glade-root-span', 104, 170, math.pi / 2, 1.0, elevation=-3.5)
prop('scenery.glade-root-span', 87, 166, -math.pi / 2, 0.9, elevation=-3.0)
# The waterfall where the stream drops into the ravine: its lip where the stream's bed
# last stands at level 2, falling the stream's way (the piece's +Z is downstream).
(ax, az), (bx, bz) = STREAM[-2], STREAM[-1]
dx, dz = bx - ax, bz - az
dl = math.hypot(dx, dz)
dx, dz = dx / dl, dz / dl
lip = (ax, az)
for k in range(80):
    x, z = ax + dx * k * 0.5, az + dz * k * 0.5
    if height(x, z) < 4.0:
        break
    lip = (x, z)
prop('scenery.glade-waterfall', round(lip[0] + dx * 0.5, 2), round(lip[1] + dz * 0.5, 2),
     math.atan2(dx, dz) + math.pi, 1.0, elevation=-0.4)  # the piece's +Z maps to -yaw
# The sleeping Alate's flower bed at the glade's west edge.
for x, z, asset, s, y in ((146, 212, 'pl-flowers-white', 9.0, 0.3), (150, 208, 'pl-flowers-bluebell', 8.0, 1.2),
                          (142, 216, 'pl-flowers-blue', 8.0, 2.0), (153, 214, 'pl-flowers-yellow', 7.0, 2.8)):
    prop('scenery.' + asset, x, z, y, s)
# Stones along the stream's banks, as in the glade's target frame.
for _ in range(60):
    t = rng.uniform(0, len(STREAM) - 1.001)
    k = int(t)
    (ax, az), (bx, bz) = STREAM[k], STREAM[k + 1]
    x, z = ax + (bx - ax) * (t - k), az + (bz - az) * (t - k)
    nx, nz = -(bz - az), bx - ax
    nl = math.hypot(nx, nz)
    side = rng.choice((-1, 1)) * rng.uniform(6.5, 9.0)
    px, pz = x + nx / nl * side, z + nz / nl * side
    if LO < px < HI and not wet(px, pz) and free(px, pz, 2.0, 0.9, False):
        placed.append(('scenery.giants-pebbles' if rng.random() < 0.6 else 'scenery.giants-mossy-stone',
                       px, pz, rng.uniform(0, math.tau), rng.uniform(0.25, 0.45), 2.0, False))

objects = [dict({'id': f'dress.{i}', 'asset': a, 'x': round(x, 2), 'z': round(z, 2), 'yaw': round(y, 3),
                 'scale': round(s, 3)}, **({'elevation': ELEVATION[i]} if i in ELEVATION else {}))
           for i, (a, x, z, y, s, _, _) in enumerate(placed)]
for o in objects:
    if 'elevation' in o:
        o['heightMode'] = 'absolute'
c.call('edit', label='Founding: the dressing', commands=[
    {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': objects}])
print(len(objects), 'scenery objects')


# ======================================================================== the entities
def lattice(v, side):
    """A building centre on the 4-cell lattice for a footprint side (edges at -0.5 + 4k)."""
    off = (side / 2 - 0.5) % 4
    return round((v - off) / 4) * 4 + off


E = []


def entity(definition, id, x, z, owner='none', rotation=0.0, state=None, activation=None, **extra):
    e = {'definition': definition, 'id': id, 'owner': owner, 'position': {'x': x, 'y': z},
         'rotation': rotation}
    if state:
        e['initialState'] = state
    if activation:
        e['activation'] = activation
    e.update(extra)
    E.append(e)
    return id


def building(definition, id, x, z, side, owner='none', state=None, rotation=0.0):
    complete = {'construction': 'complete'}
    complete.update(state or {})
    return entity(definition, id, lattice(x, side), lattice(z, side), owner, rotation, complete)


def amber(id, cx, cz, toward, amount=2500):
    """Five amber nodes in an arc, the hollow facing `toward` (the editor's amber site)."""
    cx, cz = lattice(cx, 4), lattice(cz, 4)
    octant = round(math.atan2(toward[1] - cz, toward[0] - cx) / (math.pi / 4)) % 8
    sx, sz = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)][octant]
    px, pz = -sz, sx
    if sx == 0 or sz == 0:
        offsets = [(px * a + sx * b, pz * a + sz * b) for a, b in ((-8, 0), (-4, -4), (0, -4), (4, -4), (8, 0))]
    else:
        offsets = [(-8 * px, -8 * pz + 4 * sz), (-4 * px, -4 * pz), (0, 0), (4 * px, 4 * pz), (8 * px + 4 * sx, 8 * pz)]
    for k, (a, b) in enumerate(offsets):
        entity('building.neutral.amber-mine', f'amber.{id}.{k}', cx + a, cz + b,
               state={'amount': amount, 'construction': 'complete'},
               appearance={'asset': f'asset.models.resources.amber-crystal-{k % 3 + 1}'})


NEST = (lattice(202, 20), lattice(242, 20))
# Player 1: the Nest (raised as the column arrives), the column on the south road.
building('building.ants.fort', 'colony.nest', *NEST, 20, 'player.1', state={'inventory': {'item.amber': 350}})
entity('unit.ants.queen', 'queen', 202, 340, 'player.1', 0.0)
COLUMN = [('unit.ants.warrior', 'raider.1', 198, 346), ('unit.ants.warrior', 'raider.2', 206, 346),
          ('unit.ants.warrior', 'raider.3', 198, 352), ('unit.ants.warrior', 'raider.4', 206, 352),
          ('unit.ants.archer', 'needler.1', 199, 359), ('unit.ants.archer', 'needler.2', 205, 359)]
COLUMN += [('unit.ants.settler', f'worker.{i + 1}', 197 + 4 * (i % 3), 366 + 5 * (i // 3)) for i in range(5)]
for d, i, x, z in COLUMN:
    entity(d, i, x, z, 'player.1', 0.0)

# Amber: two sites in the glade, the Rootbound pocket's, the bog's.
amber('glade', 176, 216, NEST)
amber('glade-south', 160, 262, NEST)
amber('roots', 352, 210, (326, 246))
amber('bog', 70, 298, (90, 272))
building('building.neutral.corrupted-root', 'substance-spring', 260, 232, 8)

# The feral brood (player 2): burrows on the ridge and in the den, spires, defenders.
building('building.ants.barracks', 'burrow.ne.1', 314, 70, 12, 'player.2')
building('building.ants.barracks', 'burrow.ne.2', 344, 96, 12, 'player.2')
building('building.ants.tower', 'spire.ne', 300, 116, 8, 'player.2')
building('building.ants.barracks', 'burrow.nw', 82, 88, 12, 'player.2')
building('building.ants.tower', 'spire.nw', 106, 116, 8, 'player.2')
FERAL = [('unit.ants.warrior', 302, 92), ('unit.ants.warrior', 306, 96), ('unit.ants.warrior', 326, 86),
         ('unit.ants.warrior', 330, 90), ('unit.ants.archer', 316, 104), ('unit.ants.archer', 340, 72),
         ('unit.ants.burster', 322, 112), ('unit.ants.warrior', 288, 124), ('unit.ants.warrior', 296, 124),
         ('unit.ants.warrior', 92, 104), ('unit.ants.warrior', 98, 100), ('unit.ants.warrior', 70, 104),
         ('unit.ants.archer', 86, 112), ('unit.ants.warrior', 112, 128)]
for k, (d, x, z) in enumerate(FERAL):
    entity(d, f'feral.{k + 1}', x, z, 'player.2', rng.uniform(0, 360))

# Oleander and her porters (player 3, dormant): they arrive along the west road.
entity('unit.ants.royal-alchemist', 'oleander', 40, 238, 'player.3', 90.0, activation='script')
entity('unit.ants.settler', 'porter.1', 36, 233, 'player.3', 90.0, activation='script')
entity('unit.ants.settler', 'porter.2', 42, 242, 'player.3', 90.0, activation='script')

# The sleeping Alate in its flower (the easter egg): it belongs to the dormant ally, so
# nobody's warriors bite it (a placed neutral unit outside a camp is killed at once).
entity('unit.ants.winglancer', 'alate-sleeper', 148, 212, 'player.3', 120.0)

# Neutral camps.
CAMPS = []


def camp(id, home, members, loot=None, drops=None, aggro=10, leash=20):
    ids = []
    for k, (d, x, z) in enumerate(members):
        ids.append(entity(d, f'{id}.{k + 1}', x, z, 'none', math.degrees(math.atan2(home[1] - z, home[0] - x)) % 360))
    record = {'id': id, 'members': ids, 'home': {'x': home[0], 'y': home[1]}, 'aggroRange': aggro,
              'leash': leash, 'aggression': 'players'}
    if loot:
        record['lootPool'] = loot
    if drops:
        record['fixedDrops'] = drops
    CAMPS.append(record)


camp('mushrooms', (130, 300), [('unit.neutral.capling', 125, 296), ('unit.neutral.capling', 135, 296),
                               ('unit.neutral.capling', 130, 305), ('unit.neutral.puffcap', 130, 299)],
     drops=['item.resin-salve'], aggro=8, leash=16)
camp('bog-toads', (80, 272), [('unit.neutral.young-toad', 74, 270), ('unit.neutral.young-toad', 86, 272),
                              ('unit.neutral.tree-frog', 70, 278), ('unit.neutral.tree-frog', 90, 278),
                              ('unit.neutral.tadpole-spirit', 80, 280)],
     loot='loot.camp.medium', aggro=10, leash=20)
camp('rootbound', (338, 226), [('unit.neutral.rootling', 332, 222), ('unit.neutral.rootling', 344, 230),
                               ('unit.neutral.old-root', 340, 220)],
     loot='loot.camp.medium', aggro=10, leash=20)
camp('hornets', (198, 108), [('unit.neutral.hornet-guard', 192, 108), ('unit.neutral.hornet-guard', 204, 104),
                             ('unit.neutral.needle-wasp', 188, 100), ('unit.neutral.needle-wasp', 200, 96),
                             ('unit.neutral.nest-mother', 196, 102)],
     drops=['item.warcaller-standard'], aggro=11, leash=18)

# Neutral features.
building('building.neutral.sap-spring', 'sap-spring', 146, 346, 12)
building('building.neutral.merchant', 'peddler', 306, 314, 12, state={'wares': [
    {'item': 'item.resin-salve', 'price': [{'item': 'item.amber', 'amount': 75}], 'stock': 3, 'restockTicks': 2400},
    {'item': 'item.moon-dew', 'price': [{'item': 'item.amber', 'amount': 75}], 'stock': 2, 'restockTicks': 2400},
    {'item': 'item.trailkeeper-flask', 'price': [{'item': 'item.amber', 'amount': 125}], 'stock': 2, 'restockTicks': 3600},
    {'item': 'item.mossweave-wraps', 'price': [{'item': 'item.amber', 'amount': 150}], 'stock': 1, 'restockTicks': 7200},
    {'item': 'item.scout-whistle', 'price': [{'item': 'item.amber', 'amount': 100}], 'stock': 1, 'restockTicks': 4800},
]})
building('building.neutral.amber-cache', 'falls-cache', 154, 134, 4,
         state={'inventory': {'item.quickstep-spurs': 1, 'item.restoration-draught': 1}})
building('building.neutral.rubble', 'nook-rubble', 334, 322, 4, rotation=90.0)
building('building.neutral.dew-spring', 'nook-spring', 360, 336, 12)
entity('item.moon-dew', 'nook-dew', 350, 342)
building('building.neutral.amber-cache', 'road-cache', 190, 330, 4, state={'inventory': {'item.resin-salve': 1}})
building('building.neutral.seed-pod', 'road-pod.1', 220, 350, 4)
building('building.neutral.seed-pod', 'road-pod.2', 186, 366, 4)
building('building.neutral.seed-pod', 'roots-pod', 360, 232, 4)
building('building.neutral.supply-crate', 'den-crate', 98, 74, 4, state={'inventory': {'item.trailkeeper-flask': 1}})
building('building.neutral.seed-pod', 'hollow-pod', 134, 336, 4)

REGIONS = [
    ('rise', 237, 272, 6), ('glade', 204, 238, 44), ('sap', 146, 346, 16), ('merchant', 306, 314, 18),
    ('bog', 80, 282, 24), ('roots', 342, 220, 22), ('hornets', 198, 104, 20), ('falls', 154, 134, 9),
    ('bridge', 96, 172, 14), ('ridge', 322, 86, 40), ('den', 88, 92, 36), ('spring', 260, 232, 16),
    ('nook', 356, 334, 14), ('mushrooms', 130, 300, 16), ('alate-near', 148, 212, 16), ('west', 40, 238, 10),
    ('meet', 168, 238, 8), ('front', 214, 214, 10),
]
OBJECTIVES = [
    {'id': 'colony', 'title': 'Build the colony',
     'description': 'Raise a Brood Mound and a Soldier Burrow in the glade.'},
    {'id': 'burrows', 'title': 'Destroy the feral burrows',
     'description': 'Destroy the feral burrows on the north-east ridge and in the den across the ravine.'},
    {'id': 'spring', 'title': 'The substance spring', 'optional': True,
     'description': 'Build a Substance Siphon on the spring at the glade\'s east edge.'},
]
FERAL_WAVES = [
    # Raiders from 3:00, a wave every 90 s, one more every second wave.
    {'after': 180, 'every': 90, 'units': [{'unit': 'unit.ants.warrior', 'count': 3}],
     'grow': {'every': 2, 'unit': 'unit.ants.warrior', 'count': 1}, 'from': 'trained', 'target': {'region': 'front'}},
    # From 8:00 the mutated join: Bursters with a Needler, one more Burster every second wave.
    {'after': 480, 'every': 90, 'units': [{'unit': 'unit.ants.burster', 'count': 1},
                                          {'unit': 'unit.ants.archer', 'count': 1}],
     'grow': {'every': 2, 'unit': 'unit.ants.burster', 'count': 1}, 'from': 'trained', 'target': {'region': 'front'}},
    # From 12:00 a column of Needlers every other minute or so.
    {'after': 720, 'every': 130, 'units': [{'unit': 'unit.ants.archer', 'count': 2}],
     'grow': {'every': 3, 'unit': 'unit.ants.archer', 'count': 1}, 'from': 'trained', 'target': {'region': 'front'}},
]
# Saved cameras for the scenes (docs/campaign.md "Scenes"): yaw 0 puts the eye due south
# looking north, 90 east; eyes stand over open ground or high above the giants.
CAMERAS = [
    # The opening: the column running up the south road towards the camera (the scene pans
    # along), the glade and the mossy ring as they stream in, the Queen in the ring's mouth
    # from her column, and Tiv close, looking up at her.
    {'id': 'arrival', 'x': 203, 'y': 330, 'height': 3, 'distance': 62, 'pitch': 13, 'yaw': 172, 'fov': 24},
    {'id': 'glade-mouth', 'x': 220, 'y': 280, 'height': 2, 'distance': 72, 'pitch': 26, 'yaw': 250, 'fov': 36},
    {'id': 'rise-front', 'x': 236, 'y': 271, 'height': 7, 'distance': 44, 'pitch': 5, 'yaw': 287.7, 'fov': 18},
    {'id': 'rise-over', 'x': 224.7, 'y': 275.3, 'height': 2.2, 'distance': 14, 'pitch': 18, 'yaw': 150.6, 'fov': 30},
    {'id': 'glade-wide', 'x': 204, 'y': 244, 'height': 0, 'distance': 130, 'pitch': 34, 'yaw': 150, 'fov': 40},
    # Oleander's arrival: down the west road with her (panned along), then the two of them,
    # each past the other's shoulder, and the spring she names.
    {'id': 'west-road', 'x': 52, 'y': 238, 'height': 4, 'distance': 52, 'pitch': 20, 'yaw': 104, 'fov': 30},
    {'id': 'meet-over-queen', 'x': 167.4, 'y': 240.1, 'height': 6.5, 'distance': 44, 'pitch': 6, 'yaw': 124, 'fov': 22},
    {'id': 'meet-over-oleander', 'x': 180.1, 'y': 240.3, 'height': 7, 'distance': 40, 'pitch': 6, 'yaw': 230, 'fov': 24,
     'roll': -1.5},
    {'id': 'spring', 'x': 260, 'y': 232, 'height': 1, 'distance': 40, 'pitch': 22, 'yaw': 200, 'fov': 38},
    # The victory: whoever speaks where the fighting left them (panned there), then the colony.
    {'id': 'victory-close', 'x': 204, 'y': 236, 'height': 6, 'distance': 32, 'pitch': 20, 'yaw': 10, 'fov': 26},
    {'id': 'victory', 'x': 202, 'y': 244, 'height': 0, 'distance': 104, 'pitch': 42, 'yaw': 20, 'fov': 40},
]
PLAYERS = [
    # The feral brood, sick with the substance: the same violet-grey as in the Hollow.
    {'player': 2, 'race': 'ants', 'team': 'enemy', 'vision': False, 'color': '#8a7f96',
     'ai': {'behaviour': 'waves', 'rebuild': False, 'waves': FERAL_WAVES}},
    {'player': 3, 'race': 'ants', 'team': 'ally', 'vision': True,
     'ai': {'behaviour': 'passive', 'dormant': True}},
]
shutil.copyfile(os.path.join(HERE, 'founding.lua'), os.path.join(SHOWCASE, 'mission.lua'))
c.call('edit', label='Founding: entities, camps and the mission', commands=[
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': E},
    {'op': 'set', 'document': D, 'pointer': '/camps', 'value': CAMPS},
    {'op': 'set', 'document': D, 'pointer': '/mission', 'value': {
        'players': PLAYERS, 'objectives': OBJECTIVES, 'cameras': CAMERAS,
        'regions': [{'id': i, 'x': x, 'y': z, 'radius': r} for i, x, z, r in REGIONS]}},
])
print('mission', c.call('problems'))
c.call('save', force=True)

# ======================================================================== into the campaign
os.makedirs(os.path.dirname(MISSION), exist_ok=True)
shutil.move(SHOWCASE, MISSION)
pkg = json.load(open(os.path.join(MISSION, 'package.json')))
pkg['dependencies'] = {'base': '>=0.4.0'}
with open(os.path.join(MISSION, 'package.json'), 'w') as f:
    json.dump(pkg, f, indent=2, ensure_ascii=False)
    f.write('\n')

# The stream's flow and foam (the blocks make still water): downstream along the stream to
# the fall, white water where it drops into the ravine and below it, along the ravine.
header, fw, fh, fsize, flow = read_grid(os.path.join(MISSION, 'terrain', 'water-flow.grid'))
flow = bytearray(flow)
FALL = (164, 121)


def along(path, x, z):
    """The nearest segment's direction and the distance to it."""
    best = (1e9, (0.0, 0.0))
    for (ax, az), (bx, bz) in zip(path, path[1:]):
        dx, dz = bx - ax, bz - az
        ll = dx * dx + dz * dz
        t = max(0.0, min(1.0, ((x - ax) * dx + (z - az) * dz) / ll))
        d = math.hypot(x - ax - dx * t, z - az - dz * t)
        if d < best[0]:
            best = (d, (dx / math.sqrt(ll), dz / math.sqrt(ll)))
    return best


RAVINE_DOWN = [[168, 96], [158, 120], [138, 144], [118, 160], [92, 168], [62, 170], [18, 174]]
for cz in range(fh):
    for cx in range(fw):
        x, z = cx - 16 + 0.5, cz - 16 + 0.5
        if not wet(x, z):
            continue
        i = (cz * fw + cx) * 4
        fall = math.hypot(x - FALL[0], z - FALL[1])
        ds, dir_s = along(STREAM, x, z)
        dr, dir_r = along(RAVINE_DOWN, x, z)
        if ds < 9 and height(x, z) > 2:
            speed, (fx, fz) = 0.55, dir_s
            foam = 40 + (150 if fall < 10 else 0)
        elif dr < 14:
            speed, (fx, fz) = 0.35, dir_r
            foam = max(0, int(230 - fall * 7))
        else:
            continue
        flow[i] = max(0, min(255, int(128 + 127 * fx * speed)))
        flow[i + 1] = max(0, min(255, int(128 + 127 * fz * speed)))
        flow[i + 2] = max(0, min(255, foam))
n = fw * fh
planes = b''.join(bytes(flow[k::fsize]) for k in range(fsize))
with open(os.path.join(MISSION, 'terrain', 'water-flow.grid'), 'wb') as f:
    f.write(header + zlib.compress(planes, 9))
print('moved to', MISSION)
