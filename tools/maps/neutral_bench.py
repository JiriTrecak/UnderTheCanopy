"""The neutral bench: a small flat sandbox map for trying the neutral map features (D86): a
Pill-Bug Peddler with a hero beside it and amber to spend, the two springs with wounded
units near them, crates and a seed pod to break, rubble in both turns, and items lying on
the ground. Run with Blender's Python (numpy):

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" tools/maps/neutral_bench.py
"""
import json, os, shutil, struct, zlib

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'maps', 'showcase', 'neutral-bench.utcm')
SIZE = 256
SPAN = SIZE + 32
SIDE = SPAN + 1
# Footprint edges lie on the 4-cell grid (from -0.5): centres at 1.5 mod 4 for 4-, 12- and
# 20-cell sides, 3.5 mod 4 for 8-cell ones.
START = (29.5, 129.5)
FEATURES = [
    ('building.neutral.merchant', 'merchant', (101.5, 109.5), 0.0),
    ('building.neutral.sap-spring', 'sap-spring', (121.5, 109.5), 0.0),
    ('building.neutral.dew-spring', 'dew-spring', (141.5, 109.5), 0.0),
    ('building.neutral.amber-cache', 'amber-cache', (101.5, 141.5), 0.0),
    ('building.neutral.supply-crate', 'supply-crate', (109.5, 141.5), 0.0),
    ('building.neutral.seed-pod', 'seed-pod', (117.5, 141.5), 0.0),
    ('building.neutral.rubble', 'rubble', (133.5, 141.5), 0.0),
    ('building.neutral.rubble', 'rubble-turned', (149.5, 145.5), 90.0),
]
ITEMS = [('item.moon-dew', (126.0, 152.0)), ('item.thornband', (131.0, 152.0))]
ARMY = [('unit.ants.warrior', (104.0 + 4.0 * i, 128.0)) for i in range(4)]
WOUNDED = [('unit.ants.warrior', (121.0, 119.0), 40), ('unit.ants.archer', (125.0, 119.0), 30)]
HERO = ('unit.ants.queen', (101.0, 119.0))


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
    package = {'format': 1, 'id': 'neutral-bench', 'kind': 'map', 'name': 'Bench: Neutral features',
               'description': 'A sandbox for the neutral map features: a merchant, springs, crates, rubble and items on the ground.',
               'version': '1.0.0', 'dependencies': {'base': '>=0.3.5'}}
    with open(os.path.join(OUT, 'package.json'), 'w') as f:
        json.dump(package, f, indent=2, ensure_ascii=False)
        f.write('\n')
    entity = lambda d, i, p, owner='player.1', r=0.0, state=None: dict(
        {'definition': d, 'id': i, 'owner': owner, 'position': {'x': p[0], 'y': p[1]}, 'rotation': r},
        **({'initialState': state} if state else {}))
    game = {
        'biome': 'frozen-dream',
        'sandbox': True,
        'camps': [],
        'size': SIZE,
        'playerStarts': [{'mainFort': 'start.player.1/main-fort', 'player': 1, 'setup': 'setup.ants',
                          'x': START[0], 'z': START[1]}],
        'entities': [
            # Sandbox maps spawn no start setup: a stocked main hall pays for the merchant's wares.
            entity('building.ants.fort', 'bench.hall', START,
                   state={'construction': 'complete', 'inventory': {'item.amber': 2000}}),
        ] + [
            entity(d, 'bench.' + name, p, 'none', r, {'construction': 'complete'}) for d, name, p, r in FEATURES
        ] + [
            entity(d, f'bench.item.{i}', p, 'none') for i, (d, p) in enumerate(ITEMS)
        ] + [
            entity(d, f'bench.army.{i}', p) for i, (d, p) in enumerate(ARMY)
        ] + [
            entity(d, f'bench.wounded.{i}', p, state={'health': hp}) for i, (d, p, hp) in enumerate(WOUNDED)
        ] + [
            entity(HERO[0], 'bench.hero', HERO[1], state={'experience': 5200}),
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
