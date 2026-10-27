"""The Glass Road rebuilt (2026-10-27, after Jiri's playtest): a deliberate rework of the
shipped map's terrain, placements and dressing, run once over the map package in place. The
first build is `glass_road.py` (a record). The package stays the source of truth: this
script never writes `mission.lua`, `autoplay.lua` or the texts, and after it the map is
edited in place like any other.

The map (x east, z south; playable 32–416, every quarter of it used):
- the forward camp in the south-west (level 1), the Nest at (97.5, 349.5);
- the Glass Road: from the west edge along the middle of the map (z 236) past the west
  outpost and the toll house, up the wide main ramp onto the north terrace (level 2) at the
  east outpost, and north across the forecourt to the station gate;
- the beetle station on a plateau in the north-east (level 3), ringed by a timber palisade
  on stone footings, its gate (two timber leaves, 32 cells) at the foot of its ramp;
- side paths: the north-west wood (the sap spring, the surveyor's glade behind a gap in the
  ferns), the hornets' ledge (north), the bog (south), the briar amber (east), the Hollow
  Stag's grove (south-east corner), the old tunnel behind rubble north-west of the camp.

Run with your own server up (`CANOPY_MCP_PORT=7461 ./target/release/canopy mcp --data
"$PWD"`): `CANOPY_MCP_PORT=7461 python3 tools/maps/glass_road_natural.py [terrain|all]`.
`terrain` stops after the blocks and writes a picture to PICTURE."""
import json, math, os, random, struct, sys, zlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import mcp

M = 'glass-road'
SIZE = 448
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
PACKAGE = os.path.join(ROOT, 'base', 'campaigns', 'vanguard.utcc', 'missions', M + '.utcm')
D = f'campaigns/vanguard.utcc/missions/{M}.utcm/map.json'
SC = f'campaigns/vanguard.utcc/missions/{M}.utcm/scenery.json'
TAU = math.tau
HALF_PI = math.pi / 2
STONE, ROOTS = 0, 1  # Underleaf's face styles: its sandstone cliff, the root bank


def blob(cx, cz, r, n=40, squash=1.0, wobble=0.08, seed=0):
    """A wobbling circle outline."""
    return [[cx + r * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.cos(a),
             cz + r * squash * (1 + wobble * math.sin(3 * a + seed) + 0.5 * wobble * math.cos(5 * a + 2 * seed)) * math.sin(a)]
            for a in (k * TAU / n for k in range(n))]


def box(x0, z0, x1, z1):
    return [[x0, z0], [x1, z0], [x1, z1], [x0, z1]]


# ---------------------------------------------------------------------------------------
# The layout. Everything below names these.

ROAD_Z = 236            # the road's west run, the middle of the lowland band
TERRACE = 204           # the north terrace's southern cliff line (level 2 above)
CAMP = (104, 346)
NEST = (97.5, 349.5)
OUTPOST_W = (164, ROAD_Z)
TOLL = (238, 270)       # the toll house's plaza, just south of the road
RAMP_X = 296            # the main ramp up onto the terrace
OUTPOST_E = (RAMP_X, 184)
FORECOURT = (344, 156)
PLATEAU = (292, 36, 404, 128)   # the station, level 3
GATE_Z = 121.5          # the gate's two leaves close the mouth of the station ramp's notch
GATE_X = (331.5, 347.5)
STATION_RAMP = (324, 356)       # its ramp, notched into the plateau's south face
CONDENSER = (340, 58)
BOG = (214, 352)
BRIAR = (370, 250)
GROVE = (362, 364)
LEDGE = (196, 62)
GLADE = (66, 66)
SPRING = (96, 140)
POND = (204, 150)
TUNNEL_IN = (57.5, 297.5)       # the rubble closing the gap at the camp's north-west edge
TUNNEL_POCKET = (42, 296)
TUNNEL_MOUND = (43.5, 295.5)    # the old mound in its hollow
TUNNEL_OUT = (267.5, 63.5)      # the old mound in the field outside the station's west side


def terrain(c):
    def blocks(op, points, **kw):
        c.call('map_blocks', map=M, op=op, points=points, fill=True, **kw)

    def area(op, x0, z0, x1, z1, **kw):
        c.call('map_blocks', map=M, op=op, **{'from': [x0 + 0.1, z0 + 0.1]}, to=[x1 - 0.1, z1 - 0.1], **kw)

    def cells(kind, points):
        c.call('map_blocks', map=M, op='fill', points=points, kind=kind)

    def path(kind, points, radius):
        c.call('map_blocks', map=M, op='cells', points=points, radius=radius, kind=kind)

    def ramp(top, foot, radius):
        c.call('map_blocks', map=M, op='ramp', **{'from': list(top)}, to=list(foot), radius=radius)

    def opened(points, level=None):
        blocks('paint', points, kind='ground')
        cells('ground', points)
        if level:
            blocks('level', points, level=level)

    c.call('map_blocks', map=M, op='flat')
    area('paint', -16, -16, SIZE + 16, SIZE + 16, kind='wall')

    # --- The lowland (level 1): the camp, the road's band, the bog, the briar, the grove.
    opened(blob(*CAMP, 62, seed=1, wobble=0.07))
    opened([[62, 306], [60, 262], [170, 258], [184, 300], [150, 312]])            # camp up to the road
    opened(box(-16, TERRACE, 312, 262))                                           # the road's band
    opened([[150, 262], [262, 262], [266, 300], [246, 306], [184, 308]])          # its south shoulder
    opened(blob(*TOLL, 24, seed=12, wobble=0.1))                                  # the toll house's plaza
    opened(blob(*BOG, 44, seed=2, wobble=0.1))
    opened([[150, 336], [182, 318], [190, 380], [156, 390]])                      # camp to bog
    opened([[244, 318], [300, 314], [330, 336], [336, 380], [300, 398], [250, 392]])  # bog to the grove
    opened(blob(*GROVE, 46, seed=5, wobble=0.06))
    opened([[296, 210], [344, 214], [360, 240], [344, 272], [300, 262]])          # the road's east end
    opened(blob(*BRIAR, 36, seed=3, wobble=0.1))
    opened([[350, 278], [396, 284], [394, 330], [352, 326]])                      # briar to the grove
    opened(box(48, 292, 64, 300))                                                 # the tunnel's gap
    opened(blob(*TUNNEL_POCKET, 10, seed=11, wobble=0.08))                       # and its hollow

    # --- The north terrace (level 2): the wood, the meadow, the outpost and the forecourt.
    north = [[44, 108], [64, 96], [104, 92], [132, 104], [148, 112], [176, 92], [212, 98], [236, 70],
             [244, 44], [290, 42], [290, 124], [400, 124], [400, 186], [330, 204],
             [44, 204]]
    opened(north, level=2)
    opened(blob(*GLADE, 25, seed=4, wobble=0.12), level=2)                       # surveyor's glade
    gap = [[88, 110], [84, 96], [76, 84]]                                       # the gap to it
    c.call('map_blocks', map=M, op='paint', points=gap, radius=1.5, kind='ground')
    c.call('map_blocks', map=M, op='level', points=gap, radius=1.5, level=2)
    path('ground', gap, 7.5)
    opened(blob(*LEDGE, 24, seed=6, wobble=0.1), level=3)                        # hornets' ledge
    # Tongues of forest floor that give the terrace its shape (each leaves wide ways round).
    for outline in (blob(160, 140, 14, seed=13, wobble=0.15),       # between the wood and meadow
                    blob(250, 150, 15, seed=14, wobble=0.15),       # in the meadow
                    blob(56, 166, 10, seed=15, wobble=0.15)):       # the wood's west edge
        blocks('paint', outline, kind='wall')
        cells('wall', outline)

    # --- The station: the plateau (level 3), its ramp notched deep into its south face: the
    # gate closes the notch's mouth between cliffs, so it is the only way up.
    area('paint', *PLATEAU, kind='ground')
    area('level', *PLATEAU, level=3)

    c.call('map_blocks', map=M, op='round', **{'from': [-15.9, -15.9]}, to=[SIZE + 15.9, SIZE + 15.9])

    # Faces: sandstone for the station and the ledge, root banks for the terrace's edges.
    blocks('face', north, style=ROOTS)
    area('face', *PLATEAU, style=STONE)
    blocks('face', blob(*LEDGE, 24, seed=6, wobble=0.1), style=STONE)
    knoll = blob(*GROVE, 24, seed=7, wobble=0.07)
    blocks('level', knoll, level=2)
    blocks('face', knoll, style=ROOTS)

    # Ramps, wide: the main road onto the terrace (32 cells), the station's (32), the side ways.
    # (A ramp's line on a block boundary takes `radius` blocks to each side: 4 is eight
    # blocks, 3 six.)
    ramp((RAMP_X, TERRACE - 2), (RAMP_X, TERRACE + 14), 4)
    area('level', STATION_RAMP[0], 116, STATION_RAMP[1], PLATEAU[3], level=2)  # (after the rounding)
    ramp((340, 101), (340, 115), 4)
    ramp((108, TERRACE - 2), (108, TERRACE + 14), 3)        # up into the north-west wood
    ramp((220, TERRACE - 2), (220, TERRACE + 14), 3)        # up into the meadow
    ramp((LEDGE[0], LEDGE[1] + 20), (LEDGE[0], LEDGE[1] + 34), 3)  # the hornets' ledge
    ramp((GROVE[0] - 22, GROVE[1]), (GROVE[0] - 38, GROVE[1]), 4)  # onto the grove's knoll

    # Water: the bog (wadeable), a still pond in the meadow.
    cells('shallow', blob(*BOG, 28, seed=8, wobble=0.14))
    cells('shallow', blob(*POND, 11, seed=9, wobble=0.15))


# ---------------------------------------------------------------------------------------
# Entities. Ids the script names: queen, oleander, korrath, vosk, engineer, camp.nest,
# station.bastion, station.gate, station.gate.1, tunnel.rubble, tunnel.in, tunnel.out,
# caravan.<n>.<member>, spill.<n>, briar.<n>, stag.<n>.

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


def ent(id, definition, x, z, owner='none', rotation=0.0, script=False, appearance=None, **initial):
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
    if appearance:
        e['appearance'] = appearance
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


P1, P2, P3, P4, P5 = 'player.1', 'player.2', 'player.3', 'player.4', 'player.5'
# The caravans' waypoints along the road, west to east (the script has the same).
ROUTE = [(36, ROAD_Z), (100, ROAD_Z), (OUTPOST_W[0], ROAD_Z), (232, ROAD_Z), (RAMP_X, ROAD_Z),
         (RAMP_X, 192), (304, 172), (340, 170), (340, 158)]  # the last before the station gate
CARAVANS = 6
SPILLS = []


def spill_spots():
    """Where an ambushed caravan's goods fall: one spot every 24 cells along the road, either
    side of its middle."""
    spots = []
    for (x0, z0), (x1, z1) in zip(ROUTE[:-2], ROUTE[1:-1]):
        n = max(1, round(math.hypot(x1 - x0, z1 - z0) / 24))
        for k in range(n):
            t = (k + 0.5) / n
            x, z = x0 + (x1 - x0) * t, z0 + (z1 - z0) * t
            side = 5 if k % 2 else -5
            if abs(z1 - z0) < abs(x1 - x0):
                spots.append((round(x) + 0.5, round(z + side) + 0.5))
            else:
                spots.append((round(x + side) + 0.5, round(z) + 0.5))
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
    for i, (d, x, z) in enumerate([('unit.ants.warrior', 98, 286), ('unit.ants.warrior', 106, 284),
                                    ('unit.ants.warrior', 114, 286), ('unit.ants.archer', 95, 292),
                                    ('unit.ants.archer', 117, 292)]):
        ent(f'camp.guard.{i}', d, x, z, P1, 0)
    ent('queen', 'unit.ants.queen', 110, 376, P1, 315, experience=720)
    ent('oleander', 'unit.ants.royal-alchemist', 103, 379, P1, 315, experience=400)
    ent('korrath', 'unit.ants.marshal', 112, 316, P1, 180, experience=400)
    ent('camp.cache.0', 'building.neutral.amber-cache', 77.5, 381.5, inventory={'item.resin-salve': 1})
    ent('camp.cache.1', 'building.neutral.amber-cache', 69.5, 377.5, inventory={'item.moon-dew': 1})

    # --- The old tunnel: rubble at the camp's north-west edge, the mounds at either end. ---
    ent('tunnel.rubble', 'building.neutral.rubble', *TUNNEL_IN, rotation=90)
    ent('tunnel.in', 'building.ants.house', *TUNNEL_MOUND, P1, 90, script=True)
    ent('tunnel.out', 'building.ants.house', *TUNNEL_OUT, P1, script=True)

    # --- The station (player 2), up on its plateau. ---
    ent('station.bastion', 'building.beetles.great-bastion', 377.5, 57.5, P2)
    ent('station.lodge', 'building.beetles.war-lodge', 383.5, 91.5, P2)
    for i, (x, z) in enumerate([(307.5, 59.5), (307.5, 91.5)]):
        ent(f'station.deposit.{i}', 'building.neutral.corrupted-root', x, z)
        ent(f'station.well.{i}', 'building.beetles.substance-well', x, z, P2)
    # The gate: two timber leaves (the road's player 5, never rebuilt) across the ramp's foot.
    for i, x in enumerate(GATE_X):
        ent('station.gate' + ('' if i == 0 else '.1'), 'building.neutral.beetle-gate', x, GATE_Z, P5,
            appearance={'asset': 'asset.models.environment.glass-road-timber-gate'})
    ent('station.tower.0', 'building.beetles.hornwatch', 315.5, 107.5, P2)
    ent('station.tower.1', 'building.beetles.hornwatch', 367.5, 107.5, P2)
    ent('vosk', 'unit.beetles.coilkeeper', 344, 84, P2, 180, experience=1120)
    for i, (x, z) in enumerate([(332, 88), (348, 90), (356, 74)]):
        ent(f'station.shield.{i}', 'unit.beetles.shieldbearer', x, z, P2, 180)
    for i, (x, z) in enumerate([(360, 72), (322, 74), (396, 74)]):
        ent(f'station.bolter.{i}', 'unit.beetles.bolter', x, z, P2, 180)
    ent('station.tinker.0', 'unit.beetles.worker', 398, 44, P2, 200)
    ent('station.tinker.1', 'unit.beetles.worker', 396, 112, P2, 200)

    # --- The road's outposts (player 5): Hornwatch towers either side of the way. ---
    ent('outpost.west.deposit', 'building.neutral.corrupted-root', 187.5, 255.5)
    ent('outpost.west.well', 'building.beetles.substance-well', 187.5, 255.5, P5)
    ent('outpost.west.tower.0', 'building.beetles.hornwatch', OUTPOST_W[0] - 0.5, 215.5, P5)
    ent('outpost.west.tower.1', 'building.beetles.hornwatch', OUTPOST_W[0] - 0.5, 255.5, P5)
    for i, (d, x, z) in enumerate([('unit.beetles.shieldbearer', 152, 226), ('unit.beetles.shieldbearer', 152, 246),
                                    ('unit.beetles.bolter', 172, 228), ('unit.beetles.bolter', 172, 244)]):
        ent(f'outpost.west.{d.split(".")[-1]}.{i}', d, x, z, P5, 270)
    ent('outpost.east.deposit', 'building.neutral.corrupted-root', 351.5, 191.5)
    ent('outpost.east.well', 'building.beetles.substance-well', 351.5, 191.5, P5)
    ent('outpost.east.tower.0', 'building.beetles.hornwatch', 263.5, 183.5, P5)
    ent('outpost.east.tower.1', 'building.beetles.hornwatch', 331.5, 183.5, P5)
    for i, (d, x, z) in enumerate([('unit.beetles.shieldbearer', 288, 182), ('unit.beetles.shieldbearer', 306, 182),
                                    ('unit.beetles.bolter', 290, 170), ('unit.beetles.bolter', 304, 170)]):
        ent(f'outpost.east.{d.split(".")[-1]}.{i}', d, x, z, P5, 180)

    # --- The caravans (player 3), spawned by the script at the road's west end. ---
    for n in range(CARAVANS):
        for i in range(2):
            ent(f'caravan.{n}.cart.{i}', 'unit.beetles.supply-cart', 38 + 7 * i, ROAD_Z - 3 + 6 * i, P3, 90, script=True)
            ent(f'caravan.{n}.guard.{i}', 'unit.beetles.bolter', 31, ROAD_Z - 3 + 6 * i, P3, 90, script=True)
    SPILLS[:] = spill_spots()
    loot = ['item.resin-salve', 'item.moon-dew', 'item.trailkeeper-flask', 'item.resin-salve', 'item.heartseed',
            'item.moon-dew', 'item.thornband', 'item.quickstep-spurs']
    for i, (x, z) in enumerate(SPILLS):
        ent(f'spill.{i}', loot[i % len(loot)], math.floor(x), math.floor(z), script=True)

    # --- The toll house: the peddler, and the engineer asleep by its crates. ---
    ent('toll.peddler', 'building.neutral.merchant', 245.5, 277.5, wares=[
        {'item': 'item.resin-salve', 'price': [{'item': 'item.amber', 'amount': 75}], 'stock': 3, 'restockTicks': 2400},
        {'item': 'item.moon-dew', 'price': [{'item': 'item.amber', 'amount': 75}], 'stock': 2, 'restockTicks': 2400},
        {'item': 'item.trailkeeper-flask', 'price': [{'item': 'item.amber', 'amount': 125}], 'stock': 2, 'restockTicks': 3600},
        {'item': 'item.barkguard', 'price': [{'item': 'item.amber', 'amount': 150}], 'stock': 1, 'restockTicks': 7200},
        {'item': 'item.firefly-pendant', 'price': [{'item': 'item.amber', 'amount': 175}], 'stock': 1, 'restockTicks': 7200},
        {'item': 'item.stormwing-spurs', 'price': [{'item': 'item.amber', 'amount': 325}], 'stock': 1, 'restockTicks': 12000},
        {'item': 'item.amber-carapace', 'price': [{'item': 'item.amber', 'amount': 400}], 'stock': 1, 'restockTicks': 12000},
    ])
    ent('engineer', 'unit.beetles.worker', 226, 284, P4, 30)
    ent('toll.crate.0', 'building.neutral.supply-crate', 221.5, 277.5, inventory={'item.resin-salve': 1})
    ent('toll.crate.1', 'building.neutral.supply-crate', 217.5, 285.5, lootPool='loot.camp.easy')

    # --- Springs: sap in the north-west wood, dew on the way to the grove. ---
    ent('spring.sap', 'building.neutral.sap-spring', 97.5, 141.5)
    ent('spring.dew', 'building.neutral.dew-spring', 305.5, 353.5)

    # --- The briar amber (an expansion once the Covenant is gone). ---
    for i, (x, z) in enumerate([(389.5, 237.5), (393.5, 245.5), (393.5, 253.5), (389.5, 261.5)]):
        ent(f'briar.amber.{i}', 'building.neutral.amber-mine', x, z, amount=2500)
    ent('briar.deposit', 'building.neutral.corrupted-root', 347.5, 263.5)

    # --- Neutral camps: few and small, off the road. ---
    camp('glade', GLADE, [('unit.neutral.spitter', GLADE[0] + 2, GLADE[1] + 6, 150),
                          ('unit.neutral.webling', GLADE[0] + 8, GLADE[1] + 10, 150),
                          ('unit.neutral.webling', GLADE[0] - 4, GLADE[1] + 12, 150)],
         loot='loot.camp.easy')
    camp('hornets', LEDGE, [('unit.neutral.nest-mother', LEDGE[0], LEDGE[1] - 4, 180),
                            ('unit.neutral.hornet-guard', LEDGE[0] - 6, LEDGE[1] + 6, 180)],
         drops=['item.warcaller-standard'])
    camp('bog', (BOG[0], BOG[1] - 4), [('unit.neutral.tadpole-spirit', BOG[0], BOG[1] - 6, 0),
                                       ('unit.neutral.young-toad', BOG[0] - 9, BOG[1] + 2, 20),
                                       ('unit.neutral.tree-frog', BOG[0] + 9, BOG[1] + 2, 340)],
         drops=['item.restoration-draught'])
    camp('briar', (BRIAR[0] - 2, BRIAR[1]), [('unit.neutral.briar-hulk', BRIAR[0] - 8, BRIAR[1] - 2, 270),
                                             ('unit.neutral.bloom-witch', BRIAR[0] + 4, BRIAR[1] - 8, 250)],
         aggro=12, leash=26, loot='loot.camp.medium')
    camp('stag', GROVE, [('unit.neutral.hollow-stag', GROVE[0] + 2, GROVE[1] + 2, 270),
                         ('unit.neutral.willow-wisp', GROVE[0] - 8, GROVE[1] - 6, 270),
                         ('unit.neutral.willow-wisp', GROVE[0] - 6, GROVE[1] + 10, 270)],
         aggro=12, leash=28, drops=['item.royal-crest'])
    # The surveyor's satchel: a beetle surveyor never came back from the glade.
    ent('glade.satchel', 'item.amber-carapace', GLADE[0] - 8, GLADE[1] - 6)
    ent('glade.crate', 'building.neutral.supply-crate', 77.5, 57.5, inventory={'item.moon-dew': 1, 'item.heartseed': 1})


PLAYERS = [
    {'player': 2, 'race': 'beetles', 'team': 'enemy', 'vision': False,
     'ai': {'behaviour': 'waves', 'rebuild': False, 'waves': [
         {'every': 120, 'from': 'trained', 'target': {'region': 'camp'},
          'units': [{'unit': 'unit.beetles.shieldbearer', 'count': 2}, {'unit': 'unit.beetles.bolter', 'count': 2}],
          'grow': {'every': 3, 'unit': 'unit.beetles.breacher', 'count': 1}},
     ]}},
    {'player': 3, 'race': 'beetles', 'team': 'enemy', 'vision': False, 'ai': {'behaviour': 'passive'}},
    {'player': 4, 'race': 'beetles', 'team': 'ally', 'vision': False, 'ai': {'behaviour': 'passive'}},
    {'player': 5, 'race': 'beetles', 'team': 'enemy', 'vision': False, 'ai': {'behaviour': 'passive'}},
]

REGIONS = [
    {'id': 'camp', 'x': 100, 'y': 333, 'radius': 44},
    {'id': 'gate', 'x': 340, 'y': 170, 'radius': 16},
    {'id': 'forecourt', 'x': 340, 'y': 134, 'radius': 10},
    {'id': 'station', 'x': 348, 'y': 76, 'radius': 44},
    {'id': 'condenser', 'x': CONDENSER[0], 'y': 78, 'radius': 12},
    {'id': 'rubble', 'x': 66, 'y': 296, 'radius': 18},
    {'id': 'toll', 'x': 238, 'y': 272, 'radius': 20},
    {'id': 'engineer', 'x': 226, 'y': 284, 'radius': 14},
    {'id': 'road-west', 'x': 40, 'y': ROAD_Z, 'radius': 10},
    {'id': 'outpost-w', 'x': 136, 'y': ROAD_Z, 'radius': 30},
    {'id': 'bog', 'x': BOG[0], 'y': BOG[1] - 6, 'radius': 40},
    {'id': 'briar', 'x': BRIAR[0] - 4, 'y': BRIAR[1], 'radius': 36},
    {'id': 'grove', 'x': GROVE[0], 'y': GROVE[1], 'radius': 44},
    {'id': 'glade', 'x': GLADE[0], 'y': GLADE[1], 'radius': 22},
]

OBJECTIVES = [
    {'id': 'speak', 'title': 'objective.glass-road.speak.title', 'description': 'objective.glass-road.speak.description'},
    {'id': 'condenser', 'title': 'objective.glass-road.condenser.title', 'description': 'objective.glass-road.condenser.description'},
    {'id': 'caravans', 'title': 'objective.glass-road.caravans.title', 'optional': True,
     'description': 'objective.glass-road.caravans.description'},
    {'id': 'briar', 'title': 'objective.glass-road.briar.title', 'optional': True,
     'description': 'objective.glass-road.briar.description'},
    {'id': 'stag', 'title': 'objective.glass-road.stag.title', 'optional': True,
     'description': 'objective.glass-road.stag.description'},
]

# Saved cameras for places a scene shows for themselves: (eye x, z, metres above the ground
# there), (target x, z, metres), fov, roll.
SHOTS = [
    ('tunnel-out', (238, 96, 18), (TUNNEL_OUT[0], TUNNEL_OUT[1], 2), 36, 0),
    ('cond-rise', (340, 116, 34), (CONDENSER[0], CONDENSER[1], 10), 40, 0),
]


class Frame:
    """The saved map's closed ground and every cell's distance to the nearest open cell, so
    the frame can be dressed by depth; and its heights."""

    def __init__(self, package):
        raw = open(os.path.join(package, 'terrain', 'blocked.grid'), 'rb').read()
        self.w, self.h = struct.unpack('<II', raw[8:16])
        closed = zlib.decompress(raw[16:])
        big = 10 ** 6
        w, h = self.w, self.h
        d = [0 if not closed[i] else big for i in range(w * h)]
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

    def ground(self, x, z):
        return self.heights[(int(round(z)) + 16) * self.hw + int(round(x)) + 16]

    def depth(self, x, z):
        """Cells from the nearest open cell (0 on open ground)."""
        gx, gz = int(x) + 16, int(z) + 16
        if not (0 <= gx < self.w and 0 <= gz < self.h):
            return 99
        return self.d[gz * self.w + gx] / 3


def cameras(frame):
    out = []
    for id, (ex, ez, el), (tx, tz, tl), fov, roll in SHOTS:
        dx, dz = ex - tx, ez - tz
        dh = frame.ground(ex, ez) + el - frame.ground(tx, tz) - tl
        flat = math.hypot(dx, dz)
        c = {'id': id, 'x': tx, 'y': tz, 'height': tl, 'distance': round(math.hypot(flat, dh), 2),
             'pitch': round(math.degrees(math.atan2(dh, flat)), 2),
             'yaw': round(math.degrees(math.atan2(dx, dz)) % 360, 2), 'fov': fov}
        if roll:
            c['roll'] = roll
        out.append(c)
    return out


# ---------------------------------------------------------------------------------------
# Dressing: the road and its outposts, the station's palisade, the side places, the frame.

SCENERY = []
rng = random.Random(27)
MEASURED = {}
PLACED = []  # (piece, x, z, radius) of the giants, to keep them apart
FLAT = {'leaf-oak', 'leaf-beech', 'leaf-maple', 'bracken', 'pine-needles-a', 'pine-needles-b', 'twig',
        'maple-seed', 'moss', 'pebbles', 'dew-drop', 'dew-drops'}


def put(asset, x, z, yaw=0.0, scale=1.0):
    SCENERY.append({'id': f'dress.{len(SCENERY)}', 'asset': 'scenery.' + asset, 'x': round(x, 2),
                    'z': round(z, 2), 'yaw': round(yaw, 3), 'scale': round(scale, 3)})


def heading(dx, dz):
    """The yaw that turns a piece's length (its model x) along (dx, dz): positive yaw turns
    north (−z) towards west, so east turns towards north."""
    return math.atan2(-dz, dx)


def palisade(points, posts=True):
    """A palisade along a polyline: 8 m sections end to end (a little overlapped), posts at
    the corners and ends. Rails face the right of the walking direction."""
    for (x0, z0), (x1, z1) in zip(points, points[1:]):
        length = math.hypot(x1 - x0, z1 - z0)
        n = max(1, math.ceil(length / 7.6))
        yaw = heading(x1 - x0, z1 - z0)
        for k in range(n):
            t = (k + 0.5) / n
            put('glass-road-palisade-wall', x0 + (x1 - x0) * t, z0 + (z1 - z0) * t, yaw)
    if posts:
        for x, z in points:
            put('glass-road-palisade-post', x, z, rng.uniform(0, TAU))


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


def giant(frame, piece, x, z, scale=1.0, yaw=None, force=False, overlap=0.72):
    r, height = measure('giants-' + piece)
    r, height = r * scale, height * scale
    if not force:
        d = frame.depth(x, z)
        if d < 0.5:
            return False
        if piece not in FLAT and d < r * 0.55:
            return False  # its blocker would reach the open ground
        k = 2.6 if z < 120 else 0.9 if z > 330 else 1.6
        if height > 4 + d * k:
            return False
        flat = piece in FLAT
        for (p, px, pz, pr) in PLACED:
            if (p in FLAT) == flat and math.hypot(px - x, pz - z) < (r + pr) * overlap:
                return False
    PLACED.append((piece, x, z, r))
    put('giants-' + piece, x, z, rng.uniform(0, TAU) if yaw is None else yaw, scale)
    return True


def spot(frame, near, far, area=None):
    for _ in range(4000):
        x0, z0, x1, z1 = area or (-12, -12, SIZE + 12, SIZE + 12)
        x, z = rng.uniform(x0, x1), rng.uniform(z0, z1)
        if near <= frame.depth(x, z) <= far:
            return x, z
    return None


def clusters(frame, pieces, count, near, far, size=(3, 6), spread=1.5, scale=(0.75, 1.15), area=None, overlap=0.75):
    for _ in range(count):
        piece = rng.choice(pieces)
        r = measure('giants-' + piece)[0]
        for _ in range(20):
            s = spot(frame, near, far, area)
            if not s:
                break
            n, done = rng.randint(*size), 0
            for _ in range(n * 6):
                a, dd = rng.uniform(0, TAU), rng.uniform(0, r * spread * math.sqrt(n))
                if giant(frame, piece, s[0] + dd * math.cos(a), s[1] + dd * math.sin(a), rng.uniform(*scale), overlap=overlap):
                    done += 1
                    if done >= n:
                        break
            if done:
                break


def near_route(x, z, margin):
    """Within `margin` cells of the road's middle line."""
    for (x0, z0), (x1, z1) in zip(ROUTE, ROUTE[1:]):
        dx, dz = x1 - x0, z1 - z0
        t = max(0.0, min(1.0, ((x - x0) * dx + (z - z0) * dz) / (dx * dx + dz * dz)))
        if math.hypot(x - x0 - t * dx, z - z0 - t * dz) < margin:
            return True
    return False


def busy(x, z, r=2.0):
    """A point a building or a placed unit stands on (or near)."""
    for e in ENTITIES:
        if e.get('activation') == 'script' and not e['definition'].startswith('building.'):
            continue
        ex, ez = e['position']['x'], e['position']['y']
        if e['definition'].startswith('building.'):
            w, dd = footprint(e['definition'])
            if e['rotation'] in (90.0, 270.0):
                w, dd = dd, w
            if abs(x - ex) < w / 2 + r and abs(z - ez) < dd / 2 + r:
                return True
        elif math.hypot(x - ex, z - ez) < r + 2:
            return True
    return False


def road():
    """The road: a worn track with flat stones along its middle, lantern posts by it, stony
    ground at its bends."""
    path = [(-8, ROAD_Z)] + ROUTE[1:]
    k = 0
    for (x0, z0), (x1, z1) in zip(path, path[1:]):
        length = math.hypot(x1 - x0, z1 - z0)
        n = max(1, round(length / 15))
        yaw = heading(x1 - x0, z1 - z0)
        for i in range(n):
            t = (i + 0.5) / n
            put('glass-road-track-' + 'ab'[k % 2], x0 + (x1 - x0) * t + rng.uniform(-1, 1),
                z0 + (z1 - z0) * t + rng.uniform(-1, 1), yaw + rng.uniform(-0.08, 0.08) + (math.pi if k % 3 == 2 else 0))
            k += 1
    for x, z in ((OUTPOST_W[0], ROAD_Z), (232, ROAD_Z + 4), (RAMP_X, ROAD_Z), (RAMP_X, 196), (306, 174), (340, 166)):
        put('glass-road-stony-ground', x + rng.uniform(-2, 2), z + rng.uniform(-2, 2), rng.uniform(0, TAU))
    # Lantern posts in pairs, off the track, the whole way.
    for x in (40, 88, 124, 204, 260):
        for s in (-1, 1):
            if not busy(x, ROAD_Z + 13 * s, 2):
                put('glass-road-lantern', x, ROAD_Z + 13 * s, rng.uniform(0, TAU))
    for x, z in ((278, 222), (314, 222), (318, 158), (362, 158)):
        if not busy(x, z, 2):
            put('glass-road-lantern', x, z, rng.uniform(0, TAU))


def outposts():
    # The west outpost: a palisade under the terrace behind its north tower, posts and
    # stones round the south one, the well's tank.
    palisade([(142, 208.5), (186, 208.5)])
    for x, z in ((150, 264), (178, 266)):
        put('glass-road-palisade-post', x, z, rng.uniform(0, TAU))
    put('glass-road-tank-small', 196, 250, 0.4)
    put('glass-road-fieldstones-small', 141, 262, 1.1)
    for x, z in ((OUTPOST_W[0], 216), (OUTPOST_W[0], 256)):
        put('glass-road-stony-ground', x, z, rng.uniform(0, TAU), 0.9)
    # The east outpost at the top of the main ramp: palisades along the terrace's edge either
    # side of the way up, the well's tank.
    palisade([(226, 199), (256, 199)])
    palisade([(340, 199), (372, 199)])
    put('glass-road-tank-small', 362, 186, 1.6)
    for x, z in ((263.5, 184), (331.5, 184)):
        put('glass-road-stony-ground', x, z, rng.uniform(0, TAU), 0.9)
    # The toll house: a watchtower by the road, stones and a lantern, the peddler's yard.
    put('glass-road-palisade-tower', 262, 268, 0.2)
    put('glass-road-stony-ground', 236, 276, 0.6, 1.2)
    put('glass-road-fieldstones-small', 254, 296, 2.0)
    put('glass-road-fieldstones-small', 210, 292, 0.7)
    put('glass-road-tank-small', 213, 278, 1.0, 0.85)


def station():
    """The palisade round the plateau, the front wall and its gate towers, the condenser and
    its glass in the yard."""
    x0, z0, x1, z1 = PLATEAU
    # Round the plateau's rim (west, north, east), and its south rim either side of the ramp.
    palisade([(STATION_RAMP[0] - 3, z1 - 3), (x0 + 3, z1 - 3), (x0 + 3, z0 + 3), (x1 - 3, z0 + 3),
              (x1 - 3, z1 - 3), (STATION_RAMP[1] + 3, z1 - 3)])
    # Along the tops of the ramp's notch, down to the gate; watchtowers flanking its mouth.
    palisade([(STATION_RAMP[0] - 3, z1 - 3), (STATION_RAMP[0] - 3, 100)])
    palisade([(STATION_RAMP[1] + 3, z1 - 3), (STATION_RAMP[1] + 3, 100)])
    put('glass-road-palisade-tower', STATION_RAMP[0] - 9, z1 - 8, 0.0)
    put('glass-road-palisade-tower', STATION_RAMP[1] + 9, z1 - 8, 0.0)
    put('glass-road-palisade-tower', x0 + 6, z0 + 6, 0.3)
    put('glass-road-palisade-tower', x1 - 6, z0 + 6, -0.3)
    # The yard: the condenser at the end of the axis, glass round it, stony ground.
    put('glass-road-condenser', *CONDENSER, 0.0)
    put('glass-road-tank', 322, 46, 0.6)
    put('glass-road-tank', 358, 44, 2.4)
    put('glass-road-tank-small', 298, 76, 1.2)
    put('glass-road-tank-small', 396, 64, 0.3)
    for x, z in ((318, 94), (362, 94), (326, 70), (354, 70)):
        put('glass-road-lantern', x, z, rng.uniform(0, TAU))
    for x, z, s in ((340, 84, 1.2), (340, 92, 1.1), (318, 84, 1.0), (360, 86, 1.0), (300, 112, 0.9),
                    (390, 116, 0.9), (330, 48, 0.8)):
        put('glass-road-stony-ground', x, z, rng.uniform(0, TAU), s)
    put('glass-road-fieldstones', 300, 118, 0.8)
    put('glass-road-fieldstones-small', 396, 120, 2.2)
    # The forecourt below the gate: stony ground, a heap or two by the palisade.
    for x, z in ((330, 156), (352, 158), (316, 168), (370, 166)):
        put('glass-road-stony-ground', x, z, rng.uniform(0, TAU), 1.1)
    put('glass-road-fieldstones', 304, 156, 1.3)
    put('glass-road-fieldstones', 384, 158, 0.2)
    # Where the old tunnel comes up outside the west wall: an old collapse.
    put('giants-pebbles', TUNNEL_OUT[0] - 14, TUNNEL_OUT[1] - 6, 1.0, 0.6)
    put('glass-road-stony-ground', TUNNEL_OUT[0], TUNNEL_OUT[1] + 8, 0.4)


def places():
    """Low pieces that make the side places readable."""
    # The tunnel's mouth at the camp: a root arch over the gap.
    put('lookdev-root-arch', 54, TUNNEL_IN[1], HALF_PI, 0.9)
    # The meadow's landmark, an acorn cup with its acorn: the scale at a glance.
    put('giants-acorn-cup', 236, 118, 0.6, 1.1)
    put('giants-acorn', 222, 108, 2.2, 1.0)
    # The bog: cattails along the shore, lilies on the water.
    for k in range(22):
        a = k * TAU / 22 + rng.uniform(-0.08, 0.08)
        r = 28 + rng.uniform(-2, 3)
        if 1.2 < a < 2.0:
            continue  # its way in from the camp's side stays clear
        for _ in range(rng.randint(2, 4)):
            put('shore-cattails', BOG[0] + r * math.cos(a) + rng.uniform(-2, 2),
                BOG[1] + r * math.sin(a) + rng.uniform(-2, 2), rng.uniform(0, 6), rng.uniform(6, 10))
    for k in range(9):
        a, r = rng.uniform(0, TAU), rng.uniform(12, 24)
        put('shore-lily-white' if k % 3 else 'shore-lily-pink', BOG[0] + r * math.cos(a), BOG[1] + r * math.sin(a),
            rng.uniform(0, 6), rng.uniform(6, 9))
    # The meadow's pond.
    for k in range(5):
        a, r = rng.uniform(0, TAU), rng.uniform(2, 8)
        put('shore-lily-white', POND[0] + r * math.cos(a), POND[1] + r * math.sin(a), rng.uniform(0, 6), rng.uniform(5, 7))
    # The hornets' ledge: their paper nest, an empty snail shell beside it.
    put('giants-hornet-nest', LEDGE[0] + 4, LEDGE[1] - 16, 0.4, 0.9)
    put('giants-snail-shell', LEDGE[0] - 16, LEDGE[1] - 8, 2.6, 0.8)
    # The Hollow Stag's grove: a ring of sorrel and clover round the knoll, mossy stones.
    for k in range(11):
        a = -HALF_PI + 0.55 + k * (TAU - 1.1) / 10
        if abs(math.remainder(a - math.pi, TAU)) < 0.9:
            continue  # its ramp, on the west
        r = 22 + rng.uniform(-1, 1)
        put('giants-sorrel' if k % 2 else 'giants-clover', GROVE[0] + r * math.cos(a), GROVE[1] + r * math.sin(a),
            rng.uniform(0, 6), rng.uniform(0.9, 1.15))
    for x, z in ((GROVE[0] + 10, GROVE[1] + 12), (GROVE[0] + 13, GROVE[1] - 8)):
        put('giants-mossy-stone', x, z, rng.uniform(0, 6), 0.7)
    for k in range(5):
        a, r = rng.uniform(0, TAU), rng.uniform(4, 14)
        put('underleaf-patch-moss-' + 'ab'[k % 2], GROVE[0] + r * math.cos(a), GROVE[1] + r * math.sin(a), rng.uniform(0, 6), 0.8)
    # The surveyor's glade: a dead beetle's husk, his things spilled.
    put('giants-beetle-husk', GLADE[0] - 12, GLADE[1] - 2, 2.4, 1.1)
    put('glass-road-tank-small', GLADE[0] + 4, GLADE[1] - 12, 1.2, 0.8)
    put('giants-dew-drops', GLADE[0] + 12, GLADE[1] + 2, 0.4, 1.0)
    # The springs' surroundings: stones.
    put('glass-road-fieldstones-small', 86, 132, 0.3)
    put('glass-road-fieldstones-small', 318, 360, 1.9)
    # The camp: an oak leaf and an acorn at its edge.
    put('giants-leaf-oak', 92, 300, 1.1, 0.7)


def verges(frame):
    """Stony ground along the edges of play (Jiri: 'a little more stony grounds'): heaps of
    fieldstones and loose boulders where open ground meets the frame, never on the road."""
    done, tries = 0, 0
    while done < 46 and tries < 20000:
        tries += 1
        x, z = rng.uniform(36, 412), rng.uniform(36, 412)
        if frame.depth(x, z) > 0 or near_route(x, z, 22) or busy(x, z, 6):
            continue
        # Only where the frame is close by (within 5 cells) on some side.
        if not any(frame.depth(x + dx, z + dz) > 0 for dx, dz in ((5, 0), (-5, 0), (0, 5), (0, -5))):
            continue
        if any(math.hypot(x - o['x'], z - o['z']) < 14 for o in SCENERY if 'fieldstones' in o['asset']):
            continue
        put(rng.choice(['glass-road-fieldstones', 'glass-road-fieldstones-small', 'glass-road-fieldstones-small']),
            x, z, rng.uniform(0, TAU), rng.uniform(0.8, 1.1))
        done += 1
    for _ in range(30):
        s = spot(frame, 0.6, 4)
        if s:
            put(rng.choice(['woodland-moss-boulder', 'shore-boulder-a', 'shore-boulder-b', 'meadow-boulder']),
                s[0], s[1], rng.uniform(0, TAU), rng.uniform(0.9, 1.4))


def frame_giants(frame):
    """The forest floor's giants in the frame round the playable ground."""
    for piece, x, z, s, yaw in [
            ('beech-trunk', 444, 446, 1.0, 0.6), ('beech-trunk', -6, 10, 1.1, 2.0),
            ('root', 398, 418, 1.0, 2.6), ('root', 430, 384, 0.9, 1.4), ('root', 30, 40, 0.9, 5.4),
            ('branch', 196, 10, 1.15, 0.06), ('feather', 252, 432, 0.9, 2.9),
            ('parasol', 110, 6, 1.0, 0.5), ('parasol', 330, 4, 0.9, 1.7), ('parasol', 446, 230, 0.85, 2.4),
            ('fern-crown', 6, 150, 0.9, 0.4), ('fern-crown', 444, 300, 0.9, 1.6), ('fern-crown', 150, 440, 0.8, 3.0),
            ('shelf-fungus', 420, 424, 0.9, 3.6), ('shelf-fungus', 22, 34, 0.8, 0.8)]:
        giant(frame, piece, x, z, s, yaw=yaw, force=True)
    # The briar thicket: a hedge of burrs, bracken and withered stems round the clearing,
    # open where its ways in are (west, to the road; south, to the grove).
    for k in range(64):
        a = k * TAU / 64
        if abs(math.remainder(a - math.pi, TAU)) < 0.55 or abs(math.remainder(a - HALF_PI, TAU)) < 0.45:
            continue
        for r in (38, 44):
            piece = ('chestnut-burr', 'bracken', 'withered', 'chestnut-burr')[(k + r) % 4]
            giant(frame, piece, BRIAR[0] + r * math.cos(a) + rng.uniform(-2, 2),
                  BRIAR[1] + r * math.sin(a) + rng.uniform(-2, 2), rng.uniform(0.7, 1.0), overlap=0.45)
    clusters(frame, ['pine-cone'], 12, 10, 90, size=(2, 4), spread=1.3, scale=(0.75, 1.15))
    clusters(frame, ['pine-needles-a', 'pine-needles-b'], 14, 2, 40, size=(3, 6), spread=1.6, scale=(0.7, 1.05))
    clusters(frame, ['dew-drops', 'dew-drop'], 8, 1, 14, size=(1, 3), spread=2.0)


def mission_section(frame):
    return {'objectives': OBJECTIVES, 'regions': REGIONS, 'players': PLAYERS, 'cameras': cameras(frame)}


def save(c):
    print(c.call('problems'))
    print(c.call('save', force=True))


if __name__ == '__main__':
    c = mcp.Client()
    stage = sys.argv[1] if len(sys.argv) > 1 else 'all'
    if stage in ('terrain', 'all'):
        terrain(c)
        save(c)
    if stage in ('place', 'all'):
        entities()
        frame = Frame(PACKAGE)
        road()
        outposts()
        station()
        places()
        verges(frame)
        frame_giants(frame)
        print(len(ENTITIES), 'entities', len(SCENERY), 'scenery objects')
        c.call('edit', label='The Glass Road rebuilt: placements, players, regions, dressing', commands=[
            {'op': 'set', 'document': SC, 'pointer': '/objects', 'value': SCENERY},
            {'op': 'set', 'document': D, 'pointer': '/entities', 'value': ENTITIES},
            {'op': 'set', 'document': D, 'pointer': '/camps', 'value': CAMPS},
            {'op': 'set', 'document': D, 'pointer': '/mission', 'value': mission_section(frame)},
        ])
        save(c)
    if os.environ.get('PICTURE'):
        c.picture(M, os.environ['PICTURE'], scale=2)
