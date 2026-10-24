"""Writes the combat lab's prepared scenarios (docs/editor.md, "The Combat Lab") into
labs/ at the repository root. Each one isolates a movement or fighting situation to watch
and measure before and after a change. Run: python3 tools/labs/scenarios.py

Units stand on whole cells; buildings on the 4-cell lattice (centre = 4k + size/2 - 0.5).
"""
import json
import os

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'labs')
TICKS = 40


def spans(size, closed):
    """Closed cells (a set of (x, y)) as merged row spans."""
    out = []
    for y in range(size):
        x = 0
        while x < size:
            if (x, y) in closed:
                start = x
                while x < size and (x, y) in closed:
                    x += 1
                out.append({'y': y, 'x': start, 'length': x - start})
            else:
                x += 1
    return out


def rect(x0, y0, x1, y1):
    """Cells of the box from (x0, y0) to (x1, y1), both included."""
    return {(x, y) for x in range(x0, x1 + 1) for y in range(y0, y1 + 1)}


def block(definition, player, group, count, x, y, spacing, columns=None):
    """`count` units packed in rows around (x, y)."""
    columns = columns or max(1, round(count ** 0.5))
    rows = (count + columns - 1) // columns
    units = []
    for k in range(count):
        cx = (k % columns) - (columns - 1) / 2
        cy = (k // columns) - (rows - 1) / 2
        units.append({
            'definition': definition,
            'owner': f'player.{player}' if player else 'none',
            'x': float(round(x + cx * spacing)),
            'y': float(round(y + cy * spacing)),
            'group': group,
        })
    return units


def building(definition, player, x, y, group=None):
    u = {'definition': definition, 'owner': f'player.{player}', 'x': x, 'y': y}
    if group:
        u['group'] = group
    return u


def order(tick, group, kind, x=None, y=None, target=None):
    o = {'tick': tick, 'group': group, 'order': kind}
    if x is not None:
        o['x'], o['y'] = float(x), float(y)
    if target:
        o['target'] = target
    return o


def scenario(id, name, description, size, seconds, closed=(), units=(), orders=()):
    lab = {'name': name, 'description': description, 'size': size, 'duration': seconds * TICKS}
    if closed:
        lab['closed'] = spans(size, set(closed))
    if units:
        lab['units'] = list(units)
    if orders:
        lab['orders'] = list(orders)
    os.makedirs(OUT, exist_ok=True)
    with open(os.path.join(OUT, f'{id}.lab.json'), 'w') as f:
        f.write(json.dumps(lab, indent=2, ensure_ascii=False) + '\n')


RAIDER, ARCHER, BOMBARDIER, GUARD = (
    'unit.ants.warrior', 'unit.ants.archer', 'unit.ants.bombardier', 'unit.ants.queensguard')
SHIELD, BOLTER = 'unit.beetles.shieldbearer', 'unit.beetles.bolter'

# 1. Two friendly groups crossing paths in the open.
scenario(
    'crossing', 'Two groups crossing',
    'Forty Raiders west to east and forty Shieldbearers south to north cross in the middle: '
    'do they weave through each other or jam?',
    192, 40,
    units=block(RAIDER, 1, 'raiders', 40, 40, 96, 4) + block(SHIELD, 1, 'shields', 40, 96, 152, 4),
    orders=[order(0, 'raiders', 'move', 156, 96), order(0, 'shields', 'move', 96, 40)])

# 2. A hundred bodies through an eight-cell gap.
wall = rect(94, 0, 97, 127) - rect(94, 60, 97, 67)
scenario(
    'choke-100', 'A hundred through a choke',
    'A hundred Shieldbearers squeeze through an eight-cell gap in a wall: throughput, '
    'stalls and the queue at the mouth.',
    192, 60, closed=wall,
    units=block(SHIELD, 1, 'army', 100, 40, 64, 4, columns=10),
    orders=[order(0, 'army', 'move', 160, 64)])

# 3. Two hundred Raiders swarming a walled base with one gate.
ring = rect(140, 60, 220, 63) | rect(140, 156, 220, 159) | rect(217, 60, 220, 159) | (
    rect(140, 60, 143, 159) - rect(140, 102, 143, 117))
scenario(
    'swarm-base', 'A swarm into a base',
    'Two hundred Raiders attack-move into a walled beetle base through its one gate, with '
    'twenty Bolters inside: the swarm at the gate and the surround.',
    256, 90, closed=ring,
    units=block(RAIDER, 1, 'swarm', 200, 50, 110, 4, columns=14)
    + block(BOLTER, 2, 'garrison', 20, 190, 110, 4, columns=5)
    + [building('building.beetles.war-lodge', 2, 195.5, 79.5),
       building('building.beetles.engine-yard', 2, 195.5, 135.5)],
    orders=[order(0, 'swarm', 'attackMove', 196, 110)])

# 4. A mixed army on a long move round obstacles.
rocks = rect(80, 40, 95, 90) | rect(140, 120, 160, 200) | rect(190, 30, 200, 110)
scenario(
    'mixed-army', 'A mixed army on the move',
    'Fast Raiders, slow Bombardiers, Archers and Queensguard together on a long move round '
    'three obstacles: does the army string out, and who arrives when?',
    256, 60, closed=rocks,
    units=block(RAIDER, 1, 'army', 16, 30, 128, 4)
    + block(ARCHER, 1, 'army', 12, 30, 150, 4)
    + block(BOMBARDIER, 1, 'army', 6, 30, 100, 6)
    + block(GUARD, 1, 'army', 8, 44, 128, 5),
    orders=[order(0, 'army', 'move', 226, 128)])

# 5. Movers through a crowd of idle allies.
scenario(
    'idle-crowd', 'Through idle allies',
    'Twenty Raiders walk straight through thirty idle Shieldbearers of their own side: do '
    'the idle ones make way?',
    128, 30,
    units=block(SHIELD, 1, 'idle', 30, 64, 64, 4, columns=6) + block(RAIDER, 1, 'movers', 20, 16, 64, 4),
    orders=[order(0, 'movers', 'move', 112, 64)])

# 6. A U-shaped trap: the straight line to the goal is blocked.
u = rect(60, 40, 63, 88) | rect(60, 40, 100, 43) | rect(60, 85, 100, 88)
scenario(
    'u-trap', 'Round a U-shaped wall',
    'Thirty Raiders whose straight way runs into a U-shaped wall open away from the goal: '
    'the route round, and units caught in the cup.',
    128, 30, closed=u,
    units=block(RAIDER, 1, 'army', 30, 85, 64, 4),
    orders=[order(0, 'army', 'move', 20, 64)])

# 7. Fifty against fifty.
scenario(
    'battle-50', 'Fifty against fifty',
    'Thirty Raiders and twenty Archers against thirty Shieldbearers and twenty Bolters, '
    'attack-moving at each other: the melee surround and the ranged lines behind.',
    192, 60,
    units=block(RAIDER, 1, 'ants', 30, 56, 96, 4) + block(ARCHER, 1, 'ant archers', 20, 36, 96, 4)
    + block(SHIELD, 2, 'beetles', 30, 136, 96, 4) + block(BOLTER, 2, 'beetle guns', 20, 156, 96, 4),
    orders=[order(0, 'ants', 'attackMove', 150, 96), order(0, 'ant archers', 'attackMove', 150, 96),
            order(0, 'beetles', 'attackMove', 40, 96), order(0, 'beetle guns', 'attackMove', 40, 96)])

# 8. Ants through a tunnel: a wall cuts the field, two mounds join both sides.
cut = rect(94, 0, 97, 175) | rect(94, 180, 97, 191) - set()
scenario(
    'tunnel', 'Ants through a tunnel',
    'Thirty Raiders move across a wall whose only way round is far to the south, with a '
    'mound on each side: the queue at the tunnel mouth.',
    192, 60, closed=cut,
    units=block(RAIDER, 1, 'ants', 30, 40, 96, 4)
    + [building('building.ants.house', 1, 63.5, 79.5), building('building.ants.house', 1, 127.5, 79.5)],
    orders=[order(0, 'ants', 'move', 160, 96)])

# 9. One walker and a hundred idle allies, for hand orders.
scenario(
    'through-a-hundred', 'One through a hundred',
    'A hundred idle Shieldbearers stand packed in the middle; a Raider waits west of them '
    'and five more south-west. Select one (click), right-click past the block and watch what '
    'the hundred do.',
    128, 120,
    units=block(SHIELD, 1, 'block', 100, 64, 64, 4, columns=10)
    + block(RAIDER, 1, 'walker', 1, 16, 64, 4)
    + block(RAIDER, 1, 'walkers', 5, 16, 100, 4, columns=5))

# 10. A ring round one standing target.
JUGGERNAUT = 'unit.neutral.hollow-stag'
scenario(
    'ring-still', 'A ring round a standing target',
    'Thirty Raiders attack one Hollow Stag (a big body) standing still: how fast the ring forms, how many '
    'get to bite at once, and whether it closes.',
    128, 30,
    units=block(RAIDER, 1, 'raiders', 30, 30, 64, 4, columns=6) + block(JUGGERNAUT, 2, 'target', 1, 56, 64, 4),
    orders=[order(0, 'raiders', 'attack', target='target'), order(0, 'target', 'hold')])

# 11. A ring round a target walking away.
scenario(
    'ring-fleeing', 'A ring round a fleeing target',
    'Thirty Raiders attack a Shieldbearer that walks away from them: do they catch it from '
    'behind, or overtake and close in front (the lab\'s lead switch)?',
    192, 30,
    units=block(RAIDER, 1, 'raiders', 30, 30, 96, 4, columns=6) + block(SHIELD, 2, 'target', 1, 70, 96, 4),
    orders=[order(0, 'raiders', 'attack', target='target'), order(0, 'target', 'move', 180, 96)])

# 12. Every unit that moves, to check bodies against models (D102).
def moving_units():
    out = []
    for race in ('ants', 'beetles', 'neutral'):
        folder = os.path.join(ROOT, 'base', 'units', race)
        for dirpath, _, names in os.walk(folder):
            for n in sorted(names):
                if not n.endswith('.json'):
                    continue
                with open(os.path.join(dirpath, n)) as f:
                    u = json.load(f)
                if (u.get('behaviors') or {}).get('movement', {}).get('speed'):
                    out.append(u['id'])
    return out

lineup = []
for k, definition in enumerate(moving_units()):
    lineup.append({'definition': definition, 'owner': 'player.1',
                   'x': float(16 + (k % 10) * 16), 'y': float(16 + (k // 10) * 16)})
scenario(
    'body-sizes', 'Every body',
    'Every unit that moves, ten to a row, to check each collision body (the Bodies layer) '
    'against its model from above.',
    192, 5, units=lineup)

# 13. Every player colour at game zoom: how the armies read (D102's model scales).
look = []
for player in range(1, 9):
    y = 28 + (player - 1) * 10
    row = ([RAIDER] * 8) + ([ARCHER] * 3) + ([SHIELD] * 2) + [BOMBARDIER, 'unit.ants.queen']
    x = 30
    for definition in row:
        look.append({'definition': definition, 'owner': f'player.{player}', 'x': float(x), 'y': float(y)})
        x += 6 if definition in (BOMBARDIER, 'unit.ants.queen') else 4
scenario(
    'look-check', 'Every colour at game zoom',
    'Each player\'s Raiders, Archers, Shieldbearers, a Bombardier and a hero, in all eight '
    'colours, to check how units read at game zoom.',
    128, 5, units=look)

print('wrote', len(os.listdir(OUT)), 'scenarios to', OUT)
