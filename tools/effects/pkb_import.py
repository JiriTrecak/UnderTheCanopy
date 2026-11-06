#!/usr/bin/env python3
"""Imports Warcraft III: Reforged's PopcornFX effects (.pkb) as our effect graphs (D113), for
the local lab only: the effects and their textures are Blizzard's, so the output goes to
`target/wc3-lab/base/effects/wc3/` (a copy of the base package that is never committed or
shipped; docs/effect-graph.md, "Reforged as the test of fidelity").

    python3 -I tools/effects/pkb_import.py OUT_DIR FILE.pkb [FILE.pkb ...] [--report REPORT.json]
        [--models SEQUENCES.json]   (each effect's model sequences: its play length)

Each renderer layer becomes one graph layer: its spawner (the layer that kicks it) becomes a
rate or a burst, its decompiled spawn and evolve scripts (`pkb_dump.decompile`) are parsed
back into expressions and rebuilt from our nodes, and its renderer becomes a billboard.

- Fields a script sets once at spawn and only reads afterwards become nodes evaluated at
  birth; fields it integrates every step are recognised as position, velocity (with its
  acceleration), orbit and drag, and use our integrator and forces.
- Particles simulate in PopcornFX units with y up (vectors are converted from their z up);
  the renderer's position, size and axis are scaled to metres (one unit is 3.125 m:
  Warcraft scales PopcornFX by 100, and 128 of its units are our 4-cell block).
- A billboard's Size is a radius in PopcornFX and a full width for us (×2).
- What it cannot rebuild (event children, ribbons, opcodes the decoder only guessed) it
  leaves out, and the report says so per layer: `ok`, `partial` with what was dropped, or
  `skipped`.
"""
import json
import math
import os
import re
import struct
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pkb_dump  # noqa: E402

SCALE = 3.125
#: The simulation step, where a script reads the frame's `scene.dt` outside an integration.
STEP = 1.0 / 60.0
TEXTURE_PREFIX = '_HD.w3mod/Textures/FX/'
#: Drawn where a texture is not in the lab: a soft glow.
FALLBACK_TEXTURE = 'asset.effects.wc3.flare.flaresimple-bw'

# ---------------------------------------------------------------------------------------
# Expressions: the decompiler's statements parsed back into trees
# ---------------------------------------------------------------------------------------

TOKEN = re.compile(r'\s*(?:(?P<num>-?(?:\d+\.?\d*(?:e[-+]?\d+)?|inf))(?![A-Za-z_])|(?P<id>[A-Za-z_][A-Za-z0-9_.]*\??)|(?P<op>->|<=|>=|==|!=|&&|\|\||[-+*/<>!?:(),]))')


def tokens(text):
    out, at = [], 0
    while at < len(text):
        if text[at].isspace():
            at += 1
            continue
        m = TOKEN.match(text, at)
        if not m or m.end() == at:
            raise ValueError('cannot read %r' % text[at:at + 20])
        kind = m.lastgroup
        out.append((kind, m.group(kind)))
        at = m.end()
        # The decoder marks operators it only guessed with `?` (`/?`, `>=?`): the operator.
        if kind == 'op' and m.group(kind) in ('+', '-', '*', '/', '<', '>', '<=', '>=', '==', '!=') \
                and at < len(text) and text[at] == '?':
            at += 1
    return out


class Parser:
    """Precedence climbing over the decompiler's syntax: calls, member calls, `.xy`
    swizzles, `->type` casts, unary `!`, binary arithmetic and comparisons, `a ? b : c`."""

    def __init__(self, text):
        self.t = tokens(text)
        self.i = 0

    def peek(self):
        return self.t[self.i] if self.i < len(self.t) else (None, None)

    def take(self, value=None):
        tok = self.peek()
        if value is not None and tok[1] != value:
            raise ValueError('expected %s, found %s' % (value, tok[1]))
        self.i += 1
        return tok

    def parse(self):
        e = self.ternary()
        if self.i != len(self.t):
            raise ValueError('trailing %s' % (self.t[self.i:],))
        return e

    def ternary(self):
        cond = self.binary(0)
        if self.peek()[1] == '?':
            self.take('?')
            a = self.ternary()
            self.take(':')
            b = self.ternary()
            return ('select', cond, a, b)
        return cond

    LEVELS = [('||',), ('&&',), ('<', '>', '<=', '>=', '==', '!='), ('+', '-'), ('*', '/')]

    def binary(self, level):
        if level == len(self.LEVELS):
            return self.unary()
        e = self.binary(level + 1)
        while self.peek()[0] == 'op' and self.peek()[1] in self.LEVELS[level]:
            op = self.take()[1]
            e = ('bin', op, e, self.binary(level + 1))
        return e

    def unary(self):
        if self.peek()[1] == '!':
            self.take()
            return ('not', self.unary())
        return self.postfix(self.atom())

    def atom(self):
        kind, value = self.take()
        if kind == 'num':
            return ('num', float(value))
        if value == '(':
            e = self.ternary()
            self.take(')')
            return e
        if kind == 'id':
            if self.peek()[1] == '(':
                self.take('(')
                args = []
                while self.peek()[1] != ')':
                    args.append(self.ternary())
                    if self.peek()[1] == ',':
                        self.take(',')
                self.take(')')
                return ('call', value, args)
            return ('var', value)
        raise ValueError('unexpected %s' % value)

    def postfix(self, e):
        while True:
            tok = self.peek()
            if tok[1] == '->':
                self.take()
                e = ('cast', self.take()[1], e)
            elif tok[0] == 'id' and tok[1].startswith('.') is False and False:
                break
            else:
                return e


def parse(text):
    # `x.xy` swizzles come through as identifiers ending in `.xy`; a call followed by one
    # (`f(x).xy`) is rewritten to a call of `swizzle_xy`.
    text = re.sub(r'\)\.([xyzw]{1,4})\b', r').__swz_\1', text)
    text = re.sub(r'\)\.__swz_([xyzw]{1,4})', r', "\1")__SWZ', text)
    if '__SWZ' in text:
        raise ValueError('swizzle of an expression')
    return Parser(text).parse()


# ---------------------------------------------------------------------------------------
# Graph building
# ---------------------------------------------------------------------------------------


class Unsupported(Exception):
    pass


def yup(v):
    """A PopcornFX vector (z up) in our space (y up); other widths unchanged: four
    components are colours and HSV shifts, never positions."""
    v = list(v)
    if len(v) == 3:
        v[1], v[2] = v[2], v[1]
    return v


class Builder:
    """One graph layer: nodes, wires, and the value each expression became."""

    def __init__(self, layer, attributes=None):
        self.layer = layer
        #: The effect's attributes (its parameters) by name, at their defaults.
        self.attributes = attributes or {}
        self.nodes = []
        self.wires = []
        self.dropped = []
        self.samplers = {s['name']: s for s in layer['samplers']}
        self.attribute_nodes = {}
        #: Born of another layer's events (its emitter is where the event happened).
        self.event_born = False
        #: Why each temporary that could not be built failed.
        self.failed = {}
        #: Declared widths of the layer's fields (a float4 is a colour: no axis swap).
        self.widths = {}

    def node(self, kind, inputs=None, name=None):
        n = {'id': '%s%d' % (kind, len(self.nodes)), 'kind': kind, 'at': [0, 0]}
        if inputs:
            n['inputs'] = inputs
        if name:
            n['name'] = name
        self.nodes.append(n)
        return n['id']

    def wire(self, value, to_node, pin):
        """Feeds `value` (a literal list, or ('out', node, pin)) into `to_node.pin`."""
        if value[0] == 'lit':
            n = next(x for x in self.nodes if x['id'] == to_node)
            v = value[1]
            n.setdefault('inputs', {})[pin] = v[0] if len(v) == 1 else v
        else:
            self.wires.append({'from': '%s.%s' % (value[1], value[2]), 'to': '%s.%s' % (to_node, pin)})

    def event_node(self):
        if 'event' not in self.attribute_nodes:
            self.attribute_nodes['event'] = self.node('event')
        return self.attribute_nodes['event']

    def attribute(self, kind):
        if kind not in self.attribute_nodes:
            self.attribute_nodes[kind] = self.node(kind)
        return ('out', self.attribute_nodes[kind], 'value')

    def op(self, kind, *args, pins=('a', 'b')):
        n = self.node(kind)
        for value, pin in zip(args, pins):
            self.wire(value, n, pin)
        return ('out', n, 'value')

    def swizzle(self, v, comps, width=3):
        idx = ['xyzw'.index(c) for c in comps]
        # Their z is our y (positions; a float4 is a colour, read as it is).
        if width == 3 and not (v[0] == 'lit' and len(v[1]) == 4):
            idx = [{1: 2, 2: 1}.get(i, i) for i in idx]
        if v[0] == 'lit':
            vals = v[1] * 4 if len(v[1]) == 1 else v[1] + [0.0] * 4
            return ('lit', [vals[i] for i in idx])
        n = self.node('split')
        self.wire(v, n, 'value')
        outs = [('out', n, 'xyzw'[i]) for i in idx]
        if len(outs) == 1:
            return outs[0]
        if comps in ('xyz', 'xyzw'[:len(comps)]) and len(comps) >= 3:
            return v  # the leading components: the value itself where a vector is read
        raise Unsupported('swizzle .%s of a computed value' % comps)

    #: The `->floatN` a swizzle is cast to (set by the parser's caller; 4 otherwise).
    cast_width = 4

    def swizzle_code(self, v, code, width):
        """The decoder's swizzle opcode: 3-bit selectors from the lowest bits, 0–3 a
        component of the input, 4 zero, 5 one; their z is our y on both sides."""
        ours = {1: 2, 2: 1}
        sel = [(code >> (3 * j)) & 7 for j in range(4)][:width]
        out = [None] * width
        for j, c in enumerate(sel):
            jj = ours.get(j, j) if width == 3 else j
            out[jj] = ('const', 0.0) if c == 4 else ('const', 1.0) if c == 5 else ('comp', c)
        if any(o is None or (o[0] == 'comp' and o[1] > 3) or (o[0] == 'const' and False) for o in out):
            raise Unsupported('swizzle code %04x' % code)
        if v[0] == 'lit':
            vals = v[1] * 4 if len(v[1]) == 1 else v[1] + [0.0] * 4
            src = lambda c: vals[ours.get(c, c) if len(v[1]) == 3 else c]  # noqa: E731
            return ('lit', [o[1] if o[0] == 'const' else src(o[1]) for o in out])
        # A vector of three or four (read past y) is in our space: its z is our y.
        vector = width == 3 or (width < 3 and any(o[0] == 'comp' and o[1] >= 2 for o in out))
        n = None
        parts = []
        for o in out:
            if o[0] == 'const':
                parts.append(('lit', [o[1]]))
            else:
                if n is None:
                    n = self.node('split')
                    self.wire(v, n, 'value')
                c = ours.get(o[1], o[1]) if vector else o[1]
                parts.append(('out', n, 'xyzw'[c]))
        if width == 1:
            return parts[0]
        if n is not None and all(p == ('out', n, 'x') for p in parts) and not vector:
            return v  # a number broadcast
        kind = 'makeVector' if width == 3 else 'compose'
        if width == 2:
            raise Unsupported('swizzle to two components')
        c = self.node(kind)
        for part, pin in zip(parts, 'xyzw'):
            self.wire(part, c, pin)
        return ('out', c, 'value')

    COMPARE = {'<': '<', '>': '>', '<=': '<=', '>=': '>=', '==': '==', '!=': '!='}

    def binop(self, op, a, b):
        if op in self.COMPARE:
            if a[0] == 'lit' and b[0] == 'lit':
                x, y = a[1][0], b[1][0]
                return ('lit', [1.0 if {'<': x < y, '>': x > y, '<=': x <= y, '>=': x >= y,
                                        '==': x == y, '!=': x != y}[op] else 0.0])
            n = self.node('compare', {'op': self.COMPARE[op]})
            self.wire(a, n, 'a')
            self.wire(b, n, 'b')
            return ('out', n, 'value')
        if op in ('&&', '||'):
            for x, y in ((a, b), (b, a)):
                if x[0] == 'lit':
                    t = x[1][0] > 0.5
                    if op == '&&':
                        return y if t else ('lit', [0.0])
                    return ('lit', [1.0]) if t else y
            return self.op('and' if op == '&&' else 'or', a, b)
        if a[0] == 'lit' and b[0] == 'lit' and len(a[1]) == len(b[1]) or (a[0] == 'lit' and b[0] == 'lit' and 1 in (len(a[1]), len(b[1]))):
            x, y = a[1], b[1]
            n = max(len(x), len(y))
            x = x * n if len(x) == 1 else x
            y = y * n if len(y) == 1 else y
            f = {'+': lambda p, q: p + q, '-': lambda p, q: p - q, '*': lambda p, q: p * q,
                 '/': lambda p, q: p / q if q else 0.0}[op]
            return ('lit', [f(p, q) for p, q in zip(x, y)])
        kind = {'+': 'add', '-': 'subtract', '*': 'multiply', '/': 'divide'}.get(op)
        if not kind:
            raise Unsupported('operator %s' % op)
        return self.op(kind, a, b)

    def sampler(self, name):
        s = self.samplers.get(name)
        if s is None:
            raise Unsupported('sampler %s' % name)
        return s

    def expr(self, e, env):
        """`e` as a value: ('lit', numbers) or ('out', node, pin)."""
        k = e[0]
        if k == 'num':
            v = e[1]
            return ('lit', [v if math.isfinite(v) else 1e6])
        if k == 'var':
            name = e[1]
            if name in env:
                return env[name]
            if name in self.failed:
                raise Unsupported(self.failed[name])
            if name in ('true', 'false'):
                return ('lit', [1.0 if name == 'true' else 0.0])
            if name == 'scene.dt':
                return ('lit', [STEP])
            if name == 'scene.time':
                return self.attribute('effectTime')
            if name == '__a_Game.TargetPosition':
                # The effect's other end, in their units.
                if 'target' not in self.attribute_nodes:
                    self.attribute_nodes['target'] = self.node('target')
                return self.binop('*', ('out', self.attribute_nodes['target'], 'position'),
                                  ('lit', [1.0 / SCALE]))
            if name == '__a_Game.TeamColor':
                return ('out', self.node('teamColour'), 'value')
            if name.startswith('__a_') and name[4:] in self.attributes:
                return ('lit', self.attributes[name[4:]])
            if name == 'self.lifeRatio':
                return self.attribute('lifeRatio')
            if name == 'self.invLife':
                return self.op('divide', ('lit', [1.0]), self.attribute('life'))
            if re.match(r'^f\d+$', name):
                # A constant the decoder could not place (offsets, mostly): none.
                self.dropped.append('%s: an unread constant, taken as 0' % name)
                return ('lit', [0.0])
            m = re.match(r'^(.*)\.([xyzw]{1,4})$', name)
            if m and (m.group(1) in env or m.group(1).startswith('self.')):
                width = self.widths.get(m.group(1), 3)
                return self.swizzle(self.expr(('var', m.group(1)), env), m.group(2), width)
            raise Unsupported('variable %s' % name)
        if k == 'bin':
            return self.binop(e[1], self.expr(e[2], env), self.expr(e[3], env))
        if k == 'select':
            cond = self.expr(e[1], env)
            a, c = self.expr(e[2], env), self.expr(e[3], env)
            if cond[0] == 'lit':
                return a if cond[1][0] > 0.5 else c
            n = self.node('select')
            self.wire(cond, n, 'condition')
            self.wire(a, n, 'a')
            self.wire(c, n, 'b')
            return ('out', n, 'value')
        if k == 'not':
            v = self.expr(e[1], env)
            if v[0] == 'lit':
                return ('lit', [0.0 if v[1][0] > 0.5 else 1.0])
            return self.op('not', v, pins=('x',))
        if k == 'call':
            return self.call(e[1], e[2], env)
        if k == 'cast':
            width = {'float': 1, 'float2': 2, 'float3': 3, 'float4': 4}.get(e[1], 4)
            before, self.cast_width = self.cast_width, width
            try:
                return self.expr(e[2], env)
            finally:
                self.cast_width = before
        raise Unsupported(k)

    def call(self, fn, args, env):
        base = fn.split('.')[-1]
        if base in ('float3', 'float4', 'float2', 'float', 'int'):
            vals = [self.expr(a, env) for a in args]
            width = {'float': 1, 'int': 1, 'float2': 2, 'float3': 3, 'float4': 4}[base]
            if len(vals) == 1:
                v = vals[0]
                if v[0] == 'lit' and len(v[1]) == 1:
                    return ('lit', v[1] * width)
                return v  # a number broadcasts where a vector is wanted
            if all(v[0] == 'lit' and len(v[1]) == 1 for v in vals):
                return ('lit', yup([v[1][0] for v in vals]))
            raise Unsupported('%s of computed parts' % base)
        if base == 'swz46_0024':
            # A number made into a vector along the up axis (the decoder's guess, confirmed
            # by the particles it moves rising).
            v = self.expr(args[0], env)
            if v[0] == 'lit':
                return ('lit', [0.0, v[1][0], 0.0])
            return self.op('multiply', ('lit', [0.0, 1.0, 0.0]), v)
        if base == 'vrand':
            # A random direction, or a random point in a shell between two radii.
            d = ('out', self.node('randomDirection'), 'value')
            if len(args) >= 3:
                r = self.node('random')
                self.wire(self.expr(args[0], env), r, 'min')
                self.wire(self.expr(args[1], env), r, 'max')
                return self.binop('*', d, ('out', r, 'value'))
            return d
        if base == 'rand':
            lo, hi = self.expr(args[0], env), self.expr(args[1], env)
            n = self.node('random')
            self.wire(lo, n, 'min')
            self.wire(hi, n, 'max')
            return ('out', n, 'value')
        if base == 'sample':
            s = self.sampler(fn.split('.')[0])
            if s.get('kind') != 'curve':
                raise Unsupported('sample of %s' % s.get('kind'))
            keys = []
            for key in s['keys']:
                k = {'t': key['t'], 'v': yup(key['value']) if s['dimension'] == 3 else list(key['value'])}
                if 'tangent_in' in key:
                    k['in'] = yup(key['tangent_in']) if s['dimension'] == 3 else list(key['tangent_in'])
                    k['out'] = yup(key['tangent_out']) if s['dimension'] == 3 else list(key['tangent_out'])
                keys.append(k)
            n = self.node('curve', {'curve': {'keys': keys}}, name=s.get('label'))
            x = self.expr(args[0], env)
            if not (x[0] == 'out' and x[1] == self.attribute_nodes.get('lifeRatio')):
                self.wire(x, n, 'x')
            return ('out', n, 'value')
        if base == 'samplePosition':
            s = self.sampler(fn.split('.')[0])
            shape = str(s.get('shape')).rstrip('?')
            radius = s.get('radius', 1.0)
            if shape == 'Cylinder':
                v = ('out', self.node('cylinder', {'radius': radius, 'height': s.get('height?', 0.0),
                                                   'inner': s.get('innerRadius?', 0.0)}), 'position')
            elif shape == 'Sphere':
                v = ('out', self.node('sphere', {'radius': radius, 'inner': s.get('innerRadius?', 0.0)}), 'position')
            elif shape == 'Box':
                v = ('out', self.node('box', {'size': list(s.get('f16') or [1.0, 1.0, 1.0])}), 'position')
            elif shape == 'Ellipsoid':
                scale = list(s.get('f22') or [1.0, 1.0, 1.0])
                v = ('out', self.node('ellipsoid', {'radius': [radius * c for c in scale],
                                                    'inner': s.get('innerRadius?', 0.0)}), 'position')
            else:
                raise Unsupported('shape %s' % shape)
            # Shapes are described y up, as ours (offsets are lifts: (0, 0.3, 0) and the
            # like; flat boxes are (x, 0, z)), unlike the z-up scripts that sample them.
            offset = s.get('f13_offset?')
            if offset and any(offset):
                v = self.binop('+', v, ('lit', list(offset)))
            return v
        if base == 'fn18?':
            # Saturate: held between 0 and 1.
            return self.op('clamp', self.expr(args[0], env), pins=('x',))
        if base == 'fn17?':
            return self.op('fract', self.expr(args[0], env), pins=('x',))
        if base == 'un01?':
            # The inverse square root (normalising).
            return self.binop('/', ('lit', [1.0]), self.op('sqrt', self.expr(args[0], env), pins=('x',)))
        if base in ('alu0c', 'alu0d', 'alu0e', 'alu0f', 'alu09'):
            # Comparisons and a logical or, read from where scripts use them.
            op = {'alu0c': '<', 'alu0d': '<=', 'alu0e': '>', 'alu0f': '>=', 'alu09': '||'}[base]
            return self.binop(op, self.expr(args[0], env), self.expr(args[1], env))
        if base == 'closestF3' and fn.startswith('__spatialLayer'):
            # Reforged keys a spatial layer by the inserting particle's id, which the
            # particles it spawned query: where their parent is now.
            if 'parent' not in self.attribute_nodes:
                self.attribute_nodes['parent'] = self.node('parent')
            return ('out', self.attribute_nodes['parent'], 'position')
        if base == 'intersect' and fn.startswith('scene.'):
            # The scene, as the ground under the emitter: (normal, distance) as theirs.
            n = self.node('groundHit')
            self.wire(self.expr(args[0], env), n, 'origin')
            self.wire(self.expr(args[1], env), n, 'direction')
            self.wire(self.expr(args[2], env), n, 'length')
            return ('out', n, 'value')
        if base == 'isRunning':
            return ('out', self.node('effectRunning'), 'value')
        if base == 'position' and fn.startswith('effect.'):
            return ('lit', [0.0, 0.0, 0.0])  # the effect's emitter, our origin
        if base == 'age' and fn.startswith('effect.'):
            return self.attribute('effectTime')
        m = re.match(r'^swz[0-9a-f]{2}_([0-9a-f]{4})$', base)
        if m and base not in ('swz46_0024', 'swzc0_1008'):
            return self.swizzle_code(self.expr(args[0], env), int(m.group(1), 16), self.cast_width)
        if base == 'un31?' and args and args[0][0] == 'call' and args[0][1] == 'un33?':
            # Any component non-zero.
            v = self.expr(args[0][2][0], env)
            return self.binop('>', self.op('length', v, pins=('x',)), ('lit', [0.0]))
        if base == 'un33?':
            return self.binop('!=', self.expr(args[0], env), ('lit', [0.0]))
        if base == 'un16?':
            return self.op('fract', self.expr(args[0], env), pins=('x',))
        if base == 'alu04':
            return self.binop('-', self.expr(args[0], env), self.expr(args[1], env))
        if base == 'swzc0_1008':
            # A pair made a vector (x, y, 0) in their space: across the ground in ours.
            v = self.expr(args[0], env)
            if v[0] == 'lit':
                x = v[1] * 2 if len(v[1]) == 1 else v[1]
                return ('lit', [x[0], 0.0, x[1]])
            n = self.node('split')
            self.wire(v, n, 'value')
            m = self.node('makeVector')
            self.wire(('out', n, 'x'), m, 'x')
            self.wire(('out', n, 'y'), m, 'z')
            return ('out', m, 'value')
        if base == 'rgb2hsv':
            return self.op('rgbToHsv', self.expr(args[0], env), pins=('colour',))
        if base.startswith('xform_'):
            # Local to world. Our particles live in the effect emitter's space, where a
            # layer born of events has its emitter at the event: points move there.
            v = self.expr(args[0], env)
            if v[0] == 'lit' and len(v[1]) == 1:
                v = ('lit', [v[1][0]] * 3)
            if self.event_born and '_f_' in base:
                return self.binop('+', v, ('out', self.event_node(), 'position'))
            if '_f_' in base:
                # Where the effect's emitter is now: a missile's head as it flies (0 for
                # an effect that stays put), so births there stay behind as a trail.
                if 'emitter' not in self.attribute_nodes:
                    self.attribute_nodes['emitter'] = self.node('emitter')
                # Metres to their units (particles simulate in PopcornFX units).
                carried = self.binop('*', ('out', self.attribute_nodes['emitter'], 'position'),
                                     ('lit', [1.0 / SCALE]))
                return carried if v == ('lit', [0.0, 0.0, 0.0]) else self.binop('+', v, carried)
            return v
        if base.startswith('extractPayloadElement'):
            ev = self.event_node()
            element = literal(args[0]) if args else None
            if base.endswith('F3'):
                return ('out', ev, 'position')
            if base.endswith('F4'):
                return ('out', ev, 'colour')
            # A generate's payload: the child's place in its batch (2) and its share of
            # the way (5).
            if element == 5:
                return ('out', ev, 'share')
            if element == 2:
                return ('out', ev, 'order')
            return self.swizzle(('out', ev, 'value'), 'x')
        if base == 'neg':
            v = self.expr(args[0], env)
            if v[0] == 'lit':
                return ('lit', [-x for x in v[1]])
            return self.op('negate', v, pins=('x',))
        if base == 'rcp':
            return self.binop('/', ('lit', [1.0]), self.expr(args[0], env))
        if base in ('sqrt?', 'exp?'):
            return self.op(base.rstrip('?'), self.expr(args[0], env), pins=('x',))
        if base in ('min?', 'max?'):
            return self.op(base.rstrip('?'), self.expr(args[0], env), self.expr(args[1], env))
        if base == 'lerp':
            return self.op('lerp', *(self.expr(a, env) for a in args), pins=('a', 'b', 't'))
        if base == 'dot':
            return self.op('dot', self.expr(args[0], env), self.expr(args[1], env))
        if base == 'hsv2rgb':
            # hsv2rgb(rgb2hsv(c) + shift [+ jitter]): a colour shifted in HSV.
            inner = args[0]
            shifts = []
            while inner[0] == 'bin' and inner[1] == '+':
                shifts.append(inner[3])
                inner = inner[2]
            if inner[0] != 'call' or inner[1].split('.')[-1] != 'rgb2hsv':
                return self.op('hsvToRgb', self.expr(args[0], env), pins=('colour',))
            colour = self.expr(inner[2][0], env)
            n = self.node('hsvShift')
            self.wire(colour, n, 'colour')
            total = None
            for sh in shifts:
                v = self.expr(sh, env)
                if v[0] == 'lit':
                    v = ('lit', v[1][:3] if len(v[1]) >= 3 else v[1] * 3)
                total = v if total is None else self.binop('+', total, v)
            if total is not None:
                if total[0] == 'lit':
                    total = ('lit', total[1][:3])
                self.wire(total, n, 'shift')
            return ('out', n, 'value')
        if base == 'rotate':
            q = args[1] if len(args) == 2 else None
            if q is not None and q[0] == 'cast':
                q = q[2]
            if (q is not None and q[0] == 'call' and q[1].startswith('swzf6_2124') and q[2]
                    and q[2][0][0] == 'call' and q[2][0][1] == 'un07?'):
                # A quaternion about their up axis from a half-angle (un07? gives its sine
                # and cosine): a turn about our y by twice it, mirrored by the axis swap.
                half = self.expr(q[2][0][2][0], env)
                angle = self.binop('*', half, ('lit', [-2.0]))
                n = self.node('rotate', {'axis': [0.0, 1.0, 0.0]})
                self.wire(self.expr(args[0], env), n, 'vector')
                self.wire(angle, n, 'angle')
                return ('out', n, 'value')
            return self.op('rotate', *(self.expr(a, env) for a in args), pins=('vector', 'axis', 'angle'))
        raise Unsupported('function %s' % fn)


def statements(layer, stage):
    for sc in layer['scripts']:
        if sc['stage'] == stage:
            return sc['code']
    return []


def assignments(code):
    out = []
    for line in code:
        m = re.match(r'^([A-Za-z_][A-Za-z0-9_.]*) = (.*)$', line)
        if m:
            out.append((m.group(1), m.group(2)))
    return out


def evolve_code(layer):
    for stage in (4, 3, 5):
        code = statements(layer, stage)
        if code:
            return code
    return []


def texture_asset(path):
    # Any build's copy of the FX folder (`_HD.w3mod/`, `_DE.w3mod/` …).
    m = re.match(r'^[^/]*\.w3mod/textures/fx/', path or '', re.IGNORECASE)
    if not m:
        return None
    rel = path[m.end():].rsplit('.', 1)[0]
    parts = [re.sub(r'[^a-z0-9]+', '-', p.lower()).strip('-') for p in rel.split('/')]
    return 'asset.effects.wc3.' + '.'.join(parts)


MODES = {'ScreenAligned': 'camera', 'ViewposAligned': 'camera', 'AxisAligned': 'axis',
         'AxisAlignedSpheroid': 'spheroid', 'AxisAlignedCapsule': 'spheroid', 'PlaneAligned': 'plane'}


INT_TYPES = ('int2', 'int3', 'int4', 'bool2')  # generate accumulators; plain ints are numbers
WIDTH = {'float': 1, 'float2': 2, 'float3': 3, 'float4': 4, 'bool': 1, 'int': 1}


def inline_temps(code):
    """Assignments of a script with its temporaries (`t1 = …`) inlined, and its other
    statements (kicks) likewise."""
    temps = {}
    out, calls = [], []

    def inline(text):
        # Temporaries are stored expanded, so one pass resolves them.
        return re.sub(r'\bt\d+\b', lambda m: '(%s)' % temps[m.group(0)]
                      if m.group(0) in temps else m.group(0), text)
    for line in code:
        m = re.match(r'^([A-Za-z_][A-Za-z0-9_.]*) = (.*)$', line)
        if m and re.match(r'^t\d+$', m.group(1)):
            temps[m.group(1)] = inline(m.group(2))
            # Transcribed in order as values of their own (shared, not copied).
            out.append((m.group(1), m.group(2)))
        elif m:
            out.append((m.group(1), m.group(2)))
        else:
            calls.append(inline(line))
            out.append(('!call', line))
    return out, calls, dict(temps)


def walk(e):
    yield e
    for x in e[1:]:
        if isinstance(x, tuple):
            yield from walk(x)
        elif isinstance(x, list):
            for y in x:
                if isinstance(y, tuple):
                    yield from walk(y)


def life_of(layer):
    life = layer['summary'].get('life', '1')
    if life.startswith('0 (instant)'):
        return 0.0
    if life == 'inf':
        return math.inf
    try:
        return float(life)
    except ValueError:
        m = re.match(r'rand\(([-\d.e]+), ([-\d.e]+)', life)
        return max(float(m.group(1)), float(m.group(2))) if m else 1.0


def generate_of(layer):
    """The amount and spacing of a spawner's `generate(state, amount, spacing, …)`."""
    for sc in layer['scripts']:
        assigns, calls, temps = inline_temps(sc['code'])
        texts = calls + list(temps.values()) + [t for n, t in assigns if n != '!call']
        for text in texts:
            m = re.search(r'\bgenerate\(', text)
            if not m:
                continue
            try:
                tree = parse(text[m.start():])
            except ValueError:
                try:
                    tree = parse(text)
                except ValueError:
                    continue
            for node in walk(tree):
                if node[0] == 'call' and node[1].split('.')[-1] == 'generate' and len(node[2]) >= 3:
                    return node[2][1], node[2][2]
    return None


def literal(e, dt=STEP):
    """A tree of numbers and `scene.dt` evaluated, or None."""
    k = e[0]
    if k == 'num':
        return e[1]
    if e == ('var', 'scene.dt'):
        return dt
    if k == 'bin' and e[1] in ('+', '-', '*', '/'):
        a, c = literal(e[2], dt), literal(e[3], dt)
        if a is None or c is None:
            return None
        return {'+': a + c, '-': a - c, '*': a * c, '/': a / c if c else 0.0}[e[1]]
    if k == 'call' and e[1] in ('float', 'int') and len(e[2]) == 1:
        return literal(e[2][0], dt)
    return None


def kick_of(layer, event):
    """The trigger condition and payload position (current, start) of a layer's kicks of
    `event` in its evolve script, as trees."""
    cond, position, position_kind = None, None, None
    for name, text in [(None, c) for c in inline_temps(evolve_code(layer))[1]]:
        if '%s.kick' % event not in text:
            continue
        try:
            tree = parse(text)
        except ValueError:
            continue
        for node in walk(tree):
            if node[0] != 'call':
                continue
            base = node[1].split('.')[-1]
            if base == 'trigger' and node[2]:
                cond = node[2][0]
            if base == 'buildPayloadElement' and len(node[2]) >= 3:
                cur = node[2][2]
                # The tag's high byte is the element's type: 1 a float3 (a position; 0x102
                # interpolated over the step), 2 a float4 (an orientation, a colour), 0 a
                # number. The position is the float3; untagged ones keep the old guess.
                tag = node[2][3] if len(node[2]) >= 4 and node[2][3][0] == 'num' else None
                kind = int(tag[1]) >> 8 if tag is not None else None
                if cur[0] != 'num' and kind in (None, 1):
                    if kind == 1 or position is None or position_kind != 1:
                        position, position_kind = cur, kind
    return cond, position


def sample_curve(sampler, x):
    """A curve sampler's first component at `x` (Hermite between keys)."""
    keys = sampler.get('keys') or []
    if not keys:
        return 0.0
    if x <= keys[0]['t']:
        return float(keys[0]['value'][0])
    for a, b in zip(keys, keys[1:]):
        if x <= b['t']:
            span = max(b['t'] - a['t'], 1e-9)
            u = (x - a['t']) / span
            p0, p1 = float(a['value'][0]), float(b['value'][0])
            m0 = float((a.get('tangent_out') or [p1 - p0])[0])
            m1 = float((b.get('tangent_in') or [p1 - p0])[0])
            u2, u3 = u * u, u * u * u
            return (2 * u3 - 3 * u2 + 1) * p0 + (u3 - 2 * u2 + u) * m0 + (-2 * u3 + 3 * u2) * p1 + (u3 - u2) * m1
    return float(keys[-1]['value'][0])


def rate_over_life(layer, samples=17):
    """A spawner's births a second at evenly spaced shares of its life, when its generate
    amount is a function of its life (a spray tapering by a curve), else None. Randoms
    count as their mean."""
    gen = generate_of(layer)
    if gen is None:
        return None
    amount, spacing = gen
    samplers = {s['name']: s for s in layer['samplers']}
    reads_life = any(n == ('var', 'self.lifeRatio') for n in walk(amount))
    if not reads_life:
        return None

    def ev(e, u):
        k = e[0]
        if k == 'num':
            return e[1]
        if k == 'var':
            if e[1] == 'scene.dt':
                return 1.0
            if e[1] == 'self.lifeRatio':
                return u
            raise ValueError(e[1])
        if k == 'bin' and e[1] in '+-*/':
            a, b = ev(e[2], u), ev(e[3], u)
            return {'+': a + b, '-': a - b, '*': a * b, '/': a / b if b else 0.0}[e[1]]
        if k == 'call':
            base = e[1].split('.')[-1]
            if base == 'rand':
                return (ev(e[2][0], u) + ev(e[2][1], u)) * 0.5
            if base == 'sample' and e[1].split('.')[0] in samplers:
                return sample_curve(samplers[e[1].split('.')[0]], ev(e[2][0], u))
            if base in ('float', 'int', 'cast') and e[2]:
                return ev(e[2][0], u)
            if base in ('min?', 'max?'):
                return (min if base == 'min?' else max)(ev(e[2][0], u), ev(e[2][1], u))
        if k == 'cast':
            return ev(e[2], u)
        raise ValueError(k)

    try:
        sp = ev(spacing, 0.0) or 1.0
        rates = [max(ev(amount, i / (samples - 1)), 0.0) / sp for i in range(samples)]
    except (ValueError, IndexError, TypeError):
        return None
    peak = max(rates)
    if peak <= 0:
        return None
    return peak, [r / peak for r in rates]


class Plan:
    """How every layer of an effect is born: at the effect's start (a burst or a rate), or
    of another layer's events; spawners that only generate fold into their parents."""

    def __init__(self, analysis):
        self.layers = analysis['layers']
        self.kicker = {}
        for route in analysis['event_routes']:
            for to in route['to_layers']:
                self.kicker.setdefault(to, (route['event'], route['from_layer']))
        #: Watchers: layers whose `__e_Trigger` the dump lists with no targets (they fire
        #: when the effect stops); the layers nothing kicks are theirs.
        self.watchers = [r['from_layer'] for r in analysis['event_routes']
                         if r['event'] == '__e_Trigger' and not r['to_layers'] and r['from_layer'] >= 0]
        self.memo = {}
        #: Emit specs per parent layer index.
        self.emits = {}
        #: Rate curves over a stream's span, by the layer it spawns (a tapering spray).
        self.rate_curves = {}
        #: When layers born at the start are born (seconds): a root event stream fires its
        #: signals at the times its sampler lists (Starfall's star lands at 0.98 s).
        self.delay = {}

    def root_times(self, i):
        for s in self.layers[i]['samplers'] if i >= 0 else []:
            if s.get('kind') == 'eventStream':
                raw = s.get('data', {}).get('f10') or [0]
                times = [struct.unpack('<f', struct.pack('<I', int(x) & 0xffffffff))[0] for x in raw]
                return [t if math.isfinite(t) and t >= 0 else 0.0 for t in times]
        return [0.0]

    def is_root(self, i):
        return i < 0 or str(self.layers[i].get('kind', '')).startswith('root')

    def foldable(self, i):
        """A spawner that does nothing but generate (no motion or values of its own)."""
        layer = self.layers[i]
        if layer['renderers'] or 'generate' not in layer['summary']:
            return False
        return not any(WIDTH.get(f['type'], 0) >= 3 and not f['name'].startswith('self.')
                       for f in layer['fields'])

    def emit(self, parent, spec):
        specs = self.emits.setdefault(parent, [])
        key = repr(sorted((k, repr(v)) for k, v in spec.items()))
        for s in specs:
            if s['key'] == key:
                return s['id']
        spec = dict(spec, key=key, id='ev%d' % len(specs))
        specs.append(spec)
        return spec['id']

    def event_spec(self, parent, event):
        """How built layer `parent` fires `event`."""
        layer = self.layers[parent]
        if event == '__e_OnSpawn':
            return {'when': 'birth'}
        if event == '__e_OnDeath':
            return {'when': 'death'}
        cond, position = kick_of(layer, event)
        spec = {'position': position}
        gen = generate_of(layer) if 'generate' in layer['summary'] else None
        if gen is not None:
            amount, spacing = gen
            sp = literal(spacing) or 1.0
            k = literal(amount)
            if k is not None and ('scene.dt',) != () and not any(n == ('var', 'scene.dt') for n in walk(amount)):
                if life_of(layer) == 0.0:
                    return dict(spec, when='birth', count=k / sp)
                return dict(spec, when='step', count=k / sp)
            per_second = literal(amount, 1.0)
            uses_dt = any(n == ('var', 'scene.dt') for n in walk(amount))
            if per_second is not None and uses_dt:
                return dict(spec, when='interval', interval=sp / max(per_second, 1e-6))
            curved = rate_over_life(layer) if uses_dt else None
            if curved is not None:
                # A rate that changes over the emitter's life (a spray tapering off).
                peak, shape = curved
                return dict(spec, when='interval', interval=1.0 / peak, curve=shape)
            return dict(spec, when='distance', interval=sp)
        if cond is not None and any(n == ('var', 'self.lifeRatio') for n in walk(cond)):
            return dict(spec, when='death')
        if cond is not None and cond != ('var', 'true'):
            return dict(spec, when='step', condition=cond)
        return dict(spec, when='step')

    def birth(self, i):
        """('start', rate or None, life or count) | ('event', parent, emit id) | None."""
        if i in self.memo:
            return self.memo[i]
        self.memo[i] = None
        out = None
        if i not in self.kicker and not self.is_root(i):
            # Nothing kicks it: a watcher lives from the start; the rest are born of a
            # watcher's trigger (when the effect stops), or else at the start.
            watcher = next((w for w in self.watchers if w != i), None)
            if i in self.watchers or watcher is None:
                out = ('start', None, 1.0)
            else:
                up = self.birth(watcher)
                if up is not None:
                    out = ('event', watcher, self.emit(watcher, self.event_spec(watcher, '__e_Trigger')))
            self.memo[i] = out
            return out
        if i in self.kicker:
            event, parent = self.kicker[i]
            if self.is_root(parent):
                out = ('start', None, 1.0)
                self.delay[i] = self.root_times(parent)
            elif self.foldable(parent):
                s = self.layers[parent]['summary']
                up = self.birth(parent)
                if up is not None and up[0] == 'start':
                    # Generated from the start: the spawner's rate or count.
                    self.delay[i] = self.delay.get(parent, [0.0])
                    life = life_of(self.layers[parent])
                    if 'rate_per_s' in s:
                        try:
                            rate = float(s['rate_per_s'])
                        except ValueError:
                            rate = 10.0
                            curved = rate_over_life(self.layers[parent])
                            if curved is not None:
                                rate, shape = curved
                                self.rate_curves[i] = shape
                        out = ('start', rate, 0.0 if math.isinf(life) else life)
                    else:
                        try:
                            count = float(s.get('count_per_update', '1'))
                        except ValueError:
                            count = 1.0
                        out = ('start', None, count)
                elif up is not None:
                    _, grand, emit_id = up
                    outer = next(x for x in self.emits[grand] if x['id'] == emit_id)
                    gen = generate_of(self.layers[parent])
                    spec = {k: v for k, v in outer.items() if k not in ('key', 'id')}
                    if gen is not None:
                        amount, spacing = gen
                        sp = literal(spacing) or 1.0
                        k = literal(amount)
                        per_second = literal(amount, 1.0)
                        uses_dt = any(n == ('var', 'scene.dt') for n in walk(amount))
                        if not uses_dt and k is not None:
                            spec['count'] = spec.get('count', 1.0) * k / sp
                        elif uses_dt and per_second is not None and spec.get('when') == 'birth':
                            # A spawner born with its parent, generating while it lives.
                            spec = {'when': 'interval', 'interval': sp / max(per_second, 1e-6),
                                    'position': spec.get('position')}
                        elif uses_dt and per_second is not None:
                            life = life_of(self.layers[parent])
                            life = 1.0 if math.isinf(life) else life
                            spec['count'] = spec.get('count', 1.0) * per_second * life / sp
                        elif uses_dt and rate_over_life(self.layers[parent]) is not None:
                            peak, shape = rate_over_life(self.layers[parent])
                            spec = {'when': 'interval', 'interval': 1.0 / peak, 'curve': shape,
                                    'position': spec.get('position')}
                        else:
                            spec = {'when': 'distance', 'interval': sp, 'position': spec.get('position')}
                    out = ('event', grand, self.emit(grand, spec))
            else:
                up = self.birth(parent)
                if up is not None:
                    out = ('event', parent, self.emit(parent, self.event_spec(parent, event)))
        self.memo[i] = out
        return out


def transcribe(analysis, li, birth, plan, ids, attributes):
    """One layer as our graph layer, its scripts transcribed statement by statement: the
    fields a script updates become the particle's attributes, each read seeing the latest
    value (raises Unsupported for a renderer we lack)."""
    layer = analysis['layers'][li]
    renderer = layer['renderers'][0] if layer['renderers'] else None
    if renderer is not None and renderer['class'] not in ('Billboard', 'Ribbon'):
        raise Unsupported('%s renderer' % renderer['class'])
    b = Builder(layer, attributes)
    b.event_born = birth[0] == 'event'
    types = {f['name']: f['type'] for f in layer['fields']}
    b.widths = {n: WIDTH.get(t, 1) for n, t in types.items()}
    if birth[0] == 'start':
        _, rate, life = birth
        times = plan.delay.get(li, [0.0])
        if rate is None:
            for t in times:
                b.node('burst', {'count': life, 'time': t} if t else {'count': life})
        else:
            inputs = {'rate': rate, 'duration': life, 'start': times[0]} if times[0] \
                else {'rate': rate, 'duration': life}
            if li in plan.rate_curves:
                shape = plan.rate_curves[li]
                inputs['curve'] = {'keys': [{'t': round(k / (len(shape) - 1), 4), 'v': [round(v, 4)]}
                                            for k, v in enumerate(shape)]}
            b.node('rate', inputs)
    else:
        _, parent, emit_id = birth
        b.node('onEvent', {'layer': ids[parent], 'event': emit_id})
    spawn, _, _ = inline_temps(statements(layer, 0))
    evolve, _, _ = inline_temps(evolve_code(layer))
    assigned = {n for n, _ in spawn} | {n for n, _ in evolve}
    evolve_text = ' '.join(t for _, t in evolve)
    state = [n for n, _ in evolve
             if n != '!call' and not re.match(r'^t\d+$', n)
             and not n.startswith('self.') and types.get(n) not in INT_TYPES
             and re.search(r'\b%s\b' % re.escape(n), evolve_text)]
    evolved = set(state)
    # Fields set at spawn and read later keep their birth value (a birth time, a random
    # pick): stored on the particle, never recomputed.
    state += [n for n, _ in spawn
              if n != '!call' and not re.match(r'^t\d+$', n) and not n.startswith('self.')
              and types.get(n) not in INT_TYPES and n not in evolved
              and re.search(r'\b%s\b' % re.escape(n), evolve_text)]
    state = list(dict.fromkeys(state))

    def defaults(env):
        # Fields no script sets come with the event: a position is where it happened.
        for name, ty in types.items():
            if name in assigned or name.startswith('self.') or ty in INT_TYPES or name in env:
                continue
            if WIDTH.get(ty) == 3:
                env[name] = b.attribute('position')
            elif ty == 'bool':
                env[name] = ('lit', [1.0])
            else:
                env[name] = ('lit', [0.0] * WIDTH.get(ty, 1))

    def value(env, name, tree):
        try:
            env[name] = b.expr(tree, env)
        except Unsupported as e:
            if re.match(r'^t\d+$', name):
                # A temporary: reported where something reads it.
                b.failed[name] = str(e)
                env.pop(name, None)
            else:
                b.dropped.append('%s: %s' % (name, e))

    # Birth.
    env = {}
    defaults(env)
    life_cap = None
    if birth[0] == 'event':
        parent_life = life_of(analysis['layers'][birth[1]])
        life_cap = parent_life if math.isfinite(parent_life) and parent_life > 0 else 2.0
    else:
        # Born at the start and living forever: until it ends itself (watchers, loops).
        life_cap = 1000.0
    for name, text in spawn:
        if name == '!call':
            continue
        try:
            tree = parse(text)
        except ValueError as e:
            b.dropped.append('%s: %s' % (name, e))
            continue
        if name == 'self.invLife':
            n = b.node('setLife')
            try:
                if tree[0] == 'call' and tree[1] == 'rcp':
                    life = b.expr(tree[2][0], env)
                else:
                    inv = b.expr(tree, env)
                    life = (('lit', [1.0 / inv[1][0] if inv[1][0] else (life_cap or 2.0)])
                            if inv[0] == 'lit' else b.binop('/', ('lit', [1.0]), inv))
                if life[0] == 'lit' and not (0 < life[1][0] < 1e5):
                    life = ('lit', [life_cap or 2.0])
                b.wire(life, n, 'life')
            except Unsupported as e:
                b.dropped.append('life: %s' % e)
            continue
        if name.startswith('self.') or types.get(name) in INT_TYPES:
            continue
        value(env, name, tree)
    def at_birth(n):
        # Evaluated when the particle is born, whatever it reads (its life ratio is 0 then).
        next(x for x in b.nodes if x['id'] == n)['atBirth'] = True
        return n

    def widen(v, name):
        # A number given to a vector field fills every component (their float3(x)).
        width = WIDTH.get(types.get(name), 1)
        if width < 2:
            return v
        if v[0] == 'lit':
            return ('lit', v[1] * width) if len(v[1]) == 1 else v
        return b.binop('*', v, ('lit', [1.0] * width))

    for name in state:
        init = env.get(name, ('lit', [0.0] * WIDTH.get(types.get(name), 1)))
        n = at_birth(b.node('setAttribute', {'name': name}))
        b.wire(widen(init, name), n, 'value')

    # Every step, in the script's order.
    uenv = dict(env)
    for name in state:
        reader = b.node('attribute', {'name': name})
        width = WIDTH.get(types.get(name), 1)
        uenv[name] = ('out', reader, {1: 'x', 3: 'xyz'}.get(width, 'value'))
    for name, text in evolve:
        if name == '!call':
            m = re.match(r'^self\.kill\((.*), ParticleContext\w*\)$', text)
            if m:
                try:
                    cond = b.expr(parse(m.group(1)), uenv)
                    if cond[0] != 'lit' or cond[1][0] > 0.5:
                        b.wire(cond, b.node('kill'), 'condition')
                except (ValueError, Unsupported) as e:
                    b.dropped.append('kill: %s' % e)
            # A point put in a spatial layer: the layer that reads it (`closestF3`) follows
            # this particle (its parent), so the particle stands at that point.
            if re.match(r'^__spatialLayer_\d+\.insert\(', text) and renderer is None:
                try:
                    point = next(n[2][1] for n in walk(parse(text))
                                 if n[0] == 'call' and n[1].endswith('.appendPayload') and len(n[2]) >= 2
                                 and n[2][0][0] == 'call' and n[2][0][1].endswith('.allocatePayload'))
                    b.wire(b.expr(point, uenv), b.node('setPosition'), 'position')
                except (StopIteration, ValueError, Unsupported) as e:
                    b.dropped.append('spatial point: %s' % e)
            continue
        if name.startswith('self.') or types.get(name) in INT_TYPES:
            continue
        try:
            value(uenv, name, parse(text))
        except ValueError as e:
            b.dropped.append('%s: %s' % (name, e))
    for name in state:
        if name in evolved and name in uenv and uenv[name] != env.get(name):
            n = b.node('setAttribute', {'name': name})
            b.wire(widen(uenv[name], name), n, 'value')

    # The particle's own position follows the field it is drawn (or emits) at, so events
    # carry it.
    pos_field = None
    if renderer is not None:
        pos_field = next((i['field'] for i in renderer['particle_inputs'] if i['input'] == 'Position'), None)
    emits = plan.emits.get(li, [])
    placed = False
    if pos_field is None:
        for spec in emits:
            if spec.get('position') is not None:
                placed = True
                # Where it is from birth on (a jump on its first step would lay a trail).
                for e_ in (env, uenv):
                    try:
                        v = b.expr(spec['position'], e_)
                        n = b.node('setPosition')
                        if e_ is env:
                            at_birth(n)
                        b.wire(v, n, 'position')
                    except Unsupported as e:
                        b.dropped.append('position: %s' % e)
                break
        if not placed and not b.event_born:
            # A layer with no position of its own stands at the effect's transform: where
            # a missile's head is now (what it kicks is born there; 0 for one that stays).
            for e_ in (env, uenv):
                n = b.node('setPosition')
                if e_ is env:
                    at_birth(n)
                b.wire(b.expr(('call', 'xform_l2w_f_masked', [('num', 0.0), ('num', 17.0)]), e_), n, 'position')
    elif pos_field in assigned:
        for e_, label in ((env, 'start position'), (uenv, 'position')):
            if pos_field in e_ and (e_ is env or uenv[pos_field] != env.get(pos_field)):
                n = b.node('setPosition')
                if e_ is env:
                    at_birth(n)
                b.wire(e_[pos_field], n, 'position')

    for spec in emits:
        n = b.node('emit', {'when': spec['when']})
        b.nodes[-1]['id'] = spec['id']
        n = spec['id']
        if 'count' in spec:
            b.wire(('lit', [max(spec['count'], 0.0)]), n, 'count')
        if 'interval' in spec and 'curve' in spec:
            shape = spec['curve']
            keys = [{'t': round(k / (len(shape) - 1), 4), 'v': [round(v, 4)]} for k, v in enumerate(shape)]
            c = b.node('curve', {'curve': {'keys': keys}})
            k = b.op('max', ('out', c, 'value'), ('lit', [1e-4]))
            b.wire(b.op('divide', ('lit', [spec['interval']]), k), n, 'interval')
        elif 'interval' in spec:
            b.wire(('lit', [spec['interval']]), n, 'interval')
        if spec.get('condition') is not None:
            try:
                b.wire(b.expr(spec['condition'], uenv), n, 'condition')
            except Unsupported as e:
                b.dropped.append('event condition: %s' % e)

    if renderer is not None and renderer['class'] == 'Ribbon':
        draw = b.node('ribbon', {
            'texture': texture_asset(renderer['properties'].get('Diffuse.DiffuseMap')) or FALLBACK_TEXTURE,
            'blend': 'alpha' if str(renderer['properties'].get('Transparent.Type', '')).startswith('AlphaBlend') else 'additive',
        })
        pins = {'Position': ('position', SCALE), 'Size': ('width', SCALE * 2), 'Color': ('colour', None),
                'TextureU': ('along', None), 'Enabled': ('enabled', None)}
        for inp in renderer['particle_inputs']:
            target = pins.get(inp['input'])
            if not target or inp['field'] not in uenv:
                continue
            pin, scale = target
            v = uenv[inp['field']]
            if scale:
                v = b.binop('*', v, ('lit', [scale]))
            b.wire(v, draw, pin)
        return b
    if renderer is not None:
        props = renderer['properties']
        distort = 'Distortion' in renderer['features_on'] and 'Distortion.DistortionMap' in props
        draw = b.node('billboard', {
            'texture': texture_asset(props.get('Distortion.DistortionMap' if distort else 'Diffuse.DiffuseMap'))
            or FALLBACK_TEXTURE,
            # The dump writes enums as "PlaneAligned (5)".
            'mode': MODES.get(str(props.get('BillboardingMode', '')).split(' ')[0], 'camera'),
            'blend': 'distort' if distort else
            'alpha' if str(props.get('Transparent.Type', '')).startswith('AlphaBlend') else 'additive',
            'soft': round(float(renderer['properties'].get('SoftParticles.SoftnessDistance', 0) or 0) * SCALE, 3)
            if 'SoftParticles' in renderer['features_on'] else 0,
        })
        sub = renderer['properties'].get('Atlas.SubDiv')
        if 'Atlas' in renderer['features_on'] and isinstance(sub, (list, tuple)) and len(sub) == 2 and sub != [1, 1]:
            next(x for x in b.nodes if x['id'] == draw)['inputs']['flipbook'] = [sub[0], sub[1]]
        # PopcornFX's billboard Size is a radius; ours is the full width.
        pins = {'Position': ('position', SCALE), 'Size': ('size', SCALE * 2), 'Color': ('colour', None),
                'Rotation': ('rotation', None), 'Axis': ('axis', SCALE), 'NormalAxis': ('axis', SCALE),
                'TextureID': ('frame', None), 'Enabled': ('enabled', None)}
        inputs = {i['input'] for i in renderer['particle_inputs']}
        plane = next(x for x in b.nodes if x['id'] == draw)['inputs']['mode'] == 'plane'
        # One axis pin: a flat sprite's normal, else the stretch axis.
        skip = {'Axis'} if plane and 'NormalAxis' in inputs else {'NormalAxis'} if 'Axis' in inputs else set()
        for inp in renderer['particle_inputs']:
            target = pins.get(inp['input'])
            if not target or inp['field'] not in uenv or inp['input'] in skip:
                continue
            pin, scale = target
            v = uenv[inp['field']]
            if scale:
                v = b.binop('*', v, ('lit', [scale]))
            b.wire(v, draw, pin)
    return b


def arrange(nodes, wires):
    """Columns by dependency, as the editor's Arrange does."""
    index = {n['id']: i for i, n in enumerate(nodes)}
    column = [0] * len(nodes)
    for _ in range(len(nodes)):
        moved = False
        for w in wires:
            a, b = index.get(w['from'].rsplit('.', 1)[0]), index.get(w['to'].rsplit('.', 1)[0])
            if a is not None and b is not None and column[b] < column[a] + 1:
                column[b] = column[a] + 1
                moved = True
        if not moved:
            break
    heights = {}
    for i, n in enumerate(nodes):
        y = heights.get(column[i], 0)
        n['at'] = [column[i] * 266, y]
        heights[column[i]] = y + 300


def import_effect(path, name, sequences=None):
    pk = pkb_dump.PKB(path)
    analysis = pkb_dump.analyse(pk)
    attributes = {}
    for a in analysis.get('attributes', []):
        default = a.get('default') or [0.0]
        width = {'float': 1, 'float2': 2, 'float3': 3, 'float4': 4}.get(str(a.get('type')), 1)
        values = []
        for x in default[:width]:
            try:
                x = float(x)
            except (TypeError, ValueError):
                x = 0.0
            values.append(x if math.isfinite(x) else 0.0)
        attributes[a['name']] = values or [0.0]
    plan = Plan(analysis)
    layers = analysis['layers']
    births = {i: plan.birth(i) for i in range(len(layers))}
    # Built: renderer layers, and layers that emit to others (their motion carries events).
    built = [i for i in range(len(layers)) if births[i] is not None
             and (layers[i]['renderers'] or i in plan.emits) and not plan.foldable(i)]
    # Emitters of built children must be built too.
    for i in list(built):
        b = births[i]
        if b[0] == 'event' and b[1] not in built:
            built.append(b[1])
    built.sort()
    ids, used = {}, set()
    for i in built:
        label = layers[i].get('node') or 'layer%d' % i
        base = re.sub(r'[^a-z0-9]+', '-', label.lower()).strip('-') or 'layer%d' % i
        ident, n = base, 1
        while ident in used:
            n += 1
            ident = '%s-%d' % (base, n)
        used.add(ident)
        ids[i] = ident
    graphs, report = [], []
    longest = 0.0
    for i, layer in enumerate(layers):
        label = layer.get('node') or 'layer%d' % i
        if i not in built:
            if layer['renderers']:
                report.append({'layer': label, 'status': 'skipped', 'why': 'born of nothing the importer follows'})
            continue
        try:
            b = transcribe(analysis, i, births[i], plan, ids, attributes)
        except Unsupported as e:
            report.append({'layer': label, 'status': 'skipped', 'why': str(e)})
            continue
        arrange(b.nodes, b.wires)
        if births[i][0] == 'start' and births[i][1] is not None:
            longest = max(longest, births[i][2])
        graphs.append({'id': ids[i], 'name': label, 'graph': {'nodes': b.nodes, 'wires': b.wires}})
        report.append({'layer': label, 'status': 'partial' if b.dropped else 'ok', 'dropped': b.dropped})
    effect = {
        'id': 'effect.wc3.' + name,
        'name': 'WC3 ' + name.replace('-', ' '),
        'description': 'Imported from Warcraft III: Reforged (local lab only).',
        'durationTicks': max(8, int(math.ceil((longest or 1.0) * 40))),
        'loop': any(b is not None and b[0] == 'start' and b[1] is not None and b[2] == 0.0
                    for b in births.values()),
        'graphs': graphs,
    }
    # Warcraft plays an effect through its model: it emits during the model's Birth and
    # Stand sequences and stops at Death, so those are its length (a looping Stand loops).
    playing = [q for q in sequences or [] if not q[0].lower().startswith(('death', 'decay'))]
    if playing:
        ms = sum(max(0, end - start) for _, start, end, _ in playing)
        effect['durationTicks'] = max(8, int(math.ceil(ms / 25.0)))
        effect['loop'] = any(q[0].lower().startswith('stand') and not q[3] for q in playing)
    return effect, report


def main(argv):
    if len(argv) < 2:
        print(__doc__)
        return 2
    report_path = None
    if '--report' in argv:
        k = argv.index('--report')
        report_path = argv[k + 1]
        argv = argv[:k] + argv[k + 2:]
    models = {}
    if '--models' in argv:
        k = argv.index('--models')
        with open(argv[k + 1]) as f:
            models = json.load(f)
        argv = argv[:k] + argv[k + 2:]
    out_dir, files = argv[0], argv[1:]
    os.makedirs(out_dir, exist_ok=True)
    reports = {}
    for path in files:
        stem = os.path.basename(path).rsplit('.', 1)[0]
        name = re.sub(r'[^a-z0-9]+', '-', stem.lower()).strip('-')
        try:
            effect, report = import_effect(path, name, models.get(stem.split(':')[-1]))
        except Exception as e:  # a file the dumper cannot read
            reports[name] = [{'layer': '*', 'status': 'skipped', 'why': 'unreadable: %s' % e}]
            continue
        reports[name] = report
        if effect['graphs']:
            with open(os.path.join(out_dir, name + '.json'), 'w') as f:
                json.dump(effect, f, indent=1)
    if report_path:
        with open(report_path, 'w') as f:
            json.dump(reports, f, indent=1)
    layers = [r for rs in reports.values() for r in rs]
    count = {s: sum(1 for r in layers if r['status'] == s) for s in ('ok', 'partial', 'skipped')}
    print('effects %d, layers %d: ok %d, partial %d, skipped %d' % (
        len(reports), len(layers), count['ok'], count['partial'], count['skipped']))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
