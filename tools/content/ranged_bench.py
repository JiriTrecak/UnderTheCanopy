"""The ranged bench (base/maps/showcase/ranged-bench.utcm), for reviewing every ranged attack:

    python3 tools/content/ranged_bench.py

Every ant and beetle ranged unit and tower stands in a row facing a camp of every ranged
forest creature, close enough that both sides shoot from the first ticks; the ground, size and
biome are the beetle bench's (bright sand, the hardest ground for effects to read on).
"""
import json
import os
import shutil

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SHOWCASE = os.path.join(ROOT, 'base', 'maps', 'showcase')
SOURCE = os.path.join(SHOWCASE, 'beetle-bench.utcm')
OUT = os.path.join(SHOWCASE, 'ranged-bench.utcm')

SHOOTERS = ['unit.ants.archer', 'unit.ants.nectarist', 'unit.ants.banneret', 'unit.ants.bombardier',
            'unit.beetles.bolter', 'unit.beetles.arcwright', 'unit.beetles.mender', 'unit.beetles.steamwing',
            'unit.beetles.bombard', 'unit.beetles.juggernaut', 'unit.beetles.warden']
HEROES = ['unit.ants.queen', 'unit.ants.royal-alchemist', 'unit.beetles.forgemaster', 'unit.beetles.coilkeeper']
TOWERS = ['building.ants.tower', 'building.ants.venom-spire', 'building.beetles.hornwatch',
          'building.beetles.arc-tower']
CREEPS = ['needle-wasp', 'thorn-slinger', 'twigcaster', 'seedcaster', 'old-root', 'spitter', 'tree-frog',
          'willow-wisp', 'dew-sprite', 'bloom-witch', 'root-seer', 'tadpole-spirit', 'violet-sporeling',
          'puffcap']


def main():
    source = json.load(open(os.path.join(SOURCE, 'map.json')))
    keep = [e for e in source['entities'] if e['id'] in ('bench.hall',)]
    entities = list(keep)
    # The shooters: a row along z = 70, x from 60, away from the hall.
    for i, d in enumerate(SHOOTERS):
        entities.append({'definition': d, 'id': f'bench.shooter.{i}', 'owner': 'player.1',
                         'position': {'x': 60.0 + i * 7.0, 'y': 70.0}, 'rotation': 0.0})
    for i, d in enumerate(HEROES):
        entities.append({'definition': d, 'id': f'bench.hero.{i}', 'owner': 'player.1',
                         'initialState': {'experience': 5200},
                         'position': {'x': 70.0 + i * 14.0, 'y': 62.0}, 'rotation': 0.0})
    # Towers on the building lattice behind them.
    for i, d in enumerate(TOWERS):
        entities.append({'definition': d, 'id': f'bench.tower.{i}', 'owner': 'player.1',
                         'initialState': {'construction': 'complete'},
                         'position': {'x': 143.5 + i * 12.0, 'y': 67.5}, 'rotation': 0.0})
    # The creatures: a camp in two rows facing them across a 14-cell gap, its home between
    # the lines so it opens fire at once and the shooters answer.
    members = []
    for i, name in enumerate(CREEPS):
        cid = f'bench.camp.{i}'
        members.append(cid)
        entities.append({'definition': 'unit.neutral.' + name, 'id': cid, 'owner': 'none',
                         'position': {'x': 66.0 + (i % 7) * 13.0, 'y': 84.0 + (i // 7) * 6.0}, 'rotation': 180.0})
    doc = {
        'biome': source['biome'],
        'sandbox': True,
        'camps': [{'aggression': 'players', 'aggroRange': 24, 'home': {'x': 105.0, 'y': 78.0}, 'id': 'bench.camp',
                   'leash': 40, 'lootPool': 'loot.camp.easy', 'members': members}],
        'size': source['size'],
        'playerStarts': source['playerStarts'],
        'entities': entities,
        'terrain': source['terrain'],
    }
    if os.path.isdir(OUT):
        shutil.rmtree(OUT)
    shutil.copytree(os.path.join(SOURCE, 'terrain'), os.path.join(OUT, 'terrain'))
    shutil.copy(os.path.join(SOURCE, 'scenery.json'), os.path.join(OUT, 'scenery.json'))
    with open(os.path.join(OUT, 'map.json'), 'w') as f:
        f.write(json.dumps(doc, indent=2, ensure_ascii=False) + '\n')
    package = {'format': 1, 'id': 'ranged-bench', 'kind': 'map', 'name': 'Bench: Ranged attacks',
               'description': 'Every ranged unit and tower facing every ranged forest creature, for reviewing '
                              'projectiles, trails and impacts.',
               'version': '1.0.0', 'dependencies': {'base': '>=0.4.0'}}
    with open(os.path.join(OUT, 'package.json'), 'w') as f:
        f.write(json.dumps(package, indent=2, ensure_ascii=False) + '\n')
    print('wrote', OUT)


if __name__ == '__main__':
    main()
