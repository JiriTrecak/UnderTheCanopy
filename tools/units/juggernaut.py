"""The beetles' Juggernaut from Tripo, made game-ready and animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/juggernaut.py -- \\
        <unit-juggernaut-segmented.glb>

The walker is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-juggernaut-concept.png), split by Tripo's detailed segmentation into about
120 parts that share one texture atlas. It is a rigid machine, animated like the buildings
(D65): parts become a few nodes, and the cook turns animated nodes into palette joints.
- The parts are grouped by position: the hull (dome, prow, furnace ring, cockpit and crew,
  the cannon mounts and hip knuckles), six legs, each an upper limb (hip to the arch) and a
  shin with its foot, and the two cannons.
- The legs hang from the hull, so they follow its bob and jolts; every sampled frame solves
  each leg's lift so its foot stays on the ground (planted feet never sink or float), and a
  swinging leg lifts on top of that.
- The royal-blue plates become the team mask in the atlas's alpha (violet glass excluded).
- The furnace window glows violet: a copy of its glass with an emission map of the violet
  texels sits just proud of it. Units have no work activity, so the glow is plain emission
  (activeGlow would stay dark); at death the copy sinks into the hull and the furnace is dark.
- Muzzle flashes are emissive flame cones in the barrels, scaled up for the shot.
- Clips: idle, walk (an alternating-tripod gait; `step` events), attack (both cannons recoil,
  `release` at the shot), hit, death (legs buckle, the hull crashes down, the furnace dies).
- Sockets: projectile, both muzzles, the furnace, a steam vent and the six feet.

It also writes the heavy shell the cannons fire as the asset's second geometry
(geometry_2.glb), for the projectile effect effect.units.juggernaut-shell."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
GLB = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-juggernaut'
OUT = os.path.join(ROOT, 'base', 'assets', 'units', NAME)
HEIGHT = 7.0  # model metres at display scale 1: about four soldiers (1.05 m models at scale 2)
S = HEIGHT / 7.0  # offsets below were set at 7 metres
FPS = kit.FPS
V = mathutils.Vector

# Leg directions (degrees around the legs' centre, Blender +X right, -Y forward) and the parts
# Tripo's detailed segmentation made (numbers hold for this one export).
LEGS = {'fl': -149, 'ml': 178, 'bl': 145, 'fr': -32, 'mr': -1, 'br': 34}  # in the model's bounds frame
TRIPOD_A, TRIPOD_B = ('fl', 'mr', 'bl'), ('fr', 'ml', 'br')
FURNACE = 'tripo_part_53'  # the furnace window: violet glass behind a grille
HULL_ONLY = {'tripo_part_45', 'tripo_part_49', 'tripo_part_5'}  # cannon mounts, the prow

kit.reset()
bpy.ops.import_scene.gltf(filepath=GLB)
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        for c in o.children:
            m = c.matrix_world.copy()
            c.parent = None
            c.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']

# --- Normalise: HEIGHT tall, ground at 0; the origin is the centre of the six feet, so the
# walker turns about its legs.
pts = [o.matrix_world @ v.co for o in parts for v in o.data.vertices]
lo = V([min(p[i] for p in pts) for i in range(3)])
hi = V([max(p[i] for p in pts) for i in range(3)])
scale = HEIGHT / (hi.z - lo.z)
shift = mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation((-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z))
for o in parts:
    o.data.transform(shift @ o.matrix_world)
    o.matrix_world = mathutils.Matrix.Identity(4)
    o.rotation_mode = 'XYZ'


def centre(o):
    vs = [v.co for v in o.data.vertices]
    return sum(vs, V()) / len(vs)


def bounds(objs):
    vs = [v.co for o in objs for v in o.data.vertices]
    return V([min(v[i] for v in vs) for i in range(3)]), V([max(v[i] for v in vs) for i in range(3)])


# --- Groups by position, in the model's own frame (centred on its bounds): the legs by
# their direction from the legs' centre, the cannons ahead of the prow.
LEG_CENTRE = V((0.0, 1.25 * S, 0.0))
groups = {k: [] for k in ['hull', 'cannon_l', 'cannon_r'] + [f'{k}_upper' for k in LEGS] + [f'{k}_lower' for k in LEGS]}
for o in parts:
    c = centre(o)
    mn, _ = bounds([o])
    d = c - LEG_CENTRE
    r = math.hypot(d.x, d.y)
    a = math.degrees(math.atan2(d.y, d.x))
    g = 'hull'
    if o.name in HULL_ONLY:
        pass
    elif (c.y < -3.2 * S or mn.y < -3.6 * S) and abs(c.x) > 0.9 * S:
        g = 'cannon_l' if c.x < 0 else 'cannon_r'
    elif r > 3.0 * S and c.z < 4.3 * S:
        leg = min(LEGS, key=lambda k: abs((a - LEGS[k] + 180) % 360 - 180))
        if abs((a - LEGS[leg] + 180) % 360 - 180) < 30:
            g = f'{leg}_lower' if (c.z < 2.6 * S and r > 4.3 * S) else f'{leg}_upper'
    groups[g].append(o)
# The origin is the centre of the six feet, so the walker turns about its legs.
feet = [o for g, objs in groups.items() if g.endswith('_lower') for o in objs if bounds([o])[1].z < 1.0 * S]
fys = [centre(o).y for o in feet]
C = V((0.0, (min(fys) + max(fys)) / 2, 0.0))
for o in parts:
    o.data.transform(mathutils.Matrix.Translation(-C))
print('feet centre shifted by', round(C.y, 2))
for g, objs in groups.items():
    print('group', g, len(objs), sum(len(o.data.polygons) for o in objs))

MAT = parts[0].data.materials[0]
assert all(o.data.materials[0] == MAT for o in parts), 'expected one shared atlas'


# --- The furnace glass copy (before joining): the window, just proud of the hull.
furnace = bpy.data.objects[FURNACE]
glass = furnace.copy()
glass.data = furnace.data.copy()
glass.name = glass.data.name = 'furnace_glow'
bpy.context.scene.collection.objects.link(glass)
glass.data.transform(mathutils.Matrix.Translation((0, -0.025 * S, 0)))


def join(objs, name):
    if len(objs) > 1:
        with bpy.context.temp_override(active_object=objs[0], selected_editable_objects=objs,
                                       selected_objects=objs, object=objs[0]):
            bpy.ops.object.join()
    obj = objs[0]
    obj.name = obj.data.name = name
    for p in obj.data.polygons:
        p.use_smooth = True
    return obj


nodes = {g: join(objs, g) for g, objs in groups.items()}
hull = nodes['hull']
# A light decimation of the hull keeps the whole walker under 10k triangles with the extras.
mod = hull.modifiers.new('decimate', 'DECIMATE')
mod.ratio = 0.9
with bpy.context.temp_override(object=hull, active_object=hull):
    bpy.ops.object.modifier_apply(modifier=mod.name)


def uv_mask(obj, size):
    """Per texel of a size² map, 1 where `obj`'s UV triangles cover it."""
    out = np.zeros((size, size), np.float32)
    me = obj.data
    me.calc_loop_triangles()
    uvl = me.uv_layers.active.data
    for tri in me.loop_triangles:
        uv = np.array([uvl[i].uv[:] for i in tri.loops]) * size - 0.5
        (x0, y0), (x1, y1) = np.floor(uv.min(0)).astype(int) - 1, np.ceil(uv.max(0)).astype(int) + 1
        xs, ys = np.meshgrid(np.arange(max(x0, 0), min(x1, size - 1) + 1), np.arange(max(y0, 0), min(y1, size - 1) + 1))
        (ax, ay), (bx, by), (cx, cy) = uv
        den = (by - cy) * (ax - cx) + (cx - bx) * (ay - cy)
        if abs(den) < 1e-12:
            continue
        l0 = ((by - cy) * (xs - cx) + (cx - bx) * (ys - cy)) / den
        l1 = ((cy - ay) * (xs - cx) + (ax - cx) * (ys - cy)) / den
        on = (l0 > -0.1) & (l1 > -0.1) & (1 - l0 - l1 > -0.1)
        out[ys, xs] = np.maximum(out[ys, xs], on.astype(np.float32))
    return out


# --- Team colour: blue plates into the shared atlas's alpha; violet glass stays out.
def texture_of(mat, socket='Base Color'):
    bsdf = mat.node_tree.nodes['Principled BSDF']
    link = bsdf.inputs[socket].links
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


node = texture_of(MAT)
W, H = node.image.size
px = np.array(node.image.pixels[:], dtype=np.float32).reshape(H, W, 4)
rgb = px[:, :, :3]
r_, g_, b_ = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
violet = np.clip((b_ - g_ - 0.12) / 0.12, 0, 1) * np.clip((r_ - g_ - 0.04) / 0.08, 0, 1) * (b_ > 0.25)
blue = b_ - np.maximum(r_, g_)
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (b_ > 0.08) * (1.0 - violet) * np.clip((0.6 * b_ - r_) / 0.1, 0, 1)
# The hull's own furnace glass is dark (cold) glass; the glowing copy covers it while alive.
glass_islands = uv_mask(glass, W) if W == H else np.zeros((H, W), np.float32)
cold = rgb * (1.0 - 0.8 * (violet * glass_islands)[..., None])
team = bpy.data.images.new(NAME + '-team', W, H, alpha=True)
team.pixels[:] = np.concatenate([cold, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
GLOW_MAT = MAT.copy()  # before the rename: the furnace copy, emission added below
MAT.name = 'TC_' + NAME
MAT.use_backface_culling = True  # closed shells: single-sided
MAT['teamColorMask'] = 'baseColorAlpha'
print('atlas', W, H, 'team mask mean', round(float(mask.mean()), 4), 'violet mean', round(float(violet.mean()), 4))


# --- The furnace glow: an emission map of the violet texels of the window's own UV islands.
G = 512
small = rgb.reshape(G, H // G, G, W // G, 3).mean(axis=(1, 3)) if H % G == 0 and W % G == 0 else None
vsmall = violet.reshape(G, H // G, G, W // G).mean(axis=(1, 3))
lum = small @ np.array([0.2126, 0.7152, 0.0722], np.float32)
weight = uv_mask(glass, G) * np.clip(vsmall * 1.6 + np.clip((lum - 0.25) / 0.3, 0, 1) * vsmall, 0, 1)
print('furnace glow texels', int((weight > 0.5).sum()))
glow_rgb = weight[..., None] * np.array((0.62, 0.36, 1.0), np.float32)
gimg = bpy.data.images.new('furnace-glow', G, G, alpha=False)
gimg.pixels[:] = np.concatenate([glow_rgb, np.ones_like(glow_rgb[:, :, :1])], axis=2).ravel()
gimg.pack()
GLOW_MAT.name = 'juggernaut-furnace'
# The copy shares the atlas (its alpha is unused: the copy takes no team colour) over the
# cold glass; its emission is what glows.
tex = GLOW_MAT.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = gimg
bsdf = GLOW_MAT.node_tree.nodes['Principled BSDF']
GLOW_MAT.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 4.0
GLOW_MAT.use_backface_culling = True
glass.data.materials[0] = GLOW_MAT


# --- Hierarchy: hull at its middle; legs and cannons hang from it.
def set_pivot(obj, pivot_world, parent=None, rot_z=0.0):
    """Origin at `pivot_world`, local frame turned by rot_z; mesh unchanged in the world."""
    frame = mathutils.Matrix.Translation(pivot_world) @ mathutils.Matrix.Rotation(rot_z, 4, 'Z')
    obj.data.transform(frame.inverted())
    obj.rotation_mode = 'XYZ'
    if parent is not None:
        obj.parent = parent
        obj.matrix_parent_inverse = mathutils.Matrix.Identity(4)
        local = parent.matrix_world.inverted() @ frame
    else:
        local = frame
    obj.matrix_basis = local
    bpy.context.view_layer.update()
    return obj


_, hhi = bounds([hull])
HULL_PIVOT = V((0.0, 0.0, 3.6 * S))
set_pivot(hull, HULL_PIVOT)

legs = {}
for k, deg in LEGS.items():
    up, low = nodes[f'{k}_upper'], nodes[f'{k}_lower']
    vs_up = [v.co.copy() for v in up.data.vertices]
    vs_low = [v.co.copy() for v in low.data.vertices]
    radial = sorted(vs_up, key=lambda p: math.hypot(p.x, p.y))
    near = radial[:max(8, len(radial) // 8)]
    hip = sum(near, V()) / len(near)
    zmax = max(p.z for p in vs_low)
    top = [p for p in vs_low if p.z > zmax - 0.4 * S]
    knee = sum(top, V()) / len(top)
    foot = [p for p in vs_low if p.z < 0.3 * S]
    foot_c = sum(foot, V()) / len(foot)
    az = math.atan2(foot_c.y - hip.y, foot_c.x - hip.x)
    set_pivot(up, hip, hull, az)
    set_pivot(low, knee, up, az)  # the shin's frame along the leg, like the upper's
    legs[k] = {'upper': up, 'lower': low, 'az': az, 'hip': hip, 'knee': knee, 'foot': foot_c,
               'ground': min(p.z for p in vs_low)}
    print('leg', k, 'hip', [round(v, 2) for v in hip], 'knee', [round(v, 2) for v in knee],
          'foot', [round(v, 2) for v in foot_c], 'az', round(math.degrees(az)))

cannons = {}
for side in ('l', 'r'):
    cn = nodes[f'cannon_{side}']
    vs = [v.co.copy() for v in cn.data.vertices]
    ymin = min(p.y for p in vs)
    tip = [p for p in vs if p.y < ymin + 0.25 * S]
    muzzle = V((sum(p.x for p in tip) / len(tip), ymin, (min(p.z for p in tip) + max(p.z for p in tip)) / 2))
    c = sum(vs, V()) / len(vs)
    set_pivot(cn, V((muzzle.x, c.y, muzzle.z)), hull)
    cannons[side] = {'node': cn, 'muzzle': muzzle}
    print('cannon', side, 'muzzle', [round(v, 2) for v in muzzle], 'length', round(max(p.y for p in vs) - ymin, 2))

gc = centre(glass)
set_pivot(glass, gc, hull)
print('furnace', [round(v, 2) for v in gc])

# --- Muzzle flashes: flame cones in the barrels, hidden (scaled down) except at the shot.
FLASH = kit.material('juggernaut-flash', color=(1.0, 0.5, 0.14), roughness=1.0)
FLASH.node_tree.nodes['Principled BSDF'].inputs['Emission Color'].default_value = (1.0, 0.42, 0.08, 1.0)
FLASH.node_tree.nodes['Principled BSDF'].inputs['Emission Strength'].default_value = 6.0
FLASH['noShadow'] = True
FLASH.use_backface_culling = True


def flame(name):
    """A flame along -Y from its origin: a core cone and four petals."""
    pieces = [kit.cylinder(name + '-core', 0.75 * S, 3.2 * S, verts=12, r_top=0.03 * S)]
    for i in range(5):
        petal = kit.cylinder(f'{name}-petal{i}', 0.32 * S, 1.7 * S, verts=6, r_top=0.01 * S)
        petal.rotation_euler = (math.radians(62), 0, math.radians(36 + 72 * i))
        pieces.append(petal)
    for p in pieces[1:]:
        p.data.transform(mathutils.Euler(p.rotation_euler).to_matrix().to_4x4())
        p.rotation_euler = (0, 0, 0)
    obj = join(pieces, name)
    obj.data.transform(mathutils.Matrix.Rotation(math.radians(90), 4, 'X'))  # +Z -> -Y
    obj.data.materials.clear()
    obj.data.materials.append(FLASH)
    obj.data.uv_layers.new()
    kit.cube_uv(obj, 1.0)
    return obj


for side, cn in cannons.items():
    f = flame(f'flash_{side}')
    f.parent = cn['node']
    f.matrix_parent_inverse = mathutils.Matrix.Identity(4)
    f.location = cn['muzzle'] - (cn['node'].matrix_world.translation) + V((0, 0.25 * S, 0))
    f.rotation_mode = 'XYZ'
    cn['flash'] = f


# --- Sockets.
def socket(name, at_world, parent):
    e = kit.socket(name, (0, 0, 0))
    e.parent = parent
    e.matrix_parent_inverse = mathutils.Matrix.Identity(4)
    e.location = parent.matrix_world.inverted() @ at_world
    return e


bpy.context.view_layer.update()
for side, cn in cannons.items():
    socket(f'socket_muzzle_{side}', cn['muzzle'] - V((0, 0.3 * S, 0)), cn['node'])
mid = (cannons['l']['muzzle'] + cannons['r']['muzzle']) / 2
socket('socket_projectile', mid - V((0, 0.3 * S, 0)), hull)
socket('socket_furnace', gc - V((0, 0.5 * S, 0)), hull)
socket('socket_steam', V((0, hhi.y - 1.2 * S, hhi.z - 1.6 * S)), hull)
for k, leg in legs.items():
    socket(f'socket_foot_{k}', V((leg['foot'].x, leg['foot'].y, 0.05 * S)), leg['lower'])

# --- Motion: every channel sampled from functions of the frame, legs solved for the ground.
bpy.context.view_layer.update()
REST = {o.name: (o.location.copy(), o.rotation_euler.copy(), o.scale.copy()) for o in bpy.data.objects}


def verts(obj):
    a = np.array([v.co[:] + (1.0,) for v in obj.data.vertices], np.float64)
    return a.T


LEG_VERTS = {k: (verts(l['upper']), verts(l['lower'])) for k, l in legs.items()}
BODY_VERTS = [(hull, verts(hull))] + [(c['node'], verts(c['node'])) for c in cannons.values()]


def mat(loc, rot):
    return np.array(mathutils.Matrix.Translation(loc) @ mathutils.Euler(rot, 'XYZ').to_matrix().to_4x4())


def hull_matrix(pose):
    rest = REST[hull.name]
    return mat(rest[0] + V(pose.get('hull_loc', (0, 0, 0))), (pose.get('pitch', 0.0), pose.get('roll', 0.0), 0.0))


def leg_matrices(hm, k, lift, yaw, knee):
    up, low = legs[k]['upper'], legs[k]['lower']
    um = hm @ mat(REST[up.name][0], (0.0, lift, legs[k]['az'] + yaw))
    lm = um @ mat(REST[low.name][0], (0.0, knee, 0.0))
    return um, lm


def leg_floor(hm, k, lift, yaw, knee):
    um, lm = leg_matrices(hm, k, lift, yaw, knee)
    vu, vl = LEG_VERTS[k]
    return min((um @ vu)[2].min(), (lm @ vl)[2].min())


def contact_lift(hm, k, yaw, knee):
    """The lift that puts leg k's lowest point on its rest ground height."""
    target = legs[k]['ground']
    a, b = -0.9, 0.9  # floor falls as lift grows
    for _ in range(30):
        m = (a + b) / 2
        if leg_floor(hm, k, m, yaw, knee) > target:
            a = m
        else:
            b = m
    return (a + b) / 2


def body_floor(pose):
    hm = hull_matrix(pose)
    low = (hm @ BODY_VERTS[0][1])[2].min()
    for side in ('l', 'r'):
        cnode = cannons[side]['node']
        cm = hm @ mat(REST[cnode.name][0], (pose.get('cannon_pitch', 0.0), 0, 0))
        low = min(low, (cm @ verts(cnode))[2].min())
    return low


def smooth(keys, f):
    """Smoothstep between (frame, value) keys, held outside them."""
    if f <= keys[0][0]:
        return keys[0][1]
    for (f0, v0), (f1, v1) in zip(keys, keys[1:]):
        if f <= f1:
            t = (f - f0) / (f1 - f0)
            t = t * t * (3 - 2 * t)
            if isinstance(v0, tuple):
                return tuple(a + (b - a) * t for a, b in zip(v0, v1))
            return v0 + (v1 - v0) * t
    return keys[-1][1]


CLIPS = []


def bake(name, length, pose_at, step=2):
    """Keys every node's channels for clip `name` from pose_at(frame) -> pose dict:
    hull_loc, pitch, roll; legs {k: (yaw, extra lift, knee)} (lift solved for the ground);
    cannon_y, cannon_pitch, flash (scale), glow_y."""
    frames = sorted(set(list(range(0, length + 1, step)) + [length]))
    keys = {}

    def put(obj, path, index, f, value):
        keys.setdefault((obj.name, path, index), []).append((f, value))

    worst, worst_at = 0.0, None
    for f in frames:
        pose = pose_at(f)
        hm = hull_matrix(pose)
        rest = REST[hull.name]
        loc = rest[0] + V(pose.get('hull_loc', (0, 0, 0)))
        for i in range(3):
            put(hull, 'location', i, f, loc[i])
        put(hull, 'rotation_euler', 0, f, pose.get('pitch', 0.0))
        put(hull, 'rotation_euler', 1, f, pose.get('roll', 0.0))
        for k in legs:
            yaw, extra, knee = pose.get('legs', {}).get(k, (0.0, 0.0, 0.0))
            lift = contact_lift(hm, k, yaw, knee) - extra
            if max(abs(lift), abs(yaw), abs(knee)) > worst:
                worst = max(abs(lift), abs(yaw), abs(knee))
                worst_at = (k, f, round(lift, 3), round(yaw, 3), round(knee, 3))
            put(legs[k]['upper'], 'rotation_euler', 1, f, lift)
            put(legs[k]['upper'], 'rotation_euler', 2, f, legs[k]['az'] + yaw)
            put(legs[k]['lower'], 'rotation_euler', 1, f, knee)
        for side, cn in cannons.items():
            rest_c = REST[cn['node'].name][0]
            put(cn['node'], 'location', 1, f, rest_c.y + pose.get('cannon_y', 0.0))
            put(cn['node'], 'rotation_euler', 0, f, pose.get('cannon_pitch', 0.0))
            s = max(pose.get('flash', 0.0), 0.002)
            for i in range(3):
                put(cn['flash'], 'scale', i, f, s if i != 1 else s * (0.9 + 0.3 * s))
        put(glass, 'location', 1, f, REST[glass.name][0].y + pose.get('glow_y', 0.0))
    for (oname, path, index), ks in keys.items():
        obj = bpy.data.objects[oname]
        ad = obj.animation_data_create()
        action = bpy.data.actions.get(name) or bpy.data.actions.new(name)
        ad.action = action
        for f, value in ks:
            getattr(obj, path)[index] = value
            obj.keyframe_insert(data_path=path, index=index, frame=f)
        for fc in kit._fcurves(action):
            if fc.data_path == path and fc.array_index == index:
                for kp in fc.keyframe_points:
                    kp.interpolation = 'LINEAR'
    for obj in {bpy.data.objects[o] for o, _, _ in keys}:
        ad = obj.animation_data
        track = ad.nla_tracks.new()
        track.name = name
        track.strips.new(name, 0, ad.action)
        ad.action = None
    CLIPS.append(name)
    print('clip', name, length, 'frames; largest leg angle off rest', round(worst, 3), worst_at)


# Walk: alternating tripods, 60 frames (2 s of clip; the game plays clips at 1.5x).
WALK = 60
YAW, LIFT, TUCK = 0.16, 0.22, 0.2
BOB_DOWN, BOB_UP, ROLL = 0.09 * S, 0.06 * S, 0.03


def walk_pose(f):
    pose = {'legs': {}}
    half = WALK / 2
    for k in legs:
        phase = 0 if k in TRIPOD_A else half
        t = (f - phase) % WALK
        forward = 1.0 if math.cos(legs[k]['az']) < 0 else -1.0  # yaw sign that moves the foot to -Y
        if t < half:  # swing: lift and reach forward
            u = t / half
            s = u * u * (3 - 2 * u)
            yaw = -YAW + 2 * YAW * s
            arc = math.sin(math.pi * u)
            pose['legs'][k] = (forward * yaw, LIFT * arc, TUCK * arc)
        else:  # stance: push back
            u = (t - half) / half
            pose['legs'][k] = (forward * (YAW - 2 * YAW * u), 0.0, 0.0)
    # The hull drops as each tripod plants and rides up mid-stride; leans over the stance side.
    w = 2 * math.pi * f / half
    z = -BOB_DOWN * (0.5 + 0.5 * math.cos(w)) + BOB_UP * (0.5 - 0.5 * math.cos(w))
    pose['hull_loc'] = (0.0, 0.03 * S * math.sin(w), z)
    pose['roll'] = ROLL * math.sin(2 * math.pi * f / WALK)
    pose['pitch'] = 0.012 * math.sin(w)
    return pose


# Idle: the hull breathes on its legs.
IDLE = 90


def idle_pose(f):
    w = 2 * math.pi * f / IDLE
    return {'hull_loc': (0.0, 0.0, 0.035 * S * math.sin(w)), 'pitch': 0.006 * math.sin(w)}


# Attack: brace, both cannons fire at RELEASE, the recoil rocks the hull back.
ATTACK, RELEASE = 36, 10


def attack_pose(f):
    return {
        'hull_loc': (0.0, smooth([(0, 0.0), (8, -0.03 * S), (RELEASE, -0.03 * S), (13, 0.2 * S), (22, -0.02 * S), (32, 0.0)], f),
                     smooth([(0, 0.0), (8, -0.07 * S), (RELEASE, -0.07 * S), (13, 0.03 * S), (22, -0.02 * S), (32, 0.0)], f)),
        'pitch': smooth([(0, 0.0), (8, 0.02), (RELEASE, 0.02), (13, -0.07), (22, 0.015), (32, 0.0)], f),
        'cannon_y': smooth([(0, 0.0), (RELEASE, 0.0), (RELEASE + 1, 0.6 * S), (RELEASE + 6, 0.55 * S), (30, 0.0)], f),
        'flash': smooth([(0, 0.0), (RELEASE, 0.0), (RELEASE + 1, 1.15), (RELEASE + 3, 0.8), (RELEASE + 5, 0.0)], f),
    }


# Hit: a jolt that rocks the hull and settles.
HIT = 18


def hit_pose(f):
    return {
        'hull_loc': (0.0, smooth([(0, 0.0), (3, 0.14 * S), (8, -0.04 * S), (14, 0.01 * S), (HIT, 0.0)], f),
                     smooth([(0, 0.0), (3, -0.06 * S), (8, 0.02 * S), (HIT, 0.0)], f)),
        'roll': smooth([(0, 0.0), (3, 0.05), (8, -0.025), (13, 0.008), (HIT, 0.0)], f),
        'pitch': smooth([(0, 0.0), (3, -0.03), (9, 0.012), (HIT, 0.0)], f),
    }


# Death: the front legs give, then all six splay; the hull crashes down, bounces and settles
# nose-down; the furnace sinks dark.
DEATH = 72
final = {'pitch': 0.09, 'roll': 0.08, 'cannon_pitch': -0.12}
a, b = 0.0, 4.0 * S
for _ in range(30):  # the drop that sets the lowest point of hull and cannons just on the ground
    m = (a + b) / 2
    if body_floor(dict(final, hull_loc=(0, 0, -m))) > 0.06 * S:
        a = m
    else:
        b = m
DROP = (a + b) / 2
print('death drop', round(DROP, 2))
FRONT = ('fl', 'fr')


def death_pose(f):
    drop = smooth([(0, 0.0), (8, 0.12 * DROP), (20, 0.4 * DROP), (36, DROP), (41, DROP - 0.16 * S), (48, DROP), (DEATH, DROP)], f)
    pose = {
        'hull_loc': (smooth([(0, 0.0), (36, 0.1 * S), (DEATH, 0.1 * S)], f), smooth([(0, 0.0), (20, -0.2 * S), (36, -0.3 * S), (DEATH, -0.3 * S)], f), -drop),
        'pitch': smooth([(0, 0.0), (20, 0.12), (36, final['pitch']), (41, final['pitch'] - 0.02), (48, final['pitch']), (DEATH, final['pitch'])], f),
        'roll': smooth([(0, 0.0), (14, -0.03), (36, final['roll']), (DEATH, final['roll'])], f),
        'cannon_pitch': smooth([(0, 0.0), (20, -0.05), (36, final['cannon_pitch']), (DEATH, final['cannon_pitch'])], f),
        'glow_y': smooth([(0, 0.0), (22, 0.0), (40, 0.6 * S), (DEATH, 0.6 * S)], f),
        'legs': {},
    }
    for k in legs:
        knee = smooth([(0, 0.0), (20, -0.28 if k in FRONT else -0.06), (36, -0.32), (DEATH, -0.32)], f)
        yaw = smooth([(0, 0.0), (36, 0.08 if math.cos(legs[k]['az']) < 0 else -0.08), (DEATH, 0.08 if math.cos(legs[k]['az']) < 0 else -0.08)], f)
        pose['legs'][k] = (yaw, 0.0, knee)
    # Mid-fall, a buckling leg is briefly off the ground before the hull lands on it.
    if 6 < f < 34:
        for k in FRONT:
            yaw, _, knee = pose['legs'][k]
            pose['legs'][k] = (yaw, 0.1 * math.sin(math.pi * (f - 6) / 28), knee)
    return pose


bake('walk', WALK, walk_pose)
bake('attack', ATTACK, attack_pose)
bake('hit', HIT, hit_pose)
bake('death', DEATH, death_pose)
bake('idle', IDLE, idle_pose)  # last: its first frame is the rest pose the file keeps

# Back to the rest pose for the exported nodes.
for o in bpy.data.objects:
    if o.name in REST:
        o.location, o.rotation_euler, o.scale = REST[o.name]
for side in ('l', 'r'):
    cannons[side]['flash'].scale = (0.002, 0.002, 0.002)
bpy.context.scene.frame_set(0)

tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
print('triangles', tris)
sockets = sorted(o.name for o in bpy.data.objects if o.name.startswith('socket_'))
asset = {
    'id': 'asset.models.units.' + NAME, 'name': 'Juggernaut', 'kind': 'unit',
    'tags': ['beetles', 'machine', 'tripo', 'animated', 'team-mask'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'geometry', 'index': 2, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': [MAT.name]},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            # Event times are seconds into the clip.
            {'semantic': 'walk', 'clip': 'walk', 'loop': True,
             'events': [{'name': 'step', 'time': round((WALK / 2 - 1) / FPS, 3)},
                        {'name': 'step', 'time': round((WALK - 1) / FPS, 3)}]},
            {'semantic': 'run', 'clip': 'walk', 'loop': True,
             'events': [{'name': 'step', 'time': round((WALK / 2 - 1) / FPS, 3)},
                        {'name': 'step', 'time': round((WALK - 1) / FPS, 3)}]},
            {'semantic': 'attack', 'clip': 'attack', 'loop': False,
             'events': [{'name': 'release', 'time': round(RELEASE / FPS, 3)}]},
            {'semantic': 'hit', 'clip': 'hit', 'loop': False, 'events': []},
            {'semantic': 'death', 'clip': 'death', 'loop': False,
             'events': [{'name': 'crash', 'time': round(36 / FPS, 3)}]},
        ],
        'sockets': [{'name': s, 'node': s, 'offset': [0, 0, 0]} for s in sockets],
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'character': 'base', 'healthHeight': round(HEIGHT + 0.6, 1)},
    'license': 'Generated with Tripo (image to 3D, detailed segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/units/juggernaut.py.',
}
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)


# --- The shell: a heavy iron ball with a brass driving band and a glowing fuse.
kit.reset()
iron = kit.material('shell-iron', color=(0.07, 0.07, 0.08), metallic=0.85, roughness=0.38)
brass = kit.material('shell-brass', color=(0.78, 0.56, 0.22), metallic=0.9, roughness=0.32)
fuse = kit.material('shell-fuse', color=(1.0, 0.6, 0.2), roughness=1.0)
fuse.node_tree.nodes['Principled BSDF'].inputs['Emission Color'].default_value = (1.0, 0.5, 0.12, 1.0)
fuse.node_tree.nodes['Principled BSDF'].inputs['Emission Strength'].default_value = 10.0
R = 0.5
bm = bmesh.new()
bmesh.ops.create_uvsphere(bm, u_segments=20, v_segments=12, radius=R)
me = bpy.data.meshes.new('shell')
bm.to_mesh(me)
bm.free()
ball = kit.link(bpy.data.objects.new('shell', me))
kit.finish(ball, iron, uv=1.0)
band = kit.torus('band', R * 0.98, 0.07, segments=24, ring=6)
kit.finish(band, brass, uv=1.0)
kit.finish(kit.cylinder('fuse', 0.12, 0.14, at=(0, 0, R * 0.92), verts=8), fuse, uv=1.0)
kit.finish(kit.cylinder('base-plug', 0.12, 0.14, at=(0, 0, -R * 0.92 - 0.14), verts=8), brass, uv=1.0)
bpy.ops.export_scene.gltf(
    filepath=os.path.join(OUT, 'geometry_2.glb'), export_format='GLB', export_yup=True,
    export_apply=True, export_extras=True, export_animations=False)
print('shell', sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH'), 'triangles')
