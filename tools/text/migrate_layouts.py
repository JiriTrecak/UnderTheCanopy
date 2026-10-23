"""One-off migration (D96): moves the literal strings of the interface layouts
(config/ui/layouts/*.json: `text`, `placeholder`, `options` and tooltips' `title`, `body`,
`note`) into the base text library, `base/text/en/ui.json`, as keys `ui.<layout>.<name>`,
rewriting each literal in place as `"@<key>"` so the files keep their layout.

    python3 tools/text/migrate_layouts.py
"""
import glob
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import textlib  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SKIP = {'gallery'}  # the widget showcase, not a shipped screen
FIELD = re.compile(r'"(text|placeholder|title|body|note)"(\s*:\s*)"((?:[^"\\]|\\.)*)"')
OPTIONS = re.compile(r'"options"(\s*:\s*)\[([^\]]*)\]')
STRING = re.compile(r'"((?:[^"\\]|\\.)*)"')


def slug(text, words=5):
    text = re.sub(r'\[[^\]]*\]', ' ', text)  # markup
    parts = re.findall(r'[a-z0-9]+', text.lower())
    return '-'.join(parts[:words]) or 'text'


def wanted(text):
    return not text.startswith('@') and re.search(r'[A-Za-z]', text)


def main():
    entries = textlib.read(os.path.join(ROOT, 'base'), 'ui')
    by_string = {}
    moved = 0
    for path in sorted(glob.glob(os.path.join(ROOT, 'config/ui/layouts/*.json'))):
        layout = os.path.basename(path)[:-5]
        if layout in SKIP:
            continue
        source = open(path).read()
        last_title = {'slug': 'text'}

        def key_for(text, hint=None):
            nonlocal moved
            known = by_string.get((layout, text))
            if known:
                return known
            base = f'ui.{layout}.{hint or slug(text)}'
            key, n = base, 2
            while key in entries and entries[key] != text:
                key, n = f'{base}-{n}', n + 1
            entries[key] = text
            by_string[(layout, text)] = key
            moved += 1
            return key

        def field(m):
            name, colon, raw = m.group(1), m.group(2), m.group(3)
            text = json.loads(f'"{raw}"')
            if not wanted(text):
                return m.group(0)
            if name == 'title':
                last_title['slug'] = slug(text)
                key = key_for(text)
            elif name in ('body', 'note'):
                key = key_for(text, f"{last_title['slug']}.tip" if name == 'body' else f"{last_title['slug']}.note")
            else:
                key = key_for(text)
            return f'"{name}"{colon}"@{key}"'

        def options(m):
            colon, inner = m.group(1), m.group(2)

            def one(s):
                text = json.loads(f'"{s.group(1)}"')
                return f'"@{key_for(text)}"' if wanted(text) else s.group(0)
            return f'"options"{colon}[{STRING.sub(one, inner)}]'

        out = FIELD.sub(field, source)
        out = OPTIONS.sub(options, out)
        json.loads(out)
        if out != source:
            open(path, 'w').write(out)
    textlib.write(os.path.join(ROOT, 'base'), 'ui', entries)
    print(f'{moved} layout strings moved into base/text/en/ui.json')


if __name__ == '__main__':
    main()
