"""Shared kit of the building scripts (tools/buildings/<building>.py), run in Blender:
clean, model-ready buildings built from simple solids (art direction: few big readable
forms), with one material per surface so team colour is a whole material, separate
objects for every moving part, socket empties for effects, and actions exported as the
model's clips (D65: `idle` is the rest pose, `work` while it works, `spawn` once).

Blender is Z-up in metres; the exporter turns it Y-up. The building faces -Y (glTF +Z,
the asset transform's forward), its pivot is the centre of its footprint on the ground."""
import bpy, bmesh, json, math, os
import mathutils
import numpy as np

FPS = 30


def reset():
    bpy.ops.wm.read_factory_settings(use_empty=True)
    bpy.context.scene.render.fps = FPS


# --- Materials -------------------------------------------------------------------------

def _image(path, name, colorspace='sRGB'):
    img = bpy.data.images.load(path)
    img.name = name
    img.colorspace_settings.name = colorspace
    img.pack()
    return img


def _array_image(name, rgb, colorspace='sRGB'):
    h, w = rgb.shape[:2]
    img = bpy.data.images.new(name, w, h, alpha=False)
    img.colorspace_settings.name = colorspace
    img.pixels[:] = np.concatenate([rgb, np.ones((h, w, 1), np.float32)], axis=2).ravel()
    img.pack()
    return img


def normal_from(img, strength=3.0):
    """A tangent-space normal map from an image's luminance (bevels and seams)."""
    w, h = img.size
    px = np.array(img.pixels[:], dtype=np.float32).reshape(h, w, 4)
    lum = px[:, :, :3] @ np.array([0.2126, 0.7152, 0.0722], dtype=np.float32)
    dx = (np.roll(lum, -1, axis=1) - np.roll(lum, 1, axis=1)) * strength
    dy = (np.roll(lum, -1, axis=0) - np.roll(lum, 1, axis=0)) * strength
    n = np.stack([-dx, -dy, np.ones_like(lum)], axis=-1)
    n /= np.linalg.norm(n, axis=-1, keepdims=True)
    return _array_image(img.name + '-normal', n * 0.5 + 0.5, 'Non-Color')


def material(name, base=None, color=(0.5, 0.5, 0.5), metallic=0.0, roughness=0.6,
             normal=0.0, emissive=None, emission=0.0, extras=None, size=512):
    """A glTF-friendly principled material: an optional base texture (shrunk to `size`),
    a normal map derived from it, an optional emissive texture and strength."""
    mat = bpy.data.materials.new(name)
    mat.use_nodes = True
    nt = mat.node_tree
    bsdf = nt.nodes['Principled BSDF']
    bsdf.inputs['Metallic'].default_value = metallic
    bsdf.inputs['Roughness'].default_value = roughness
    bsdf.inputs['Base Color'].default_value = (*color, 1.0)
    if base is not None:
        img = _image(base, name + '-base') if isinstance(base, str) else base
        if img.size[0] > size:
            img.scale(size, size)
        tex = nt.nodes.new('ShaderNodeTexImage')
        tex.image = img
        nt.links.new(tex.outputs['Color'], bsdf.inputs['Base Color'])
        if normal > 0:
            nm = nt.nodes.new('ShaderNodeNormalMap')
            nt_img = nt.nodes.new('ShaderNodeTexImage')
            nt_img.image = normal_from(img, normal)
            nt.links.new(nt_img.outputs['Color'], nm.inputs['Color'])
            nt.links.new(nm.outputs['Normal'], bsdf.inputs['Normal'])
    if emissive is not None:
        tex = nt.nodes.new('ShaderNodeTexImage')
        tex.image = emissive
        nt.links.new(tex.outputs['Color'], bsdf.inputs['Emission Color'])
        bsdf.inputs['Emission Strength'].default_value = emission
    for k, v in (extras or {}).items():
        mat[k] = v
    return mat


def fire_texture(name, size=256, seed=1):
    """Flames streaming up (tiles along V, so `flow` scrolls them): orange to yellow."""
    rng = np.random.default_rng(seed)
    v = np.linspace(0, 1, size, endpoint=False)
    u = np.linspace(0, 1, size, endpoint=False)
    uu, vv = np.meshgrid(u, v)
    field = np.zeros((size, size), np.float32)
    for k in (2, 3, 5, 8):
        phase = rng.uniform(0, 2 * np.pi, 2)
        field += np.sin(2 * np.pi * (uu * k + np.sin(2 * np.pi * vv * k + phase[0]) * 0.15) + phase[1]) / k
        field += np.sin(2 * np.pi * (vv * k + uu * 0.5) + phase[0]) / (k * 1.5)
    field = (field - field.min()) / (field.max() - field.min())
    heat = np.clip(field ** 1.6 * 1.3, 0, 1)[..., None]
    rgb = np.concatenate([np.ones_like(heat), 0.35 + 0.55 * heat, 0.05 + 0.35 * heat ** 3], axis=2) * (0.35 + 0.65 * heat)
    return _array_image(name, rgb.astype(np.float32))


def magic_texture(name, color=(0.55, 1.0, 0.25), size=256, seed=2):
    """Swirling contained magic (tiles along V): bright veins over a softer glow."""
    rng = np.random.default_rng(seed)
    u = np.linspace(0, 1, size, endpoint=False)
    uu, vv = np.meshgrid(u, u)
    field = np.zeros((size, size), np.float32)
    for k in (1, 2, 3, 5):
        phase = rng.uniform(0, 2 * np.pi, 3)
        field += np.sin(2 * np.pi * (uu * k + vv * k * 0.5) + phase[0] + np.sin(2 * np.pi * vv * k + phase[1])) / k
    field = (field - field.min()) / (field.max() - field.min())
    veins = np.exp(-((field - 0.5) / 0.06) ** 2)
    glow = (0.45 + 0.55 * veins)[..., None]
    return _array_image(name, (glow * np.array(color, np.float32)).astype(np.float32))


# --- Geometry --------------------------------------------------------------------------

def finish(obj, mat, bevel=0.0, segments=2, smooth=True, uv=None):
    """Material, bevel (applied on export), smooth shading by angle, UVs."""
    obj.data.materials.clear()
    obj.data.materials.append(mat)
    if bevel > 0:
        mod = obj.modifiers.new('bevel', 'BEVEL')
        mod.width = bevel
        mod.segments = segments
        mod.limit_method = 'ANGLE'
    if smooth:
        for p in obj.data.polygons:
            p.use_smooth = True
        obj.data.set_sharp_from_angle(angle=math.radians(40))
    if uv is not None:
        cube_uv(obj, uv)
    return obj


def cube_uv(obj, size):
    """Box-projected UVs at one texture repeat per `size` metres (world scale)."""
    me = obj.data
    bm = bmesh.new()
    bm.from_mesh(me)
    layer = bm.loops.layers.uv.verify()
    m = obj.matrix_world
    for f in bm.faces:
        n = (m.to_3x3() @ f.normal).normalized()
        axis = max(range(3), key=lambda i: abs(n[i]))
        for loop in f.loops:
            p = m @ loop.vert.co
            if axis == 0:
                uv = (p.y, p.z)
            elif axis == 1:
                uv = (p.x, p.z)
            else:
                uv = (p.x, p.y)
            loop[layer].uv = (uv[0] / size, uv[1] / size)
    bm.to_mesh(me)
    bm.free()


def link(obj, parent=None):
    bpy.context.scene.collection.objects.link(obj)
    if parent is not None:
        obj.parent = parent
    return obj


def cylinder(name, r, h, at=(0, 0, 0), verts=32, r_top=None, cap=True):
    bm = bmesh.new()
    bmesh.ops.create_cone(bm, cap_ends=cap, cap_tris=False, segments=verts, radius1=r,
                          radius2=r if r_top is None else r_top, depth=h)
    bmesh.ops.translate(bm, verts=bm.verts, vec=(0, 0, h / 2))
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = bpy.data.objects.new(name, me)
    obj.location = at
    return link(obj)


def box(name, size, at=(0, 0, 0), rot=(0, 0, 0)):
    bm = bmesh.new()
    bmesh.ops.create_cube(bm, size=1.0)
    bmesh.ops.scale(bm, vec=size, verts=bm.verts)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = bpy.data.objects.new(name, me)
    obj.location = at
    obj.rotation_euler = rot
    return link(obj)


def torus(name, major, minor, at=(0, 0, 0), rot=(0, 0, 0), arc=360.0, segments=48, ring=10):
    """A torus or an arc of one (degrees, starting on +X), around Z before `rot`."""
    bm = bmesh.new()
    rings = []
    steps = max(4, int(segments * arc / 360.0))
    closed = arc >= 360.0
    for i in range(steps + (0 if closed else 1)):
        a = math.radians(arc) * i / steps
        centre = mathutils.Vector((math.cos(a) * major, math.sin(a) * major, 0))
        out = mathutils.Vector((math.cos(a), math.sin(a), 0))
        rings.append([bm.verts.new(centre + (out * math.cos(b) + mathutils.Vector((0, 0, 1)) * math.sin(b)) * minor)
                      for b in (2 * math.pi * j / ring for j in range(ring))])
    count = len(rings) if closed else len(rings) - 1
    for i in range(count):
        r0, r1 = rings[i], rings[(i + 1) % len(rings)]
        for j in range(ring):
            bm.faces.new([r0[j], r0[(j + 1) % ring], r1[(j + 1) % ring], r1[j]])
    if not closed:
        bm.faces.new(list(reversed(rings[0])))
        bm.faces.new(rings[-1])
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = bpy.data.objects.new(name, me)
    obj.location = at
    obj.rotation_euler = rot
    return link(obj)


def dome(name, r, at=(0, 0, 0), squash=1.0, segments=48, rings=16):
    """The upper half of a sphere, open at the bottom, UVs along meridians."""
    bm = bmesh.new()
    bm.loops.layers.uv.verify()
    bmesh.ops.create_uvsphere(bm, u_segments=segments, v_segments=rings * 2, radius=r, calc_uvs=True)
    bmesh.ops.delete(bm, geom=[v for v in bm.verts if v.co.z < -1e-4], context='VERTS')
    bmesh.ops.scale(bm, vec=(1, 1, squash), verts=bm.verts)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = bpy.data.objects.new(name, me)
    obj.location = at
    return link(obj)


def ring_wall(name, r_in, r_out, h, at=(0, 0, 0), start=0.0, arc=360.0, segments=64):
    """A wall on a ring (or an arc of it, degrees from +X), flat on top."""
    bm = bmesh.new()
    steps = max(2, int(segments * arc / 360.0))
    closed = arc >= 360.0
    cols = []
    for i in range(steps + (0 if closed else 1)):
        a = math.radians(start + arc * i / steps)
        c, s = math.cos(a), math.sin(a)
        cols.append([bm.verts.new((c * r, s * r, z)) for r, z in ((r_in, 0), (r_out, 0), (r_out, h), (r_in, h))])
    count = len(cols) if closed else len(cols) - 1
    for i in range(count):
        a0, a1 = cols[i], cols[(i + 1) % len(cols)]
        for j in range(4):
            bm.faces.new([a0[j], a1[j], a1[(j + 1) % 4], a0[(j + 1) % 4]])
    if not closed:
        bm.faces.new(list(reversed(cols[0])))
        bm.faces.new(cols[-1])
    bmesh.ops.recalc_face_normals(bm, faces=bm.faces)
    me = bpy.data.meshes.new(name)
    bm.to_mesh(me)
    bm.free()
    obj = bpy.data.objects.new(name, me)
    obj.location = at
    return link(obj)


def carve(obj, cutter, operation='DIFFERENCE'):
    """Applies a boolean with `cutter` to `obj` now and removes the cutter."""
    mod = obj.modifiers.new('bool', 'BOOLEAN')
    mod.operation = operation
    mod.solver = 'EXACT'
    mod.object = cutter
    bpy.context.view_layer.objects.active = obj
    bpy.ops.object.modifier_apply(modifier=mod.name)
    bpy.data.objects.remove(cutter, do_unlink=True)
    return obj


def scale_uv(obj, su, sv):
    for loop_uv in obj.data.uv_layers.active.data:
        loop_uv.uv = (loop_uv.uv[0] * su, loop_uv.uv[1] * sv)


def socket(name, at):
    e = bpy.data.objects.new(name, None)
    e.empty_display_size = 0.3
    e.location = at
    return link(e)


# --- Clips and export ------------------------------------------------------------------

def clip(obj, name, keys, path='rotation_euler', index=1):
    """An action `name` on `obj` from (frame, value) keys on one channel, kept for export."""
    obj.rotation_mode = 'XYZ' if path == 'rotation_euler' else obj.rotation_mode
    obj.animation_data_create()
    action = bpy.data.actions.new(name)
    obj.animation_data.action = action
    for frame, value in keys:
        getattr(obj, path)[index] = value
        obj.keyframe_insert(data_path=path, index=index, frame=frame)
    for fc in _fcurves(action):
        for kp in fc.keyframe_points:
            kp.interpolation = 'BEZIER'
    track = obj.animation_data.nla_tracks.new()
    track.name = name
    track.strips.new(name, int(keys[0][0]), action)
    obj.animation_data.action = None
    return action


def _fcurves(action):
    if hasattr(action, 'fcurves') and action.fcurves is not None:
        try:
            return list(action.fcurves)
        except TypeError:
            pass
    out = []
    for layer in getattr(action, 'layers', []):
        for strip in layer.strips:
            for bag in strip.channelbags:
                out.extend(bag.fcurves)
    return out


def export(folder, name, asset):
    os.makedirs(folder, exist_ok=True)
    bpy.ops.wm.save_as_mainfile(filepath=os.path.join(folder, 'source.blend'))
    bpy.ops.export_scene.gltf(
        filepath=os.path.join(folder, 'geometry.glb'), export_format='GLB', export_yup=True,
        export_apply=True, export_image_format='AUTO', export_extras=True,
        export_animations=True, export_animation_mode='ACTIONS', export_force_sampling=True)
    with open(os.path.join(folder, 'asset.json'), 'w') as f:
        json.dump(asset, f, indent=2, ensure_ascii=False)
        f.write('\n')
    tris = sum(sum(len(p.vertices) - 2 for p in o.data.polygons) for o in bpy.data.objects if o.type == 'MESH')
    print('built', name, tris, 'triangles (before bevels)')
