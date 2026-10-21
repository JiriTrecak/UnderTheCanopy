"""Authors Underleaf, the look-dev map of the Underleaf biome (docs/art-direction.md), through
the editor's MCP tools: the block operations a person uses in the editor, then the set
dressing a map author places from the Place catalog. It shows the biome's range across one
map, west to east, as the first ant missions use it:

- **The Hollow** (west): a sunken basin (level 0) under banks of roots, one ramp down into
  it, a stricken colony's ground: withered patches, bleached mushrooms and still pools of
  over-bright substance.
- **The Glade** (middle): open soil and moss at level 1, a mossy terrace (level 2) behind
  a stone cliff with a ramp, a tea-dark stream wading across it with a deep pool, moss
  cushions, blossoms, a mushroom ring.
- **The Glass Road** (east): beetle territory on two terraces of cut masonry (levels 2 and
  3), a paved road climbing them from the glade, lanterns, glass tanks of substance, a
  pipe run, and the condenser on top.
- The frame is forest floor all round, with two tongues of forest between the regions and
  a rock outcrop at the glade's south.

Run with your own server up (`./target/release/canopy mcp --data "$PWD"`, after checking
that port 7431 is free or yours): `python3 tools/maps/underleaf_lookdev.py`. It creates the
map, so remove the package first to rebuild it."""
import math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'underleaf-lookdev'
SIZE = 320
BORDER = 16
c = mcp.Client()


def blocks(op, points, **kw):
    c.call('map_blocks', map=M, op=op, points=points, fill=True, **kw)


def line(op, points, radius, **kw):
    c.call('map_blocks', map=M, op=op, points=points, radius=radius, **kw)


def cells(kind, points):
    c.call('map_blocks', map=M, op='fill', points=points, kind=kind)


def ramp(top, foot, level, radius=1.6):
    c.call('map_blocks', map=M, op='ramp', **{'from': top}, to=foot, radius=radius, level=level)


def circle(cx, cz, r, n=28, squash=1.0, wobble=0.0, seed=0):
    return [[cx + r * (1 + wobble * math.sin(3 * a + seed)) * math.cos(a),
             cz + r * squash * (1 + wobble * math.sin(3 * a + seed)) * math.sin(a)]
            for a in (k * 2 * math.pi / n for k in range(n))]


import time  # noqa: E402
for attempt in range(60):  # a fresh server checks the package first
    try:
        c.call('create_map', id=M, name='Underleaf', size=SIZE, biome='underleaf', folder='showcase')
        break
    except RuntimeError as error:
        if 'check' not in str(error):
            raise
        time.sleep(2)
D = f'maps/showcase/{M}.utcm/map.json'
c.call('edit', label='No starts yet; the playable area; late afternoon', commands=[
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/playable',
     'value': {'x': BORDER, 'z': BORDER, 'width': SIZE - 2 * BORDER, 'height': SIZE - 2 * BORDER}},
    {'op': 'set', 'document': D, 'pointer': '/landscape',
     'value': {'environment': {'hour': 16.5, 'playing': True}}},
])
c.call('map_blocks', map=M, op='flat')

# Forest floor everywhere; open ground inside a wandering outline.
c.call('map_blocks', map=M, op='paint', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9], kind='wall')
OPEN = [[26, 40], [60, 24], [104, 30], [140, 22], [184, 26], [230, 20], [276, 26], [298, 52],
        [294, 110], [300, 160], [294, 220], [298, 268], [270, 298], [220, 292], [176, 300],
        [132, 294], [92, 298], [50, 292], [24, 268], [30, 220], [22, 170], [28, 110]]
blocks('paint', OPEN, kind='ground')
cells('ground', OPEN)
# Forest tongues between the regions, and a rock outcrop.
cells('wall', [[112, 22], [134, 22], [138, 64], [128, 92], [116, 80]])
cells('wall', [[124, 300], [148, 300], [144, 262], [132, 240], [120, 258]])
cells('rock', circle(186, 262, 9, 18, wobble=0.15, seed=1))

# --- The Hollow: a sunken basin (level 0) under root banks, one ramp in from the east.
BASIN = circle(70, 160, 40, 36, squash=1.15, wobble=0.08, seed=2)
blocks('level', BASIN, level=0)
line('face', BASIN + [BASIN[0]], 1.2, style=1)
ramp([109.6, 160], [102.4, 160], level=1, radius=1.6)  # covers 98–114

# --- The Glade: a mossy shelf (level 2) above the Hollow's side behind a stone cliff, with
# two ramps up to it; a mossy stone terrace (level 3) in its north; a stream across it.
GLADE = [[124, 20], [224, 20], [224, 300], [124, 300]]
blocks('level', GLADE, level=2)
ramp([121.6, 120], [118.4, 120], level=2, radius=1.6)  # covers 112–128
ramp([121.6, 212], [118.4, 212], level=2, radius=1.6)
TERRACE = [[150, 30], [214, 30], [218, 70], [196, 92], [156, 88], [146, 60]]
blocks('level', TERRACE, level=3)
ramp([176, 94.4], [176, 97.6], level=3, radius=1.6)  # covers 88–104
STREAM = [[138, 18], [140, 60], [146, 104], [156, 140], [150, 176], [156, 214], [166, 252], [160, 302]]
c.call('map_blocks', map=M, op='cells', points=STREAM, radius=3.5, kind='shallow')
cells('deep', circle(152, 182, 10, 22, wobble=0.12, seed=3))

# --- The Glass Road: two terraces of beetle masonry (levels 3 and 4).
SHELF = [[226, 30], [292, 34], [296, 130], [292, 200], [296, 284], [230, 290], [222, 200], [226, 120]]
TOP = [[252, 112], [290, 112], [292, 208], [252, 208]]
blocks('level', SHELF, level=3)
blocks('level', TOP, level=4)
blocks('face', SHELF, style=2)
blocks('face', TOP, style=2)
ramp([221.6, 160], [218.4, 160], level=3, radius=1.6)  # covers 212–228: the brush reaches 1.6 blocks past both ends
ramp([245.6, 160], [242.4, 160], level=4, radius=1.6)  # covers 236–252

c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9])

# Two starts so the map plays: the founding queen in the glade, the beetles on their shelf.
print(c.call('map_place', map=M, kind='start', x=196.5, z=200.5))
print(c.call('map_place', map=M, kind='start', x=268.5, z=256.5))

# --- Set dressing (the Place catalog's pieces), as a map author places it.
P = []


def put(asset, x, z, yaw=0.0, scale=1.0):
    P.append({'id': f'dress.{len(P)}', 'asset': 'scenery.' + asset, 'x': x, 'z': z,
              'yaw': yaw, 'scale': scale})


# The Hollow
put('hollow-collapsed-mound', 60, 152, 0.6, 1.0)
put('underleaf-patch-withered-a', 74, 140, 0.4, 1.4)
put('underleaf-patch-withered-b', 84, 180, 2.1, 1.3)
put('underleaf-patch-withered-a', 44, 182, 4.0, 1.1)
put('underleaf-patch-withered-b', 52, 128, 5.1, 1.2)
put('underleaf-patch-withered-a', 92, 158, 1.0, 0.9)
put('substance-pool-large', 80, 166, 0.3)
put('substance-pool', 40, 150, 1.7)
put('substance-pool-small', 70, 134, 2.6)
put('substance-pool-small', 56, 184, 4.4)
put('substance-pool', 92, 142, 3.0, 0.8)
for i, (x, z, s) in enumerate([(84, 152, 3.2), (48, 170, 3.6), (94, 186, 2.6), (40, 136, 2.8)]):
    put('hollow-bleached-mushrooms', x, z, i * 1.7, s)
for i, (x, z, s) in enumerate([(66, 176, 3.0), (78, 128, 2.6)]):
    put('hollow-toppled-mushrooms', x, z, i * 2.3 + 0.5, s)
for i, (x, z, s) in enumerate([(36, 176, 3.6), (100, 136, 3.0), (70, 198, 3.8), (34, 148, 3.2), (100, 176, 3.4)]):
    put('hollow-withered-fern' if i % 2 else 'hollow-withered-fern-tall', x, z, i * 2.1, s)

# The Glade
put('underleaf-patch-moss-a', 196, 132, 0.2, 1.1)
put('underleaf-patch-moss-b', 214, 150, 1.4, 1.0)
put('underleaf-patch-moss-a', 182, 112, 3.1, 0.9)
put('underleaf-patch-blossoms', 132, 132, 0.8, 1.2)
put('underleaf-patch-blossoms', 170, 232, 2.0, 1.0)
put('underleaf-patch-leaves-a', 206, 236, 1.1, 1.0)
put('underleaf-patch-leaves-b', 128, 210, 2.6, 1.1)
put('glade-mushroom-ring', 200, 116, 0.0, 1.0)
put('lookdev-leaf-oak', 168, 158, 0.7, 0.7)
put('lookdev-acorn', 124, 166, 1.2, 0.8)
put('lookdev-litter-beech', 186, 168, 2.2, 1.0)
put('meadow-stick-a', 196, 182, 0.3, 2.0)
put('woodland-river-stones', 150, 128, 0.5, 2.0)
put('woodland-river-stones', 160, 222, 2.5, 1.8)

# The Glass Road: a road from the glade up both terraces to the condenser; each ramp of
# the road is four blocks long (one level over 16 cells), as its ramp piece is.
put('beetle-road-end', 184, 160, -math.pi / 2)
for x in (192, 200, 208, 232, 260, 268):
    put('beetle-road-straight', x, 160, math.pi / 2)
for x in (220, 244):
    put('beetle-road-ramp', x, 160, -math.pi / 2)  # climbing east
for x, z in ((210, 151), (210, 169), (232, 151), (232, 169), (270, 151), (270, 169)):
    put('beetle-lantern', x, z, 0.0)
put('beetle-substance-tank', 238, 134, 0.8)
put('beetle-substance-tank', 238, 186, 2.2)
put('beetle-substance-tank-small', 266, 136, 0.3)
put('beetle-substance-tank-small', 266, 184, 1.4)
for z in (124, 116):
    put('beetle-pipe-run', 284, z, 0.0)
put('beetle-condenser', 281, 160, -math.pi / 2)
# The station's wall round the top terrace, a gate where the road comes in.
for z in (126, 134, 142, 178, 186, 194):
    put('beetle-wall', 257, z, math.pi / 2)
for x in (265, 273, 281):
    put('beetle-wall', x, 118, 0.0)
    put('beetle-wall', x, 202, 0.0)
for x, z in ((257, 151), (257, 169), (257, 118), (257, 202), (287, 118), (287, 202)):
    put('beetle-wall-tower', x, z, 0.0)

import glob, json  # noqa: E402
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
known = {json.load(open(p))['id'] for p in glob.glob(os.path.join(ROOT, 'base', 'scenery', '**', '*.json'), recursive=True)}
missing = sorted({o['asset'] for o in P if o['asset'] not in known})
if missing:
    print('not placed (no such scenery yet):', missing)
P = [o for o in P if o['asset'] in known]
SC = f'maps/showcase/{M}.utcm/scenery.json'
c.call('edit', label='Set dressing', commands=[
    {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': P}])
print(c.call('problems'))
print(c.call('save'))
