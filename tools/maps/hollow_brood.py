"""Authors The Hollow Brood, the ant campaign's first mission (docs/story.md), through the
editor's MCP tools: the terrain only (blocks); the mission's units, players, cameras and
script are added by the campaign's own step.

A journey, Warcraft III's first-mission shape: the Queen's escort comes in from the
south-west along a root path beside a stream, climbs a mossy terrace, comes down its far
side and descends into the Hollow, a sunken basin in the north-east where the stricken
brood's nest stands. Side clearings off the path hold feral camps, a sick pool, the
survivors' chamber and a lost patrol. Forest walls frame the path so it reads as a road.

Run with the server up (`just mcp`): `python3 tools/maps/hollow_brood.py`. It creates the
map, so remove the package first to rebuild it."""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'hollow-brood'
FOLDER = os.environ.get('FOLDER', 'showcase')
BORDER = 32
PLAY = 224
SIZE = PLAY + 2 * BORDER
LO, HI = BORDER, BORDER + PLAY
c = mcp.Client()


def blocks(op, points, **kw):
    c.call('map_blocks', map=M, op=op, points=points, **kw)


def clearing(x, z, r, n=14):
    import math
    return [[x + r * math.cos(a * 2 * math.pi / n), z + r * math.sin(a * 2 * math.pi / n)]
            for a in range(n)]


c.call('create_map', id=M, name='The Hollow Brood', size=SIZE, biome='frozen-dream', folder=FOLDER)
D = f'maps/{FOLDER}/{M}.utcm/map.json'
c.call('edit', label='The playable area; no starts, units or camps yet', commands=[
    {'op': 'set', 'document': D, 'pointer': '/playable',
     'value': {'x': BORDER, 'z': BORDER, 'width': PLAY, 'height': PLAY}},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
])
c.call('map_blocks', map=M, op='flat')

# Forest everywhere; the road and its clearings are carved out of it.
c.call('map_blocks', map=M, op='paint', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9], kind='wall')
road = [[52, 240], [84, 228], [110, 214], [128, 192], [140, 166], [152, 146], [172, 128],
        [190, 112], [204, 98]]
blocks('paint', road, kind='ground', radius=2.2)
for x, z, r in [(54, 238, 18),    # the start, where the escort fades in
                (152, 224, 16),   # the first feral camp, east of the road
                (96, 168, 15),    # the sick pool, north-west of the terrace
                (62, 118, 13),    # the lost patrol
                (236, 172, 15),   # the survivors' chamber
                (156, 138, 12)]:  # a feral picket on the terrace's far side
    blocks('paint', clearing(x, z, r), kind='ground', fill=True)
for spur in ([[110, 214], [132, 222], [152, 224]],
             [[118, 200], [104, 184], [96, 168]],
             [[96, 168], [78, 142], [62, 118]],
             [[172, 128], [204, 150], [236, 172]]):
    blocks('paint', spur, kind='ground', radius=1.6)

# The stream along the first leg, south of the road.
c.call('map_blocks', map=M, op='fill', kind='shallow',
       points=[[34, 250], [64, 252], [96, 244], [128, 250], [160, 256], [160, 262], [126, 258],
               [96, 252], [64, 260], [34, 258]])

# The mossy terrace the road climbs and leaves.
terrace = [[118, 204], [150, 200], [176, 176], [196, 146], [198, 120], [172, 106], [140, 120],
           [120, 150], [112, 180]]
blocks('level', terrace, level=2, fill=True)

# The Hollow: a sunken basin in the north-east, ringed by forest, one way in.
hollow = clearing(214, 74, 40, 20)
blocks('paint', hollow, kind='ground', fill=True)
blocks('level', hollow, level=0, fill=True)
c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9])

# Ramps: up onto the terrace, down off it, down into the Hollow.
c.call('map_blocks', map=M, op='ramp', **{'from': [132, 188]}, to=[122, 200], radius=1.8, level=2)
c.call('map_blocks', map=M, op='ramp', **{'from': [182, 120]}, to=[192, 108], radius=1.8, level=2)
c.call('map_blocks', map=M, op='ramp', **{'from': [196, 104]}, to=[188, 114], radius=1.8, level=1)
c.call('map_blocks', map=M, op='ramp', **{'from': [198, 102]}, to=[206, 92], radius=1.8, level=1)

# Still pools of over-bright substance in the Hollow and at the sick pool.
for x, z, r in [(198, 66, 5), (232, 96, 4), (96, 164, 5)]:
    c.call('map_blocks', map=M, op='fill', kind='shallow', points=clearing(x, z, r, 10))

print(c.call('problems'))
# A mission has no skirmish starts; the campaign's checks judge it.
print(c.call('save', force=True))
