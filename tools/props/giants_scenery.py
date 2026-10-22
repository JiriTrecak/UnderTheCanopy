"""Writes the placeable scenery for the forest-floor giants (docs/art-direction.md, What big
means): one `base/scenery/giants/<name>.json` per model in `base/assets/environment/giants-*`.

    python3 tools/props/giants_scenery.py

Blockers are measured from each model's geometry, in its own metres:
- `body`: an ellipse over the whole footprint, shrunk by a share (nuts, stones, ruins);
- `foot`: an ellipse over what stands in the lowest `FOOT` of its height (a mushroom's stem,
  a fern's crown, a plant's stalks), so units pass under caps and fronds;
- none for pieces units step over (needles)."""
import json, os, struct

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ASSETS = os.path.join(ROOT, 'base', 'assets', 'environment')
OUT = os.path.join(ROOT, 'base', 'scenery', 'giants')
FOOT = 0.12

# name: (display name, category, blocker, share)
PIECES = {
    'giants-acorn': ('Acorn', 'landmark', 'body', 0.85),
    'giants-acorn-cup': ('Acorn cup', 'landmark', 'body', 0.9),
    'giants-beechnut': ('Beech nut husk', 'landmark', 'body', 0.75),
    'giants-chestnut-burr': ('Chestnut burr', 'landmark', 'body', 0.8),
    'giants-hazelnut': ('Hazelnut', 'landmark', 'body', 0.8),
    'giants-pine-cone': ('Pine cone', 'landmark', 'body', 0.8),
    'giants-hornet-nest': ("Hornets' nest", 'landmark', 'body', 0.85),
    'giants-maple-seed': ('Maple seed', 'landmark', 'body', 0.6),
    'giants-bilberries': ('Bilberry sprig', 'foliage', 'foot', 1.0),
    'giants-parasol': ('Parasol mushroom', 'foliage', 'foot', 1.0),
    'giants-porcini': ('King bolete', 'foliage', 'foot', 1.0),
    'giants-mycena': ('Glowing bonnets', 'foliage', 'foot', 1.0),
    'giants-shelf-fungus': ('Shelf fungus', 'foliage', None, 0),
    'giants-inkcap': ('Ink caps', 'foliage', 'foot', 1.0),
    'giants-morel': ('Morel', 'foliage', 'foot', 1.0),
    'giants-buttons': ('Button mushrooms', 'foliage', 'foot', 1.0),
    'giants-puffballs': ('Puffballs', 'foliage', 'body', 0.85),
    'giants-withered': ('Withered mushrooms', 'foliage', 'foot', 1.0),
    'giants-fern-crown': ('Fern crown', 'foliage', 'body', 0.4),
    'giants-fern-frond': ('Fern frond', 'foliage', 'foot', 1.0),
    'giants-fiddleheads': ('Fiddleheads', 'foliage', 'foot', 1.0),
    'giants-harts-tongue': ("Hart's-tongue fern", 'foliage', 'body', 0.45),
    'giants-bracken': ('Fallen bracken', 'foliage', None, 0),
    'giants-beech-trunk': ('Beech trunk', 'landmark', 'foot', 0.9),
    'giants-branch': ('Fallen branch', 'landmark', 'body', 0.75),
    'giants-twig': ('Twig', 'landmark', 'body', 0.75),
    'giants-bark-slab': ('Bark slab', 'landmark', 'body', 0.85),
    'giants-root': ('Giant root', 'landmark', 'body', 0.7),
    'giants-leaf-oak': ('Fallen oak leaf', 'landmark', 'body', 0.75),
    'giants-leaf-beech': ('Fallen beech leaf', 'landmark', 'body', 0.75),
    'giants-leaf-maple': ('Fallen maple leaf', 'landmark', 'body', 0.75),
    'giants-feather': ('Feather', 'landmark', 'body', 0.7),
    'giants-moss': ('Moss cushion', 'foliage', 'body', 0.8),
    'giants-sorrel': ('Wood sorrel', 'foliage', 'foot', 1.0),
    'giants-clover': ('Clover', 'foliage', 'foot', 1.0),
    'giants-grass': ('Grass clump', 'foliage', 'foot', 1.0),
    'giants-pebbles': ('Pebbles', 'landmark', 'body', 0.85),
    'giants-mossy-stone': ('Mossy stone', 'landmark', 'body', 0.85),
    'giants-snail-shell': ('Snail shell', 'landmark', 'body', 0.85),
    'giants-beetle-husk': ('Beetle husk', 'landmark', 'body', 0.85),
    'giants-pine-needles-a': ('Pine needles', 'landmark', None, 0),
    'giants-pine-needles-b': ('Crossed pine needles', 'landmark', None, 0),
    'giants-dew-drop': ('Dew drop', 'landmark', 'body', 0.9),
    'giants-dew-drops': ('Dew drops', 'landmark', None, 0),
}


def positions(path):
    """Every vertex of a GLB, in glTF axes (Y up), with node translations and scales."""
    data = open(path, 'rb').read()
    n = struct.unpack('<I', data[12:16])[0]
    gltf = json.loads(data[20:20 + n])
    bin_at = 20 + n + 8
    out = []
    for node in gltf.get('nodes', []):
        if 'mesh' not in node:
            continue
        t = node.get('translation', [0, 0, 0])
        s = node.get('scale', [1, 1, 1])
        for prim in gltf['meshes'][node['mesh']]['primitives']:
            acc = gltf['accessors'][prim['attributes']['POSITION']]
            view = gltf['bufferViews'][acc['bufferView']]
            start = bin_at + view.get('byteOffset', 0) + acc.get('byteOffset', 0)
            stride = view.get('byteStride', 12)
            for i in range(acc['count']):
                x, y, z = struct.unpack_from('<fff', data, start + i * stride)
                out.append((x * s[0] + t[0], y * s[1] + t[1], z * s[2] + t[2]))
    return out


def ellipse(points, share):
    xs = [p[0] for p in points]
    zs = [p[2] for p in points]
    w, d = (max(xs) - min(xs)) * share, (max(zs) - min(zs)) * share
    return {'width': round(max(w, 0.6), 2), 'depth': round(max(d, 0.6), 2), 'shape': 'ellipse',
            'x': round((max(xs) + min(xs)) / 2, 2), 'z': round((max(zs) + min(zs)) / 2, 2)}


def main():
    os.makedirs(OUT, exist_ok=True)
    for name, (label, category, mode, share) in PIECES.items():
        glb = os.path.join(ASSETS, name, 'geometry.glb')
        if not os.path.exists(glb):
            print('missing', name)
            continue
        piece = {'id': 'scenery.' + name, 'name': label, 'category': category, 'type': 'prop',
                 'model': 'asset.models.environment.' + name}
        if mode:
            pts = positions(glb)
            if mode == 'foot':
                top = max(p[1] for p in pts)
                pts = [p for p in pts if p[1] <= top * FOOT]
            piece['blockers'] = [ellipse(pts, share)]
        with open(os.path.join(OUT, name.removeprefix('giants-') + '.json'), 'w') as f:
            json.dump(piece, f, indent=2, ensure_ascii=False)
            f.write('\n')
        print('wrote', name, piece.get('blockers', ''))


main()
