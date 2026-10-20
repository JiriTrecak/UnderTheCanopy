"""Authors Frost Hollow, the block editor's look-dev scene (D80, the target frame
`art/styles/target-frost-cliff.jpg`), through the editor's MCP tools: only what a person
does with the block editor, levels, kinds, a ramp, cells and faces. Everything you see in
it is generated from those blocks by the `frost-hollow` biome's recipe.

Run with the server up (`just mcp`): `python3 tools/maps/frost_hollow.py`. It creates the
map, so remove the package first to rebuild it.

The scene, as Jiri drew it over the target, around (128, 128) seen from the south:
- the top level (2) in the north-west, ending in a bank of roots (a cliff of the biome's
  Roots face) with a closed band of roots at its foot;
- a wide diagonal ramp down to the south-east, from the top level past the acorn;
- the lower ground (level 1) at the same level as the frozen pond, its shore an outline at
  a near-diagonal painted cell by cell;
- a mushroom frame on the top level east of the ramp, a leaf frame in the south-west, a
  rocky far bank beyond the pond.
Elsewhere the map is open ground for the start in the west, framed by forest floor.

Coordinates are world cells, found by mapping the target's pixels through the shot's
camera (`--camera 128,128,0.7 --pitch 48`)."""
import os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'frost-hollow'
c = mcp.Client()
E = 0.05  # keeps an area inside the blocks it names


def box(op, x0, z0, x1, z1, **kw):
    """`op` over the blocks of world cells x0..x1, z0..z1 (block edges, multiples of 4)."""
    c.call('map_blocks', map=M, op=op, **{'from': [x0 + E, z0 + E]}, to=[x1 - E, z1 - E], **kw)


def fill(kind, points):
    """Paints the cells inside the outline `points` a kind."""
    c.call('map_blocks', map=M, op='fill', points=points, kind=kind)


c.call('create_map', id=M, name='Frost Hollow', size=256, biome='frost-hollow', folder='showcase')
c.call('map_blocks', map=M, op='flat')

# Forest floor everywhere around the scene; open ground in the west for the start.
box('paint', -16, -16, 272, 272, kind='wall')
box('paint', 12, 12, 76, 244, kind='ground')

# The lower ground south of the bank and east of the ramp.
box('paint', 76, 108, 200, 180, kind='ground')

# The top level: the north-west plateau and the ground at the ramp's head.
box('level', 56, 64, 140, 108, level=2)
box('paint', 60, 68, 136, 108, kind='ground')
# The bank of roots along the plateau's south edge: its cliff takes the Roots face, and a
# closed band of roots lies at its foot.
box('face', 56, 96, 128, 108, style=1)
box('paint', 76, 108, 128, 124, kind='wall')
fill('ground', [[76, 120], [100, 125], [118, 123.5], [128, 121], [128, 124], [76, 124]])

# The mushroom frame on the top level east of the ramp, its cliffs roots too.
box('level', 136, 64, 172, 100, level=2)
box('paint', 136, 64, 172, 100, kind='wall')
box('face', 136, 64, 172, 100, style=1)

# The ramp: wide and diagonal, from the plateau's corner down to the south-east.
c.call('map_blocks', map=M, op='ramp', **{'from': [129, 95]}, to=[143, 116], radius=1.6, level=2)

# The pond at the lower ground's level, its shore a near-diagonal outline; the rocky far
# bank beyond it.
fill('ice', [[170, 96], [164, 106], [158, 116], [147, 131], [139, 146], [136, 154],
             [134, 176], [200, 176], [200, 96]])
fill('rock', [[178, 96], [173, 110], [166, 130], [160, 150], [157, 176], [230, 176], [230, 96]])

# The leaf frame in the south-west, its edge cell by cell.
fill('wall', [[56, 133], [84, 133.5], [96, 135.5], [104, 141], [109, 148], [111, 158],
              [112, 180], [56, 180]])

# One player: a sandbox scene with a warrior at the ramp's foot, its start in the open west.
D = 'maps/showcase/frost-hollow.utcm/map.json'
c.call('edit', label='Frost Hollow: one player, a warrior', commands=[
    {'op': 'set', 'document': D, 'pointer': '/sandbox', 'value': True},
    {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': [{
        'mainFort': 'start.player.1/main-fort', 'player': 1, 'setup': 'setup.ants', 'x': 41.5, 'z': 129.5}]},
    {'op': 'set', 'document': D, 'pointer': '/entities', 'value': [{
        'definition': 'unit.ants.warrior', 'id': 'frost.warrior', 'owner': 'player.1',
        'position': {'x': 131.0, 'y': 131.0}, 'rotation': 200.0}]},
])
# Landmarks placed by hand, as the editor's props place them: the acorn at the bank's end
# by the ramp, mushrooms in their frame.
SC = 'maps/showcase/frost-hollow.utcm/scenery.json'
def obj(i, asset, x, z, scale, yaw=0.0):
    return {'asset': asset, 'elevation': 0.0, 'heightMode': 'terrain', 'id': 'frost.%d' % i, 'locked': False,
            'scale': scale, 'visible': True, 'x': x, 'yaw': yaw, 'z': z}
c.call('edit', label='Frost Hollow: landmarks', commands=[
    {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': [
        obj(0, 'scenery.lookdev-acorn', 123.5, 121.0, 1.8, 0.6),
        obj(1, 'scenery.lookdev-mushrooms', 150.0, 88.0, 1.7, 1.2),
        obj(2, 'scenery.lookdev-mushrooms', 163.0, 95.0, 1.4, 2.6),
    ]}])
print(c.call('problems'))
print(c.call('save'))
