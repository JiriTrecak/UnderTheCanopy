"""The ants' buildings (docs/ants.md: sand-clay spire cathedrals with royal-blue drapes and
blue-violet blossoms) from Tripo, made game-ready and animated in Blender from one config each:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/ant_building.py -- \\
        tools/buildings/ants/<building>.json <ants-<building>-whole.glb> <ants-<building>-segmented.glb>

Every ant building is a Tripo image-to-3D model of its approved concept (`whole`: one mesh, one
texture atlas) and Tripo's segmentation of it (`segmented`: dozens of parts, each with its own
texture). The building is the whole model; the segmentation only says where its moving parts
are: the whole model's faces near those parts are split off into their own nodes, so the
building keeps one material and one texture atlas (a node with a glow of its own gets a copy).
The config (tools/buildings/ants/) says what is special about each; this script does the rest
(D65: at rest nothing moves or glows):
- normalise: footprint centred, ground at 0, `width` metres across, optional `turn` (degrees
  about Z) so the entrance faces -Y. Both exports get the same transform;
- team colour: the royal-blue drapes and banners (navy texels, not the light blossoms) go into
  every texture's alpha;
- glow: texel classes (`blossoms` light blue-violet, `amber` resin and honey, `liquid` raw
  substance, `dark` openings, `navy`, `all`) get an emission texture on the `body` (the whole model) or on a
  named moving part. `active` glow is dark at rest and eases in while the building works;
  `always` glow (raw substance only, the D65 exception) is lit at rest too; `team: false` keeps
  the player colour off that material (raw substance);
- moving parts (`moves`, `spawn.moves`): the body faces near the listed segmented parts become a
  node named `node`, with its hinge as the origin (`cut`, metres at a 16-metre build, is how near). `work` loops (starting and ending at rest, whole cycles only):
  `sway` and `nod` rotate about a local axis, `pulse` scales, `bob` rises, `open` rotates out,
  holds and rotates back. `pieces` lists extra segmented parts that are cut from the body but
  stay still (so a glow can differ there); `drop` lists stray segmented parts whose faces go;
- `idle` holds the rest pose; `spawn` and `attack` (optional, a tower's per-shot clip) play once; `pulse` with an `axis` scales along
  that axis only (a spire stretching up keeps its seams closed);
- `floor` {point, radius}: a dark disc closing an open shaft;
- `cap`: closes the holes the cut opens in the body and the open sides of the nodes, so a moving
  part never reveals a void where it stood. `true` caps every node; a list of node names caps only
  those (not still pieces or single-sheet banners, which a cap would cover with a flat fill);
- sockets: points on parts (`top`, `centre`, `front`, `bottom`, `back`) or fixed points
  (metres at a 16-metre build), for effects."""
import json, math, os, sys

import bpy
import mathutils
import numpy as np
from mathutils.bvhtree import BVHTree

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
ARGS = sys.argv[sys.argv.index('--') + 1:]
CFG = json.load(open(ARGS[0]))
WHOLE, SEGMENTED = ARGS[1], ARGS[2]
NAME = CFG['name']
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = float(CFG['width'])
S = WIDTH / 16.0  # offsets in configs are metres at a 16-metre build
LUM = np.array([0.2126, 0.7152, 0.0722], np.float32)
VIOLET = (0.55, 0.32, 1.0)
AXIS = {'x': 0, 'y': 1, 'z': 2}


def import_meshes(path, prefix):
    before = set(bpy.data.objects)
    bpy.ops.import_scene.gltf(filepath=path)
    new = [o for o in bpy.data.objects if o not in before]
    for o in new:
        if o.type != 'MESH':
            for c in o.children:
                m = c.matrix_world.copy()
                c.parent = None
                c.matrix_world = m
    meshes = [o for o in new if o.type == 'MESH']
    for o in [o for o in new if o.type != 'MESH']:
        bpy.data.objects.remove(o, do_unlink=True)
    for o in meshes:
        o.data.transform(o.matrix_world)
        o.matrix_world = mathutils.Matrix.Identity(4)
        o['source'] = prefix
    return meshes


kit.reset()
whole = import_meshes(WHOLE, 'whole')
if len(whole) > 1:
    with bpy.context.temp_override(active_object=whole[0], selected_editable_objects=whole):
        bpy.ops.object.join()
    whole = [whole[0]]
body = whole[0]
body.name = body.data.name = 'body'
seg = {o.name: o for o in import_meshes(SEGMENTED, 'segmented')}

# --- Normalise (both exports share Tripo's frame).
turn = mathutils.Matrix.Rotation(math.radians(CFG.get('turn', 0)), 4, 'Z')
pts = [turn @ v.co for v in body.data.vertices]
lo = mathutils.Vector([min(p[i] for p in pts) for i in range(3)])
hi = mathutils.Vector([max(p[i] for p in pts) for i in range(3)])
scale = WIDTH / max(hi.x - lo.x, hi.y - lo.y)
shift = mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation(
    (-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z)) @ turn
for o in [body] + list(seg.values()):
    o.data.transform(shift)
    for p in o.data.polygons:
        p.use_smooth = True
HEIGHT = (hi.z - lo.z) * scale
DEPTH = (hi.y - lo.y) * scale
print('size %.1f x %.1f x %.1f m, %d segmented parts' % ((hi.x - lo.x) * scale, DEPTH, HEIGHT, len(seg)))
sp = [v.co for o in seg.values() for v in o.data.vertices]
print('segmented bounds x %.2f..%.2f y %.2f..%.2f z %.2f..%.2f' % (
    min(p.x for p in sp), max(p.x for p in sp), min(p.y for p in sp), max(p.y for p in sp),
    min(p.z for p in sp), max(p.z for p in sp)))

# --- Moving parts and still pieces: joined segmented parts, cut out of the body.
groups = {}
for mv in (CFG.get('moves', []) + CFG.get('spawn', {}).get('moves', []) + CFG.get('attack', {}).get('moves', [])
           + CFG.get('pieces', [])):
    if 'parts' not in mv:
        continue  # moves a node another entry defines
    groups.setdefault(mv['node'], {'parts': mv['parts'], 'hinge': mv.get('hinge', 'bottom'),
                                   'hinge_offset': mv.get('hinge_offset')})
import bmesh  # noqa: E402

# The segmented parts only say where each node is: its faces are the body's own faces near them
# (one atlas for the whole building), split off into their own object.
cut_tol = CFG.get('cut', 0.06) * S
deps = bpy.context.evaluated_depsgraph_get()
trees = {name: BVHTree.FromObject(o, deps) for name, o in seg.items()}


def near(poly_center, names):
    for n in names:
        hit = trees[n].find_nearest(poly_center)
        if hit[0] is not None and hit[3] < cut_tol:
            return True
    return False


def edge_keys(me):
    """Boundary edges of a mesh as rounded coordinate pairs (survive bmesh round trips)."""
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)  # Tripo splits vertices at UV seams
    keys = {tuple(sorted(tuple(round(c, 4) for c in v.co) for v in e.verts)) for e in bm.edges if e.is_boundary}
    bm.free()
    return keys


def cap(me, original, only=None):
    """Closes the boundary loops a cut opened (not the model's own openings): each new face takes
    the mean UV of its rim, so it samples the clay around it."""
    bm = bmesh.new()
    bm.from_mesh(me)
    bmesh.ops.remove_doubles(bm, verts=bm.verts, dist=1e-5)  # UVs live on loops and survive
    key = lambda e: tuple(sorted(tuple(round(c, 4) for c in v.co) for v in e.verts))  # noqa: E731
    new_edges = [e for e in bm.edges if e.is_boundary and key(e) not in original
                 and (only is None or key(e) in only)]
    if not new_edges:
        bm.free()
        return 0
    uv = bm.loops.layers.uv.active
    rim_uv = {}
    for e in new_edges:
        for v in e.verts:
            uvs = [lp[uv].uv.copy() for lp in v.link_loops]
            if uvs:
                rim_uv[v] = sum(uvs, mathutils.Vector((0, 0))) / len(uvs)
    made = bmesh.ops.holes_fill(bm, edges=new_edges, sides=0)['faces']
    for f in made:
        pts = [rim_uv[v] for v in f.verts if v in rim_uv]
        mean = sum(pts, mathutils.Vector((0, 0))) / len(pts) if pts else mathutils.Vector((0.5, 0.5))
        for lp in f.loops:
            lp[uv].uv = mean
        f.smooth = True
    bm.to_mesh(me)
    bm.free()
    return len(made)


ORIGINAL_EDGES = edge_keys(body.data) if CFG.get('cap') else set()
nodes = {}
taken = set()
for key, gr in groups.items():
    faces = [p.index for p in body.data.polygons if p.index not in taken and near(p.center, gr['parts'])]
    taken.update(faces)
    bm = bmesh.new()
    bm.from_mesh(body.data)
    bm.faces.ensure_lookup_table()
    keep = set(faces)
    bmesh.ops.delete(bm, geom=[f for f in bm.faces if f.index not in keep], context='FACES')
    me = bpy.data.meshes.new(key)
    bm.to_mesh(me)
    bm.free()
    me.materials.append(body.data.materials[0])
    for poly in me.polygons:
        poly.material_index = 0
        poly.use_smooth = True
    o = kit.link(bpy.data.objects.new(key, me))
    nodes[key] = o
    print('node %-14s %5d body faces from %d segmented part(s)' % (key, len(faces), len(gr['parts'])))
# `drop`: segmented parts that are stray bits of the Tripo model (a floating petal): their faces go.
dropped = [p.index for p in body.data.polygons if p.index not in taken and near(p.center, CFG.get('drop', []))]
taken.update(dropped)
if dropped:
    print('dropped %d body faces near %s' % (len(dropped), ', '.join(CFG['drop'])))
bm = bmesh.new()
bm.from_mesh(body.data)
bm.faces.ensure_lookup_table()
bmesh.ops.delete(bm, geom=[bm.faces[i] for i in sorted(taken)], context='FACES')
bm.to_mesh(body.data)
bm.free()
for o in list(seg.values()):
    bpy.data.objects.remove(o, do_unlink=True)
if CFG.get('cap'):
    # `cap: true` closes every opening; a list closes only those nodes' openings and the body
    # holes they leave (a still piece or a single-sheet banner must stay open).
    capped = list(nodes) if CFG['cap'] is True else CFG['cap']
    rims = set().union(*[edge_keys(nodes[k].data) for k in capped]) if capped else set()
    print('capped %d holes in the body' % cap(body.data, ORIGINAL_EDGES, None if CFG['cap'] is True else rims))
    for key in capped:
        print('capped %d openings of %s' % (cap(nodes[key].data, ORIGINAL_EDGES), key))


# --- Textures: team mask in alpha, glow classes, per material.
def texture_node(mat):
    link = mat.node_tree.nodes['Principled BSDF'].inputs['Base Color'].links
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


def classes(rgb):
    lum = rgb @ LUM
    r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
    blue = b - np.maximum(r, g)
    return lum, blue, {
        'blossoms': np.clip((blue - 0.1) / 0.1, 0, 1) * np.clip((lum - 0.26) / 0.08, 0, 1),
        'amber': np.clip((r - b - 0.25) / 0.15, 0, 1) * np.clip((r - g - 0.08) / 0.1, 0, 1) * np.clip((lum - 0.15) / 0.1, 0, 1),
        'liquid': np.clip((b - 0.4) / 0.15, 0, 1) * np.clip((r - 0.2) / 0.1, 0, 1) * np.clip((blue - 0.08) / 0.1, 0, 1),
        'dark': np.clip((0.09 - lum) / 0.05, 0, 1),
        'all': np.ones_like(lum),
        # The navy the team mask takes (drapes, windows, the Fungus Garden's gills): with
        # `team: false` a node's navy glows instead of taking the player colour.
        'navy': np.clip((blue - 0.04) / 0.1, 0, 1) * np.clip((0.3 - lum) / 0.06, 0, 1) * (b > 0.08),
    }


materials = []


ORIGINAL = {}
ATLAS_INFO = {}


def darkest_uv(rgb, lum):
    """A UV in a flat, dark, neutral patch of the atlas (for added geometry such as a shaft floor)."""
    blur = lum.copy()
    for _ in range(4):
        blur = (blur + np.roll(blur, 2, 0) + np.roll(blur, -2, 0) + np.roll(blur, 2, 1) + np.roll(blur, -2, 1)) / 5
    blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
    score = blur + np.abs(lum - blur) * 4 + np.clip(blue, 0, 1) * 4
    H, W = lum.shape
    iy, ix = np.unravel_index(np.argmin(score[8:-8, 8:-8]), score[8:-8, 8:-8].shape)
    return ((ix + 8.5) / W, (iy + 8.5) / H)


def prepare(obj, label, glow, fresh=True):
    """Team alpha and glow on `obj`'s material. The body's atlas is processed once; a node with a
    glow of its own gets a copy that starts again from the original atlas pixels."""
    mat = obj.data.materials[0].copy() if fresh else obj.data.materials[0]
    mat.name = 'TC_%s_%s' % (NAME.replace('-', '_'), label.replace('-', '_'))
    obj.data.materials[0] = mat
    if 'activeGlow' in mat:
        del mat['activeGlow']  # a copy of the body's material must not inherit its glow behaviour
    node = texture_node(mat)
    if 'atlas' in ORIGINAL:
        node.image = ORIGINAL['atlas']
    else:
        ORIGINAL['atlas'] = node.image
    for t in [n for n in mat.node_tree.nodes if n.type == 'TEX_IMAGE' and n.image and n.image.name.endswith('-glow')]:
        mat.node_tree.nodes.remove(t)
    H, W = node.image.size[1], node.image.size[0]
    rgb = np.array(node.image.pixels[:], dtype=np.float32).reshape(H, W, 4)[:, :, :3].copy()
    lum, blue, cls = classes(rgb)
    team = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * np.clip((0.3 - lum) / 0.06, 0.0, 1.0) * (rgb[:, :, 2] > 0.08)
    cls['blossoms'] = cls['blossoms'] * (1.0 - np.clip(team * 3, 0, 1))  # banner trims are not blossoms
    ATLAS_INFO.setdefault('dark_uv', darkest_uv(rgb, lum))
    if glow and glow.get('team') is False:
        team[:] = 0  # raw substance and the like never take the player's colour
    img = bpy.data.images.new('%s-%s-team' % (NAME, label), W, H, alpha=True)
    img.pixels[:] = np.concatenate([rgb, team[..., None]], axis=2).ravel()
    img.alpha_mode = 'CHANNEL_PACKED'
    img.pack()
    node.image = img
    mat['teamColorMask'] = 'baseColorAlpha'
    materials.append(mat)
    note = 'team %.3f' % team.mean()
    if glow:
        weight = np.zeros((H, W), np.float32)
        for c in glow['classes']:
            weight = np.maximum(weight, cls[c])
        color = np.array(glow.get('color', VIOLET), np.float32)
        em = weight[..., None] * color * np.clip(rgb.max(axis=2, keepdims=True) * 1.6, 0.4, 1.0)
        eimg = bpy.data.images.new('%s-%s-glow' % (NAME, label), W, H, alpha=False)
        eimg.pixels[:] = np.concatenate([em, np.ones_like(em[:, :, :1])], axis=2).ravel()
        eimg.pack()
        tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
        tex.image = eimg
        bsdf = mat.node_tree.nodes['Principled BSDF']
        mat.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
        bsdf.inputs['Emission Strength'].default_value = glow.get('strength', 4.0)
        if not glow.get('always'):
            mat['activeGlow'] = {'pulse': glow.get('pulse', 0.8), 'flow': glow.get('flow', 0.0)}
        note += ', glow %s %.4f%s' % ('+'.join(glow['classes']), float((weight > 0.3).mean()),
                                       ' always' if glow.get('always') else '')
    print('material', mat.name, W, 'px,', note)


GLOW = {g.get('on', 'body'): g for g in CFG.get('glow', [])}
prepare(body, 'body', GLOW.get('body'))
for key, o in nodes.items():
    if key in GLOW:
        o.data.materials[0] = body.data.materials[0].copy()  # its own glow: its own material
        prepare(o, key, GLOW[key], fresh=False)
    else:
        o.data.materials[0] = body.data.materials[0]


# --- `floor`: a dark disc closing an open shaft (Tripo leaves tunnels open to the ground).
if CFG.get('floor'):
    fl = CFG['floor']
    centre = mathutils.Vector(fl['point']) * S
    r = fl['radius'] * S
    bm = bmesh.new()
    ring = [bm.verts.new((centre.x + r * math.cos(2 * math.pi * k / 32), centre.y + r * math.sin(2 * math.pi * k / 32), centre.z))
            for k in range(32)]
    face = bm.faces.new(ring)
    uv = bm.loops.layers.uv.verify()
    for lp in face.loops:
        lp[uv].uv = ATLAS_INFO['dark_uv']
    me = bpy.data.meshes.new('shaft-floor')
    bm.to_mesh(me)
    bm.free()
    me.materials.append(body.data.materials[0])
    floor = kit.link(bpy.data.objects.new('shaft-floor', me))
    with bpy.context.temp_override(active_object=body, selected_editable_objects=[body, floor]):
        bpy.ops.object.join()
    print('floor disc r %.2f at (%.2f %.2f %.2f)' % (r, *centre))


# --- Hinges.
def extent(o):
    vs = np.array([tuple(v.co) for v in o.data.vertices])
    return vs.min(axis=0), vs.max(axis=0)


def point_on(o, where):
    if not isinstance(where, str):
        return mathutils.Vector(where) * S
    lo_, hi_ = extent(o)
    lo_, hi_ = lo_ + np.array(o.location), hi_ + np.array(o.location)
    c = (lo_ + hi_) / 2
    return mathutils.Vector({'top': (c[0], c[1], hi_[2]), 'bottom': (c[0], c[1], lo_[2]),
                             'front': (c[0], lo_[1], c[2]), 'back': (c[0], hi_[1], c[2]),
                             'centre': tuple(c)}[where])


for key, o in nodes.items():
    gr = groups[key]
    hinge = point_on(o, gr['hinge'])
    if gr.get('hinge_offset'):
        hinge += mathutils.Vector(gr['hinge_offset']) * S
    o.data.transform(mathutils.Matrix.Translation(-hinge))
    o.location = hinge
    o.rotation_mode = 'XYZ'
    lo_, hi_ = extent(o)
    print('node %-14s hinge (%.2f %.2f %.2f) size %.2f x %.2f x %.2f' % (key, *hinge, *(hi_ - lo_)))


# --- Clips.
def wave(mv, frames):
    """(path, index, keys) for one motion over `frames` frames: whole cycles, rest at both ends."""
    kind = mv['kind']
    cycles = max(1, round(frames / (mv.get('period', 2.0) * kit.FPS)))
    amp, phase = mv.get('amp', 5.0), mv.get('phase', 0.0)
    keys = []
    for f in list(range(0, frames, 3)) + [frames]:
        t = f / frames
        if kind == 'open':
            hold = mv.get('hold', 0.4)
            rise = (1 - hold) / 2
            s = min(1.0, t / rise, (1 - t) / rise)
            v = amp * (0.5 - 0.5 * math.cos(math.pi * max(0.0, s)))
        else:
            env = min(1.0, t / 0.12, (1 - t) / 0.12)  # ease in and out of rest
            v = amp * env * math.sin(2 * math.pi * (cycles * t + phase))
        keys.append((f, v))
    if kind in ('sway', 'nod', 'open'):
        return 'rotation_euler', AXIS[mv.get('axis', 'x')], [(f, math.radians(v)) for f, v in keys]
    if kind == 'pulse':
        if mv.get('axis'):
            return 'scale', AXIS[mv['axis']], [(f, 1.0 + v / 100.0) for f, v in keys]
        return 'scale', None, [(f, 1.0 + v / 100.0) for f, v in keys]
    if kind == 'bob':
        return 'location', 2, [(f, v * S) for f, v in keys]
    raise ValueError(kind)


def slot_curves(action, obj):
    slot = obj.animation_data.action_slot
    out = []
    for layer in action.layers:
        for strip in layer.strips:
            bag = strip.channelbag(slot)
            if bag:
                out.extend(bag.fcurves)
    return out


REST = {k: (o.location.copy(), o.rotation_euler.copy(), o.scale.copy()) for k, o in nodes.items()}


def clip(name, frames, moves):
    """One action keying every node (rest where it doesn't move), so blends stay clean."""
    action = bpy.data.actions.new(name)
    tracks = {k: [] for k in nodes}
    for mv in moves:
        tracks[mv['node']].append(wave(mv, frames))
    for key, o in nodes.items():
        loc, rot, scl = REST[key]
        o.animation_data_create()
        o.animation_data.action = action
        for path, value in (('location', loc), ('rotation_euler', rot), ('scale', scl)):
            setattr(o, path, value)
            for f in (0, frames):
                o.keyframe_insert(data_path=path, frame=f)
        for path, index, keys in tracks[key]:
            for f, v in keys:
                if index is None:
                    o.scale = (v, v, v)
                    o.keyframe_insert(data_path='scale', frame=f)
                else:
                    getattr(o, path)[index] = (loc[index] + v) if path == 'location' else v
                    o.keyframe_insert(data_path=path, index=index, frame=f)
                    if path == 'scale':
                        o.scale = scl
        for fc in slot_curves(action, o):
            for kp in fc.keyframe_points:
                kp.interpolation = 'BEZIER'
        o.animation_data.action = None
        o.location, o.rotation_euler, o.scale = loc, rot, scl
        track = o.animation_data.nla_tracks.new()
        track.name = name
        track.strips.new(name, 0, action)


def events(spec):
    return [{'name': e['name'], 'time': round(e['frame'] / kit.FPS, 3)} for e in spec.get('events', [])]


animations = [{'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []}]
if nodes:
    work = CFG.get('work', {})
    clip('idle', 2, [])
    clip('work', work.get('frames', 120), CFG.get('moves', []))
    animations.append({'semantic': 'work', 'clip': 'work', 'loop': True, 'events': events(work)})
    for one_shot in ('spawn', 'attack'):  # a tower plays `attack` once per shot its crew fires (D68)
        if CFG.get(one_shot):
            spec = CFG[one_shot]
            clip(one_shot, spec['frames'], spec['moves'])
            animations.append({'semantic': one_shot, 'clip': one_shot, 'loop': False, 'events': events(spec)})

# --- Sockets (on a moving node, they ride with it).
for s in CFG.get('sockets', []):
    owner = nodes.get(s.get('node'))
    at = point_on(owner, s.get('at', 'top')) if owner else mathutils.Vector(s['point']) * S
    at += mathutils.Vector(s.get('offset', (0, 0, 0))) * S
    e = kit.socket(s['name'], at)
    if owner:
        m = e.matrix_world.copy()
        e.parent = owner
        e.matrix_world = m
    print('socket %-18s (%.2f %.2f %.2f)' % (s['name'], *at))

fp = CFG.get('footprint', int(round(WIDTH)))
asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': CFG['title'], 'kind': 'building',
    'tags': ['ants', 'tripo', 'team-mask'] + (['animated'] if nodes else []) + CFG.get('tags', []), 'race': 'ants',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': [m.name for m in materials]},
        'animations': animations,
        'blocker': {'shape': 'box', 'size': [fp, fp], 'offset': [0, 0]},
        'vegetationClearance': fp // 2,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(HEIGHT + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) concept for '
               'Under the Canopy; made game-ready and animated by tools/buildings/ant_building.py with '
               'tools/buildings/ants/%s.' % os.path.basename(ARGS[0]),
}
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)
