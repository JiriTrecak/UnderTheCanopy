"""The beetle bench: a small flat sandbox map for reviewing beetle buildings in the game,
one finished building beside a stocked beetle main hall, so its states can be triggered by hand
(research at the Shell Forge plays its `work` state, D65). Run with Blender's Python (numpy):

    "$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)" tools/maps/beetle_bench.py
"""
import json, os, shutil, struct, zlib

import numpy as np

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'maps', 'showcase', 'beetle-bench.utcm')
SIZE = 256
SPAN = SIZE + 32
SIDE = SPAN + 1
# Buildings stand on the 4-cell grid at 1.5 mod 4.
START = (113.5, 129.5)
FORGE = (137.5, 125.5)


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
               'description': 'A sandbox for reviewing beetle buildings and their work states.',
               'version': '1.0.0', 'dependencies': {'base': '>=0.3.5'}}
    with open(os.path.join(OUT, 'package.json'), 'w') as f:
        json.dump(package, f, indent=2, ensure_ascii=False)
        f.write('\n')
    game = {
        'biome': 'autumn-forest',
        'sandbox': True,
        'camps': [],
        'size': SIZE,
        'playerStarts': [{'mainFort': 'start.player.1/main-fort', 'player': 1, 'setup': 'setup.beetles',
                          'x': START[0], 'z': START[1]}],
        'entities': [
            # Sandbox maps spawn no start setup: a stocked main hall pays for the research.
            {'definition': 'building.beetles.bastion', 'id': 'bench.hall', 'owner': 'player.1',
             'initialState': {'construction': 'complete', 'inventory': {'item.amber': 20000, 'item.wood': 20000}},
             'position': {'x': START[0], 'y': START[1]}, 'rotation': 0.0},
            {'definition': 'building.beetles.shell-forge', 'id': 'bench.forge', 'owner': 'player.1',
             'initialState': {'construction': 'complete'},
             'position': {'x': FORGE[0], 'y': FORGE[1]}, 'rotation': 0.0},
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
