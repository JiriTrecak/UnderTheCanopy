// GPU culling of static instances for the camera (view 0) and the sun (view 1): each
// visible instance is appended to its model's list for that view, then every draw's
// indirect instance count is set from the lists. The sun's list keeps only casters whose
// shadow can reach the camera's view (shadow.rs, `CasterCull`). Canopy instances the
// cutaway or the RTS ceiling may cut this frame go to a third list (view 2) instead of
// the camera's: only they take the depth prepass that carries the cuts (statics.wgsl).

struct Params {
    // 6 planes per view: n·p + d ≥ 0 inside
    planes: array<vec4f, 12>,
    // items, draws, 0, 0
    counts: vec4u,
    // The camera's side and near planes for casters (CasterCull::planes).
    caster_planes: array<vec4f, 5>,
    // xyz towards the sun, w the margin
    sweep: vec4f,
    // x: the lowest ground (CasterCull::floor)
    floor: vec4f,
    // the shadow map's depth row
    sun_depth: vec4f,
    // the camera's view-projection
    view_proj: mat4x4f,
    // x: the cutaway is open (0/1), y, z: blocks of its mask across and down
    cutaway: vec4f,
}

struct Item {
    // world bounding sphere
    sphere: vec4f,
    // model, casts shadow, hidden, what may cut it (CUT_*)
    info: vec4u,
}

// A canopy instance tall enough for the cutaway (statics.wgsl `cut_away`).
const CUT_CUTAWAY: u32 = 1u;
// A canopy instance reaching the RTS ceiling (statics.wgsl `above_rts`).
const CUT_CEILING: u32 = 2u;
// The cutaway mask (cutaway.rs): texels and texels on a side of a block.
const MASK: vec2f = vec2f(192.0, 108.0);
const MASK_BLOCK: i32 = 8;
// How far wind may lift or bend a vertex beyond the bounds (statics.rs `SWAY_REACH`).
const SWAY_REACH: f32 = 1.0;

struct Region {
    // first visible slot per list (camera, sun, camera with cuts), 0
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
// The farthest covering unit's depth per block of the cutaway mask, 0 where none.
@group(0) @binding(7) var<storage, read> cut_blocks: array<f32>;

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
    var length = max((1.0 - depth) / rate, 0.0) + radius;
    if (towards_sun.y > 1e-3) {
        length = min(length, max((sphere.y + radius - params.floor.x) / towards_sun.y, 0.0));
    }
    let end = sphere.xyz - towards_sun * length;
    for (var i = 0u; i < 5u; i++) {
        let p = params.caster_planes[i];
        if (dot(p.xyz, sphere.xyz) + p.w < -radius && dot(p.xyz, end) + p.w < -radius) {
            return false;
        }
    }
    return true;
}

// Whether any fragment of the instance may be cut: it reaches the RTS ceiling, or a unit
// stands more than the cutaway's 0.4 m behind its nearest point somewhere in the mask
// texels its screen rectangle reads (the fragment's bilinear read reaches a texel past
// it). Instances that cannot be cut draw without the prepass, exactly as they look.
fn may_cut(item: Item) -> bool {
    let cuts = item.info.w;
    if ((cuts & CUT_CEILING) != 0u) {
        return true;
    }
    if ((cuts & CUT_CUTAWAY) == 0u || params.cutaway.x < 0.5) {
        return false;
    }
    let centre = item.sphere.xyz;
    let r = item.sphere.w + SWAY_REACH;
    var lo = vec2f(1e30);
    var hi = vec2f(-1e30);
    for (var i = 0u; i < 8u; i++) {
        let corner = centre + vec3f(
            select(-r, r, (i & 1u) != 0u),
            select(-r, r, (i & 2u) != 0u),
            select(-r, r, (i & 4u) != 0u),
        );
        let clip = params.view_proj * vec4f(corner, 1.0);
        if (clip.w <= 1e-4) {
            return true;
        }
        let ndc = clip.xy / clip.w;
        lo = min(lo, ndc);
        hi = max(hi, ndc);
    }
    let w_row = vec4f(params.view_proj[0].w, params.view_proj[1].w, params.view_proj[2].w, params.view_proj[3].w);
    let nearest = dot(w_row, vec4f(centre, 1.0)) - r * length(w_row.xyz);
    // Mask texels, rows from the top, with a texel of margin on each side.
    let t0 = vec2f(lo.x * 0.5 + 0.5, 0.5 - hi.y * 0.5) * MASK - 0.5;
    let t1 = vec2f(hi.x * 0.5 + 0.5, 0.5 - lo.y * 0.5) * MASK - 0.5;
    let last = vec2i(MASK) - 1;
    let a = clamp(vec2i(floor(t0)) - 1, vec2i(0), last) / MASK_BLOCK;
    let b = clamp(vec2i(floor(t1)) + 2, vec2i(0), last) / MASK_BLOCK;
    let across = i32(params.cutaway.y);
    for (var y = a.y; y <= b.y; y++) {
        for (var x = a.x; x <= b.x; x++) {
            if (cut_blocks[y * across + x] - 0.4 > nearest - 0.01) {
                return true;
            }
        }
    }
    return false;
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
            var list = view;
            if (view == 0u && item.info.w != 0u && may_cut(item)) {
                list = 2u;
            }
            let slot = atomicAdd(&counters[model * 3u + list], 1u);
            visible[regions[model].base[list] + slot] = index;
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
    args[index * 5u + 1u] = atomicLoad(&counters[draw.x * 3u + draw.y]);
}
