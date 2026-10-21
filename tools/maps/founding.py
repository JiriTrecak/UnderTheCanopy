"""Authors Founding, the ant campaign's second mission (docs/story.md), through the editor's
MCP tools: the terrain only (blocks); units, players, cameras and the script come from the
campaign's own step.

A lush glade in the middle, where the Queen founds her colony, with amber and a substance
spring; the feral brood's burrows on a ridge in the north-east beyond a stream, and a
smaller den in the north-west across a ravine, reached over a narrow root bridge. Two ways
into the glade: the north-east ramp and the north-west bridge.

Run with the server up (`just mcp`): `python3 tools/maps/founding.py`. It creates the map,
so remove the package first to rebuild it."""
import math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'founding'
FOLDER = os.environ.get('FOLDER', 'showcase')
BORDER = 32
PLAY = 256
SIZE = PLAY + 2 * BORDER
c = mcp.Client()


def blocks(op, points, **kw):
    c.call('map_blocks', map=M, op=op, points=points, **kw)


def circle(x, z, r, n=18):
    return [[x + r * math.cos(a * 2 * math.pi / n), z + r * math.sin(a * 2 * math.pi / n)]
            for a in range(n)]


c.call('create_map', id=M, name='Founding', size=SIZE, biome='frozen-dream', folder=FOLDER)
D = f'maps/{FOLDER}/{M}.utcm/map.json'
c.call('edit', label='The playable area; no starts, units or camps yet', commands=[
    {'op': 'set', 'document': D, 'pointer': '/playable',
     'value': {'x': BORDER, 'z': BORDER, 'width': PLAY, 'height': PLAY}},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
])
c.call('map_blocks', map=M, op='flat')
c.call('map_blocks', map=M, op='paint', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9], kind='wall')

# The glade, open and generous, with a southern arrival meadow.
blocks('paint', circle(160, 182, 58, 24), kind='ground', fill=True)
blocks('paint', circle(150, 254, 22), kind='ground', fill=True)
# The north-east: a stream, then the ridge with the feral burrows.
blocks('paint', [[200, 140], [236, 108], [262, 80]], kind='ground', radius=3)
ridge = circle(256, 76, 34, 20)
blocks('paint', ridge, kind='ground', fill=True)
blocks('level', ridge, level=2, fill=True)
c.call('map_blocks', map=M, op='fill', kind='shallow',
       points=[[196, 96], [226, 120], [252, 150], [288, 172], [288, 182], [248, 160],
               [220, 130], [190, 104]])
# The north-west: a ravine of deep water and the root bridge to the second den.
den = circle(78, 76, 26, 16)
blocks('paint', den, kind='ground', fill=True)
c.call('map_blocks', map=M, op='fill', kind='deep',
       points=[[34, 128], [90, 118], [140, 104], [160, 70], [168, 36], [184, 36], [176, 76],
               [150, 120], [96, 134], [34, 144]])
blocks('paint', [[104, 150], [96, 126], [86, 102]], kind='ground', radius=1.6)
# Forest tongues that shape the two approaches.
blocks('paint', [[214, 210], [236, 230], [262, 250]], kind='wall', radius=3)
blocks('paint', [[96, 214], [74, 236]], kind='wall', radius=3)
c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9])
c.call('map_blocks', map=M, op='ramp', **{'from': [234, 98]}, to=[222, 112], radius=2, level=2)
# The substance spring at the glade's east edge.
c.call('map_blocks', map=M, op='fill', kind='shallow', points=circle(206, 206, 6, 10))

print(c.call('problems'))
# A mission has no skirmish starts; the campaign's checks judge it.
print(c.call('save', force=True))
