// GPU culling of static instances for the camera (view 0) and the sun (view 1): each
// visible instance is appended to its model's list for that view, then every draw's
// indirect instance count is set from the lists. The sun's list keeps only casters whose
// shadow can reach the camera's view (shadow.rs, `CasterCull`).

struct Params {
    // 6 planes per view: n·p + d ≥ 0 inside
    planes: array<vec4f, 12>,
    // items, draws, 0, 0
    counts: vec4u,
    // The camera's side and near planes for casters (CasterCull::planes).
    caster_planes: array<vec4f, 5>,
    // xyz towards the sun, w the margin
    sweep: vec4f,
    // the shadow map's depth row
    sun_depth: vec4f,
}

struct Item {
    // world bounding sphere
    sphere: vec4f,
    // model, casts shadow, hidden, 0
    info: vec4u,
}

struct Region {
    // first visible slot per view (camera, sun), 0, 0
    base: vec4u,
}

struct Draw {
    // model, view, 0, 0
    info: vec4u,
}

@group(0) @binding(0) var<uniform> params: Params;
@group(0) @binding(1) var<storage, read> items: array<Item>;
@group(0) @binding(2) var<storage, read> regions: array<Region>;
@group(0) @binding(3) var<storage, read_write> counters: array<atomic<u32>>;
@group(0) @binding(4) var<storage, read_write> visible: array<u32>;
@group(0) @binding(5) var<storage, read> draws: array<Draw>;
@group(0) @binding(6) var<storage, read_write> args: array<u32>;

fn inside(view: u32, sphere: vec4f) -> bool {
    for (var i = 0u; i < 6u; i++) {
        let p = params.planes[view * 6u + i];
        if (dot(p.xyz, sphere.xyz) + p.w < -sphere.w) {
            return false;
        }
    }
    return true;
}

// `CasterCull::casts_into_view`.
fn casts_into_view(sphere: vec4f) -> bool {
    let towards_sun = params.sweep.xyz;
    let radius = sphere.w + params.sweep.w;
    let rate = -dot(params.sun_depth.xyz, towards_sun);
    if (rate <= 1e-6) {
        return true;
    }
    let depth = dot(params.sun_depth, vec4f(sphere.xyz, 1.0));
    let length = max((1.0 - depth) / rate, 0.0) + radius;
    let end = sphere.xyz - towards_sun * length;
    for (var i = 0u; i < 5u; i++) {
        let p = params.caster_planes[i];
        if (dot(p.xyz, sphere.xyz) + p.w < -radius && dot(p.xyz, end) + p.w < -radius) {
            return false;
        }
    }
    return true;
}

@compute @workgroup_size(64)
fn cull(@builtin(global_invocation_id) id: vec3u) {
    let index = id.x;
    if (index >= params.counts.x) {
        return;
    }
    let item = items[index];
    if (item.info.z != 0u) {
        return;
    }
    let model = item.info.x;
    for (var view = 0u; view < 2u; view++) {
        if (view == 1u && item.info.y == 0u) {
            continue;
        }
        if (inside(view, item.sphere) && (view == 0u || casts_into_view(item.sphere))) {
            let slot = atomicAdd(&counters[model * 2u + view], 1u);
            visible[regions[model].base[view] + slot] = index;
        }
    }
}

@compute @workgroup_size(64)
fn finish(@builtin(global_invocation_id) id: vec3u) {
    let index = id.x;
    if (index >= params.counts.y) {
        return;
    }
    let draw = draws[index].info;
    args[index * 5u + 1u] = atomicLoad(&counters[draw.x * 2u + draw.y]);
}
