"""Sets each beetle unit model's gait nominalSpeed from its unit's movement speed: the clip
then plays at rate 1 when the unit walks at its normal pace, drawn at its display scale
times the definition's modelScale."""
import json, glob, os
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))) + '/base/'
defs = {}
for f in glob.glob(ROOT + 'units/beetles/**/*.json', recursive=True):
    d = json.load(open(f))
    defs.setdefault(d['model'], d)
for model, d in sorted(defs.items()):
    if not model.startswith('asset.models.units.beetles-'):
        continue
    path = ROOT + 'assets/units/' + model.split('.')[-1] + '/asset.json'
    if not os.path.exists(path):
        continue
    asset = json.load(open(path))
    move = d['behaviors'].get('movement', {})
    speed = move.get('speed')
    if not speed:
        continue
    drawn = asset.get('display', {}).get('scale', 1) * d.get('modelScale', 1)
    changed = False
    for a in asset['capabilities'].get('animations', []):
        if a['semantic'] in ('walk', 'run', 'carry_walk', 'carry_run'):
            pace = move.get('walkSpeed', speed) if a['semantic'] in ('walk', 'carry_walk') and 'walkSpeed' in move else speed
            value = round(pace / drawn, 3)
            if a.get('nominalSpeed') != value:
                a['nominalSpeed'] = value
                changed = True
    if changed:
        open(path, 'w').write(json.dumps(asset, indent=2, ensure_ascii=False) + '\n')
        print(model, drawn)
