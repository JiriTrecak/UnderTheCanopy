"""The data of the icon review page (tools/icons/review.html): every icon the base game uses,
what uses it and what that does, as `manifest.json` plus `icons/<id>.png` in an output folder,
ready to publish next to the page.

    python3 tools/icons/review.py <out dir>

Each entry: `id`, `name`, `category`, `race`, `uses` (the definitions that show it, with
their descriptions), `image`, `version` (a hash of the image: a regenerated icon asks for a
new vote) and `style` (`new` once the icon is composed by `icons.py pick`, else `old`)."""
import hashlib, json, os, shutil, sys

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
BASE = os.path.join(ROOT, 'base')


def icon_assets():
    out = {}
    for top, _, files in os.walk(os.path.join(BASE, 'assets')):
        if 'asset.json' in files:
            a = json.load(open(os.path.join(top, 'asset.json')))
            if a.get('kind') == 'icon':
                out[a['id']] = (top, a)
    return out


def walk(node, path, found, ids):
    """Records each definition (the nearest object with a name) that names an icon id."""
    if isinstance(node, dict):
        for k, v in node.items():
            walk(v, path + [node], found, ids)
    elif isinstance(node, list):
        for v in node:
            walk(v, path, found, ids)
    elif isinstance(node, str) and node in ids:
        owner = next((p for p in reversed(path) if isinstance(p, dict) and 'name' in p), None)
        if owner is not None:
            desc = owner.get('description') or (owner.get('presentation') or {}).get('description') or ''
            found.setdefault(node, []).append({'name': owner['name'], 'does': desc,
                                               'passive': owner.get('activation') == 'passive'})


def category(rel, uses):
    if rel.startswith('icons/command-'):
        return 'Command'
    if rel.startswith(('icons/abilities/', 'icons/spell-', 'spells/', 'custom/')):
        return 'Passive' if uses and all(u['passive'] for u in uses) else 'Ability'
    if rel.startswith('icons/research/'):
        return 'Research'
    if rel.startswith('portraits/units/'):
        return 'Portrait'
    if rel.startswith('portraits/buildings/'):
        return 'Building'
    if rel.startswith('icons/item-'):
        return 'Item'
    if rel.startswith('icons/resource-') or rel.startswith('portraits/resources/'):
        return 'Resource'
    if rel.startswith('icons/stat-'):
        return 'Stat'
    return 'Other'


def race(rel, asset):
    if asset.get('race'):
        return asset['race'].capitalize()
    for r in ('ants', 'beetles', 'neutral'):
        if f'/{r}' in '/' + rel or f'-{r}-' in rel or f'/{r}-' in rel:
            return r.capitalize()
    return 'Shared'


def main(out):
    assets = icon_assets()
    found = {}
    for top, _, files in os.walk(BASE):
        if os.path.join(BASE, 'assets') in top:
            continue
        for f in files:
            if f.endswith('.json'):
                try:
                    walk(json.load(open(os.path.join(top, f))), [], found, assets)
                except (ValueError, UnicodeDecodeError):
                    pass
    config = os.path.join(ROOT, 'config')
    for top, _, files in os.walk(config):
        for f in files:
            if f.endswith('.json'):
                text = open(os.path.join(top, f)).read()
                for i in assets:
                    if i in text:
                        found.setdefault(i, []).append({'name': 'Interface', 'does': '', 'passive': False})
    os.makedirs(os.path.join(out, 'icons'), exist_ok=True)
    entries = []
    for i, (top, asset) in sorted(assets.items()):
        if i not in found:
            continue
        rel = os.path.relpath(top, os.path.join(BASE, 'assets'))
        uses, seen = [], set()
        for u in found[i]:
            if u['name'] not in seen:
                seen.add(u['name'])
                uses.append({'name': u['name'], 'does': u['does']})
        img = os.path.join(top, 'image.png')
        data = open(img, 'rb').read()
        shutil.copyfile(img, os.path.join(out, 'icons', i + '.png'))
        record = os.path.join(top, 'generation.json')
        new = os.path.exists(record) and 'background' in json.load(open(record)).get('install', {})
        entries.append({'id': i, 'name': asset.get('name', i), 'category': category(rel, found[i]),
                        'race': race(rel, asset), 'uses': uses, 'image': f'icons/{i}.png',
                        'version': hashlib.sha1(data).hexdigest()[:12], 'style': 'new' if new else 'old',
                        'folder': os.path.relpath(top, ROOT)})
    with open(os.path.join(out, 'manifest.json'), 'w') as f:
        json.dump({'icons': entries}, f, indent=1)
    print(len(entries), 'icons ->', out)


if __name__ == '__main__':
    if len(sys.argv) != 2:
        sys.exit(__doc__)
    main(sys.argv[1])
