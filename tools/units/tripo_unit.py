"""A rigged, animated Tripo character made game-ready in Blender:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/units/tripo_unit.py -- <spec.json>

Tripo Studio makes the unit from its A-pose reference (image to 3D, textured, about 10k
faces), rigs it (humanoid, Mixamo skeleton) and retargets preset clips onto it; the export is
one GLB with the skeleton and every clip. Here:
- the unit is scaled to `height` metres, its feet on the ground, facing -Y (glTF +Z); only the
  meshes skinned to the rig are kept and measured (the cook refuses a model that floats);
- the team colour becomes a mask in the base colour's alpha: the concepts' royal-blue plates;
- Tripo's clips are renamed to the game's animation semantics (idle, walk, run, attack, hit,
  death, cast, ...) and the attack clip gets its contact event;
- the asset is written with tools/buildings/kit.py's exporter.

spec.json: {"glb": path, "name": "beetles-bolter", "title": "Bolter", "height": 2.0,
  "clips": {"idle": "idle", "walk": "walk", "attack": "shoot", ...}  (semantic -> Tripo clip),
  "attackEvent": {"name": "hit" | "release", "share": 0.55}, "healthHeight": 2.3,
  "tags": [...], "loops": ["idle", "walk", "run", ...]}"""
import json, os, sys

import bpy
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
SPEC = json.load(open(sys.argv[sys.argv.index('--') + 1]))
NAME = SPEC['name']
OUT = os.path.join(ROOT, 'base', 'assets', 'units', NAME)
LOOPS = set(SPEC.get('loops', ['idle', 'walk', 'run', 'carry', 'carry_walk', 'carry_run', 'build', 'chop']))

kit.reset()
bpy.ops.import_scene.gltf(filepath=SPEC['glb'])
scene = bpy.context.scene
rigs = [o for o in scene.objects if o.type == 'ARMATURE']
assert rigs, 'the GLB has no skeleton'
rig = rigs[0]
# Tripo's exports carry a stray icosphere at the origin: anything not skinned to the rig goes,
# so only the body is measured (it once lifted every unit by half its height).
for o in [o for o in scene.objects if o.type == 'MESH' and o.parent is not rig]:
    print('removing stray', o.name)
    bpy.data.objects.remove(o, do_unlink=True)
meshes = [o for o in scene.objects if o.type == 'MESH']

# --- Scale and ground: measure the rest pose, scale the armature root.
scene.frame_set(0)
rig.animation_data_create()
rest_action = rig.animation_data.action
rig.animation_data.action = None
bpy.context.view_layer.update()
pts = [o.matrix_world @ v.co for o in meshes for v in o.data.vertices]
lo = mathutils.Vector([min(p[i] for p in pts) for i in range(3)])
hi = mathutils.Vector([max(p[i] for p in pts) for i in range(3)])
s = SPEC['height'] / (hi.z - lo.z)
top = [o for o in scene.objects if o.parent is None]
for o in top:
    o.location = (o.location - mathutils.Vector(((lo.x + hi.x) / 2, (lo.y + hi.y) / 2, lo.z))) * s
    o.scale = o.scale * s
bpy.context.view_layer.update()
print('height', round((hi.z - lo.z) * s, 2), 'scale', round(s, 4))


# --- Team colour: the royal-blue plates into each material's base-colour alpha.
def base_texture(mat):
    bsdf = next((n for n in mat.node_tree.nodes if n.type == 'BSDF_PRINCIPLED'), None)
    link = bsdf.inputs['Base Color'].links if bsdf else []
    node = link[0].from_node if link else None
    while node is not None and node.type != 'TEX_IMAGE':
        node = node.inputs[0].links[0].from_node if node.inputs and node.inputs[0].links else None
    return node


slots = []
for mat in {m for o in meshes for m in o.data.materials if m and m.use_nodes}:
    node = base_texture(mat)
    if node is None:
        continue
    w, h = node.image.size
    px = np.array(node.image.pixels[:], np.float32).reshape(h, w, 4)
    rgb = px[:, :, :3]
    blue = rgb[:, :, 2] - np.maximum(rgb[:, :, 0], rgb[:, :, 1])
    mask = np.clip((blue - 0.04) / 0.1, 0.0, 1.0) * (rgb[:, :, 2] > 0.08)
    if mask.mean() < 0.002:
        continue
    img = bpy.data.images.new(mat.name + '-team', w, h, alpha=True)
    img.pixels[:] = np.concatenate([rgb, mask[..., None]], axis=2).ravel()
    img.alpha_mode = 'CHANNEL_PACKED'
    img.pack()
    node.image = img
    mat.name = 'TC_' + NAME
    mat['teamColorMask'] = 'baseColorAlpha'
    slots.append(mat.name)
    print('team mask', mat.name, round(float(mask.mean()), 3))

# --- Clips: rename Tripo's actions to the semantics, drop the rest.
actions = list(bpy.data.actions)
print('clips in file', [a.name for a in actions])
keep = {}
sources = {}
for semantic, source in SPEC['clips'].items():
    match = [a for a in actions if a.name == source] or [a for a in actions if source.lower() in a.name.lower()]
    if not match:
        print('MISSING clip', semantic, source)
        continue
    sources[semantic] = match[0]
used = set()
for semantic, src in sources.items():
    act = src.copy() if src.name in used or src in keep.values() else src
    used.add(src.name)
    keep[semantic] = act
for semantic, act in keep.items():
    act.name = semantic
    act.use_fake_user = True
for a in actions:
    if a not in keep.values():
        bpy.data.actions.remove(a)
rig.animation_data.action = keep.get('idle')
for t in list(rig.animation_data.nla_tracks):
    rig.animation_data.nla_tracks.remove(t)

def hand_bone():
    names = [b.name for b in rig.pose.bones]
    for key in (SPEC.get('hand', 'RightHand'), 'RightHand', 'righthand', 'Hand_R', 'hand_r'):
        for n in names:
            if key.lower() in n.lower():
                return n
    return None


def trim_to_first_strike(act):
    """Tripo's attack presets are combos several seconds long. Keep the first swing: the hand
    rises to its highest point, then the strike is the lowest point after it (or, for a shot,
    the fastest moment). Returns the strike frame relative to the clip start."""
    bone = hand_bone()
    start, end = (int(f) for f in act.frame_range)
    rig.animation_data.action = act
    z = []
    for f in range(start, end + 1):
        scene.frame_set(f)
        z.append((rig.matrix_world @ rig.pose.bones[bone].head).z if bone else 0.0)
    z = np.array(z)
    n = len(z)
    window = max(8, int(n * 0.6))
    peak = int(np.argmax(z[:window]))
    after = z[peak:]
    strike = peak + int(np.argmin(after[: max(4, int(kit.FPS * 1.2))]))
    cut = min(n - 1, strike + int(kit.FPS * 0.35))
    lead = max(0, peak - int(kit.FPS * 0.45))
    if SPEC.get('trimAttack', True):
        for fc in kit._fcurves(act):
            for i in reversed(range(len(fc.keyframe_points))):
                x = fc.keyframe_points[i].co.x
                if x > start + cut or x < start + lead:
                    fc.keyframe_points.remove(fc.keyframe_points[i])
            for k in fc.keyframe_points:
                k.co.x -= lead
                k.handle_left.x -= lead
                k.handle_right.x -= lead
        act.use_frame_range = True
        act.frame_start, act.frame_end = start, start + cut - lead
    print('attack trimmed', n, '->', cut - lead + 1, 'frames, strike at', strike - lead)
    return strike - lead


def crop(act, a, b):
    """Keep frames a..b of an action (relative to its start), shifted to start at 0."""
    start = int(act.frame_range[0])
    for fc in kit._fcurves(act):
        for i in reversed(range(len(fc.keyframe_points))):
            x = fc.keyframe_points[i].co.x - start
            if x < a or x > b:
                fc.keyframe_points.remove(fc.keyframe_points[i])
        for k in fc.keyframe_points:
            k.co.x -= a
            k.handle_left.x -= a
            k.handle_right.x -= a
    act.use_frame_range = True
    act.frame_start, act.frame_end = start, start + (b - a)


# Explicit trims: {"attack": [first, last, event frame], "cast": [...]} in source frames.
event_frames = {}
for semantic, (a, b, at) in SPEC.get('trim', {}).items():
    if semantic in keep:
        crop(keep[semantic], a, b)
        event_frames[semantic] = at - a
        print('cropped', semantic, a, b, 'event at', at - a)

animations = []
event = SPEC.get('attackEvent', {'name': 'hit', 'share': 0.55})
strike = trim_to_first_strike(keep['attack']) if 'attack' in keep and SPEC.get('trimAttack', True) else event_frames.get('attack')
rig.animation_data.action = keep.get('idle')
for semantic, act in keep.items():
    start, end = act.frame_range
    seconds = (end - start) / kit.FPS
    events = []
    if semantic == 'attack':
        at = strike / kit.FPS if strike is not None else seconds * event['share']
        events = [{'name': event['name'], 'time': round(at, 3)}]
    if semantic == 'cast':
        at = event_frames.get('cast', (end - start) * 0.5)
        events = [{'name': 'cast', 'time': round(at / kit.FPS, 3)}]
    animations.append({'semantic': semantic, 'clip': semantic, 'loop': semantic in LOOPS, 'events': events})

faces = sum(len(o.data.polygons) for o in meshes)
print('faces', faces)
asset = {
    'id': 'asset.models.units.' + NAME, 'name': SPEC['title'], 'kind': 'unit',
    'tags': SPEC.get('tags', []) + ['tripo', 'skinned'], 'race': SPEC.get('race', 'beetles'),
    'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                  {'role': 'source', 'index': 1, 'format': 'blend'}],
    'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
    'capabilities': {
        'teamColor': {'mode': 'mask', 'slots': slots},
        'animations': animations,
        'groundContact': {'mode': 'pivot'},
    },
    'display': {'scale': SPEC.get('displayScale', 2), 'character': 'base', 'healthHeight': SPEC.get('healthHeight', 2.3)},
    'license': 'Generated with Tripo (image to 3D, auto-rig, preset animations) from an ImageGen (gpt-image-2) '
               'concept for Under the Canopy; prepared by tools/units/tripo_unit.py.',
}
kit.export(OUT, NAME, asset)
print('wrote', OUT, [a['semantic'] for a in animations])
