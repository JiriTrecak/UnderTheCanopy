"""The Glass Road's set dressing for the Underleaf biome (beetle territory: engineered roads,
glass tanks of substance, lanterns), procedural in Blender, headless:

    /Applications/Blender.app/Contents/MacOS/Blender -b -P tools/props/beetle_road.py

Built after the concept `art/styles/target-underleaf-road.jpg`: big readable forms in the
beetles' materials (riveted dark steel, brass, cut blue-grey stone) with their contained
magic glowing blue-violet inside glass (never green). These are neutral decoration, so
nothing is royal blue: blue on the beetles' own buildings is the team colour.

- **Road tiles** 8 x 8 m (two blocks: they snap to the block lattice): fitted flagstones
  (`underleaf-paving`) on a low slab with bevelled edges and brass edging strips; straight,
  corner, T-junction, crossing and end pieces, so a road is laid tile by tile in the editor.
- **Lantern**: a stone foot, a steel post and a brass cage round violet glass; a small
  violet light (scenery `light`).
- **Glass tank**: a steel-and-brass column of glass holding glowing substance, on a cut
  stone plinth, with a pipe down to the ground.
- **Pipe run**: a brass pipe on stone posts, 8 m, to run beside a road.

Every piece: pivot at the centre of its footprint on the ground, forward -Y (glTF +Z).
Writes base/assets/environment/underleaf/<piece>/ (geometry.glb, source.blend, asset.json)."""
import math, os, sys

import bpy, bmesh
import mathutils
import numpy as np

sys.path.insert(0, os.path.join(os.path.dirname(os.path.dirname(os.path.abspath(__file__))), 'buildings'))
import kit  # noqa: E402

ROOT = os.path.dirname(os.path.dirname(os.path.dirname(os.path.abspath(__file__))))
OUT = os.path.join(ROOT, 'base', 'assets', 'environment', 'underleaf')
PAVING = os.path.join(ROOT, 'base', 'assets', 'terrain', 'underleaf-paving', 'source.png')
MASONRY = os.path.join(ROOT, 'base', 'assets', 'terrain', 'underleaf-masonry', 'source.png')
VIOLET = np.array((0.55, 0.32, 1.0), np.float32)
LILAC = np.array((0.86, 0.78, 1.0), np.float32)
LICENSE = ('Original procedural model for Under the Canopy (tools/props/beetle_road.py), '
           'textures by gpt-image-2.')


def materials():
    m = {}
    m['brass'] = kit.material('brass', color=(0.62, 0.43, 0.2), metallic=0.9, roughness=0.38)
    m['steel'] = kit.material('steel', color=(0.16, 0.165, 0.18), metallic=0.7, roughness=0.55)
    m['stone'] = kit.material('stone', base=MASONRY, roughness=0.8, normal=2.0, size=512)
    m['paving'] = kit.material('paving', base=PAVING, roughness=0.85, normal=2.5, size=512)
    glow = kit.magic_texture('substance', color=tuple(VIOLET), seed=5)
    m['liquid'] = kit.material('liquid', color=(0.1, 0.05, 0.25), roughness=0.1,
                               emissive=glow, emission=3.2)
    lamp = kit._array_image('lamp', np.ones((8, 8, 3), np.float32) * (0.6 * VIOLET + 0.4 * LILAC))
    m['lamp'] = kit.material('lamp', color=(0.5, 0.4, 0.9), roughness=0.15, emissive=lamp, emission=4.0)
    glass = kit.material('glass', color=(0.75, 0.7, 0.95), roughness=0.05)
    bsdf = glass.node_tree.nodes['Principled BSDF']
    bsdf.inputs['Alpha'].default_value = 0.28
    glass.surface_render_method = 'BLENDED'
    m['glass'] = glass
    return m


def join(objs, name):
    # Textured materials need UVs on every mesh (the exporter writes -1 indices otherwise).
    for o in objs:
        if not o.data.uv_layers:
            kit.cube_uv(o, 1.0)
    bpy.ops.object.select_all(action='DESELECT')
    for o in objs:
        o.select_set(True)
    bpy.context.view_layer.objects.active = objs[0]
    bpy.ops.object.convert(target='MESH')  # applies bevel modifiers
    bpy.ops.object.join()
    o = bpy.context.view_layer.objects.active
    o.name = name
    return o


def asset(name, title, kind='prop', light=None, tags=()):
    a = {
        'id': 'asset.models.environment.' + name,
        'name': title,
        'kind': kind,
        'tags': ['underleaf', 'glass-road', 'beetles', 'procedural', *tags],
        'resources': [{'role': 'geometry', 'index': 1, 'format': 'glb'},
                      {'role': 'source', 'index': 1, 'format': 'blend'}],
        'transform': {'scale': 1, 'pivot': [0, 0, 0], 'up': 'Y', 'forward': '+Z'},
        'capabilities': {'vegetationClearance': 1, 'groundContact': {'mode': 'terrain'}},
        'license': LICENSE,
    }
    return a


# --- Road tiles --------------------------------------------------------------------------

TILE = 8.0
SLAB = 0.14


def slab(m, length=TILE):
    """The paved top of one tile, bevelled, UVs at one paving repeat per 4 m."""
    o = kit.box('slab', (TILE, length, SLAB), at=(0, 0, SLAB / 2))
    kit.finish(o, m['paving'], bevel=0.04, uv=4.0)
    return o


def edging(m, side, length=TILE, along='y', offset=0.0):
    """A brass edging strip on a stone kerb along one side (`side` ±1), with bolted plates."""
    parts = []
    w = 0.42
    pos = side * (TILE / 2 - w / 2)
    if along == 'y':
        kerb = kit.box('kerb', (w, length, 0.3), at=(pos, offset, 0.15))
        strip = kit.box('strip', (0.16, length, 0.08), at=(pos, offset, 0.34))
    else:
        kerb = kit.box('kerb', (length, w, 0.3), at=(offset, pos, 0.15))
        strip = kit.box('strip', (length, 0.16, 0.08), at=(offset, pos, 0.34))
    kit.finish(kerb, m['stone'], bevel=0.05, uv=2.0)
    kit.finish(strip, m['brass'], bevel=0.02)
    parts += [kerb, strip]
    for k in range(int(length // 2)):
        t = -length / 2 + 1.0 + 2.0 * k + offset
        at = (pos, t, 0.36) if along == 'y' else (t, pos, 0.36)
        bolt = kit.cylinder('bolt', 0.09, 0.08, at=at, verts=8)
        kit.finish(bolt, m['brass'], smooth=False)
        parts.append(bolt)
    return parts


def road(name, title, sides, length=TILE, rise=0.0):
    """`sides`: which edges carry a kerb: any of 'w', 'e', 'n', 's' (x−, x+, y+, y−).
    With `rise`, the tile climbs that many metres along +y over its `length`: a ramp
    piece for a block ramp of that run (one level, 3 m, over four blocks)."""
    kit.reset()
    m = materials()
    parts = [slab(m, length)]
    for s in sides:
        if s in 'we':
            parts += edging(m, -1 if s == 'w' else 1, length=length, along='y')
        else:
            parts += edging(m, 1 if s == 'n' else -1, along='x', offset=0.0)
    if rise:
        # Bevel and shear: every vertex climbs with y, the pivot at the ramp's middle.
        bpy.ops.object.select_all(action='DESELECT')
        for o in parts:
            o.select_set(True)
        bpy.context.view_layer.objects.active = parts[0]
        bpy.ops.object.convert(target='MESH')
        bpy.ops.object.transform_apply(location=True, rotation=True, scale=True)
        for o in parts:
            for v in o.data.vertices:
                v.co.z += v.co.y * rise / length
    stone = [p for p in parts if p.data.materials[0].name == m['stone'].name]
    brass = [p for p in parts if p.data.materials[0].name == m['brass'].name]
    join(parts[:1], name + '-paving')
    if stone:
        join(stone, name + '-kerb')
    if brass:
        join(brass, name + '-brass')
    a = asset(name, title, tags=('road',))
    a['capabilities']['vegetationClearance'] = 4
    kit.export(os.path.join(OUT, name), name, a)


# --- Lantern ------------------------------------------------------------------------------

def lantern():
    name = 'beetle-lantern'
    kit.reset()
    m = materials()
    stone = [kit.box('foot', (1.0, 1.0, 0.5), at=(0, 0, 0.25))]
    kit.finish(stone[0], m['stone'], bevel=0.06, uv=1.5)
    steel = [kit.cylinder('post', 0.17, 1.8, at=(0, 0, 0.5), verts=8, r_top=0.13),
             kit.cylinder('collar', 0.3, 0.16, at=(0, 0, 2.2), verts=8),
             kit.cylinder('cap', 0.46, 0.4, at=(0, 0, 3.12), verts=8, r_top=0.1)]
    brass = [kit.torus('ring-a', 0.42, 0.06, at=(0, 0, 2.36), segments=24, ring=6),
             kit.torus('ring-b', 0.42, 0.06, at=(0, 0, 3.1), segments=24, ring=6),
             kit.cylinder('finial', 0.08, 0.35, at=(0, 0, 3.5), verts=8, r_top=0.0)]
    for k in range(6):
        a = 2 * math.pi * k / 6
        brass.append(kit.cylinder(f'bar-{k}', 0.035, 0.76, at=(0.4 * math.cos(a), 0.4 * math.sin(a), 2.34), verts=6))
    glass = [kit.cylinder('glass', 0.34, 0.76, at=(0, 0, 2.34), verts=16)]
    for o in steel:
        kit.finish(o, m['steel'], bevel=0.02)
    for o in brass:
        kit.finish(o, m['brass'])
    kit.finish(glass[0], m['lamp'], uv=1.0)
    join(stone, name + '-stone')
    join(steel, name + '-steel')
    join(brass, name + '-brass')
    a = asset(name, 'Beetle lantern')
    kit.export(os.path.join(OUT, name), name, a)


# --- Glass tank ---------------------------------------------------------------------------

def tank(name='beetle-substance-tank', height=7.0, radius=1.5, title='Glass substance tank'):
    kit.reset()
    m = materials()
    stone = [kit.cylinder('plinth', radius + 0.75, 0.7, verts=8)]
    kit.finish(stone[0], m['stone'], bevel=0.08, uv=1.5)
    top = height - 1.2
    steel = [kit.cylinder('base', radius + 0.35, 0.55, at=(0, 0, 0.7), verts=16),
             kit.dome('lid', radius + 0.2, at=(0, 0, top + 0.1), squash=0.55, segments=24, rings=6),
             kit.cylinder('chimney', 0.22, 0.9, at=(0, 0, top + 0.6), verts=8)]
    brass = [kit.torus(f'band-{k}', radius + 0.08, 0.11, at=(0, 0, z), segments=32, ring=8)
             for k, z in enumerate((1.3, (1.3 + top) / 2, top))]
    brass.append(kit.torus('lid-rim', radius + 0.22, 0.09, at=(0, 0, top + 0.12), segments=32, ring=8))
    brass.append(kit.cylinder('finial', 0.12, 0.5, at=(0, 0, top + 1.5), verts=8, r_top=0.0))
    for k in range(6):
        a = 2 * math.pi * k / 6 + 0.3
        steel.append(kit.cylinder(f'strut-{k}', 0.1, top - 1.25, at=((radius + 0.05) * math.cos(a), (radius + 0.05) * math.sin(a), 1.25), verts=6))
    # A pipe from the lid down the side into the plinth.
    bend = 0.7
    brass.append(kit.torus('pipe-bend', bend, 0.16, at=(radius, 0, top - 0.6), rot=(math.pi / 2, 0, 0), arc=90, segments=12, ring=8))
    brass.append(kit.cylinder('pipe', 0.16, top - 0.6 - 0.7, at=(radius + bend, 0, 0.7), verts=8))
    brass.append(kit.cylinder('flange', 0.26, 0.12, at=(radius + bend, 0, 0.7), verts=8))
    liquid = [kit.cylinder('liquid', radius - 0.12, (top - 1.25) * 0.86, at=(0, 0, 1.25), verts=24)]
    glass = [kit.cylinder('glass', radius, top - 1.25, at=(0, 0, 1.25), verts=24, cap=False)]
    for o in steel:
        kit.finish(o, m['steel'], bevel=0.02)
    for o in brass:
        kit.finish(o, m['brass'])
    kit.finish(liquid[0], m['liquid'], uv=2.0)
    kit.finish(glass[0], m['glass'], uv=2.0)
    join(stone, name + '-stone')
    join(steel, name + '-steel')
    join(brass, name + '-brass')
    a = asset(name, title, tags=('substance',))
    a['capabilities']['vegetationClearance'] = 2
    kit.export(os.path.join(OUT, name), name, a)


# --- Pipe run -----------------------------------------------------------------------------

def pipes():
    name = 'beetle-pipe-run'
    kit.reset()
    m = materials()
    stone, brass = [], []
    for y in (-TILE / 2 + 0.6, 0.0, TILE / 2 - 0.6):
        p = kit.box('post', (0.7, 0.7, 0.75), at=(0, y, 0.375))
        kit.finish(p, m['stone'], bevel=0.05, uv=1.5)
        stone.append(p)
        f = kit.cylinder('clamp', 0.34, 0.16, at=(0, y - 0.08, 1.0), verts=12)
        f.rotation_euler = (math.pi / 2, 0, 0)
        brass.append(f)
    pipe = kit.cylinder('pipe', 0.24, TILE, at=(0, TILE / 2, 1.0), verts=12)
    pipe.rotation_euler = (math.pi / 2, 0, 0)
    brass.append(pipe)
    for o in brass:
        kit.finish(o, m['brass'])
    join(stone, name + '-stone')
    join(brass, name + '-brass')
    kit.export(os.path.join(OUT, name), name, asset(name, 'Beetle pipe run'))


# --- The condenser ------------------------------------------------------------------------

def condenser():
    """The Glass Road's landmark: a riveted steel tower on a stepped stone base, three glass
    columns of substance round it piped into its shoulders, a violet furnace eye on its
    front, a chimney. About 19 m across and 21 m tall; its front faces -Y."""
    name = 'beetle-condenser'
    kit.reset()
    m = materials()
    stone, steel, brass, liquid, glass, lamp = [], [], [], [], [], []
    stone.append(kit.cylinder('tier-a', 9.4, 1.0, verts=8))
    stone.append(kit.cylinder('tier-b', 7.8, 0.9, at=(0, 0, 1.0), verts=8))
    for k in range(3):  # steps up the front
        stone.append(kit.box(f'step-{k}', (4.0, 1.2, 0.6 * (k + 1)), at=(0, -9.4 - 0.6 + 1.2 * k + 0.6, 0.3 * (k + 1))))
    base = 1.9
    steel.append(kit.cylinder('tower', 4.4, 7.6, at=(0, 0, base), verts=16, r_top=3.8))
    steel.append(kit.cylinder('neck', 3.0, 4.2, at=(0, 0, base + 7.6), verts=16, r_top=2.5))
    steel.append(kit.dome('cap', 2.7, at=(0, 0, base + 11.8), squash=0.7, segments=24, rings=8))
    steel.append(kit.cylinder('chimney', 0.55, 4.5, at=(1.2, 1.0, base + 11.0), verts=10))
    brass.append(kit.torus('chimney-rim', 0.6, 0.12, at=(1.2, 1.0, base + 15.5), segments=16, ring=6))
    for z, r in ((base + 0.3, 4.45), (base + 3.8, 4.15), (base + 7.5, 3.85), (base + 11.7, 2.55)):
        brass.append(kit.torus(f'band-{z:.1f}', r, 0.18, at=(0, 0, z), segments=40, ring=8))
    brass.append(kit.cylinder('finial', 0.25, 2.2, at=(0, 0, base + 13.4), verts=8, r_top=0.0))
    # Riveted ribs up the tower.
    for k in range(8):
        a = 2 * math.pi * k / 8 + math.pi / 8
        rib = kit.box(f'rib-{k}', (0.35, 0.3, 7.4), at=(4.15 * math.cos(a), 4.15 * math.sin(a), base + 3.8), rot=(0, 0, a))
        rib.rotation_euler = (0, 0, a)
        steel.append(rib)
    # The furnace eye on the front.
    eye = kit.cylinder('eye', 1.35, 0.5, at=(0, -4.1, base + 4.2), verts=24)
    eye.rotation_euler = (math.pi / 2, 0, 0)
    lamp.append(eye)
    rim = kit.torus('eye-rim', 1.45, 0.2, at=(0, -4.25, base + 4.2), rot=(math.pi / 2, 0, 0), segments=32, ring=8)
    brass.append(rim)
    # Three glass columns of substance, piped into the shoulders.
    for k, a in enumerate((math.radians(0), math.radians(90), math.radians(180))):
        cx, cy = 6.0 * math.cos(a), 6.0 * math.sin(a)
        h0, h1 = base, base + 8.5
        steel.append(kit.cylinder(f'col-foot-{k}', 1.9, 0.6, at=(cx, cy, h0 - 0.1), verts=12))
        glass.append(kit.cylinder(f'col-glass-{k}', 1.55, h1 - h0 - 0.5, at=(cx, cy, h0 + 0.5), verts=20, cap=False))
        liquid.append(kit.cylinder(f'col-liquid-{k}', 1.4, (h1 - h0 - 0.5) * 0.82, at=(cx, cy, h0 + 0.5), verts=16))
        steel.append(kit.dome(f'col-lid-{k}', 1.75, at=(cx, cy, h1), squash=0.6, segments=20, rings=6))
        for z in (h0 + 0.55, (h0 + h1) / 2, h1 - 0.05):
            brass.append(kit.torus(f'col-band-{k}-{z:.1f}', 1.62, 0.12, at=(cx, cy, z), segments=24, ring=6))
        for j in range(4):
            b = 2 * math.pi * j / 4 + a
            steel.append(kit.cylinder(f'col-strut-{k}-{j}', 0.1, h1 - h0 - 0.5,
                                      at=(cx + 1.6 * math.cos(b), cy + 1.6 * math.sin(b), h0 + 0.5), verts=6))
        # A pipe from the lid arching into the tower's shoulder.
        d = mathutils.Vector((-cx, -cy, 0)).normalized()
        start = mathutils.Vector((cx, cy, h1 + 0.9))
        end = mathutils.Vector((0, 0, base + 9.5)) - d * 2.7
        mid = (start + end) / 2 + mathutils.Vector((0, 0, 1.6))
        pts = [start, mid, end]
        for i in range(2):
            p0, p1 = pts[i], pts[i + 1]
            seg = kit.cylinder(f'pipe-{k}-{i}', 0.32, (p1 - p0).length, verts=10)
            seg.rotation_mode = 'QUATERNION'
            seg.rotation_quaternion = mathutils.Vector((0, 0, 1)).rotation_difference(p1 - p0)
            seg.location = p0
            brass.append(seg)
        brass.append(kit.torus(f'pipe-joint-{k}', 0.42, 0.14, at=tuple(mid), segments=12, ring=6))
    for o in stone:
        kit.finish(o, m['stone'], bevel=0.08, uv=2.0)
    for o in steel:
        kit.finish(o, m['steel'], bevel=0.03)
    for o in brass:
        kit.finish(o, m['brass'])
    for o in liquid:
        kit.finish(o, m['liquid'], uv=2.0)
    for o in glass:
        kit.finish(o, m['glass'], uv=2.0)
    for o in lamp:
        kit.finish(o, m['lamp'], uv=1.0)
    join(stone, name + '-stone')
    join(steel, name + '-steel')
    join(brass, name + '-brass')
    join(liquid, name + '-liquid')
    join(glass, name + '-glass')
    join(lamp, name + '-eye')
    a = asset(name, 'Beetle condenser', tags=('substance', 'landmark'))
    a['capabilities']['vegetationClearance'] = 6
    kit.export(os.path.join(OUT, name), name, a)


# --- Station walls ------------------------------------------------------------------------

def wall(name='beetle-wall', title='Glass Road: station wall', length=TILE, height=4.2):
    """A straight stretch of the beetle station's wall: cut masonry on a stepped footing, a
    riveted steel cope along its top and a brass band, buttresses at its ends; 8 m long
    (two blocks) along x, about 1.6 m thick, so stretches join end to end on the lattice."""
    kit.reset()
    m = materials()
    stone = [kit.box('footing', (length, 2.0, 0.6), at=(0, 0, 0.3)),
             kit.box('body', (length, 1.4, height - 0.6), at=(0, 0, 0.6 + (height - 0.6) / 2))]
    for x in (-length / 2 + 0.6, length / 2 - 0.6):
        stone.append(kit.box('buttress', (1.2, 2.4, height * 0.8), at=(x, 0, height * 0.4)))
    steel = [kit.box('cope', (length, 1.7, 0.35), at=(0, 0, height + 0.15))]
    for k in range(round(length / 1.6)):
        x = -length / 2 + 0.8 + 1.6 * k
        steel.append(kit.box(f'merlon-{k}', (0.9, 1.5, 0.6), at=(x, 0, height + 0.6)))
    brass = [kit.box('band', (length + 0.02, 1.46, 0.22), at=(0, 0, height * 0.62))]
    for k in range(int(length // 1.0)):
        x = -length / 2 + 0.5 + 1.0 * k
        for y in (-0.74, 0.74):
            brass.append(kit.cylinder(f'rivet-{k}-{y}', 0.07, 0.06, at=(x, y, height * 0.62), verts=8))
            brass[-1].rotation_euler = (math.pi / 2, 0, 0)
    for o in stone:
        kit.finish(o, m['stone'], bevel=0.06, uv=2.0)
    for o in steel:
        kit.finish(o, m['steel'], bevel=0.04)
    for o in brass:
        kit.finish(o, m['brass'])
    join(stone, name + '-stone')
    join(steel, name + '-steel')
    join(brass, name + '-brass')
    kit.export(os.path.join(OUT, name), name, asset(name, title, tags=('wall',)))


def gate_tower(name='beetle-wall-tower', title='Glass Road: wall tower'):
    """A square tower to end a wall or flank a gate: masonry, a steel crown, a lantern of
    violet glass under a brass cap. 4 x 4 m (one block), 7 m tall."""
    kit.reset()
    m = materials()
    stone = [kit.box('base', (4.0, 4.0, 0.8), at=(0, 0, 0.4)), kit.box('shaft', (3.4, 3.4, 5.4), at=(0, 0, 3.5))]
    steel = [kit.box('crown', (4.0, 4.0, 0.5), at=(0, 0, 6.4)), kit.cylinder('roof', 2.0, 1.4, at=(0, 0, 6.65), verts=4, r_top=0.2)]
    steel[-1].rotation_euler = (0, 0, math.pi / 4)
    lamp = [kit.box('window', (1.0, 0.12, 1.4), at=(0, -1.72, 4.6))]
    brass = [kit.box('frame', (1.3, 0.16, 1.7), at=(0, -1.7, 4.6)), kit.cylinder('finial', 0.15, 1.2, at=(0, 0, 8.0), verts=8, r_top=0.0)]
    for o in stone:
        kit.finish(o, m['stone'], bevel=0.06, uv=2.0)
    for o in steel:
        kit.finish(o, m['steel'], bevel=0.04)
    for o in brass:
        kit.finish(o, m['brass'])
    kit.finish(lamp[0], m['lamp'], uv=1.0)
    join(stone, name + '-stone')
    join(steel, name + '-steel')
    join(brass, name + '-brass')
    join(lamp, name + '-lamp')
    kit.export(os.path.join(OUT, name), name, asset(name, title, tags=('wall',)))


if '--only' in sys.argv:
    condenser()
    sys.exit(0)
if '--walls' in sys.argv:
    wall()
    gate_tower()
    sys.exit(0)
road('beetle-road-straight', 'Glass Road: straight', 'we')
road('beetle-road-corner', 'Glass Road: corner', 'wn')
road('beetle-road-junction', 'Glass Road: junction', 'w')
road('beetle-road-crossing', 'Glass Road: crossing', '')
road('beetle-road-end', 'Glass Road: end', 'wen')
road('beetle-road-ramp', 'Glass Road: ramp', 'we', length=16.0, rise=3.0)
lantern()
tank()
tank('beetle-substance-tank-small', height=4.6, radius=0.95, title='Glass substance tank, small')
pipes()
condenser()
wall()
gate_tower()
