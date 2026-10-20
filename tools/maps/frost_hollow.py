"""Authors Frost Hollow, the block editor's look-dev scene (D80, the target frame
`art/styles/target-frost-cliff.jpg`), through the editor's MCP tools: only what a person
does with the block editor, raising, painting, ramps and cuts. Everything you see in it is
generated from those blocks by the `frost-hollow` biome's recipe.

Run with the server up (`just mcp`): `python3 tools/maps/frost_hollow.py`, then save with
the MCP `save` tool. It creates the map, so remove the package first to rebuild it.

The scene sits around (128, 128): a frosted plateau (level 2) whose east edge runs down a
diagonal and then straight south, a frozen pond (level 1) east of it, a root bank across
the plateau's north-west, a mushroom frame to the north-east, a leaf frame to the south-west
and a rubble ramp at the cliff's corner down to the ice. Elsewhere the map is open ground
for the start."""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'frost-hollow'
c = mcp.Client()
E = 0.05  # keeps an area inside the blocks it names
# The layout is drawn at the warrior's scale in the target and scaled about it: K blocks
# here for every block of the drawing, so the scene stands as large against a unit as the
# target's does.
K = 1.4
AX, AZ = 128, 128


def T(x, z):
    """A drawing point, scaled about the warrior and snapped to the block lattice."""
    s = lambda v, a: a + round((v - a) * K / 4) * 4
    return s(x, AX), s(z, AZ)


def blocks(op, x0, z0, x1, z1, **kw):
    """`op` over the blocks covering drawing cells x0..x1, z0..z1."""
    (x0, z0), (x1, z1) = T(x0, z0), T(x1, z1)
    c.call('map_blocks', map=M, op=op, **{'from': [x0 + E, z0 + E]}, to=[x1 - E, z1 - E], **kw)


def cut(x, z):
    """Cuts the south-east corner of the block whose north-west corner is drawing (x, z)."""
    x, z = T(x, z)
    c.call('map_blocks', map=M, op='cut', **{'from': [x + 1, z + 1]}, corner=[x + 3.9, z + 3.9])


c.call('create_map', id=M, name='Frost Hollow', size=256, biome='frost-hollow', folder='showcase')
c.call('map_blocks', map=M, op='flat')

# The frame everywhere around the scene; open ground in the west for the start.
blocks('paint', -16, -16, 272, 272, kind='wall')
blocks('paint', 8, 8, 72, 248, kind='ground')

# The pond, a level below the plateau.
blocks('level', 136, 88, 164, 180, level=1)
blocks('paint', 136, 88, 164, 180, kind='ice')

# The plateau and the frame around it.
blocks('level', 60, 72, 140, 180, level=2)
blocks('paint', 76, 88, 140, 180, kind='ground')
# Its east edge: a diagonal up to the north-east (one block east per block north, each
# step's outer corner cut), then south, stepping west once.
(ex, ez) = T(140, 124)
for k in range(1, 5):
    x0, z0 = ex, ez - 4 * k
    x1 = ex + 4 * k
    c.call('map_blocks', map=M, op='level', **{'from': [x0 + E, z0 + E]}, to=[x1 - E, z0 + 4 - E], level=2)
    c.call('map_blocks', map=M, op='paint', **{'from': [x0 + E, z0 + E]}, to=[x1 - E, z0 + 4 - E], kind='ground')
    c.call('map_blocks', map=M, op='cut', **{'from': [x1 - 3, z0 + 1]}, corner=[x1 - 0.1, z0 + 3.9])
blocks('level', 136, 152, 140, 180, level=1)
blocks('paint', 136, 152, 140, 180, kind='ice')
cut(136, 148)

# A rubble ramp at the cliff's corner, down to the ice.
blocks('ramp', 136, 124, 140, 128)

# The root bank west of the warrior; the mushroom frame north-east; the leaf frame
# south-west; the far bank beyond the pond.
blocks('paint', 84, 112, 124, 128, kind='wall')
blocks('level', 136, 88, 152, 112, level=2)
blocks('paint', 136, 88, 152, 112, kind='wall')
blocks('paint', 60, 132, 108, 180, kind='wall')
blocks('paint', 160, 88, 180, 180, kind='rock')
# Corners cut so the frames' outlines run at 45° rather than in boxes.
def cut_at(x, z, corner):
    """Cuts the given corner (dx, dz in 0/1) of the block at drawing point (x, z)."""
    x, z = T(x, z)
    c.call('map_blocks', map=M, op='cut', **{'from': [x + 1, z + 1]}, corner=[x + 0.1 + 3.8 * corner[0], z + 0.1 + 3.8 * corner[1]])
cut_at(84, 112, (0, 0))
cut_at(84, 124, (0, 1))
cut_at(120, 112, (1, 0))
cut_at(104, 132, (1, 0))
cut_at(104, 176, (1, 1))
# One player: a sandbox scene with a warrior at the cliff, its start in the open west.
D = 'maps/showcase/frost-hollow.utcm/map.json'
c.call('edit', label='Frost Hollow: one player, a warrior', commands=[
    {'op': 'set', 'document': D, 'pointer': '/sandbox', 'value': True},
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': [{
        'mainFort': 'start.player.1/main-fort', 'player': 1, 'setup': 'setup.ants', 'x': 41.5, 'z': 129.5}]},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': [{
        'definition': 'unit.ants.warrior', 'id': 'frost.warrior', 'owner': 'player.1',
        'position': {'x': 129.0, 'y': 131.0}, 'rotation': 200.0}]},
])
# Landmarks placed by hand, as the editor's props place them: the acorn at the root
# bank's end and the mushrooms north-east.
SC = 'maps/showcase/frost-hollow.utcm/scenery.json'
def obj(i, asset, x, z, scale, yaw=0.0):
    x, z, scale = AX + (x - AX) * K, AZ + (z - AZ) * K, scale * K
    return {'asset': asset, 'elevation': 0.0, 'heightMode': 'terrain', 'id': 'frost.%d' % i, 'locked': False,
            'scale': scale, 'visible': True, 'x': x, 'yaw': yaw, 'z': z}
c.call('edit', label='Frost Hollow: landmarks', commands=[
    {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': [
        obj(0, 'scenery.lookdev-acorn', 123.5, 124.0, 1.35, 0.6),
        obj(1, 'scenery.lookdev-mushrooms', 143.0, 104.0, 0.6, 1.2),
        obj(2, 'scenery.lookdev-mushrooms', 149.0, 107.0, 0.5, 2.6),
    ]}])
print(c.call('problems'))
print(c.call('save'))
