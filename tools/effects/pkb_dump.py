#!/usr/bin/env python3
"""Dump PopcornFX 2.5 baked effects (.pkb, as shipped in Warcraft III: Reforged).

Usage:
  python3 -I pkb_dump.py FILE.pkb [FILE.pkb ...]   readable tree with decompiled scripts
  python3 -I pkb_dump.py --asm FILE.pkb            also print the raw disassembly of every script
  python3 -I pkb_dump.py --json FILE.pkb           structured JSON (layers, curves, renderers, scripts)
  python3 -I pkb_dump.py --objects FILE.pkb        every serialized object with its raw fields

Stdlib only. Everything marked '?' is a guess; see the notes printed by --help-notes.

File layout (reverse engineered, certain):
  u32 magic 0xCA000B11 | u8[4] version (2,5,1,1) | u32 build (63447) | u32 objectCount
  u32 classCount | u32 stringTableOffset | u32 0
  classCount x (u32 classIndex, u32 instanceCount)        -- classIndex indexes the string table
  objectCount x (u32 size, object bytes)                  -- objects in serialization order, refs are 1-based
     object = u8 0x20 | u32 classStringIndex | u16 fieldCount | fieldCount x (u16 fieldId, value)
     value encoding is NOT self-describing: it depends on the class schema (only non-default fields are
     written). Scalars are 4 bytes, arrays are u32 count + elements, localized strings are
     count x (fourcc lang reversed, u32 string index), 16-byte values are raw float4/int4.
  string table at stringTableOffset: u32 count, then count x (u8 length, bytes)
"""
import json
import struct
import sys

# ----------------------------------------------------------------------------------------------
# HBO object reader
# ----------------------------------------------------------------------------------------------

# Field schemas per class. Types: u=u32, i=i32, f=f32, r=object ref (1-based, 0=null), s=string index,
# b=u8, x16=16 raw bytes (float4/int4), i2=int2, f3=float3, u3=3 x u32, L=localized string list,
# A<t>=array of <t>. Field meanings are noted in SEMANTICS below.
SCHEMA = {
    'CParticleEffect': {1: 'r', 3: 'r'},
    'CParticleAttributeList': {0: 'Ar'},
    'CParticleAttributeDeclaration': {0: 's', 1: 'u', 2: 'L', 13: 'x16', 29: 'x16', 39: 'x16'},
    'CLayerGraphCompileCache': {1: 'Ar', 2: 'Ar', 3: 'Ar', 5: 'Au3'},
    'CLayerGraphCompileCache_EntrySlot': {0: 's', 1: 'u'},
    'CLayerGraphCompileCache_EventSlot': {0: 's', 1: 'i', 2: 'Au'},
    'CLayerGraphCompileCache_LayerSlot': {0: 'r', 1: 'Au', 2: 'Au'},
    'CLayerCompileCache': {0: 'Ax16', 1: 'Ax16', 2: 'Ar', 3: 'Ar', 5: 'Ar', 7: 'Ar', 8: 'Ar', 9: 'r',
                           10: 'Ar', 12: 'r', 13: 'u', 14: 'b', 15: 'b', 16: 'b', 17: 'b', 18: 'b',
                           20: 'u', 22: 'f', 23: 'f'},
    'CLayerCompileCacheField': {0: 's', 1: 'u', 2: 'u', 3: 'u', 4: 'i', 5: 'u'},
    'CLayerCompileCacheAttrib': {0: 's', 2: 'u', 3: 'u', 4: 'u', 8: 'x16', 9: 'x16', 10: 'x16'},
    'CLayerCompileCacheSampler': {0: 's', 1: 'r', 3: 'u', 4: 'r'},
    'CParticleNodeSamplerData_EventStream': {10: 'Au'},
    'CLayerCompileCacheEvent': {0: 's', 1: 'u', 2: 'Ar', 3: 's'},
    'CLayerCompileCacheEventPayload': {0: 's', 1: 'u', 2: 'u', 3: 'u', 4: 'u', 5: 'u'},
    'CCompilerBlobCache': {1: 'u', 2: 'Au', 3: 'Ar', 4: 'Ar', 5: 'r'},
    'CCompilerBlobCacheExternal': {0: 's', 1: 's', 2: 'i', 3: 'u', 4: 'u', 5: 'u', 6: 'u'},
    'CCompilerBlobCacheEntryPoint': {0: 's', 2: 'u'},
    'CCompilerBlobCacheFunctionDef': {0: 's', 1: 'i', 2: 'u', 3: 'Ar'},
    'CCompilerBlobCacheFunctionArg': {0: 'i', 1: 's', 2: 'u'},
    'CParticleNodeSamplerData_Shape': {0: 'L', 7: 'i2', 9: 'u', 13: 'f3', 15: 'u', 17: 'f', 18: 'f', 20: 'f'},
    'CParticleNodeSamplerData_Curve': {0: 'L', 7: 'i2', 9: 'u', 10: 'u', 14: 'u', 16: 'Af', 17: 'Af', 18: 'Af'},
    'CLayerCompileCacheRenderer': {0: 'u', 1: 'Ar', 2: 'Ar', 3: 's', 4: 'u'},
    'CLayerCompileCacheRendererParticleInput': {0: 'u', 1: 'u', 2: 's', 3: 'u'},
    'CLayerCompileCacheRendererProperty': {0: 's', 1: 'u', 2: 'x16', 3: 's'},
}
SIZES = {'u': 4, 'i': 4, 'f': 4, 'r': 4, 's': 4, 'b': 1, 'h': 2, 'x16': 16, 'i2': 8, 'f3': 12, 'u3': 12}
GENERIC = ['u', 'Au', 'x16', 'i2', 'f3', 'b', 'Ax16', 'Ai2', 'Af3', 'h']


def _end(t, b, q):
    if t == 'L':
        return q + 4 + 8 * struct.unpack_from('<I', b, q)[0]
    if t[0] == 'A':
        n = struct.unpack_from('<I', b, q)[0]
        if n > 1 << 20:
            raise ValueError
        return q + 4 + n * SIZES[t[1:]]
    return q + SIZES[t]


def _plan(b, q, nf, last, hints):
    """Find field types that tile the object exactly (schema first, then generic guesses)."""
    if nf == 0:
        return [] if q == len(b) else None
    if q + 2 > len(b):
        return None
    fid = struct.unpack_from('<H', b, q)[0]
    if fid <= last or fid > 200:
        return None
    for t in ([hints[fid]] if fid in hints else GENERIC):
        try:
            q2 = _end(t, b, q + 2)
        except (ValueError, struct.error):
            continue
        if q2 > len(b):
            continue
        rest = _plan(b, q2, nf - 1, fid, hints)
        if rest is not None:
            return [(fid, t)] + rest
    return None


def _scalar(t, b, p):
    if t in ('u', 'r', 's'):
        return struct.unpack_from('<I', b, p)[0]
    if t == 'i':
        return struct.unpack_from('<i', b, p)[0]
    if t == 'f':
        return struct.unpack_from('<f', b, p)[0]
    if t == 'b':
        return b[p]
    if t == 'h':
        return struct.unpack_from('<H', b, p)[0]
    if t == 'x16':
        return b[p:p + 16]
    if t == 'i2':
        return struct.unpack_from('<2i', b, p)
    if t == 'f3':
        return struct.unpack_from('<3f', b, p)
    if t == 'u3':
        return struct.unpack_from('<3I', b, p)
    raise ValueError(t)


class Obj:
    def __init__(self, idx, cls, f, guessed, raw):
        self.idx, self.cls, self.f, self.guessed, self.raw = idx, cls, f, guessed, raw

    def __repr__(self):
        return '%s#%d' % (self.cls, self.idx)


class PKB:
    def __init__(self, path):
        d = open(path, 'rb').read()
        self.path = path
        magic, ver, build, nobj, ncls, stroff = struct.unpack_from('<I4sIIII', d, 0)
        if magic != 0xCA000B11:
            raise SystemExit('%s: not a baked PopcornFX file (magic %08x)' % (path, magic))
        self.version = '.'.join(map(str, ver))
        self.build = build
        n = struct.unpack_from('<I', d, stroff)[0]
        q = stroff + 4
        self.S = []
        for _ in range(n):
            ln = d[q]
            self.S.append(d[q + 1:q + 1 + ln].decode('latin1'))
            q += 1 + ln
        p = 0x1c
        self.class_counts = {}
        for _ in range(ncls):
            ci, c = struct.unpack_from('<II', d, p)
            self.class_counts[self.S[ci]] = c
            p += 8
        self.objs = []
        for i in range(nobj):
            sz = struct.unpack_from('<I', d, p)[0]
            b = d[p + 4:p + 4 + sz]
            p += 4 + sz
            _flag, ci, nf = struct.unpack_from('<BIH', b, 0)
            cls = self.S[ci]
            sch = SCHEMA.get(cls, {})
            plan = _plan(b, 7, nf, -1, sch)
            F, guessed = {}, {}
            if plan is None:
                F['raw'] = b[7:].hex()
            else:
                q = 7
                for fid, t in plan:
                    q += 2
                    if t == 'L':
                        cnt = struct.unpack_from('<I', b, q)[0]
                        q += 4
                        v = []
                        for _ in range(cnt):
                            si = struct.unpack_from('<I', b, q + 4)[0]
                            v.append(self.S[si] if si < len(self.S) else si)
                            q += 8
                    elif t[0] == 'A':
                        cnt = struct.unpack_from('<I', b, q)[0]
                        q += 4
                        es = SIZES[t[1:]]
                        v = [_scalar(t[1:], b, q + j * es) for j in range(cnt)]
                        q += cnt * es
                    else:
                        v = _scalar(t, b, q)
                        q += SIZES[t]
                    if t == 's' or t == 'As':
                        v = self.S[v] if t == 's' else [self.S[x] for x in v]
                    F[fid] = v
                    if fid not in sch:
                        guessed[fid] = t
            self.objs.append(Obj(i, cls, F, guessed, b))

    def ref(self, r):
        return self.objs[r - 1] if r else None


# ----------------------------------------------------------------------------------------------
# Value helpers
# ----------------------------------------------------------------------------------------------

# Base type ids used by fields/args (certain for the ones observed: 1 bool, 5 u8, 25 int, 26 int2,
# 27 int3, 31 float, 32 float2, 33 float3, 34 float4). The VM uses base id + 1.
BASE_TYPES = {1: 'bool', 2: 'bool2', 3: 'bool3', 4: 'bool4', 5: 'u8', 21: 'uint', 22: 'uint2',
              23: 'uint3', 24: 'uint4', 25: 'int', 26: 'int2', 27: 'int3', 28: 'int4',
              31: 'float', 32: 'float2', 33: 'float3', 34: 'float4', 36: 'orientation'}


def vmtype(t):
    return BASE_TYPES.get(t - 1, 'opaque' if t == 0 else 't%02x' % t)


def fmt_f(x):
    if x != x:
        return 'nan'
    if x in (float('inf'), float('-inf')):
        return 'inf' if x > 0 else '-inf'
    if x == 0:
        return '0'
    return ('%.6g' % x)


def x16_floats(b):
    return [fmt_f(v) for v in struct.unpack('<4f', b)]


def fmt_const(lanes8, t):
    """Constants are 32-byte splats; vectors repeat their components (xyz xyz xy ...)."""
    ty = vmtype(t)
    u = struct.unpack('<8I', lanes8)
    f = struct.unpack('<8f', lanes8)
    width = {'float2': 2, 'float3': 3, 'float4': 4, 'int2': 2, 'int3': 3, 'int4': 4,
             'bool2': 2, 'bool3': 3, 'bool4': 4}.get(ty, 1)
    if ty.startswith('float'):
        vals = [fmt_f(x) for x in f[:width]]
    elif ty.startswith('bool'):
        vals = ['true' if x else 'false' for x in u[:width]]
    elif ty.startswith('int') or ty.startswith('uint'):
        vals = [str(struct.unpack('<i', struct.pack('<I', x))[0]) for x in u[:width]]
    else:
        vals = ['0x%x' % x for x in u[:width]]
    if width == 1:
        return vals[0]
    return '%s(%s)' % (ty, ', '.join(vals))


# ----------------------------------------------------------------------------------------------
# Bytecode
# ----------------------------------------------------------------------------------------------
# Blob (u32 array of CCompilerBlobCache field 2):
#   9 x u32 header: [0, 0, constBytes, codeBytes, 0, ?, ?, ?, ?]
#   constBytes/32 constants of 32 bytes (8 lanes, vectors repeat their components)
#   codeBytes of bytecode.
# Operand (4 bytes): u16 index, u8 bank (0 const, 1/2/3 register banks, 0x10 = zero/default?),
#   u8 vm type (base type + 1). ffffffff = no operand.
# Opcodes (lengths certain, names partly guessed):
#   43 LOAD   dst, u16 external                     44 STORE src, u16 external
#   4a MOVE   dst, src  (between float4 and an orientation, base type 36: the value as it is)
#   4b CAST   dst, src                              4d CVT/SWIZZLE u8 sub, u16 mask, dst, src
#   4e ALU    u8 sub, dst, a, b                      4f UNARY u8 sub, dst, a
#   50 FN2    u8 sub, dst, a, b (1d = dot)          51 FN3   u8 sub, dst, a, b, c
#   52 SELECT dst, a, b, cond   (cond ? b : a)
#   53 CALL   u8 hasResult, u16 objectExternal (ffff=none), u16 function,
#             (args+1) x (u8 flags, operand)  -- first is the result
ALU = {0x00: '+', 0x01: '-', 0x02: '*', 0x03: '/?', 0x08: '&&', 0x0d: '<', 0x10: '>=?'}
ALU_UNARY = {0x05: 'neg', 0x0b: '!'}
FN2 = {0x1d: 'dot', 0x1b: 'min?', 0x1c: 'max?'}
FN3 = {0x00: 'lerp'}
UNARY = {0x00: 'sqrt?', 0x11: 'rcp', 0x0d: 'exp?', 0x18: 'fn18?', 0x17: 'fn17?'}


def decode_blob(pk, bo):
    words = bo.f.get(2, [])
    blob = struct.pack('<%dI' % len(words), *words)
    hdr = struct.unpack_from('<9I', blob, 0)
    cbytes, clen = hdr[2], hdr[3]
    consts = [blob[36 + i * 32:36 + i * 32 + 32] for i in range(cbytes // 32)]
    code = blob[36 + cbytes:36 + cbytes + clen]
    exts = [pk.ref(r).f.get(0, '?') for r in bo.f.get(3, [])]
    fns = []
    for r in bo.f.get(4, []):
        fd = pk.ref(r)
        fns.append((fd.f.get(0, '?'), len(fd.f.get(3, []))))
    ins = []
    p = 0

    def opnd(q):
        idx, bank, ty = struct.unpack_from('<HBB', code, q)
        if idx == 0xffff and bank == 0xff:
            return None
        return (bank, idx, ty)

    try:
        while p < len(code):
            op = code[p]
            if op == 0x43:
                ins.append((p, 'load', opnd(p + 1), exts[struct.unpack_from('<H', code, p + 5)[0]]))
                p += 7
            elif op == 0x44:
                ins.append((p, 'store', opnd(p + 1), exts[struct.unpack_from('<H', code, p + 5)[0]]))
                p += 7
            elif op == 0x4a:
                ins.append((p, 'move', opnd(p + 1), opnd(p + 5)))
                p += 9
            elif op == 0x4b:
                ins.append((p, 'cast', opnd(p + 1), opnd(p + 5)))
                p += 9
            elif op == 0x4d:
                sub, mask = struct.unpack_from('<BH', code, p + 1)
                ins.append((p, 'cvt', sub, mask, opnd(p + 4), opnd(p + 8)))
                p += 12
            elif op == 0x4e:
                ins.append((p, 'alu', code[p + 1], opnd(p + 2), opnd(p + 6), opnd(p + 10)))
                p += 14
            elif op == 0x4f:
                ins.append((p, 'un', code[p + 1], opnd(p + 2), opnd(p + 6)))
                p += 10
            elif op == 0x50:
                ins.append((p, 'fn2', code[p + 1], opnd(p + 2), opnd(p + 6), opnd(p + 10)))
                p += 14
            elif op == 0x51:
                ins.append((p, 'fn3', code[p + 1], opnd(p + 2), opnd(p + 6), opnd(p + 10), opnd(p + 14)))
                p += 18
            elif op == 0x52:
                ins.append((p, 'sel', opnd(p + 1), opnd(p + 5), opnd(p + 9), opnd(p + 13)))
                p += 17
            elif op == 0x53:
                has, obj, fn = struct.unpack_from('<BHH', code, p + 1)
                name, nargs = fns[fn]
                q = p + 6
                ops = []
                for _ in range(nargs + 1):
                    ops.append((code[q], opnd(q + 1)))
                    q += 5
                ins.append((p, 'call', has, None if obj == 0xffff else exts[obj], name, ops))
                p = q
            else:
                ins.append((p, 'unknown', code[p:].hex(' ')))
                break
    except (IndexError, struct.error) as e:
        ins.append((p, 'unknown', 'decode error %r' % e))
    return dict(header=hdr, consts=consts, exts=exts, fns=fns, ins=ins, stage=bo.f.get(1, 0))


def opstr(o, consts):
    if o is None:
        return '_'
    bank, idx, ty = o
    if bank == 0:
        return fmt_const(consts[idx], ty) if idx < len(consts) else 'k%d?' % idx
    if bank == 0x10:
        return '0'
    return '%s%d' % ({1: 'f', 2: 'b', 3: 'r'}.get(bank, 'x%d_' % bank), idx)


def asm_lines(dec):
    c = dec['consts']
    out = []
    for i in dec['ins']:
        p, k = i[0], i[1]
        if k == 'load':
            s = 'LOAD   %s <- %s' % (opstr(i[2], c), i[3])
        elif k == 'store':
            s = 'STORE  %s -> %s' % (opstr(i[2], c), i[3])
        elif k == 'cast':
            s = 'CAST   %s = %s' % (opstr(i[2], c), opstr(i[3], c))
        elif k == 'move':
            s = 'MOVE   %s = %s' % (opstr(i[2], c), opstr(i[3], c))
        elif k == 'cvt':
            s = 'CVT%02x[%04x] %s = %s' % (i[2], i[3], opstr(i[4], c), opstr(i[5], c))
        elif k == 'alu':
            s = 'ALU%02x  %s = %s, %s' % (i[2], opstr(i[3], c), opstr(i[4], c), opstr(i[5], c))
        elif k == 'un':
            s = 'UN%02x   %s = %s' % (i[2], opstr(i[3], c), opstr(i[4], c))
        elif k == 'fn2':
            s = 'FN2_%02x %s = %s, %s' % (i[2], opstr(i[3], c), opstr(i[4], c), opstr(i[5], c))
        elif k == 'fn3':
            s = 'FN3_%02x %s = %s, %s, %s' % (i[2], opstr(i[3], c), opstr(i[4], c), opstr(i[5], c), opstr(i[6], c))
        elif k == 'sel':
            s = 'SEL    %s = %s ? %s : %s' % (opstr(i[2], c), opstr(i[5], c), opstr(i[4], c), opstr(i[3], c))
        elif k == 'call':
            args = ', '.join('%02x:%s' % (fl, opstr(o, c)) for fl, o in i[5])
            s = 'CALL   %s%s(%s)' % (i[3] + '.' if i[3] else '', i[4], args)
        else:
            s = '???    ' + i[2]
        out.append('%04x  %s' % (p, s))
    return out


def decompile(dec):
    """Turn the register code into 'external = expression' statements."""
    c = dec['consts']
    ins = dec['ins']
    # count reads per definition
    cur = {}
    uses = {}
    ndef = 0

    def reads(i):
        k = i[1]
        if k in ('store',):
            return [i[2]]
        if k in ('cast', 'move'):
            return [i[3]]
        if k == 'cvt':
            return [i[5]]
        if k == 'alu':
            return [i[4], i[5]]
        if k == 'un':
            return [i[4]]
        if k == 'fn2':
            return [i[4], i[5]]
        if k == 'fn3':
            return [i[4], i[5], i[6]]
        if k == 'sel':
            return [i[3], i[4], i[5]]
        if k == 'call':
            return [o for _, o in i[5][1:]]
        return []

    def writes(i):
        k = i[1]
        if k in ('load', 'cast', 'move', 'alu', 'un', 'sel', 'fn2', 'fn3'):
            return i[2] if k in ('load', 'cast', 'move', 'sel') else i[3]
        if k == 'cvt':
            return i[4]
        if k == 'call':
            return i[5][0][1] if i[2] else None
        return None

    defid = []
    for i in ins:
        for o in reads(i):
            if o and o[0] not in (0, 0x10):
                key = (o[0], o[1])
                if key in cur:
                    uses[cur[key]] = uses.get(cur[key], 0) + 1
        w = writes(i)
        if w and w[0] not in (0, 0x10):
            cur[(w[0], w[1])] = ndef
            defid.append(ndef)
            ndef += 1
        else:
            defid.append(None)

    env = {}
    stmts = []
    ntmp = [0]

    def ex(o):
        if o is None:
            return '_'
        if o[0] in (0, 0x10):
            return opstr(o, c)
        return env.get((o[0], o[1]), opstr(o, c))

    def bind(w, d, e, atomic=False):
        if w is None:
            return
        if not atomic and uses.get(d, 0) > 1 and len(e) > 12:
            ntmp[0] += 1
            t = 't%d' % ntmp[0]
            stmts.append('%s = %s' % (t, e))
            e = t
        env[(w[0], w[1])] = e

    for i, d in zip(ins, defid):
        k = i[1]
        if k == 'load':
            bind(i[2], d, i[3], True)
        elif k == 'store':
            stmts.append('%s = %s' % (i[3], ex(i[2])))
        elif k == 'cast':
            bind(i[2], d, '%s(%s)' % (vmtype(i[2][2]), ex(i[3])))
        elif k == 'move':
            bind(i[2], d, ex(i[3]), True)
        elif k == 'cvt':
            sub, mask, dst, src = i[2], i[3], i[4], i[5]
            if sub == 0x2a:
                e = 'float3(%s)' % ex(src)
            elif sub == 0x02 and mask == 0x100:
                e = 'float4(%s)' % ex(src)
            elif sub == 0x0c and mask == 0x8:
                e = '%s.xy' % ex(src)
            else:
                e = 'swz%02x_%04x(%s)->%s' % (sub, mask, ex(src), vmtype(dst[2]))
            bind(dst, d, e)
        elif k == 'alu':
            sub, dst, a, b = i[2], i[3], i[4], i[5]
            if b is None and sub in ALU_UNARY:
                e = '%s(%s)' % (ALU_UNARY[sub], ex(a))
            elif sub in ALU:
                e = '(%s %s %s)' % (ex(a), ALU[sub], ex(b))
            else:
                e = 'alu%02x(%s, %s)' % (sub, ex(a), ex(b))
            bind(dst, d, e)
        elif k == 'un':
            bind(i[3], d, '%s(%s)' % (UNARY.get(i[2], 'un%02x?' % i[2]), ex(i[4])))
        elif k == 'fn2':
            bind(i[3], d, '%s(%s, %s)' % (FN2.get(i[2], 'fn2_%02x?' % i[2]), ex(i[4]), ex(i[5])))
        elif k == 'fn3':
            bind(i[3], d, '%s(%s, %s, %s)' % (FN3.get(i[2], 'fn3_%02x?' % i[2]), ex(i[4]), ex(i[5]), ex(i[6])))
        elif k == 'sel':
            bind(i[2], d, '(%s ? %s : %s)' % (ex(i[5]), ex(i[4]), ex(i[3])))
        elif k == 'call':
            args = [ex(o) for fl, o in i[5][1:] if fl != 0x00 or o is not None]
            e = '%s%s(%s)' % (i[3] + '.' if i[3] else '', i[4], ', '.join(args))
            if i[2]:
                bind(i[5][0][1], d, e)
            else:
                stmts.append(e)
        else:
            stmts.append('??? undecoded: %s' % i[2][:120])
    return stmts


# ----------------------------------------------------------------------------------------------
# Semantics
# ----------------------------------------------------------------------------------------------

SHAPE_TYPES = {0: 'Box?', 1: 'Sphere', 2: 'Ellipsoid?', 3: 'Cylinder', 4: 'Capsule?', 5: 'Cone?', 6: 'Mesh?'}
SAMPLER_KINDS = {3: 'curve', 4: 'eventStream', 5: 'shape'}
RENDERER_CLASSES = {0: 'Billboard', 1: 'Ribbon', 2: 'Mesh?', 3: 'Light?', 4: 'Sound?'}
INPUT_SLOTS = {0: 'Position', 1: 'Size', 2: 'Enabled', 3: 'Size2?', 4: 'Axis', 5: 'NormalAxis',
               6: 'Rotation', 9: 'SelfID', 10: 'ParentID'}
PROP_TYPES = {0: 'feature', 1: 'bool', 3: 'int', 4: 'int2', 7: 'float', 8: 'float2', 9: 'float3',
              10: 'float4', 12: 'texture', 13: 'atlas?', 14: 'mesh?'}
# Enum lists come from PopcornFXCore/Materials/Interface/Editor.pkri (PopcornFX 2.5)
ENUMS = {
    'BillboardingMode': {'billboard': ['ScreenAligned', 'ViewposAligned', 'AxisAligned', 'AxisAlignedSpheroid',
                                       'AxisAlignedCapsule', 'PlaneAligned'],
                         'ribbon': ['ViewposAligned', 'NormalAxisAligned', 'SideAxisAligned']},
    'Transparent.Type': ['Additive', 'AdditiveNoAlpha', 'AlphaBlend', 'PremultipliedAlpha'],
    'Transparent.SortMode': ['CameraDistance', 'ByCustomValue'],
    'Opaque.Type': ['Solid', 'Masked'],
    'Atlas.Source': ['External', 'Procedural'],
    'Atlas.Blending': ['None', 'Linear'],
    'SoundAttenuation.AttenuationModel': ['Linear', 'InverseLinear'],
}


def prop_value(pk, p, rclass):
    name = p.f.get(0, '?')
    ty = p.f.get(1, 0)
    raw = p.f.get(2, b'\0' * 16)
    if ty == 0:
        return name, (raw[:4] != b'\0\0\0\0')
    if ty == 1:
        return name, (raw[:4] != b'\0\0\0\0')
    if ty in (3, 4):
        vals = struct.unpack('<4i', raw)[:ty - 2]
        en = ENUMS.get(name)
        if isinstance(en, dict):
            en = en['ribbon' if rclass == 1 else 'billboard']
        if en and ty == 3:
            v = vals[0]
            return name, '%s (%d)' % (en[v] if 0 <= v < len(en) else '?', v)
        return name, vals[0] if ty == 3 else list(vals)
    if 7 <= ty <= 10:
        f = struct.unpack('<4f', raw)[:ty - 6]
        return name, float(f[0]) if ty == 7 else [float(x) for x in f]
    if ty == 12:
        return name, p.f.get(3, '')
    return name, {'type': ty, 'raw': raw.hex(), 'str': p.f.get(3)}


def curve_dict(o):
    dim = o.f.get(9, 1)
    t = o.f.get(16, [])
    v = o.f.get(17, [])
    tg = o.f.get(18, [])
    keys = []
    for i, ti in enumerate(t):
        k = {'t': ti, 'value': v[i * dim:(i + 1) * dim]}
        if tg:
            k['tangent_in'] = tg[i * 2 * dim:i * 2 * dim + dim]
            k['tangent_out'] = tg[i * 2 * dim + dim:(i + 1) * 2 * dim]
        keys.append(k)
    extra = {('f%d' % k): v for k, v in o.f.items() if k not in (0, 7, 9, 16, 17, 18)}
    return {'label': (o.f.get(0) or [None])[0], 'dimension': dim,
            'interpolation': 'hermite (tangents present)' if tg else 'linear? (no tangents)',
            'keys': keys, 'unknown_fields': extra}


def shape_dict(o):
    st = o.f.get(15, 0)
    d = {'label': (o.f.get(0) or [None])[0], 'shape': SHAPE_TYPES.get(st, st)}
    if 17 in o.f:
        d['radius'] = o.f[17]
    if 18 in o.f:
        d['innerRadius?'] = o.f[18]
    if 20 in o.f:
        d['height?'] = o.f[20]
    if 13 in o.f:
        d['f13_offset?'] = list(o.f[13])
    for k, v in o.f.items():
        if k not in (0, 7, 13, 15, 17, 18, 20):
            d['f%d' % k] = v
    return d


def layer_prefix(fields):
    for f in fields:
        if '__' in f and not f.startswith('__'):
            return f.split('__')[0]
    return None


def analyse(pk):
    out = {'file': pk.path, 'version': pk.version, 'build': pk.build, 'classes': pk.class_counts}
    eff = next(o for o in pk.objs if o.cls == 'CParticleEffect')
    # attributes
    attrs = []
    al = pk.ref(eff.f.get(3, 0))
    if al:
        for r in al.f.get(0, []):
            a = pk.ref(r)
            attrs.append({'name': a.f.get(0), 'type': BASE_TYPES.get(a.f.get(1, 31), a.f.get(1, 31)),
                          'description': a.f.get(2),
                          'default': x16_floats(a.f[13]) if 13 in a.f else None,
                          'min': x16_floats(a.f[29]) if 29 in a.f else None,
                          'max': x16_floats(a.f[39]) if 39 in a.f else None,
                          'other': {('f%d' % k): v for k, v in a.f.items() if k not in (0, 1, 2, 13, 29, 39)}})
    out['attributes'] = attrs
    g = pk.ref(eff.f[1])
    entries = [pk.ref(r).f for r in g.f.get(1, [])]
    events = [pk.ref(r).f for r in g.f.get(3, [])]
    slots = [pk.ref(r) for r in g.f.get(2, [])]
    out['entries'] = [{'name': e.get(0), 'layer?': e.get(1)} for e in entries if e]
    out['event_routes'] = [{'event': e.get(0, '<entry>'), 'from_layer': e.get(1, 0), 'to_layers': e.get(2, [])}
                           for e in events]
    layers = []
    for li, ls in enumerate(slots):
        lc = pk.ref(ls.f[0])
        fields = []
        for r in lc.f.get(2, []):
            fo = pk.ref(r)
            fields.append({'name': fo.f.get(0), 'type': BASE_TYPES.get(fo.f.get(1), fo.f.get(1))})
        fnames = [f['name'] for f in fields]
        L = {'index': li, 'node': layer_prefix(fnames), 'fields': fields,
             'events_out': [pk.ref(r).f.get(0) for r in lc.f.get(5, [])],
             'samplers': [], 'renderers': [], 'scripts': []}
        for r in lc.f.get(7, []):
            so = pk.ref(r)
            data = pk.ref(so.f.get(1, 0))
            kind = SAMPLER_KINDS.get(so.f.get(3), so.f.get(3))
            s = {'name': so.f.get(0), 'kind': kind}
            if data is not None and data.cls.endswith('_Curve'):
                s.update(curve_dict(data))
            elif data is not None and data.cls.endswith('_Shape'):
                s.update(shape_dict(data))
                if so.f.get(4):
                    s['f4?'] = so.f[4]
            elif data is not None:
                s['data'] = {('f%d' % k): v for k, v in data.f.items()}
            L['samplers'].append(s)
        for r in lc.f.get(8, []):
            ro = pk.ref(r)
            rclass = ro.f.get(0, 0)
            ins = []
            for ir in ro.f.get(1, []):
                pi = pk.ref(ir).f
                slot = pi.get(0, 0)
                fi = pi.get(1, 0)
                ins.append({'input': pi.get(2) or INPUT_SLOTS.get(slot, 'slot%d' % slot),
                            'slot': slot, 'field': fnames[fi] if fi < len(fnames) else fi})
            props = {}
            enabled = []
            for pr in ro.f.get(2, []):
                po = pk.ref(pr)
                n, v = prop_value(pk, po, rclass)
                if po.f.get(1, 0) == 0:
                    if v:
                        enabled.append(n)
                else:
                    props[n] = v
            L['renderers'].append({'class': RENDERER_CLASSES.get(rclass, rclass), 'material': ro.f.get(3),
                                   'f4_order?': ro.f.get(4, 0), 'features_on': enabled,
                                   'properties': props, 'particle_inputs': ins})
        blobs = [pk.ref(r) for r in lc.f.get(10, [])]
        if lc.f.get(9):
            blobs.append(pk.ref(lc.f[9]))
        seen = {}
        for bo in blobs:
            dec = decode_blob(pk, bo)
            st = dec['stage']
            code = repr([i[1:] for i in dec["ins"]])
            dup = seen.get(code)
            seen.setdefault(code, st)
            role = {0: 'spawn (runs once per new particle)', 3: 'evolve stage 3?', 4: 'evolve stage 4?',
                    5: 'evolve stage 5?'}.get(st, 'stage %d' % st)
            L['scripts'].append({'stage': st, 'role': role, 'same_as_stage': dup, 'externals': dec['exts'],
                                 'functions': [f for f, _ in dec['fns']], 'code': decompile(dec),
                                 'asm': asm_lines(dec)})
        L['kind'] = ('renderer layer' if L['renderers'] else
                     'root (effect event stream)' if any(x['kind'] == 'eventStream' for x in L['samplers']) else
                     'spawner' if any('generate' in s['functions'] for s in L['scripts']) else 'other')
        L['summary'] = summarize(L)
        layers.append(L)
    out['layers'] = layers
    return out


def summarize(L):
    """Pull headline numbers out of the decompiled code (heuristic, see 'code' for the truth)."""
    s = {}
    for sc in L['scripts']:
        for st in sc['code']:
            if st.startswith('self.invLife = '):
                e = st[len('self.invLife = '):]
                if e.startswith('rcp(') and e.endswith(')'):
                    s['life'] = e[4:-1]
                else:
                    try:
                        v = float(e)
                        s['life'] = '0 (instant)' if v == float('inf') else ('%.4g' % (1.0 / v)) if v else 'inf'
                    except ValueError:
                        s['life'] = '1/(%s)' % e
            if 'generate(' in st and L['kind'] == 'spawner':
                g = st[st.index('generate('):]
                s.setdefault('generate', g)
                args = g[len('generate('):].split(', ')
                if len(args) > 1 and args[1].startswith('(scene.dt * '):
                    s['rate_per_s'] = args[1][len('(scene.dt * '):-1]
                elif len(args) > 1:
                    s['count_per_update'] = args[1]
    try:
        if 'rate_per_s' in s and s.get('life', '').replace('.', '', 1).isdigit():
            s['total~'] = '%.4g' % (float(s['rate_per_s']) * float(s['life']))
    except ValueError:
        pass
    return s


# ----------------------------------------------------------------------------------------------
# Output
# ----------------------------------------------------------------------------------------------

def fl(v):
    if isinstance(v, float):
        return fmt_f(v)
    if isinstance(v, (list, tuple)):
        return '(' + ', '.join(fl(x) for x in v) + ')'
    return str(v)


def print_tree(a, asm=False):
    print('=== %s' % a['file'])
    print('PopcornFX %s build %d' % (a['version'], a['build']))
    if a['attributes']:
        print('Attributes:')
        for at in a['attributes']:
            print('  %s : %s  default=%s min=%s max=%s %s' % (at['name'], at['type'], at['default'], at['min'],
                                                              at['max'], at['other'] or ''))
    print('Event routing:')
    for e in a['event_routes']:
        print('  layer %s --%s--> layers %s' % (e['from_layer'], e['event'], e['to_layers']))
    for L in a['layers']:
        print('\n--- layer %d [%s]%s' % (L['index'], L['kind'], ' node ' + L['node'] if L['node'] else ''))
        if L['summary']:
            print('  summary: %s' % '; '.join('%s = %s' % kv for kv in L['summary'].items()))
        print('  fields: %s' % ', '.join('%s:%s' % (f['name'], f['type']) for f in L['fields']))
        if L['events_out']:
            print('  events: %s' % ', '.join(L['events_out']))
        for s in L['samplers']:
            if s['kind'] == 'curve':
                print('  %s curve%s dim=%d %s' % (s['name'], ' "%s"' % s['label'] if s.get('label') else '',
                                                 s['dimension'], s['interpolation']))
                if s.get('unknown_fields'):
                    print('      unknown fields: %s' % s['unknown_fields'])
                for k in s['keys']:
                    line = '      t=%-7s v=%s' % (fmt_f(k['t']), fl(k['value']))
                    if 'tangent_in' in k:
                        line += '  in=%s out=%s' % (fl(k['tangent_in']), fl(k['tangent_out']))
                    print(line)
            elif s['kind'] == 'shape':
                print('  %s shape %s' % (s['name'], ', '.join('%s=%s' % (k, fl(v)) for k, v in s.items()
                                                       if k not in ('name', 'kind') and v is not None)))
            else:
                print('  %s %s %s' % (s['name'], s['kind'], s.get('data', '')))
        for r in L['renderers']:
            print('  renderer %s  material=%s  order?=%s' % (r['class'], r['material'], r['f4_order?']))
            print('      features on: %s' % ', '.join(r['features_on']))
            for k, v in r['properties'].items():
                print('      %s = %s' % (k, fl(v)))
            print('      inputs: %s' % ', '.join('%s<-%s' % (i['input'], i['field']) for i in r['particle_inputs']))
        for sc in L['scripts']:
            if sc['same_as_stage'] is not None:
                print('  script stage %d: identical to stage %d' % (sc['stage'], sc['same_as_stage']))
                continue
            print('  script stage %d (%s):' % (sc['stage'], sc['role']))
            for st in sc['code']:
                print('      ' + st)
            if asm:
                for l in sc['asm']:
                    print('        | ' + l)


def jsonable(x):
    if isinstance(x, bytes):
        return x.hex()
    if isinstance(x, dict):
        return {str(k): jsonable(v) for k, v in x.items()}
    if isinstance(x, (list, tuple)):
        return [jsonable(v) for v in x]
    if isinstance(x, float) and (x != x or x in (float('inf'), float('-inf'))):
        return str(x)
    return x


def main(argv):
    flags = {a for a in argv if a.startswith('--')}
    files = [a for a in argv if not a.startswith('--')]
    if not files:
        print(__doc__)
        return 1
    for fn in files:
        pk = PKB(fn)
        if '--objects' in flags:
            for o in pk.objs:
                print('#%d %s %s%s' % (o.idx + 1, o.cls, jsonable(o.f),
                                       '  guessed=%s' % o.guessed if o.guessed else ''))
            continue
        a = analyse(pk)
        if '--json' in flags:
            print(json.dumps(jsonable(a), indent=1))
        else:
            print_tree(a, asm='--asm' in flags)
    return 0


if __name__ == '__main__':
    sys.exit(main(sys.argv[1:]))
