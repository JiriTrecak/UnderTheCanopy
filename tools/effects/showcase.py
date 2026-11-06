"""Our showcase effects, written in the authoring nodes (D115, docs/effect-graph.md):
Pillar of Flame, Substance Nova, Violet Gate, Golden Mending, Firework, the fires and the
hero level-up. Each layer is a chain from its Spawner to its renderer; the recipes of skill
canopy-effect-design. Writes `<package>/effects/ours/*.json` (the lab while a look is
agreed; textures are `asset.effects.fx.*` from tools/effects/fx_textures.py); `--ship`
writes the agreed ones the game plays into the base package instead (SHIPPED).

    python3 -I tools/effects/showcase.py <package dir>
    python3 -I tools/effects/showcase.py base --ship
"""
import json
import os
import sys

FX = 'asset.effects.fx.'


def keys(*pairs):
    """A curve or gradient: (t, value) pairs, values numbers or lists."""
    return {'keys': [{'t': t, 'v': v if isinstance(v, list) else [v]} for t, v in pairs]}


def layer(id, name, *chain, values=()):
    """A layer: `chain` is (kind, settings[, id]) in flow order, wired out → in; `values` are
    (id, kind, settings, target node index, target setting) value nodes on the side."""
    nodes, wires, index = [], [], {}
    for i, step in enumerate(chain):
        kind, settings = step[0], step[1]
        if kind == 'life':
            # Life is the Spawner's setting.
            nodes[0]['inputs']['life'] = settings['seconds']
            index[i] = 0
            continue
        nid = step[2] if len(step) > 2 else '%s%d' % (kind, i)
        index[i] = len(nodes)
        nodes.append({'id': nid, 'kind': kind, 'at': [0.0, i * 150.0], 'inputs': dict(settings)})
        if len(nodes) > 1:
            wires.append({'from': '%s.out' % nodes[-2]['id'], 'to': '%s.in' % nid})
    for j, (vid, kind, settings, target, pin) in enumerate(values):
        nodes.append({'id': vid, 'kind': kind, 'at': [-300.0, target * 150.0], 'inputs': settings})
        wires.append({'from': '%s.value' % vid, 'to': '%s.%s' % (nodes[index[target]]['id'], 'life' if pin == 'seconds' else pin)})
    for n in nodes:
        if not n['inputs']:
            del n['inputs']
    return {'id': id, 'name': name, 'graph': {'nodes': nodes, 'wires': wires}}


def burst(count, time=0.0):
    return ('spawner', {'mode': 'burst', 'count': count, 'time': time})


def stream(rate, duration=0.0, delay=0.0):
    return ('spawner', {'mode': 'stream', 'rate': rate, 'duration': duration, 'delay': delay})


def life(lo, hi=None):
    return ('life', {'seconds': [lo, lo if hi is None else hi]})


def at(y, shape='point', **more):
    return ('shape', dict({'shape': shape, 'offset': [0.0, y, 0.0]}, **more))


def size(curve, lo=1.0, hi=1.0, stretch=0.0):
    s = {'size': [lo, hi], 'curve': keys(*curve)}
    if stretch:
        s['stretch'] = stretch
    return ('sizeOverLife', s)


def colour(gradient, lo=1.0, hi=1.0, shift=(0.0, 0.0, 0.0)):
    return ('colourOverLife', {'gradient': keys(*gradient), 'brightness': [lo, hi], 'shift': list(shift)})


def board(texture, **more):
    return ('billboard', dict({'texture': FX + texture}, **more))


def ground(texture, soft):
    return board(texture, facing='ground', soft=soft)


def spin(lo=0.0, hi=360.0, slo=0.0, shi=0.0):
    return ('spin', {'angle': [lo, hi], 'speed': [slo, shi]})


FLIP = ('flipbook', {})

# Palettes (linear RGBA keys over life).
HOT = [(0, [0, 0, 0, 0]), (0.12, [1.6, 0.75, 0.3, 1]), (0.45, [1.2, 0.32, 0.05, 0.8]), (1, [0.35, 0.03, 0, 0])]
HOT_FAST = [(0, [0, 0, 0, 0]), (0.06, [2.0, 1.2, 0.6, 1]), (0.3, [1.4, 0.45, 0.08, 0.8]), (1, [0.3, 0.02, 0, 0])]
EMBER = [(0, [0, 0, 0, 0]), (0.1, [1.6, 0.9, 0.35, 1]), (0.6, [1.1, 0.3, 0.03, 1]), (1, [0.4, 0.04, 0, 0])]
VIOLET = [(0, [0, 0, 0, 0]), (0.15, [0.75, 0.5, 1.4, 1]), (0.5, [0.45, 0.22, 1.0, 0.8]), (1, [0.18, 0.04, 0.45, 0])]
VIOLET_FAST = [(0, [0, 0, 0, 0]), (0.08, [1.2, 1.0, 1.6, 1]), (0.35, [0.55, 0.3, 1.2, 0.8]), (1, [0.15, 0.03, 0.4, 0])]
VIOLET_LATE = [(0, [0, 0, 0, 0]), (0.7, [0.7, 0.45, 1.4, 1]), (1, [0.2, 0.05, 0.5, 0])]
GOLD = [(0, [0, 0, 0, 0]), (0.2, [1.4, 1.05, 0.45, 1]), (0.55, [1.0, 0.55, 0.12, 0.8]), (1, [0.4, 0.1, 0.0, 0])]
TWINKLE = [(0, 0.0), (0.2, 1.0), (0.45, 0.4), (0.7, 1.0), (1, 0.0)]


def pillar_of_flame():
    """A burst of fire that erupts and burns down (~2 s)."""
    return {'id': 'effect.ours.pillar-of-flame', 'name': 'Pillar of Flame', 'durationTicks': 100, 'loop': False}, [
        layer('flash', 'Flash', burst(1), life(0.4), at(2.0), size([(0, 2.0), (0.15, 16.0), (1, 9.0)]),
              colour(HOT_FAST, shift=(0, 0.1, 2.5)), spin(-20, 20), board('burst-flash-streak')),
        layer('shock', 'Shock ring', burst(1), life(0.5), at(0.4), size([(0, 3.0), (0.4, 15.0), (1, 18.0)]),
              colour(HOT_FAST, shift=(0, 0.2, 2.0)), spin(), ground('ring-shock', 0.8)),
        layer('scorch', 'Burning ground', burst(2), life(1.8, 2.2), at(0.3), size([(0, 1.0), (1, 1.0)], 10, 12),
              colour([(0, [0, 0, 0, 0]), (0.08, [1.6, 0.55, 0.1, 1]), (0.5, [0.9, 0.2, 0.02, 1]), (1, [0, 0, 0, 0])],
                     shift=(0, 0.2, 0.6)), spin(), ground('crack-web', 0.6)),
        layer('glow', 'Ground glow', stream(6, 1.4), life(0.5, 0.7), at(0.5), size([(0, 1.0), (1, 1.0)], 10, 13),
              colour([(0, [0, 0, 0, 0]), (0.3, [0.9, 0.35, 0.06, 1]), (1, [0, 0, 0, 0])], 0.5, 1.0),
              ground('soft-disc', 0.5)),
        layer('body', 'Dark body', stream(18, 1.1), life(0.9, 1.4), ('shape', {'shape': 'cylinder', 'radius': 2.6, 'height': 1.0}),
              ('launch', {'direction': 'cone', 'angle': 20, 'speed': [5, 11]}),
              ('physics', {'gravity': [0, 4, 0], 'drag': 1.2}), size([(0, 3.0), (1, 8.0)], 0.8, 1.2),
              colour([(0, [0.5, 0.18, 0.06, 0]), (0.15, [0.35, 0.1, 0.04, 0.75]), (1, [0.06, 0.04, 0.04, 0])]),
              spin(0, 360, -40, 40), FLIP, board('smoke', blend='alpha', soft=1.0)),
        layer('flames', 'Flames', stream(60, 1.2), life(0.6, 1.0), ('shape', {'shape': 'cylinder', 'radius': 2.4, 'height': 0.6}),
              ('launch', {'direction': 'cone', 'angle': 20, 'speed': [7, 16]}),
              ('physics', {'gravity': [0, 7, 0], 'drag': 1.6}), size([(0, 2.0), (0.4, 4.5), (1, 3.0)], 0.7, 1.3),
              colour(HOT, 0.35, 0.8, (0, 0.3, 0.2)), spin(-15, 15), FLIP, board('flame', soft=1.0)),
        layer('core', 'Core flames', stream(20, 0.9), life(0.5, 0.8), ('shape', {'shape': 'cylinder', 'radius': 1.0, 'height': 0.5}),
              ('launch', {'direction': 'cone', 'angle': 5, 'speed': [14, 24]}),
              ('physics', {'drag': 1.8}), size([(0, 2.5), (0.3, 5.5), (1, 3.0)], 0.9, 1.3),
              colour(HOT_FAST, 0.5, 0.9, (0, 0.2, 0.6)), FLIP, board('flame', soft=1.0)),
        layer('sparks', 'Sparks', burst(45), life(0.35, 0.8), at(1.5, 'sphere', radius=0.6, fill='surface'),
              ('launch', {'direction': 'outward', 'speed': [16, 22]}),
              ('physics', {'gravity': [0, -14, 0], 'drag': 2.2}), size([(0, 0.2), (0.06, 0.6), (1, 0.0)], stretch=0.055),
              colour(HOT_FAST, 1.0, 5.0, (0, 0.2, 3.0)), board('pin-dot', facing='velocity')),
        layer('embers', 'Embers', stream(55, 1.6), life(0.9, 1.7),
              ('shape', {'shape': 'cylinder', 'radius': 3.0, 'height': 2.0, 'inner': 0.2}),
              ('launch', {'direction': 'cone', 'angle': 25, 'speed': [6, 16]}), ('physics', {'drag': 0.7}),
              ('orbit', {'speed': [-90, 90]}), size([(0, 0.2), (0.15, 0.7), (0.5, 0.4), (0.7, 0.7), (1, 0.0)], 0.6, 1.4),
              colour(EMBER, 1.0, 6.0, (0, 0.2, 2.0)), spin(0, 360, -300, 300),
              ('flipbook', {'columns': 4, 'rows': 2, 'last': 7, 'play': 'random'}), board('embers')),
        layer('smoke', 'Smoke', stream(10, 1.4, 0.3), life(2.0, 2.8),
              ('shape', {'shape': 'cylinder', 'radius': 2.0, 'height': 1.0, 'offset': [0, 4, 0]}),
              ('launch', {'direction': 'cone', 'angle': 20, 'speed': [5, 9]}), ('physics', {'drag': 0.4}),
              size([(0, 4.0), (1, 10.0)], 0.8, 1.3), spin(0, 360, -30, 30),
              ('flipbook', {'first': 18, 'last': 63}),
              colour([(0, [0.35, 0.16, 0.08, 0]), (0.18, [0.12, 0.1, 0.1, 0.55]), (1, [0.08, 0.08, 0.08, 0])]),
              board('smoke', blend='alpha', soft=2.0)),
    ]


def substance_nova():
    """A violet slam that tears outward (~1.4 s)."""
    return {'id': 'effect.ours.substance-nova', 'name': 'Substance Nova', 'durationTicks': 64, 'loop': False}, [
        layer('flash', 'Flash', burst(1), life(0.3), at(1.5), size([(0, 3.0), (0.12, 14.0), (1, 6.0)]),
              colour(VIOLET_FAST, shift=(0, 0.1, 3.0)), spin(), board('burst-flash')),
        layer('shock', 'Shock ring', burst(1), life(0.45), at(0.5), size([(0, 2.0), (0.5, 22.0), (1, 26.0)]),
              colour(VIOLET_FAST, shift=(0, 0.15, 3.0)), spin(), ground('ring-hard', 0.6)),
        layer('smear', 'Smear rings', stream(26, 0.4), life(0.32), at(0.45), size([(0, 3.0), (1, 22.0)]),
              colour(VIOLET, 0.5, 1.0, (0, 0.1, 0.8)), spin(), ground('ring-shock', 0.6)),
        layer('cracks', 'Cracks', burst(1), life(1.6), at(0.3), size([(0, 17.0), (1, 17.0)]),
              colour([(0, [0, 0, 0, 0]), (0.06, [1.0, 0.8, 1.6, 1]), (0.3, [0.5, 0.25, 1.1, 1]), (1, [0, 0, 0, 0])],
                     shift=(0, 0.1, 1.2)), spin(), ground('crack-web', 0.5)),
        layer('glow', 'Ground glow', burst(2), life(0.6, 0.8), at(0.5), size([(0, 8.0), (1, 20.0)]),
              colour(VIOLET, 0.6, 0.9), ground('soft-disc', 0.5)),
        layer('sparks', 'Sparks', burst(60), life(0.45, 0.9),
              ('shape', {'shape': 'ring', 'radius': 2.0, 'height': 0.3, 'offset': [0, 0.6, 0]}),
              ('launch', {'direction': 'outward', 'speed': [16, 28]}), ('physics', {'gravity': [0, 6, 0], 'drag': 2.8}),
              size([(0, 0.3), (0.07, 0.8), (1, 0.0)], stretch=0.07), colour(VIOLET_FAST, 1.0, 5.0, (0, 0.25, 3.5)),
              board('pin-dot', facing='velocity')),
        layer('shards', 'Glints', stream(70, 0.35), life(0.8, 1.6),
              ('shape', {'shape': 'cylinder', 'radius': 6.0, 'height': 0.5, 'inner': 0.3}),
              ('launch', {'direction': 'cone', 'angle': 30, 'speed': [3, 9]}), ('physics', {'drag': 1.5}),
              ('orbit', {'speed': [-70, 70]}), size(TWINKLE, 0.5, 1.9), colour(VIOLET, 1.0, 4.0, (0, 0.2, 1.5)),
              spin(), board('star-glint')),
        layer('dust', 'Dust', burst(14), life(0.7, 1.1), ('shape', {'shape': 'ring', 'radius': 3.0, 'height': 0.3}),
              ('launch', {'direction': 'outward', 'speed': [12, 21]}), ('physics', {'gravity': [0, 2, 0], 'drag': 2.5}),
              size([(0, 2.0), (1, 6.0)]), spin(), FLIP,
              colour([(0, [0.3, 0.25, 0.35, 0]), (0.2, [0.22, 0.18, 0.26, 0.6]), (1, [0.15, 0.13, 0.18, 0])]),
              board('smoke', blend='alpha', soft=1.0)),
    ]


def violet_gate():
    """A ground portal that opens, holds and breathes (~4 s)."""
    intro = [(0, 0.0), (0.12, 1.15), (0.2, 1.0), (0.9, 1.0), (1, 0.0)]
    hold = lambda c: [(0, [0, 0, 0, 0]), (0.1, c), (0.9, c), (1, [0, 0, 0, 0])]  # noqa: E731
    return {'id': 'effect.ours.violet-gate', 'name': 'Violet Gate', 'durationTicks': 160, 'loop': False}, [
        layer('disc', 'Sigil disc', burst(1), life(4.0), at(0.35), size(intro, 11, 11),
              colour(hold([0.6, 0.35, 1.3, 1]), shift=(0, 0.35, 0.2)), spin(0, 0, 30, 30), ground('swirl', 0.4)),
        layer('counter', 'Counter disc', burst(1), life(4.0), at(0.45), size(intro, 8, 8),
              colour(hold([0.6, 0.35, 1.3, 1]), shift=(0, 0.3, 0.4)), spin(0, 0, -60, -60), ground('swirl-blur', 0.4)),
        layer('rim', 'Rim', burst(1), life(4.0), at(0.4), size(intro, 12.5, 12.5),
              colour([(0, [0, 0, 0, 0]), (0.08, [1.2, 0.9, 1.8, 1]), (0.25, [0.6, 0.4, 1.3, 1]), (0.9, [0.6, 0.4, 1.3, 1]),
                      (1, [0, 0, 0, 0])], shift=(0, 0.15, 1.0)), ground('ring-thick', 0.4)),
        layer('heart', 'Imploding rings', stream(5, 3.2, 0.3), life(1.1, 1.4), at(0.6), size([(0, 9.0), (1, 0.0)]),
              colour(VIOLET_LATE, 1.0, 2.0, (0, 0.2, 2.5)), spin(0, 360, 120, 280), ground('ring-wispy', 0.4)),
        layer('curtain', 'Rising curtain', stream(34, 3.2, 0.3), life(1.1, 1.5),
              ('shape', {'shape': 'ring', 'radius': 5.6, 'height': 0.2}),
              ('launch', {'direction': 'up', 'speed': [4, 7]}), ('physics', {'gravity': [0, 4, 0]}),
              size([(0, 0.0), (0.2, 0.7), (1, 0.0)], stretch=0.45), colour(VIOLET, 0.6, 1.6, (0, 0.2, 1.4)),
              board('soft-beam-thin', facing='velocity')),
        layer('dust', 'Drifting motes', stream(50, 3.4, 0.2), life(1.4, 2.4),
              ('shape', {'shape': 'cylinder', 'radius': 5.5, 'height': 0.5}),
              ('launch', {'direction': 'up', 'speed': [1, 3]}), ('physics', {'drag': 0.6}), ('orbit', {'speed': [23, 80]}),
              size([(0, 0.0), (0.15, 0.6), (1, 0.0)], 0.3, 1.5), colour(VIOLET, 0.5, 4.0, (0, 0.2, 2.0)), board('pin-dot')),
        layer('pulse', 'Breathing glow', stream(1.2, 3.6), life(2.0), at(0.3), size([(0, 10.0), (1, 15.0)]),
              colour([(0, [0, 0, 0, 0]), (0.4, [0.3, 0.15, 0.7, 1]), (1, [0, 0, 0, 0])]), ground('soft-disc', 0.5)),
        layer('opening', 'Opening flash', burst(1), life(0.5), at(1.0), size([(0, 2.0), (0.15, 12.0), (1, 0.0)]),
              colour(VIOLET_FAST, shift=(0, 0.1, 3.0)), spin(), board('star-glint')),
    ]


def golden_mending():
    """A heal that lifts off the target (~1.5 s)."""
    return {'id': 'effect.ours.golden-mending', 'name': 'Golden Mending', 'durationTicks': 60, 'loop': False}, [
        layer('core', 'Core glow', burst(1), life(1.0), at(2.2), size([(0, 0.0), (0.25, 9.0), (1, 5.0)]),
              colour(GOLD, shift=(0, 0.15, 1.6)), board('soft-dot-wide')),
        layer('ring', 'Ground ring', stream(5, 0.6), life(0.6), at(0.4), size([(0, 2.0), (1, 9.0)]),
              colour(GOLD, shift=(0, 0.25, 2.5)), ground('ring-thick', 0.4)),
        layer('rise', 'Rising light', stream(22, 0.6), life(0.6, 0.9),
              ('shape', {'shape': 'cylinder', 'radius': 1.4, 'height': 0.3}),
              ('launch', {'direction': 'up', 'speed': [1, 3]}), ('physics', {'gravity': [0, 9, 0]}),
              size([(0, 0.8), (0.3, 2.4), (1, 0.6)], stretch=0.4), colour(GOLD, 1.2, 2.2, (0, 0.15, 1.8)),
              board('soft-beam', facing='velocity')),
        layer('glints', 'Spiral glints', stream(40, 0.6), life(0.6, 1.1),
              ('shape', {'shape': 'cylinder', 'radius': 1.8, 'height': 2.5, 'inner': 0.5}),
              ('launch', {'direction': 'up', 'speed': [1, 4]}), ('physics', {'gravity': [0, 8, 0]}),
              ('orbit', {'speed': [143, 286]}),
              size([(0, 0.0), (0.2, 1.6), (0.4, 0.7), (0.6, 1.6), (0.8, 0.7), (1, 0.0)], 0.5, 2.0),
              colour(GOLD, 1.5, 4.0, (0, 0.2, 2.5)), spin(), board('star-glint')),
        layer('motes', 'Motes', stream(24, 0.8), life(1.0, 1.6), at(2.0, 'sphere', radius=2.0, inner=0.6),
              ('launch', {'direction': 'outward', 'speed': [1, 3]}), ('physics', {'gravity': [0, 3, 0], 'drag': 1.0}),
              size([(0, 0.0), (0.2, 1.0), (1, 0.0)], 0.5, 1.5), colour(GOLD, 1.0, 4.0, (0, 0.15, 2.0)), board('pin-dot')),
    ]


def firework():
    """Rockets that burst into sparks as they die, leave puffs as they climb, and whose
    sparks crackle into tiny flashes when they go out (events between layers)."""
    sparkle = [(0, [0, 0, 0, 0]), (0.05, [2.0, 1.2, 0.5, 1]), (0.4, [1.4, 0.5, 0.1, 1]), (1, [0.3, 0.05, 0.0, 0])]
    return {'id': 'effect.ours.firework', 'name': 'Firework', 'durationTicks': 120, 'loop': False}, [
        layer('rocket', 'Rockets', stream(5, 1.2), life(0.9, 1.2), ('shape', {'shape': 'disc', 'radius': 1.0}),
              ('launch', {'direction': 'cone', 'angle': 20, 'speed': [11, 14]}), ('physics', {'gravity': [0, -8, 0]}),
              ('emit', {'when': 'death', 'count': [60, 60]}, 'pop'), ('emit', {'when': 'interval', 'interval': 0.025}, 'puff'),
              size([(0, 1.2), (1, 1.2)]), colour(HOT_FAST, shift=(0, 0.1, 2.0)), board('pin-dot')),
        layer('trail', 'Trail', ('spawner', {'mode': 'event', 'layer': 'rocket', 'event': 'puff'}), life(0.4, 0.7),
              ('launch', {'direction': 'random', 'speed': [0.0, 0.6]}), size([(0, 0.5), (1, 1.6)]),
              colour([(0, [0, 0, 0, 0]), (0.05, [1.2, 0.6, 0.25, 1]), (1, [0.1, 0.03, 0.01, 0])], 0.4, 0.8),
              board('soft-dot')),
        layer('sparks', 'Burst', ('spawner', {'mode': 'event', 'layer': 'rocket', 'event': 'pop'}), life(0.8, 1.4),
              ('launch', {'direction': 'random', 'speed': [9, 15]}), ('physics', {'gravity': [0, -6, 0], 'drag': 1.6}),
              size([(0, 0.5), (1, 0.15)], stretch=0.06), colour(sparkle, 1.0, 1.0, (0, 0.2, 1.5)),
              ('emit', {'when': 'death', 'chance': 0.3}, 'fizzle'), board('pin-dot', facing='velocity')),
        layer('crackle', 'Crackle', ('spawner', {'mode': 'event', 'layer': 'sparks', 'event': 'fizzle'}), life(0.08, 0.16),
              size([(0, 1.0), (1, 1.0)], 0.8, 1.6), colour([(0, [3.0, 2.6, 2.2, 1]), (1, [3.0, 2.6, 2.2, 1])]), spin(),
              board('star-glint')),
    ]


# Fire (tools/effects/fire_textures.py): whole flames come from the simulated `fire-loop`
# flipbook, its temperature colour baked in, so layers tint it only near white. Its
# flame rises from 0.12 of the frame above its lower edge: a sprite's centre sits 0.38 of
# its size above the ground it burns on.
FIRE_IN_OUT = [(0, [1, 0.95, 0.9, 0]), (0.25, [1, 0.95, 0.9, 1]), (0.75, [1, 0.92, 0.85, 1]), (1, [1, 0.85, 0.75, 0])]
LOOP = ('flipbook', {'play': 'rate', 'fps': 30, 'start': 'random'})
SMOKE_DARK = [(0, [0.3, 0.2, 0.14, 0]), (0.2, [0.13, 0.11, 0.1, 0.42]), (0.6, [0.12, 0.11, 0.11, 0.26]), (1, [0.12, 0.12, 0.12, 0])]
FLICKER = [(0, 0.0), (0.15, 1.0), (0.4, 0.75), (0.65, 1.0), (1, 0.0)]


def light(id, colour, reach, height, intensity):
    """A flickering point light: two quick pulses out of step (legacy light layer)."""
    return {'id': id, 'enabled': True, 'shape': 'light', 'sustain': True, 'colour': colour, 'accent': colour,
            'durationTicks': 40, 'count': 1, 'size': reach, 'height': height, 'intensity': intensity,
            'motion': {'opacity': {'periodTicks': 5, 'min': 0.72, 'max': 1.0, 'easing': 'sine'},
                       'scale': {'periodTicks': 9, 'min': 0.9, 'max': 1.0, 'easing': 'sine', 'phase': 0.3}}}


def torch():
    """A torch's flame: a tall flickering tongue, a white-hot heart, sparks and a thread of
    smoke, lighting what stands near. Plays from the emitter (a torch's head), looping."""
    return {'id': 'effect.ours.torch', 'name': 'Torch', 'durationTicks': 80, 'loop': True,
            'layers': [light('light', '#ff9a48', 11.0, 1.2, 9.0)]}, [
        layer('halo', 'Halo', stream(14), life(0.12, 0.3), at(1.2), size([(0, 1.0), (1, 1.0)], 5.0, 6.5),
              colour([(0, [0.9, 0.38, 0.08, 0]), (0.3, [0.9, 0.38, 0.08, 0.3]), (1, [0.9, 0.38, 0.08, 0])], 0.6, 1.0),
              board('soft-dot-wide')),
        layer('body', 'Flame', stream(4.5), life(1.1, 1.5), at(1.45, 'sphere', radius=0.08),
              size([(0, 0.85), (0.3, 1.0), (1, 0.92)], 3.6, 4.2),
              colour(FIRE_IN_OUT, 0.85, 1.1, (0, 0.08, 0.9)), spin(-6, 6), LOOP, board('fire-loop', soft=0.3)),
        layer('tongues', 'Tongues', stream(5), life(0.45, 0.75), at(0.7, 'sphere', radius=0.18),
              ('launch', {'direction': 'up', 'speed': [0.8, 1.8]}), ('turbulence', {'strength': 3, 'scale': 0.8, 'speed': 2}),
              size([(0, 0.6), (0.3, 1.0), (1, 0.5)], 1.4, 2.1),
              colour(FIRE_IN_OUT, 0.7, 1.1, (0, 0.08, 0.8)), spin(-12, 12), LOOP, board('fire-loop', soft=0.2)),
        layer('heart', 'Heart', stream(18), life(0.1, 0.22), at(0.35, 'sphere', radius=0.1),
              size([(0, 0.8), (0.3, 1.0), (1, 0.7)], 1.0, 1.4),
              colour([(0, [1.0, 0.75, 0.4, 0]), (0.3, [1.0, 0.8, 0.45, 0.7]), (1, [1.0, 0.6, 0.2, 0])], 1.2, 2.0, (0, 0.1, 1.0)),
              board('soft-dot-tight')),
        layer('sparks', 'Sparks', stream(7), life(0.7, 1.6), at(0.9, 'sphere', radius=0.3),
              ('launch', {'direction': 'cone', 'angle': 25, 'speed': [1.2, 3.2]}), ('physics', {'gravity': [0, 1.5, 0], 'drag': 0.6}),
              ('turbulence', {'strength': 6, 'scale': 0.7, 'speed': 1.5}),
              size([(0, 0.0), (0.1, 1.0), (0.6, 0.6), (1, 0.0)], 0.09, 0.18, stretch=0.03),
              colour(EMBER, 1.0, 4.0, (0, 0.2, 1.8)), board('pin-dot', facing='velocity')),
        layer('smoke', 'Smoke', stream(2.5), life(2.2, 3.0), at(3.2, 'sphere', radius=0.2),
              ('launch', {'direction': 'cone', 'angle': 10, 'speed': [0.9, 1.5]}), ('physics', {'drag': 0.3}),
              ('turbulence', {'strength': 1.2, 'scale': 1.5, 'speed': 0.6}),
              size([(0, 0.5), (1, 1.0)], 2.0, 3.2), spin(0, 360, -20, 20),
              ('flipbook', {'first': 14, 'last': 63}), colour(SMOKE_DARK), board('fire-smoke', blend='alpha', soft=0.6)),
    ]


def ground_fire():
    """A patch of burning ground about 5 m across: flames of every size licking up from a
    glowing bed, sparks, a tall smoke plume and a scorched, lit floor. Looping."""
    bed = [(0, [1.0, 0.35, 0.06, 0]), (0.3, [1.0, 0.35, 0.06, 1]), (0.7, [0.9, 0.28, 0.04, 1]), (1, [1.0, 0.35, 0.06, 0])]
    return {'id': 'effect.ours.ground-fire', 'name': 'Ground fire', 'durationTicks': 120, 'loop': True,
            'layers': [light('light', '#ff8a3a', 17.0, 2.0, 16.0)]}, [
        layer('char', 'Scorch', ('spawner', {'mode': 'burst', 'count': 1, 'ends': 'effect'}), at(0.04), size([(0, 1.0), (1, 1.0)], 7.5, 8.0),
              colour([(0, [1, 1, 1, 0.9]), (1, [1, 1, 1, 0.9])]),
              spin(), board('ground-scorch', facing='ground', blend='alpha', soft=0.2)),
        layer('glow', 'Ground glow', stream(10), life(0.25, 0.5), at(0.3, 'disc', radius=0.6),
              size([(0, 1.0), (1, 1.0)], 6.0, 7.5), colour(bed, 0.12, 0.2, (0, 0.1, 0.3)), spin(),
              ground('soft-disc', 0.2)),
        layer('coals', 'Coals', stream(45), life(0.6, 1.4), at(0.1, 'disc', radius=2.4),
              size(FLICKER, 0.2, 0.5), colour(EMBER, 0.6, 2.5, (0, 0.2, 1.2)), board('soft-dot-tight')),
        layer('flames', 'Flames', stream(9), life(1.0, 1.6), at(1.35, 'disc', radius=1.9),
              size([(0, 0.7), (0.3, 1.0), (1, 0.85)], 3.0, 4.4),
              colour(FIRE_IN_OUT, 0.7, 1.0, (0, 0.06, 0.7)), spin(-8, 8), LOOP, board('fire-loop', soft=0.4)),
        layer('core', 'Core flames', stream(3.5), life(1.2, 1.8), at(2.2, 'disc', radius=0.7),
              size([(0, 0.8), (0.3, 1.0), (1, 0.9)], 5.5, 6.5),
              colour(FIRE_IN_OUT, 0.7, 0.95, (0, 0.06, 0.7)), spin(-5, 5), LOOP, board('fire-loop', soft=0.5)),
        layer('sparks', 'Sparks', stream(22), life(0.8, 2.0), at(0.8, 'disc', radius=1.8),
              ('launch', {'direction': 'cone', 'angle': 25, 'speed': [2, 5]}), ('physics', {'gravity': [0, 1.5, 0], 'drag': 0.5}),
              ('turbulence', {'strength': 8, 'scale': 1.2, 'speed': 1.2}),
              size([(0, 0.0), (0.1, 1.0), (0.6, 0.6), (1, 0.0)], 0.08, 0.18, stretch=0.03),
              colour(EMBER, 1.0, 5.0, (0, 0.2, 2.0)), board('pin-dot', facing='velocity')),
        layer('smoke', 'Smoke', stream(6), life(3.0, 4.2), at(5.0, 'disc', radius=1.2),
              ('launch', {'direction': 'cone', 'angle': 12, 'speed': [1.6, 2.6]}), ('physics', {'drag': 0.2, 'wind': [0.6, 0, 0.2]}),
              ('turbulence', {'strength': 1.5, 'scale': 3.0, 'speed': 0.5}),
              size([(0, 0.45), (1, 1.0)], 3.5, 5.5), spin(0, 360, -15, 15),
              ('flipbook', {'first': 14, 'last': 63}), colour(SMOKE_DARK), board('fire-smoke', blend='alpha', soft=1.2)),
    ]


# Level up: warm gold, rising. A breath in (a ground ring and glints drawn to the hero),
# then the release at RELEASE seconds: a flash, shock rings, a flower of light blooming
# under the hero, a column of streaks, glints spiralling up, rings climbing the body and
# pollen motes that linger. Ground layers follow the hero as it walks.
RELEASE = 0.3
GOLD_HOT = [(0, [0, 0, 0, 0]), (0.08, [2.2, 1.7, 0.9, 1]), (0.35, [1.6, 0.95, 0.25, 0.85]), (1, [0.6, 0.15, 0.0, 0])]
GOLD_LATE = [(0, [0, 0, 0, 0]), (0.75, [1.5, 1.1, 0.45, 1]), (1, [2.0, 1.6, 0.8, 0])]
GOLD_GLINT = [(0, [0, 0, 0, 0]), (0.12, [2.4, 1.75, 0.6, 1]), (0.5, [1.8, 0.9, 0.12, 0.85]), (0.82, [1.1, 0.25, 0.0, 0.6]), (1, [0.8, 0.05, 0.0, 0])]
# White-hot at the peak (it burns to white through the bloom), cooling to gold and amber.
WHITE_HOT = [(0, [0, 0, 0, 0]), (0.06, [3.0, 2.8, 2.4, 1]), (0.3, [2.4, 1.8, 0.9, 0.9]), (0.7, [1.5, 0.8, 0.2, 0.6]), (1, [0.6, 0.2, 0.0, 0])]
GOLD_MIDPEAK = [(0, [0, 0, 0, 0]), (0.4, [1.5, 1.05, 0.4, 1]), (1, [0.5, 0.12, 0.0, 0])]


def level_up():
    """A hero gains a level (~2 s)."""
    r = RELEASE
    after = lambda rate, duration: stream(rate, duration, r)  # noqa: E731
    flash = {'id': 'light', 'enabled': True, 'startTick': int(r * 40), 'shape': 'light', 'colour': '#ffc35a',
             'accent': '#fff0c8', 'durationTicks': 36, 'count': 1, 'size': 18.0, 'height': 3.0, 'intensity': 30.0}
    follow = {'follow': 'emitter'}
    return {'id': 'effect.ours.level-up', 'name': 'Level up', 'durationTicks': 84, 'loop': False,
            'layers': [flash]}, [
        layer('gather', 'Gathering ring', burst(1), life(r + 0.02), at(0.4, **follow),
              size([(0, 1.0), (0.6, 0.5), (1, 0.15)], 11.0, 11.0), colour(GOLD_LATE, 1.3, 1.3, (0, 0.2, 2.5)),
              spin(), ground('ring-thick', 0.4)),
        layer('drawn', 'Drawn glints', burst(40), life(r - 0.04, r), at(2.6, 'sphere', radius=5.0, inner=0.6),
              ('launch', {'direction': 'outward', 'speed': [-16, -12]}),
              size([(0, 0.5), (0.6, 1.0), (1, 0.6)], 0.6, 1.5, stretch=0.02), colour(GOLD_LATE, 1.5, 3.5, (0, 0.2, 2.5)),
              spin(), board('star-glint')),
        layer('flash', 'Flash', burst(1, r), life(0.25), at(3.0), size([(0, 0.4), (0.15, 1.0), (1, 0.6)], 15.0, 15.0),
              colour(WHITE_HOT, 0.6, 0.6, (0, 0.0, 1.2)), board('soft-dot-wide')),
        layer('rays', 'Rays', burst(1, r), life(0.3), at(3.0), size([(0, 0.3), (0.15, 1.0), (1, 0.7)], 20.0, 20.0),
              colour(GOLD_HOT, shift=(0, 0.1, 3.0)), spin(), board('burst-flash')),
        layer('shock', 'Shock wave', burst(1, r), life(0.45), at(0.4, **follow),
              size([(0, 0.1), (0.3, 0.75), (1, 1.0)], 14.0, 14.0), colour(GOLD_HOT, 1.0, 1.0, (0, 0.25, 3.5)),
              spin(), ground('ring-clean', 0.4)),
        layer('wisps', 'Shock wisps', after(8, 0.25), life(0.4, 0.5), at(0.5, **follow),
              size([(0, 0.15), (0.3, 0.7), (1, 0.95)], 11.0, 13.0), colour(GOLD_HOT, 0.45, 0.7, (0, 0.25, 2.0)),
              spin(), ground('ring-wispy', 0.4)),
        layer('pool', 'Ground glow', burst(2, r), life(1.0, 1.2), at(0.3, **follow),
              size([(0, 0.3), (0.3, 1.0), (1, 1.1)], 12.0, 13.0), colour(GOLD_HOT, 0.25, 0.35, (0, 0.1, 0.5)),
              ground('soft-disc', 0.3)),
        layer('bloom', 'Bloom', burst(1, r), life(1.6), at(0.12, **follow),
              size([(0, 0.25), (0.15, 1.0), (1, 1.1)], 8.0, 8.0),
              colour([(0, [0, 0, 0, 0]), (0.08, [1.6, 1.15, 0.5, 1]), (0.55, [1.1, 0.65, 0.15, 0.8]), (1, [0.4, 0.08, 0.0, 0])], shift=(0, 0.15, 1.6)),
              spin(0, 360, 25, 25), ground('bloom-sigil', 0.3)),
        layer('column', 'Column', after(110, 0.5), life(0.35, 0.55),
              ('shape', {'shape': 'cylinder', 'radius': 0.9, 'height': 0.3, 'offset': [0, 0.3, 0]}),
              ('launch', {'direction': 'up', 'speed': [35, 55]}), ('physics', {'gravity': [0, 20, 0]}),
              size([(0, 1.0), (1, 0.5)], 0.8, 1.6, stretch=0.1), colour(WHITE_HOT, 0.5, 1.6, (0, 0.0, 2.0)),
              board('soft-beam-thin', facing='velocity')),
        layer('core', 'White core', after(45, 0.45), life(0.3, 0.45),
              ('shape', {'shape': 'cylinder', 'radius': 0.35, 'height': 0.3, 'offset': [0, 0.3, 0]}),
              ('launch', {'direction': 'up', 'speed': [45, 60]}), ('physics', {'gravity': [0, 20, 0]}),
              size([(0, 1.0), (1, 0.6)], 0.5, 0.9, stretch=0.12), colour(WHITE_HOT, 1.5, 2.5, (0, -0.1, 2.5)),
              board('soft-beam-thin', facing='velocity')),
        layer('veil', 'Veil of light', after(60, 0.55), life(0.4, 0.6),
              ('shape', {'shape': 'cylinder', 'radius': 1.8, 'inner': 0.4, 'height': 8.0, 'offset': [0, 4.5, 0]}),
              ('launch', {'direction': 'up', 'speed': [3, 3.2]}),
              size([(0, 0.6), (0.4, 1.0), (1, 0.8)], 1.6, 4.5, stretch=2.4),
              colour([(0, [0, 0, 0, 0]), (0.3, [0.75, 0.42, 0.1, 0.4]), (0.45, [0.8, 0.45, 0.1, 0.45]), (1, [0.45, 0.1, 0.0, 0])], 0.6, 1.4),
              board('soft-beam', facing='velocity', soft=3.0)),
        layer('streaks', 'Outer streaks', after(55, 0.6), life(0.4, 0.65),
              ('shape', {'shape': 'cylinder', 'radius': 2.3, 'inner': 0.6, 'height': 1.2, 'offset': [0, 0.4, 0]}),
              ('launch', {'direction': 'up', 'speed': [9, 18]}), ('physics', {'gravity': [0, 8, 0]}),
              size([(0, 0.6), (0.4, 1.0), (1, 0.4)], 0.7, 1.5, stretch=0.12), colour(GOLD_MIDPEAK, 0.5, 1.5, (0, 0.2, 2.0)),
              board('soft-beam', facing='velocity')),
        layer('glints', 'Spiral glints', stream(110, 0.7, r - 0.05), life(0.5, 1.0),
              ('shape', {'shape': 'cylinder', 'radius': 2.5, 'inner': 0.3, 'height': 3.0, 'offset': [0, 0.8, 0]}),
              ('launch', {'direction': 'cone', 'angle': 40, 'speed': [4, 18]}), ('orbit', {'speed': [60, 280]}),
              ('physics', {'gravity': [0, 6, 0], 'drag': 0.5}),
              size([(0, 0.6), (0.15, 0.85), (0.3, 1.0), (0.6, 0.7), (1, 0.0)], 0.3, 1.4),
              colour(GOLD_GLINT, 1.0, 3.0, (0, 0.2, 2.0)), spin(), board('star-glint')),
        layer('rings', 'Climbing rings', after(20, 0.55), life(0.6, 0.8), at(0.5),
              ('launch', {'direction': 'up', 'speed': [6, 16]}), ('physics', {'drag': 1.2}),
              size([(0, 1.0), (1, 0.55)], 4.4, 5.6),
              colour([(0, [0, 0, 0, 0]), (0.12, [1.8, 1.3, 0.5, 1]), (0.5, [1.3, 0.7, 0.15, 0.7]), (1, [0.5, 0.1, 0.0, 0])], 0.8, 1.2, (0, 0.25, 2.5)),
              spin(), ground('ring-thick', 0.3)),
        layer('pollen', 'Pollen', stream(40, 0.6, r + 0.05), life(1.2, 2.0), at(2.6, 'sphere', radius=2.5, inner=0.3),
              ('launch', {'direction': 'outward', 'speed': [1, 4]}), ('physics', {'gravity': [0, 2.5, 0], 'drag': 1.2}),
              ('turbulence', {'strength': 3, 'scale': 2, 'speed': 1}),
              size(TWINKLE, 0.2, 0.6), colour(GOLD, 1.5, 5.0, (0, 0.15, 2.2)), board('pin-dot')),
    ]


# Agreed effects the game plays: builder → (id, file under effects/, name, description).
SHIPPED = {
    'level_up': ('effect.heroes.level-up', 'heroes/level-up.json', 'Hero level up', 'A hero gains a level.'),
}


def main():
    package = sys.argv[1]
    ship = '--ship' in sys.argv[2:]
    makers = (pillar_of_flame, substance_nova, violet_gate, golden_mending, firework, torch, ground_fire, level_up)
    for make in makers:
        if ship and make.__name__ not in SHIPPED:
            continue
        head, layers = make()
        doc = dict(head, description='', graphs=layers)
        doc.setdefault('layers', [])
        name = head['id'].split('.')[-1]
        path = os.path.join(package, 'effects', 'ours', name + '.json')
        if ship:
            doc['id'], file, doc['name'], doc['description'] = SHIPPED[make.__name__]
            path = os.path.join(package, 'effects', file)
        os.makedirs(os.path.dirname(path), exist_ok=True)
        with open(path, 'w') as f:
            json.dump(doc, f, indent=2)
            f.write('\n')
        print('wrote', name, len(layers), 'layers',
              sum(len(l['graph']['nodes']) for l in layers), 'nodes')


if __name__ == '__main__':
    main()
