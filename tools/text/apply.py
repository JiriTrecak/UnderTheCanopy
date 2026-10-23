"""Applies a batch of code-to-key edits (D96): each edit replaces an exact snippet in a source
file (it must occur exactly `count` times, default 1) and adds the English it carried to the
base text library.

    python3 tools/text/apply.py edits.json

edits.json: {"area": "game", "edits": [{"file": "...", "old": "...", "new": "...",
             "count": 1}], "text": {"key": "English"}}
"""
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import textlib  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def main():
    batch = json.load(open(sys.argv[1]))
    sources = {}
    for edit in batch['edits']:
        path = os.path.join(ROOT, edit['file'])
        text = sources.get(path) or open(path).read()
        count = text.count(edit['old'])
        want = edit.get('count', 1)
        if count != want:
            sys.exit(f"{edit['file']}: expected {want} of {edit['old']!r}, found {count}")
        sources[path] = text.replace(edit['old'], edit['new'])
    for path, text in sources.items():
        open(path, 'w').write(text)
    base = os.path.join(ROOT, 'base')
    entries = textlib.read(base, batch['area'])
    for key, value in batch.get('text', {}).items():
        if key in entries and entries[key] != value:
            sys.exit(f'{key} already holds {entries[key]!r}')
        entries[key] = value
    textlib.write(base, batch['area'], entries)
    print(f"{len(batch['edits'])} edits, {len(batch.get('text', {}))} strings → text/en/{batch['area']}.json")


if __name__ == '__main__':
    main()
