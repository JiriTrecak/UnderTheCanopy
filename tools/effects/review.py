"""The data of the spell review page (tools/effects/review.html): every ability of the base game
with its icon, who casts it, what it does and a looping clip of each effect it plays, as
`manifest.json` plus `media/` in an output folder, ready to publish next to the page.

    python3 tools/effects/review.py <out dir> [--only <text>] [--size 480x300]

Needs the release game and the cooked pack (`cargo build --release -p canopy`, then the cook)
and ffmpeg. Each effect is played on its own on the ranged bench's open sand at noon, away from its buildings
(`canopy shot --effect <id>,0 --sequence`), framed by its size, travelling layers from 6 m
west, and encoded as an animated WebP at game speed. Each entry carries a `version` (a hash of
its ability and effect files): a rebuilt spell asks for a new vote. `--only` renders the
abilities whose id contains the text (the manifest keeps the others' last clips).
"""
import hashlib
import json
import math
import os
import shutil
import subprocess
import sys
import tempfile

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE = os.path.join(ROOT, 'base')
GAME = os.path.join(ROOT, 'target', 'release', 'canopy')
STAGE = ('ranged-bench', 128.0, 182.0)
# The order a spell's moments happen in.
EVENTS = ['accepted', 'released', 'projectile', 'impact', 'summoned', 'statusApplied', 'healed', 'damaged',
          'enhancedHit', 'interval', 'finished']


def load_dir(sub):
    out = {}
    for top, _, files in os.walk(os.path.join(BASE, sub)):
        for f in files:
            if f.endswith('.json'):
                path = os.path.join(top, f)
                doc = json.load(open(path))
                if 'id' in doc:
                    out[doc['id']] = (path, doc)
    return out


def owners(units):
    """Ability id → names of the units and heroes that bind it."""
    found = {}
    for _, (_, doc) in units.items():
        for b in ((doc.get('behaviors') or {}).get('abilities') or {}).get('bindings', []):
            found.setdefault(b.get('ability'), []).append(doc.get('name', doc['id']))
    return found


def icons():
    out = {}
    for top, _, files in os.walk(os.path.join(BASE, 'assets')):
        if 'asset.json' in files and 'image.png' in files:
            a = json.load(open(os.path.join(top, 'asset.json')))
            out[a['id']] = os.path.join(top, 'image.png')
    return out


def race_of(path):
    rel = os.path.relpath(path, os.path.join(BASE, 'abilities'))
    if rel.startswith('ants') or '/queen/' in rel or 'alchemist' in rel or '/general/' in rel:
        return 'Ants'
    if rel.startswith('neutral'):
        return 'Neutral'
    return 'Beetles'


def framing(effect):
    """Zoom and length (ticks) to show an effect whole."""
    reach = 3.0
    for l in effect.get('layers', []):
        size = float(l.get('size', 1))
        if l.get('shape') in ('ring', 'glow', 'billboard', 'rain', 'pillar'):
            reach = max(reach, size * 0.6 + float(l.get('spread', 0) or 0))
        e = l.get('emitter')
        if e:
            # How far a particle gets: its speed over the shorter of its life and its drag time.
            life = float(e.get('lifetimeTicks', 20)) / 40.0
            drag = float(e.get('drag', 0.0))
            span = min(life, 1.0 / drag) if drag > 0 else life
            travel = float(e.get('speed', 0)) * (1.0 + float(e.get('speedVariation', 0))) * span
            reach = max(reach, min(16.0, travel + float(e.get('spawnRadius', 0))))
    # The camera stops at 0.515 (the players' closest): clips render larger and keep the
    # middle, twice for spells and three times for unit-sized effects.
    crop = 3 if reach < 3.5 else 2
    zoom = max(0.52, min(1.0, reach / 12.0))
    ticks = int(effect.get('durationTicks', 40))
    if not effect.get('loop'):
        tail = max([int(l['emitter']['lifetimeTicks']) for l in effect.get('layers', []) if l.get('emitter')] or [0])
        ticks += min(tail, 80)
    return zoom, crop, max(8, min(ticks, 200))


def render(effect_id, effect, size, scratch, out_file):
    zoom, crop, ticks = framing(effect)
    step = max(1, round(ticks / 24))
    frames = ticks // step + 1
    stem = os.path.join(scratch, 'f')
    for f in os.listdir(scratch):
        os.remove(os.path.join(scratch, f))
    w, h = (int(v) for v in size.split('x'))
    cmd = [GAME, 'shot', '--data', ROOT, '--map', STAGE[0], '--camera', f'{STAGE[1]},{STAGE[2]},{zoom:.2f}',
           '--hour', '12', '--size', f'{w * crop}x{h * crop}', '--effect', f'{effect_id},0',
           '--sequence', f'{frames},{step}', '--out', stem + '.png']
    r = subprocess.run(cmd, capture_output=True, text=True)
    if r.returncode != 0 or not os.path.exists(stem + '.png'):
        print('  failed', effect_id, r.stderr.strip()[-300:])
        return False
    os.rename(stem + '.png', stem + '-000.png')
    fps = 40.0 / step
    r = subprocess.run(['ffmpeg', '-y', '-loglevel', 'error', '-framerate', f'{fps:.3f}', '-i', stem + '-%03d.png',
                        '-vf', f'crop={w}:{h}', '-loop', '0', '-c:v', 'libwebp_anim', '-quality', '72', '-preset', 'picture', out_file],
                       capture_output=True, text=True)
    if r.returncode != 0:
        print('  ffmpeg failed', effect_id, r.stderr.strip()[-300:])
        return False
    return True


def digest(paths):
    h = hashlib.sha1()
    for p in paths:
        h.update(open(p, 'rb').read())
    return h.hexdigest()[:12]


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    out = args[0]
    only = args[args.index('--only') + 1] if '--only' in args else None
    size = args[args.index('--size') + 1] if '--size' in args else '480x300'
    if not os.path.exists(GAME):
        sys.exit('build the game first: cargo build --release -p canopy')
    abilities = load_dir('abilities')
    effects = load_dir('effects')
    by_ability = owners(load_dir('units'))
    icon_files = icons()
    os.makedirs(os.path.join(out, 'media'), exist_ok=True)
    os.makedirs(os.path.join(out, 'icons'), exist_ok=True)
    manifest_path = os.path.join(out, 'manifest.json')
    previous = {}
    if os.path.exists(manifest_path):
        previous = {s['id']: s for s in json.load(open(manifest_path)).get('spells', [])}
    entries = []
    scratch = tempfile.mkdtemp(prefix='spell-review-')
    rendered = {}
    try:
        for aid, (apath, ability) in sorted(abilities.items()):
            bindings = (ability.get('presentation') or {}).get('effects', [])
            if not bindings:
                continue
            bindings = sorted(bindings, key=lambda b: EVENTS.index(b['event']) if b['event'] in EVENTS else 99)
            files = [apath] + [effects[b['effect']][0] for b in bindings if b['effect'] in effects]
            entry = {'id': aid, 'name': ability.get('name', aid), 'race': race_of(apath),
                     'casters': by_ability.get(aid, []), 'does': ability.get('description', ''),
                     'passive': ability.get('activation') == 'passive', 'version': digest(files), 'effects': []}
            icon = (ability.get('presentation') or {}).get('icon')
            if icon in icon_files:
                shutil.copyfile(icon_files[icon], os.path.join(out, 'icons', aid + '.png'))
                entry['icon'] = f'icons/{aid}.png'
            for b in bindings:
                eid = b['effect']
                if eid not in effects:
                    continue
                path, effect = effects[eid]
                name = eid.replace('effect.', '') + '.webp'
                media = os.path.join(out, 'media', name)
                fresh = only is None or only in aid
                if fresh and eid not in rendered:
                    print(aid, '·', eid)
                    rendered[eid] = render(eid, effect, size, scratch, media)
                entry['effects'].append({'id': eid, 'name': effect.get('name', eid), 'event': b['event'],
                                         'anchor': b.get('anchor'), 'about': effect.get('description', ''),
                                         'media': f'media/{name}', 'version': digest([path])})
            if only is not None and only not in aid and aid in previous:
                entry = previous[aid] | {'casters': entry['casters'], 'does': entry['does']}
            entries.append(entry)
    finally:
        shutil.rmtree(scratch, ignore_errors=True)
    with open(manifest_path, 'w') as f:
        json.dump({'spells': entries}, f, indent=1)
    print(len(entries), 'spells ->', out)


if __name__ == '__main__':
    main()
