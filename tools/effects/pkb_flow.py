"""Converts Warcraft III: Reforged's PopcornFX effects to our authoring nodes (D115), for the
local lab only (the effects are Blizzard's: target/wc3-lab, never committed or shipped).

    "$PY" -I tools/effects/pkb_flow.py OUT_DIR FILE.pkb [...] [--models SEQUENCES.json]
        [--report REPORT.json] [--jobs N]
    "$PY" -I tools/effects/pkb_flow.py --convert EFFECT.json [...]   (rewrites primitive graphs in place)

`$PY` is a Python with numpy (Blender's). Each effect is first transcribed into compiler
primitives by `pkb_import` (its scripts statement by statement), then every layer is run on
the CPU (`Sim`: 96 particles at the scripts' own 1/60 s step) and the authoring nodes are
fitted to what its particles do: where they are born (Shape), how they leave (Launch), the
forces on them (Physics, Orbit, Turbulence), their size, colour, spin and frames over life,
the events they fire and the layers born of them. Close, not exact: what a fixed node
cannot say (a script's own condition) is approximated and reported.
"""
import json
import math
import os
import re
import sys
from concurrent.futures import ProcessPoolExecutor

import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import pkb_import as imp  # noqa: E402

SCALE = imp.SCALE
DT = imp.STEP
N = 96
#: The longest a layer is simulated, seconds (particles living forever: until the effect ends).
HORIZON = 5.0
#: Where the target stands in the simulation (beams and paths), metres.
TARGET = np.array([12.0, 1.0, 0.0])
CATALOGUE = os.path.join(os.path.dirname(os.path.abspath(__file__)), '..', '..', 'crates', 'fx', 'src', 'catalogue.rs')


def catalogue_defaults():
    """Each primitive's literal input defaults, read from the catalogue."""
    text = open(CATALOGUE).read()
    out = {}
    for block in re.split(r'\n\s*NodeDef \{', text)[1:]:
        m = re.search(r'kind: "(\w+)"', block)
        if not m:
            continue
        pins = {}
        for p in re.finditer(r'(?:input|setting)\("(\w+)",\s*[\w:]+,\s*Numbers\(&\[([^\]]*)\]\)\)', block):
            pins[p.group(1)] = [float(x) for x in p.group(2).split(',') if x.strip()]
        out[m.group(1)] = pins
    return out


DEFAULTS = catalogue_defaults()


# ---------------------------------------------------------------------------------------
# Running a primitive layer
# ---------------------------------------------------------------------------------------

def w(a, n):
    """`a` (particles × width) as width `n`: a number fills it, wider ones are cut."""
    if a.shape[1] == n:
        return a
    if a.shape[1] == 1:
        return np.repeat(a, n, axis=1)
    if a.shape[1] > n:
        return a[:, :n]
    return np.concatenate([a, np.zeros((a.shape[0], n - a.shape[1]))], axis=1)


def pair(a, b):
    if a.shape[1] == b.shape[1]:
        return a, b
    if a.shape[1] == 1 or b.shape[1] == 1:
        n = max(a.shape[1], b.shape[1])
        return w(a, n), w(b, n)
    n = min(a.shape[1], b.shape[1])
    return a[:, :n], b[:, :n]


def hermite(keys, x):
    """A curve's value at `x` (particles × 1) → particles × components."""
    comps = len(keys[0]['v'])
    out = np.zeros((x.shape[0], comps))
    x = np.clip(x[:, 0], 0.0, 1.0)
    ts = [k['t'] for k in keys]
    for i in range(x.shape[0]):
        t = x[i]
        if t <= ts[0]:
            out[i] = keys[0]['v'][:comps]
            continue
        if t >= ts[-1]:
            out[i] = keys[-1]['v'][:comps]
            continue
        j = max(0, np.searchsorted(ts, t) - 1)
        a, b = keys[j], keys[j + 1]
        span = max(b['t'] - a['t'], 1e-9)
        u = (t - a['t']) / span
        p0, p1 = np.array(a['v'][:comps]), np.array(b['v'][:comps])
        m0 = np.array(a.get('out', [0.0] * comps)[:comps]) if 'out' in a else p1 - p0
        m1 = np.array(b.get('in', [0.0] * comps)[:comps]) if 'in' in b else p1 - p0
        u2, u3 = u * u, u * u * u
        out[i] = (2 * u3 - 3 * u2 + 1) * p0 + (u3 - 2 * u2 + u) * m0 + (-2 * u3 + 3 * u2) * p1 + (u3 - u2) * m1
    return out


def rgb_to_hsv(c):
    r, g, b = c[:, 0], c[:, 1], c[:, 2]
    hi, lo = np.maximum(r, np.maximum(g, b)), np.minimum(r, np.minimum(g, b))
    d = hi - lo
    h = np.zeros_like(hi)
    safe = np.where(d > 1e-6, d, 1.0)
    h = np.where(hi == r, (g - b) / safe, np.where(hi == g, (b - r) / safe + 2.0, (r - g) / safe + 4.0))
    h = np.where(d > 1e-6, np.mod(h / 6.0, 1.0), 0.0)
    s = np.where(hi > 1e-6, d / np.where(hi > 1e-6, hi, 1.0), 0.0)
    return np.stack([h, s, hi], axis=1)


def hsv_to_rgb(c):
    h, s, v = np.mod(c[:, 0], 1.0), np.clip(c[:, 1], 0, 1), np.maximum(c[:, 2], 0)
    k = np.array([1.0, 2.0 / 3.0, 1.0 / 3.0])
    p = np.abs(np.mod(h[:, None] + k, 1.0) * 6.0 - 3.0)
    return v[:, None] * (1.0 + (np.clip(p - 1.0, 0, 1) - 1.0) * s[:, None])


class Sim:
    """One primitive layer's particles, stepped as the compiled program would."""

    def __init__(self, graph, effect_length, loop, seed=1):
        self.nodes = {n['id']: n for n in graph['nodes']}
        self.order = [n['id'] for n in graph['nodes']]
        self.feeds = {}
        for wire in graph.get('wires', []):
            self.feeds[wire['to']] = wire['from']
        self.rng = np.random.default_rng(seed)
        self.stored = {}
        self.length, self.loop = effect_length, loop
        self.state = {}
        self.pos = np.zeros((N, 3))
        self.age = np.zeros(N)
        self.life = np.ones(N)
        self.birth_time = np.zeros(N)
        self.memo = {}

    def kinds(self, kind):
        return [self.nodes[i] for i in self.order if self.nodes[i]['kind'] == kind]

    def lit(self, v):
        if isinstance(v, (int, float)):
            return np.full((N, 1), float(v))
        if isinstance(v, list) and v and all(isinstance(x, (int, float)) for x in v):
            return np.tile(np.array(v, float), (N, 1))
        return np.zeros((N, 1))

    def inp(self, node, pin, default=None):
        src = self.feeds.get('%s.%s' % (node['id'], pin))
        if src is not None:
            nid, out = src.rsplit('.', 1)
            return self.val(nid, out)
        if pin in node.get('inputs', {}):
            return self.lit(node['inputs'][pin])
        d = DEFAULTS.get(node['kind'], {}).get(pin)
        if d is not None:
            return self.lit(d)
        return self.lit(default if default is not None else 0.0)

    def text(self, node, name, default=''):
        v = node.get('inputs', {}).get(name, default)
        return v if isinstance(v, str) else default

    def keep(self, nid, make):
        if nid not in self.stored:
            self.stored[nid] = make()
        return self.stored[nid]

    def val(self, nid, pin='value'):
        key = (nid, pin)
        if key in self.memo:
            return self.memo[key]
        node = self.nodes[nid]
        v = self.compute(node, pin)
        if v.ndim == 1:
            v = v[:, None]
        self.memo[key] = v
        return v

    def compute(self, node, pin):
        k, nid = node['kind'], node['id']
        i = lambda p, d=None: self.inp(node, p, d)  # noqa: E731
        u = lambda: self.rng.random((N, 1))  # noqa: E731
        if k == 'random':
            def make():
                lo, hi = pair(i('min'), i('max'))
                return lo + self.rng.random(lo.shape) * (hi - lo)
            return self.keep(nid, make)
        if k == 'randomDirection':
            def make():
                y = self.rng.random(N) * 2 - 1
                a = self.rng.random(N) * 2 * math.pi
                r = np.sqrt(np.maximum(1 - y * y, 0))
                return np.stack([r * np.cos(a), y, r * np.sin(a)], 1)
            return self.keep(nid, make)
        if k in ('cylinder', 'sphere', 'box', 'ellipsoid'):
            def make():
                if k == 'box':
                    return (self.rng.random((N, 3)) - 0.5) * w(i('size'), 3)
                inner = np.clip(i('inner')[:, 0], 0, 1)
                if k == 'cylinder':
                    a = self.rng.random(N) * 2 * math.pi
                    r = i('radius')[:, 0] * np.sqrt(inner ** 2 + (1 - inner ** 2) * self.rng.random(N))
                    y = self.rng.random(N) * i('height')[:, 0]
                    return np.stack([r * np.cos(a), y, r * np.sin(a)], 1)
                yy = self.rng.random(N) * 2 - 1
                a = self.rng.random(N) * 2 * math.pi
                rr = np.sqrt(np.maximum(1 - yy * yy, 0))
                d = np.stack([rr * np.cos(a), yy, rr * np.sin(a)], 1)
                s = (inner ** 3 + (1 - inner ** 3) * self.rng.random(N)) ** (1 / 3)
                if k == 'sphere':
                    return d * (i('radius')[:, 0] * s)[:, None]
                return d * s[:, None] * w(i('radius'), 3)
            return self.keep(nid, make)
        if k == 'attribute':
            v = self.state.get(self.text(node, 'name'), np.zeros((N, 4)))
            return {'xyz': v[:, :3], 'x': v[:, :1]}.get(pin, v)
        if k == 'lifeRatio':
            return (self.age / self.life)[:, None]
        if k == 'life':
            return self.life[:, None]
        if k == 'age':
            return self.age[:, None]
        if k == 'effectTime':
            return (self.birth_time + self.age)[:, None]
        if k == 'effectRunning':
            # The steady state: ending is fitted from the graph (Ends with effect, Emit at
            # the end), so lives and rates are measured as they are while it plays.
            return np.ones((N, 1))
        if k == 'emitter':
            return np.zeros((N, 3))
        if k == 'target':
            return np.tile(TARGET / (1.0 if pin == 'position' else 1e9), (N, 1)) if pin == 'position' else np.zeros((N, 3))
        if k == 'teamColour':
            # White: the colour is fitted untinted, and Colour over life's `team` tints it.
            return np.ones((N, 4))
        if k == 'parent':
            return np.ones((N, 1)) if pin == 'alive' else np.zeros((N, 3))
        if k == 'event':
            if pin in ('order', 'share'):
                return self.keep(nid + '.' + pin, u)
            if pin == 'colour':
                return np.ones((N, 4))
            if pin == 'value':
                return np.zeros((N, 4))
            return np.zeros((N, 3))
        if k == 'position':
            return self.pos.copy()
        if k == 'velocity':
            return np.zeros((N, 3))
        if k == 'spawnIndex':
            return np.arange(N, dtype=float)[:, None]
        if k in ('multiply', 'add', 'subtract', 'divide', 'min', 'max', 'and', 'or'):
            a, b = pair(i('a'), i('b'))
            if k == 'multiply':
                return a * b
            if k == 'add':
                return a + b
            if k == 'subtract':
                return a - b
            if k == 'divide':
                return np.where(b != 0, a / np.where(b != 0, b, 1), 0.0)
            if k == 'min':
                return np.minimum(a, b)
            if k == 'max':
                return np.maximum(a, b)
            if k == 'and':
                return ((a > 0.5) & (b > 0.5)).astype(float)
            return ((a > 0.5) | (b > 0.5)).astype(float)
        if k == 'compare':
            a, b = pair(i('a'), i('b'))
            op = self.text(node, 'op', '<') or '<'
            r = {'<': a < b, '<=': a <= b, '>': a > b, '>=': a >= b, '==': a == b, '!=': a != b}.get(op, a < b)
            return r[:, :1].astype(float)
        if k == 'select':
            a, b = pair(i('a'), i('b'))
            return np.where(i('condition')[:, :1] > 0.5, a, b)
        if k == 'not':
            return (i('x') <= 0.5).astype(float)
        if k == 'negate':
            return -i('x')
        if k == 'exp':
            return np.exp(np.clip(i('x'), -50, 50))
        if k == 'sqrt':
            return np.sqrt(np.maximum(i('x'), 0))
        if k == 'fract':
            return np.mod(i('x'), 1.0)
        if k == 'clamp':
            x = i('x')
            return np.clip(x, w(i('min', 0.0), x.shape[1]), w(i('max', 1.0), x.shape[1]))
        if k == 'length':
            return np.linalg.norm(i('x'), axis=1, keepdims=True)
        if k == 'normalize':
            x = i('x')
            n = np.linalg.norm(x, axis=1, keepdims=True)
            return np.where(n > 1e-9, x / np.where(n > 1e-9, n, 1), 0)
        if k == 'dot':
            a, b = pair(i('a'), i('b'))
            return np.sum(a * b, axis=1, keepdims=True)
        if k == 'lerp':
            a, b = pair(i('a'), i('b'))
            t = i('t')
            return a + (b - a) * (t[:, :1] if t.shape[1] != a.shape[1] else t)
        if k == 'split':
            v = i('value')
            c = 'xyzw'.index(pin) if pin in 'xyzw' else 0
            return v[:, c:c + 1] if c < v.shape[1] else (v[:, :1] if v.shape[1] == 1 and c == 0 else np.zeros((N, 1)))
        if k == 'makeVector':
            return np.concatenate([i('x')[:, :1], i('y')[:, :1], i('z')[:, :1]], 1)
        if k == 'compose':
            return np.concatenate([i('x')[:, :1], i('y')[:, :1], i('z')[:, :1], i('w', 1.0)[:, :1]], 1)
        if k == 'rotate':
            v, axis, ang = w(i('vector'), 3), w(i('axis'), 3), i('angle')[:, :1]
            n = np.linalg.norm(axis, axis=1, keepdims=True)
            kx = np.where(n > 1e-9, axis / np.where(n > 1e-9, n, 1), np.array([0, 1.0, 0]))
            c, s = np.cos(ang), np.sin(ang)
            return v * c + np.cross(kx, v) * s + kx * np.sum(kx * v, 1, keepdims=True) * (1 - c)
        if k in ('hsvShift', 'rgbToHsv', 'hsvToRgb'):
            col = w(i('colour'), 4)
            if k == 'rgbToHsv':
                return np.concatenate([rgb_to_hsv(col), col[:, 3:]], 1)
            if k == 'hsvToRgb':
                return np.concatenate([hsv_to_rgb(col), col[:, 3:]], 1)
            sh = i('shift')
            sh = np.concatenate([np.zeros((N, 2)), sh], 1) if sh.shape[1] == 1 else w(sh, 3)
            return np.concatenate([hsv_to_rgb(rgb_to_hsv(col) + sh), col[:, 3:]], 1)
        if k == 'curve':
            keys = node.get('inputs', {}).get('curve', {}).get('keys') or [{'t': 0, 'v': [0.0]}]
            x = i('x') if ('%s.x' % nid) in self.feeds else (self.age / self.life)[:, None]
            return hermite(keys, x[:, :1])
        if k == 'groundHit':
            o, d, ln = w(i('origin'), 3), w(i('direction'), 3), i('length')[:, 0]
            dy = np.where(np.abs(d[:, 1]) > 1e-6, d[:, 1], -1e-6)
            t = -o[:, 1] / dy
            hit = (d[:, 1] < -1e-6) & (t >= 0) & (t <= ln)
            out = np.zeros((N, 4))
            out[hit, 1] = 1.0
            out[hit, 3] = np.maximum(t[hit], 1e-4)
            return out
        return np.zeros((N, 1))

    def run(self, birth_times, problems):
        self.birth_time = birth_times
        b = self.kinds('setLife')
        self.life = np.clip(self.inp(b[0], 'life')[:, 0] if b else np.ones(N), 0.01, 1e5)
        nominal = self.life.copy()
        for n in self.kinds('setAttribute'):
            if n.get('atBirth'):
                self.state[self.text(n, 'name')] = w(self.inp(n, 'value'), 4)
        for n in self.kinds('setPosition'):
            if n.get('atBirth'):
                self.pos = w(self.inp(n, 'position'), 3)
        self.memo = {}
        draw = (self.kinds('billboard') + self.kinds('ribbon') or [None])[0]
        span = min(float(np.max(self.life)), HORIZON if self.loop or np.max(self.life) > 100 else max(HORIZON, self.length))
        steps = max(2, int(span / DT) + 1)
        rec = {k: [] for k in ('pos', 'size', 'colour', 'rotation', 'frame', 'axis', 'alive')}
        emits = {n['id']: [] for n in self.kinds('emit')}
        alive = np.ones(N, bool)
        death = np.full(N, np.inf)
        for s in range(steps):
            self.age = np.full(N, s * DT)
            self.memo = {}
            sets = [(self.text(n, 'name'), w(self.inp(n, 'value'), 4))
                    for n in self.kinds('setAttribute') if not n.get('atBirth')] if s else []
            moves = [w(self.inp(n, 'position'), 3) for n in self.kinds('setPosition') if not n.get('atBirth')] if s else []
            kills = [self.inp(n, 'condition')[:, 0] > 0.5 for n in self.kinds('kill')] if s else []
            for n in self.kinds('emit'):
                cond = self.inp(n, 'condition', 1.0)[:, 0] > 0.5 if ('%s.condition' % n['id']) in self.feeds or 'condition' in n.get('inputs', {}) else np.ones(N, bool)
                count = float(np.mean(self.inp(n, 'count', 1.0)[:, 0]))
                every = self.inp(n, 'interval', 0.1)[:, 0].copy()
                emits[n['id']].append((cond & alive, count, every))
            for name, v in sets:
                self.state[name] = v
            for v in moves:
                self.pos = v
            if draw is not None:
                pins = {'pos': ('position', 3), 'colour': ('colour', 4), 'rotation': ('rotation', 1),
                        'frame': ('frame', 1), 'axis': ('axis', 3)}
                pins['size'] = ('width' if draw['kind'] == 'ribbon' else 'size', 1)
                for key, (pin, width) in pins.items():
                    if key == 'pos' and ('%s.position' % draw['id']) not in self.feeds:
                        rec[key].append(self.pos * SCALE)
                    else:
                        rec[key].append(w(self.inp(draw, pin), width).copy())
            else:
                rec['pos'].append(self.pos * SCALE)
            for kc in kills:
                newly = kc & alive
                death[newly] = s * DT
                alive &= ~kc
            ended = (s * DT >= self.life) & alive
            death[ended] = s * DT
            alive &= ~ended
            rec['alive'].append(alive.copy())
            if not alive.any():
                break
        death = np.minimum(death, (len(rec['alive']) - 1) * DT)
        out = {k: np.array(v) for k, v in rec.items() if v}
        out['death'] = np.maximum(death, DT)
        out['nominal'] = nominal
        out['emits'] = emits
        out['draw'] = draw
        return out


# ---------------------------------------------------------------------------------------
# Fitting the authoring nodes
# ---------------------------------------------------------------------------------------

def r3(x):
    if isinstance(x, (list, tuple, np.ndarray)):
        return [r3(v) for v in x]
    x = float(x)
    if not math.isfinite(x):
        return 0.0
    if x == 0 or abs(x) >= 100:
        return round(x, 1)
    return float('%.3g' % x)


def p(a, q):
    return float(np.percentile(a, q)) if len(a) else 0.0


def over_life(series, death, samples=17):
    """`series` (steps × particles × c) at each particle's life share: samples × particles × c."""
    steps = series.shape[0]
    t = np.linspace(0, 1, samples)
    idx = np.clip(np.round(t[:, None] * death[None, :] / DT).astype(int), 0, steps - 1)
    return series[idx, np.arange(series.shape[1])[None, :]], t


def simplify_keys(t, values, tol):
    """Douglas–Peucker on curve samples: the keys a linear curve needs within `tol`."""
    keep = {0, len(t) - 1}

    def split(a, b):
        if b <= a + 1:
            return
        ta, tb = t[a], t[b]
        worst, at = 0.0, None
        for i in range(a + 1, b):
            u = (t[i] - ta) / (tb - ta)
            line = values[a] + (values[b] - values[a]) * u
            err = float(np.max(np.abs(values[i] - line)))
            if err > worst:
                worst, at = err, i
        if at is not None and worst > tol:
            keep.add(at)
            split(a, at)
            split(at, b)

    split(0, len(t) - 1)
    return [{'t': r3(t[i]), 'v': r3(values[i])} for i in sorted(keep)]


def fit_shape(p0):
    c = np.median(p0, axis=0)
    d = p0 - c
    rxz = np.hypot(d[:, 0], d[:, 2])
    r = np.linalg.norm(d, axis=1)
    if p(r, 98) < 0.05:
        return {'shape': 'point', 'offset': r3(c)}, None
    yr = p(d[:, 1], 98) - p(d[:, 1], 2)
    rad = p(rxz, 98)
    if yr < 0.1 * max(rad, 1e-3):
        inner = p(rxz, 2) / max(rad, 1e-6)
        off = [c[0], float(np.median(p0[:, 1])), c[2]]
        if inner > 0.85:
            return {'shape': 'ring', 'radius': r3(rad), 'offset': r3(off)}, 'ring'
        return {'shape': 'disc', 'radius': r3(rad), 'inner': r3(inner), 'offset': r3(off)}, 'disc'
    sx, sy, sz = np.std(d[:, 0]), np.std(d[:, 1]), np.std(d[:, 2])
    box = max(p(np.abs(d[:, 0]), 99), p(np.abs(d[:, 2]), 99))
    if p(rxz, 99) > 1.25 * box and box > 0:
        size = [2 * p(np.abs(d[:, i]), 99) for i in range(3)]
        return {'shape': 'box', 'size': r3(size), 'offset': r3(c)}, 'box'
    ext = np.array([max(p(np.abs(d[:, i]), 99), 1e-6) for i in range(3)])
    rn = np.linalg.norm(d / ext, axis=1)
    if ext.max() > 1.3 * ext.min() and p(rn, 99) < 1.15:
        # Inside an ellipsoid (a box's corners and a cylinder's rims reach further out).
        rr = float(ext.max())
        s = {'shape': 'ellipsoid', 'radius': r3(rr), 'size': r3(ext / rr), 'offset': r3(c)}
        inner = p(rn, 2)
        if inner > 0.9:
            s['fill'] = 'surface'
        elif inner > 0.3:
            s['inner'] = r3(inner)
        return s, 'ellipsoid'
    if max(sx, sy, sz) < 1.4 * min(sx, sy, sz):
        rr = p(r, 98)
        inner = p(r, 2) / max(rr, 1e-6)
        if inner > 0.9:
            return {'shape': 'sphere', 'radius': r3(rr), 'fill': 'surface', 'offset': r3(c)}, 'sphere'
        return {'shape': 'sphere', 'radius': r3(rr), 'inner': r3(inner), 'offset': r3(c)}, 'sphere'
    ylo = p(p0[:, 1], 1)
    inner = p(rxz, 2) / max(rad, 1e-6)
    s = {'shape': 'cylinder', 'radius': r3(rad), 'height': r3(yr), 'offset': r3([c[0], ylo, c[2]])}
    if inner > 0.3:
        s['inner'] = r3(inner)
    return s, 'cylinder'


def fit_launch(v0, p0, shaped):
    s = np.linalg.norm(v0, axis=1)
    if p(s, 95) < 0.1:
        return None
    u = v0 / np.maximum(s, 1e-9)[:, None]
    speed = [r3(p(s, 5)), r3(p(s, 95))]
    if shaped:
        d = p0 - np.median(p0, axis=0)
        dn = d / np.maximum(np.linalg.norm(d, axis=1), 1e-9)[:, None]
        if np.median(np.sum(u * dn, axis=1)) > 0.9:
            return {'direction': 'outward', 'speed': speed}
    m = np.mean(u, axis=0)
    if np.linalg.norm(m) < 0.35:
        return {'direction': 'random', 'speed': speed}
    axis = m / np.linalg.norm(m)
    angle = math.degrees(p(np.arccos(np.clip(u @ axis, -1, 1)), 90))
    if angle < 3:
        if axis[1] > 0.99:
            return {'direction': 'up', 'speed': speed}
        return {'direction': 'axis', 'axis': r3(axis), 'speed': speed}
    return {'direction': 'cone', 'axis': r3(axis), 'angle': r3(min(2 * angle, 360)), 'speed': speed}


def fit_motion(pos, alive):
    """Gravity, drag, orbit and turbulence from the paths (steps × particles × 3)."""
    nodes, notes = [], []
    if pos.shape[0] < 4:
        return nodes, notes
    v = (pos[1:] - pos[:-1]) / DT
    a = (v[1:] - v[:-1]) / DT
    ok = alive[2:] & alive[1:-1] & alive[:-2]
    # The scripts' position lags their velocity by a step at birth: fit from the second.
    ok[:2] = False
    vv, aa = v[:-1][ok], a[ok]
    if len(aa) < 8:
        return nodes, notes
    # Orbit: a steady turn about the emitter's up axis.
    xz = pos[:-1, :, [0, 2]]
    ang = np.arctan2(xz[..., 1], xz[..., 0])
    dang = np.angle(np.exp(1j * (ang[1:] - ang[:-1]))) / DT
    rad = np.hypot(xz[..., 0], xz[..., 1])
    okw = alive[1:-1] & alive[:-2] & (rad[1:] > 0.1)
    orbit = None
    if okw.sum() > 8:
        per = np.array([np.median(dang[:, j][okw[:, j]]) for j in range(dang.shape[1]) if okw[:, j].sum() > 2])
        if len(per) and np.median(np.abs(per)) > math.radians(15):
            orbit = [r3(math.degrees(p(-per, 5))), r3(math.degrees(p(-per, 95)))]
            nodes.append(('orbit', {'speed': orbit}))
    # a = g − k·v (least squares over every sample).
    rows = len(aa)
    A = np.zeros((rows * 3, 4))
    for c in range(3):
        A[c::3, c] = 1.0
        A[c::3, 3] = -vv[:, c]
    sol, *_ = np.linalg.lstsq(A, aa.reshape(-1), rcond=None)
    g, k = sol[:3], sol[3]
    if k < 0:
        k, g = 0.0, aa.mean(axis=0)
    if orbit:
        g = np.array([0.0, g[1], 0.0])
    resid = aa - (g[None, :] - k * vv)
    rms_a, rms_r = math.sqrt(np.mean(aa ** 2)), math.sqrt(np.mean(resid ** 2))
    phys = {}
    if np.linalg.norm(g) > 0.3:
        phys['gravity'] = r3(g)
    if k > 0.05:
        phys['drag'] = r3(k)
    if phys:
        nodes.append(('physics', phys))
    if not orbit and rms_a > 1.0 and rms_r > 0.5 * rms_a:
        nodes.append(('turbulence', {'strength': r3(rms_r), 'scale': 2.0, 'speed': 1.0}))
        notes.append('motion approximated by turbulence')
    return nodes, notes


def fit_size(size, death, stretch_axis=None):
    m, t = over_life(size[..., :1], death)
    m = np.abs(m[..., 0])
    scale = np.mean(m, axis=0)
    good = scale > 1e-6
    if not good.any():
        return None
    shape = np.median(m[:, good] / scale[good][None, :], axis=1)
    peak = max(float(np.max(shape)), 1e-6)
    shape = shape / peak
    lo, hi = p(scale[good], 5) * peak, p(scale[good], 95) * peak
    s = {'size': [r3(lo), r3(hi)]}
    if float(np.max(shape) - np.min(shape)) > 0.03:
        s['curve'] = {'keys': [{'t': k['t'], 'v': [k['v']] if not isinstance(k['v'], list) else k['v']}
                               for k in simplify_keys(t, shape[:, None], 0.03)]}
    return s


def fit_colour(colour, death, alpha_blend):
    m, t = over_life(colour, death)
    lum = np.max(m[..., :3], axis=2)
    bright = np.mean(lum, axis=0)
    good = bright > 1e-5
    if not good.any():
        return {'gradient': {'keys': [{'t': 0, 'v': [0, 0, 0, 0]}, {'t': 1, 'v': [0, 0, 0, 0]}]}}
    med = float(np.median(bright[good]))
    rel = bright / max(med, 1e-9)
    spread = (p(rel[good], 5), p(rel[good], 95))
    vary = not alpha_blend and spread[1] / max(spread[0], 1e-6) > 1.15
    div = np.where(good, rel, 1.0) if vary else np.ones_like(rel)
    norm = m.copy()
    norm[..., :3] = m[..., :3] / div[None, :, None]
    grad = np.median(norm, axis=1)
    scale = max(1.0, float(np.max(np.abs(grad))))
    s = {'gradient': {'keys': simplify_keys(t, grad, 0.03 * scale)}}
    if vary:
        s['brightness'] = [r3(spread[0]), r3(spread[1])]
    # Hue spread per particle: their hues around the median where the colour is vivid.
    mid = m[len(t) // 2]
    from_rgb = mid[:, :3]
    hi, lo = from_rgb.max(axis=1), from_rgb.min(axis=1)
    vivid = (hi > 1e-3) & ((hi - lo) / np.maximum(hi, 1e-6) > 0.15)
    if vivid.sum() > 8:
        hsv = rgb_to_hsv(from_rgb[vivid])
        ang = hsv[:, 0] * 2 * np.pi
        centre = np.angle(np.mean(np.exp(1j * ang)))
        off = np.abs(np.angle(np.exp(1j * (ang - centre)))) / (2 * np.pi)
        spread_h = p(off, 95)
        if spread_h > 0.01:
            s['hue'] = r3(spread_h)
    return s


def fit_spin(rot, death):
    a0 = rot[0, :, 0]
    last = np.clip(np.round(death / DT).astype(int), 1, rot.shape[0] - 1)
    a1 = rot[last, np.arange(rot.shape[1]), 0]
    spd = (a1 - a0) / np.maximum(last * DT, DT)
    if np.ptp(a0) < 1 and np.max(np.abs(spd)) < 1:
        return None if abs(np.median(a0)) < 0.5 else {'angle': [r3(np.median(a0))] * 2}
    angle = [0.0, 360.0] if np.ptp(a0) > 300 else [r3(p(a0, 5)), r3(p(a0, 95))]
    s = {'angle': angle}
    if np.max(np.abs(spd)) >= 1:
        s['speed'] = [r3(p(spd, 5)), r3(p(spd, 95))]
    return s


def fit_flipbook(frame, death, sub):
    cols, rows = sub
    fb = {'columns': cols, 'rows': rows}
    f0 = frame[0, :, 0]
    last = np.clip(np.round(death / DT).astype(int), 1, frame.shape[0] - 1)
    f1 = frame[last, np.arange(frame.shape[1]), 0]
    total = cols * rows
    if np.max(np.abs(f1 - f0)) < 0.5:
        lo, hi = int(np.floor(p(f0, 1))), int(np.floor(p(f0, 99)))
        if hi > lo:
            return dict(fb, first=lo, last=hi, play='random')
        return dict(fb, first=lo, last=lo)
    m, t = over_life(frame, death)
    # Over life or at a rate: which spreads less across particles of different lives.
    first, lastf = int(np.floor(p(f0, 5))), int(min(total - 1, np.floor(np.max(frame))))
    count = max(1, lastf - first + 1)
    s = dict(fb, first=first, last=lastf)
    if p(f0, 95) - p(f0, 5) > 2:
        s['start'] = 'random'
    ages = np.arange(frame.shape[0]) * DT
    span = f1 - f0
    per_life = np.std(span / count)
    rate = span / np.maximum(last * DT, DT)
    if np.ptp(death) > 0.05 and np.std(rate) / max(abs(np.mean(rate)), 1e-6) < 0.1 and per_life > 0.05:
        s['play'] = 'rate'
        s['fps'] = r3(np.median(rate))
    else:
        loops = max(1.0, round(float(np.median(span)) / count, 2))
        if loops != 1:
            s['loops'] = r3(loops)
    del ages
    return s


def emit_settings(n, fires, death, at_end=False):
    """An emit primitive as an authoring Emit; conditions read from when they fired
    (`at_end`: the condition is the effect stopping)."""
    when = n.get('inputs', {}).get('when', 'death')
    s = {'when': when}
    if 'interval' in n.get('inputs', {}):
        s['interval'] = r3(n['inputs']['interval'])
    counts = [c for _, c, _ in fires] or [1.0]
    count = float(np.mean(counts))
    s['count'] = [r3(count), r3(count)]
    notes = []
    if at_end and when == 'step':
        s['when'] = 'end'
        return s, notes
    if fires and when in ('interval', 'distance'):
        every = np.array([e for _, _, e in fires])  # steps × particles
        if np.ptp(every[:, 0]) > 1e-6 * max(abs(float(every[0, 0])), 1e-6):
            # The rate changes over the particle's life: a peak and a curve.
            m, t = over_life(every[:, :, None], death)
            rate = 1.0 / np.maximum(np.median(m[..., 0], axis=1), 1e-4)
            peak = float(rate.max())
            s['interval'] = r3(1.0 / peak)
            s['rateCurve'] = {'keys': [{'t': k['t'], 'v': k['v'] if isinstance(k['v'], list) else [k['v']]}
                                       for k in simplify_keys(t, (rate / peak)[:, None], 0.03)]}
    if fires:
        hits = np.array([f for f, _, _ in fires])  # steps × particles
        if not hits.all():
            per = hits.sum(axis=0)
            fired = per > 0
            if when == 'step':
                if np.median(per[fired]) <= 1.5 if fired.any() else True:
                    first = np.argmax(hits, axis=0) * DT / np.maximum(death, DT)
                    s['when'] = 'death' if fired.any() and np.median(first[fired]) > 0.6 else 'birth'
                    if fired.mean() < 0.97:
                        s['chance'] = r3(max(fired.mean(), 0.01))
                else:
                    s['when'] = 'interval'
                    s['interval'] = r3(float(np.mean(death[fired])) / max(float(np.mean(per[fired])), 1.0))
                notes.append('a condition read as %s' % s['when'])
            elif fired.mean() < 0.97:
                s['chance'] = r3(max(fired.mean(), 0.01))
    return s, notes


def upstream(graph, node, pin, kind):
    """Whether `node.pin` is fed, through any wires, by a `kind` node."""
    nodes = {n['id']: n for n in graph['nodes']}
    up = {}
    for wire in graph.get('wires', []):
        up.setdefault(wire['to'], []).append(wire['from'].rsplit('.', 1)[0])
    stack, seen = list(up.get('%s.%s' % (node, pin), [])), set()
    while stack:
        i = stack.pop()
        if i in seen:
            continue
        seen.add(i)
        if nodes.get(i, {}).get('kind') == kind:
            return True
        for w in graph.get('wires', []):
            if w['to'].rsplit('.', 1)[0] == i:
                stack.append(w['from'].rsplit('.', 1)[0])
    return False


def uses(graph, kind):
    return any(n['kind'] == kind for n in graph['nodes'])


def feeds_from(graph, sink_kind, source_kind, at_birth=None):
    """Whether a `sink_kind` node's inputs reach a `source_kind` node."""
    nodes = {n['id']: n for n in graph['nodes']}
    up = {}
    for wire in graph.get('wires', []):
        up.setdefault(wire['to'].rsplit('.', 1)[0], []).append(wire['from'].rsplit('.', 1)[0])
    for n in graph['nodes']:
        if n['kind'] != sink_kind or (at_birth is not None and bool(n.get('atBirth')) != at_birth):
            continue
        seen, stack = set(), [n['id']]
        while stack:
            i = stack.pop()
            if i in seen:
                continue
            seen.add(i)
            if nodes.get(i, {}).get('kind') == source_kind:
                return True
            stack.extend(up.get(i, []))
    return False


def fit_layer(layer, effect_length, loop, seed):
    """One primitive layer → (authoring layers, notes)."""
    g = layer['graph']
    sim = Sim(g, effect_length, loop, seed)
    notes = []
    starts = []
    spawners = []
    for n in g['nodes']:
        ins = n.get('inputs', {})
        if n['kind'] == 'burst':
            spawners.append({'mode': 'burst', 'count': r3(ins.get('count', 1)), 'time': r3(ins.get('time', 0))})
            starts.append(np.full(N, float(ins.get('time', 0))))
        elif n['kind'] == 'rate':
            d = float(ins.get('duration', 0)) or effect_length
            sp = {'mode': 'stream', 'rate': r3(ins.get('rate', 10)), 'delay': r3(ins.get('start', 0)),
                  'duration': r3(ins.get('duration', 0))}
            span = min(d, max(effect_length, 0.1))
            u = sim.rng.random(N)
            if isinstance(ins.get('curve'), dict):
                keys = ins['curve']['keys']
                sp['rateCurve'] = {'keys': [{'t': k['t'], 'v': list(k['v'])} for k in keys]}
                # Births follow the curve: the inverse of its running area.
                grid = np.linspace(0, 1, 65)
                dens = np.maximum(hermite(keys, grid[:, None])[:, 0], 0) + 1e-9
                cum = np.concatenate([[0], np.cumsum((dens[1:] + dens[:-1]) * 0.5)])
                u = np.interp(u * cum[-1], cum, grid)
            spawners.append(sp)
            starts.append(float(ins.get('start', 0)) + u * span)
        elif n['kind'] == 'onEvent':
            spawners.append({'mode': 'event', 'layer': ins.get('layer', ''), 'event': ins.get('event', '')})
            starts.append(sim.rng.random(N))
    if not spawners:
        return [], ['no spawner']
    run = sim.run(starts[0], notes)
    death, nominal = run['death'], run['nominal']
    alive = run['alive']
    born = alive[0] | (death > DT)
    pos = run['pos']
    chain = []
    life_s = nominal if not uses(g, 'kill') else death
    life = [r3(p(life_s, 2)), r3(p(life_s, 98))]
    if life[0] > 100:
        life = [1000.0, 1000.0]
    chain.append(('life', {'seconds': life}))
    p0 = pos[0]
    follows = None
    if uses(g, 'parent'):
        follows = 'parent'
    elif feeds_from(g, 'setPosition', 'emitter', at_birth=False):
        # Stands at the emitter (a missile's head) unless it moves on its own (a star
        # falling from above the emitter).
        moved = np.linalg.norm(pos[-1] - pos[0], axis=1) if pos.shape[0] > 1 else np.zeros(1)
        if p(moved, 90) < 0.05:
            follows = 'emitter'
    beam = uses(g, 'target') and any(n['kind'] == 'event' for n in g['nodes']) and spawners[0]['mode'] == 'event'
    shape, shaped = fit_shape(p0)
    chain.append(('shape', shape))
    if beam:
        chain.append(('beam', {}))
        notes.append('drawn as a beam')
    elif uses(g, 'target'):
        chain.append(('path', {}))
        notes.append('drawn along a path to the target')
    elif follows:
        chain.append(('follow', {'with': follows}))
    else:
        if pos.shape[0] > 2:
            v0 = np.where(alive[1][:, None], (pos[2] - pos[1]) / DT, (pos[1] - pos[0]) / DT)
            launch = fit_launch(v0[born], p0[born], shaped)
            if launch:
                chain.append(('launch', launch))
        motion, more = fit_motion(pos, alive)
        chain += motion
        notes += more
    draw = run['draw']
    if draw is not None:
        alpha_blend = draw.get('inputs', {}).get('blend') == 'alpha'
        size = fit_size(run['size'], death)
        stretch = None
        mode = draw.get('inputs', {}).get('mode', 'camera')
        facing = {'plane': 'ground'}.get(mode, 'camera')
        if mode in ('axis', 'spheroid', 'velocity') and pos.shape[0] > 2:
            v = (pos[1:] - pos[:-1]) / DT
            ax = run['axis'][:-1]
            ok = alive[:-1]
            sp, al = np.linalg.norm(v[ok], axis=1), np.linalg.norm(ax[ok], axis=1)
            moving = sp > 0.2
            if moving.sum() > 8:
                cos = np.sum(v[ok][moving] * ax[ok][moving], axis=1) / (sp[moving] * np.maximum(al[moving], 1e-9))
                if np.median(np.abs(cos)) > 0.9:
                    facing, stretch = 'velocity', float(np.median(al[moving] / sp[moving]))
            if facing == 'camera':
                med = np.median(ax[ok], axis=0) if ok.any() else np.array([0, 1.0, 0])
                facing = 'axis'
                axis_v = r3(med)
        if size:
            if stretch:
                size['stretch'] = r3(stretch)
            chain.append(('sizeOverLife', size))
        colour = fit_colour(run['colour'], death, alpha_blend)
        if uses(g, 'teamColour'):
            colour['team'] = 'on'
        chain.append(('colourOverLife', colour))
        if draw['kind'] == 'billboard':
            spin = fit_spin(run['rotation'], death)
            if spin:
                chain.append(('spin', spin))
            sub = draw.get('inputs', {}).get('flipbook')
            if sub and list(sub) != [1, 1]:
                chain.append(('flipbook', fit_flipbook(run['frame'], death, [int(sub[0]), int(sub[1])])))
    emits = []
    for n in sim.kinds('emit'):
        at_end = upstream(g, n['id'], 'condition', 'effectRunning')
        s, more = emit_settings(n, run['emits'][n['id']], death, at_end)
        notes += more
        emits.append(('emit', s, n['id']))
    chain += emits
    if uses(g, 'effectRunning') and uses(g, 'kill'):
        chain.append(('endsWithEffect', {}))
    if draw is not None:
        ins = draw.get('inputs', {})
        if draw['kind'] == 'ribbon':
            chain.append(('ribbon', {'texture': ins.get('texture', ''), 'blend': ins.get('blend', 'additive')}))
        else:
            b = {'texture': ins.get('texture', ''), 'blend': ins.get('blend', 'additive'), 'facing': facing}
            if facing == 'axis':
                b['axis'] = axis_v
            if ins.get('soft'):
                b['soft'] = ins['soft']
            chain.append(('billboard', b))
    layers = []
    for k, spawner in enumerate(spawners if not emits else spawners[:1]):
        if k and spawner['mode'] != spawners[0]['mode']:
            continue
        layers.append((layer['id'] if not k else '%s-%d' % (layer['id'], k + 1), layer.get('name', layer['id']),
                       merge_steps([('spawner', dict(spawner))] + chain)))
    if len(spawners) > 1 and emits:
        notes.append('%d bursts kept as one' % len(spawners))
    return layers, notes


def merge_steps(chain):
    """Life and the end with the effect are the Spawner's settings, following is the
    Shape's."""
    spawner = chain[0][1]
    out = [chain[0]]
    for step in chain[1:]:
        kind, settings = step[0], step[1]
        if kind == 'life':
            spawner['life'] = settings['seconds']
        elif kind == 'endsWithEffect':
            spawner['ends'] = 'effect'
            if settings.get('fade'):
                spawner['fade'] = settings['fade']
        elif kind == 'follow':
            shape = next((x[1] for x in out if x[0] == 'shape'), None)
            if shape is not None:
                shape['follow'] = settings.get('with', 'parent')
        else:
            out.append(step)
    return out


#: What a hidden sprayer may do and still fold into the layer it feeds.
STILL = {'spawner', 'shape', 'emit'}


def fold_sprayers(graphs):
    """A layer that draws nothing, born of another's events, standing still and firing one
    stream of events at one layer (PopcornFX's spray: blood, footprints, debris) becomes
    that layer's Spawner `spray`: the layer listens to the events it listened to and
    streams `rate` a second from each for the sprayer's life."""
    by_id = {g['id']: g for g in graphs}
    listeners = {}
    for g in graphs:
        sp = g['graph']['nodes'][0]
        if sp['kind'] == 'spawner' and sp.get('inputs', {}).get('mode') == 'event':
            listeners.setdefault(sp['inputs'].get('layer'), []).append(g)
    gone = set()
    for g in graphs:
        nodes = g['graph']['nodes']
        sp = nodes[0].get('inputs', {})
        kinds = [n['kind'] for n in nodes]
        if sp.get('mode') != 'event' or not set(kinds) <= STILL or kinds.count('emit') != 1:
            continue
        kids = listeners.get(g['id'], [])
        if len(kids) != 1 or kids[0]['id'] == g['id']:
            continue
        shape = next((n.get('inputs', {}) for n in nodes if n['kind'] == 'shape'), {})
        if shape.get('shape', 'point') != 'point' or np.linalg.norm(shape.get('offset', [0, 0, 0])) > 0.1:
            continue
        emit = next(n for n in nodes if n['kind'] == 'emit')
        e = emit.get('inputs', {})
        if e.get('when') != 'interval' or kids[0]['graph']['nodes'][0]['inputs'].get('event') != emit['id']:
            continue
        child = kids[0]['graph']['nodes'][0]['inputs']
        life = sp.get('life', [1.0, 1.0])
        count = float(np.mean(e.get('count', [1.0, 1.0])))
        rate = count * float(e.get('chance', 1.0)) / max(float(e.get('interval', 0.1)), 1e-4)
        child['layer'] = sp.get('layer', '')
        child['event'] = sp.get('event', '')
        child['spray'] = r3(float(np.mean(life)))
        child['rate'] = r3(rate)
        if e.get('rateCurve'):
            child['rateCurve'] = e['rateCurve']
        gone.add(g['id'])
    return [g for g in graphs if g['id'] not in gone]


def as_graph(chain):
    nodes, wires = [], []
    for i, step in enumerate(chain):
        kind, settings = step[0], step[1]
        nid = step[2] if len(step) > 2 else '%s%d' % (kind, i)
        n = {'id': nid, 'kind': kind, 'at': [0.0, i * 150.0]}
        if settings:
            n['inputs'] = settings
        nodes.append(n)
        if i:
            wires.append({'from': '%s.out' % nodes[i - 1]['id'], 'to': '%s.in' % nid})
    return {'nodes': nodes, 'wires': wires}


def convert(args):
    path, name, sequences = args
    try:
        effect, report = imp.import_effect(path, name, sequences)
    except Exception as e:  # noqa: BLE001 - a file the dumper cannot read
        return name, None, [{'layer': '*', 'status': 'skipped', 'why': 'unreadable: %s' % e}]
    length = effect['durationTicks'] / 40.0
    graphs, out = [], []
    for i, layer in enumerate(effect['graphs']):
        try:
            made, notes = fit_layer(layer, length, effect['loop'], seed=i + 1)
        except Exception as e:  # noqa: BLE001 - reported per layer
            out.append({'layer': layer['id'], 'status': 'skipped', 'why': 'fit: %s' % e})
            continue
        for lid, lname, chain in made:
            graphs.append({'id': lid, 'name': lname, 'graph': as_graph(chain)})
        out.append({'layer': layer['id'], 'status': 'approximate' if notes else 'ok', 'notes': notes})
    effect['graphs'] = fold_sprayers(graphs)
    return name, effect, report + out


def convert_file(path):
    """An effect file of primitive graphs rewritten in authoring nodes, in place."""
    effect = json.load(open(path))
    length = effect.get('durationTicks', 40) / 40.0
    graphs, notes_all = [], []
    for i, layer in enumerate(effect.get('graphs', [])):
        if any(n['kind'] == 'spawner' for n in layer['graph']['nodes']):
            graphs.append(layer)
            continue
        made, notes = fit_layer(layer, length, effect.get('loop', False), seed=i + 1)
        for lid, lname, chain in made:
            graphs.append({'id': lid, 'name': lname, 'graph': as_graph(chain)})
        notes_all += ['%s: %s' % (layer['id'], n) for n in notes]
    effect['graphs'] = fold_sprayers(graphs)
    with open(path, 'w') as fh:
        json.dump(effect, fh, indent=1)
        fh.write('\n')
    return notes_all


def main(argv):
    if len(argv) >= 2 and argv[0] == '--convert':
        for path in argv[1:]:
            notes = convert_file(path)
            print(os.path.basename(path), 'converted', '; '.join(notes))
        return 0
    if len(argv) < 2:
        print(__doc__)
        return 2
    opts = {}
    for flag in ('--models', '--report', '--jobs'):
        if flag in argv:
            k = argv.index(flag)
            opts[flag] = argv[k + 1]
            argv = argv[:k] + argv[k + 2:]
    models = json.load(open(opts['--models'])) if '--models' in opts else {}
    out_dir, files = argv[0], argv[1:]
    os.makedirs(out_dir, exist_ok=True)
    work = []
    for f in files:
        stem = os.path.basename(f).rsplit('.', 1)[0]
        name = re.sub(r'[^a-z0-9]+', '-', stem.lower()).strip('-')
        work.append((f, name, models.get(stem.split(':')[-1])))
    reports = {}
    jobs = int(opts.get('--jobs', os.cpu_count() or 4))
    with ProcessPoolExecutor(max_workers=jobs) as pool:
        for name, effect, report in pool.map(convert, work, chunksize=4):
            reports[name] = report
            if effect and effect['graphs']:
                with open(os.path.join(out_dir, name + '.json'), 'w') as fh:
                    json.dump(effect, fh, indent=1)
    if '--report' in opts:
        with open(opts['--report'], 'w') as fh:
            json.dump(reports, fh, indent=1)
    rows = [r for rs in reports.values() for r in rs if 'notes' in r or r.get('status') == 'skipped']
    count = {s: sum(1 for r in rows if r['status'] == s) for s in ('ok', 'approximate', 'skipped')}
    print('effects %d: layers ok %d, approximate %d, skipped %d' % (
        len(reports), count['ok'], count['approximate'], count['skipped']))
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
