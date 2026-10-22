"""Authors The Glass Road, the ant campaign's third mission (docs/story.md, Mission 3),
through the editor's MCP tools: the terrain (blocks), the set dressing (giants and the
beetles' road pieces), the entities, camps, computer players, regions and objectives. The
script itself is `tools/maps/glass_road.lua`, copied in as the package's `mission.lua`.

The map (x east, z south; playable 32–416):
- the forward camp in a wooded south-west corner (Nest at 97.5, 345.5);
- the Glass Road, a raised causeway (level 2, faced in beetle masonry) from the west edge
  (z 216) east to an outpost at its first bend (168, 216), north, then east along a canal
  past the abandoned toll house (216, 160) and a second outpost (276, 160) to the station;
- the beetle station on a walled plateau (level 4) in the north-east, its forecourt
  (level 3) below the gate, the condenser at the end of the road's axis (340, 60);
- off the road: the bog pond (south), the briar thicket and its amber (east), the Hollow
  Stag's sacred grove on a root knoll (south-east), the hornets' ledge (north), a spring in
  the north-west wood, the surveyor's glade behind a gap in the trees (north-west, secret),
  the old ant tunnel behind rubble at the camp's north edge (secret), the sleeping engineer
  by the toll house (easter egg).

Run with your own server up (`CANOPY_MCP_PORT=7453 ./target/release/canopy mcp --data "$PWD"`,
the same variable for this client): `CANOPY_MCP_PORT=7453 python3 tools/maps/glass_road.py`.
It removes and recreates the map (authored in maps/showcase, the editor's reach, then moved
into the campaign's missions/). Placement is seeded: a rebuild gives the same map.
`--terrain` stops after the blocks and writes a picture to the scratch path in PICTURE."""
import json, math, os, random, shutil, struct, sys, time
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'glass-road'
SIZE = 448
BORDER = 32
LO, HI = BORDER, SIZE - BORDER  # the playable square, 32–416
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SHOWCASE = os.path.join(ROOT, 'base', 'maps', 'showcase', M + '.utcm')
MISSION = os.path.join(ROOT, 'base', 'campaigns', 'vanguard.utcc', 'missions', M + '.utcm')
D = f'maps/showcase/{M}.utcm/map.json'
SC = f'maps/showcase/{M}.utcm/scenery.json'
PK = f'maps/showcase/{M}.utcm/package.json'
TAU = math.tau
MASONRY, ROOTS = 2, 1  # Underleaf's face styles


def blob(cx, cz, r, n=32, squash=1.0, wobble=0.08, seed=0):
    """A wobbling circle outline."""
    return [[cx + r * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.cos(a),
             cz + r * squash * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.sin(a)]
            for a in (k * TAU / n for k in range(n))]


def box(x0, z0, x1, z1):
    return [[x0, z0], [x1, z0], [x1, z1], [x0, z1]]


# ---------------------------------------------------------------------------------------
# The layout. Everything below names these.

# The causeway: 16 cells (four blocks) wide, its paving one 8 m tile down the middle.
ROAD_A = 216          # z of the west run, from the edge to the first bend
ROAD_B = 168          # x of the short north run
ROAD_C = 160          # z of the long east run along the canal
ROAD_D = 340          # x of the run up into the station
BEND_W = (ROAD_B, ROAD_A)
BEND_N = (ROAD_B, ROAD_C)
BEND_E = (ROAD_D, ROAD_C)
OUTPOST_W = (168, 212)
OUTPOST_E = (276, 176)
TOLL = (216, 182)
PADS = [(148, 192, 192, 232),   # the west outpost at the first bend
        (196, 168, 240, 196),   # the toll plaza, south of the road
        (256, 168, 296, 192),   # the east outpost
        (16, 204, 40, 228)]     # where the road leaves the map, a little wider
# Ramps (centre, the way they fall, top level): to the camp, the north-west wood, down
# into the canal, off the toll plaza and the east outpost; up to the forecourt and the gate.
RAMPS = [(104, 232, 0, 1, 2), (124, 200, 0, -1, 2), (244, 144, 0, -1, 2),
         (216, 204, 0, 1, 2), (276, 200, 0, 1, 2), (ROAD_D, 144, 0, 1, 3), (ROAD_D, 104, 0, 1, 4)]
PLATEAU = (292, 40, 404, 96)   # the station, level 4
FORECOURT = (316, 96, 368, 136)  # below the gate, level 3
GATE = (340, 96)
CONDENSER = (340, 60)
CAMP = (100, 342)
NEST = (97.5, 349.5)
BOG = (214, 330)
BRIAR = (360, 248)
GROVE = (366, 370)
LEDGE = (232, 74)
GLADE = (64, 76)
SPRING_NW = (97.5, 161.5)
TUNNEL_IN = (61.5, 261.5)  # the rubble closing the gap at the camp's north-west edge
TUNNEL_POCKET = (46, 262)  # the old mound's hollow behind it
TUNNEL_OUT = (303.5, 59.5)  # the old mound behind the station's west wall


def terrain(c):
    def blocks(op, points, **kw):
        c.call('map_blocks', map=M, op=op, points=points, fill=True, **kw)

    def line(op, points, radius, **kw):
        c.call('map_blocks', map=M, op=op, points=points, radius=radius, **kw)

    def area(op, x0, z0, x1, z1, **kw):
        c.call('map_blocks', map=M, op=op, **{'from': [x0 + 0.1, z0 + 0.1]}, to=[x1 - 0.1, z1 - 0.1], **kw)

    def cells(kind, points):
        c.call('map_blocks', map=M, op='fill', points=points, kind=kind)

    def path(kind, points, radius):
        c.call('map_blocks', map=M, op='cells', points=points, radius=radius, kind=kind)

    def ramp(cx, cz, dx, dz, level, radius=1.6):
        """A ramp of 4 × 4 blocks centred on (cx, cz), falling along (dx, dz)."""
        c.call('map_blocks', map=M, op='ramp', **{'from': [cx - 1.6 * dx, cz - 1.6 * dz]},
               to=[cx + 1.6 * dx, cz + 1.6 * dz], radius=radius, level=level)

    def opened(points):
        blocks('paint', points, kind='ground')
        cells('ground', points)

    c.call('map_blocks', map=M, op='flat')
    area('paint', -16, -16, SIZE + 16, SIZE + 16, kind='wall')

    # --- The lowland (level 1): open ground inside wandering outlines, in lanes divided
    # by tongues of forest floor. ---
    opened(blob(*CAMP, 50, seed=1))
    opened([[64, 300], [64, 236], [150, 232], [176, 240], [170, 280], [140, 300]])   # camp to road
    opened(box(52, 255.5, 66, 267.5))                                                # the tunnel's gap
    opened(blob(*TUNNEL_POCKET, 10, seed=11, wobble=0.1))                           # and its hollow
    opened([[50, 204], [52, 150], [84, 128], [140, 122], [158, 148], [156, 206]])    # north-west wood
    opened(blob(*GLADE, 22, seed=4, wobble=0.12))                                    # surveyor's glade
    path('ground', [[92, 134], [86, 116], [78, 100]], 4.5)                          # the gap to it
    opened([[176, 152], [154, 126], [164, 74], [206, 48], [284, 46], [292, 100],
            [316, 100], [320, 152]])                                                 # north lowland
    opened([[176, 196], [304, 196], [304, 214], [262, 246], [222, 262], [184, 262],
            [168, 250]])                                                             # the meadow
    path('ground', [[206, 256], [212, 296]], 9)                                     # meadow to bog
    opened(blob(*BOG, 40, seed=2, wobble=0.1))
    opened([[244, 312], [300, 316], [334, 330], [338, 350], [296, 366], [250, 366]])  # bog to grove
    opened([[262, 204], [312, 198], [330, 236], [334, 330], [304, 326], [290, 262],
            [268, 232]])                                                             # east corridor
    opened(blob(*BRIAR, 30, seed=3, wobble=0.12))
    path('ground', [[320, 250], [338, 250]], 6)                                       # thicket's west way in
    path('ground', [[350, 206], [354, 224]], 5.5)                                     # and its north one
    opened(blob(GROVE[0], GROVE[1] - 6, 34, seed=5, wobble=0.06))

    # --- The causeway (level 2), its pads, and the station. ---
    road = [[-16, ROAD_A], [ROAD_B, ROAD_A], [ROAD_B, ROAD_C], [ROAD_D, ROAD_C]]
    for (x0, z0), (x1, z1) in zip(road, road[1:]):
        area('paint', min(x0, x1) - 8, min(z0, z1) - 8, max(x0, x1) + 8, max(z0, z1) + 8, kind='ground')
        area('level', min(x0, x1) - 8, min(z0, z1) - 8, max(x0, x1) + 8, max(z0, z1) + 8, level=2)
    for x0, z0, x1, z1 in PADS:
        area('paint', x0, z0, x1, z1, kind='ground')
        area('level', x0, z0, x1, z1, level=2)
    area('paint', *PLATEAU, kind='ground')
    area('level', *PLATEAU, level=4)
    area('paint', *FORECOURT, kind='ground')
    area('level', *FORECOURT, level=3)

    # Raised places off the road: the hornets' ledge and the Hollow Stag's knoll.
    ledge = blob(*LEDGE, 20, seed=6, wobble=0.12)
    blocks('level', ledge, level=2)
    knoll = blob(*GROVE, 26, seed=7, wobble=0.07)
    blocks('level', knoll, level=2)

    c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9])

    # Faces: beetle masonry for everything the beetles built, roots for the knoll.
    for (x0, z0), (x1, z1) in zip(road, road[1:]):
        area('face', min(x0, x1) - 8, min(z0, z1) - 8, max(x0, x1) + 8, max(z0, z1) + 8, style=MASONRY)
    for p in PADS:
        area('face', *p, style=MASONRY)
    area('face', *PLATEAU, style=MASONRY)
    area('face', *FORECOURT, style=MASONRY)
    blocks('face', knoll, style=ROOTS)

    for x, z, dx, dz, level in RAMPS:
        ramp(x, z, dx, dz, level)
    ramp(LEDGE[0], LEDGE[1] + 24, 0, 1, 2)
    ramp(GROVE[0] - 4, GROVE[1] - 30, 0, -1, 2)

    # Water: the beetles' canal under the long run's north face; the bog pond.
    cells('shallow', box(176, 139.5, 324, 151.5))
    cells('shallow', blob(*BOG, 27, seed=8, wobble=0.14))
    cells('deep', blob(BOG[0] + 4, BOG[1] - 2, 13, seed=9, wobble=0.15))


# ---------------------------------------------------------------------------------------
# Entities. Ids the script names: queen, oleander, korrath, vosk, engineer, camp.nest,
# station.bastion, tunnel.rubble, tunnel.in, tunnel.out, caravan.<n>.<member>, spill.<n>.

ENTITIES = []
CAMPS = []


FOOTPRINTS = {}


def footprint(definition):
    """A building's footprint (width, depth), from its definition."""
    if definition not in FOOTPRINTS:
        group, race, name = definition.split('.')
        d = json.load(open(os.path.join(ROOT, 'base', 'buildings', race, name + '.json')))
        FOOTPRINTS[definition] = (d['footprint']['width'], d['footprint']['depth'])
    return FOOTPRINTS[definition]


def ent(id, definition, x, z, owner='none', rotation=0.0, script=False, **initial):
    if definition.startswith('building.'):
        w, d = footprint(definition)
        if rotation in (90, 270):
            w, d = d, w
        for v, s in ((x, w), (z, d)):
            if (v - s / 2 + 0.5) % 4:
                raise SystemExit(f'{id}: {x}, {z} is off the building lattice')
    e = {'definition': definition, 'id': id, 'owner': owner,
         'position': {'x': float(x), 'y': float(z)}, 'rotation': float(rotation)}
    if definition.startswith('building.'):
        initial.setdefault('construction', 'complete')
    if initial:
        e['initialState'] = initial
    if script:
        e['activation'] = 'script'
    ENTITIES.append(e)
    return e


def camp(id, home, members, aggro=10, leash=24, loot=None, drops=None):
    """A neutral camp: `members` are (definition, x, z, rotation)."""
    ids = []
    for i, (d, x, z, r) in enumerate(members):
        ids.append(ent(f'{id}.{i}', d, x, z, rotation=r)['id'])
    c = {'id': id, 'members': ids, 'home': {'x': float(home[0]), 'y': float(home[1])},
         'aggroRange': aggro, 'leash': leash, 'aggression': 'players'}
    if loot:
        c['lootPool'] = loot
    if drops:
        c['fixedDrops'] = drops
    CAMPS.append(c)


P1, P2, P3, P4 = 'player.1', 'player.2', 'player.3', 'player.4'
# Waypoints of the caravans along the causeway, west to east (the script has the same).
ROUTE = [(36, ROAD_A), (100, ROAD_A), (ROAD_B, ROAD_A), (ROAD_B, ROAD_C), (216, ROAD_C),
         (276, ROAD_C), (ROAD_D, ROAD_C), (ROAD_D, 124)]
CARAVANS = 6
# Where an ambushed caravan's crates fall: one spot every 24 cells along the road.
SPILLS = []


def spill_spots():
    spots = []
    for (x0, z0), (x1, z1) in zip(ROUTE[:-2], ROUTE[1:-1]):
        n = max(1, round(math.hypot(x1 - x0, z1 - z0) / 24))
        for k in range(n):
            t = (k + 0.5) / n
            x, z = x0 + (x1 - x0) * t, z0 + (z1 - z0) * t
            # On the paving, beside the middle line, on the 4-cell lattice (+1.5).
            if z0 == z1:
                spots.append((4 * round((x - 1.5) / 4) + 1.5, z0 + 1.5 + (4 if k % 2 else -4)))
            else:
                spots.append((x0 + 1.5 + (4 if k % 2 else -4), 4 * round((z - 1.5) / 4) + 1.5))
    return spots


def entities():
    # --- The forward camp (player 1). ---
    ent('camp.nest', 'building.ants.fort', *NEST, P1, inventory={'item.amber': 650})
    ent('camp.mound', 'building.ants.house', 127.5, 335.5, P1)
    for i, (x, z) in enumerate([(66, 324), (61, 333), (60, 342), (62, 351), (67, 359)]):
        ent(f'camp.amber.{i}', 'building.neutral.amber-mine', x + 1.5 - x % 4, z + 1.5 - z % 4, amount=2500)
    ent('camp.deposit', 'building.neutral.corrupted-root', 135.5, 371.5)
    for i in range(6):
        ent(f'camp.worker.{i}', 'unit.ants.settler', 80 + 3 * (i % 3), 368 + 3 * (i // 3), P1, 180)
    for i, (d, x, z) in enumerate([('unit.ants.warrior', 116, 318), ('unit.ants.warrior', 121, 320),
                                    ('unit.ants.warrior', 111, 320), ('unit.ants.archer', 106, 314),
                                    ('unit.ants.archer', 126, 314)]):
        ent(f'camp.guard.{i}', d, x, z, P1, 0)
    ent('queen', 'unit.ants.queen', 112, 378, P1, 315, experience=720)
    ent('oleander', 'unit.ants.royal-alchemist', 118, 381, P1, 315, experience=400)
    ent('korrath', 'unit.ants.marshal', 118, 330, P1, 160, experience=400)
    ent('camp.cache.0', 'building.neutral.amber-cache', 77.5, 381.5, inventory={'item.resin-salve': 1})
    ent('camp.cache.1', 'building.neutral.amber-cache', 69.5, 377.5, inventory={'item.moon-dew': 1})

    # --- The old tunnel: rubble in a gap of the root bank, the mounds the script opens. ---
    ent('tunnel.rubble', 'building.neutral.rubble', *TUNNEL_IN, rotation=90)
    ent('tunnel.in', 'building.ants.house', 47.5, 263.5, P1, script=True)
    ent('tunnel.out', 'building.ants.house', *TUNNEL_OUT, P1, script=True)

    # --- The beetle station (player 2). ---
    B = P2
    ent('station.bastion', 'building.beetles.great-bastion', 385.5, 61.5, B)
    ent('station.lodge', 'building.beetles.war-lodge', 379.5, 83.5, B)
    ent('station.den', 'building.beetles.brood-den', 363.5, 47.5, B)
    ent('station.tower.0', 'building.beetles.hornwatch', 323.5, 87.5, B)
    ent('station.tower.1', 'building.beetles.hornwatch', 355.5, 87.5, B)
    ent('vosk', 'unit.beetles.coilkeeper', ROAD_D, 92, B, 180, experience=1120)
    for i, (x, z) in enumerate([(334, 82), (346, 82), (360, 76), (322, 76)]):
        ent(f'station.shield.{i}', 'unit.beetles.shieldbearer', x, z, B, 180)
    for i, (x, z) in enumerate([(330, 72), (350, 72), (372, 70), (316, 66)]):
        ent(f'station.bolter.{i}', 'unit.beetles.bolter', x, z, B, 180)
    for i, (x, z) in enumerate([(392, 80), (370, 48)]):
        ent(f'station.tinker.{i}', 'unit.beetles.worker', x, z, B, 200)
    # The forecourt: a pair of shields at the foot of the gate ramp.
    for i, x in enumerate((332, 348)):
        ent(f'station.porter.{i}', 'unit.beetles.shieldbearer', x, 122, B, 180)

    # --- The outposts: a pumping well on a deposit, Hornwatch towers, a small guard. ---
    for name, well, towers, guard in [
            ('west', (187.5, 211.5), [(187.5, 195.5), (187.5, 227.5)],
             [('shieldbearer', 168, 202), ('shieldbearer', 162, 222), ('bolter', 158, 208), ('bolter', 178, 222)]),
            ('east', (275.5, 179.5), [(259.5, 179.5), (291.5, 179.5)],
             [('shieldbearer', 268, 160), ('shieldbearer', 284, 160), ('bolter', 268, 172), ('bolter', 284, 172)])]:
        ent(f'outpost.{name}.deposit', 'building.neutral.corrupted-root', *well)
        ent(f'outpost.{name}.well', 'building.beetles.substance-well', *well, B)
        for i, t in enumerate(towers):
            ent(f'outpost.{name}.tower.{i}', 'building.beetles.hornwatch', *t, B)
        for i, (u, x, z) in enumerate(guard):
            ent(f'outpost.{name}.{u}.{i}', f'unit.beetles.{u}', x, z, B, 270)

    # --- Caravans (player 3), spawned one after another at the west end of the road. ---
    for n in range(CARAVANS):
        for i, (u, dx, dz) in enumerate([('worker', 0, -2), ('worker', 4, 2), ('bolter', -5, -3), ('bolter', -5, 3)]):
            role = 'cart' if u == 'worker' else 'guard'
            ent(f'caravan.{n}.{role}.{i % 2}', f'unit.beetles.{u}', ROUTE[0][0] + dx + 2, ROUTE[0][1] + dz,
                P3, 90, script=True)
    SPILLS[:] = spill_spots()
    loot = ['item.resin-salve', 'item.moon-dew', 'item.trailkeeper-flask', 'item.resin-salve',
            'item.heartseed', 'item.moon-dew', 'item.thornband', 'item.quickstep-spurs']
    for i, (x, z) in enumerate(SPILLS):
        ent(f'spill.{i}', 'building.neutral.supply-crate', x, z, script=True,
            inventory={loot[i % len(loot)]: 1})

    # --- The toll house: the peddler, and the engineer asleep by his broken cart. ---
    ent('toll.peddler', 'building.neutral.merchant', 229.5, 181.5, wares=[
        {'item': 'item.resin-salve', 'price': [{'item': 'item.amber', 'amount': 75}], 'stock': 3, 'restockTicks': 2400},
        {'item': 'item.moon-dew', 'price': [{'item': 'item.amber', 'amount': 75}], 'stock': 2, 'restockTicks': 2400},
        {'item': 'item.trailkeeper-flask', 'price': [{'item': 'item.amber', 'amount': 125}], 'stock': 2, 'restockTicks': 3600},
        {'item': 'item.barkguard', 'price': [{'item': 'item.amber', 'amount': 150}], 'stock': 1, 'restockTicks': 7200},
        {'item': 'item.firefly-pendant', 'price': [{'item': 'item.amber', 'amount': 175}], 'stock': 1, 'restockTicks': 7200},
        {'item': 'item.stormwing-spurs', 'price': [{'item': 'item.amber', 'amount': 325}], 'stock': 1, 'restockTicks': 12000},
        {'item': 'item.amber-carapace', 'price': [{'item': 'item.amber', 'amount': 400}], 'stock': 1, 'restockTicks': 12000},
    ])
    ent('engineer', 'unit.beetles.worker', 205, 188, P4, 30)
    ent('toll.crate.0', 'building.neutral.supply-crate', 205.5, 181.5, inventory={'item.resin-salve': 1})
    ent('toll.crate.1', 'building.neutral.supply-crate', 201.5, 185.5, lootPool='loot.camp.easy')

    # --- Springs. ---
    ent('spring.sap', 'building.neutral.sap-spring', *SPRING_NW)
    ent('spring.dew', 'building.neutral.dew-spring', 301.5, 341.5)

    # --- The briar thicket's amber (an expansion once the Covenant is gone). ---
    for i, (x, z) in enumerate([(377.5, 237.5), (381.5, 245.5), (381.5, 253.5), (377.5, 261.5)]):
        ent(f'briar.amber.{i}', 'building.neutral.amber-mine', x, z, amount=2500)
    ent('briar.deposit', 'building.neutral.corrupted-root', 343.5, 231.5)

    # --- Neutral camps. ---
    camp('ring', (146, 264), [('unit.neutral.capling', 140, 260, 200), ('unit.neutral.capling', 151, 259, 160),
                              ('unit.neutral.capling', 146, 270, 0), ('unit.neutral.puffcap', 146, 264, 180)],
         loot='loot.camp.easy')
    camp('canal', (196, 128), [('unit.neutral.puddle-wisp', 190, 130, 90), ('unit.neutral.puddle-wisp', 200, 132, 270),
                               ('unit.neutral.dew-sprite', 194, 124, 180), ('unit.neutral.dew-sprite', 202, 125, 180)],
         loot='loot.camp.easy')
    camp('bog', (216, 364), [('unit.neutral.young-toad', 208, 362, 0), ('unit.neutral.young-toad', 224, 362, 0),
                             ('unit.neutral.tree-frog', 212, 368, 330), ('unit.neutral.tree-frog', 222, 368, 30),
                             ('unit.neutral.tadpole-spirit', 216, 358, 0)],
         drops=['item.restoration-draught'])
    camp('briar', BRIAR, [('unit.neutral.briar-hulk', 350, 244, 270), ('unit.neutral.briar-hulk', 354, 256, 270),
                          ('unit.neutral.bloom-witch', 364, 250, 270), ('unit.neutral.seedcaster', 360, 240, 250),
                          ('unit.neutral.seedcaster', 362, 260, 290)],
         aggro=12, leash=26, loot='loot.camp.medium')
    camp('stag', GROVE, [('unit.neutral.hollow-stag', GROVE[0], GROVE[1], 315),
                         ('unit.neutral.barkguard', GROVE[0] - 9, GROVE[1] - 4, 315),
                         ('unit.neutral.barkguard', GROVE[0] + 2, GROVE[1] - 11, 315),
                         ('unit.neutral.willow-wisp', GROVE[0] - 6, GROVE[1] + 8, 315),
                         ('unit.neutral.willow-wisp', GROVE[0] + 9, GROVE[1] + 2, 315)],
         aggro=12, leash=28, drops=['item.royal-crest'])
    camp('hornets', LEDGE, [('unit.neutral.nest-mother', LEDGE[0], LEDGE[1] - 4, 180),
                            ('unit.neutral.hornet-guard', LEDGE[0] - 7, LEDGE[1] + 4, 180),
                            ('unit.neutral.hornet-guard', LEDGE[0] + 7, LEDGE[1] + 4, 180),
                            ('unit.neutral.needle-wasp', LEDGE[0] - 4, LEDGE[1] - 9, 180),
                            ('unit.neutral.needle-wasp', LEDGE[0] + 5, LEDGE[1] - 9, 180)],
         drops=['item.warcaller-standard'])
    camp('glade', GLADE, [('unit.neutral.spitter', GLADE[0] + 2, GLADE[1] + 6, 150),
                          ('unit.neutral.webling', GLADE[0] + 8, GLADE[1] + 10, 150),
                          ('unit.neutral.webling', GLADE[0] - 4, GLADE[1] + 12, 150)],
         loot='loot.camp.easy')
    # The surveyor's satchel: a beetle surveyor never came back from the glade.
    ent('glade.satchel', 'item.amber-carapace', GLADE[0] - 6, GLADE[1] - 6)
    ent('glade.crate', 'building.neutral.supply-crate', 73.5, 69.5,
        inventory={'item.moon-dew': 1, 'item.heartseed': 1})


PLAYERS = [
    {'player': 2, 'race': 'beetles', 'team': 'enemy', 'vision': False,
     'ai': {'behaviour': 'waves', 'rebuild': True, 'waves': [
         # Turned on when Vosk refuses the Queen at the gate (ai_waves).
         {'every': 110, 'until': 540, 'from': 'trained', 'target': {'region': 'camp'},
          'units': [{'unit': 'unit.beetles.shieldbearer', 'count': 2}, {'unit': 'unit.beetles.bolter', 'count': 2}],
          'grow': {'every': 2, 'unit': 'unit.beetles.bolter', 'count': 1}},
         {'every': 120, 'from': 'trained', 'target': {'region': 'camp'},
          'units': [{'unit': 'unit.beetles.shieldbearer', 'count': 3}, {'unit': 'unit.beetles.bolter', 'count': 3},
                    {'unit': 'unit.beetles.breacher', 'count': 2}],
          'grow': {'every': 3, 'unit': 'unit.beetles.breacher', 'count': 1}},
     ]}},
    {'player': 3, 'race': 'beetles', 'team': 'enemy', 'vision': False, 'ai': {'behaviour': 'passive'}},
    {'player': 4, 'race': 'beetles', 'team': 'ally', 'vision': False, 'ai': {'behaviour': 'passive'}},
]

REGIONS = [
    {'id': 'camp', 'x': 100, 'y': 342, 'radius': 44},
    {'id': 'gate', 'x': ROAD_D, 'y': 160, 'radius': 14},
    {'id': 'forecourt', 'x': ROAD_D, 'y': 124, 'radius': 16},
    {'id': 'station', 'x': 350, 'y': 68, 'radius': 46},
    {'id': 'condenser', 'x': ROAD_D, 'y': 76, 'radius': 12},
    {'id': 'rubble', 'x': 66, 'y': 262, 'radius': 18},
    {'id': 'toll', 'x': 210, 'y': 186, 'radius': 18},
    {'id': 'road-west', 'x': 40, 'y': ROAD_A, 'radius': 10},
    {'id': 'outpost-w', 'x': 150, 'y': 216, 'radius': 30},
    {'id': 'bog', 'x': BOG[0], 'y': BOG[1], 'radius': 40},
    {'id': 'briar', 'x': BRIAR[0], 'y': BRIAR[1], 'radius': 36},
    {'id': 'grove', 'x': GROVE[0], 'y': GROVE[1], 'radius': 44},
    {'id': 'glade', 'x': GLADE[0], 'y': GLADE[1], 'radius': 20},
]


def script():
    """The mission script with the caravans' spill spots filled in."""
    lua = open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'glass_road.lua')).read()
    spots = ', '.join('{ %g, %g }' % s for s in SPILLS)
    return lua.replace('--@SPILLS@', spots)

OBJECTIVES = [
    {'id': 'speak', 'title': 'Speak with the beetles',
     'description': 'Take the Glass Road to the beetle station and ask its keeper for the secret of the glass.'},
    {'id': 'condenser', 'title': 'Take the condenser',
     'description': 'Break the beetle station (its Great Bastion) and bring Aurelith to the condenser.'},
    {'id': 'caravans', 'title': 'Ambush the caravans', 'optional': True,
     'description': 'Ambush three beetle supply caravans on the Glass Road: kill both carts of each.'},
]


def create(c):
    for path in (SHOWCASE, MISSION):
        if os.path.exists(path):
            shutil.rmtree(path)
    for attempt in range(60):  # a fresh server checks the package first
        try:
            c.call('create_map', id=M, name='The Glass Road', size=SIZE, biome='underleaf', folder='showcase')
            return
        except RuntimeError as error:
            if 'check' not in str(error) and 'exists' not in str(error):
                raise
            time.sleep(2)


if __name__ == '__main__':
    c = mcp.Client()
    # `--entities`: keep the terrain of the map the server has open (it must be in
    # maps/showcase), only place everything again.
    if '--entities' not in sys.argv:
        create(c)
        c.call('edit', label='The playable area; a clear, cool afternoon', commands=[
            {'op': 'set', 'document': D, 'pointer': '/playerStarts', 'value': []},
            {'op': 'set', 'document': D, 'pointer': '/entities', 'value': []},
            {'op': 'set', 'document': D, 'pointer': '/camps', 'value': []},
            {'op': 'set', 'document': D, 'pointer': '/playable',
             'value': {'x': BORDER, 'z': BORDER, 'width': SIZE - 2 * BORDER, 'height': SIZE - 2 * BORDER}},
            {'op': 'set', 'document': D, 'pointer': '/landscape',
             'value': {'environment': {'hour': 14.5, 'playing': True, 'weather': {'kind': 'clear'}}}},
        ])
        terrain(c)
    if '--terrain' in sys.argv:
        out = os.environ.get('PICTURE', '/tmp/glass-road.png')
        c.picture(M, out, scale=2)
        c.call('save', force=True)
        print('picture', out)
        sys.exit(0)
    entities()
    # The package keeps its script beside map.json; the check reads it from there.
    c.call('save', force=True)
    with open(os.path.join(SHOWCASE, 'mission.lua'), 'w') as f:
        f.write(script())
    c.call('edit', label='The camp, the station, the road and the forest', commands=[
        {'op': 'set', 'document': D, 'pointer': '/entities', 'value': ENTITIES},
        {'op': 'set', 'document': D, 'pointer': '/camps', 'value': CAMPS},
        {'op': 'set', 'document': D, 'pointer': '/mission',
         'value': {'objectives': OBJECTIVES, 'regions': REGIONS, 'players': PLAYERS}},
        {'op': 'set', 'document': PK, 'pointer': '/description',
         'value': 'Act I, Mission Three. The beetles keep substance in glass and are not changed by it: '
                  'Aurelith takes the Glass Road to their station to learn how.'},
    ])
    print(c.call('problems'))
    print(c.call('save', force=True))
    # Into the campaign: the editor authors maps only under maps/.
    if '--keep' not in sys.argv:
        shutil.move(SHOWCASE, MISSION)
        print('moved to', MISSION)
