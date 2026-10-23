"""One-off migration (D96): moves every name and description out of a package's content files
(units, buildings, items, resources, abilities, research, races, and rules/commands.json and
rules/combat.json) into its text library, `text/en/<area>.json`.

    python3 tools/text/migrate_content.py [package folder, default base]
"""
import glob
import json
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import textlib  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))


def dump(path, doc):
    with open(path, 'w') as f:
        f.write(json.dumps(doc, indent=2, ensure_ascii=False) + '\n')


def main():
    base = os.path.join(ROOT, sys.argv[1] if len(sys.argv) > 1 else 'base')
    pending = textlib.Pending()
    moved = 0
    for folder in textlib.AREAS:
        for p in sorted(glob.glob(os.path.join(base, folder, '**', '*.json'), recursive=True)):
            rel = os.path.relpath(p, base)
            with open(p) as f:
                doc = json.load(f)
            before = len(doc)
            pending.take(rel, doc)
            if len(doc) != before:
                dump(p, doc)
                moved += before - len(doc)
    commands_path = os.path.join(base, 'rules', 'commands.json')
    if os.path.exists(commands_path):
        with open(commands_path) as f:
            commands = json.load(f)
        entries = {}
        for group, prefix in (('actions', 'command.'), ('categories', ''), ('navigation', 'navigation.')):
            for id_, entry in commands.get(group, {}).items():
                for field in textlib.FIELDS:
                    if field in entry:
                        entries[f'{prefix}{id_}.{field}'] = entry.pop(field)
        if entries:
            dump(commands_path, commands)
            pending.areas.setdefault('commands', {}).update(entries)
            moved += len(entries)
    combat_path = os.path.join(base, 'rules', 'combat.json')
    if os.path.exists(combat_path):
        with open(combat_path) as f:
            combat = json.load(f)
        entries = {}
        for field, prefix in (('armorTypes', 'armor'), ('damageTypes', 'damage')):
            for id_, entry in combat.get(field, {}).items():
                if 'name' in entry:
                    entries[f'{prefix}.{id_}.name'] = entry.pop('name')
        if entries:
            dump(combat_path, combat)
            pending.areas.setdefault('combat', {}).update(entries)
            moved += len(entries)
    pending.flush(base)
    print(f'{moved} strings moved into {os.path.relpath(base, ROOT)}/text/{textlib.LANGUAGE}')


if __name__ == '__main__':
    main()
