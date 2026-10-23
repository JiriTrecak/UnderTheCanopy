"""The text library's files (D96): `text/<language>/<area>.json`, flat maps of key → string,
kept sorted by key so generators and hand edits merge cleanly.

Content text is keyed by the element's id: `<id>.name`, `<id>.description`; races, commands
and combat types get their kind (`race.ants.name`, `command.move.name`, `armor.light.name`).
"""
import json
import os

LANGUAGE = 'en'

# Content folders whose elements carry a name and a description, and the area file their
# text goes to.
AREAS = {
    'units': 'units',
    'buildings': 'buildings',
    'items': 'items',
    'resources': 'items',
    'abilities': 'abilities',
    'research': 'research',
    'races': 'races',
}
FIELDS = ('name', 'description')


def area_of(rel):
    """The area a content file's text belongs to (`units/ants/warrior.json` → units)."""
    return AREAS.get(rel.split('/', 1)[0])


def key_of(rel, doc):
    """The key prefix of a content document."""
    if rel.startswith('races/'):
        return 'race.' + doc['id']
    return doc['id']


def take(rel, doc):
    """Removes a content document's text, returning (area, {key: string}); (None, {}) for
    files that carry none."""
    area = area_of(rel)
    if area is None or not isinstance(doc, dict) or 'id' not in doc:
        return None, {}
    prefix = key_of(rel, doc)
    out = {}
    for field in FIELDS:
        if field in doc:
            out[f'{prefix}.{field}'] = doc.pop(field)
    return area, out


def path(base, area, language=LANGUAGE):
    return os.path.join(base, 'text', language, area + '.json')


def read(base, area, language=LANGUAGE):
    p = path(base, area, language)
    if not os.path.exists(p):
        return {}
    with open(p) as f:
        return json.load(f)


FIELD_ORDER = {'name': 0, 'description': 1}


def order(key):
    """Keys sort by element, then name before description."""
    head, _, last = key.rpartition('.')
    return (head, FIELD_ORDER.get(last, 2), last) if last in FIELD_ORDER else (key, -1, '')


def write(base, area, entries, language=LANGUAGE):
    p = path(base, area, language)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    ordered = {k: entries[k] for k in sorted(entries, key=order)}
    with open(p, 'w') as f:
        f.write(json.dumps(ordered, indent=2, ensure_ascii=False) + '\n')


def merge(base, area, entries, language=LANGUAGE):
    """Sets `entries` in an area file, keeping every other key."""
    current = read(base, area, language)
    current.update(entries)
    write(base, area, current, language)


class Pending:
    """Text taken from documents as a generator writes them, merged into the library once at
    the end (`flush`)."""

    def __init__(self):
        self.areas = {}

    def take(self, rel, doc):
        area, entries = take(rel, doc)
        if area:
            self.areas.setdefault(area, {}).update(entries)

    def flush(self, base):
        for area, entries in sorted(self.areas.items()):
            merge(base, area, entries)
        self.areas = {}
