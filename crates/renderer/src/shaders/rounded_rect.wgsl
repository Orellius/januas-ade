// rounded_rect.wgsl — SDF-based rounded rectangle primitive.
//
// Instanced draws. Each instance is (pos, size, radii[4], border_width,
// fill, border, shadow_color, shadow_offset, shadow_blur, gradient_index).
// Vertex stage stamps a quad expanded outward by the shadow's reach so the
// fragment stage can paint the shadow halo past the rect's own bounds.
// Fragment stage:
//   1. Evaluates the rounded-box signed distance field with per-corner radii
//      for fill + border.
//   2. Optionally mixes an ellipse-farthest-corner radial gradient into the
//      fill before the border lands on top (when gradient_index >= 0).
//   3. Evaluates the SDF again at shadow_offset for the shadow's gaussian
//      falloff.
//   4. Composites all three regions with pre-multiplied alpha (pipeline
//      blend = One, OneMinusSrcAlpha).
//
// Per-corner SDF caveat: the IQ formulation picks ONE radius per fragment
// (the corner whose quadrant the fragment lives in), so the SDF is
// discontinuous across the p.x == 0 and p.y == 0 axes whenever the radii on
// either side of that axis differ. The discontinuity is harmless when those
// axes sit well inside the filled region — visually, the seam is buried in
// pixels where sd << 0 and never crosses the visible iso-line (sd ≈ 0).
// Concretely: for any rect where the visible curvature ends a comfortable
// distance from both center axes, the seam stays invisible. The portal
// shape [TL=100, TR=100, BR=6, BL=6] on a 180×280 rect satisfies this. If a
// future shape pushes a corner's curvature into either center axis (very
// short rect with very large radii, or radii larger than half_size on the
// shorter axis), replace with a min-of-four-corner SDF.

struct Globals {
    viewport: vec2<f32>,
};

struct Gradient {
    center: vec2<f32>,
    _pad0: vec2<f32>,
    // First four stop offsets; the fifth is implicit at 1.0.
    offsets: vec4<f32>,
    color0: vec4<f32>,
    color1: vec4<f32>,
    color2: vec4<f32>,
    color3: vec4<f32>,
    color4: vec4<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(0) @binding(1) var<storage, read> gradients: array<Gradient>;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) half_size: vec2<f32>,
    @location(2) radii: vec4<f32>,
    @location(3) border_width: f32,
    @location(4) fill: vec4<f32>,
    @location(5) border: vec4<f32>,
    @location(6) shadow_color: vec4<f32>,
    @location(7) shadow_offset: vec2<f32>,
    @location(8) shadow_blur: f32,
    @location(9) @interpolate(flat) gradient_index: i32,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @location(0) inst_pos: vec2<f32>,
    @location(1) inst_size: vec2<f32>,
    @location(2) inst_radii: vec4<f32>,
    @location(3) inst_border_width: f32,
    @location(4) inst_fill: vec4<f32>,
    @location(5) inst_border: vec4<f32>,
    @location(6) inst_shadow_color: vec4<f32>,
    @location(7) inst_shadow_offset: vec2<f32>,
    @location(8) inst_shadow_blur: f32,
    @location(9) inst_gradient_index: i32,
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
    out.radii = inst_radii;
    out.border_width = inst_border_width;
    out.fill = inst_fill;
    out.border = inst_border;
    out.shadow_color = inst_shadow_color;
    out.shadow_offset = inst_shadow_offset;
    out.shadow_blur = inst_shadow_blur;
    out.gradient_index = inst_gradient_index;
    return out;
}

// Iñigo Quilez's per-corner rounded-box SDF: negative inside, zero on the
// boundary, positive outside. `radii` is ordered [TL, TR, BR, BL]; renderer
// uses y-down screen coords, so local.y > 0 means below center.
fn sd_rounded_box(p: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let r_top = select(radii.x, radii.y, p.x > 0.0);
    let r_bot = select(radii.w, radii.z, p.x > 0.0);
    let r = select(r_top, r_bot, p.y > 0.0);
    let q = abs(p) - half_size + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

// Sample a radial gradient at `local` relative to the rect's center.
// Ellipse-farthest-corner formulation: gradient axis extends along whichever
// half-extent of the rect is longer, so a wide banner and a tall portal
// both bound their stops at the same normalized distance.
fn sample_gradient(idx: i32, local: vec2<f32>, half_size: vec2<f32>) -> vec4<f32> {
    let g = gradients[idx];
    // Convert normalized 0..1 center to rect-local coords (origin at center).
    let center_local = (g.center - vec2<f32>(0.5, 0.5)) * (half_size * 2.0);
    let axis = max(half_size.x, half_size.y);
    let d = length(local - center_local) / max(axis, 1e-3);

    let o0 = g.offsets.x;
    let o1 = g.offsets.y;
    let o2 = g.offsets.z;
    let o3 = g.offsets.w;
    // Fifth offset is implicit at 1.0.

    var col = g.color0;
    col = mix(col, g.color1, smoothstep(o0, o1, d));
    col = mix(col, g.color2, smoothstep(o1, o2, d));
    col = mix(col, g.color3, smoothstep(o2, o3, d));
    col = mix(col, g.color4, smoothstep(o3, 1.0, d));
    return col;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let d = sd_rounded_box(in.local, in.half_size, in.radii);
    // fwidth(d) is the per-pixel rate of change of the SDF — exactly the
    // anti-aliasing width that produces a one-pixel-wide soft edge.
    let aa = max(fwidth(d), 1e-4);

    // Inner boundary sits border_width pixels inside the outer boundary.
    let inner_d = d + in.border_width;
    let outer_vis = 1.0 - smoothstep(-aa, aa, d);
    let inner_vis = 1.0 - smoothstep(-aa, aa, inner_d);
    let border_vis = outer_vis - inner_vis;

    // Fill color: either the flat fill, or the flat fill composited with
    // the radial gradient (gradient alpha controls the mix, so callers can
    // paint a halo over a transparent body or over a solid body).
    var fill_rgba = in.fill;
    if (in.gradient_index >= 0) {
        let g = sample_gradient(in.gradient_index, in.local, in.half_size);
        // Source-over: gradient over fill, both already in linear space.
        let a = g.a;
        fill_rgba = vec4<f32>(
            mix(in.fill.rgb, g.rgb, a),
            in.fill.a + g.a * (1.0 - in.fill.a),
        );
    }

    let fa = inner_vis * fill_rgba.a;
    let ba = border_vis * in.border.a;

    // Shadow contribution: SDF evaluated at offset-shifted position, gaussian
    // falloff outside the offset shape, full opacity inside it, masked OFF
    // wherever the rect itself paints so the rect's own pixels don't get
    // doubled with shadow underneath.
    let p_shadow = in.local - in.shadow_offset;
    let d_shadow = sd_rounded_box(p_shadow, in.half_size, in.radii);
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
        fill_rgba.rgb * fa
        + in.border.rgb * ba
        + in.shadow_color.rgb * sa;
    return vec4<f32>(rgb_pm, fa + ba + sa);
}
