// rounded_rect.wgsl — SDF-based rounded rectangle primitive (S5.5a).
//
// One pipeline, instanced draws. Each instance is a (pos, size, radius,
// border_width, fill, border, shadow_color, shadow_offset, shadow_blur)
// tuple. Vertex stage stamps a quad expanded outward by the shadow's
// reach so the fragment stage can paint the shadow halo past the rect's
// own bounds. Fragment stage evaluates the rounded-box signed distance
// field for fill + border, evaluates it again at `shadow_offset` for the
// shadow's gaussian falloff, then composites all three with pre-multiplied
// alpha (pipeline blend = One, OneMinusSrcAlpha).

struct Globals {
    viewport: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) radius: f32,
    @location(3) border_width: f32,
    @location(4) fill: vec4<f32>,
    @location(5) border: vec4<f32>,
    @location(6) shadow_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_blur: f32,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @location(0) inst_pos: vec2<f32>,
    @location(1) inst_size: vec2<f32>,
    @location(2) inst_radius: f32,
    @location(3) inst_border_width: f32,
    @location(4) inst_fill: vec4<f32>,
    @location(5) inst_border: vec4<f32>,
    @location(6) inst_shadow_color: vec4<f32>,
    @location(7) inst_shadow_offset: vec2<f32>,
    @location(8) inst_shadow_blur: f32,
) -> VsOut {
    // Triangle-strip unit quad: vid 0→(0,0) 1→(1,0) 2→(0,1) 3→(1,1).
    let corner = vec2<f32>(f32(vid & 1u), f32((vid >> 1u) & 1u));

    // Expand the quad outward by enough margin for the shadow to fade out.
    // 3σ covers ~99.7% of a gaussian, so blur * 3 is the visible reach.
    // If the shadow is also offset, the expansion has to cover the offset
    // direction too. Margin of 0 collapses to the original tight quad.
    let margin_blur = inst_shadow_blur * 3.0;
    let margin_offset = max(abs(inst_shadow_offset.x), abs(inst_shadow_offset.y));
    let margin = max(margin_blur, margin_offset);
    let margin_vec = vec2<f32>(margin, margin);

    let expanded_size = inst_size + margin_vec * 2.0;
    let expanded_pos = inst_pos - margin_vec;
    let pixel_pos = expanded_pos + corner * expanded_size;

    // Pixel space (origin top-left, y-down) → clip space (origin center, y-up).
    let clip_x = pixel_pos.x / globals.viewport.x * 2.0 - 1.0;
    let clip_y = 1.0 - pixel_pos.y / globals.viewport.y * 2.0;

    var out: VsOut;
    out.clip = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    // Local coord relative to the original rect's center, spanning the
    // expanded quad. SDFs are evaluated against the ORIGINAL half_size,
    // so points outside the original rect get positive distance.
    out.local = (corner - vec2<f32>(0.5, 0.5)) * expanded_size;
    out.half_size = inst_size * 0.5;
    out.radius = inst_radius;
    out.border_width = inst_border_width;
    out.fill = inst_fill;
    out.border = inst_border;
    out.shadow_color = inst_shadow_color;
    out.shadow_offset = inst_shadow_offset;
    out.shadow_blur = inst_shadow_blur;
    return out;
}

// Inigo Quilez's rounded-box SDF: negative inside, zero on the boundary,
// positive outside, expressed in the same pixel units as `p`.
fn sd_rounded_box(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let d = sd_rounded_box(in.local, in.half_size, in.radius);
    // fwidth(d) is the per-pixel rate of change of the SDF — exactly the
    // anti-aliasing width that produces a one-pixel-wide soft edge.
    let aa = max(fwidth(d), 1e-4);

    // Inner boundary sits border_width pixels inside the outer boundary.
    let inner_d = d + in.border_width;
    let outer_vis = 1.0 - smoothstep(-aa, aa, d);
    let inner_vis = 1.0 - smoothstep(-aa, aa, inner_d);
    let border_vis = outer_vis - inner_vis;

    let fa = inner_vis * in.fill.a;
    let ba = border_vis * in.border.a;

    // Shadow contribution: SDF evaluated at offset-shifted position, gaussian
    // falloff outside the offset shape, full opacity inside it, masked OFF
    // wherever the rect itself paints so the rect's own pixels don't get
    // doubled with shadow underneath.
    let p_shadow = in.local - in.shadow_offset;
    let d_shadow = sd_rounded_box(p_shadow, in.half_size, in.radius);
    let blur = max(in.shadow_blur, 1e-3);
    let shadow_inside = step(d_shadow, 0.0);
    let shadow_outside = exp(-d_shadow * d_shadow / (2.0 * blur * blur));
    let shadow_intensity = max(shadow_inside, shadow_outside);
    let outside_rect = smoothstep(-aa, aa, d);
    let sa = in.shadow_color.a * shadow_intensity * outside_rect;

    // Pre-multiplied alpha output. Three disjoint coverage regions (inner /
    // border / shadow-outside-rect) so summing pre-multiplied contributions
    // is correct under the (One, OneMinusSrcAlpha) blend.
    let rgb_pm =
        in.fill.rgb * fa
        + in.border.rgb * ba
        + in.shadow_color.rgb * sa;
    return vec4<f32>(rgb_pm, fa + ba + sa);
}
