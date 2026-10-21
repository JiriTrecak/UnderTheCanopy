"""Authors Frozen Dream, a two-player map after StarCraft II's Ever Dream LE (Jiri's
reference picture), through the editor's MCP tools: only what a person does with the
block editor, with point symmetry on, so every edit of the north-east half is made in
the south-west too.

Three levels, as Ever Dream reads them: the mains on paved platforms (level 3) in the
north-east and south-west corners; one ramp from each down to a shelf of frozen moss
(level 2) along its edge of the map, holding the natural and the third; ramps from the
shelf down to the dusty lowland (level 1) that winds round the map, with a moss pocket
in each outer arm and a moss island in the middle. Everything outside the playable
outline, and two islands that shape the lanes, is frozen forest.

Run with the server up (`just mcp`): `python3 tools/maps/frozen_dream.py`. It creates the
map, so remove the package first to rebuild it."""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'frozen-dream'
SIZE = 256
c = mcp.Client()


def blocks(op, points, **kw):
    """`op` over the blocks inside the outline `points` (world cells)."""
    c.call('map_blocks', map=M, op=op, points=points, fill=True, **kw)


def cells(kind, points):
    """Paints the cells inside the outline `points` a kind."""
    c.call('map_blocks', map=M, op='fill', points=points, kind=kind)


def ramp(top, foot, radius=1.8, level=None):
    """A ramp from `top` down to `foot`."""
    kw = {} if level is None else {'level': level}
    c.call('map_blocks', map=M, op='ramp', **{'from': top}, to=foot, radius=radius, **kw)


def mirror(p):
    return [SIZE - p[0], SIZE - p[1]]


c.call('create_map', id=M, name='Frozen Dream', size=SIZE, biome='frozen-dream', folder='skirmish')
D = 'maps/skirmish/frozen-dream.utcm/map.json'
c.call('edit', label='No starts yet', commands=[
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
])
c.call('map_blocks', map=M, op='flat', symmetry='point')

# Frozen forest everywhere; the playable outline, in blocks and then cell by cell for its
# edge (half drawn, half mirrored).
c.call('map_blocks', map=M, op='paint', **{'from': [-15.9, -15.9]}, to=[271.9, 271.9], kind='wall')
outline_east = [
    [128, 128], [128, 10], [150, 14], [168, 9], [200, 12], [232, 10], [246, 24], [247, 60],
    [243, 96], [248, 132], [245, 172], [238, 204], [222, 222], [190, 226], [160, 236],
    [128, 246],
]
outline_west = [
    [128, 128], [128, 10], [100, 15], [72, 11], [44, 16], [22, 24], [12, 44], [16, 76],
    [10, 104], [14, 128],
]
for outline in (outline_east, outline_west):
    blocks('paint', outline, kind='ground')
cells('ground', [
    [128, 128], [128, 10], [150, 14], [168, 9], [200, 12], [232, 10], [246, 24], [247, 60],
    [243, 96], [248, 132], [245, 172], [238, 204], [222, 222], [190, 226], [160, 236],
    [128, 246],
])
cells('ground', [
    [128, 128], [128, 10], [100, 15], [72, 11], [44, 16], [22, 24], [12, 44], [16, 76],
    [10, 104], [14, 128],
])
# A forest island that shapes the lanes east of the middle.
cells('wall', [[166, 140], [184, 134], [194, 148], [186, 164], [168, 162], [160, 150]])

# The main: a paved platform in the north-east corner.
main = [[150, 16], [214, 16], [226, 30], [226, 58], [208, 74], [166, 74], [150, 60]]
blocks('level', main, level=3)
# The moss shelf along the east edge: the natural beside the main, the third below it.
shelf = [[226, 22], [248, 26], [248, 70], [246, 120], [248, 150], [242, 168], [224, 174],
         [206, 162], [206, 140], [196, 124], [178, 110], [176, 74], [208, 74], [226, 58]]
blocks('level', shelf, level=2)
blocks('level', main, level=3)
# A moss pocket in the north-west arm, and the moss island in the middle.
pocket = [[72, 46], [96, 40], [116, 48], [122, 64], [110, 86], [88, 92], [72, 80]]
blocks('level', pocket, level=2)
island = [[104, 116], [118, 102], [140, 100], [156, 112], [152, 140], [138, 154], [116, 156],
          [100, 144]]
blocks('level', island, level=2)

# Round every plateau's corners at 45°.
c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[271.9, 271.9])

# One ramp from the main down to the shelf; ramps from the shelf down to the lowland;
# the pocket's and the island's ramps.
ramp([214, 62], [228, 78], radius=1.8, level=3)
ramp([184, 110], [168, 124], radius=1.8, level=2)
ramp([216, 166], [206, 182], radius=1.8, level=2)
ramp([96, 88], [100, 104], radius=1.8, level=2)
ramp([150, 114], [164, 100], radius=1.8, level=2)

# The shelf and the moss falls to the lowland as banks of roots; the mains stand on worked
# stone (the biome's own cliff).
for outline in (shelf, pocket, island):
    blocks('face', outline, style=1)
blocks('unface', main)

# Players, amber and camps (placed one by one: placements are not mirrored by MCP).
for p in ([188.5, 42.5], mirror([188.5, 42.5])):
    print(c.call('map_place', map=M, kind='start', x=p[0], z=p[1]))
sites = [(234, 84), (234, 146), (96, 64), (60, 30), (196, 130)]
for (x, z) in sites:
    for p in ([x, z], mirror([x, z])):
        print(c.call('map_place', map=M, kind='amber', x=p[0], z=p[1]))
print(c.call('map_place', map=M, kind='camp', x=128, z=128, tier='hard'))
for (p, tier) in [((100, 150), 'medium'), ((172, 126), 'easy')]:
    for q in (list(p), mirror(list(p))):
        print(c.call('map_place', map=M, kind='camp', x=q[0], z=q[1], tier=tier))
print(c.call('problems'))
print(c.call('save'))
