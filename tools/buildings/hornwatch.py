"""The beetles' Hornwatch (the defensive tower) from Tripo, made game-ready and animated in
Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/hornwatch.py -- \\
        <beetle-hornwatch-segmented.glb>

The tower is Tripo's image-to-3D model of the approved concept
(art/styles/beetle-hornwatch-idle.png), split by Tripo's segmentation into parts. Here (D65):
- the one shared texture atlas gets the team mask (royal-blue plates) in its alpha;
- the turret (the domed cap with the cannon) is its own node `turret`, its pivot on the
  tower's axis, so it can turn to face a target; the cannon is a child node `barrel` that
  kicks back on every shot (`attack`, and the `work` loop while the tower fires);
- the glass of the power canister on the right is its own node, and its texels of the
  atlas get an active blue-violet glow: dark while the tower rests, alive while it works;
- sockets: `socket_muzzle` at the cannon mouth (on the turret, so it turns with it) and
  `socket_canister` in the canister glass.
At rest nothing moves and nothing glows."""
import math, os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
TOWER = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-hornwatch'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 8.0  # fills its 8-cell footprint: buildings have no walkable perimeter
S = WIDTH / 8.0  # offsets below are in metres at the 8-metre build

# Segmented parts by role (tripo_part_<n>, this export only).
TURRET = 'tripo_part_1'  # domed cap and cannon, split below into turret and barrel
CANISTER = 'tripo_part_3'  # the power canister with its pipes, right side (+X)
# Static: 0 plinth, 2 upper body, 4/6/9 lower body, 5/7 horns, 8/10-13 trims and pipes.

kit.reset()
bpy.ops.import_scene.gltf(filepath=TOWER)
for o in list(bpy.context.scene.objects):
    if o.type != 'MESH':
        for c in o.children:
            m = c.matrix_world.copy()
            c.parent = None
            c.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
parts = [o for o in bpy.context.scene.objects if o.type == 'MESH']
# Normalise: footprint centred, ground at 0, WIDTH across, transforms applied. The plinth is
# Tripo's own clean iron base; there is no dirt to strip.
pts = [o.matrix_world @ v.co for o in parts for v in o.data.vertices]
lo = mathutils.Vector([min(p[i] for p in pts) for i in range(3)])
hi = mathutils.Vector([max(p[i] for p in pts) for i in range(3)])
scale = WIDTH / max(hi.x - lo.x, hi.y - lo.y)
shift = mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation((-(lo.x + hi.x) / 2, -(lo.y + hi.y) / 2, -lo.z))
for o in parts:
    o.data.transform(shift @ o.matrix_world)
    o.matrix_world = mathutils.Matrix.Identity(4)
    for p in o.data.polygons:
        p.use_smooth = True
by_name = {o.name: o for o in parts}
height = (hi.z - lo.z) * scale


def texture_of(mat, socket='Base Color'):
    bsdf = mat.node_tree.nodes['Principled BSDF']
    link = bsdf.inputs[socket].links
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


def pixels(img):
    w, h = img.size
    return np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)


def split(obj, name, keep):
    """Moves the faces of `obj` whose world centre passes `keep(centre)` into a new object."""
    bpy.ops.object.select_all(action='DESELECT')
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    bpy.ops.object.mode_set(mode='EDIT')
    bm = bmesh.from_edit_mesh(obj.data)
    for f in bm.faces:
        f.select_set(False)
    for f in bm.faces:
        if keep(f.calc_center_median()):
            f.select_set(True)
    bmesh.update_edit_mesh(obj.data)
    bpy.ops.mesh.separate(type='SELECTED')
    bpy.ops.object.mode_set(mode='OBJECT')
    new = [o for o in bpy.context.selected_objects if o is not obj][0]
    new.name = name
    new.data.name = name
    return new


def rehome(obj, origin):
    """Puts the node origin at `origin` (world) without moving the mesh."""
    obj.data.transform(mathutils.Matrix.Translation(-mathutils.Vector(origin)))
    obj.location = origin
    obj.rotation_mode = 'XYZ'  # glTF imports use quaternions; the clips key Euler angles


# --- Turret and barrel. The cannon lies along Y with its mouth at −Y (the front), above
# the dome (whose top is at about 7.3 m); its mouth hangs down in front of the dome.
cap = by_name[TURRET]
barrel = split(cap, 'barrel', lambda c: c.z > 7.45 * S or (c.y < -1.5 * S and c.z > 6.9 * S))
cap.name = cap.data.name = 'turret'
dome = [v.co for v in cap.data.vertices if v.co.z < 6.3 * S]
axis = mathutils.Vector(((min(v.x for v in dome) + max(v.x for v in dome)) / 2,
                         (min(v.y for v in dome) + max(v.y for v in dome)) / 2,
                         min(v.co.z for v in cap.data.vertices)))
rehome(cap, axis)
bv = [v.co for v in barrel.data.vertices]
mouth_y = min(v.y for v in bv)
mouth = [v for v in bv if v.y < mouth_y + 0.35 * S]
mouth_c = mathutils.Vector((sum(v.x for v in mouth) / len(mouth), mouth_y,
                            (min(v.z for v in mouth) + max(v.z for v in mouth)) / 2))
top = [v for v in bv if v.z > 7.6 * S]
# The trunnion: on the tower's axis, at the height of the barrel's bore line.
trunnion = mathutils.Vector((sum(v.x for v in top) / len(top), axis.y, mouth_c.z))
rehome(barrel, trunnion)
m = barrel.matrix_world.copy()
barrel.parent = cap
barrel.matrix_world = m
barrel.matrix_parent_inverse = mathutils.Matrix.Identity(4)
barrel.location = trunnion - axis
print('turret axis', [round(v, 2) for v in axis], 'trunnion', [round(v, 2) for v in trunnion],
      'mouth', [round(v, 2) for v in mouth_c])

# --- The canister glass: the cylinder wall of the canister between its brass caps.
can = by_name[CANISTER]
# Its wall: a least-squares circle through the face centres of a slice through the glass.
ring = np.array([(p.center.x, p.center.y) for p in can.data.polygons
                 if 2.4 * S < p.center.z < 3.2 * S and p.center.x > 1.95 * S])
sol = np.linalg.lstsq(np.c_[2 * ring, np.ones(len(ring))], (ring ** 2).sum(axis=1), rcond=None)[0]
can_axis = mathutils.Vector((sol[0], sol[1], 0))
can_r = math.sqrt(sol[2] + sol[0] ** 2 + sol[1] ** 2)
GLASS_Z = (1.8 * S, 3.95 * S)  # between the foot ring and the brass cap
glass = split(can, 'canister-glass',
              lambda c: GLASS_Z[0] < c.z < GLASS_Z[1]
              and abs((mathutils.Vector((c.x, c.y, 0)) - can_axis).length - can_r) < 0.16 * S)
print('canister axis', [round(v, 2) for v in can_axis], 'r', round(can_r, 2), 'glass faces', len(glass.data.polygons))

# --- Team colour: one shared atlas, so its alpha takes the mask once for every part.
mats = {o.data.materials[0] for o in bpy.context.scene.objects if o.type == 'MESH'}
print('materials', [m.name for m in mats])
atlas = next(iter(mats))
node = texture_of(atlas)
px = pixels(node.image)
rgb = px[:, :, :3]
blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08)
team = bpy.data.images.new('hornwatch-team', node.image.size[0], node.image.size[1], alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
atlas.name = 'TC_hornwatch'
atlas['teamColorMask'] = 'baseColorAlpha'
print('team mask mean', round(float(mask.mean()), 3))

# Active glow on the one material, so the atlas is not duplicated: an emission texture that
# is black except on the texels the canister glass maps to, where the dark glass glows
# blue-violet from within (brass ribs stay brass).
h, w = rgb.shape[:2]
covered = np.zeros((h, w), bool)
uvs = glass.data.uv_layers.active.data
for poly in glass.data.polygons:
    tri = [uvs[i].uv for i in poly.loop_indices]
    for k in range(1, len(tri) - 1):
        (u0, v0), (u1, v1), (u2, v2) = [(t[0] * w, t[1] * h) for t in (tri[0], tri[k], tri[k + 1])]
        x0, x1 = int(max(0, min(u0, u1, u2) - 1)), int(min(w - 1, max(u0, u1, u2) + 1))
        y0, y1 = int(max(0, min(v0, v1, v2) - 1)), int(min(h - 1, max(v0, v1, v2) + 1))
        xs, ys = np.meshgrid(np.arange(x0, x1 + 1) + 0.5, np.arange(y0, y1 + 1) + 0.5)
        d = (v1 - v2) * (u0 - u2) + (u2 - u1) * (v0 - v2)
        if abs(d) < 1e-9:
            continue
        a = ((v1 - v2) * (xs - u2) + (u2 - u1) * (ys - v2)) / d
        b = ((v2 - v0) * (xs - u2) + (u0 - u2) * (ys - v2)) / d
        covered[y0:y1 + 1, x0:x1 + 1] |= (a > -0.02) & (b > -0.02) & (1 - a - b > -0.02)
for _ in range(3):  # bleed a few texels so mips and filtering keep the edges lit
    covered = covered | np.roll(covered, 1, 0) | np.roll(covered, -1, 0) | np.roll(covered, 1, 1) | np.roll(covered, -1, 1)
lum = rgb @ np.array([0.2126, 0.7152, 0.0722], np.float32)
sat = rgb.max(axis=2) - rgb.min(axis=2)
weight = np.clip((0.22 - lum) / 0.12, 0.0, 1.0) * np.clip((0.22 - sat) / 0.1, 0.0, 1.0) * covered
glow = weight[..., None] * np.array((0.55, 0.32, 1.0), np.float32)
print('glass texels', round(float(covered.mean()), 4))
img = bpy.data.images.new('hornwatch-glass-glow', w, h, alpha=False)
img.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
img.pack()
tex = atlas.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = img
bsdf = atlas.node_tree.nodes['Principled BSDF']
atlas.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 4.0
atlas['activeGlow'] = {'pulse': 0.9, 'flow': 0.0}

# --- Sockets. The muzzle socket rides on the turret, so it turns with it but does not jump
# back with the recoil (the flash stays where the shot left).
muzzle = kit.socket('socket_muzzle', mouth_c - mathutils.Vector((0, 0.3 * S, 0)))
muzzle.parent = cap
muzzle.matrix_parent_inverse = mathutils.Matrix.Identity(4)
muzzle.location = mouth_c - mathutils.Vector((0, 0.3 * S, 0)) - axis
kit.socket('socket_canister', (can_axis.x, can_axis.y, (GLASS_Z[0] + GLASS_Z[1]) / 2))


# --- Clips: the barrel kicks back along +Y (away from its mouth) and its mouth lifts, then
# it runs back out. Several channels per clip, so a local helper instead of kit.clip.
def clip(obj, name, channels):
    obj.animation_data_create()
    action = bpy.data.actions.new(name)
    obj.animation_data.action = action
    for (path, index), keys in channels.items():
        for frame, value in keys:
            getattr(obj, path)[index] = value
            obj.keyframe_insert(data_path=path, index=index, frame=frame)
    for fc in kit._fcurves(action):
        for kp in fc.keyframe_points:
            kp.interpolation = 'BEZIER'
    track = obj.animation_data.nla_tracks.new()
    track.name = name
    track.strips.new(name, 0, action)
    obj.animation_data.action = None
    return action


REST_Y, REST_P = barrel.location.y, 0.0
KICK, LIFT = 0.75 * S, -0.08  # metres back; radians (negative lifts the mouth at −Y)
FIRE = 2  # the recoil moment, frames


def shot(end):
    """One shot: still, the kick at FIRE, a short hold, the run back out by `end`."""
    y = [(0, REST_Y), (FIRE, REST_Y), (FIRE + 2, REST_Y + KICK), (FIRE + 5, REST_Y + KICK * 0.92),
         (FIRE + 20, REST_Y), (end, REST_Y)]
    p = [(0, REST_P), (FIRE, REST_P), (FIRE + 2, LIFT), (FIRE + 6, LIFT * 0.5), (FIRE + 16, REST_P),
         (end, REST_P)]
    return {('location', 1): y, ('rotation_euler', 0): p}


clip(barrel, 'idle', {('location', 1): [(0, REST_Y), (2, REST_Y)], ('rotation_euler', 0): [(0, REST_P), (2, REST_P)]})
ATTACK, WORK = 24, 36  # 0.8 s one shot; the work loop fires once every 1.2 s (clip time)
clip(barrel, 'attack', shot(ATTACK))
clip(barrel, 'work', shot(WORK))
# The kick is snappy, the return eased: the kick keys get linear in.
for action in (bpy.data.actions['attack'], bpy.data.actions['work']):
    for fc in kit._fcurves(action):
        for kp in fc.keyframe_points:
            if FIRE <= kp.co.x < FIRE + 2:
                kp.interpolation = 'LINEAR'

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Hornwatch', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask', 'tower'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': ['TC_hornwatch']},
        'animations': [
            {'semantic': 'idle', 'clip': 'idle', 'loop': True, 'events': []},
            # Event times are seconds into the clip.
            {'semantic': 'work', 'clip': 'work', 'loop': True,
             'events': [{'name': 'fire', 'time': round(FIRE / kit.FPS, 3)}]},
            {'semantic': 'attack', 'clip': 'attack', 'loop': False,
             'events': [{'name': 'fire', 'time': round(FIRE / kit.FPS, 3)}]},
        ],
        'blocker': {'shape': 'box', 'size': [8, 8], 'offset': [0, 0]},
        'vegetationClearance': 4,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': 1, 'healthHeight': round(height + 0.6, 1)},
    'shading': 'building',
    'license': 'Generated with Tripo (image to 3D, segmentation) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; animated by tools/buildings/hornwatch.py.',
}
kit.export(OUT, NAME, asset)
