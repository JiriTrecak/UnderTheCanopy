"""The beetles' Arc Tower (the Hornwatch's tier-3 refit) from Tripo, made game-ready and
animated in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/buildings/arc_tower.py -- <arc-tower.glb>

The tower is Tripo's image-to-3D model of the chosen concept (the old repository's
.asset-work/concepts/buildings/beetle-arc-tower-work-2.png): the Hornwatch's armoured base with
a substance coil in place of its cannon. One textured mesh, unsegmented; parts are split here by
position. Here (D65):
- it fills its 8-cell footprint and the royal-blue plates become the team mask;
- the violet glass of the coil and the side canister is dimmed in the base colour and given an
  active glow instead: dark at rest, alive while the tower works;
- the crown (the brass sphere and its horns, above the top coil ring) is its own node `crown`
  and jumps on every discharge (`attack`, and the `work` loop while the tower fires);
- sockets: `socket_muzzle` in the sphere, where arcs leave, and `socket_coil` in the glass.
At rest nothing moves and nothing glows."""
import os, sys

import bmesh
import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
GLB = sys.argv[sys.argv.index('--') + 1:][0]
NAME = 'beetles-arc-tower'
OUT = os.path.join(ROOT, 'base', 'assets', 'buildings', NAME)
WIDTH = 8.0  # fills its 8-cell footprint, like the Hornwatch it replaces
V = mathutils.Vector

kit.reset()
bpy.ops.import_scene.gltf(filepath=GLB)
meshes = [o for o in bpy.context.scene.objects if o.type == 'MESH']
for o in meshes:
    m = o.matrix_world.copy()
    o.parent = None
    o.matrix_world = m
for o in [o for o in bpy.context.scene.objects if o.type != 'MESH']:
    bpy.data.objects.remove(o, do_unlink=True)
bpy.ops.object.select_all(action='DESELECT')
for o in meshes:
    o.select_set(True)
bpy.context.view_layer.objects.active = meshes[0]
if len(meshes) > 1:
    bpy.ops.object.join()
tower = bpy.context.view_layer.objects.active
tower.name = tower.data.name = 'tower'
bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)

# Normalise: footprint centred, ground at 0, WIDTH across.
pts = np.array([v.co[:] for v in tower.data.vertices])
lo, hi = pts.min(0), pts.max(0)
scale = WIDTH / max(hi[0] - lo[0], hi[1] - lo[1])
tower.data.transform(mathutils.Matrix.Scale(scale, 4) @ mathutils.Matrix.Translation(
    (-(lo[0] + hi[0]) / 2, -(lo[1] + hi[1]) / 2, -lo[2])))
for p in tower.data.polygons:
    p.use_smooth = True
pts = np.array([v.co[:] for v in tower.data.vertices])
height = float(pts[:, 2].max())
print('height', round(height, 2))


def split(obj, name, keep):
    """Moves the faces of `obj` whose centre passes `keep(centre)` into a new object."""
    bpy.ops.object.select_all(action='DESELECT')
    bpy.context.view_layer.objects.active = obj
    obj.select_set(True)
    bpy.ops.object.mode_set(mode='EDIT')
    bm = bmesh.from_edit_mesh(obj.data)
    for f in bm.faces:
        f.select_set(keep(f.calc_center_median()))
    bmesh.update_edit_mesh(obj.data)
    bpy.ops.mesh.separate(type='SELECTED')
    bpy.ops.object.mode_set(mode='OBJECT')
    new = [o for o in bpy.context.selected_objects if o is not obj][0]
    new.name = new.data.name = name
    return new


# --- The crown: everything above the coil's top ring, near the tower's axis (the sphere and
# its horns). The coil axis is the centre of the tall glass column's vertices.
column = pts[(pts[:, 2] > height * 0.45) & (pts[:, 2] < height * 0.75)]
axis = V((float(np.median(column[:, 0])), float(np.median(column[:, 1])), 0))
CROWN_Z = height * 0.8
crown = split(tower, 'crown', lambda c: c.z > CROWN_Z and (V((c.x, c.y, 0)) - axis).length < WIDTH * 0.3)
cv = np.array([v.co[:] for v in crown.data.vertices])
pivot = V((axis.x, axis.y, float(cv[:, 2].min())))
crown.data.transform(mathutils.Matrix.Translation(-pivot))
crown.location = pivot
crown.rotation_mode = 'XYZ'
sphere_z = float(np.percentile(cv[:, 2], 70))
print('axis', [round(v, 2) for v in axis], 'crown faces', len(crown.data.polygons), 'from z', round(pivot.z, 2))

# --- Team colour and the glass glow, on the one shared texture.
mat = tower.data.materials[0]
bsdf = mat.node_tree.nodes['Principled BSDF']
node = bsdf.inputs['Base Color'].links[0].from_node
while node.type != 'TEX_IMAGE':
    node = node.inputs[0].links[0].from_node
w, h = node.image.size
px = np.array(node.image.pixels[:], np.float32).reshape(h, w, 4)
rgb = px[:, :, :3].copy()
r, g, b = rgb[:, :, 0], rgb[:, :, 1], rgb[:, :, 2]
violet = (r > g * 1.6) & (b > 0.25) & (b > g * 2.0)  # the substance glass: red and blue, no green
blue = b - np.maximum(r, g)
mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (b > 0.08) * ~violet
glow = np.where(violet[..., None], rgb * 1.2, 0.0).astype(np.float32)
rgb[violet] *= 0.3  # dim glass at rest; the emission brings it back while working
team = bpy.data.images.new(NAME + '-team', w, h, alpha=True)
team.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
team.alpha_mode = 'CHANNEL_PACKED'
team.pack()
node.image = team
mat.name = 'TC_' + NAME
mat['teamColorMask'] = 'baseColorAlpha'
img = bpy.data.images.new(NAME + '-glow', w, h, alpha=False)
img.pixels[:] = np.concatenate([glow, np.ones_like(glow[:, :, :1])], axis=2).ravel()
img.pack()
tex = mat.node_tree.nodes.new('ShaderNodeTexImage')
tex.image = img
mat.node_tree.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
bsdf.inputs['Emission Strength'].default_value = 4.0
mat['activeGlow'] = {'pulse': 0.9, 'flow': 0.0}
print('team mask', round(float(mask.mean()), 3), 'glass texels', round(float(violet.mean()), 4))

# --- Sockets: the sphere (rides the crown) and the glass column.
muzzle = kit.socket('socket_muzzle', V((axis.x, axis.y, sphere_z)))
muzzle.parent = crown
muzzle.matrix_parent_inverse = mathutils.Matrix.Identity(4)
muzzle.location = V((0, 0, sphere_z - pivot.z))
kit.socket('socket_coil', V((axis.x, axis.y, height * 0.6)))


# --- Clips: the crown jumps on each discharge and settles.
def clip(obj, name, keys):
    obj.animation_data_create()
    action = bpy.data.actions.new(name)
    obj.animation_data.action = action
    for frame, value in keys:
        obj.location[2] = pivot.z + value
        obj.keyframe_insert(data_path='location', index=2, frame=frame)
    track = obj.animation_data.nla_tracks.new()
    track.name = name
    track.strips.new(name, 0, action)
    obj.animation_data.action = None


FIRE = 2
JUMP = 0.25


def discharge(end):
    return [(0, 0.0), (FIRE, 0.0), (FIRE + 2, JUMP), (FIRE + 6, JUMP * 0.4), (FIRE + 14, 0.0), (end, 0.0)]


clip(crown, 'idle', [(0, 0.0), (2, 0.0)])
ATTACK, WORK = 24, 36
clip(crown, 'attack', discharge(ATTACK))
clip(crown, 'work', discharge(WORK))

asset = {
    'id': 'asset.models.buildings.' + NAME, 'name': 'Arc Tower', 'kind': 'building',
    'tags': ['beetles', 'tripo', 'animated', 'team-mask', 'tower'], 'race': 'beetles',
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': [mat.name]},
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
    'license': 'Generated with Tripo (image to 3D) from an ImageGen (gpt-image-2) concept for Under the '
               'Canopy; animated by tools/buildings/arc_tower.py.',
}
kit.export(OUT, NAME, asset)
blend1 = os.path.join(OUT, 'source.blend1')
if os.path.exists(blend1):
    os.remove(blend1)
