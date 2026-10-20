"""The beetle bench: a small flat sandbox map for reviewing beetle buildings and trying the
beetles' kits in the game. Finished buildings stand beside a stocked tier-2 hall, so their
states can be triggered by hand (research at the Shell Forge plays its `work` state, D65), and
the Alembic researches Specialist Training. An army and the three heroes at full experience
wait west of the hall; a creep camp east of the forge is something to cast on. Run with
Blender's Python (numpy):

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" tools/maps/beetle_bench.py
"""
import json, os, shutil, struct, zlib

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'maps', 'showcase', 'beetle-bench.utcm')
SIZE = 256
SPAN = SIZE + 32
SIDE = SPAN + 1
# Footprint edges lie on the 4-cell grid (from -0.5): centres at 1.5 mod 4 for the
# 20-cell hall, 3.5 mod 4 for the 16-cell forge.
START = (113.5, 129.5)
FORGE = (135.5, 127.5)
BUILDINGS = [('building.beetles.alembic', 'alembic', (93.5, 129.5)),
             ('building.beetles.ancestor-shrine', 'shrine', (113.5, 149.5))]
ARMY = ([('unit.beetles.worker', 2), ('unit.beetles.bolter', 4), ('unit.beetles.shieldbearer', 2),
         ('unit.beetles.breacher', 2), ('unit.beetles.arcwright', 1), ('unit.beetles.mender', 1),
         ('unit.beetles.warden', 1), ('unit.beetles.steamwing', 1), ('unit.beetles.bombard', 1),
         ('unit.beetles.juggernaut', 1)])
HEROES = ('unit.beetles.forgemaster', 'unit.beetles.coilkeeper', 'unit.beetles.hornbreaker')
# A creep camp for the beetles' spells to land on (sandbox maps hold one player).
TARGETS = [('unit.neutral.barkguard', 4), ('unit.neutral.thorn-slinger', 3), ('unit.neutral.briar-hulk', 1)]
WELL = (91.5, 147.5)
CAMP = (170.0, 124.0)


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
    package = {'format': 1, 'id': 'beetle-bench', 'kind': 'map', 'name': 'Bench: Beetles',
               'description': 'A sandbox for reviewing beetle buildings and trying the beetle army and heroes.',
               'version': '1.0.0', 'dependencies': {'base': '>=0.3.5'}}
    with open(os.path.join(OUT, 'package.json'), 'w') as f:
        json.dump(package, f, indent=2, ensure_ascii=False)
        f.write('\n')
    game = {
        'biome': 'autumn-forest',
        'sandbox': True,
        'camps': [{'aggression': 'players', 'aggroRange': 10, 'home': {'x': CAMP[0], 'y': CAMP[1]}, 'id': 'bench.camp',
                   'leash': 24, 'lootPool': 'loot.camp.easy',
                   'members': [f'bench.camp.{i}' for i in range(sum(n for _, n in TARGETS))]}],
        'size': SIZE,
        'playerStarts': [{'mainFort': 'start.player.1/main-fort', 'player': 1, 'setup': 'setup.beetles',
                          'x': START[0], 'z': START[1]}],
        'entities': [
            # Sandbox maps spawn no start setup: a stocked main hall pays for the research.
            {'definition': 'building.beetles.great-bastion', 'id': 'bench.hall', 'owner': 'player.1',
             'initialState': {'construction': 'complete', 'inventory': {'item.amber': 20000}},
             'position': {'x': START[0], 'y': START[1]}, 'rotation': 0.0},
            {'definition': 'building.beetles.shell-forge', 'id': 'bench.forge', 'owner': 'player.1',
             'initialState': {'construction': 'complete'},
             'position': {'x': FORGE[0], 'y': FORGE[1]}, 'rotation': 0.0},
        ] + [
            {'definition': d, 'id': 'bench.' + name, 'owner': 'player.1', 'initialState': {'construction': 'complete'},
             'position': {'x': p[0], 'y': p[1]}, 'rotation': 0.0} for d, name, p in BUILDINGS
        ] + [
            # Substance comes from a Well built on this deposit (only drop-off storage counts towards
            # the stock, and a map cannot start with a Well standing on its deposit).
            {'definition': 'building.neutral.corrupted-root', 'id': 'bench.deposit', 'owner': 'none',
             'initialState': {'construction': 'complete'}, 'position': {'x': WELL[0], 'y': WELL[1]}, 'rotation': 0.0}
        ] + [
            {'definition': d, 'id': f'bench.army.{i}', 'owner': 'player.1',
             'position': {'x': 40.0 + 8.0 * (i % 5), 'y': 104.0 + 8.0 * (i // 5)}, 'rotation': 0.0}
            for i, d in enumerate(d for d, n in ARMY for _ in range(n))
        ] + [
            # Heroes come at full experience, so every ability can be learned and tried.
            {'definition': d, 'id': f'bench.hero.{i}', 'owner': 'player.1', 'initialState': {'experience': 5200},
             'position': {'x': 46.0 + 16.0 * i, 'y': 150.0}, 'rotation': 0.0}
            for i, d in enumerate(HEROES)
        ] + [
            {'definition': d, 'id': f'bench.camp.{i}', 'owner': 'none',
             'position': {'x': CAMP[0] - 6.0 + 6.0 * (i % 3), 'y': CAMP[1] - 6.0 + 6.0 * (i // 3)}, 'rotation': 180.0}
            for i, d in enumerate(d for d, n in TARGETS for _ in range(n))
        ],
        'terrain': {'paint': [], 'textureGrid': {'cellSize': 5, 'origin': [-0.5, -0.5]}, 'waterProfiles': []},
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
    write_grid(os.path.join(t, 'grass.grid'), flat, 1, SIDE)
    write_grid(os.path.join(t, 'rock.grid'), flat, 1, SIDE)
    with open(os.path.join(OUT, 'scenery.json'), 'w') as f:
        json.dump({'objects': []}, f, indent=2)
        f.write('\n')
    print('wrote', OUT)


if __name__ == '__main__':
    main()
