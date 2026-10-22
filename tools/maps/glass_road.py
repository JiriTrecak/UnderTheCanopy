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
import json, math, os, random, shutil, struct, sys, time, zlib
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
OUTPOST_E = (288, 176)
TOLL = (230, 182)
PADS = [(148, 192, 192, 232),   # the west outpost at the first bend
        (212, 168, 248, 196),   # the toll plaza, south of the road
        (268, 168, 308, 192),   # the east outpost
        (16, 204, 40, 228)]     # where the road leaves the map, a little wider
# Ramps (centre, the way they fall, top level): to the camp, the north-west wood, down
# into the canal, off the toll plaza and the east outpost; up to the forecourt and the gate.
RAMPS = [(104, 232, 0, 1, 2), (120, 200, 0, -1, 2), (248, 144, 0, -1, 2),
         (232, 204, 0, 1, 2), (288, 200, 0, 1, 2), (ROAD_D, 144, 0, 1, 3), (ROAD_D, 104, 0, 1, 4)]
PLATEAU = (292, 40, 404, 96)   # the station, level 4
FORECOURT = (316, 96, 368, 136)  # below the gate, level 3
GATE = (340, 96)
CONDENSER = (340, 56)
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
    cells('shallow', blob(BOG[0] + 4, BOG[1] - 2, 13, seed=9, wobble=0.15))  # a bog: wadeable all through


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
    ent('camp.mound', 'building.ants.house', 123.5, 367.5, P1)
    ent('camp.mound.1', 'building.ants.house', 131.5, 351.5, P1)
    for i, (x, z) in enumerate([(66, 324), (61, 333), (60, 342), (62, 351), (67, 359)]):
        ent(f'camp.amber.{i}', 'building.neutral.amber-mine', x + 1.5 - x % 4, z + 1.5 - z % 4, amount=2500)
    ent('camp.deposit', 'building.neutral.corrupted-root', 135.5, 371.5)
    for i in range(6):
        ent(f'camp.worker.{i}', 'unit.ants.settler', 80 + 3 * (i % 3), 368 + 3 * (i // 3), P1, 180)
    for i, (d, x, z) in enumerate([('unit.ants.warrior', 98, 246), ('unit.ants.warrior', 104, 244),
                                    ('unit.ants.warrior', 110, 246), ('unit.ants.archer', 95, 252),
                                    ('unit.ants.archer', 113, 252)]):
        ent(f'camp.guard.{i}', d, x, z, P1, 0)
    ent('queen', 'unit.ants.queen', 110, 376, P1, 315, experience=720)
    ent('oleander', 'unit.ants.royal-alchemist', 103, 379, P1, 315, experience=400)
    ent('korrath', 'unit.ants.marshal', 112, 316, P1, 180, experience=400)
    ent('camp.cache.0', 'building.neutral.amber-cache', 77.5, 381.5, inventory={'item.resin-salve': 1})
    ent('camp.cache.1', 'building.neutral.amber-cache', 69.5, 377.5, inventory={'item.moon-dew': 1})

    # --- The old tunnel: rubble in a gap of the root bank, the mounds the script opens. ---
    ent('tunnel.rubble', 'building.neutral.rubble', *TUNNEL_IN, rotation=90)
    ent('tunnel.in', 'building.ants.house', 47.5, 263.5, P1, script=True)
    ent('tunnel.out', 'building.ants.house', *TUNNEL_OUT, P1, script=True)

    # --- The beetle station (player 2). ---
    B = P2
    # The hall and the lodge in the east half, the condenser at the end of the road; two
    # wellheads either side of the gate power its towers. The yard inside the gate stays
    # open, so the lodge's waves and the Queen's army can pass.
    ent('station.bastion', 'building.beetles.great-bastion', 377.5, 53.5, B)
    ent('station.lodge', 'building.beetles.war-lodge', 383.5, 79.5, B)
    for i, x in enumerate((315.5, 363.5)):
        ent(f'station.deposit.{i}', 'building.neutral.corrupted-root', x, 79.5)
        ent(f'station.well.{i}', 'building.beetles.substance-well', x, 79.5, B)
    ent('station.tower.0', 'building.beetles.hornwatch', 327.5, 87.5, B)
    ent('station.tower.1', 'building.beetles.hornwatch', 355.5, 87.5, B)
    ent('vosk', 'unit.beetles.coilkeeper', 344, 86, B, 180, experience=1120)
    for i, (x, z) in enumerate([(334, 80), (346, 80), (326, 70), (354, 70)]):
        ent(f'station.shield.{i}', 'unit.beetles.shieldbearer', x, z, B, 180)
    for i, (x, z) in enumerate([(324, 64), (356, 64), (392, 68), (396, 46)]):
        ent(f'station.bolter.{i}', 'unit.beetles.bolter', x, z, B, 180)
    for i, (x, z) in enumerate([(395, 60), (396, 90)]):
        ent(f'station.tinker.{i}', 'unit.beetles.worker', x, z, B, 200)

    # --- The outposts: a pumping well on a deposit, Hornwatch towers, a small guard. ---
    for name, well, towers, guard in [
            ('west', (187.5, 211.5), [(187.5, 195.5), (187.5, 227.5)],
             [('shieldbearer', 168, 202), ('shieldbearer', 162, 222), ('bolter', 158, 208), ('bolter', 178, 222)]),
            ('east', (287.5, 179.5), [(271.5, 179.5), (303.5, 179.5)],
             [('shieldbearer', 280, 160), ('shieldbearer', 296, 160), ('bolter', 279, 170), ('bolter', 296, 170)])]:
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
    ent('toll.peddler', 'building.neutral.merchant', 237.5, 181.5, wares=[
        {'item': 'item.resin-salve', 'price': [{'item': 'item.amber', 'amount': 75}], 'stock': 3, 'restockTicks': 2400},
        {'item': 'item.moon-dew', 'price': [{'item': 'item.amber', 'amount': 75}], 'stock': 2, 'restockTicks': 2400},
        {'item': 'item.trailkeeper-flask', 'price': [{'item': 'item.amber', 'amount': 125}], 'stock': 2, 'restockTicks': 3600},
        {'item': 'item.barkguard', 'price': [{'item': 'item.amber', 'amount': 150}], 'stock': 1, 'restockTicks': 7200},
        {'item': 'item.firefly-pendant', 'price': [{'item': 'item.amber', 'amount': 175}], 'stock': 1, 'restockTicks': 7200},
        {'item': 'item.stormwing-spurs', 'price': [{'item': 'item.amber', 'amount': 325}], 'stock': 1, 'restockTicks': 12000},
        {'item': 'item.amber-carapace', 'price': [{'item': 'item.amber', 'amount': 400}], 'stock': 1, 'restockTicks': 12000},
    ])
    ent('engineer', 'unit.beetles.worker', 219, 190, P4, 30)
    ent('toll.crate.0', 'building.neutral.supply-crate', 217.5, 185.5, inventory={'item.resin-salve': 1})
    ent('toll.crate.1', 'building.neutral.supply-crate', 213.5, 189.5, lootPool='loot.camp.easy')

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
    camp('bog', (208, 362), [('unit.neutral.young-toad', 200, 361, 0), ('unit.neutral.young-toad', 214, 361, 0),
                             ('unit.neutral.tree-frog', 203, 367, 330), ('unit.neutral.tree-frog', 212, 367, 30),
                             ('unit.neutral.tadpole-spirit', 207, 357, 0)],
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
    {'id': 'toll', 'x': 230, 'y': 182, 'radius': 18},
    {'id': 'engineer', 'x': 219, 'y': 190, 'radius': 14},
    {'id': 'road-west', 'x': 40, 'y': ROAD_A, 'radius': 10},
    {'id': 'outpost-w', 'x': 150, 'y': 216, 'radius': 30},
    {'id': 'bog', 'x': BOG[0], 'y': BOG[1], 'radius': 40},
    {'id': 'briar', 'x': BRIAR[0], 'y': BRIAR[1], 'radius': 36},
    {'id': 'grove', 'x': GROVE[0], 'y': GROVE[1], 'radius': 44},
    {'id': 'glade', 'x': GLADE[0], 'y': GLADE[1], 'radius': 20},
]

# The scenes' saved cameras, framed as an eye and the point it looks at: (x, z, metres above
# the ground there). Heroes stand 6–9 m tall.
SHOTS = [
    # The opening: the camp from the south-east, the road's lanterns beyond; then the
    # conversation between Aurelith (113, 327) and Korrath (112, 316), Oleander (121, 330).
    ('op-wide', (176, 404, 42), (108, 326, 0), 40, 0),
    ('op-follow', (146, 370, 16), (116, 330, 3), 36, 0),
    ('op-over-queen', (127, 349, 10), (111, 314, 5), 30, 0),
    ('op-over-korrath', (99, 293, 11), (114, 329, 5), 30, -1.5),
    ('op-two', (84, 324, 9), (108, 329, 5), 34, 0),
    ('op-korrath', (125, 310, 7), (112, 316, 6), 26, 1.5),
    ('op-road', (118, 300, 26), (106, 228, 6), 40, 0),
    # The tunnel's far end: the yard behind the station's west wall.
    ('tunnel-out', (332, 84, 16), (303.5, 59.5, 2), 36, 0),
    # The gate: the station from below, Vosk on the gate, the Queen at the road's end.
    ('gate-wide', (318, 214, 22), (342, 90, 6), 38, 0),
    ('gate-up', (348, 178, 7), (340, 102, 7), 32, 0),
    ('gate-vosk', (347, 172, 8), (340, 98, 4.5), 22, 0),
    ('gate-vosk-close', (344, 120, 3), (340, 98, 5), 30, 1.5),
    ('gate-queen', (335, 86, 9), (340, 154, 3.5), 26, -1),
    ('gate-lodge', (352, 112, 16), (379, 83, 4), 38, 0),
    # The closing: the condenser, the Queen's hands on the glass, a crane up into violet.
    ('cond-wide', (316, 116, 20), (340, 58, 6), 40, 0),
    ('cond-approach', (354, 98, 10), (340, 66, 4), 34, 0),
    ('cond-close', (345, 82, 7), (340, 56, 6), 30, 1),
    ('cond-faces', (372, 76, 10), (342, 68, 5), 30, 0),
    ('cond-rise', (340, 112, 42), (340, 56, 10), 40, 0),
]


def cameras(ground):
    """`SHOTS` as the map's saved cameras; `ground(x, z)` gives the ground's height."""
    out = []
    for id, (ex, ez, el), (tx, tz, tl), fov, roll in SHOTS:
        dx, dz = ex - tx, ez - tz
        dh = ground(ex, ez) + el - ground(tx, tz) - tl
        flat = math.hypot(dx, dz)
        c = {'id': id, 'x': tx, 'y': tz, 'height': tl, 'distance': round(math.hypot(flat, dh), 2),
             'pitch': round(math.degrees(math.atan2(dh, flat)), 2),
             'yaw': round(math.degrees(math.atan2(dx, dz)) % 360, 2), 'fov': fov}
        if roll:
            c['roll'] = roll
        out.append(c)
    return out


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


# ---------------------------------------------------------------------------------------
# Dressing: the beetles' road and station, then the forest floor's giants in the frame.

SCENERY = []
rng = random.Random(31)
MEASURED = {}
HALF_PI = math.pi / 2


def put(asset, x, z, yaw=0.0, scale=1.0):
    SCENERY.append({'id': f'dress.{len(SCENERY)}', 'asset': 'scenery.' + asset, 'x': round(x, 2),
                    'z': round(z, 2), 'yaw': round(yaw, 3), 'scale': round(scale, 3)})


def measure(name):
    """Half the larger footprint side and the height of an environment model, metres."""
    if name not in MEASURED:
        data = open(os.path.join(ROOT, 'base', 'assets', 'environment', name, 'geometry.glb'), 'rb').read()
        n = struct.unpack('<I', data[12:16])[0]
        gltf = json.loads(data[20:20 + n])
        lo, hi = [1e9] * 3, [-1e9] * 3
        for mesh in gltf['meshes']:
            for prim in mesh['primitives']:
                acc = gltf['accessors'][prim['attributes']['POSITION']]
                lo = [min(a, b) for a, b in zip(lo, acc['min'])]
                hi = [max(a, b) for a, b in zip(hi, acc['max'])]
        MEASURED[name] = (max(hi[0] - lo[0], hi[2] - lo[2]) / 2, hi[1] - lo[1])
    return MEASURED[name]


class Frame:
    """The map's closed ground (from the saved blocked.grid) and every cell's distance to
    the nearest open cell, so the frame can be dressed by depth."""

    def __init__(self, package):
        raw = open(os.path.join(package, 'terrain', 'blocked.grid'), 'rb').read()
        self.w, self.h = struct.unpack('<II', raw[8:16])
        closed = zlib.decompress(raw[16:])
        big = 10 ** 6
        w, h = self.w, self.h
        d = [0 if not closed[i] else big for i in range(w * h)]
        # Two-pass chamfer distance (3-4), in cells.
        for z in range(h):
            for x in range(w):
                i = z * w + x
                if d[i]:
                    if x > 0: d[i] = min(d[i], d[i - 1] + 3)
                    if z > 0:
                        d[i] = min(d[i], d[i - w] + 3)
                        if x > 0: d[i] = min(d[i], d[i - w - 1] + 4)
                        if x < w - 1: d[i] = min(d[i], d[i - w + 1] + 4)
        for z in range(h - 1, -1, -1):
            for x in range(w - 1, -1, -1):
                i = z * w + x
                if d[i]:
                    if x < w - 1: d[i] = min(d[i], d[i + 1] + 3)
                    if z < h - 1:
                        d[i] = min(d[i], d[i + w] + 3)
                        if x < w - 1: d[i] = min(d[i], d[i + w + 1] + 4)
                        if x > 0: d[i] = min(d[i], d[i + w - 1] + 4)
        self.d = d
        raw = open(os.path.join(package, 'terrain', 'heights.grid'), 'rb').read()
        self.hw, hh = struct.unpack('<II', raw[8:16])
        data, n = zlib.decompress(raw[16:]), self.hw * hh
        if raw[7] & 1:
            data = bytes(data[(i % 4) * n + i // 4] for i in range(n * 4))
        self.heights = struct.unpack('<%df' % n, data)

    def crowded(self, x, z, r=4):
        """Whether closed ground lies within `r` cells: a unit placed there can stick."""
        return any(self.depth(x + dx, z + dz) > 0 for dx in range(-r, r + 1) for dz in range(-r, r + 1)
                   if dx * dx + dz * dz <= r * r)

    def ground(self, x, z):
        """The ground's height at a point, metres (the vertex at its cell's corner)."""
        return self.heights[(int(round(z)) + 16) * self.hw + int(round(x)) + 16]

    def depth(self, x, z):
        """Cells from the nearest open cell (0 on open ground)."""
        gx, gz = int(x) + 16, int(z) + 16
        if not (0 <= gx < self.w and 0 <= gz < self.h):
            return 99
        return self.d[gz * self.w + gx] / 3


PLACED = []  # (piece, x, z, radius) of the giants, to keep them apart
FLAT = {'leaf-oak', 'leaf-beech', 'leaf-maple', 'bracken', 'pine-needles-a', 'pine-needles-b', 'twig',
        'maple-seed', 'moss', 'pebbles', 'dew-drop', 'dew-drops'}


def giant(frame, piece, x, z, scale=1.0, yaw=None, force=False, overlap=0.72):
    r, height = measure('giants-' + piece)
    r, height = r * scale, height * scale
    if not force:
        d = frame.depth(x, z)
        if d < 0.5:
            return False
        if piece not in FLAT and d < r * 0.55:
            return False  # its blocker would reach the open ground
        # Height grows with depth into the frame: steeply on the far (north) side, where
        # the camera looks over it, gently on the near (south) side.
        k = 2.6 if z < 120 else 0.9 if z > 330 else 1.6
        if height > 4 + d * k:
            return False
        flat = piece in FLAT
        for (p, px, pz, pr) in PLACED:
            if (p in FLAT) == flat and math.hypot(px - x, pz - z) < (r + pr) * overlap:
                return False
    PLACED.append((piece, x, z, r))
    put('giants-' + piece, x, z, rng.uniform(0, math.tau) if yaw is None else yaw, scale)
    return True


def spot(frame, near, far, box=None):
    for _ in range(4000):
        x0, z0, x1, z1 = box or (-12, -12, SIZE + 12, SIZE + 12)
        x, z = rng.uniform(x0, x1), rng.uniform(z0, z1)
        if near <= frame.depth(x, z) <= far:
            return x, z
    return None


def clusters(frame, pieces, count, near, far, size=(3, 6), spread=1.5, scale=(0.75, 1.15), box=None, overlap=0.75):
    for _ in range(count):
        piece = rng.choice(pieces)
        r = measure('giants-' + piece)[0]
        for _ in range(20):
            s = spot(frame, near, far, box)
            if not s:
                break
            n, done = rng.randint(*size), 0
            for _ in range(n * 6):
                a, dd = rng.uniform(0, math.tau), rng.uniform(0, r * spread * math.sqrt(n))
                if giant(frame, piece, s[0] + dd * math.cos(a), s[1] + dd * math.sin(a), rng.uniform(*scale), overlap=overlap):
                    done += 1
                    if done >= n:
                        break
            if done:
                break


def scatter(frame, pieces, count, near, far, scale=(0.8, 1.2), box=None, overlap=0.7):
    done = 0
    for _ in range(count * 30):
        s = spot(frame, near, far, box)
        if s and giant(frame, rng.choice(pieces), s[0], s[1], rng.uniform(*scale), overlap=overlap):
            done += 1
            if done >= count:
                return


# Yaws of the beetles' pieces. At yaw 0 a road tile runs north–south (kerbs west and east),
# a ramp tile climbs north, a corner's kerbs are west and north, a junction's kerb is
# west, an end tile is closed to the north, a wall and the condenser face south, a pipe
# run lies north–south. Positive yaw turns north towards west.
ALONG_X, ALONG_Z = HALF_PI, 0.0
CLIMB = {'n': 0.0, 'w': HALF_PI, 's': math.pi, 'e': -HALF_PI}
KERB = {'w': 0.0, 's': HALF_PI, 'e': math.pi, 'n': -HALF_PI}  # a junction's one kerb
SHOULDER = 6.0  # lanterns, either side of the middle line
PIPE = 7.4      # the pipe line, on the north or east shoulder

# Side ramps off the causeway, from RAMPS: (ramp centre, which run, the way it falls).
SIDE = [((104, 232), 'A', 's'), ((120, 200), 'A', 'n'), ((248, 144), 'C', 'n'),
        ((232, 204), 'C', 's'), ((288, 200), 'C', 's')]


def near_side(x, z, reach=10):
    for (rx, rz), run, way in SIDE:
        if run == 'A' and abs(z - ROAD_A) < 12 and abs(x - rx) < reach:
            return True
        if run == 'C' and abs(z - ROAD_C) < 12 and abs(x - rx) < reach:
            return True
    return False


def busy(x, z, r=1.0):
    """A point the station's, outposts' or plaza's buildings stand on."""
    for e in ENTITIES:
        d = e['definition']
        if not d.startswith('building.') or e['owner'] == 'none' and 'crate' not in d and 'merchant' not in d:
            continue
        w, dd = footprint(d)
        ex, ez = e['position']['x'], e['position']['y']
        if abs(x - ex) < w / 2 + r and abs(z - ez) < dd / 2 + r:
            return True
    return False


def road():
    """The paving, lanterns, pipes and tanks of the Glass Road."""
    side_tiles = {}
    for (rx, rz), run, way in SIDE:
        z = ROAD_A if run == 'A' else ROAD_C
        side_tiles[(rx, z)] = way
        put('beetle-road-ramp', rx, rz, CLIMB['n' if way == 's' else 's'])
    plaza = range(216, 248, 8)

    def tile(x, z, along):
        if (x, z) in side_tiles:
            put('beetle-road-junction', x, z, KERB['n' if side_tiles[(x, z)] == 's' else 's'])
        elif z == ROAD_C and x in plaza:
            put('beetle-road-crossing', x, z)
        else:
            put('beetle-road-straight', x, z, along)

    for x in range(ROAD_B - 8, -16, -8):
        tile(x, ROAD_A, ALONG_X)
    for z in range(ROAD_A - 8, ROAD_C, -8):
        tile(ROAD_B, z, ALONG_Z)
    for x in range(ROAD_B + 8, ROAD_D, 8):
        tile(x, ROAD_C, ALONG_X)
    put('beetle-road-corner', ROAD_B, ROAD_A, math.pi)
    put('beetle-road-corner', ROAD_B, ROAD_C, 0.0)
    put('beetle-road-corner', ROAD_D, ROAD_C, math.pi)
    # The toll plaza is paved over.
    for x in plaza:
        for z in (172, 180, 188):
            put('beetle-road-crossing', x, z)
    # D: up into the forecourt and through the gate, to the condenser.
    put('beetle-road-ramp', ROAD_D, 144, CLIMB['n'])
    for z in (132, 124, 116):
        put('beetle-road-straight', ROAD_D, z, ALONG_Z)
    put('beetle-road-ramp', ROAD_D, 104, CLIMB['n'])
    for z in (92, 84, 76):
        put('beetle-road-straight', ROAD_D, z, ALONG_Z)
    put('beetle-road-end', ROAD_D, 68, 0.0)

    # Lanterns in pairs, every 24 cells, the whole way; pipes on the north (east) shoulder
    # carrying the outposts' substance to the station.
    def lanterns(points):
        for x, z, nx, nz in points:
            for s in (-1, 1):
                lx, lz = x + s * SHOULDER * nx, z + s * SHOULDER * nz
                if not near_side(lx, lz, 9) and not busy(lx, lz, 1.5):
                    put('beetle-lantern', lx, lz)

    lanterns([(x, ROAD_A, 0, 1) for x in range(36, ROAD_B - 8, 24)])
    lanterns([(ROAD_B, z, 1, 0) for z in range(200, ROAD_C + 8, -24)])
    lanterns([(x, ROAD_C, 0, 1) for x in range(184, ROAD_D - 4, 24)])
    lanterns([(ROAD_D, z, 1, 0) for z in (122, 86)])
    for x in range(40, ROAD_B - 12, 8):
        if not near_side(x, ROAD_A, 10):
            put('beetle-pipe-run', x, ROAD_A - PIPE, ALONG_X)
    for z in range(ROAD_A - 12, ROAD_C + 8, -8):
        put('beetle-pipe-run', ROAD_B + PIPE, z, ALONG_Z)
    for x in range(ROAD_B + 12, ROAD_D - 8, 8):
        if not near_side(x, ROAD_C, 10):
            put('beetle-pipe-run', x, ROAD_C - PIPE, ALONG_X)
    # Tanks at the outposts and the plaza: the substance on its way to the condenser.
    put('beetle-substance-tank-small', 180, 203, 0.4)
    put('beetle-substance-tank-small', 180, 219, 2.1)
    put('beetle-substance-tank', 196, 186, 1.2)
    put('beetle-substance-tank-small', 263, 189, 0.9)
    put('beetle-substance-tank-small', 312, 189, 2.4)
    # The old toll house: a gate tower and a stretch of wall by the plaza, a lantern.
    put('beetle-wall-tower', 214, 194)
    put('beetle-wall', 222, 194.8, 0.0)
    put('beetle-wall-tower', 246, 194)
    put('beetle-lantern', 246, 172)


def station():
    """The walls, the condenser, tanks and pipes of the beetle station and its forecourt."""
    x0, z0, x1, z1 = PLATEAU
    put('beetle-condenser', *CONDENSER, 0.0)
    # Walls just inside the plateau's edge, a gate where the road comes up.
    for x in range(x0 + 8, x1 - 4, 8):
        put('beetle-wall', x, z0 + 1.2, math.pi)
        if not (GATE[0] - 12 < x < GATE[0] + 12):
            put('beetle-wall', x, z1 - 1.2, 0.0)
    for z in range(z0 + 8, z1 - 4, 8):
        put('beetle-wall', x0 + 1.2, z, -HALF_PI)
        put('beetle-wall', x1 - 1.2, z, HALF_PI)
    for x, z in ((x0 + 2, z0 + 2), (x1 - 2, z0 + 2), (x0 + 2, z1 - 2), (x1 - 2, z1 - 2),
                 (GATE[0] - 10, z1 - 2), (GATE[0] + 10, z1 - 2)):
        put('beetle-wall-tower', x, z)
    # Glass round the condenser, pipes feeding it from the tanks in the back yard.
    for x, z, big in ((324, 46, True), (396, 64, True), (300, 46, False), (310, 46, False),
                      (398, 78, False), (316, 62, False)):
        put('beetle-substance-tank' if big else 'beetle-substance-tank-small', x, z, rng.uniform(0, 6))
    for x in (304, 312, 320):
        put('beetle-pipe-run', x, 52, ALONG_X)
    # The forecourt: glass either side of the gate ramp's foot, lanterns at its corners.
    put('beetle-substance-tank', 324, 116, 0.6)
    put('beetle-substance-tank', 356, 116, 2.6)
    for x, z in ((320, 132), (360, 132), (320, 100), (360, 100)):
        put('beetle-lantern', x, z)
    # The old ant tunnel comes up in the back yard: an old collapse beside it.
    put('giants-pebbles', 312, 72, 1.0, 0.6)


def places():
    """Low pieces that make the side places readable, on or beside playable ground."""
    # The tunnel's mouth at the camp: a root arch over the gap, roots either side.
    put('lookdev-root-arch', 59, TUNNEL_IN[1], HALF_PI, 0.9)
    # The meadow's landmark, an acorn cup with its acorn: the scale at a glance.
    put('giants-acorn-cup', 238, 232, 0.6, 1.1)
    put('giants-acorn', 252, 224, 2.2, 1.0)
    put('giants-leaf-oak', 90, 304, 1.1, 0.7)
    put('giants-acorn', 146, 312, 0.4, 0.8)
    # The bog: cattails along the shore, lilies on the water.
    for k in range(22):
        a = k * TAU / 22 + rng.uniform(-0.08, 0.08)
        r = 27 + rng.uniform(-2, 3)
        for _ in range(rng.randint(2, 4)):  # cattails grow in clumps
            put('shore-cattails', BOG[0] + r * math.cos(a) + rng.uniform(-2, 2),
                BOG[1] + r * math.sin(a) + rng.uniform(-2, 2), rng.uniform(0, 6), rng.uniform(6, 10))
    # The hornets have taken an empty snail shell on their ledge for a nest.
    put('giants-snail-shell', LEDGE[0] + 2, LEDGE[1] - 14, 2.6, 0.95)
    for k in range(9):
        a, r = rng.uniform(0, TAU), rng.uniform(14, 24)
        put('shore-lily-white' if k % 3 else 'shore-lily-pink', BOG[0] + r * math.cos(a), BOG[1] + r * math.sin(a),
            rng.uniform(0, 6), rng.uniform(6, 9))
    # The Hollow Stag's grove: a ring of sorrel and clover round the knoll, mossy stones.
    for k in range(11):
        a = -HALF_PI + 0.55 + k * (TAU - 1.1) / 10
        r = 21 + rng.uniform(-1, 1)
        put('giants-sorrel' if k % 2 else 'giants-clover', GROVE[0] + r * math.cos(a), GROVE[1] + r * math.sin(a),
            rng.uniform(0, 6), rng.uniform(0.9, 1.15))
    for x, z in ((GROVE[0] - 12, GROVE[1] + 9), (GROVE[0] + 13, GROVE[1] - 6)):
        put('giants-mossy-stone', x, z, rng.uniform(0, 6), 0.7)
    for k in range(6):
        a, r = rng.uniform(0, TAU), rng.uniform(4, 15)
        put('giants-moss', GROVE[0] + r * math.cos(a), GROVE[1] + r * math.sin(a), rng.uniform(0, 6), 0.6)
    # The surveyor's glade: a dead beetle's husk, his things spilled.
    put('giants-beetle-husk', GLADE[0] - 10, GLADE[1] - 4, 2.4, 1.1)
    put('beetle-substance-tank-small', GLADE[0] + 2, GLADE[1] - 12, 1.2, 0.8)
    put('giants-dew-drops', GLADE[0] + 10, GLADE[1] + 2, 0.4, 1.0)


def frame_giants(frame):
    """The forest floor's giants in the frame round the playable ground."""
    # The giants, by hand: a beech behind the Hollow Stag's grove, another in the north-west
    # corner, the branch ridge along the north, the feather in the south, parasols rising
    # out of the frame north and east, roots at the trunks.
    for piece, x, z, s, yaw in [
            ('beech-trunk', 444, 446, 1.0, 0.6), ('beech-trunk', -6, 10, 1.1, 2.0),
            ('root', 398, 418, 1.0, 2.6), ('root', 430, 384, 0.9, 1.4), ('root', 30, 40, 0.9, 5.4),
            ('branch', 196, 10, 1.15, 0.06), ('feather', 252, 424, 0.9, 2.9),
            ('parasol', 110, 6, 1.0, 0.5), ('parasol', 330, 4, 0.9, 1.7), ('parasol', 446, 230, 0.85, 2.4),
            ('fern-crown', 6, 150, 0.9, 0.4), ('fern-crown', 444, 300, 0.9, 1.6), ('fern-crown', 150, 440, 0.8, 3.0),
            ('shelf-fungus', 420, 424, 0.9, 3.6), ('shelf-fungus', 22, 34, 0.8, 0.8)]:
        giant(frame, piece, x, z, s, yaw=yaw, force=True)
    # The briar thicket: a hedge of burrs, bracken and withered stems round the clearing,
    # open where its two ways in are (west and north).
    for k in range(64):
        a = k * TAU / 64
        if abs(math.remainder(a - math.pi, TAU)) < 0.3 or abs(math.remainder(a + HALF_PI, TAU)) < 0.3:
            continue
        for r in (35, 41):
            piece = ('chestnut-burr', 'bracken', 'withered', 'chestnut-burr')[(k + r) % 4]
            giant(frame, piece, BRIAR[0] + r * math.cos(a) + rng.uniform(-2, 2),
                  BRIAR[1] + r * math.sin(a) + rng.uniform(-2, 2), rng.uniform(0.7, 1.0), overlap=0.45)
    # The Glass Road's own mood over the biome's frame of giants (which brings the litter,
    # rim, seeds, mid layers, towers and ferns by itself): stands of pine cones with their
    # needles, a few dew drops.
    clusters(frame, ['pine-cone'], 12, 10, 90, size=(2, 4), spread=1.3, scale=(0.75, 1.15))
    clusters(frame, ['pine-needles-a', 'pine-needles-b'], 14, 2, 40, size=(3, 6), spread=1.6, scale=(0.7, 1.05))
    clusters(frame, ['dew-drops', 'dew-drop'], 8, 1, 14, size=(1, 3), spread=2.0)


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
    road()
    station()
    places()
    frame = Frame(SHOWCASE)
    for e in ENTITIES:
        if e['definition'].startswith('unit.') and frame.crowded(e['position']['x'], e['position']['y']):
            print('warning: placed by closed ground:', e['id'], e['position'])
    frame_giants(frame)
    print(len(SCENERY), 'scenery objects')
    c.call('edit', label='The camp, the station, the road and the forest', commands=[
        {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': SCENERY},
        {'op': 'set', 'document': D, 'pointer': '/entities', 'value': ENTITIES},
        {'op': 'set', 'document': D, 'pointer': '/camps', 'value': CAMPS},
        {'op': 'set', 'document': D, 'pointer': '/mission',
         'value': {'objectives': OBJECTIVES, 'regions': REGIONS, 'players': PLAYERS, 'cameras': cameras(frame.ground)}},
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
