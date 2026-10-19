"""Makes a skinned unit's clips play in place, editing its GLB's animation data directly:

    python3 tools/units/glb_in_place.py base/assets/units/<name>/geometry.glb [--pin attack,cast]

For models whose GLB came from elsewhere and must not be re-exported (a re-export can change
their textures and size). Every clip but `death` loses the skeleton root's horizontal root
motion: its translation keys, taken to the model's ground plane (glTF x and z, through the
root's parents), lose the straight line from the first key to the last and start at the rest
position, so the body stays over the unit's circle and keeps its sway; the vertical bob stays.
Locomotion clips (walk, run, carry_walk, carry_run) and `--pin` clips lose their horizontal
root motion entirely: some exports carry it as a sawtooth no straight line takes out, and a
running body's sway is a few centimetres nobody sees from above. The file is rewritten byte for byte
except those keys. tools/units/tripo_in_place.py does the same in a Blender source file; the
cook refuses clips whose root still travels."""
import json, struct, sys


def quat_matrix(q):
    x, y, z, w = q
    return [[1 - 2 * (y * y + z * z), 2 * (x * y - z * w), 2 * (x * z + y * w)],
            [2 * (x * y + z * w), 1 - 2 * (x * x + z * z), 2 * (y * z - x * w)],
            [2 * (x * z - y * w), 2 * (y * z + x * w), 1 - 2 * (x * x + y * y)]]


def mat_mul(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def node_linear(node):
    """The 3x3 part (rotation and scale) of a node's local transform."""
    if 'matrix' in node:
        m = node['matrix']
        return [[m[0], m[4], m[8]], [m[1], m[5], m[9]], [m[2], m[6], m[10]]]
    r = quat_matrix(node.get('rotation', [0, 0, 0, 1]))
    s = node.get('scale', [1, 1, 1])
    return [[r[i][j] * s[j] for j in range(3)] for i in range(3)]


def invert(m):
    a, b, c = m[0]
    d, e, f = m[1]
    g, h, i = m[2]
    det = a * (e * i - f * h) - b * (d * i - f * g) + c * (d * h - e * g)
    return [[(e * i - f * h) / det, (c * h - b * i) / det, (b * f - c * e) / det],
            [(f * g - d * i) / det, (a * i - c * g) / det, (c * d - a * f) / det],
            [(d * h - e * g) / det, (b * g - a * h) / det, (a * e - b * d) / det]]


def apply(m, v):
    return [sum(m[i][k] * v[k] for k in range(3)) for i in range(3)]


def main():
    path = sys.argv[1]
    pinned = ['walk', 'run', 'carry_walk', 'carry_run']
    if '--pin' in sys.argv:
        pinned += sys.argv[sys.argv.index('--pin') + 1].split(',')
    data = bytearray(open(path, 'rb').read())
    json_len = struct.unpack('<I', data[12:16])[0]
    gltf = json.loads(data[20:20 + json_len])
    bin_start = 20 + json_len + 8  # the BIN chunk's header follows the JSON chunk
    nodes = gltf['nodes']
    parent = {c: i for i, n in enumerate(nodes) for c in n.get('children', [])}
    joints = {j for skin in gltf.get('skins', []) for j in skin['joints']}
    roots = [j for j in joints if parent.get(j) not in joints]

    def floats(accessor_index):
        acc = gltf['accessors'][accessor_index]
        view = gltf['bufferViews'][acc['bufferView']]
        offset = bin_start + view.get('byteOffset', 0) + acc.get('byteOffset', 0)
        width = {'SCALAR': 1, 'VEC3': 3, 'VEC4': 4}[acc['type']]
        stride = view.get('byteStride', 4 * width)
        return offset, acc['count'], width, stride

    for root in roots:
        # The root's parent space to the model's space: x and z there are the ground plane.
        to_model = [[1, 0, 0], [0, 1, 0], [0, 0, 1]]
        p = parent.get(root)
        while p is not None:
            to_model = mat_mul(node_linear(nodes[p]), to_model)
            p = parent.get(p)
        to_parent = invert(to_model)
        rest = apply(to_model, nodes[root].get('translation', [0, 0, 0]))
        for animation in gltf.get('animations', []):
            if animation.get('name') == 'death':
                continue
            for channel in animation['channels']:
                if channel['target'].get('node') != root or channel['target']['path'] != 'translation':
                    continue
                sampler = animation['samplers'][channel['sampler']]
                if sampler.get('interpolation', 'LINEAR') == 'CUBICSPLINE':
                    sys.exit('%s: cubic-spline root keys are not handled' % animation.get('name'))
                t_off, count, _, t_stride = floats(sampler['input'])
                v_off, _, _, v_stride = floats(sampler['output'])
                times = [struct.unpack_from('<f', data, t_off + k * t_stride)[0] for k in range(count)]
                keys = [apply(to_model, struct.unpack_from('<3f', data, v_off + k * v_stride)) for k in range(count)]
                first, last = keys[0], keys[-1]
                span = (times[-1] - times[0]) or 1.0
                moved = 0.0
                for k, (t, v) in enumerate(zip(times, keys)):
                    share = (t - times[0]) / span
                    for axis in (0, 2):
                        trend = first[axis] + (last[axis] - first[axis]) * share
                        new = rest[axis] if animation.get('name') in pinned else rest[axis] + v[axis] - trend
                        moved = max(moved, abs(new - v[axis]))
                        v[axis] = new
                    struct.pack_into('<3f', data, v_off + k * v_stride, *apply(to_parent, v))
                print(animation.get('name'), nodes[root].get('name'), 'moved up to', round(moved, 3))
    open(path, 'wb').write(data)


if __name__ == '__main__':
    main()
