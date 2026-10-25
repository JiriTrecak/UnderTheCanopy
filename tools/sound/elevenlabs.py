"""Sound authoring with ElevenLabs (D97): sound effects and loops, music, designed voices and
spoken lines, requested as takes, auditioned, and one picked into an audio asset folder with
its `generation.json` (and, when the spec says so, the sound event that plays it).

Generation is authoring only: the game never calls ElevenLabs.

    python3 tools/sound/elevenlabs.py check [--online]
    python3 tools/sound/elevenlabs.py request tools/sound/specs/<spec>.json <scratch> [name ...]
    python3 tools/sound/elevenlabs.py list <scratch>
    python3 tools/sound/elevenlabs.py pick tools/sound/specs/<spec>.json <scratch> <name>=<take> ...
    python3 tools/sound/elevenlabs.py voice <scratch>/<name> <preview>   # keeps a designed voice
    python3 tools/sound/elevenlabs.py dry tools/sound/specs/<spec>.json [name ...]

The key is read from ELEVENLABS_API_KEY or ~/.config/under-the-canopy/elevenlabs-key (one
line, mode 0600). It is never printed, logged or written anywhere, and errors are redacted.

A spec is {"items": [...]}; each item:

    {"name": "ants-warrior-strike",          # unique across specs; takes go to <scratch>/<name>
     "kind": "effect" | "music" | "voice" | "line",
     "prompt": "...",                         # effect, music: what it should sound like
     "seconds": 1.5,                          # effect 0.5-30, music 3-600 (default: the model's)
     "loop": false,                           # effect: a seamless loop
     "influence": 0.3,                        # effect: how literally the prompt is followed (0-1)
     "instrumental": true,                    # music (default true)
     "takes": 2,                              # how many to request (default 2; music 1)
     "dir": "base/assets/audio/ants/warrior-strike",   # the asset folder pick writes
     "id": "asset.audio.ants.warrior-strike", "title": "Warrior strike",
     "event": {"id": "sound.ants.warrior.strike", "category": "world", ...}}   # optional:
                                              # the sound event pick creates or adds to
                                              # (`file` places it; base/sounds/<scope> else)

    voice: {"name", "kind": "voice", "speaker": "speaker.aurelith", "description": "...",
            "text": "a sample line, 100-1000 characters"}: designs previews; `voice` keeps one
            under the speaker in tools/sound/voices.json.
    line:  {"name", "kind": "line", "line": "line.founding.here-we-start-again-and",
            "speaker": "speaker.aurelith", "tags": "[calm]", "dir", "id"}: speaks the line's
            English text (read from the text library) in the speaker's kept voice; pick
            makes the event `sound.<line key>` in the package of `dir`, which the game
            plays when the line is spoken.
"""
import base64
import glob
import json
import os
import shutil
import subprocess
import sys
import urllib.error
import urllib.parse
import urllib.request

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
API = 'https://api.elevenlabs.io'
KEY_FILE = os.path.expanduser('~/.config/under-the-canopy/elevenlabs-key')
VOICES = os.path.join(ROOT, 'tools/sound/voices.json')

# Endpoints and models, in one place (ElevenLabs API, 2026-10).
EFFECT = ('/v1/sound-generation', 'eleven_text_to_sound_v2')
MUSIC = ('/v1/music', 'music_v1')
DESIGN = ('/v1/text-to-voice/design', 'eleven_ttv_v3')
KEEP_VOICE = '/v1/text-to-voice'
SPEECH = ('/v1/text-to-speech/{voice}', 'eleven_v3')
# Effects arrive as MP3 and are stored as 16-bit WAV (short sounds stay PCM in the pack);
# music and voice stay MP3 and stream (no second lossy encoding). 128 kbps works on every
# plan; ELEVENLABS_FORMAT=mp3_44100_192 on Creator and above.
FORMAT = os.environ.get('ELEVENLABS_FORMAT', 'mp3_44100_128')


def key():
    value = os.environ.get('ELEVENLABS_API_KEY', '').strip()
    if not value and os.path.exists(KEY_FILE):
        if os.stat(KEY_FILE).st_mode & 0o077:
            sys.exit(f'{KEY_FILE} must be readable by you alone: chmod 600 it')
        value = open(KEY_FILE).read().strip()
    if not value:
        sys.exit('No ElevenLabs key: set ELEVENLABS_API_KEY or write it to '
                 f'{KEY_FILE} (chmod 600).')
    return value


def redact(text, secret):
    return text.replace(secret, '[key]') if secret else text


def call(path, body=None, query=None, method='POST'):
    """One API call; returns (bytes, content type). Errors never carry the key."""
    secret = key()
    url = API + path + ('?' + urllib.parse.urlencode(query) if query else '')
    data = json.dumps(body).encode() if body is not None else None
    request = urllib.request.Request(url, data=data, method=method, headers={
        'xi-api-key': secret, 'Content-Type': 'application/json', 'Accept': '*/*'})
    try:
        with urllib.request.urlopen(request, timeout=600) as r:
            return r.read(), r.headers.get('Content-Type', '')
    except urllib.error.HTTPError as e:
        detail = redact(e.read().decode('utf-8', 'replace')[:800], secret)
        sys.exit(f'ElevenLabs {path}: HTTP {e.code}: {detail}')
    except urllib.error.URLError as e:
        sys.exit(f'ElevenLabs {path}: {redact(str(e.reason), secret)}')


def specs():
    """Every item of every spec, by name."""
    out = {}
    for path in sorted(glob.glob(os.path.join(ROOT, 'tools/sound/specs/*.json'))):
        for item in json.load(open(path)).get('items', []):
            if item['name'] in out:
                sys.exit(f"{item['name']} is in two specs")
            out[item['name']] = item
    return out


def items_of(spec, names):
    items = json.load(open(spec)).get('items', [])
    if names:
        missing = set(names) - {i['name'] for i in items}
        if missing:
            sys.exit(f'not in {spec}: {", ".join(sorted(missing))}')
        items = [i for i in items if i['name'] in names]
    return items


def text_of(key_name):
    """The English text of a library key: the base's, a campaign's or a mission's."""
    for path in glob.glob(os.path.join(ROOT, 'base/**/text/en/*.json'), recursive=True):
        value = json.load(open(path)).get(key_name)
        if value is not None:
            return value
    sys.exit(f'no text file holds {key_name}')


def voices():
    return json.load(open(VOICES)) if os.path.exists(VOICES) else {}


def request_body(item):
    """(path, query, body, extension) of an item's request, without calling anything."""
    kind = item['kind']
    if kind == 'effect':
        body = {'text': item['prompt'], 'model_id': item.get('model', EFFECT[1]),
                'prompt_influence': item.get('influence', 0.3)}
        if 'seconds' in item:
            body['duration_seconds'] = max(0.5, min(30.0, float(item['seconds'])))
        if item.get('loop'):
            body['loop'] = True
        return EFFECT[0], {'output_format': FORMAT}, body, 'mp3'
    if kind == 'music':
        body = {'prompt': item['prompt'], 'model_id': item.get('model', MUSIC[1]),
                'force_instrumental': item.get('instrumental', True)}
        if 'seconds' in item:
            body['music_length_ms'] = int(max(3, min(600, float(item['seconds']))) * 1000)
        return MUSIC[0], {'output_format': FORMAT}, body, 'mp3'
    if kind == 'voice':
        body = {'voice_description': item['description'], 'model_id': item.get('model', DESIGN[1])}
        if item.get('text'):
            body['text'] = item['text']
        else:
            body['auto_generate_text'] = True
        return DESIGN[0], {'output_format': FORMAT}, body, 'json'
    if kind == 'line':
        voice = voices().get(item['speaker'], {}).get('voice_id')
        if not voice:
            sys.exit(f"{item['name']}: no voice kept for {item['speaker']} (design one first)")
        text = text_of(item['line'])
        if item.get('tags'):
            text = f"{item['tags']} {text}"
        body = {'text': text, 'model_id': item.get('model', SPEECH[1])}
        if item.get('settings'):
            body['voice_settings'] = item['settings']
        return SPEECH[0].format(voice=voice), {'output_format': FORMAT}, body, 'mp3'
    sys.exit(f"{item['name']}: unknown kind {kind}")


def request(spec, scratch, names):
    for item in items_of(spec, names):
        if not names and picked(item):
            continue
        folder = os.path.join(scratch, item['name'])
        os.makedirs(folder, exist_ok=True)
        path, query, body, ext = request_body(item)
        takes = item.get('takes', 1 if item['kind'] in ('music', 'voice') else 2)
        start = len(glob.glob(os.path.join(folder, 'take-*'))) + 1
        record = os.path.join(folder, 'request.json')
        log = json.load(open(record)) if os.path.exists(record) else {'item': item, 'takes': []}
        for n in range(start, start + takes):
            data, kind = call(path, body, query)
            if item['kind'] == 'voice':
                previews = json.loads(data).get('previews', [])
                for p, preview in enumerate(previews, 1):
                    out = os.path.join(folder, f'take-{n}-{p}.mp3')
                    open(out, 'wb').write(base64.b64decode(preview['audio_base_64']))
                    log['takes'].append({'file': os.path.basename(out),
                                         'generated_voice_id': preview['generated_voice_id']})
                print(item['name'], f'{len(previews)} previews')
                continue
            out = os.path.join(folder, f'take-{n}.{ext}')
            open(out, 'wb').write(data)
            log['takes'].append({'file': os.path.basename(out), 'request': body, 'path': path})
            print(item['name'], os.path.relpath(out, scratch))
        json.dump(log, open(record, 'w'), indent=2, ensure_ascii=False)


def picked(item):
    record = os.path.join(ROOT, item.get('dir', ''), 'generation.json')
    return bool(item.get('dir')) and os.path.exists(record)


def list_takes(scratch):
    for folder in sorted(glob.glob(os.path.join(scratch, '*/request.json'))):
        log = json.load(open(folder))
        print(log['item']['name'], '·', log['item']['kind'])
        for take in log['takes']:
            print('   ', os.path.join(os.path.dirname(folder), take['file']))


def ffmpeg(*args):
    if not shutil.which('ffmpeg'):
        sys.exit('ffmpeg is needed to store effects as WAV')
    subprocess.run(['ffmpeg', '-hide_banner', '-loglevel', 'error', '-y', *args], check=True)


def pick(spec, scratch, choices):
    items = {i['name']: i for i in items_of(spec, [])}
    for choice in choices:
        name, _, take = choice.partition('=')
        item = items.get(name) or sys.exit(f'not in {spec}: {name}')
        if item['kind'] == 'voice':
            sys.exit(f'{name} is a voice: keep it with `voice <scratch>/{name} <preview>`')
        log = json.load(open(os.path.join(scratch, name, 'request.json')))
        chosen = next((t for t in log['takes'] if t['file'].split('.')[0] == f'take-{take}'),
                      None) or sys.exit(f'{name}: no take {take}')
        source = os.path.join(scratch, name, chosen['file'])
        folder = os.path.join(ROOT, item['dir'])
        os.makedirs(folder, exist_ok=True)
        for old in glob.glob(os.path.join(folder, 'audio.*')):
            os.remove(old)
        if item['kind'] == 'effect':
            fmt = 'wav'
            ffmpeg('-i', source, '-ac', '1' if item.get('mono', True) else '2', '-ar', '44100',
                   '-c:a', 'pcm_s16le', os.path.join(folder, 'audio.wav'))
        else:
            # Copied, not re-encoded; the remux writes the length header streaming needs.
            fmt = 'mp3'
            ffmpeg('-i', source, '-c:a', 'copy', '-write_xing', '1',
                   os.path.join(folder, 'audio.mp3'))
        asset_path = os.path.join(folder, 'asset.json')
        asset = json.load(open(asset_path)) if os.path.exists(asset_path) else {
            'id': item['id'], 'name': item.get('title', name), 'kind': 'audio',
            'tags': item.get('tags', [item['kind']]), 'license': (
                'Generated with ElevenLabs for Under the Canopy (generation.json).')}
        asset['resources'] = [{'role': 'audio', 'index': 1, 'format': fmt},
                              {'role': 'generation', 'index': 1, 'format': 'json'}]
        json.dump(asset, open(asset_path, 'w'), indent=2, ensure_ascii=False)
        open(asset_path, 'a').write('\n')
        generation = {'provider': 'elevenlabs', 'kind': item['kind'], 'spec': item,
                      'takes': [t['file'] for t in log['takes']], 'picked': chosen['file'],
                      'request': chosen.get('request'), 'endpoint': chosen.get('path')}
        if item['kind'] == 'line':
            generation['text'] = text_of(item['line'])
            generation['voice'] = voices().get(item['speaker'])
        with open(os.path.join(folder, 'generation.json'), 'w') as f:
            f.write(json.dumps(generation, indent=2, ensure_ascii=False) + '\n')
        if item['kind'] == 'line' and not item.get('event'):
            # A line's voice is the event the game looks for by the line's key (D97).
            item = dict(item, event={'id': f"sound.{item['line']}", 'category': 'dialogue',
                                     'file': os.path.join(item['dir'].split('/assets/')[0],
                                                          'sounds', 'lines',
                                                          item['line'] + '.json')})
        if item.get('event'):
            write_event(item)
        print(name, '→', os.path.relpath(folder, ROOT))


def write_event(item):
    """The sound event that plays the picked asset: created, or the clip added to it."""
    event = dict(item['event'])
    scope = event['id'][len('sound.'):].replace('.', '/')
    path = os.path.join(ROOT, event.pop('file', f'base/sounds/{scope}.json'))
    doc = json.load(open(path)) if os.path.exists(path) else {'id': event['id']}
    doc.update({k: v for k, v in event.items() if k != 'id'})
    clips = doc.setdefault('clips', [])
    if item['id'] not in clips:
        clips.append(item['id'])
    os.makedirs(os.path.dirname(path), exist_ok=True)
    with open(path, 'w') as f:
        f.write(json.dumps(doc, indent=2, ensure_ascii=False) + '\n')


def keep_voice(folder, preview):
    log = json.load(open(os.path.join(folder, 'request.json')))
    item = log['item']
    chosen = next((t for t in log['takes'] if t['file'] == f'take-{preview}.mp3'),
                  None) or sys.exit(f'no preview {preview} (previews are named like 1-2)')
    data, _ = call(KEEP_VOICE, {'voice_name': item['speaker'],
                                'voice_description': item['description'],
                                'generated_voice_id': chosen['generated_voice_id']})
    voice_id = json.loads(data)['voice_id']
    kept = voices()
    kept[item['speaker']] = {'voice_id': voice_id, 'description': item['description'],
                             'preview': chosen['file'], 'model': item.get('model', DESIGN[1])}
    with open(VOICES, 'w') as f:
        f.write(json.dumps(kept, indent=2, ensure_ascii=False, sort_keys=True) + '\n')
    print(item['speaker'], 'kept as', voice_id)


# What a restricted key needs, by kind of request.
PERMISSIONS = 'Sound Effects (effects, loops), Music, Text to Speech (lines), Voices: write (design and keep voices)'


def check(online):
    secret = key()
    print('key found')
    if online:
        # Listing the models costs nothing; a restricted key without that permission is still
        # a key ElevenLabs accepts.
        request = urllib.request.Request(API + '/v1/models', headers={'xi-api-key': secret})
        try:
            with urllib.request.urlopen(request, timeout=60) as r:
                print('key accepted;', len(json.loads(r.read())), 'models available')
        except urllib.error.HTTPError as e:
            detail = redact(e.read().decode('utf-8', 'replace'), secret)
            if 'missing_permissions' in detail:
                print('key accepted (it is restricted: it cannot list models, which this tool does not need)')
            else:
                sys.exit(f'ElevenLabs refused the key: HTTP {e.code}: {detail[:300]}')
        print('a restricted key needs:', PERMISSIONS)
    print(len(specs()), 'spec items')


def main():
    args = sys.argv[1:]
    if not args:
        sys.exit(__doc__)
    command, rest = args[0], args[1:]
    if command == 'check':
        check('--online' in rest)
    elif command == 'dry':
        for item in items_of(rest[0], rest[1:]):
            path, query, body, _ = request_body(item)
            print(json.dumps({'name': item['name'], 'path': path, 'query': query,
                              'body': body}, ensure_ascii=False))
    elif command == 'request':
        request(rest[0], rest[1], rest[2:])
    elif command == 'list':
        list_takes(rest[0])
    elif command == 'pick':
        pick(rest[0], rest[1], rest[2:])
    elif command == 'voice':
        keep_voice(rest[0], rest[1])
    else:
        sys.exit(__doc__)


if __name__ == '__main__':
    main()
