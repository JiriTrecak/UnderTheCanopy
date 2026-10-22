"""Authors The Hollow Brood, the ant campaign's first mission (docs/story.md), through the
editor's MCP tools: the blocks (terrain), the set dressing, the units, camps and the
mission's players, regions and objectives. The Lua script is the package's own
`mission.lua`, written by hand; this script never touches it.

A hero's journey across a forest floor from dusk into night, Warcraft III's first-mission shape. The
Queen's column comes in from the south-west along a root path on the south bank of a
tea-dark stream, past a mushroom circle, fords the stream where dew sprites play, passes a
dead sentry, climbs a mossy terrace with thornback foragers among the berries, comes down
its far side to the rim of the Hollow, and goes down its one ramp into the sunken basin
where the stricken brood's nest still glows. Off the road: the webbed hollow with the lost
patrol (and an old burrow beyond it), a sick pool, a sap spring by the stream, the
hornets' ledge, the survivors' chamber behind rubble, and the Moonlit Hollow behind a gap
in the terrace's north tree line. A tree frog sits on a lily pad by the start.

Run with your own server up (`CANOPY_MCP_PORT=7451 ./target/release/canopy mcp --data
"$PWD"`), from the repository root:

    CANOPY_MCP_PORT=7451 python3 tools/maps/hollow_brood.py create   # a new map in showcase/
    CANOPY_MCP_PORT=7451 python3 tools/maps/hollow_brood.py terrain  # the blocks again
    CANOPY_MCP_PORT=7451 python3 tools/maps/hollow_brood.py dress    # the scenery again
    CANOPY_MCP_PORT=7451 python3 tools/maps/hollow_brood.py units    # entities, camps, mission

`create` makes the package in `maps/showcase/` (create_map's folders); move it into
`campaigns/vanguard.utcc/missions/` afterwards (the editor finds it there by its id).
Every run ends by writing the mission section into map.json beside the saved map (the
editor does not edit missions), and warns of saved cameras whose eye stands in the forest.
Placement is seeded, so a rebuild gives the same map.

Play it headless with `cargo run --release -p canopy-sim --example mission_walk -- --data .
--map base/campaigns/vanguard.utcc/missions/hollow-brood.utcm --cap 3 --route "…"`."""
import json, math, os, random, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'hollow-brood'
BORDER = 32
PLAY = 320
SIZE = PLAY + 2 * BORDER
LO, HI = BORDER, BORDER + PLAY
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
HOUR = 18.5
# Dusk turning to night: a 240-minute day passes six minutes of the clock a minute, so
# the mission starts at 18:30 and reaches 21:00 after about 25 minutes of play.
DAY_MINUTES = 240
# The Hollow's frame withers (landscape.scatter over the biome's giants): round the basin
# the healthy towers, ferns and greens give way to withered and bleached mushrooms, dead
# ferns and withered ground, while the leaves, cones and stones of the litter stay.
HOLLOW = {'x': 306, 'y': 68, 'radius': 84}
LUSH = ['scenery.giants-porcini', 'scenery.giants-morel', 'scenery.giants-clover',
        'scenery.giants-sorrel', 'scenery.giants-fiddleheads', 'scenery.giants-fern-frond',
        'scenery.giants-harts-tongue', 'scenery.giants-grass', 'scenery.giants-bracken',
        'scenery.giants-fern-crown', 'scenery.giants-parasol', 'scenery.giants-bilberries',
        'scenery.giants-buttons', 'scenery.giants-moss', 'scenery.giants-mossy-stone']
WITHERED = [
    # Rim: toppled caps and dead fern at the edge of play, withered ground pressed flat.
    {'zone': 'frameEdge', 'pieces': ['scenery.hollow-toppled-mushrooms', 'scenery.hollow-withered-fern',
                                     'scenery.hollow-withered-fern-tall'],
     'density': 1.4, 'scale': [1.8, 3.4], 'groups': 14, 'clump': 0.5},
    {'zone': 'frameEdge', 'pieces': ['scenery.underleaf-patch-withered-a', 'scenery.underleaf-patch-withered-b'],
     'density': 0.25, 'scale': [0.8, 1.2]},
    # Mid: dead fern stands, taller inwards.
    {'zone': 'frame', 'pieces': ['scenery.hollow-withered-fern', 'scenery.hollow-withered-fern-tall'],
     'density': 0.5, 'scale': [3.5, 8.0], 'groups': 20, 'clump': 0.5, 'depth': [4, 18], 'grow': 0.8},
    # Towers: withered and bleached mushrooms in clusters of one kind.
    {'zone': 'frame', 'pieces': ['scenery.giants-withered'],
     'density': 0.3, 'scale': [1.2, 2.6], 'groups': 22, 'clump': 0.7, 'depth': [8, 34], 'grow': 0.8},
    {'zone': 'frame', 'pieces': ['scenery.hollow-bleached-mushrooms', 'scenery.hollow-toppled-mushrooms'],
     'density': 0.09, 'scale': [6.0, 12.0], 'groups': 22, 'clump': 0.6, 'depth': [10, 30], 'grow': 0.8},
    # Giants: dead caps leaning in from beyond the rect.
    {'zone': 'frame', 'pieces': ['scenery.giants-withered'],
     'density': 0.03, 'scale': [2.8, 4.5], 'groups': 50, 'clump': 0.3, 'depth': [24, 200], 'grow': 0.5},
]
LANDSCAPE = {
    'environment': {'hour': HOUR, 'playing': True, 'weather': {'kind': 'clear'}, 'dayMinutes': DAY_MINUTES},
    'scatter': [{'area': HOLLOW, 'fade': 16, 'remove': LUSH, 'layers': WITHERED}],
}


def blob(cx, cz, r, n=24, squash=1.0, wobble=0.08, seed=0):
    """A wobbling circle (an outline in world cells)."""
    return [[cx + r * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.cos(a),
             cz + r * squash * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.sin(a)]
            for a in (k * 2 * math.pi / n for k in range(n))]


# --- The journey (world cells; x east, z south) -------------------------------------------

# The main road, south-west to the Hollow's rim.
ROAD = [[44, 348], [62, 332], [84, 318], [108, 308], [134, 298], [160, 292], [182, 284],
        [192, 270], [194, 256], [196, 242], [186, 228], [164, 216], [146, 206], [136, 190],
        [132, 172], [138, 158], [150, 150]]
TERRACE_ROAD = [[150, 134], [164, 124], [186, 114], [212, 108], [238, 110], [256, 114]]
EAST_ROAD = [[270, 114], [288, 118], [304, 122]]
SPURS = {
    'lost': [[146, 206], [124, 208], [104, 202]],
    'burrow': [[70, 182], [62, 160], [66, 134], [72, 118]],
    'spring': [[196, 254], [222, 248], [250, 240]],
    'hornets': [[238, 112], [250, 126], [256, 136]],
    'survivors': [[304, 122], [318, 130], [330, 136]],
    'gap': [[228, 104], [222, 90], [214, 76], [206, 62]],
    'bank': [[100, 292], [168, 292]],
    'husk': [[270, 244], [288, 264], [300, 282]],
}
# Open places: (x, z, radius, squash).
PLACES = {
    'start': (82, 318, 21, 0.85),
    'start-bank': (108, 296, 10, 0.9),
    'mushrooms': (134, 298, 14, 0.9),
    'ford': (192, 266, 13, 1.0),
    'sentry': (196, 240, 10, 1.0),
    'web': (76, 196, 19, 0.9),
    'burrow': (72, 110, 10, 1.0),
    'sick': (126, 172, 18, 0.9),
    'spring': (262, 236, 14, 0.9),
    'thorns': (200, 108, 16, 0.8),
    'terrace-west': (166, 124, 13, 1.0),
    'terrace-east': (240, 110, 12, 1.0),
    'glade': (206, 54, 11, 0.9),
    'hornets': (270, 156, 13, 0.85),
    'rim': (300, 120, 12, 0.8),
    'hollow': (306, 68, 37, 0.8),
    'chamber': (336, 162, 14, 0.9),
    'husk': (306, 294, 13, 0.85),
}
GLADE_PLINTH = blob(206, 54, 18, 20, 0.8, 0.05, 2)
LEDGE = [[246, 144], [266, 138], [286, 146], [292, 166], [276, 178], [252, 172]]
# The stream, west edge to south edge, through the ford at (192, 266).
# Its reaches run straight or at 45 degrees, which the block water draws without steps.
STREAM = [[14, 290], [90, 290], [102, 278], [178, 278], [188, 268], [206, 268], [234, 296], [234, 336],
          [248, 350], [248, 370]]


def client():
    return mcp.Client()


def terrain(c):
    def op(name, **kw):
        c.call('map_blocks', map=M, op=name, **kw)

    every = {'from': [-15.9, -15.9], 'to': [SIZE + 15.9, SIZE + 15.9]}
    op('flat')
    op('paint', kind='wall', **every)
    # The open ground: the road, its places and spurs.
    op('paint', points=ROAD, kind='ground', radius=2.0)
    op('paint', points=TERRACE_ROAD, kind='ground', radius=2.0)
    op('paint', points=EAST_ROAD, kind='ground', radius=2.0)
    for name, line in SPURS.items():
        op('paint', points=line, kind='ground', radius=1.4)
    for i, (name, (x, z, r, sq)) in enumerate(PLACES.items()):
        op('paint', points=blob(x, z, r, 22, sq, 0.1, i), kind='ground', fill=True)

    # Levels: the terrace (2) and the hornets' ledge on it (3); the Moonlit Hollow's plinth
    # (2); the webbed hollow and the Hollow sunk a level (0).
    # The terrace hugs its open ground, so its stone edge shows along the road and the
    # forest round it stands a level lower.
    op('level', points=TERRACE_ROAD, level=2, radius=3.0)
    op('level', points=SPURS['hornets'], level=2, radius=2.6)
    for name, grow in (('terrace-west', 7), ('thorns', 7), ('terrace-east', 7)):
        x, z, r, sq = PLACES[name]
        op('level', points=blob(x, z, r + grow, 22, sq, 0.1, 7), level=2, fill=True)
    op('level', points=GLADE_PLINTH, level=2, fill=True)
    op('level', points=SPURS['gap'], level=2, radius=2.2)
    op('level', points=LEDGE, level=3, fill=True)
    web = blob(76, 196, 21, 22, 0.9, 0.08, 3)
    op('level', points=web, level=0, fill=True)
    hollow = blob(306, 68, 41, 30, 0.8, 0.06, 4)
    op('level', points=hollow, level=0, fill=True)
    op('face', points=web, style=1, fill=True)
    op('face', points=hollow, style=1, fill=True)
    # The stream, in whole blocks (water is drawn block by block), its corners cut with the
    # plateaus': wading shallows, a dark channel (none at the ford), deep pools where it
    # slows, and the lily pool by the start where the frog sits.
    op('paint', points=STREAM, kind='shallow', radius=1.5)
    op('paint', points=STREAM[:5], kind='deep', radius=0.5)
    op('paint', points=STREAM[5:], kind='deep', radius=0.5)
    for x, z, r in [(44, 290, 7), (146, 278, 6), (226, 290, 6), (236, 330, 6)]:
        op('paint', points=blob(x, z, r, 14, 0.8, 0.1, x), kind='deep', fill=True)
    op('paint', points=blob(112, 290, 8, 14, 0.7, 0.1, 5), kind='shallow', fill=True)
    # The ford: wading water all the way across where the road crosses.
    op('paint', **{'from': [184, 258]}, to=[203.9, 279.9], kind='shallow')
    op('round', **every)
    # The survivors' tunnel: exactly three blocks wide, so the rubble (12 cells) closes it.
    op('paint', **{'from': [324, 132]}, to=[335.9, 151.9], kind='ground')

    # Ramps: onto the terrace, off it, onto the ledge, down into the two hollows.
    op('ramp', **{'from': [150, 138]}, to=[150, 146], radius=1.6, level=2)
    op('ramp', **{'from': [262, 114]}, to=[270, 114], radius=1.4, level=2)
    op('ramp', **{'from': [258, 146]}, to=[256, 138], radius=1.2, level=3)
    op('ramp', **{'from': [102, 200]}, to=[94, 200], radius=1.2, level=1)
    op('ramp', **{'from': [66, 170]}, to=[67, 179], radius=1.0, level=1)  # up to the old burrow
    op('ramp', **{'from': [304, 112]}, to=[304, 104], radius=1.6, level=1)

    # The sick pool and the Hollow's pools: still water the substance lights.
    op('fill', points=blob(112, 164, 6, 14, 0.8, 0.1, 6), kind='shallow')
    for x, z, r in [(290, 58, 6), (326, 84, 5), (284, 84, 4)]:
        op('fill', points=blob(x, z, r, 12, 0.9, 0.1, x), kind='shallow')


def package():
    """Where the package is now: the editor's folder for it, and its directory."""
    for folder in ('maps/showcase', 'campaigns/vanguard.utcc/missions'):
        path = os.path.join(ROOT, 'base', folder, M + '.utcm')
        if os.path.exists(os.path.join(path, 'map.json')):
            return folder, path
    raise SystemExit('no hollow-brood package')


def dress(c):
    import hollow_brood_dress
    folder, path = package()
    import hollow_brood_units as hu
    sightlines = []
    for cam in hu.CAMERAS:
        x, z, h = hu.eye(cam)
        if h < 30:
            sightlines.append((x, z, cam['x'], cam['y']))
    objects = hollow_brood_dress.dress(path, SIZE, STREAM, sightlines)
    c.call('edit', label='Set dressing', commands=[
        {'op': 'set', 'document': f'{folder}/{M}.utcm/scenery.json', 'pointer': '/objects', 'value': objects}])
    print(len(objects), 'scenery objects')


def units(c):
    import hollow_brood_units as hu
    folder, path = package()
    hu.build()
    D = f'{folder}/{M}.utcm/map.json'
    c.call('edit', label='Units, camps and features', commands=[
        {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
        {'op': 'set', 'document': D, 'pointer': '/entities', 'value': hu.ENTITIES},
        {'op': 'set', 'document': D, 'pointer': '/camps', 'value': hu.CAMPS},
        # Dusk deepening into night over the mission, and the Hollow's withered frame.
        {'op': 'set', 'document': D, 'pointer': '/landscape', 'value': LANDSCAPE},
    ])


def write_mission():
    """The mission section, written beside the saved map (the editor does not edit missions
    yet): players, regions, objectives and cameras; the script stays in mission.lua."""
    import hollow_brood_units as hu
    folder, path = package()
    players, regions, objectives = hu.build()
    with open(os.path.join(path, 'map.json')) as f:
        document = json.load(f)
    document['mission'] = {
        'objectives': objectives,
        'regions': [{'id': i, 'x': x, 'y': z, 'radius': r} for i, x, z, r in regions],
        'players': players,
        'cameras': hu.CAMERAS,
    }
    with open(os.path.join(path, 'map.json'), 'w') as f:
        json.dump(document, f, indent=2, ensure_ascii=False)
        f.write('\n')
    # Saved cameras' eyes must stand over open ground or above the forest.
    import hollow_brood_dress as hd
    _, closed = hd.read_grid(os.path.join(path, 'terrain', 'blocked.grid'))
    side = SIZE + 32
    for cam in hu.CAMERAS:
        x, z, h = hu.eye(cam)
        i = (int(z) + 16) * side + int(x) + 16
        inside = 0 <= int(x) + 16 < side and 0 <= int(z) + 16 < side
        if not inside or (closed[i] and h < 40):
            print(f'camera {cam["id"]}: eye at ({x:.0f}, {z:.0f}), {h:.0f} up, stands in the forest')


def create(c):
    import time
    for _ in range(60):  # a fresh server checks the package first
        try:
            c.call('create_map', id=M, name='The Hollow Brood', size=SIZE, biome='underleaf', folder='showcase')
            break
        except RuntimeError as error:
            if 'check' not in str(error):
                raise
            time.sleep(2)
    D = f'maps/showcase/{M}.utcm/map.json'
    c.call('edit', label='The playable area; no starts; dusk', commands=[
        {'op': 'set', 'document': D, 'pointer': '/playable',
         'value': {'x': BORDER, 'z': BORDER, 'width': PLAY, 'height': PLAY}},
        {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
        {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
        {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
        {'op': 'set', 'document': D, 'pointer': '/landscape', 'value': LANDSCAPE},
        {'op': 'set', 'document': f'maps/showcase/{M}.utcm/package.json', 'pointer': '/description',
         'value': 'Prologue, Mission One. The Hollow has gone silent; Queen Aurelith goes to ask it why.'},
    ])
    terrain(c)


if __name__ == '__main__':
    what = sys.argv[1] if len(sys.argv) > 1 else 'terrain'
    c = client()
    if what == 'create':
        create(c)
    elif what == 'terrain':
        terrain(c)
    elif what == 'dress':
        dress(c)
    elif what == 'units':
        units(c)
    elif what == 'all':
        terrain(c)
        print(c.call('save', force=True))
        dress(c)
        units(c)
    print(c.call('problems'))
    print(c.call('save', force=True))
    write_mission()
    c.picture(M, os.environ.get('PICTURE', '/tmp/hollow-brood.png'))
