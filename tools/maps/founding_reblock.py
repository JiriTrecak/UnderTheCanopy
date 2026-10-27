"""Founding's re-blocking of 2026-10-27 (docs/story.md, Mission 2, "Built"): a record of how
the shipped map's terrain was blocked out again after Jiri's playtest ("a lot more spacious
... use the entirety of the square map ... optional paths"). It ran once over the shipped
package through the editor's MCP; the package is the source of truth, so never run it
again over the map (it would undo every later edit). `founding.py` is the first block-out.

The map, north (z = 0) up, playable 352 x 352 inside a 32-cell border, moss at level 2:

- the glade (204, 244), much larger, open to the stream valley along its north side;
- the south road the column arrives by, 30 cells wide;
- the stream valley across the north, the hornets' plateau above it (level 3, a dead end
  with the optional hoard), the ridge with the feral burrows (level 3) up two wide ramps;
- the west woods and the west road (Oleander), the root bridge (24 wide) across the ravine
  into the den (level 1), the waterfall's ledge (the secret) below the fall;
- the south-west fern gullies (level 1): the bog, the stragglers' hollow in the corner
  (the optional rescue), the sap spring; the mushroom circle above them;
- the south-east: the crossroads and the Peddler, the Rootbound pocket up a road to the
  north, the dew-spring nook behind two rubbles in the corner.

Run (once, long since done) with a server up: `CANOPY_MCP_PORT=7491 python3
tools/maps/founding_reblock.py`; it only shapes the blocks and saves. The stream's flow and
foam are written into the package's water-flow.grid by `--flow`, after the last save."""
import json, math, os, random, struct, sys, zlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

M = 'founding'
SIZE = 416
SPAN = SIZE + 32
HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(os.path.dirname(HERE))
MISSION = os.path.join(ROOT, 'base', 'campaigns', 'vanguard.utcc', 'missions', M + '.utcm')

# The stream, from the east edge to its fall into the ravine, and the ravine.
STREAM = [[408, 162], [372, 166], [340, 160], [312, 164], [284, 160], [258, 156], [234, 150],
          [210, 140], [188, 128], [168, 120]]
RAVINE = [[182, 12], [176, 56], [168, 96], [158, 120], [138, 144], [118, 160], [92, 168],
          [76, 166], [62, 172], [46, 168], [30, 176], [14, 172]]


def ring(cx, cz, r, n=28, squash=1.0, wobble=0.0, seed=0.0):
    return [[cx + r * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.cos(a),
             cz + r * squash * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.sin(a)]
            for a in (k * 2 * math.pi / n for k in range(n))]


def terrain(c):
    def blocks(op, points, **kw):
        c.call('map_blocks', map=M, op=op, points=points, fill=True, **kw)

    def line(op, points, radius, **kw):
        c.call('map_blocks', map=M, op=op, points=points, radius=radius, **kw)

    def cells(kind, points, radius):
        c.call('map_blocks', map=M, op='cells', points=points, radius=radius, kind=kind)

    def fill(kind, points):
        c.call('map_blocks', map=M, op='fill', points=points, kind=kind)

    def box(op, a, b, **kw):
        c.call('map_blocks', map=M, op=op, **{'from': list(a)}, to=list(b), **kw)

    def ramp(top, foot, level, radius):
        c.call('map_blocks', map=M, op='ramp', **{'from': list(top)}, to=list(foot), radius=radius, level=level)

    def open_area(outline, level=None):
        """Opens ground inside an outline, block by block and then cell by cell (soft edges)."""
        blocks('paint', outline, kind='ground')
        fill('ground', outline)
        if level is not None:
            blocks('level', outline, level=level)

    def trail(points, rmin, rmax, step=8.0, seed=0, level=None):
        """A road of overlapping wobbly clearings, wider and narrower as it goes."""
        r = random.Random(seed)
        for (ax, az), (bx, bz) in zip(points, points[1:]):
            n = max(1, int(math.hypot(bx - ax, bz - az) / step))
            for k in range(n + 1):
                t = k / n
                x, z = ax + (bx - ax) * t, az + (bz - az) * t
                open_area(ring(x, z, r.uniform(rmin, rmax), 14, wobble=0.1, seed=r.uniform(0, 6)), level)

    def island(x, z, r, seed):
        """A stand of forest floor in open ground."""
        fill('wall', ring(x, z, r, 16, wobble=0.15, seed=seed))
        blocks('paint', ring(x, z, r - 3, 12), kind='wall')

    c.call('map_blocks', map=M, op='flat')
    world = ((-15.9, -15.9), (SIZE + 15.9, SIZE + 15.9))
    box('level', *world, level=2)
    box('paint', *world, kind='wall')

    # --- The glade: the build ground, open along its north side to the stream valley.
    open_area(ring(204, 244, 70, 44, squash=0.8, wobble=0.05, seed=0.7))
    open_area(ring(262, 232, 22, 18, wobble=0.1, seed=1.1))    # the substance spring's bay
    open_area(ring(236, 272, 20, 16, wobble=0.1))              # the mossy rise
    open_area(ring(250, 298, 15, 16, wobble=0.1, seed=0.9))    # room past it to the south-east
    open_area(ring(150, 216, 18, 16, wobble=0.1, seed=2.6))    # the Alate's corner
    trail([[204, 296], [200, 330], [204, 362], [202, 404]], 14, 17, seed=1)   # the south road

    # --- The stream valley across the north, and the glade's front onto it.
    open_area([[170, 112], [186, 104], [256, 110], [270, 136], [312, 144], [352, 146], [400, 146],
               [400, 190], [352, 194], [318, 204], [288, 208], [262, 202], [230, 200], [196, 198],
               [176, 186], [168, 140]])

    # --- The west woods and the west road, where Oleander comes in.
    open_area([[36, 184], [66, 178], [100, 184], [132, 180], [160, 190], [170, 208], [150, 234],
               [138, 258], [96, 260], [60, 258], [36, 258]])
    trail([[150, 240], [100, 240], [60, 238], [16, 236]], 14, 16, seed=5)

    # --- The south-west: the bog, the stragglers' hollow, the sap spring (soil, level 1),
    # the mushroom circle (moss) above them.
    open_area(ring(78, 292, 30, 26, squash=0.9, wobble=0.1, seed=6), level=1)       # the bog
    ramp((86, 252), (86, 264), level=2, radius=2.2)
    trail([[74, 316], [66, 340]], 14, 16, seed=11, level=1)
    open_area(ring(62, 352, 22, 22, wobble=0.12, seed=3.3), level=1)                 # stragglers
    trail([[84, 356], [118, 350]], 13, 15, seed=12, level=1)
    open_area(ring(140, 348, 22, 22, wobble=0.12, seed=2), level=1)                  # sap spring
    ramp((178, 348), (166, 348), level=2, radius=2.0)
    open_area(ring(124, 300, 16, 18, wobble=0.1, seed=4), level=2)                  # mushrooms
    trail([[152, 282], [132, 296]], 14, 16, seed=7, level=2)
    ramp((128, 314), (132, 328), level=2, radius=2.4)

    # --- The south-east: the crossroads, the Rootbound pocket, the nook.
    open_area(ring(306, 318, 22, 22, wobble=0.1, seed=1))
    trail([[258, 286], [284, 302], [300, 316]], 15, 17, seed=2)
    trail([[294, 330], [258, 344], [216, 346]], 13, 15, seed=3)
    trail([[318, 300], [330, 272], [342, 246]], 14, 16, seed=4)
    open_area(ring(350, 228, 24, 22, wobble=0.12, seed=3))
    open_area(ring(360, 342, 18, 18, wobble=0.12, seed=5))
    # The nook's way: 24 cells (z 308-331, whole blocks), closed by two rubbles across it.
    box('paint', (328, 308), (360, 331), kind='ground')

    # --- The ridge (level 3) with the feral burrows, up two wide ramps from the valley.
    open_area([[270, 40], [300, 36], [370, 38], [380, 80], [378, 126], [350, 134], [300, 136],
               [272, 126], [264, 90]], level=3)
    ramp((292, 132), (292, 146), level=3, radius=2.6)
    ramp((358, 130), (358, 144), level=3, radius=2.2)

    # --- The hornets' plateau (level 3) above the valley, one wide ramp up.
    open_area([[196, 46], [244, 40], [254, 70], [248, 100], [222, 104], [200, 98], [192, 76]], level=3)
    ramp((228, 100), (228, 114), level=3, radius=2.2)

    # --- The den across the ravine (level 1); the root bridge lands at level 2 and ramps down.
    open_area([[40, 40], [96, 36], [148, 44], [160, 70], [154, 100], [136, 124], [112, 140],
               [80, 142], [48, 138], [36, 100]], level=1)
    open_area(ring(96, 146, 13, 14), level=2)
    ramp((96, 140), (96, 126), level=2, radius=2.4)

    # --- Stands of forest in the open: between the glade and the valley they leave it two
    # wide mouths north (x 178-223 and 253-293), the way the ridge's waves come.
    island(166, 194, 12, 0.4)
    island(238, 194, 15, 1.9)
    island(308, 208, 15, 2.7)
    island(336, 194, 10, 3.3)       # the Rootbound pocket stays a dead end
    island(108, 214, 10, 0.3)
    island(58, 210, 8, 1.7)

    # --- The ravine: level 0, deep water, its banks faced with roots.
    line('level', RAVINE, 2.6, level=0)
    line('paint', RAVINE, 2.6, kind='deep')
    # The root bridge: six blocks (24 cells) wide, north to south across the ravine.
    box('level', (84.5, 148), (107.5, 186), level=2)
    box('paint', (84.5, 148), (107.5, 186), kind='ground')
    box('face', (84.5, 150), (107.5, 184), style=1)
    # The waterfall's ledge: level 1 below the fall, a hidden way down to it from the south.
    box('level', (148.5, 126.5), (167.5, 145.5), level=1)
    box('paint', (148.5, 126.5), (167.5, 145.5), kind='ground')
    line('paint', [[172, 184], [166, 168], [158, 156]], 2.2, kind='ground')
    ramp((156, 158), (156, 148), level=2, radius=2.0)
    fill('ground', ring(169, 152, 5, 10))       # no stub of forest beside the ramp's top
    line('face', RAVINE, 3.8, style=1)

    # --- Water: the stream, shallow and wadable; the bog's pond.
    cells('shallow', STREAM, 6.0)
    fill('shallow', ring(90, 296, 14, 18, wobble=0.18, seed=0.4))

    box('round', *world)
    print('problems', c.call('problems'))
    c.call('save', force=True)


# ======================================================================== the placements
D = 'campaigns/vanguard.utcc/missions/founding.utcm/map.json'
SC = 'campaigns/vanguard.utcc/missions/founding.utcm/scenery.json'


def lattice(v, side):
    """A building centre on the 4-cell lattice for a footprint side (edges at -0.5 + 4k)."""
    off = (side / 2 - 0.5) % 4
    return round((v - off) / 4) * 4 + off


def amber(id, cx, cz, toward, amount=2500):
    """Five amber nodes in an arc, the hollow facing `toward` (the editor's amber site)."""
    cx, cz = lattice(cx, 4), lattice(cz, 4)
    octant = round(math.atan2(toward[1] - cz, toward[0] - cx) / (math.pi / 4)) % 8
    sx, sz = [(1, 0), (1, 1), (0, 1), (-1, 1), (-1, 0), (-1, -1), (0, -1), (1, -1)][octant]
    px, pz = -sz, sx
    if sx == 0 or sz == 0:
        offsets = [(px * a + sx * b, pz * a + sz * b) for a, b in ((-8, 0), (-4, -4), (0, -4), (4, -4), (8, 0))]
    else:
        offsets = [(-8 * px, -8 * pz + 4 * sz), (-4 * px, -4 * pz), (0, 0), (4 * px, 4 * pz), (8 * px + 4 * sx, 8 * pz)]
    return [{'definition': 'building.neutral.amber-mine', 'id': f'amber.{id}.{k}', 'owner': 'none',
             'position': {'x': cx + a, 'y': cz + b}, 'rotation': 0.0,
             'initialState': {'amount': amount, 'construction': 'complete'},
             'appearance': {'asset': f'asset.models.resources.amber-crystal-{k % 3 + 1}'}}
            for k, (a, b) in enumerate(offsets)]


def unit(definition, id, x, z, owner='none', toward=None, rotation=0.0):
    if toward:
        rotation = round(math.degrees(math.atan2(toward[1] - z, toward[0] - x)) % 360, 1)
    return {'definition': definition, 'id': id, 'owner': owner, 'position': {'x': x, 'y': z},
            'rotation': rotation}


def neutral(definition, id, x, z, side, rotation=0.0, **state):
    return {'definition': definition, 'id': id, 'owner': 'none',
            'position': {'x': lattice(x, side), 'y': lattice(z, side)}, 'rotation': rotation,
            'initialState': dict({'construction': 'complete'}, **state)}


# Small camps (Jiri, 2026-10-27: "2-3 units at most"), each at the end of a side way.
CAMPS = [
    ('mushrooms', (124, 300), [('unit.neutral.capling', 118, 296), ('unit.neutral.capling', 130, 296),
                               ('unit.neutral.puffcap', 124, 305)],
     {'aggroRange': 8, 'leash': 16, 'fixedDrops': ['item.resin-salve']}),
    ('bog-toads', (78, 288), [('unit.neutral.young-toad', 74, 282), ('unit.neutral.tree-frog', 80, 290),
                              ('unit.neutral.tadpole-spirit', 86, 296)],
     {'aggroRange': 10, 'leash': 20, 'lootPool': 'loot.camp.medium'}),
    ('rootbound', (344, 234), [('unit.neutral.rootling', 338, 236), ('unit.neutral.old-root', 350, 232)],
     {'aggroRange': 10, 'leash': 20, 'lootPool': 'loot.camp.medium'}),
    ('hornets', (222, 66), [('unit.neutral.hornet-guard', 214, 72), ('unit.neutral.hornet-guard', 230, 72),
                            ('unit.neutral.nest-mother', 222, 62)],
     {'aggroRange': 11, 'leash': 18, 'fixedDrops': ['item.warcaller-standard']}),
]

REGIONS = [
    ('rise', 237, 272, 6), ('glade', 200, 258, 48), ('sap', 140, 348, 16), ('merchant', 306, 316, 18),
    ('bog', 78, 290, 24), ('roots', 346, 230, 22), ('hornets', 224, 76, 26), ('falls', 157, 141, 10),
    ('bridge', 96, 172, 14), ('ridge', 322, 86, 40), ('den', 88, 92, 36), ('spring', 260, 232, 16),
    ('nook', 360, 342, 14), ('mushrooms', 124, 300, 16), ('alate-near', 148, 212, 16),
    ('west', 40, 238, 10), ('meet', 168, 238, 8), ('front', 214, 214, 10), ('strays', 62, 352, 18),
]

OBJECTIVES = [
    {'id': 'colony', 'title': 'objective.founding.colony.title', 'description': 'objective.founding.colony.description'},
    {'id': 'burrows', 'title': 'objective.founding.burrows.title', 'description': 'objective.founding.burrows.description'},
    {'id': 'spring', 'title': 'objective.founding.spring.title', 'optional': True,
     'description': 'objective.founding.spring.description'},
    {'id': 'strays', 'title': 'objective.founding.strays.title', 'optional': True,
     'description': 'objective.founding.strays.description'},
    {'id': 'hoard', 'title': 'objective.founding.hoard.title', 'optional': True,
     'description': 'objective.founding.hoard.description'},
]

# Saved cameras show places (docs/campaign.md "Scenes"); every line is a preset on its
# speaker. Yaw 0 puts the eye due south looking north, 90 east.
CAMERAS = [
    {'id': 'arrival', 'x': 203, 'y': 330, 'height': 3, 'distance': 62, 'pitch': 13, 'yaw': 172, 'fov': 24},
    {'id': 'glade-mouth', 'x': 220, 'y': 280, 'height': 2, 'distance': 72, 'pitch': 26, 'yaw': 250, 'fov': 36},
    {'id': 'glade-wide', 'x': 204, 'y': 244, 'height': 0, 'distance': 130, 'pitch': 34, 'yaw': 150, 'fov': 40},
    {'id': 'west-road', 'x': 52, 'y': 238, 'height': 4, 'distance': 52, 'pitch': 20, 'yaw': 104, 'fov': 30},
    {'id': 'spring', 'x': 260, 'y': 232, 'height': 1, 'distance': 40, 'pitch': 22, 'yaw': 200, 'fov': 38},
    {'id': 'victory', 'x': 202, 'y': 244, 'height': 0, 'distance': 104, 'pitch': 42, 'yaw': 20, 'fov': 40},
]


def places(c):
    m = json.load(open(os.path.join(MISSION, 'map.json')))
    old = {e['id']: e for e in m['entities']}
    camp_members = {mid for camp in m['camps'] for mid in camp['members']}
    E = []
    for e in m['entities']:
        i = e['id']
        if i in camp_members or i.startswith(('amber.bog.', 'amber.roots.', 'stray')) or i == 'nook-rubble.2':
            continue
        if i == 'spire.nw':
            e['position'] = {'x': 115.5, 'y': 111.5}
        if i == 'road-pod.2':
            e['position'] = {'x': 189.5, 'y': 369.5}
        if i == 'nook-rubble':
            e['position'] = {'x': 333.5, 'y': 313.5}
        if i == 'nook-spring':
            e['position'] = {'x': 365.5, 'y': 345.5}
        if i == 'nook-dew':
            e['position'] = {'x': 350, 'y': 338}
        E.append(e)
        if i == 'amber.glade-south.4':
            E.extend(amber('roots', 356, 220, (336, 248)))
            E.extend(amber('bog', 62, 294, (86, 268)))
        if i == 'nook-rubble':
            r = json.loads(json.dumps(e))
            r['id'] = 'nook-rubble.2'
            r['position'] = {'x': 333.5, 'y': 325.5}
            E.append(r)
    # The stragglers of the Hollow, hiding by an old snail shell in the south-west corner.
    E += [unit('unit.ants.warrior', 'stray.1', 56, 346, 'player.4', toward=(62, 352)),
          unit('unit.ants.warrior', 'stray.2', 68, 350, 'player.4', toward=(62, 352)),
          unit('unit.ants.archer', 'stray.3', 60, 358, 'player.4', toward=(62, 352)),
          neutral('building.neutral.supply-crate', 'strays-crate', 74, 362, 4,
                  inventory={'item.resin-salve': 1})]
    camps = []
    for id, home, members, extra in CAMPS:
        ids = []
        for k, (d, x, z) in enumerate(members):
            ids.append(f'{id}.{k + 1}')
            E.append(unit(d, ids[-1], x, z, toward=home))
        camps.append(dict({'id': id, 'members': ids, 'home': {'x': home[0], 'y': home[1]},
                           'aggroRange': extra['aggroRange'], 'leash': extra['leash'], 'aggression': 'players'},
                          **{k: v for k, v in extra.items() if k in ('lootPool', 'fixedDrops')}))
    players = m['mission']['players']
    players = [p for p in players if p['player'] != 4] + [
        {'player': 4, 'race': 'ants', 'team': 'ally', 'vision': False, 'ai': {'behaviour': 'passive'}}]
    c.call('edit', label='Founding: placements for the new layout', commands=[
        {'op': 'set', 'document': D, 'pointer': '/entities', 'value': E},
        {'op': 'set', 'document': D, 'pointer': '/camps', 'value': camps},
        {'op': 'set', 'document': D, 'pointer': '/mission/players', 'value': players},
        {'op': 'set', 'document': D, 'pointer': '/mission/objectives', 'value': OBJECTIVES},
        {'op': 'set', 'document': D, 'pointer': '/mission/cameras', 'value': CAMERAS},
        {'op': 'set', 'document': D, 'pointer': '/mission/regions',
         'value': [{'id': i, 'x': x, 'y': z, 'radius': r} for i, x, z, r in REGIONS]},
    ])
    print('problems', c.call('problems'))
    dress(c)


# Hand-placed pieces moved with their places (by asset and old position), and new ones.
MOVED = {
    ('scenery.giants-parasol', 226, 46): (256, 22),            # off the hornets' plateau
    ('scenery.giants-hornet-nest', 186, 94): (208, 54),        # onto the plateau
    ('scenery.glade-mushroom-ring', 130, 300): (124, 300),     # the mushroom circle
    ('scenery.underleaf-patch-blossoms', 356, 330): (360, 340),
    ('scenery.underleaf-patch-moss-b', 160, 270): (150, 272),
    ('scenery.giants-moss', 165, 138): (150, 143),             # off the widened ledge
    ('scenery.glade-root-span', 104, 170): (111, 168),          # flanking the wider bridge
    ('scenery.glade-root-span', 87, 166): (81, 166),
    ('scenery.glade-mushroom-ring', 114, 302): (124, 300),     # (a first try's places)
    ('scenery.underleaf-patch-blossoms', 360, 346): (360, 340),
    ('scenery.underleaf-patch-leaves-a', 330, 240): (342, 246),
}
ADDED = [
    # The stragglers' corner: the empty snail shell they shelter by, moss and blossom.
    ('scenery.giants-snail-shell', 44, 366, 2.4, 1.0),
    ('scenery.underleaf-patch-blossoms', 64, 352, 0.6, 0.9),
    ('scenery.underleaf-patch-moss-a', 76, 340, 2.0, 1.0),
    ('scenery.giants-fiddleheads', 82, 372, 1.1, 0.9),
    # The hornets' hoard by their nest: nuts and seeds piled on the plateau.
    ('scenery.giants-beechnut', 238, 50, 0.7, 1.0),
    ('scenery.giants-hazelnut', 244, 56, 2.0, 1.0),
    ('scenery.giants-beechnut', 236, 58, 3.6, 0.9),
    ('scenery.underleaf-patch-leaves-b', 222, 82, 1.2, 1.0),
    # The larger glade and the valley: blossom and moss where the colony grows.
    ('scenery.underleaf-patch-blossoms', 168, 236, 1.4, 1.0),
    ('scenery.underleaf-patch-blossoms', 250, 252, 2.6, 0.9),
    ('scenery.underleaf-patch-moss-a', 230, 228, 0.4, 1.1),
    ('scenery.underleaf-patch-moss-b', 196, 284, 2.2, 1.0),
    ('scenery.underleaf-patch-blossoms', 300, 176, 0.9, 0.9),
    ('scenery.underleaf-patch-moss-a', 330, 178, 1.9, 1.0),
    ('scenery.underleaf-patch-blossoms', 112, 200, 2.9, 0.8),
    ('scenery.underleaf-patch-moss-b', 60, 196, 0.2, 1.0),
    ('scenery.underleaf-patch-blossoms', 132, 352, 1.0, 0.8),
    ('scenery.underleaf-patch-leaves-b', 270, 346, 1.6, 0.9),
    ('scenery.underleaf-patch-moss-a', 356, 214, 2.8, 1.0),
]


def dress(c):
    s = json.load(open(os.path.join(MISSION, 'scenery.json')))
    objects = []
    for o in s['objects']:
        a = o['asset']
        if a in ('scenery.giants-pebbles', 'scenery.giants-mossy-stone') and o['scale'] < 0.5:
            continue  # the stream's stones: placed again below
        if o['id'].startswith('dress.new.'):
            continue
        key = (a, round(o['x']), round(o['z']))
        if key in MOVED:
            o['x'], o['z'] = MOVED[key]
        objects.append(o)
    # The mossy rise's cushions, in a wider ring round where she plants her standard.
    cushions = [o for o in objects if o['asset'] == 'scenery.giants-moss' and math.hypot(o['x'] - 236, o['z'] - 271) < 14]
    for k, o in enumerate(cushions):
        a = 3.49 + k * 0.62
        o['x'], o['z'] = round(236 + 12 * math.cos(a), 2), round(271 + 11 * math.sin(a), 2)
    for k, (a, x, z, yaw, scale) in enumerate(ADDED):
        objects.append({'id': f'dress.new.{k}', 'asset': a, 'x': x, 'z': z, 'yaw': yaw, 'scale': scale})
    # Stones along the stream's banks, on dry ground.
    _, hw, _, _, hdata = read_grid(os.path.join(MISSION, 'terrain', 'heights.grid'))
    _, _, _, _, wdata = read_grid(os.path.join(MISSION, 'terrain', 'water-heights.grid'))
    _, bw, _, _, bdata = read_grid(os.path.join(MISSION, 'terrain', 'blocked.grid'))
    hs = struct.unpack(f'<{len(hdata) // 4}f', hdata)
    ws = struct.unpack(f'<{len(wdata) // 4}f', wdata)

    def dry(x, z):
        k = (int(round(z)) + 16) * hw + int(round(x)) + 16
        return ws[k] <= hs[k] and bdata[(int(z) + 16) * bw + int(x) + 16] == 0

    rng = random.Random(1902)
    stones = []
    for _ in range(160):
        t = rng.uniform(0, len(STREAM) - 1.001)
        k = int(t)
        (ax, az), (bx, bz) = STREAM[k], STREAM[k + 1]
        x, z = ax + (bx - ax) * (t - k), az + (bz - az) * (t - k)
        nx, nz = -(bz - az), bx - ax
        nl = math.hypot(nx, nz)
        side = rng.choice((-1, 1)) * rng.uniform(6.5, 9.0)
        px, pz = x + nx / nl * side, z + nz / nl * side
        # Sparse, so the banks never close into a fence; none where the ways cross.
        crossings = ((222, 120), (292, 146), (358, 146), (200, 190), (272, 196))
        if (36 < px < 380 and dry(px, pz) and all(math.hypot(px - sx, pz - sz) > 24 for sx, sz in stones)
                and all(math.hypot(px - cx, pz - cz) > 22 for cx, cz in crossings)):
            stones.append((px, pz))
            objects.append({'id': f'dress.new.stone.{len(stones)}',
                            'asset': 'scenery.giants-pebbles' if rng.random() < 0.6 else 'scenery.giants-mossy-stone',
                            'x': round(px, 2), 'z': round(pz, 2), 'yaw': round(rng.uniform(0, math.tau), 3),
                            'scale': round(rng.uniform(0.25, 0.45), 3)})
    c.call('edit', label='Founding: the dressing moved with its places', commands=[
        {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': objects}])
    print('scenery', len(objects), c.call('problems'))


# ======================================================================== the stream's flow
def read_grid(path):
    raw = open(path, 'rb').read()
    w, h = struct.unpack('<II', raw[8:16])
    data = zlib.decompress(raw[16:])
    size = len(data) // (w * h)
    if raw[7] and size > 1:
        n = w * h
        out = bytearray(len(data))
        for i in range(size):
            out[i::size] = data[i * n:(i + 1) * n]
        data = bytes(out)
    return raw[:16], w, h, size, data


def flow():
    """Downstream along the stream to the fall, white water where it drops into the ravine
    and below it, along the ravine (the blocks make still water)."""
    terrain_dir = os.path.join(MISSION, 'terrain')
    _, hw, _, _, hdata = read_grid(os.path.join(terrain_dir, 'heights.grid'))
    _, _, _, _, wdata = read_grid(os.path.join(terrain_dir, 'water-heights.grid'))
    heights = struct.unpack(f'<{len(hdata) // 4}f', hdata)
    waters = struct.unpack(f'<{len(wdata) // 4}f', wdata)

    def at(grid, x, z):
        vx, vz = min(max(int(round(x)) + 16, 0), hw - 1), min(max(int(round(z)) + 16, 0), hw - 1)
        return grid[vz * hw + vx]

    def wet(x, z):
        return at(waters, x, z) > at(heights, x, z)

    def along(path, x, z):
        best = (1e9, (0.0, 0.0))
        for (ax, az), (bx, bz) in zip(path, path[1:]):
            dx, dz = bx - ax, bz - az
            ll = dx * dx + dz * dz
            t = max(0.0, min(1.0, ((x - ax) * dx + (z - az) * dz) / ll))
            d = math.hypot(x - ax - dx * t, z - az - dz * t)
            if d < best[0]:
                best = (d, (dx / math.sqrt(ll), dz / math.sqrt(ll)))
        return best

    header, fw, fh, fsize, data = read_grid(os.path.join(terrain_dir, 'water-flow.grid'))
    data = bytearray(data)
    fall_at = (164, 121)
    down = [[168, 96], [158, 120], [138, 144], [118, 160], [92, 168], [62, 170], [18, 174]]
    for cz in range(fh):
        for cx in range(fw):
            x, z = cx - 16 + 0.5, cz - 16 + 0.5
            i = (cz * fw + cx) * 4
            data[i:i + 3] = bytes((128, 128, 0))  # still, unless the stream or the ravine
            if not wet(x, z):
                continue
            fall = math.hypot(x - fall_at[0], z - fall_at[1])
            ds, dir_s = along(STREAM, x, z)
            dr, dir_r = along(down, x, z)
            if ds < 9 and at(heights, x, z) > 2:
                speed, (fx, fz) = 0.55, dir_s
                foam = 40 + (150 if fall < 10 else 0)
            elif dr < 14:
                speed, (fx, fz) = 0.35, dir_r
                foam = max(0, int(230 - fall * 7))
            else:
                continue
            data[i] = max(0, min(255, int(128 + 127 * fx * speed)))
            data[i + 1] = max(0, min(255, int(128 + 127 * fz * speed)))
            data[i + 2] = max(0, min(255, foam))
    planes = b''.join(bytes(data[k::fsize]) for k in range(fsize))
    with open(os.path.join(terrain_dir, 'water-flow.grid'), 'wb') as f:
        f.write(header + zlib.compress(planes, 9))
    print('flow written')


if __name__ == '__main__':
    if '--flow' in sys.argv:
        flow()
    else:
        import mcp
        client = mcp.Client()
        if '--places' not in sys.argv:
            terrain(client)
        places(client)
        client.call('save', force=True)
