"""Authors Amberleaf Crossing through the editor's MCP map tools (docs/editor.md, M3).
Run with the server up (`just mcp`): `python3 tools/maps/amberleaf_crossing.py`, then
save with the MCP `save` tool. It creates the map, so remove the package first to rebuild it.
Player 1 holds the north-west, player 2 the south-east; everything is mirrored by a half
turn about the centre (R below), so both players have the same distances to everything.

It no longer runs: its brush strokes (MCP `map_stroke`) went with the brush editor (D81).
The map stays as it was saved; it is kept as the record of how the map was made."""
import sys, math, json, random, os; sys.path.insert(0, os.path.dirname(os.path.abspath(__file__))); import mcp

M = 'amberleaf-crossing'
N = 511  # R(x, z) = (N - x, N - z): keeps the 4-cell building lattice (x = 1.5 mod 4)
c = mcp.Client()

def R(p): return [N - p[0], N - p[1]]

def both(tool, points, **kw):
    """One stroke on player 1's side and its mirror on player 2's."""
    c.call('map_stroke', map=M, tool=tool, points=points, **kw)
    c.call('map_stroke', map=M, tool=tool, points=[R(p) for p in points], **kw)

def one(tool, points, **kw):
    c.call('map_stroke', map=M, tool=tool, points=points, **kw)

def disc(cx, cz, r, step=5):
    """Dab points covering a disc."""
    out = []
    for z in range(int(cz - r), int(cz + r) + 1, step):
        for x in range(int(cx - r), int(cx + r) + 1, step):
            if (x - cx) ** 2 + (z - cz) ** 2 <= r * r:
                out.append([x, z])
    return out

def arc(cx, cz, r, a0, a1, step=6):
    """Points along an arc (degrees, 0 = east, 90 = south)."""
    n = max(2, int(abs(a1 - a0) / 180 * math.pi * r / step))
    return [[cx + r * math.cos(math.radians(a0 + (a1 - a0) * i / n)),
             cz + r * math.sin(math.radians(a0 + (a1 - a0) * i / n))] for i in range(n + 1)]

RNG = random.Random(7)

def blotch(x, z, r, ground, strength=1.0, dabs=7):
    """An irregular patch: dabs of varied size scattered about a point (and its mirror)."""
    pts = []
    for _ in range(dabs):
        a, d = RNG.uniform(0, 2 * math.pi), RNG.uniform(0, r * 0.8)
        pts.append(([x + math.cos(a) * d, z + math.sin(a) * d], RNG.uniform(r * 0.35, r * 0.7)))
    for (p, rad) in pts:
        both('paint', [p], radius=rad, ground=ground, strength=strength, passes=2)

def path(points, ground='warcraft-ldrt', width=5.5):
    """A worn path: dabs of varied width along a jittered line."""
    dense = []
    for (a, b) in zip(points, points[1:]):
        n = max(1, int(math.dist(a, b) / 2))
        for i in range(n):
            t = i / n
            dense.append([a[0] + (b[0] - a[0]) * t + RNG.uniform(-1, 1),
                          a[1] + (b[1] - a[1]) * t + RNG.uniform(-1, 1)])
    dense.append(points[-1])
    for p in dense:
        both('paint', [p], radius=width * RNG.uniform(0.8, 1.25), ground=ground, strength=1, passes=2)

def place_both(kind, x, z, **kw):
    c.call('map_place', map=M, kind=kind, x=x, z=z, **kw)
    p = R([x, z])
    c.call('map_place', map=M, kind=kind, x=p[0], z=p[1], **kw)

# --- The map ---------------------------------------------------------------------------
c.call('problems')  # the first check, which create_map needs
print(c.call('create_map', id=M, name='Amberleaf Crossing', size=512, biome='vibrant-forest'))
c.call('map_remove', map=M, x=256, z=256, radius=400, what=['starts'])

HALL = [109.5, 121.5]          # player 1's start (player 2's is R(HALL))
PLATEAU = [100, 112, 58]       # main plateau: centre and radius

# Ground: gentle rolling lowland (height 2) with soft hills under the woods.
for (x, z, r) in [(200, 40, 30), (40, 200, 34), (165, 315, 26), (290, 40, 22), (150, 470, 24)]:
    both('raise', disc(x, z, r * 0.6, 8), radius=r * 0.6, strength=0.25)

F = dict(strength=1, passes=3)   # flatten fully

# Main plateaus: one cliff level up (5), a wide ramp east to the natural.
both('cliff', disc(PLATEAU[0], PLATEAU[1], PLATEAU[2] - 6, 6), radius=7, level=5)
both('ramp', [[140, 104], [176, 104]], radius=8)
# The fourth expansion's high ground in the far corner, its ramp towards the third.
both('cliff', disc(66, 458, 38, 6), radius=7, level=5)
both('ramp', [[92, 430], [116, 406]], radius=6)
# Watch hills on the way to the island, with ramps front and back.
both('cliff', disc(165, 252, 18, 5), radius=6, level=5)
both('ramp', [[165, 238], [165, 214]], radius=5)
both('ramp', [[165, 266], [165, 290]], radius=5)

# The river: from the north edge, an S through a central lake, out the south edge.
RIVER = [[330, -10], [332, 70], [318, 120], [305, 160], [280, 210], [256, 256]]
RIVER_FULL = RIVER + [R(p) for p in reversed(RIVER[:-1])]
one('flatten', RIVER_FULL, radius=13, level=-2.0, **F)
one('flatten', disc(256, 256, 34, 6), radius=8, level=-2.0, **F)
one('smooth', RIVER_FULL, radius=20, passes=3, strength=1)
# The island in the lake, and the shallow fords that reach it and cross the river.
one('flatten', disc(255.5, 255.5, 16, 4), radius=6, level=2.6, **F)
one('smooth', disc(255.5, 255.5, 20, 6), radius=6, strength=0.6)
both('flatten', [[250, 252], [206, 232]], radius=12, level=0.3, **F)
both('flatten', [[286, 106], [352, 132]], radius=13, level=0.3, **F)
one('water', RIVER_FULL, radius=24, level=0.5)
one('water', disc(256, 256, 44, 8), radius=10, level=0.5)
# Ponds by the third.
both('flatten', disc(34, 372, 10, 4), radius=6, level=-0.8, **F)
both('water', disc(34, 372, 14, 4), radius=6, level=0.5)

# --- Ground paint ----------------------------------------------------------------------
# Light grass over everything, then the rest on top.
one('paint', [[x, z] for z in range(-8, 520, 16) for x in range(-8, 520, 16)], radius=13,
    ground='warcraft-lgrs', strength=1, passes=2)
# Sand along the water.
both('paint', RIVER[:-1], radius=16, ground='warcraft-zsan', strength=1, passes=2)
one('paint', disc(256, 256, 40, 8), radius=10, ground='warcraft-zsan', strength=1, passes=2)
both('paint', disc(34, 372, 13, 6), radius=6, ground='warcraft-zsan', strength=1)
# Darker grass and grassy earth across the open ground, for variety.
for (x, z, r) in [(150, 60, 18), (230, 190, 22), (60, 330, 16), (300, 90, 14), (200, 470, 16),
                  (90, 250, 18), (330, 200, 14), (20, 100, 14)]:
    blotch(x, z, r, 'warcraft-lgrd', 1.0, dabs=8)
for (x, z, r) in [(200, 130, 12), (140, 380, 12), (260, 90, 10), (60, 400, 10), (110, 300, 10)]:
    blotch(x, z, r, 'warcraft-ldrg', 0.8, dabs=6)
# Sand marks the fords.
both('paint', [[250, 252], [206, 232]], radius=9, ground='warcraft-zsan', strength=1, passes=2)
both('paint', [[286, 106], [352, 132]], radius=10, ground='warcraft-zsan', strength=1, passes=2)
# Worn paths between the bases, expansions and crossings.
for road in [
    [[136, 104], [176, 104], [205, 92]],
    [[176, 104], [196, 160], [210, 234]],
    [[196, 160], [290, 110]],
    [[150, 160], [130, 214], [120, 300], [105, 330]],
    [[105, 330], [116, 406]],
]:
    path(road)
# Bare earth around the halls and the mines, rough ground at the camps.
blotch(HALL[0], HALL[1], 18, 'warcraft-ldrt', 0.9, dabs=8)
for (x, z) in [(226, 78), (78, 348), (58, 466)]:
    blotch(x, z, 12, 'warcraft-ldrt', 0.9, dabs=6)
for (x, z) in [(238, 112), (118, 198), (40, 300), (112, 352), (165, 252), (360, 100), (84, 440)]:
    blotch(x, z, 8, 'warcraft-ldro', 0.9, dabs=5)
# Rock rims on the high ground, and rocky outcrops.
both('paint', arc(PLATEAU[0], PLATEAU[1], PLATEAU[2] - 3, 0, 360, 8), radius=4, ground='warcraft-lrok', strength=0.8)
both('paint', arc(66, 458, 35, 0, 360, 8), radius=4, ground='warcraft-lrok', strength=0.8)
both('paint', arc(165, 252, 15, 0, 360, 6), radius=3, ground='warcraft-lrok', strength=0.8)
for (x, z) in [(250, 40), (20, 160), (190, 390), (280, 170)]:
    blotch(x, z, 9, 'warcraft-lrok', 0.9, dabs=5)

# --- Forests ---------------------------------------------------------------------------
PINE, PINE2, OAK, GOLD = 'woodland-pine-a', 'woodland-pine-b', 'canopy-oak', 'autumn-tree-gold'
# The map's edge.
EDGE = [[5, 5], [5, 506], [506, 506], [506, 5], [5, 5]]
one('trees', EDGE, radius=5, density=14, tree=PINE)
# Behind each main: the woods the workers cut, close to the hall on its far side.
for r, t in ((30, PINE), (37, PINE2), (44, PINE)):
    both('trees', arc(HALL[0], HALL[1], r, 150, 300), radius=4, density=20, tree=t)
both('trees', disc(30, 30, 24, 6), radius=6, density=16, tree=PINE2)
# Woods that shape the lanes: overlapping discs, dark grass beneath.
WOODS = [
    [(200, 40, 18), (225, 30, 12), (175, 30, 10)],           # north of the natural
    [(40, 200, 20), (30, 235, 14), (45, 170, 12)],           # west of the main
    [(160, 320, 12), (185, 335, 10)],                        # behind the watch hill
    [(290, 40, 12), (300, 64, 8)],                           # by the north river
    [(150, 470, 14), (178, 480, 10)],                        # south of the fourth
    [(250, 150, 9), (236, 170, 7)],                          # between the lanes
    [(60, 300, 10), (72, 280, 8)],                           # west edge
    [(372, 50, 12), (395, 40, 9)],                           # beyond the river ford
    [(200, 420, 9), (215, 445, 8)],                          # by the south river
]
for i, wood in enumerate(WOODS):
    for (x, z, r) in wood:
        blotch(x, z, r + 6, 'warcraft-lgrd', 1.0, dabs=6)
        both('trees', disc(x, z, r, 5), radius=5, density=16, tree=[PINE, PINE2][i % 2])
        # Oaks and a few golden trees along the wood's edge.
        both('trees', arc(x, z, max(r - 3, 2), 0, 360, 7), radius=2.5, density=8, tree=GOLD if i % 4 == 0 else OAK)
# Pines crown the watch hills and thicken the mains' woods.
both('trees', arc(165, 252, 13, 200, 340, 4), radius=2, density=14, tree=PINE2)
both('trees', arc(HALL[0], HALL[1], 48, 160, 290), radius=3, density=8, tree=PINE2)

# --- Decoration (blocks nothing) -------------------------------------------------------
def props(points, prop, density, radius=6):
    both('props', points, radius=radius, density=density, prop=prop)
# Along the water: cattails, reeds, river stones, boulders on the lake shore.
props(RIVER[1:-1], 'shore-cattails', 6, radius=19)
props(RIVER[1:-1], 'woodland-reed-tuft', 5, radius=20)
props(arc(256, 256, 43, 0, 360, 10), 'meadow-boulder', 2, radius=5)
props(arc(256, 256, 40, 0, 360, 8), 'shore-cattails', 8, radius=4)
props(arc(34, 372, 15, 0, 360, 6), 'shore-cattails', 10, radius=4)
# Meadows: flowers, clover and tall grass in drifts.
for (x, z, r, prop) in [(150, 60, 14, 'pl-flowers-yellow'), (230, 190, 16, 'pl-flowers-white'),
                        (60, 330, 12, 'pl-flowers-bluebell'), (300, 90, 10, 'pl-daisy'),
                        (90, 250, 12, 'pl-clover-a'), (200, 470, 12, 'pl-flowers-blue'),
                        (130, 150, 10, 'pl-grass-tall'), (60, 450, 10, 'pl-flowers-trillium'),
                        (330, 200, 10, 'pl-grass-lush-a'), (20, 100, 10, 'woodland-grass-high-a')]:
    props(disc(x, z, r, 6), prop, 5, radius=6)
# The woods' edges: ferns, mushrooms, logs and mossy boulders.
for i, wood in enumerate(WOODS):
    for (x, z, r) in wood:
        props(arc(x, z, r + 5, 0, 360, 8), ['pl-fern-bushy', 'pl-fern-sword', 'woodland-mushroom-cluster'][i % 3], 4, radius=3)
    x, z, r = wood[0]
    props([[x + r + 6, z]], ['woodland-fallen-log', 'woodland-moss-boulder', 'woodland-hollow-log'][i % 3], 25, radius=3)
# Rocky outcrops and the mains' rims.
for (x, z) in [(250, 40), (20, 160), (190, 390), (280, 170)]:
    props(disc(x, z, 7, 4), 'meadow-boulder', 6, radius=4)
    props(disc(x, z, 9, 4), 'meadow-pebble-a', 6, radius=4)
props(arc(HALL[0], HALL[1], 26, 140, 310, 6), 'woodland-pine-stump', 4, radius=3)

# --- Starts, amber and camps -----------------------------------------------------------
c.call('map_place', map=M, kind='start', x=HALL[0], z=HALL[1])
c.call('map_place', map=M, kind='start', x=R(HALL)[0], z=R(HALL)[1])
place_both('amber', 226, 78)          # natural
place_both('amber', 78, 348)          # third
place_both('amber', 58, 466)          # fourth, on the corner's high ground
CAMPS = [
    (238, 112, 'easy'),    # guards the natural
    (118, 198, 'easy'),    # below the main
    (40, 300, 'easy'),     # west edge
    (112, 352, 'medium'),  # guards the third
    (165, 252, 'medium'),  # on the watch hill
    (360, 100, 'medium'),  # across the river ford (each player's far bank)
    (84, 440, 'hard'),     # guards the fourth
]
for (x, z, tier) in CAMPS:
    place_both('camp', x, z, tier=tier)
c.call('map_place', map=M, kind='camp', x=255.5, z=255.5, tier='hard')   # the island

print(json.dumps(c.call('problems'), indent=1))
info = c.call('map_info', map=M)
print('unreachable', info['unreachableFromPlayer1'])
print('starts', info['starts'], 'camps', len(info['camps']), 'trees', info['trees'])
os.makedirs('target/maps', exist_ok=True)
c.picture(M, 'target/maps/amberleaf-crossing-picture.png', scale=2)
open('target/maps/amberleaf-crossing-preview.png', 'wb').write(c.call('map_preview', map=M))
