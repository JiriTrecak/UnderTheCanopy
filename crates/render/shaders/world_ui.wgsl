// World UI (world_ui.rs): ground triangles inside the forward pass (selection circles,
// rally routes) and the overlay in output pixels after the final pass (vital bars,
// labels, badges, flags). Colours are linear with straight alpha. Needs Frame and Color.

// --- Ground ------------------------------------------------------------------------

struct GroundOut {
    @builtin(position) clip: vec4f,
    @location(0) world: vec3f,
    @location(1) color: vec4f,
}

@vertex
fn vs_ground(@location(0) position: vec3f, @location(1) color: vec4f) -> GroundOut {
    var out: GroundOut;
    out.clip = frame.view_proj * vec4f(position, 1.0);
    out.world = position;
    out.color = color;
    return out;
}

@fragment
fn fs_ground(in: GroundOut) -> SceneOut {
    // Not fogged: the viewer's own markers stay readable (old materials ignored fog).
    var out: SceneOut;
    out.color = in.color;
    out.depth = vec2f(0.0);
    return out;
}

// --- Overlay -----------------------------------------------------------------------

struct Overlay {
    // 2 / width, −2 / height, output is sRGB, 0
    scale: vec4f,
}

@group(1) @binding(0) var glyph_atlas: texture_2d<f32>;
@group(1) @binding(1) var icon_atlas: texture_2d<f32>;
@group(1) @binding(2) var atlas_sampler: sampler;
@group(1) @binding(3) var<uniform> overlay: Overlay;

const O_SOLID: u32 = 0u;
const O_BOX: u32 = 1u;
const O_GLYPH: u32 = 2u;
const O_ICON: u32 = 3u;
const O_VITAL: u32 = 4u;

struct OverlayIn {
    @location(0) position: vec2f,
    @location(1) uv: vec2f,
    @location(2) color: vec4f,
    @location(3) params: vec4f,
    @location(4) extra: vec4f,
    @location(5) kind: vec4u,
}

struct OverlayOut {
    @builtin(position) clip: vec4f,
    @location(0) uv: vec2f,
    @location(1) color: vec4f,
    @location(2) params: vec4f,
    @location(3) extra: vec4f,
    @location(4) @interpolate(flat) kind: vec4u,
}

@vertex
fn vs_overlay(in: OverlayIn) -> OverlayOut {
    var out: OverlayOut;
    out.clip = vec4f(in.position * overlay.scale.xy + vec2f(-1.0, 1.0), 0.0, 1.0);
    out.uv = in.uv;
    out.color = in.color;
    out.params = in.params;
    out.extra = in.extra;
    out.kind = in.kind;
    return out;
}

// The old health and mana bar (`VitalBar`): dark frame, shaded fill, separators every
// 100 health. `p` in CSS pixels from the top left.
fn vital(p: vec2f, size: vec2f, health: f32, max_health: f32, mana_fill: f32, has_mana: bool, tint: vec3f) -> vec4f {
    let mana = p.y >= 10.0;
    let local_y = select(p.y, p.y - 10.0, mana);
    let row = select(8.0, 5.0, mana);
    if ((p.y >= 8.0 && p.y < 10.0) || (mana && !has_mana)) {
        return vec4f(0.0);
    }
    var bar = vec3f(0.008, 0.012, 0.016);
    let inner = p.x >= 1.0 && p.x < size.x - 1.0 && local_y >= 1.0 && local_y < row - 1.0;
    if (inner) {
        let x = (p.x - 1.0) / (size.x - 2.0);
        let fill = select(health, mana_fill, mana);
        bar = vec3f(0.018, 0.023, 0.026);
        if (x < fill) {
            let colour = select(tint, vec3f(0.024, 0.19, 0.66), mana);
            let shade = mix(1.24, 0.62, (local_y - 1.0) / (row - 2.0));
            bar = colour * shade;
            if (local_y < 2.0) {
                bar += vec3f(0.055);
            }
        }
        if (!mana && max_health > 100.0) {
            let spacing = (size.x - 2.0) * 100.0 / max_health;
            let n = floor((p.x - 1.0) / spacing + 0.5);
            let line = 1.0 - smoothstep(0.3, 0.8, abs((p.x - 1.0) - n * spacing));
            if (n >= 1.0 && n * 100.0 < max_health) {
                bar = mix(bar, vec3f(0.025, 0.030, 0.032), line * 0.78 * smoothstep(1.0, 2.0, spacing));
            }
        }
    }
    return vec4f(bar, 1.0);
}

// Coverage of a rounded box of `size` with `radius` at pixel `p`.
fn rounded(p: vec2f, size: vec2f, radius: f32) -> f32 {
    let q = abs(p - size * 0.5) - (size * 0.5 - vec2f(radius));
    let d = length(max(q, vec2f(0.0))) + min(max(q.x, q.y), 0.0) - radius;
    return clamp(0.5 - d, 0.0, 1.0);
}

@fragment
fn fs_overlay(in: OverlayOut) -> @location(0) vec4f {
    var color = in.color;
    switch in.kind.x {
        case O_BOX: {
            let size = in.params.xy;
            let outer = rounded(in.uv, size, in.params.z);
            let b = in.params.w;
            let inner = rounded(in.uv - vec2f(b), size - vec2f(2.0 * b), max(in.params.z - b, 0.0));
            // Border colour outside the inner box, fill inside.
            let fill = vec4f(in.color.rgb, in.color.a * inner);
            let border = vec4f(in.extra.rgb, in.extra.a * (outer - inner));
            let a = fill.a + border.a;
            color = vec4f((fill.rgb * fill.a + border.rgb * border.a) / max(a, 1e-4), a);
        }
        case O_GLYPH: {
            color.a *= textureSampleLevel(glyph_atlas, atlas_sampler, in.uv, 0.0).r;
        }
        case O_ICON: {
            color *= textureSampleLevel(icon_atlas, atlas_sampler, in.uv, 0.0);
        }
        case O_VITAL: {
            color = vital(in.uv, in.params.xy, in.params.z, in.params.w, in.extra.x, in.extra.y > 0.5, in.color.rgb);
        }
        default: {}
    }
    if (color.a <= 0.0) {
        discard;
    }
    if (overlay.scale.z < 0.5) {
        color = vec4f(srgb_encode(color.rgb), color.a);
    }
    return color;
}
