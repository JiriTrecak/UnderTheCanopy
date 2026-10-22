"""The Hollow Brood's units, camps, features and its mission section: the computer players,
regions, objectives and saved cameras (tools/maps/hollow_brood.py `units`). The script is
the package's `mission.lua`; the ids here are the ones it names.

Players: 1 the Queen's column; 8 the feral brood (enemy, Passive: its ants guard where they
stand; slot 8 for white, the brood bled pale); 3 the survivors (ally, dormant until the
rubble breaks; the dead sentry is theirs too, spawned in its scene); 4 Tiv's lost patrol
(ally, dormant until the webs are cleared)."""
import math

ENTITIES = []
CAMPS = []


def unit(id, definition, owner, x, z, rotation=45.0, script=False, scale=None, **state):
    e = {'id': id, 'definition': definition, 'owner': owner, 'position': {'x': x, 'y': z}, 'rotation': rotation}
    if script:
        e['activation'] = 'script'
    if scale:
        e['appearance'] = {'scale': scale}
    if state:
        e['initialState'] = state
    ENTITIES.append(e)
    return id


def camp(id, home, members, aggro=10, leash=22, aggression='players', pool=None, drops=None):
    c = {'id': id, 'members': members, 'home': {'x': home[0], 'y': home[1]}, 'aggroRange': aggro, 'leash': leash,
         'aggression': aggression}
    if pool:
        c['lootPool'] = pool
    if drops is not None:
        c['fixedDrops'] = drops
    CAMPS.append(c)


def creeps(prefix, definition, spots, rotation=0.0):
    return [unit(f'{prefix}.{i}', definition, 'none', x, z, rotation) for i, (x, z) in enumerate(spots)]


def build():
    ENTITIES.clear()
    CAMPS.clear()
    P1, FERAL, SURVIVORS, PATROL = 'player.1', 'player.8', 'player.3', 'player.4'
    # --- The Queen's column, running north-east along the root path. ---------------------
    unit('queen', 'unit.ants.queen', P1, 60, 334, 40)
    unit('tiv', 'unit.ants.warrior', P1, 112, 304, 220)
    for i, (x, z) in enumerate([(56, 330), (65, 339), (51, 338), (60, 345)]):
        unit(f'raider.{i + 1}', 'unit.ants.warrior', P1, x, z, 40)
    for i, (x, z) in enumerate([(48, 346), (54, 352)]):
        unit(f'needler.{i + 1}', 'unit.ants.archer', P1, x, z, 40)

    # --- 1. The start: two amber caches by the road. -------------------------------------
    unit('cache.salve', 'building.neutral.amber-cache', 'none', 69.5, 305.5, 0, inventory={'item.resin-salve': 1})
    unit('cache.dew', 'building.neutral.amber-cache', 'none', 93.5, 329.5, 0, inventory={'item.moon-dew': 1})

    # --- 2. The mushroom circle on the road (easy). --------------------------------------
    camp('mushrooms', (134, 298),
         creeps('mushrooms', 'unit.neutral.capling', [(128, 293), (141, 295), (133, 305)])
         + creeps('mushrooms.puffcap', 'unit.neutral.puffcap', [(135, 298)]),
         aggro=10, pool='loot.camp.easy')

    # --- 3. The ford: dew sprites by the water; a Moon Dew. -------------------------------
    camp('ford', (194, 262),
         creeps('ford.wisp', 'unit.neutral.puddle-wisp', [(186, 262), (198, 258)])
         + creeps('ford.sprite', 'unit.neutral.dew-sprite', [(192, 255), (202, 262)]),
         aggro=10, drops=['item.moon-dew'])

    # --- 4. The dead sentry: spawned out of sight, then it falls. --------------------------
    unit('sentry', 'unit.ants.warrior', SURVIVORS, 198, 231, 220, script=True, scale=1.15)

    # --- 5. The lost patrol: the Hungry Weblings' webbed hollow, the cocooned patrol. -----
    camp('webs', (74, 194),
         creeps('webs', 'unit.neutral.webling', [(80, 196), (70, 200), (76, 188)])
         + creeps('webs.spitter', 'unit.neutral.spitter', [(66, 192)]),
         aggro=11, leash=24, drops=['item.resin-salve'])
    unit('patrol.kesh', 'unit.ants.warrior', PATROL, 84, 192, 90, script=True)
    unit('patrol.2', 'unit.ants.warrior', PATROL, 79, 198, 120, script=True)
    unit('whistle', 'item.scout-whistle', 'none', 82, 196, 0, script=True)
    # Beyond the webs, an old burrow: a lone Spitter and a forgotten cache.
    camp('burrow', (72, 112), creeps('burrow', 'unit.neutral.spitter', [(70, 108)]), aggro=8, leash=16)
    unit('cache.burrow', 'building.neutral.amber-cache', 'none', 73.5, 113.5, 0, inventory={'item.trailkeeper-flask': 1})

    # --- 6. The sick pool: twisted Raiders and a Burster at the over-bright water. --------
    for i, (x, z) in enumerate([(118, 170), (124, 160), (112, 176)]):
        unit(f'feral.sick.{i + 1}', 'unit.ants.warrior', FERAL, x, z, 200, scale=1.12)
    unit('feral.sick.burster', 'unit.ants.burster', FERAL, 128, 168, 200, scale=1.1)

    # --- A sap spring by the stream, east of the ford. ------------------------------------
    unit('spring', 'building.neutral.sap-spring', 'none', 261.5, 237.5, 0)
    # The beetle wreck: Weblings nesting in a husk, a lost beetle crate.
    camp('husk', (306, 294),
         creeps('husk', 'unit.neutral.webling', [(300, 290), (310, 288)])
         + creeps('husk.spitter', 'unit.neutral.spitter', [(304, 298), (312, 296)]),
         aggro=10, pool='loot.camp.easy')
    unit('crate.husk', 'building.neutral.supply-crate', 'none', 305.5, 293.5, 0, inventory={'item.quickstep-spurs': 1})

    # --- 7. The terrace: thornback foragers among the berries; the hornets' ledge. --------
    camp('thorns', (200, 106),
         creeps('thorns', 'unit.neutral.thorn-slinger', [(194, 102), (204, 100), (198, 112), (208, 108)]),
         aggro=10, pool='loot.camp.easy')
    camp('hornets', (272, 158),
         creeps('hornets.guard', 'unit.neutral.hornet-guard', [(266, 152), (274, 150)])
         + creeps('hornets.wasp', 'unit.neutral.needle-wasp', [(270, 162), (280, 156)])
         + creeps('hornets.mother', 'unit.neutral.nest-mother', [(278, 164)]),
         aggro=12, leash=20, drops=['item.thornband', 'item.restoration-draught'])

    # --- 8. The Hollow's rim: a feral picket; the survivors behind the rubble. -----------
    for i, (x, z) in enumerate([(298, 116), (306, 112)]):
        unit(f'feral.rim.{i + 1}', 'unit.ants.warrior', FERAL, x, z, 200, scale=1.12)
    unit('rubble', 'building.neutral.rubble', 'none', 329.5, 145.5, 0, health=260)
    unit('survivor.1', 'unit.ants.warrior', SURVIVORS, 326, 155, 0, script=True)
    unit('survivor.2', 'unit.ants.warrior', SURVIVORS, 327, 163, 330, script=True)
    unit('survivor.3', 'unit.ants.archer', SURVIVORS, 329, 171, 20, script=True)
    unit('nurse', 'unit.ants.settler', SURVIVORS, 336, 154, 340, script=True)
    # The sap spring that kept them alive: rest here before going down.
    unit('chamber.spring', 'building.neutral.sap-spring', 'none', 337.5, 165.5, 0)

    # --- 9. Secret: the Moonlit Hollow. ---------------------------------------------------
    unit('lantern', 'item.broodkeeper-lantern', 'none', 206, 53, 0)
    camp('wisp', (215, 48), creeps('wisp', 'unit.neutral.willow-wisp', [(215, 48)]), aggro=4, leash=12,
         aggression='passive', pool='loot.camp.medium')

    # --- 10. The tree frog on the lily pool. ----------------------------------------------
    camp('frog', (110, 288), [unit('frog', 'unit.neutral.tree-frog', 'none', 110, 288, 160)], aggro=3, leash=30,
         aggression='passive')
    unit('heartseed', 'item.heartseed', 'none', 110, 289, 0, script=True)

    # --- 11. The Hollow: the gate, the pools, the nest and Mother Ashka. ------------------
    for i, (x, z, d) in enumerate([(298, 86, 'unit.ants.warrior'), (310, 86, 'unit.ants.warrior'),
                                   (304, 80, 'unit.ants.burster')]):
        unit(f'feral.gate.{i + 1}', d, FERAL, x, z, 180, scale=1.12)
    for i, (x, z, d) in enumerate([(276, 82, 'unit.ants.burster')]):
        unit(f'feral.pools.{i + 1}', d, FERAL, x, z, 120, scale=1.12)
    for i, (x, z, d) in enumerate([(326, 76, 'unit.ants.warrior'), (334, 80, 'unit.ants.archer')]):
        unit(f'feral.east.{i + 1}', d, FERAL, x, z, 240, scale=1.12)
    for i, (x, z, d) in enumerate([(308, 56, 'unit.ants.warrior'), (312, 64, 'unit.ants.warrior')]):
        unit(f'feral.nest.{i + 1}', d, FERAL, x, z, 200, scale=1.1)
    unit('ashka', 'unit.ants.reaver-queen', FERAL, 290, 58, 200)

    players = [
        # The feral brood, sick with the substance: the same violet-grey as in Founding.
        {'player': 8, 'race': 'ants', 'team': 'enemy', 'vision': False, 'color': '#8a7f96',
         'ai': {'behaviour': 'passive'}},
        {'player': 3, 'race': 'ants', 'team': 'ally', 'ai': {'behaviour': 'passive', 'dormant': True}},
        {'player': 4, 'race': 'ants', 'team': 'ally', 'ai': {'behaviour': 'passive', 'dormant': True}},
    ]
    regions = [
        ('start', 84, 318, 24), ('mushrooms', 134, 298, 26), ('ford', 192, 264, 20),
        ('sentry', 198, 242, 16), ('junction', 150, 208, 18),
        ('webs', 76, 196, 26), ('sick', 126, 170, 28), ('spring', 262, 238, 18), ('husk', 306, 294, 18),
        ('terrace', 182, 116, 18), ('hornets', 258, 140, 14), ('glade', 206, 54, 13), ('rim', 300, 118, 14),
        ('chamber', 330, 140, 14), ('hollow', 304, 92, 14), ('nest', 300, 66, 30), ('frog', 110, 288, 10),
    ]
    objectives = [
        {'id': 'reach', 'title': 'The Hollow', 'description': 'Follow the root path north-east to the Hollow'},
        {'id': 'survivors', 'title': 'Survivors', 'description': 'Break through the fallen tunnel east of the rim and find who is digging'},
        {'id': 'ashka', 'title': 'Mother Ashka', 'description': 'Go down into the Hollow and put Mother Ashka to rest'},
        {'id': 'patrol', 'title': 'The lost patrol', 'description': 'Find Kesh\'s patrol in the old webs to the north-west',
         'optional': True},
    ]
    return players, regions, objectives


def camera(id, x, z, height, distance, pitch, yaw, fov=36, roll=None):
    c = {'id': id, 'x': x, 'y': z, 'height': height, 'distance': distance, 'pitch': pitch, 'yaw': yaw, 'fov': fov}
    if roll:
        c['roll'] = roll
    return c


def eye(c):
    """Where a camera's eye stands (x, z, height above the point)."""
    h = c['distance'] * math.cos(math.radians(c['pitch']))
    y = math.radians(c['yaw'])
    return c['x'] + h * math.sin(y), c['y'] + h * math.cos(y), c['distance'] * math.sin(math.radians(c['pitch']))


CAMERAS = [
    # The opening: low by the road as the column comes up, then over the stream behind it,
    # two shoulders for the talk, and wide as they set off.
    camera('open-road', 70, 328, 2, 38, 12, 128, 38),
    camera('open-follow', 84, 318, 2, 54, 30, 150, 38),
    camera('over-queen', 97, 310, 2.5, 34, 13, 300, 30),
    camera('over-tiv', 85, 317, 3.5, 26, 8, 118, 34, 1.5),
    camera('open-wide', 100, 308, 0, 80, 42, 160, 40),
    # The dead sentry.
    camera('sentry', 196, 238, 1, 44, 50, 90, 36),
    camera('sentry-low', 194, 238, 1, 30, 34, 20, 32),
    # The webs: the cocoons break.
    camera('webs', 82, 194, 2, 30, 24, 60, 38),
    # The rim: the Hollow below, its nest still lit.
    camera('rim-wide', 308, 66, 0, 78, 36, 4, 42),
    camera('rim-tiv', 300, 116, 3, 24, 10, 70, 34),
    # The survivors' tunnel.
    camera('chamber', 331, 148, 2, 28, 28, 200, 38),
    camera('over-nurse', 330, 140, 3, 22, 9, 30, 34),
    # Ashka.
    camera('hollow-wide', 304, 70, 0, 70, 38, 0, 42),
    camera('ashka', 292, 62, 4, 34, 18, 45, 38),
    camera('ashka-close', 295, 66, 5, 24, 12, 110, 34),
    camera('closing', 300, 66, 2, 30, 30, 30, 36),
    # The frog.
    camera('frog', 110, 288, 1, 16, 22, 340, 34),
]
