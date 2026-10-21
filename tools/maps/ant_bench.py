"""The ant bench: a small flat sandbox map for trying the ants' tunnels and kits in the game
(D74). A stocked Nest in the west and a Brood Mound 200 cells east are the two ends of a
tunnel; a Soldier Burrow, a Scent Hall and a Royal Chamber stand by the Nest, and a small
army waits beside it. Order the army east and it goes underground. Run with Blender's
Python (numpy):

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" tools/maps/ant_bench.py
"""
import json, os, shutil, struct, zlib

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'maps', 'showcase', 'ant-bench.utcm')
SIZE = 256
SPAN = SIZE + 32
SIDE = SPAN + 1
# Footprint edges lie on the 4-cell grid (from -0.5): centres at 1.5 mod 4 for 20- and
# 12-cell footprints, 3.5 mod 4 for 8- and 16-cell ones.
START = (29.5, 129.5)
MOUND = (231.5, 127.5)
BUILDINGS = [('building.ants.barracks', 'burrow', (29.5, 101.5)), ('building.ants.scent-hall', 'scent', (49.5, 101.5)),
             ('building.ants.sanctuary', 'royal', (29.5, 157.5)), ('building.ants.house', 'mound', MOUND)]
ARMY = ([('unit.ants.warrior', 6)] + [('unit.ants.archer', 3)] + [('unit.ants.queensguard', 1)]
        + [('unit.ants.banneret', 1), ('unit.ants.nectarist', 1), ('unit.ants.winglancer', 1), ('unit.ants.queen', 1),
           ('unit.ants.royal-alchemist', 1), ('unit.ants.marshal', 1), ('unit.ants.thornblade', 1),
           ('unit.ants.burster', 1)])
HEROES = ('unit.ants.queen', 'unit.ants.royal-alchemist', 'unit.ants.marshal')
# A creep camp for the ants' spells to land on (sandbox maps hold one player).
TARGETS = [('unit.neutral.barkguard', 4), ('unit.neutral.thorn-slinger', 3), ('unit.neutral.briar-hulk', 1)]
WELL = (47.5, 171.5)
CAMP = (126.0, 124.0)


def write_grid(path, data, code, side):
    raw = data.tobytes()
    size = 4 if code in (1, 3) else 1
    if size > 1:
        raw = b''.join(raw[plane::size] for plane in range(size))
    header = b'UTCG' + bytes([1, code, 1, 1 if size > 1 else 0]) + struct.pack('<II', side, side)
    with open(path, 'wb') as f:
        f.write(header + zlib.compress(raw, 9))


def main():
    if os.path.exists(OUT):
        shutil.rmtree(OUT)
    os.makedirs(os.path.join(OUT, 'terrain'))
    package = {'format': 1, 'id': 'ant-bench', 'kind': 'map', 'name': 'Bench: Ants',
               'description': 'A sandbox for trying the ants: tunnels between two mounds, and an army.',
               'version': '1.0.0', 'dependencies': {'base': '>=0.3.5'}}
    with open(os.path.join(OUT, 'package.json'), 'w') as f:
        json.dump(package, f, indent=2, ensure_ascii=False)
        f.write('\n')
    game = {
        'biome': 'frozen-dream',
        'sandbox': True,
        'camps': [{'aggression': 'players', 'aggroRange': 10, 'home': {'x': CAMP[0], 'y': CAMP[1]}, 'id': 'bench.camp',
                   'leash': 24, 'lootPool': 'loot.camp.easy',
                   'members': [f'bench.camp.{i}' for i in range(sum(n for _, n in TARGETS))]}],
        'size': SIZE,
        'playerStarts': [{'mainFort': 'start.player.1/main-fort', 'player': 1, 'setup': 'setup.ants',
                          'x': START[0], 'z': START[1]}],
        'entities': [
            # Sandbox maps spawn no start setup: a stocked main hall pays for everything.
            {'definition': 'building.ants.fort', 'id': 'bench.hall', 'owner': 'player.1',
             'initialState': {'construction': 'complete', 'inventory': {'item.amber': 20000}},
             'position': {'x': START[0], 'y': START[1]}, 'rotation': 0.0},
        ] + [
            {'definition': d, 'id': 'bench.' + name, 'owner': 'player.1', 'initialState': {'construction': 'complete'},
             'position': {'x': p[0], 'y': p[1]}, 'rotation': 0.0} for d, name, p in BUILDINGS
        ] + [
            # Substance comes from a Well built on this deposit (only drop-off storage counts towards
            # the stock, and a map cannot start with a Well standing on its deposit).
            {'definition': 'building.neutral.corrupted-root', 'id': 'bench.deposit', 'owner': 'none',
             'initialState': {'construction': 'complete'}, 'position': {'x': WELL[0], 'y': WELL[1]}, 'rotation': 0.0}
        ] + [
            # Heroes come at full experience, so every ability can be learned and tried.
            {'definition': d, 'id': f'bench.army.{i}', 'owner': 'player.1',
             'position': {'x': 56.0 + 6.0 * (i % 5), 'y': 116.0 + 6.0 * (i // 5)}, 'rotation': 0.0}
            for i, d in enumerate(d for d, n in ARMY for _ in range(n) if d not in HEROES)
        ] + [
            {'definition': d, 'id': f'bench.hero.{i}', 'owner': 'player.1', 'initialState': {'experience': 5200},
             'position': {'x': 60.0 + 14.0 * i, 'y': 150.0}, 'rotation': 0.0}
            for i, d in enumerate(HEROES)
        ] + [
            {'definition': d, 'id': f'bench.camp.{i}', 'owner': 'none',
             'position': {'x': CAMP[0] - 6.0 + 6.0 * (i % 3), 'y': CAMP[1] - 6.0 + 6.0 * (i // 3)}, 'rotation': 180.0}
            for i, d in enumerate(d for d, n in TARGETS for _ in range(n))
        ],
    }
    with open(os.path.join(OUT, 'map.json'), 'w') as f:
        json.dump(game, f, indent=2, ensure_ascii=False)
        f.write('\n')
    flat = np.zeros((SIDE, SIDE), dtype='<f4')
    dry = np.full((SIDE, SIDE), -17.0, dtype='<f4')
    flow = np.zeros((SPAN, SPAN, 4), dtype=np.uint8)
    flow[..., 0] = 128
    flow[..., 1] = 128
    t = os.path.join(OUT, 'terrain')
    write_grid(os.path.join(t, 'heights.grid'), flat, 1, SIDE)
    write_grid(os.path.join(t, 'water-heights.grid'), dry, 1, SIDE)
    write_grid(os.path.join(t, 'water-flow.grid'), flow, 3, SPAN)
    # Flat ground: every 4 × 4-cell block at level 0 (D80), what the heights above are made of.
    columns = SPAN // 4
    write_grid(os.path.join(t, 'blocks.grid'), np.zeros((columns, columns, 4), dtype=np.uint8), 3, columns)
    with open(os.path.join(OUT, 'scenery.json'), 'w') as f:
        json.dump({'objects': []}, f, indent=2)
        f.write('\n')
    print('wrote', OUT)


if __name__ == '__main__':
    main()
