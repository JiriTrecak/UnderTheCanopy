// Terrain: vertex-pulled chunks with stitched levels of detail, and the layered ground
// (old `sourceTerrainShader`: base, tiled height-blended layers, atlas corner tiles,
// biplanar cliffs), at most four layers per pixel. Building foundations (D44): the pad
// and bank under each building, shaded as the ground they stand on.

const CHUNK: u32 = 32u;
const NO_LAYER: u32 = 255u;
const KIND_BASE: u32 = 0u;
const KIND_TILED: u32 = 1u;
const KIND_ATLAS: u32 = 2u;
const KIND_CLIFF: u32 = 3u;
const NONE: u32 = 0xffffu;

struct Layer {
    // kind, albedo, normal, occlusion (array layers; NONE when absent)
    kind: vec4u,
    // tiled: tiling, blend, edge, breakup. atlas: columns, rows, atlas slot, 0.
    // cliff: start slope, full slope, vertical size, 0
    params: vec4f,
    // rgb tint, a: painted
    tint: vec4f,
    corners: array<vec4u, 4>,
}

struct TerrainInfo {
    // origin, span, vertices, 1 / vertices
    grid: vec4f,
    // texture cells: start x, start z, cell size; w: layer-list cell offset
    cells: vec4f,
    // texture cells x, z; layer count; mask layers
    counts: vec4u,
    // painted ground: corner x, z, 1 / width, 1 / depth (zero width: none)
    overlay: vec4f,
    layers: array<Layer, 32>,
}

struct Chunk {
    // x | z << 16
    position: u32,
    // lod | −x << 8 | +x << 16 | −z << 24 … +z in `extra`
    levels: u32,
    extra: u32,
    pad: u32,
}

@group(1) @binding(0) var heights: texture_2d<f32>;
@group(1) @binding(1) var normals: texture_2d<f32>;
@group(1) @binding(2) var masks: texture_2d_array<f32>;
@group(1) @binding(3) var atlas_cells: texture_2d_array<u32>;
@group(1) @binding(4) var selection: texture_2d<u32>;
@group(1) @binding(5) var albedo_tiles: texture_2d_array<f32>;
@group(1) @binding(6) var normal_tiles: texture_2d_array<f32>;
@group(1) @binding(7) var occlusion_tiles: texture_2d_array<f32>;
@group(1) @binding(8) var clamp_sampler: sampler;
@group(1) @binding(9) var tile_sampler: sampler;
@group(1) @binding(10) var<uniform> terrain: TerrainInfo;
@group(1) @binding(11) var overlay_image: texture_2d<f32>;
@group(2) @binding(0) var<storage, read> chunks: array<Chunk>;

fn height_at(x: u32, z: u32) -> f32 {
    let last = u32(terrain.grid.y);
    return textureLoad(heights, vec2u(min(x, last), min(z, last)), 0).r;
}

struct VsOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
}

// Height along a chunk edge shared with a coarser neighbour: on the neighbour's straight
// edge between its vertices.
fn stitched(fixed: u32, along: u32, start: u32, coarse: u32, on_x: bool) -> f32 {
    let local = along - start;
    let rem = local % coarse;
    let a = start + local - rem;
    let b = a + coarse;
    let t = f32(rem) / f32(coarse);
    if (on_x) {
        return mix(height_at(a, fixed), height_at(b, fixed), t);
    }
    return mix(height_at(fixed, a), height_at(fixed, b), t);
}

fn chunk_vertex(vid: u32, inst: u32) -> vec3f {
    let c = chunks[inst];
    let cx = c.position & 0xffffu;
    let cz = c.position >> 16u;
    let lod = c.levels & 0xffu;
    let left = (c.levels >> 8u) & 0xffu;
    let right = (c.levels >> 16u) & 0xffu;
    let top = (c.levels >> 24u) & 0xffu;
    let bottom = c.extra & 0xffu;
    let step = 1u << lod;
    let n = CHUNK >> lod;
    let i = vid % (n + 1u);
    let j = vid / (n + 1u);
    let x0 = cx * CHUNK;
    let z0 = cz * CHUNK;
    let gx = x0 + i * step;
    let gz = z0 + j * step;
    var h = height_at(gx, gz);
    if (i == 0u && left > lod) {
        h = stitched(gx, gz, z0, 1u << left, false);
    } else if (i == n && right > lod) {
        h = stitched(gx, gz, z0, 1u << right, false);
    }
    if (j == 0u && top > lod) {
        h = stitched(gz, gx, x0, 1u << top, true);
    } else if (j == n && bottom > lod) {
        h = stitched(gz, gx, x0, 1u << bottom, true);
    }
    let origin = terrain.grid.x;
    return vec3f(origin + f32(gx), h, origin + f32(gz));
}

@vertex
fn vs_main(@builtin(vertex_index) vid: u32, @builtin(instance_index) inst: u32) -> VsOut {
    let world = chunk_vertex(vid, inst);
    var out: VsOut;
    out.world = world;
    out.clip = frame.view_proj * vec4f(world, 1.0);
    return out;
}

@vertex
fn vs_shadow(@builtin(vertex_index) vid: u32, @builtin(instance_index) inst: u32) -> @builtin(position) vec4f {
    return frame.view_proj * vec4f(chunk_vertex(vid, inst), 1.0);
}

// --- Noise (old `terrainNoise`) ------------------------------------------------------

fn hash2(p0: vec2f) -> f32 {
    var p = fract(p0 * vec2f(123.34, 456.21));
    p += dot(p, p + 45.32);
    return fract(p.x * p.y);
}

fn value_noise(p: vec2f) -> f32 {
    let i = floor(p);
    var f = fract(p);
    f = f * f * (3.0 - 2.0 * f);
    let a = hash2(i);
    let b = hash2(i + vec2f(1.0, 0.0));
    let c = hash2(i + vec2f(0.0, 1.0));
    let d = hash2(i + vec2f(1.0, 1.0));
    return mix(mix(a, b, f.x), mix(c, d, f.x), f.y);
}

fn noise2(p: vec2f) -> f32 {
    return value_noise(p) * 0.65 + value_noise(p * 2.03 + vec2f(5.2, 1.3)) * 0.35;
}

fn breakup_weight(w: f32, world: vec2f, amount: f32) -> f32 {
    return clamp(w + amount * (noise2(world * 0.11) - 0.5) * 0.9 * smoothstep(0.0, 0.6, w), 0.0, 1.0);
}

fn density(w: f32, height: f32, blend: f32) -> f32 {
    return smoothstep(0.0, 1.0, clamp((w + (height - 0.5) * 0.55 - 0.5) / max(blend, 0.00001) + 0.5, 0.0, 1.0));
}

// Tangent frame of the height-field normal: x → world x, y → world z, z → normal.
fn ground_basis(n: vec3f) -> mat3x3f {
    let b = normalize(cross(vec3f(1.0, 0.0, 0.0), n));
    let t = normalize(cross(n, b));
    return mat3x3f(t, b, n);
}

fn corner_tile(layer: Layer, mask: u32) -> u32 {
    let v = layer.corners[mask / 4u];
    return v[mask % 4u];
}

struct Ground {
    ar: vec4f,
    nh: vec4f,
    om: vec2f,
    painted: f32,
}

fn sample_tile(array_index: u32, tex: u32, uv: vec2f, dx: vec2f, dy: vec2f) -> vec4f {
    if (tex == 0u) {
        return textureSampleGrad(albedo_tiles, tile_sampler, uv, array_index, dx, dy);
    } else if (tex == 1u) {
        return textureSampleGrad(normal_tiles, tile_sampler, uv, array_index, dx, dy);
    }
    return textureSampleGrad(occlusion_tiles, tile_sampler, uv, array_index, dx, dy);
}

fn occlusion_of(layer: Layer, uv: vec2f, dx: vec2f, dy: vec2f) -> vec2f {
    if (layer.kind.w == NONE) {
        return vec2f(1.0, 0.0);
    }
    return sample_tile(layer.kind.w, 2u, uv, dx, dy).rg;
}

// The height field's normal at `xz`.
fn terrain_normal(xz: vec2f) -> vec3f {
    let mask_uv = (xz - terrain.grid.x + 0.5) / terrain.grid.z;
    return normalize(textureSampleLevel(normals, clamp_sampler, mask_uv, 0.0).xyz);
}

// A cliff layer over `g` by `alpha`: its wall texture projected on the two vertical
// planes that face `normal` most.
fn cliff_over(
    g0: Ground,
    layer: Layer,
    world: vec3f,
    base_normal: vec3f,
    dx: vec2f,
    dy: vec2f,
    height_dx: f32,
    height_dy: f32,
    alpha: f32,
) -> Ground {
    var g = g0;
    let xz = world.xz;
    let size = terrain.cells.z;
    let vertical = layer.params.z;
    let signs = select(vec2f(-1.0), vec2f(1.0), base_normal.xz >= vec2f(0.0));
    let blend_x = abs(base_normal.x) / max(abs(base_normal.x) + abs(base_normal.z), 0.0001);
    let uv_x = vec2f(-xz.y * signs.x / size, world.y / vertical);
    let uv_z = vec2f(xz.x * signs.y / size, world.y / vertical);
    let ddx_x = vec2f(-dx.y * signs.x / size, height_dx / vertical);
    let ddy_x = vec2f(-dy.y * signs.x / size, height_dy / vertical);
    let ddx_z = vec2f(dx.x * signs.y / size, height_dx / vertical);
    let ddy_z = vec2f(dy.x * signs.y / size, height_dy / vertical);
    let ax = sample_tile(layer.kind.y, 0u, uv_x, ddx_x, ddy_x);
    let az = sample_tile(layer.kind.y, 0u, uv_z, ddx_z, ddy_z);
    let nx = sample_tile(layer.kind.z, 1u, uv_x, ddx_x, ddy_x).rgb * 2.0 - 1.0;
    let nz = sample_tile(layer.kind.z, 1u, uv_z, ddx_z, ddy_z).rgb * 2.0 - 1.0;
    let ox = occlusion_of(layer, uv_x, ddx_x, ddy_x);
    let oz = occlusion_of(layer, uv_z, ddx_z, ddy_z);
    let wall_normal = normalize(mix(
        vec3f(nz.x * signs.y, nz.y, nz.z * signs.y),
        vec3f(nx.z * signs.x, nx.y, -nx.x * signs.x),
        blend_x,
    ));
    let local_normal = transpose(ground_basis(base_normal)) * wall_normal;
    var wall = mix(az, ax, blend_x);
    wall = vec4f(wall.rgb * layer.tint.rgb, wall.a);
    g.ar = mix(g.ar, wall, alpha);
    g.nh = mix(g.nh, vec4f(local_normal * 0.5 + 0.5, 0.5), alpha);
    g.om = mix(g.om, mix(oz, ox, blend_x), alpha);
    g.painted = mix(g.painted, layer.tint.a, alpha);
    return g;
}

// The composited ground at `world` (with the screen derivatives of its position) on a
// surface facing `base_normal`: the cell's layers in order, cliffs gated by its slope.
fn ground_at(
    world: vec3f,
    base_normal: vec3f,
    dx: vec2f,
    dy: vec2f,
    height_dx: f32,
    height_dy: f32,
) -> Ground {
    let xz = world.xz;
    let origin = terrain.grid.x;
    let vertices = terrain.grid.z;
    let mask_uv = (xz - origin + 0.5) / vertices;
    let slope = length(base_normal.xz) / max(base_normal.y, 0.0001);
    let span = u32(terrain.grid.y);
    let cell = vec2u(clamp(floor(xz - origin - terrain.cells.w), vec2f(0.0), vec2f(f32(span - 1u))));
    let slots = textureLoad(selection, cell, 0);

    var g: Ground;
    g.ar = vec4f(0.5, 0.5, 0.5, 1.0);
    g.nh = vec4f(0.5, 0.5, 1.0, 0.5);
    g.om = vec2f(1.0, 0.0);
    g.painted = 0.0;

    for (var s = 0u; s < 4u; s++) {
        let id = slots[s];
        if (id == NO_LAYER) {
            break;
        }
        let layer = terrain.layers[id];
        let kind = layer.kind.x;
        let floor_slot = s == 0u;
        // A tiled layer that covers the cell needs no weight; one with none here adds
        // nothing, so its tiles are not read.
        let covers = kind == KIND_BASE || (kind == KIND_TILED && floor_slot);
        var weight0 = 1.0;
        if (!covers) {
            weight0 = textureSampleLevel(masks, clamp_sampler, mask_uv, id, 0.0).r;
        }

        if (kind == KIND_BASE || kind == KIND_TILED) {
            if (!covers && weight0 <= 0.0) {
                continue;
            }
            let scale = layer.params.x * 0.08;
            let uv = xz * scale;
            var lar = sample_tile(layer.kind.y, 0u, uv, dx * scale, dy * scale);
            let lnh = sample_tile(layer.kind.z, 1u, uv, dx * scale, dy * scale);
            lar = vec4f(lar.rgb * layer.tint.rgb, lar.a);
            if (covers) {
                g.ar = lar;
                g.nh = lnh;
                g.om = vec2f(1.0, 0.0);
                g.painted = 0.0;
                continue;
            }
            let blend = layer.params.y;
            let edge = layer.params.z;
            let breakup = layer.params.w;
            var weight = weight0;
            if (breakup > 0.0) {
                weight = breakup_weight(weight, xz, breakup);
            }
            lar = vec4f(lar.rgb * mix(1.0, clamp((weight - 0.25) / 0.75, 0.0, 1.0), edge), lar.a);
            var k: f32;
            if (blend >= 0.0) {
                k = clamp(((lnh.a + 0.5) * weight - g.nh.a + 0.25) / max(blend, 0.00001), 0.0, 1.0);
            } else {
                k = clamp((1.0 - lnh.a) + (weight - (1.0 - lnh.a)) * (1.0 - blend), 0.0, 1.0);
            }
            if (breakup > 0.0) {
                k = density(weight, lnh.a, blend);
            }
            g.ar = mix(g.ar, lar, k);
            g.nh = mix(g.nh, vec4f(lnh.rgb, select(0.5, lnh.a, blend >= 0.0)), k);
            g.om = mix(g.om, vec2f(1.0, 0.0), k);
            g.painted = mix(g.painted, 0.0, k);
        } else if (kind == KIND_ATLAS) {
            let cols = layer.params.x;
            let rows = layer.params.y;
            let slot = u32(layer.params.z);
            let size = terrain.cells.z;
            let grid = (xz - terrain.cells.xy) / size;
            let cell_index = vec2i(floor(grid));
            let local = fract(grid);
            let in_grid = all(cell_index >= vec2i(0)) && all(cell_index < vec2i(terrain.counts.xy));
            var code = 0u;
            var full_tile = 0u;
            if (in_grid) {
                let data = textureLoad(atlas_cells, vec2u(cell_index), slot, 0);
                code = data.r;
                full_tile = data.g;
            }
            let corners = code & 15u;
            let full = (code & 16u) != 0u || corners == 15u || floor_slot;
            if (corners == 0u && weight0 <= 0.0 && !full) {
                continue;
            }
            var tile = full_tile;
            if (!full && corners != 0u) {
                tile = corner_tile(layer, corners);
            }
            let dims = vec2f(cols, rows);
            let image_cell = vec2f(f32(tile % u32(cols)), floor(f32(tile) / cols));
            let gdx = dx / size / dims;
            let gdy = dy / size / dims;
            let footprint = max(length(gdx * 1024.0), length(gdy * 1024.0));
            let inset = min(vec2f(max(0.5, ceil(footprint))) / 1024.0, 0.4 / dims);
            let uv = clamp((image_cell + local) / dims, image_cell / dims + inset, (image_cell + 1.0) / dims - inset);
            var lar = sample_tile(layer.kind.y, 0u, uv, gdx, gdy);
            lar = vec4f(lar.rgb * layer.tint.rgb, lar.a);
            let lnh = sample_tile(layer.kind.z, 1u, uv, gdx, gdy);
            let lom = occlusion_of(layer, uv, gdx, gdy);
            var alpha = lnh.a;
            if (full) {
                alpha = 1.0;
            } else if (corners == 0u) {
                alpha = weight0;
            }
            g.ar = mix(g.ar, lar, alpha);
            g.nh = mix(g.nh, vec4f(lnh.rgb, 0.5), alpha);
            g.om = mix(g.om, lom, alpha);
            g.painted = mix(g.painted, layer.tint.a, alpha);
        } else if (kind == KIND_CLIFF) {
            if (weight0 <= 0.0) {
                continue;
            }
            let alpha = weight0 * smoothstep(layer.params.x, layer.params.y, slope);
            if (alpha <= 0.0) {
                continue;
            }
            g = cliff_over(g, layer, world, base_normal, dx, dy, height_dx, height_dy, alpha);
        }
    }

    // The map's painted ground over every layer (premultiplied alpha), before lighting.
    if (terrain.overlay.z > 0.0) {
        let uv = (world.xz - terrain.overlay.xy) * terrain.overlay.zw;
        if (all(uv >= vec2f(0.0)) && all(uv <= vec2f(1.0))) {
            let o = textureSampleGrad(overlay_image, clamp_sampler, uv, dx * terrain.overlay.zw, dy * terrain.overlay.zw);
            g.ar = vec4f(o.rgb + g.ar.rgb * (1.0 - o.a), g.ar.a);
        }
    }
    return g;
}

@fragment
fn fs_main(in: VsOut) -> SceneOut {
    let frag = in.clip;
    let world = in.world;
    let base_normal = terrain_normal(world.xz);
    let g = ground_at(world, base_normal, dpdx(world.xz), dpdy(world.xz), dpdx(world.y), dpdy(world.y));
    let tangent = 2.0 * (g.nh.rgb - 0.50196) * vec3f(1.0, 1.0, 0.55);
    let normal = normalize(ground_basis(base_normal) * tangent);
    var surface: Surface;
    surface.position = world;
    surface.normal = normal;
    surface.albedo = g.ar.rgb;
    surface.roughness = g.ar.a;
    surface.metalness = g.om.y;
    surface.occlusion = g.om.x;
    surface.backside = 0.0;
    surface.specular = 1.0 - g.painted;
    surface.model = LIGHT_GROUND;
    let view = normalize(frame.eye.xyz - world);
    let color = surface_light(surface, view, frag.xy);
    return scene_out(vec4f(color, 1.0), world);
}

// --- Building foundations (D44, `terrain/foundations.rs`) -----------------------------

// The terrain surface as its triangles draw it (diagonal from (x + 1, z) to (x, z + 1)).
fn ground_height(xz: vec2f) -> f32 {
    let span = terrain.grid.y;
    let g = clamp(xz - terrain.grid.x, vec2f(0.0), vec2f(span));
    let cell = min(floor(g), vec2f(span - 1.0));
    let f = g - cell;
    let i = vec2u(cell);
    let b = height_at(i.x + 1u, i.y);
    let c = height_at(i.x, i.y + 1u);
    if (f.x + f.y <= 1.0) {
        let a = height_at(i.x, i.y);
        return a + (b - a) * f.x + (c - a) * f.y;
    }
    let d = height_at(i.x + 1u, i.y + 1u);
    return d + (c - d) * (1.0 - f.x) + (b - d) * (1.0 - f.y);
}

struct FoundationRecord {
    // centre x, z; half width, depth
    rect: vec4f,
    // x: base height
    base: vec4f,
}

@group(2) @binding(1) var<storage, read> foundations: array<FoundationRecord>;

// Must match `terrain/foundations.rs`.
const SIDE_SEGMENTS: u32 = 14u;
const CORNER_STEPS: u32 = 6u;
const SIDE_SPOKES: u32 = 20u;
const SPOKES: u32 = 80u;
const BANDS: u32 = 8u;
const BANK_VERTICES: u32 = 3840u;
const HALF_PI: f32 = 1.5707964;
const LIFT: f32 = 0.05;
const TOE_SINK: f32 = 0.02;
const MIN_BANK: f32 = 0.4;
const BATTER: f32 = 2.4;
const MAX_BANK: f32 = 5.0;
const CORNER_SHARE: f32 = 0.4;
const MAX_CORNER: f32 = 3.0;

// How the bank's width varies along the rim (`foundations::ragged`).
fn ragged(edge: vec2f, center: vec2f) -> f32 {
    let s = edge.x + edge.y;
    let phase = center.x * 0.37 + center.y * 0.61;
    return 1.0 + 0.2 * sin(0.45 * s + phase) + 0.1 * sin(1.1 * s + 1.7 * phase);
}

// The radius of the rim's rounded corners.
fn corner_radius(f: FoundationRecord) -> f32 {
    return min(CORNER_SHARE * min(f.rect.z, f.rect.w), MAX_CORNER);
}

struct Spoke {
    edge: vec2f,
    out: vec2f,
}

// Spoke `k` around the rim (the footprint with rounded corners): a point on it and the
// outward direction. Each side has its straight samples, then the corner's arc.
fn spoke(f: FoundationRecord, k: u32) -> Spoke {
    let side = (k / SIDE_SPOKES) % 4u;
    let j = k % SIDE_SPOKES;
    var corners = array<vec2f, 4>(vec2f(-1.0, -1.0), vec2f(1.0, -1.0), vec2f(1.0, 1.0), vec2f(-1.0, 1.0));
    let r = corner_radius(f);
    let inner = f.rect.zw - vec2f(r);
    let ca = f.rect.xy + corners[side] * inner;
    let cb = f.rect.xy + corners[(side + 1u) % 4u] * inner;
    var turn = (f32(side) - 1.0) * HALF_PI;
    var s: Spoke;
    if (j <= SIDE_SEGMENTS) {
        s.out = vec2f(cos(turn), sin(turn));
        s.edge = mix(ca, cb, f32(j) / f32(SIDE_SEGMENTS)) + s.out * r;
    } else {
        turn += HALF_PI * f32(j - SIDE_SEGMENTS) / f32(CORNER_STEPS);
        s.out = vec2f(cos(turn), sin(turn));
        s.edge = cb + s.out * r;
    }
    return s;
}

// The bank's width at a rim point.
fn bank_width(f: FoundationRecord, edge: vec2f) -> f32 {
    let drop = max(f.base.x - LIFT - ground_height(edge), 0.0);
    return clamp(max(MIN_BANK, drop * BATTER) * ragged(edge, f.rect.xy), MIN_BANK, MAX_BANK);
}

// A point of the pad or bank along spoke `s`: `u` < 0 lies on the pad, 0 is the rim, 1
// the toe, where the bank meets the ground tangentially.
fn bank_point(f: FoundationRecord, s: Spoke, width: f32, u: f32) -> vec3f {
    let top = f.base.x - LIFT;
    let p = s.edge + s.out * width * u;
    if (u <= 0.0) {
        return vec3f(p.x, top, p.y);
    }
    let t = smoothstep(0.0, 1.0, min(u, 1.0));
    return vec3f(p.x, mix(top, ground_height(p) - TOE_SINK, t), p.y);
}

struct FoundationVertex {
    world: vec3f,
    normal: vec3f,
    // 0 on the pad and at the rim, 1 at the toe
    toe: f32,
}

fn foundation_vertex(vid: u32, inst: u32) -> FoundationVertex {
    let f = foundations[inst];
    var v: FoundationVertex;
    if (vid >= BANK_VERTICES) {
        // The pad: a fan from the centre to the rim.
        let i = vid - BANK_VERTICES;
        let k = (i / 3u + select(0u, 1u, i % 3u == 2u)) % SPOKES;
        var p = f.rect.xy;
        if (i % 3u != 0u) {
            p = spoke(f, k).edge;
        }
        v.world = vec3f(p.x, f.base.x - LIFT, p.y);
        v.normal = vec3f(0.0, 1.0, 0.0);
        v.toe = 0.0;
        return v;
    }
    let quad = vid / 6u;
    var corners = array<vec2u, 6>(
        vec2u(0u, 0u), vec2u(0u, 1u), vec2u(1u, 0u),
        vec2u(1u, 0u), vec2u(0u, 1u), vec2u(1u, 1u),
    );
    let o = corners[vid % 6u];
    let k = (quad / BANDS + o.x) % SPOKES;
    let u = f32(quad % BANDS + o.y) / f32(BANDS);
    let s = spoke(f, k);
    let w = bank_width(f, s.edge);
    v.world = bank_point(f, s, w, u);
    v.toe = u;
    // Smooth normals: central differences across the rings (onto the pad at the rim, so
    // the shoulder is lit as the pad) and the neighbouring spokes.
    let du = 0.5 / f32(BANDS);
    let down = bank_point(f, s, w, u + du) - bank_point(f, s, w, u - du);
    let sa = spoke(f, (k + 1u) % SPOKES);
    let sb = spoke(f, (k + SPOKES - 1u) % SPOKES);
    var along = bank_point(f, sa, bank_width(f, sa.edge), u) - bank_point(f, sb, bank_width(f, sb.edge), u);
    if (dot(along.xz, along.xz) < 1e-6) {
        along = vec3f(-s.out.y, 0.0, s.out.x);
    }
    var n = normalize(cross(along, down));
    if (n.y < 0.0) {
        n = -n;
    }
    v.normal = n;
    return v;
}

struct FoundationOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) normal: vec3f,
    @location(2) toe: f32,
}

@vertex
fn vs_foundation(@builtin(vertex_index) vid: u32, @builtin(instance_index) inst: u32) -> FoundationOut {
    let v = foundation_vertex(vid, inst);
    var out: FoundationOut;
    out.world = v.world;
    out.normal = v.normal;
    out.toe = v.toe;
    out.clip = frame.view_proj * vec4f(v.world, 1.0);
    return out;
}

@vertex
fn vs_foundation_shadow(@builtin(vertex_index) vid: u32, @builtin(instance_index) inst: u32) -> @builtin(position) vec4f {
    return frame.view_proj * vec4f(foundation_vertex(vid, inst).world, 1.0);
}

// The pad and the mound are the ground itself: the terrain's composite at the same place
// (its layers, world-space mapping and cliff rule exactly as the terrain's there), lit
// with the mound's normal, which turns into the terrain's towards the toe so the two
// meet without a seam.
@fragment
fn fs_foundation(in: FoundationOut) -> SceneOut {
    let world = in.world;
    let ground_normal = terrain_normal(world.xz);
    let dx = dpdx(world);
    let dy = dpdy(world);
    let g = ground_at(world, ground_normal, dx.xz, dy.xz, dx.y, dy.y);
    let bank = normalize(in.normal);
    var n = normalize(mix(bank, ground_normal, smoothstep(0.55, 1.0, in.toe)));
    n = normalize(vec3f(n.x, max(n.y, 0.05), n.z));
    let tangent = 2.0 * (g.nh.rgb - 0.50196) * vec3f(1.0, 1.0, 0.55);
    var surface: Surface;
    surface.position = world;
    surface.normal = normalize(ground_basis(n) * tangent);
    surface.albedo = g.ar.rgb;
    surface.roughness = g.ar.a;
    surface.metalness = g.om.y;
    surface.occlusion = g.om.x;
    surface.backside = 0.0;
    surface.specular = 1.0 - g.painted;
    surface.model = LIGHT_GROUND;
    let view = normalize(frame.eye.xyz - world);
    let color = surface_light(surface, view, in.clip.xy);
    return scene_out(vec4f(color, 1.0), world);
}
