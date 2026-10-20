"""The game's icons (docs/art-direction.md#icons): gpt-image requests, installing the chosen
images as icon assets over a background from the library, and review sheets.

Runs on Blender's bundled Python, which has numpy and OpenImageIO:

    PY="$(ls /Applications/Blender.app/Contents/Resources/*/python/bin/python3* | head -1)"
    "$PY" tools/icons/icons.py requests tools/icons/specs/commands.json <out dir> [name ...]
    ls <out dir>/*.json | xargs -P 4 -n 1 sh tools/buildings/concept.sh
    "$PY" tools/icons/icons.py candidates tools/icons/specs/commands.json <sheet.png> [name ...]
    "$PY" tools/icons/icons.py pick tools/icons/specs/commands.json command-move=1 ...
    "$PY" tools/icons/icons.py recompose [folder prefix]
    "$PY" tools/icons/icons.py sheet <out.png> <cell px> <columns> <image>...

**requests** turns a spec into one request file per icon for `tools/buildings/concept.sh`
(1024², outputs in the old repository's `.asset-work/concepts/buildings/` as
`icon-<name>-<n>.png`) for the items not picked yet, or for the items named after the output
folder (to repaint one). A spec is
`{"n", "items": [{name, kind, faction?, dir, subject, refs?, refnote?, n?, background?,
after?, level?}]}`, reference paths inside the old repository, `dir` the icon's asset folder.
Each prompt is the shared rules (`STYLE`), the icon type's rules (`KINDS`), the faction's
palette (`FACTIONS`), then the subject; the subject's concept goes first in `refs`.

Icons are painted with a real transparent background (`MODEL`), their glows semi-transparent,
and the background is the game's own: **pick** keeps the chosen image as the asset's
`source.png` (a working file) and composes `image.png` over the type's background from
`BACKGROUNDS` (or the item's `background`), with the type's marks (`finish`). Changing a background, say for every command, is one edit of
`BACKGROUNDS` and a **recompose**, which rebuilds every `image.png` from its foreground and the
choices kept in its `generation.json`.

**Upgrade lines** read as one item getting better: an item with `after` (the previous
level's name) and `level` is painted from the previous level's chosen image, in the same
pose, size and framing, with only its materials improved, so request it once the previous
level is picked. `level` also draws that many gold diamonds.

**install** is the older path for cut-outs (resources): `[{"src", "dir", "id", "name",
"tags", "race"?, "key"?, "alpha"?, "crop"?, "request"?}]`; `key` cuts an image generated on
flat white out to transparency.

**sheet** lays images out on the HUD's slot colour to check them at their real size."""
import json, os, subprocess, sys
import numpy as np
import OpenImageIO as oiio

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SIZE = 256


def old_repository():
    """The old repository sits beside the main checkout (also from a worktree)."""
    common = subprocess.check_output(['git', '-C', ROOT, 'rev-parse', '--path-format=absolute',
                                      '--git-common-dir'], text=True).strip()
    return os.path.join(os.path.dirname(common), '..', 'Settlers 3 Web')


OUTPUTS = '.asset-work/concepts/buildings'
# The quality variant of GPT Image 2.5: real alpha, so glows stay part of the icon.
MODEL = 'gpt-image-2.5-sunburst'

STYLE = """Use case: one game icon for Under the Canopy, an RTS of insect armies on the forest floor, in the icon tradition of Warcraft III and StarCraft II. The game shows it at 46 pixels on its command card and as small as 22 pixels in buff badges: design it to read instantly at 32 pixels. Readability wins over every other wish.
THE RULES (every icon in the game follows them):
1. ONE subject showing ONE idea: exactly the object described below and nothing else. No second subject, no crowd of small figures, no scenery, no ground, no props around it.
2. A big, bold silhouette: the subject fills about 70% of the frame, centred a little up and left, and would be recognisable as a solid black shape alone. Chunky simple forms, crisp clean edges, no thin lines, no filigree, no small parts, no fine texture.
3. Light and value: one warm key light from the upper left. Paint the subject in three or four big value steps; its lit side is the brightest area of the whole icon. A thin bright rim light on the right edge separates it from the background.
4. Background: FULLY TRANSPARENT (real alpha) everywhere outside the subject and its own glow: no backdrop colour, gradient, texture, smoke, clouds, rays, sparkles, particles, bokeh, ground, cast shadow or vignette. A glow, where the icon has one, is semi-transparent and fades out completely well inside the frame. (The game puts its own background behind the icon.)
5. Limited palette: the subject's two or three main colours.
6. Effects (energy, lightning, fire, liquid, smoke, motion lines) ONLY where the subject below says the effect is part of it. Then the effect is one bold, simple shape (a single fork, a single splash, a single flame), never many thin strands or a glowing haze.
7. Rendering: hand-painted, stylised 3D like Warcraft III: Reforged icons, glossy materials with clear edge highlights, saturated but controlled colour.
8. Keep the bottom-right corner dark and empty (the game draws a hotkey letter there). NO text, letters, numbers, runes, symbols, UI, frame, border or bevel (the game adds the frame)."""

KINDS = {
    'unit': """ICON TYPE: UNIT PORTRAIT. The unit's head and the top of its shoulders, like a Warcraft III unit icon: the head is LARGE, about half the frame's height, seen from the front-left three-quarter view, eyes in the upper third, cropped at the chest. Its one signature feature (horn, crest, mandibles, hood) is clearly visible; any weapon shows only as its tip at the edge, if at all. Behind the head, one soft warm amber glow (part of the icon, semi-transparent, fading out well inside the frame), bright enough that a dark chitin head reads sharply against it.""",
    'machine': """ICON TYPE: UNIT PORTRAIT of a machine. The machine's front, seen from the front-left three-quarter view, close enough that it fills the frame, its one most recognisable part (cannon, wings, furnace window) large in the centre. Behind it, one soft warm amber glow (part of the icon, semi-transparent, fading out well inside the frame), bright enough that dark steel reads sharply against it.""",
    'hero': """ICON TYPE: HERO PORTRAIT. Like a unit portrait but closer and grander: the head and its unique crown shape (horn, helm, halo) fill the upper two thirds, a slightly low heroic angle from the front-left. Behind the head, one soft golden glow (part of the icon, semi-transparent, fading out well inside the frame), bright enough that the dark crown shape reads sharply against it.""",
    'building': """ICON TYPE: BUILDING PORTRAIT. The whole building seen from the front-left at about 35 degrees from above, as in the reference, centred and filling about 85% of the frame's width. No ground, base plate or scenery: it stands on nothing. Simplify it: keep its outline and its two or three biggest features, drop small ornament. A soft warm amber glow behind the building's top, fading out.""",
    'ability': """ICON TYPE: ACTIVE ABILITY. Show the ONE object, body part or effect that does the action, in close-up, the way Warcraft III does (Storm Bolt is the thrown hammer, Thunder Clap the hoof striking the ground, Holy Light a burst of light): never the caster's whole body, never a battle scene. A diagonal pose gives it motion. A soft glow of the ability's energy colour may sit close around the subject, fading out.""",
    'passive': """ICON TYPE: PASSIVE OR AURA. A calm, still, symmetric emblem seen from the front, centred: the one object that stands for the lasting effect. No motion, no action. A faint, quiet glow of its energy colour close behind it at most (the game adds the passive frame).""",
    'research-grown': """ICON TYPE: RESEARCH (an upgrade of a living colony). A still life of the one improved grown thing (a mandible, a chitin needle, a carapace plate, a wing, a sac, a tunnel mouth) alone, close-up, angled diagonally, floating. Ant research is grown and secreted, never machines or metal. No glow unless the subject itself glows (the game adds the level marks).""",
    'research': """ICON TYPE: RESEARCH (an upgrade). A still life of the one improved piece of equipment alone, close-up, angled diagonally, floating. Research reads as engineering: a better machine part, never a mutation. No glow unless the subject itself glows (the game adds the level marks).""",
    'resource': """ICON TYPE: RESOURCE, a CUT-OUT: it sits on the HUD's resource ribbon and in cost rows, as small as 24 pixels, so it keeps its transparency and gets no background. The resource itself as one compact object, centred, filling about 85% of the frame, with a thin dark outline all around it and no glow, haze or shadow outside the outline.""",
    'item': """ICON TYPE: ITEM (a treasure a hero carries). The object alone, close-up and angled diagonally, filling about 75% of the frame. Grown and forest-made materials (amber, bark, chitin, silk, bone, leaves, gold). Magic in it glows blue to violet, never green. A faint glow of its own main colour close behind it, fading out.""",
    'command': """ICON TYPE: COMMAND (a basic order shared by every race). One symbolic object, simple and bold, like a StarCraft II command icon: clean polished steel with a little bronze, almost monochrome, centred, filling about 70% of the frame. No creature, no hand, no scene, no glow.""",
}

FACTIONS = {
    'beetles': """FACTION: BEETLES, a fortress race of steam and contained magic. Their bodies are glossy black chitin, never glowing. Materials: dark riveted steel, polished brass, and royal-blue painted plates (big clean solid areas). Their fire is orange furnace fire, their steam white. Substance, the forest's magic, glows violet ONLY inside glass (tanks, coils, lanterns): never green, never on a body. Match the reference image's design and materials.""",
    'ants': """FACTION: ANTS, a superorganism living with the forest's magic: bio-magic, alive and beautiful, never corruption. Their bodies are glossy warm sand-and-bronze chitin with ivory mandibles and big black eyes; royal-blue silk and plates, gold trim for royalty, warm amber resin. Everything they use is grown or secreted (clay, amber, chitin, silk, fungus). Substance is a luminous violet LIQUID light, never green, never crystals. Match the reference image's design and materials.""",
    'creatures': """NEUTRAL FOREST CREATURES, the wild camps of the forest floor: chunky Warcraft proportions and a glossy finish, in their own natural materials (bark, moss, mushroom caps, water, fur, chitin, paper nests). Corrupted creatures show rot and magenta-violet spore light; none of them glows green.""",
    'neutral': """No faction: a race-neutral design.""",
}

DEFAULT_REFNOTE = "the first image is the subject's approved concept: keep its design, materials and colours, but simplify it to the icon's few big shapes and ignore its pose, background and staging."
CHAIN_NOTE = "the first image is level {prev} of this same upgrade line, already approved. Paint level {level}: the SAME object in exactly the same pose, angle, size, position in the frame and lighting, so the two icons line up when swapped; only its materials and finish improve as described below. Keep the transparent background."

# The library of backgrounds: radial gradients, nearly invisible on purpose (the subject
# carries the icon). (centre colour, corner colour) in sRGB 0–255.
BACKGROUNDS = {
    'command': ((20, 25, 32), (7, 9, 11)),
    'ability': ((19, 16, 25), (6, 5, 8)),
    'passive': ((15, 15, 17), (5, 5, 6)),
    'research': ((19, 20, 22), (6, 6, 7)),
    'research-grown': ((22, 17, 12), (7, 5, 4)),
    'portrait': ((21, 16, 12), (6, 5, 4)),
    'building': ((19, 15, 11), (6, 5, 4)),
    'item': ((18, 15, 18), (6, 5, 6)),
}
KIND_BACKGROUND = {'unit': 'portrait', 'machine': 'portrait', 'hero': 'portrait', 'resource': 'none'}
KIND_FRAME = {'passive': 'passive'}


def requests(spec_path, out, only=()):
    spec = json.load(open(spec_path))
    os.makedirs(out, exist_ok=True)
    items = {it['name']: it for it in spec['items']}
    for it in spec['items']:
        if only and it['name'] not in only:
            continue
        if not only and picked(it):
            continue
        refs = list(it.get('refs', []))
        note = it.get('refnote') or (DEFAULT_REFNOTE if refs else '')
        if it.get('after'):
            prev = items[it['after']]
            record = os.path.join(ROOT, prev['dir'], 'generation.json')
            src = json.load(open(record)).get('install', {}).get('src') if os.path.exists(record) else None
            if not src:
                print(it['name'], 'skipped: pick', prev['name'], 'first')
                continue
            refs = [src] + refs
            note = CHAIN_NOTE.format(prev=prev.get('level', 1), level=it.get('level', 2))
        lines = [STYLE, KINDS[it['kind']], FACTIONS[it.get('faction', 'neutral')]]
        if refs:
            lines.append('REFERENCES: ' + note)
        lines.append('THE SUBJECT: ' + it['subject'])
        req = {'slug': 'icon-' + it['name'], 'model': MODEL, 'size': '1024x1024', 'n': it.get('n', spec.get('n', 1)),
               'quality': 'high', 'background': 'transparent',
               'references': refs, 'prompt': '\n'.join(lines)}
        with open(os.path.join(out, it['name'] + '.json'), 'w') as f:
            json.dump(req, f, indent=2)
        print(it['name'], len(refs), 'references')


def picked(it):
    """Whether the item's folder already holds an icon installed by `pick`."""
    record = os.path.join(ROOT, it['dir'], 'generation.json')
    return os.path.exists(record) and 'background' in json.load(open(record)).get('install', {})


def flood(seed, passable):
    """The passable pixels connected to the seed pixels (4-neighbourhood)."""
    region = seed & passable
    while True:
        grown = region.copy()
        grown[1:, :] |= region[:-1, :]
        grown[:-1, :] |= region[1:, :]
        grown[:, 1:] |= region[:, :-1]
        grown[:, :-1] |= region[:, 1:]
        grown &= passable
        if (grown == region).all():
            return region
        region = grown


def background(name, size=SIZE):
    centre, corner = (np.array(c, np.float32) / 255.0 for c in BACKGROUNDS[name])
    t = (np.arange(size, dtype=np.float32) + 0.5) / size
    x, y = np.meshgrid(t, t)
    d = np.clip(np.hypot(x - 0.47, y - 0.45) / 0.72, 0, 1)
    k = d * d * (3 - 2 * d)
    return centre * (1 - k[:, :, None]) + corner * k[:, :, None]


def compose(foreground, bg_name, frame=None, level=0):
    """`image.png` from a straight-alpha foreground over a library background, with marks;
    background `none` keeps the cut-out (resources)."""
    if bg_name == 'none':
        return foreground
    fg = foreground.get_pixels(oiio.FLOAT)
    a = fg[:, :, 3:4]
    rgb = fg[:, :, :3] * a + background(bg_name, fg.shape[0]) * (1 - a)
    size = rgb.shape[0]
    buf = oiio.ImageBuf(oiio.ImageSpec(size, size, 3, oiio.FLOAT))
    buf.set_pixels(oiio.ROI(0, size, 0, size, 0, 1, 0, 3), np.ascontiguousarray(rgb, np.float32))
    return finish(buf, frame, level)


def key_white(buf):
    """Premultiplied RGBA from an image on flat white: the white connected to the border
    becomes transparent (white highlights inside the object stay), with a soft edge."""
    rgb = buf.get_pixels(oiio.FLOAT)[:, :, :3]
    h, w = rgb.shape[:2]
    whiteness = rgb.min(axis=2)
    near = whiteness > 0.88
    seed = np.zeros((h, w), bool)
    seed[0, :] = seed[-1, :] = seed[:, 0] = seed[:, -1] = True
    bg = flood(seed, near)
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
    """Downscaled (premultiplied in, straight alpha out for RGBA)."""
    n = buf.spec().nchannels
    roi = oiio.ROI(0, size, 0, size, 0, 1, 0, n)
    out = oiio.ImageBufAlgo.resize(buf, filtername='lanczos3', roi=roi)
    # Lanczos rings: keep values in range so premultiplied colour never exceeds alpha.
    out = oiio.ImageBufAlgo.clamp(out, 0.0, 1.0)
    return oiio.ImageBufAlgo.unpremult(out) if n == 4 else out


GOLD = np.array([0.93, 0.78, 0.42], np.float32)
BRONZE = np.array([0.55, 0.46, 0.30], np.float32)


def coverage(inside, size, ss=4):
    """Anti-aliased coverage (size × size) of the shape `inside(x, y)`, x and y in pixels."""
    t = (np.arange(size * ss, dtype=np.float32) + 0.5) / ss
    x, y = np.meshgrid(t, t)
    return inside(x, y).astype(np.float32).reshape(size, ss, size, ss).mean(axis=(1, 3))


def finish(buf, frame=None, level=0):
    """The category marks the generator paints itself, so they are identical on every icon:
    `frame: "passive"` darkens a border band with a thin bronze line inside it (Warcraft III's
    passive frame: the icon reads as not pressable), `level` 1–3 draws that many gold
    diamonds in the top-left corner (a research's level)."""
    if not frame and not level:
        return buf
    img = buf.get_pixels(oiio.FLOAT)
    n = img.shape[2]
    rgb = img[:, :, :3]
    size = rgb.shape[0]
    u = size / 256.0
    if frame == 'passive':
        edge = lambda x, y: np.minimum(np.minimum(x, y), np.minimum(size - x, size - y))
        band = coverage(lambda x, y: edge(x, y) < 18 * u, size)[:, :, None]
        line = coverage(lambda x, y: (edge(x, y) > 12 * u) & (edge(x, y) < 16 * u), size)[:, :, None]
        rgb = rgb * (1 - 0.72 * band)
        rgb = rgb * (1 - line) + BRONZE * line
    for i in range(level):
        cx, cy, r = (30 + 34 * i) * u, 30 * u, 15 * u
        outline = coverage(lambda x, y: np.abs(x - cx) + np.abs(y - cy) < r + 5 * u, size)[:, :, None]
        fill = coverage(lambda x, y: np.abs(x - cx) + np.abs(y - cy) < r, size)[:, :, None]
        shade = coverage(lambda x, y: (np.abs(x - cx) + np.abs(y - cy) < r) & (y > cy), size)[:, :, None]
        rgb = rgb * (1 - outline) + np.array([0.04, 0.03, 0.02], np.float32) * outline
        rgb = rgb * (1 - fill) + GOLD * fill
        rgb = rgb * (1 - 0.3 * shade)
    out = np.concatenate([rgb, img[:, :, 3:]], axis=2) if n == 4 else rgb
    res = oiio.ImageBuf(oiio.ImageSpec(size, size, n, oiio.FLOAT))
    res.set_pixels(oiio.ROI(0, size, 0, size, 0, 1, 0, n), np.ascontiguousarray(out, np.float32))
    return res


def write_asset(d, resources, defaults):
    """The folder's asset.json; an existing folder keeps its id, name, tags and race, so a
    replaced icon keeps every reference working."""
    path = os.path.join(d, 'asset.json')
    old = json.load(open(path)) if os.path.exists(path) else {}
    asset = {'id': old.get('id', defaults.get('id')), 'name': old.get('name', defaults.get('title')),
             'kind': 'icon', 'tags': old.get('tags', defaults.get('tags', []))}
    race = old.get('race', defaults.get('race'))
    if race:
        asset['race'] = race
    asset['resources'] = resources
    asset['license'] = 'Original artwork for Under the Canopy: GPT Image from the game\'s own concept art (docs/art-direction.md#icons).'
    if not asset['id']:
        sys.exit(f'{d}: a new icon folder needs "id" and "title" in its spec item')
    with open(path, 'w') as f:
        json.dump(asset, f, indent=2)
        f.write('\n')
    return asset


def pick(spec_path, picks):
    """Installs the chosen candidate of each `name=n` as its item's icon."""
    spec = json.load(open(spec_path))
    items = {it['name']: it for it in spec['items']}
    outputs = os.path.join(old_repository(), OUTPUTS)
    for p in picks:
        name, n = p.split('=')
        it = items[name]
        src = os.path.join(OUTPUTS, f'icon-{name}-{n}.png')
        d = os.path.join(ROOT, it['dir'])
        os.makedirs(d, exist_ok=True)
        fg = resized(load(os.path.join(outputs, f'icon-{name}-{n}.png'), alpha=True), SIZE)
        fg.set_write_format(oiio.UINT8)
        for f in os.listdir(d):
            if f not in ('asset.json',):
                os.remove(os.path.join(d, f))
        fg.write(os.path.join(d, 'source.png'))
        request = json.load(open(os.path.join(outputs, f'icon-{name}.prompt.json')))
        record = {k: request[k] for k in ('slug', 'model', 'size', 'quality', 'background', 'references', 'prompt') if k in request}
        record['install'] = {'src': src, 'kind': it['kind'],
                             'background': it.get('background', KIND_BACKGROUND.get(it['kind'], it['kind'])),
                             'frame': it.get('frame', KIND_FRAME.get(it['kind'])), 'level': it.get('level', 0)}
        with open(os.path.join(d, 'generation.json'), 'w') as f:
            json.dump(record, f, indent=2)
            f.write('\n')
        rebuild(d)
        write_asset(d, [{'role': 'image', 'index': 1, 'format': 'png'},
                        {'role': 'source', 'index': 1, 'format': 'png'},
                        {'role': 'generation', 'index': 1, 'format': 'json'}], it)
        print(name, '->', it['dir'])


def rebuild(d):
    """`image.png` from the folder's foreground and the choices in its generation.json."""
    choice = json.load(open(os.path.join(d, 'generation.json')))['install']
    fg = oiio.ImageBuf(os.path.join(d, 'source.png'))
    img = compose(fg, choice['background'], choice.get('frame'), choice.get('level', 0))
    img.set_write_format(oiio.UINT8)
    img.write(os.path.join(d, 'image.png'))


def recompose(prefix='base/assets'):
    """Rebuilds every composed icon whose folder path starts with `prefix`."""
    count = 0
    for top, _, files in os.walk(os.path.join(ROOT, 'base/assets')):
        if os.path.relpath(top, ROOT).startswith(prefix) and {'source.png', 'generation.json'} <= set(files):
            if 'background' in json.load(open(os.path.join(top, 'generation.json'))).get('install', {}):
                rebuild(top)
                count += 1
    print(count, 'icons recomposed')


def candidates(spec_path, out, only=()):
    """A sheet of every generated candidate, one row per item (in spec order, candidates
    left to right), composed over the item's background with its marks, at 160 and 46 px."""
    spec = json.load(open(spec_path))
    outputs = os.path.join(old_repository(), OUTPUTS)
    rows = []
    for it in spec['items']:
        if only and it['name'] not in only:
            continue
        row, n = [], 1
        while os.path.exists(os.path.join(outputs, f'icon-{it["name"]}-{n}.png')):
            fg = resized(load(os.path.join(outputs, f'icon-{it["name"]}-{n}.png'), alpha=True), SIZE)
            bg = it.get('background', KIND_BACKGROUND.get(it['kind'], it['kind']))
            img = compose(fg, bg, it.get('frame', KIND_FRAME.get(it['kind'])), it.get('level', 0))
            px = img.get_pixels(oiio.FLOAT)
            if px.shape[2] == 4:  # a cut-out: over the sheet's colour
                px = px[:, :, :3] * px[:, :, 3:] + np.array([24, 24, 27], np.float32) / 255.0 * (1 - px[:, :, 3:])
            row.append(px)
            n += 1
        if row:
            rows.append((it['name'], row))
    big, small, gap = 160, 46, 8
    cols = max(len(r) for _, r in rows)
    w = gap + cols * (big + small + 3 * gap)
    h = gap + len(rows) * (big + gap)
    canvas = np.empty((h, w, 3), np.float32)
    canvas[:] = np.array([24, 24, 27], np.float32) / 255.0
    for y, (name, row) in enumerate(rows):
        for x, img in enumerate(row):
            px, py = gap + x * (big + small + 3 * gap), gap + y * (big + gap)
            for size, ox, oy in ((big, 0, 0), (small, big + gap, big - small)):
                b = oiio.ImageBuf(oiio.ImageSpec(SIZE, SIZE, 3, oiio.FLOAT))
                b.set_pixels(oiio.ROI(0, SIZE, 0, SIZE, 0, 1, 0, 3), np.ascontiguousarray(img))
                canvas[py + oy:py + oy + size, px + ox:px + ox + size] = resized(b, size).get_pixels(oiio.FLOAT)
        print(y + 1, name, len(row), 'candidates')
    buf = oiio.ImageBuf(oiio.ImageSpec(w, h, 3, oiio.UINT8))
    buf.set_pixels(oiio.ROI(0, w, 0, h, 0, 1, 0, 3), canvas)
    buf.write(out)


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
        img.set_write_format(oiio.UINT8)
        for f in os.listdir(d):
            if f != 'asset.json':
                os.remove(os.path.join(d, f))
        img.write(os.path.join(d, 'image.png'))
        resources = [{'role': 'image', 'index': 1, 'format': 'png'}]
        if it.get('request'):
            record = dict(it['request'])
            record['install'] = {k: it[k] for k in ('crop', 'alpha', 'key') if k in it}
            with open(os.path.join(d, 'generation.json'), 'w') as f:
                json.dump(record, f, indent=2)
                f.write('\n')
            resources.append({'role': 'generation', 'index': 1, 'format': 'json'})
        asset = write_asset(d, resources, it)
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
    cmd, args = sys.argv[1] if len(sys.argv) > 1 else '', sys.argv[2:]
    if cmd == 'requests':
        requests(args[0], args[1], args[2:])
    elif cmd == 'pick':
        pick(args[0], args[1:])
    elif cmd == 'candidates':
        candidates(args[0], args[1], args[2:])
    elif cmd == 'recompose':
        recompose(*args[:1])
    elif cmd == 'install':
        install(args[0])
    elif cmd == 'sheet':
        sheet(args[0], int(args[1]), int(args[2]), args[3:])
    else:
        sys.exit(__doc__)
