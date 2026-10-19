"""Shared helpers for writing the beetle content."""
import json, os

# The repository this file sits in (tools/content/beetles), so a worktree writes its own base/.
ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))) + '/'
BASE = ROOT + 'base/'
WRITTEN = []


def write(rel, doc):
    path = BASE + rel
    os.makedirs(os.path.dirname(path), exist_ok=True)
    text = json.dumps(doc, indent=2, ensure_ascii=False) + '\n'
    with open(path, 'w') as f:
        f.write(text)
    WRITTEN.append(rel)


def remove(rel):
    path = BASE + rel
    if os.path.exists(path):
        os.remove(path)


def cost(amber, substance=0):
    items = [{'item': 'item.amber', 'amount': amber}]
    if substance:
        items.append({'item': 'item.root', 'amount': substance})
    return items


REVALIDATE = ['caster', 'target', 'range', 'visibility', 'relationship']


def rp(name):
    return {'rankParameter': name}


def cast(prepare, recover, mana, cooldown, channel=None):
    c = {
        'prepareTicks': prepare,
        'recoverTicks': recover,
        'cost': {'resource': 'mana', 'amount': mana, 'commit': 'release'},
        'cooldown': {'ticks': cooldown, 'start': 'release', 'scope': 'ability'},
        'cancelBeforeRelease': 'refund',
        'revalidateAtRelease': REVALIDATE,
    }
    if channel:
        c['channel'] = channel
    return c


def self_aim():
    return {'kind': 'self', 'alive': True, 'visible': True, 'relations': ['ally'], 'allowSelf': True,
            'range': 0, 'outOfRange': 'reject'}


def unit_aim(relations, range_, allow_self=False, **extra):
    t = {'kind': 'unit', 'alive': True, 'visible': True, 'relations': relations, 'allowSelf': allow_self,
         'range': range_, 'outOfRange': 'reject'}
    t.update(extra)
    return t


def point_aim(relations, range_, radius, **extra):
    t = {'kind': 'point', 'radius': radius, 'alive': True, 'visible': True, 'relations': relations,
         'allowSelf': False, 'range': range_, 'outOfRange': 'reject'}
    t.update(extra)
    return t


def anims(prepare='cast', release='cast', recover='idle'):
    return {'prepare': prepare, 'release': release, 'recover': recover, 'fallback': 'idle'}


def fx(id_, effect, event, anchor='target', lifetime='finite', **extra):
    b = {'id': id_, 'effect': effect, 'event': event, 'anchor': anchor, 'lifetime': lifetime}
    b.update(extra)
    return b


def ability(id_, name, description, activation, ranks, targeting, cast_, on_release, icon, effects,
            animations=None, **extra):
    a = {'id': id_, 'name': name, 'description': description, 'activation': activation}
    for key in ('delivery', 'persistent', 'aura', 'autocast', 'combatModifiers'):
        if key in extra:
            a[key] = extra.pop(key)
    a['ranks'] = ranks
    a['targeting'] = targeting
    a['cast'] = cast_
    if 'statuses' in extra:
        a['statuses'] = extra.pop('statuses')
    a['onRelease'] = on_release
    if 'triggers' in extra:
        a['triggers'] = extra.pop('triggers')
    assert not extra, extra
    a['presentation'] = {'icon': icon, 'animations': animations or anims(), 'effects': effects}
    return a


def passive(id_, name, description, ranks, on_release, icon, effects, **extra):
    return ability(id_, name, description, 'passive', ranks, self_aim(), cast(0, 0, 0, 0), on_release, icon,
                   effects, animations=anims('idle', 'idle', 'idle'), **extra)


def binding(id_, ability_id, rank, hotkey, column, preference='enemy', interval=20, levels=None,
            threat=None, controls=('player', 'ai', 'scenario'), icon=None):
    b = {'id': id_, 'ability': ability_id, 'initialRank': rank, 'controls': list(controls)}
    if hotkey:
        b['command'] = {'hotkey': hotkey, 'column': column}
        if icon:
            b['command']['icon'] = icon
    if levels:
        b['learning'] = {'requiredLevels': levels}
    if 'ai' in controls:
        ai = {'preference': preference, 'intervalTicks': interval}
        if threat:
            ai['threatRadius'] = threat
        b['ai'] = ai
    return b


# ----- effects ----------------------------------------------------------------------------

VIOLET = '#8a5cff'
VIOLET_LIGHT = '#c9b8ff'
BLUE = '#5b7bff'
BLUE_LIGHT = '#b8c8ff'
FIRE = '#ff8a2a'
FIRE_LIGHT = '#ffd9a0'
SPARK = '#ffc65a'
SPARK_LIGHT = '#fff0b0'
STEAM = '#e8ecf2'
WHITE = '#ffffff'
SMOKE = '#8a8a90'

SPARK_TEX = {'asset': 'asset.effects.holy-light-spark', 'role': 'image', 'index': 1}
DUST_TEX = {'asset': 'asset.effects.construction-dust', 'role': 'image', 'index': 1}
SIGIL_TEX = {'asset': 'asset.effects.holy-light-sigil', 'role': 'image', 'index': 1}
BURST_TEX = {'asset': 'asset.effects.ants.resin-burst', 'role': 'image', 'index': 1}
CRESCENT_TEX = {'asset': 'asset.effects.seismic-crescent', 'role': 'image', 'index': 1}
FRACTURE_TEX = {'asset': 'asset.effects.marshal.crown-fracture', 'role': 'image', 'index': 1}
WHEEL_TEX = {'asset': 'asset.effects.marshal.cadence-wheel', 'role': 'image', 'index': 1}
SHELL_TEX = {'asset': 'asset.effects.marshal.carapace-shell', 'role': 'image', 'index': 1}
WARD_TEX = {'asset': 'asset.effects.marshal.chitin-ward', 'role': 'image', 'index': 1}
FLIPBOOK = {'columns': 4, 'rows': 4, 'firstFrame': 0, 'frames': 16, 'frameTicks': 2, 'mode': 'once',
            'randomStart': False}


def layer(id_, shape, colour, accent, duration, count, size, height, **extra):
    l = {'id': id_, 'enabled': True, 'shape': shape, 'colour': colour, 'accent': accent,
         'durationTicks': duration, 'count': count, 'size': size, 'height': height}
    l.update(extra)
    return l


def emitter(mode, lifetime, rate, speed, *, variation=0.4, direction=(0, 1, 0), cone=60, gravity=0.0,
            drag=0.5, spawn=0.2, start=0.3, end=0.05, spin=0, fade_in=0.0, fade_out=0.5):
    return {'mode': mode, 'lifetimeTicks': lifetime, 'rate': rate, 'speed': speed, 'speedVariation': variation,
            'direction': {'x': direction[0], 'y': direction[1], 'z': direction[2]}, 'coneDegrees': cone,
            'gravity': gravity, 'drag': drag, 'spawnRadius': spawn, 'startSize': start, 'endSize': end,
            'spin': spin, 'fadeIn': fade_in, 'fadeOut': fade_out}


def sparks(id_, colour=SPARK, accent=SPARK_LIGHT, count=18, speed=7.0, size=0.26, height=0.0, lifetime=16,
           gravity=14.0, cone=80, direction=(0, 1, 0), start=0, intensity=3.0):
    return layer(id_, 'particles', colour, accent, lifetime + 2, count, size, height, startTick=start,
                 blend='additive', intensity=intensity, texture=SPARK_TEX,
                 emitter=emitter('burst', lifetime, count, speed, variation=0.5, direction=direction,
                                 cone=cone, gravity=gravity, drag=0.4, spawn=0.15, start=size, end=size * 0.15))


def puffs(id_, colour=STEAM, accent=WHITE, count=6, speed=1.6, size=0.9, height=0.4, lifetime=30, start=0,
          rise=-1.5, spawn=0.3, cone=70, direction=(0, 1, 0), mode='burst', duration=None, blend='normal'):
    return layer(id_, 'particles', colour, accent, duration or lifetime + 4, count, size, height, startTick=start,
                 blend=blend, texture=DUST_TEX,
                 emitter=emitter(mode, lifetime, count, speed, variation=0.5, direction=direction, cone=cone,
                                 gravity=rise, drag=1.6, spawn=spawn, start=size * 0.5, end=size * 1.6,
                                 fade_in=0.1, fade_out=0.7))


def flash(id_, colour, accent, size, intensity, duration=6, height=1.0, start=0, **extra):
    l = layer(id_, 'light', colour, accent, duration, 1, size, height, intensity=intensity, startTick=start)
    l.update(extra)
    return l


def ring(id_, colour, accent, size, duration, height=0.06, **extra):
    l = layer(id_, 'ring', colour, accent, duration, 1, size, height, blend='additive')
    l.update(extra)
    return l


def glow(id_, colour, accent, size, duration, height=0.5, **extra):
    l = layer(id_, 'glow', colour, accent, duration, 1, size, height, blend='additive')
    l.update(extra)
    return l


def burst(id_, colour, accent, count, size, duration, height=1.0, **extra):
    l = layer(id_, 'burst', colour, accent, duration, count, size, height, blend='additive')
    l.update(extra)
    return l


def beam(id_, colour, accent, size, duration, height=1.2, **extra):
    l = layer(id_, 'beam', colour, accent, duration, 1, size, height, blend='additive')
    l.update(extra)
    return l


def explosion(id_, size, height=1.2, start=0, colour=WHITE, accent=WHITE, **extra):
    l = layer(id_, 'billboard', colour, accent, 32, 1, size, height, startTick=start, blend='normal',
              flipbook=FLIPBOOK, texture=BURST_TEX)
    l.update(extra)
    return l


def ground(id_, texture, colour, accent, size, duration, height=0.08, **extra):
    l = layer(id_, 'billboard', colour, accent, duration, 1, size, height, orientation='ground',
              blend='additive', texture=texture)
    l.update(extra)
    return l


def pulse(period, low, high, easing='sine'):
    return {'periodTicks': period, 'min': low, 'max': high, 'easing': easing}


def effect(id_, name, description, duration, layers, loop=False):
    return {'id': id_, 'name': name, 'description': description, 'durationTicks': duration, 'loop': loop,
            'layers': layers}
