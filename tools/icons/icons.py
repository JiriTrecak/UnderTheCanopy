"""Icons in the game's one icon family (docs/art-direction.md#icons): gpt-image requests,
installing the chosen images as icon assets, and review sheets.

Runs on Blender's bundled Python, which has numpy and OpenImageIO:

    PY="$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)"
    "$PY" tools/icons/icons.py requests tools/icons/specs/beetles-units.json <out dir>
    ls <out dir>/*.json | xargs -P 4 -n 1 sh tools/buildings/concept.sh
    "$PY" tools/icons/icons.py install <plan.json>
    "$PY" tools/icons/icons.py sheet <out.png> <cell px> <columns> <image>...

**requests** turns a spec into one request file per icon for `tools/buildings/concept.sh`
(gpt-image-2, 1024², outputs in the old repository's `.asset-work/concepts/buildings/` as
`icon-<name>-<n>.png`). A spec is `{"n", "anchors": {type: [refs]}, "items": [{name, kind,
faction, subject, refs, refnote?, n?}]}`; reference paths are inside the old repository. Each
prompt is the family's style, the icon type's rules (`KINDS`), the faction's palette
(`FACTIONS`), then the subject. The subject's approved concept goes first in `refs`; the
type's anchors (approved icons of the same type) are appended so the set stays one family.
The anchors are the chosen images of `specs/style-test.json`, copied to the old repository's
`.asset-work/concepts/buildings/refs/icons/anchor-<type>.png`.

**install** writes each chosen image as an icon asset: `{"src", "dir", "id", "name", "tags",
"race"?, "key"?, "alpha"?, "crop"?, "lift"?, "request"?}` per entry. `key` cuts an image
generated on flat white out to transparency (resources: gpt-image-2 has no transparent output). The image is cut to `crop`
(fractions x0, y0, x1, y1), downscaled to `SIZE` (Lanczos) and lifted by the gamma `lift`
(portraits use 0.85: black chitin on the HUD's dark slots needs it) as `image.png`; `alpha`
keeps a cut-out's transparency (resources). The request it came from is kept as
`generation.json` (a working file, never cooked). An existing folder keeps its id, name and
tags, so replacing an icon in place keeps every reference working; the old art's other files
(a source image) are removed with it.

**sheet** lays images out on the HUD's slot colour to check them at their real size."""
import json, os, sys
import numpy as np
import OpenImageIO as oiio

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SIZE = 256

STYLE = """Use case: one game icon for Under the Canopy, a Warcraft III: Reforged-like RTS of insect armies on the forest floor. The game shows it small (about 56 to 72 pixels) on its HUD command card, so it must read instantly at that size.
THE ICON FAMILY (every icon in the game follows it): a square, full-bleed, hand-painted stylised 3D illustration in the manner of Warcraft III: Reforged unit and ability icons. ONE subject, big in the frame, with chunky, simple, bold forms, crisp clean edges, glossy materials with clear edge highlights, saturated but controlled colour. Strong value contrast: the subject is clearly brighter and more saturated than its background, and a bright rim light separates its silhouette. Light: a warm key light from the upper left and a cool blue-violet rim light from behind on the right. The background is dark, simple and soft, never scenery: a smoky vignette that falls to near-black in the corners. Keep the bottom-right corner calm and dark (the game draws a hotkey letter there). NO text, letters, numbers, runes, UI, frame, border or bevel (the game draws the frame). No noise, grain, sparkle, specks or tiny clutter; few, large, clear shapes."""

KINDS = {
    'unit': """ICON TYPE: UNIT PORTRAIT. A bust, like a Warcraft III unit icon: the unit's head, shoulders and the top of its signature prop, seen from the front-left three-quarter view, slightly from above, eyes in the upper third, cropped at the chest and at the frame's edges. Calm and ready, looking slightly past the viewer. Background: dark warm charcoal-brown smoke with a clear warm amber glow directly behind the head, bright enough (mid-value at its centre) that a black chitin silhouette reads sharply against it, falling to near-black in the corners. The head is large: it takes about a third of the frame.""",
    'machine': """ICON TYPE: UNIT PORTRAIT of a machine. The machine's front, seen from the front-left three-quarter view, slightly from above, close enough that it fills the frame and is cropped at the edges, its most recognisable part (cannon, wings, furnace window) in the centre. Background: dark warm charcoal-brown smoke with a clear warm amber glow directly behind the machine, bright enough (mid-value at its centre) that black and dark-steel forms read sharply against it, falling to near-black in the corners.""",
    'hero': """ICON TYPE: HERO PORTRAIT. Like a unit portrait but closer and grander: the head and its unique crown shape (horn, helm, halo) fill the upper half, a slightly low heroic angle from the front-left, shoulders and the top of the signature weapon below. Background: dark smoke with a clear golden glow behind the head, bright enough (mid-value at its centre) that the black crown shape reads sharply against it, falling to near-black in the corners.""",
    'building': """ICON TYPE: BUILDING PORTRAIT. The whole building seen from the front-left at about 35 degrees from above, as in the reference, centred and filling about 90% of the frame's width, slightly cropped at the bottom. No ground, dirt, base plate or scenery around it: it stands on darkness with a soft dark shadow. Background: dark charcoal with a soft warm amber glow behind the building's top.""",
    'ability': """ICON TYPE: ABILITY (an active spell or skill). It shows the ACTION at its peak moment, not a portrait: a dynamic, diagonal composition with motion and energy, the caster's weapon or body part as the subject when it helps. The ability's energy colour lights the scene and glows behind the subject: abilities are the brightest icons in the game, but the corners still fall to darkness.""",
    'passive': """ICON TYPE: PASSIVE OR AURA ability. A calm, symmetric, centred emblem-like composition (the subject seen from the front) with a radiating glow of the ability's energy colour behind it and darkness in the corners.""",
    'research-grown': """ICON TYPE: RESEARCH (an upgrade of a living colony). A still life of the one improved grown thing (a mandible, a chitin needle, a carapace plate, a fungus, a wing, a tunnel mouth), close-up and slightly angled, floating on a dark earthy-brown background with a soft violet-and-amber glow behind it. Ant research is cultivated, grown and secreted, never machines or metal.""",
    'research': """ICON TYPE: RESEARCH (an upgrade). A still life of the one improved piece of equipment, close-up and slightly angled, floating on a dark forge-steel-grey background with a soft warm glow behind it. Research reads as engineering: a better machine, never a mutation.""",
    'resource': """ICON TYPE: RESOURCE, a CUT-OUT (the game keys it out: it sits on the HUD's resource bar and in cost rows, as small as 24 pixels). The resource itself as one compact object, centred, filling about 85% of the frame, on a PLAIN, FLAT, PURE WHITE background (#ffffff) with nothing else on it: no vignette, no glow or haze outside the object, no cast shadow, no reflection. A thin dark outline runs around the object, and a soft inner glow keeps it readable. This overrides the family's background rule; everything else about the family applies.""",
    'item': """ICON TYPE: ITEM (a treasure a hero carries). The object alone, close-up and slightly angled, filling about 80% of the frame, floating on a dark background with a soft glow of its own main colour behind it. Grown and forest-made materials (amber, bark, chitin, silk, bone, leaves, gold) with clear edge highlights. Magic in it glows blue to violet, never green.""",
    'command': """ICON TYPE: COMMAND (a basic order shared by every race). One clear symbolic object, centred, filling about 75% of the frame, on a dark neutral background with a soft warm glow behind it. Race-neutral materials: steel, bronze, leather, wood. No creature in it unless stated.""",
}

FACTIONS = {
    'beetles': """FACTION: BEETLES, a fortress race of steam and contained magic. Their bodies are plain glossy black chitin, never glowing and never mutated. Materials: dark riveted steel, brass, and royal-blue painted plates (the faction's colour, large clean solid areas). Substance, the forest's magic, glows deep blue to violet ONLY inside glass (tanks, coils, canisters, lanterns, gauges): never green, never yellow, never on a body. Match the reference images' materials and palette exactly.""",
    'ants': """FACTION: ANTS, a superorganism living in harmony with the forest's magic: BIO-MAGIC, alive and beautiful, never corruption. Their bodies are glossy warm sand-and-bronze chitin with ivory mandibles and big black eyes; their faction colour is ROYAL BLUE (silk drapes, tabards, the blue plates on their backs), with small blue-violet blossoms and ivory trim. Everything they use is grown or secreted: clay, amber resin, shed chitin, silk, fungus, pheromone mist. Substance, the forest's magic, is a luminous blue-violet LIQUID light, never green, never crystals. Match the reference concept's materials and palette.""",
    'creatures': """NEUTRAL FOREST CREATURES, the wild camps of the forest floor: drawn with the same chunky Warcraft proportions and glossy finish as the armies, in their own natural materials (bark, moss, mushroom caps, water, fur, chitin, paper nests). Corrupted creatures show rot, fungus and magenta-violet spore light; none of them glows green.""",
    'neutral': """No faction: a race-neutral design.""",
}

DEFAULT_REFNOTE = "the first image is the subject's approved concept: keep its design, materials and colours exactly."
ANCHOR_NOTE = " The other images are approved icons of the same family: match their rendering, lighting, background and framing, but not their subject."


def requests(spec_path, out):
    spec = json.load(open(spec_path))
    os.makedirs(out, exist_ok=True)
    anchors = spec.get('anchors', {})
    for it in spec['items']:
        own = list(it.get('refs', []))
        refs = own + [a for a in anchors.get(it['kind'], []) if a not in own]
        lines = [STYLE, KINDS[it['kind']], FACTIONS[it.get('faction', 'neutral')]]
        if refs:
            note = it.get('refnote') or (DEFAULT_REFNOTE if own else '') + (ANCHOR_NOTE if len(refs) > len(own) else '')
            lines.append('REFERENCES: ' + note.strip())
        lines.append('THE SUBJECT: ' + it['subject'])
        req = {'slug': 'icon-' + it['name'], 'size': '1024x1024', 'n': it.get('n', spec.get('n', 1)),
               'quality': 'high', 'references': refs, 'prompt': '\n'.join(lines)}
        with open(os.path.join(out, it['name'] + '.json'), 'w') as f:
            json.dump(req, f, indent=2)
        print(it['name'], len(refs), 'references')


def key_white(buf):
    """Premultiplied RGBA from an image on flat white: the white connected to the border
    becomes transparent (white highlights inside the object stay), with a soft edge."""
    rgb = buf.get_pixels(oiio.FLOAT)[:, :, :3]
    h, w = rgb.shape[:2]
    whiteness = rgb.min(axis=2)
    near = whiteness > 0.88
    bg = np.zeros((h, w), bool)
    bg[0, :], bg[-1, :], bg[:, 0], bg[:, -1] = near[0, :], near[-1, :], near[:, 0], near[:, -1]
    while True:
        grown = bg.copy()
        grown[1:, :] |= bg[:-1, :]
        grown[:-1, :] |= bg[1:, :]
        grown[:, 1:] |= bg[:, :-1]
        grown[:, :-1] |= bg[:, 1:]
        grown &= near
        if (grown == bg).all():
            break
        bg = grown
    # The edge: background pixels fade by how white they are; one ring of object pixels
    # next to the background loses the white it was blended with.
    alpha = np.where(bg, np.clip((1.0 - whiteness) / 0.12, 0.0, 1.0), 1.0)
    ring = np.zeros_like(bg)
    ring[1:, :] |= bg[:-1, :]
    ring[:-1, :] |= bg[1:, :]
    ring[:, 1:] |= bg[:, :-1]
    ring[:, :-1] |= bg[:, 1:]
    ring &= ~bg
    alpha = np.where(ring, np.minimum(alpha, 0.6 + 0.4 * np.clip((0.95 - whiteness) / 0.3, 0, 1)), alpha)
    # Colour of a partly transparent pixel without the white behind it: c = a*o + (1-a)*1.
    a = alpha[:, :, None]
    obj = np.clip((rgb - (1.0 - a)) / np.maximum(a, 1e-3), 0.0, 1.0)
    out = np.concatenate([obj * a, a], axis=2).astype(np.float32)
    res = oiio.ImageBuf(oiio.ImageSpec(w, h, 4, oiio.FLOAT))
    res.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 4), out)
    return res


def load(path, alpha=False, key=False):
    """RGB, or premultiplied RGBA when `alpha` and the image has an alpha channel, or when
    `key` (an image on flat white to cut out)."""
    buf = oiio.ImageBuf(path)
    if key:
        return key_white(buf)
    n = buf.spec().nchannels
    if alpha and n == 4:
        return oiio.ImageBufAlgo.premult(buf)
    if n > 3:
        return oiio.ImageBufAlgo.channels(buf, (0, 1, 2))
    if n < 3:
        return oiio.ImageBufAlgo.channels(buf, (0, 0, 0))
    return buf


def resized(buf, size):
    n = buf.spec().nchannels
    roi = oiio.ROI(0, size, 0, size, 0, 1, 0, n)
    out = oiio.ImageBufAlgo.resize(buf, filtername='lanczos3', roi=roi)
    # Lanczos rings: keep values in range so premultiplied colour never exceeds alpha.
    out = oiio.ImageBufAlgo.clamp(out, 0.0, 1.0)
    return oiio.ImageBufAlgo.unpremult(out) if n == 4 else out


def install(plan_path):
    for it in json.load(open(plan_path)):
        d = os.path.join(ROOT, it['dir'])
        os.makedirs(d, exist_ok=True)
        img = load(it['src'], alpha=it.get('alpha', False), key=it.get('key', False))
        if it.get('crop'):
            w, h = img.spec().width, img.spec().height
            x0, y0, x1, y1 = it['crop']
            img = oiio.ImageBufAlgo.cut(img, oiio.ROI(int(x0 * w), int(x1 * w), int(y0 * h), int(y1 * h)))
        img = resized(img, SIZE)
        if it.get('lift', 1.0) != 1.0:
            img = oiio.ImageBufAlgo.pow(img, (it['lift'],) * 3 + ((1.0,) if img.spec().nchannels == 4 else ()))
        img.set_write_format(oiio.UINT8)
        img.write(os.path.join(d, 'image.png'))
        path = os.path.join(d, 'asset.json')
        old = json.load(open(path)) if os.path.exists(path) else {}
        resources = [{'role': 'image', 'index': 1, 'format': 'png'}]
        # The old art's other files (a source image, another format) go with it.
        for f in os.listdir(d):
            if f not in ('asset.json', 'image.png', 'generation.json'):
                os.remove(os.path.join(d, f))
        stale = os.path.join(d, 'generation.json')
        if it.get('request'):
            record = dict(it['request'])
            record['install'] = {k: it[k] for k in ('crop', 'lift', 'alpha', 'key') if k in it}
            with open(stale, 'w') as f:
                json.dump(record, f, indent=2)
                f.write('\n')
            resources.append({'role': 'generation', 'index': 1, 'format': 'json'})
        elif os.path.exists(stale):
            os.remove(stale)
        asset = {'id': old.get('id', it['id']), 'name': old.get('name', it['name']), 'kind': 'icon',
                 'tags': old.get('tags', it.get('tags', []))}
        race = old.get('race', it.get('race'))
        if race:
            asset['race'] = race
        asset['resources'] = resources
        asset['license'] = 'Original artwork for Under the Canopy: gpt-image-2 from the game\'s own concept art (docs/art-direction.md#icons).'
        with open(path, 'w') as f:
            json.dump(asset, f, indent=2)
            f.write('\n')
        print(asset['id'], '->', it['dir'])


def sheet(out, cell, cols, paths):
    gap = 6
    rows = (len(paths) + cols - 1) // cols
    w, h = cols * (cell + gap) + gap, rows * (cell + gap) + gap
    canvas = np.empty((h, w, 3), np.float32)
    canvas[:] = np.array([8, 13, 9], np.float32) / 255.0
    for i, p in enumerate(paths):
        img = resized(load(p, alpha=True), cell).get_pixels(oiio.FLOAT)
        x, y = gap + (i % cols) * (cell + gap), gap + (i // cols) * (cell + gap)
        a = img[:, :, 3:4] if img.shape[2] == 4 else 1.0
        canvas[y:y + cell, x:x + cell] = img[:, :, :3] * a + canvas[y:y + cell, x:x + cell] * (1 - a)
    buf = oiio.ImageBuf(oiio.ImageSpec(w, h, 3, oiio.UINT8))
    buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 3), canvas)
    buf.write(out)
    print(out, w, 'x', h)


if __name__ == '__main__':
    cmd, args = sys.argv[1], sys.argv[2:]
    if cmd == 'requests':
        requests(args[0], args[1])
    elif cmd == 'install':
        install(args[0])
    elif cmd == 'sheet':
        sheet(args[0], int(args[1]), int(args[2]), args[3:])
    else:
        sys.exit(__doc__)
