"""Authors The Glass Road, the ant campaign's third mission (docs/story.md), through the
editor's MCP tools: the terrain only (blocks); units, players, cameras and the script come
from the campaign's own step.

The colony's forward camp in a wooded south-west corner; the Glass Road, a raised beetle
causeway, running from the west edge to the north-east, with two beetle outposts along it;
the walled beetle station with its condenser on a high plateau in the north-east; a lower
caravan track joining the road from the south-east.

Run with the server up (`just mcp`): `python3 tools/maps/glass_road.py`. It creates the
map, so remove the package first to rebuild it."""
import math, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'glass-road'
FOLDER = os.environ.get('FOLDER', 'showcase')
BORDER = 32
PLAY = 288
SIZE = PLAY + 2 * BORDER
c = mcp.Client()


def blocks(op, points, **kw):
    c.call('map_blocks', map=M, op=op, points=points, **kw)


def circle(x, z, r, n=18):
    return [[x + r * math.cos(a * 2 * math.pi / n), z + r * math.sin(a * 2 * math.pi / n)]
            for a in range(n)]


c.call('create_map', id=M, name='The Glass Road', size=SIZE, biome='frozen-dream', folder=FOLDER)
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

# Open lowland across the middle of the map, with woods left standing in clumps.
low = [[48, 300], [48, 150], [110, 110], [190, 96], [250, 150], [300, 200], [310, 300],
       [200, 316], [120, 316]]
blocks('paint', low, kind='ground', fill=True)
for x, z, r in [(150, 220, 16), (230, 250, 14), (96, 190, 12), (270, 220, 10)]:
    blocks('paint', circle(x, z, r), kind='wall', fill=True)
# The forward camp in the south-west, room for a colony.
blocks('paint', circle(84, 272, 34, 20), kind='ground', fill=True)

# The Glass Road: a causeway one level up, from the west edge to the station.
road = [[34, 168], [80, 156], [130, 150], [180, 140], [226, 118], [262, 96]]
blocks('paint', road, kind='ground', radius=2.4)
blocks('level', road, level=2, radius=2.4)
# The beetle station: walled plateau, one level above the road.
station = circle(284, 72, 34, 20)
blocks('paint', station, kind='ground', fill=True)
blocks('level', station, level=3, fill=True)
# Two outposts along the road.
for x, z in [(136, 132), (214, 104)]:
    pad = circle(x, z, 12, 12)
    blocks('paint', pad, kind='ground', fill=True)
    blocks('level', pad, level=2, fill=True)
c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9])

# Ramps: onto the causeway from the camp's side and the caravan track, up into the station.
c.call('map_blocks', map=M, op='ramp', **{'from': [100, 160]}, to=[100, 176], radius=2, level=2)
c.call('map_blocks', map=M, op='ramp', **{'from': [196, 136]}, to=[204, 152], radius=2, level=2)
c.call('map_blocks', map=M, op='ramp', **{'from': [258, 92]}, to=[248, 100], radius=1.8, level=3)

# A canal of still water along the road's north side.
c.call('map_blocks', map=M, op='fill', kind='shallow',
       points=[[60, 128], [130, 120], [180, 112], [190, 120], [130, 132], [60, 140]])

print(c.call('problems'))
# A mission has no skirmish starts; the campaign's checks judge it.
print(c.call('save', force=True))
