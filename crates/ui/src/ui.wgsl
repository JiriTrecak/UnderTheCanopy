// The UI pass: one instanced quad per primitive (see draw.rs, `Quad`).
//
// Colours arrive as sRGB with straight alpha and the pass blends premultiplied in sRGB
// space, as browsers do, unless the target is an sRGB format (then the output is
// linearised and the hardware re-encodes it).

struct Globals {
    size: vec2<f32>,
    srgb_target: u32,
    _pad: u32,
};

@group(0) @binding(0) var<uniform> g: Globals;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var atlas: texture_2d<f32>;
@group(1) @binding(0) var image: texture_2d<f32>;

struct Instance {
    @location(0) rect: vec4<f32>,
    @location(1) uv: vec4<f32>,
    @location(2) color: vec4<f32>,
    @location(3) color2: vec4<f32>,
    @location(4) params: vec4<f32>,
    @location(5) shade: f32,
    @location(6) flags: u32,
};

struct VOut {
    @builtin(position) pos: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) @interpolate(flat) half_size: vec2<f32>,
    @location(3) @interpolate(flat) color: vec4<f32>,
    @location(4) @interpolate(flat) color2: vec4<f32>,
    @location(5) @interpolate(flat) params: vec4<f32>,
    @location(6) @interpolate(flat) shade: f32,
    @location(7) @interpolate(flat) flags: u32,
    @location(8) @interpolate(flat) offset: vec2<f32>,
};

const KIND_SHAPE: u32 = 0u;
const KIND_IMAGE: u32 = 1u;
const KIND_GLYPH: u32 = 2u;
const KIND_SHADOW: u32 = 3u;
const KIND_INSET: u32 = 4u;
const KIND_SWEEP: u32 = 5u;
const KIND_LINEAR: u32 = 6u;
const KIND_RADIAL: u32 = 7u;
const FLAG_PREMULTIPLIED: u32 = 256u;
const FLAG_GRAYSCALE: u32 = 512u;
const FLAG_LINEAR_TEXELS: u32 = 1024u;

@vertex
fn vs(@builtin(vertex_index) vi: u32, inst: Instance) -> VOut {
    // Two triangles: corners 0 1 2, 2 1 3.
    var corners = array<vec2<f32>, 6>(
        vec2(0.0, 0.0), vec2(1.0, 0.0), vec2(0.0, 1.0),
        vec2(0.0, 1.0), vec2(1.0, 0.0), vec2(1.0, 1.0),
    );
    let c = corners[vi];
    let p = mix(inst.rect.xy, inst.rect.zw, c);
    var out: VOut;
    out.pos = vec4(p.x / g.size.x * 2.0 - 1.0, 1.0 - p.y / g.size.y * 2.0, 0.0, 1.0);
    let center = (inst.rect.xy + inst.rect.zw) * 0.5;
    out.local = p - center;
    out.half_size = (inst.rect.zw - inst.rect.xy) * 0.5;
    out.uv = mix(inst.uv.xy, inst.uv.zw, c);
    out.offset = inst.uv.xy;
    out.color = inst.color;
    out.color2 = inst.color2;
    out.params = inst.params;
    out.shade = inst.shade;
    out.flags = inst.flags;
    return out;
}

fn sd_round_rect(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2(r);
    return length(max(q, vec2(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// Antialiased coverage of the inside (d < 0) of a distance field.
fn cover(d: f32) -> f32 {
    return clamp(0.5 - d, 0.0, 1.0);
}

fn premul(c: vec4<f32>) -> vec4<f32> {
    return vec4(c.rgb * c.a, c.a);
}

// A soft edge for shadows: 1 inside, fading over `blur` pixels either side of the edge.
fn soft_inside(d: f32, blur: f32) -> f32 {
    if blur <= 0.0 {
        return cover(d);
    }
    return 1.0 - smoothstep(-blur, blur, d);
}

fn to_linear(c: vec3<f32>) -> vec3<f32> {
    let lo = c / 12.92;
    let hi = pow((c + 0.055) / 1.055, vec3(2.4));
    return select(hi, lo, c <= vec3(0.04045));
}

fn to_srgb(c: vec3<f32>) -> vec3<f32> {
    let lo = c * 12.92;
    let hi = 1.055 * pow(c, vec3(1.0 / 2.4)) - 0.055;
    return select(hi, lo, c <= vec3(0.0031308));
}

@fragment
fn fs(in: VOut) -> @location(0) vec4<f32> {
    // Sampled in uniform control flow (mips need derivatives). The glyph atlas has one
    // level, so glyphs sample it explicitly where they need it.
    let texel = textureSample(image, samp, in.uv);

    let kind = in.flags & 0xffu;
    let r = in.params.x;
    let border = in.params.y;
    let soft = in.params.z;
    let param = in.params.w;
    let d = sd_round_rect(in.local, in.half_size, r);
    var out = vec4(0.0);

    switch kind {
        case KIND_SHAPE: {
            var fill = premul(in.color);
            if border > 0.0 {
                let inner = sd_round_rect(in.local, in.half_size - vec2(border), max(r - border, 0.0));
                fill = mix(premul(in.color2), fill, cover(inner));
            }
            out = fill * cover(d);
        }
        case KIND_IMAGE: {
            var t = texel;
            if (in.flags & FLAG_LINEAR_TEXELS) != 0u && t.a > 0.0 {
                // Straight or premultiplied, encode the colour, not the coverage.
                let straight = select(t.rgb / t.a, t.rgb, (in.flags & FLAG_PREMULTIPLIED) == 0u);
                let encoded = to_srgb(straight);
                t = vec4(select(encoded * t.a, encoded, (in.flags & FLAG_PREMULTIPLIED) == 0u), t.a);
            }
            if (in.flags & FLAG_PREMULTIPLIED) == 0u {
                t = premul(t);
            }
            if (in.flags & FLAG_GRAYSCALE) != 0u {
                t = vec4(vec3(dot(t.rgb, vec3(0.2126, 0.7152, 0.0722))), t.a);
            }
            out = vec4(t.rgb * in.color.rgb, t.a) * in.color.a;
            if r > 0.0 {
                out *= cover(d);
            }
        }
        case KIND_GLYPH: {
            var a = textureSampleLevel(atlas, samp, in.uv, 0.0).r;
            if soft > 0.0 {
                // Blurred shadow: a 5×5 tent over the glyph's padded atlas cell.
                let texel_size = 1.0 / vec2<f32>(textureDimensions(atlas));
                let step = texel_size * soft * 0.5;
                var sum = 0.0;
                for (var y = -2; y <= 2; y++) {
                    for (var x = -2; x <= 2; x++) {
                        let w = f32(3 - abs(x)) * f32(3 - abs(y));
                        sum += w * textureSampleLevel(atlas, samp, in.uv + vec2(f32(x), f32(y)) * step, 0.0).r;
                    }
                }
                a = sum / 81.0;
            }
            out = premul(in.color) * a;
        }
        case KIND_SHADOW: {
            // The quad is the shadow shape grown by blur + 1; the element sits at -offset.
            let shape_half = in.half_size - vec2(soft + 1.0);
            let ds = sd_round_rect(in.local, shape_half, r + border);
            let de = sd_round_rect(in.local + in.offset, shape_half - vec2(border), r);
            out = premul(in.color) * soft_inside(ds, soft) * clamp(0.5 + de, 0.0, 1.0);
        }
        case KIND_INSET: {
            let di = sd_round_rect(in.local - in.offset, in.half_size - vec2(border), max(r - border, 0.0));
            out = premul(in.color) * (1.0 - soft_inside(di, soft)) * cover(d);
        }
        case KIND_SWEEP: {
            let angle = atan2(in.local.x, -in.local.y);
            let frac = fract(angle / 6.2831853 + 1.0);
            // Antialias the sweep edge by its arc length in pixels.
            let edge = clamp(0.5 + (param - frac) * 6.2831853 * length(in.local), 0.0, 1.0);
            let swept = select(edge, 1.0, param >= 1.0);
            var fill = mix(premul(in.color2), premul(in.color), swept);
            var mask = cover(d);
            if border > 0.0 {
                let inner = sd_round_rect(in.local, in.half_size - vec2(border), max(r - border, 0.0));
                mask *= clamp(0.5 + inner, 0.0, 1.0);
            }
            out = fill * mask;
        }
        case KIND_LINEAR: {
            let a = radians(param);
            let dir = vec2(sin(a), -cos(a));
            let len = abs(in.half_size.x * 2.0 * dir.x) + abs(in.half_size.y * 2.0 * dir.y);
            let t = clamp(dot(in.local, dir) / max(len, 1.0) + 0.5, 0.0, 1.0);
            out = mix(premul(in.color), premul(in.color2), t) * cover(d);
        }
        case KIND_RADIAL: {
            let t = clamp(length(in.local / max(in.half_size * 1.4142135, vec2(1.0))), 0.0, 1.0);
            out = mix(premul(in.color), premul(in.color2), t) * cover(d);
        }
        default: {}
    }
    out = vec4(min(out.rgb * in.shade, vec3(out.a)), out.a);
    if g.srgb_target != 0u && out.a > 0.0 {
        out = vec4(to_linear(out.rgb / out.a) * out.a, out.a);
    }
    return out;
}
