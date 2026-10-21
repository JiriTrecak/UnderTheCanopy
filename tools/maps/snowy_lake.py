"""Authors Snowy Lake, a four-player map round a frozen lake, through the editor's MCP
tools: only what a person does with the block editor. Every corner is the same, turned a
quarter round the middle, so the four players start alike.

Three levels: each main on a paving plateau (level 3) in a corner, one ramp down to its
natural on a moss shelf (level 2) along the next edge, a ramp from the shelf down to the
lowland (level 1) that rings the lake. The lake in the middle is deep water with an ice
shelf along its shore and floes drifting on it (the biome's water), its outline drawn
cell by cell; an island of frosted giants stands at its heart, out of reach. Frozen
forest frames the playable area.

Run with the server up (`just mcp`): `python3 tools/maps/snowy_lake.py`. It creates the
map, so remove the package first to rebuild it."""
import math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'snowy-lake'
SIZE = 320
BORDER = 16
C = SIZE / 2
c = mcp.Client()


def turn(p, k):
    """`p` turned `k` quarter turns round the middle."""
    x, z = p
    for _ in range(k % 4):
        x, z = C - (z - C), C + (x - C)
    return [x, z]


def blocks(op, points, **kw):
    c.call('map_blocks', map=M, op=op, points=points, fill=True, **kw)


def ramp(top, foot, level, radius=1.8):
    c.call('map_blocks', map=M, op='ramp', **{'from': top}, to=foot, radius=radius, level=level)


def circle(cx, cz, r, n=28):
    return [[cx + r * math.cos(a * 2 * math.pi / n), cz + r * math.sin(a * 2 * math.pi / n)]
            for a in range(n)]


c.call('create_map', id=M, name='Snowy Lake', size=SIZE, biome='frozen-dream', folder='skirmish')
D = f'maps/skirmish/{M}.utcm/map.json'
c.call('edit', label='No starts yet; the playable area', commands=[
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/playable',
     'value': {'x': BORDER, 'z': BORDER, 'width': SIZE - 2 * BORDER, 'height': SIZE - 2 * BORDER}},
])
c.call('map_blocks', map=M, op='flat')

# Frozen forest everywhere, open ground inside the playable area but for a wandering
# edge and four forest tongues between the bases.
c.call('map_blocks', map=M, op='paint', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9], kind='wall')
edge = [[24, 22], [84, 18], [128, 24], [160, 20], [196, 26], [240, 18], [298, 24], [302, 84],
        [296, 128], [300, 160], [294, 200], [302, 240], [296, 298], [236, 302], [192, 296],
        [160, 300], [124, 294], [80, 302], [22, 296], [18, 236], [24, 192], [20, 160],
        [26, 120], [18, 80]]
blocks('paint', edge, kind='ground')
for k in range(4):
    tongue = [turn(p, k) for p in [[18, 140], [52, 132], [66, 150], [54, 168], [18, 176]]]
    blocks('paint', tongue, kind='wall')

for k in range(4):
    # The main: a paving plateau in the corner.
    main = [turn(p, k) for p in [[30, 30], [100, 30], [108, 44], [108, 88], [94, 102], [44, 102], [30, 88]]]
    blocks('level', main, level=3)
    # The natural: a moss shelf along the next edge.
    shelf = [turn(p, k) for p in [[108, 26], [172, 26], [186, 40], [186, 74], [164, 92], [124, 92], [108, 80]]]
    blocks('level', shelf, level=2)
    blocks('level', main, level=3)

# Round every plateau's corners at 45°.
c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9])

# The lake: deep water cell by cell (a round shore, not a staircase of blocks), an
# island at its heart.
def cells(kind, points):
    c.call('map_blocks', map=M, op='fill', points=points, kind=kind)


cells('deep', circle(C, C, 48, 40))
cells('ground', circle(C, C, 13, 24))

for k in range(4):
    # Main down to its shelf, shelf down to the lowland.
    ramp(turn([102, 66], k), turn([118, 66], k), level=3)
    ramp(turn([160, 84], k), turn([160, 100], k), level=2)
    # The shelf falls to the lowland as a bank of roots; the main stands on worked stone.
    shelf = [turn(p, k) for p in [[108, 26], [172, 26], [186, 40], [186, 74], [164, 92], [124, 92], [108, 80]]]
    blocks('face', shelf, style=1)

# Players, amber and camps, a quarter turn apart.
for k in range(4):
    print(c.call('map_place', map=M, kind='start', x=turn([64.5, 62.5], k)[0], z=turn([64.5, 62.5], k)[1]))
for k in range(4):
    for p in ([150, 46], [112, 128]):
        q = turn(p, k)
        print(c.call('map_place', map=M, kind='amber', x=q[0], z=q[1]))
    for p, tier in (([138, 112], 'medium'), ([196, 60], 'easy')):
        q = turn(p, k)
        print(c.call('map_place', map=M, kind='camp', x=q[0], z=q[1], tier=tier))
for k in range(4):
    q = turn([C, 104], k)
    print(c.call('map_place', map=M, kind='camp', x=q[0], z=q[1], tier='hard'))
# The island's frosted giants.
SC = f'maps/skirmish/{M}.utcm/scenery.json'
giants = [{'id': f'island.{i}', 'asset': asset, 'x': C + dx, 'z': C + dz, 'yaw': yaw, 'scale': scale}
          for i, (asset, dx, dz, yaw, scale) in enumerate([
              ('scenery.frost-pine-a', -3.0, -2.0, 0.4, 1.6),
              ('scenery.frost-pine-b', 4.0, 1.0, 1.9, 1.4),
              ('scenery.mega-mushroom', 0.5, 5.0, 2.7, 0.45),
              ('scenery.frost-pine-sapling', -5.0, 4.0, 0.9, 1.2),
          ])]
c.call('edit', label='The island', commands=[
    {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': giants}])
print(c.call('problems'))
print(c.call('save'))
