"""One-off migration (D96): moves a campaign's text into text libraries, leaving keys where
the text was.

- campaign.json: each step's title, chapter line, story, page texts and note →
  `campaign.<campaign>.<step>.<field>` in the campaign's `text/en/campaign.json`;
- each mission's map.json objectives → `objective.<mission>.<id>.title|description`, and
  every prose string of its mission.lua (dialogue lines, objective lines) →
  `line.<mission>.<words>`, in the mission's own `text/en/mission.json`;
- the scripts' speaker names (capitalised words: "Aurelith", "Nurse Ardi") →
  `speaker.<name>` in the campaign's text, shared by its missions.

    python3 tools/text/migrate_campaign.py base/campaigns/vanguard.utcc
"""
import glob
import json
import os
import re
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import textlib  # noqa: E402


def slug(text, words=5):
    parts = re.findall(r'[a-z0-9]+', text.lower())
    return '-'.join(parts[:words]) or 'line'


def lua_strings(src):
    """(start, end, quote, value) of every short string literal outside comments."""
    out, i, n = [], 0, len(src)
    while i < n:
        if src.startswith('--', i):
            if src.startswith('--[[', i):
                j = src.find(']]', i)
                i = n if j < 0 else j + 2
            else:
                j = src.find('\n', i)
                i = n if j < 0 else j
            continue
        c = src[i]
        if c in '"\'':
            j = i + 1
            value = []
            while j < n and src[j] != c:
                if src[j] == '\\' and j + 1 < n:
                    value.append(src[j:j + 2])
                    j += 2
                    continue
                value.append(src[j])
                j += 1
            out.append((i, j + 1, c, ''.join(value)))
            i = j + 1
            continue
        if src.startswith('[[', i):
            j = src.find(']]', i)
            i = n if j < 0 else j + 2
            continue
        i += 1
    return out


def prose(value):
    words = value.split()
    return re.search(r'[A-Za-z]', value) and (
        len(words) >= 4 or re.search(r'[.!?…]["\']?$', value))


def speaker(value):
    return re.fullmatch(r"[A-Z][A-Za-z'\-]+( [A-Z][A-Za-z'\-]+){0,2}", value)


def unescape(value):
    return value.encode('utf-8').decode('unicode_escape').encode('latin-1').decode('utf-8') \
        if '\\' in value else value


def write_json(path, doc):
    with open(path, 'w') as f:
        f.write(json.dumps(doc, indent=2, ensure_ascii=False) + '\n')


def main():
    root = os.path.abspath(sys.argv[1])
    campaign_id = json.load(open(os.path.join(root, 'package.json')))['id']
    path = os.path.join(root, 'campaign.json')
    campaign = json.load(open(path))
    entries = textlib.read(root, 'campaign')
    for step in campaign.get('steps', []):
        base = f"campaign.{campaign_id}.{step['id']}"
        for field, key in (('title', 'title'), ('chapterLine', 'chapter-line'),
                           ('story', 'story'), ('note', 'note')):
            if isinstance(step.get(field), str) and not step[field].startswith(base):
                entries[f'{base}.{key}'] = step[field]
                step[field] = f'{base}.{key}'
        for n, page in enumerate(step.get('pages', []), 1):
            if isinstance(page.get('text'), str) and not page['text'].startswith(base):
                entries[f'{base}.page-{n}'] = page['text']
                page['text'] = f'{base}.page-{n}'
    write_json(path, campaign)
    speakers = entries
    moved = 0
    for mission in sorted(glob.glob(os.path.join(root, 'missions', '*.utcm'))):
        mid = os.path.basename(mission)[:-5]
        entries = textlib.read(mission, 'mission')
        map_path = os.path.join(mission, 'map.json')
        doc = json.load(open(map_path))
        for o in (doc.get('mission') or {}).get('objectives') or []:
            for field in ('title', 'description'):
                key = f"objective.{mid}.{o['id']}.{field}"
                if isinstance(o.get(field), str) and o[field] and o[field] != key:
                    entries[key] = o[field]
                    o[field] = key
        write_json(map_path, doc)
        script_path = os.path.join(mission, 'mission.lua')
        if os.path.exists(script_path):
            src = open(script_path).read()
            by_text = {v: k for k, v in entries.items() if k.startswith('line.')}
            parts, last = [], 0
            for start, end, quote, value in lua_strings(src):
                if speaker(value):
                    key = f'speaker.{slug(value)}'
                    speakers[key] = value
                    parts.append(src[last:start])
                    parts.append(f'"{key}"')
                    last = end
                    continue
                if not prose(value):
                    continue
                text = unescape(value)
                key = by_text.get(text)
                if not key:
                    stem = f'line.{mid}.{slug(text)}'
                    key, k = stem, 2
                    while key in entries and entries[key] != text:
                        key, k = f'{stem}-{k}', k + 1
                    entries[key] = text
                    by_text[text] = key
                parts.append(src[last:start])
                parts.append(f'"{key}"')
                last = end
            parts.append(src[last:])
            open(script_path, 'w').write(''.join(parts))
        textlib.write(mission, 'mission', entries)
        moved += len(entries)
    textlib.write(root, 'campaign', speakers)
    moved += len(speakers)
    print(f'{moved} strings in {os.path.relpath(root)} and its missions')


if __name__ == '__main__':
    main()
