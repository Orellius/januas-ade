// rounded_rect.wgsl — SDF-based rounded rectangle primitive (S5.5a).
//
// One pipeline, instanced draws. Each instance is a (pos, size, radius,
// border_width, fill, border) tuple. Vertex stage stamps a unit quad at the
// instance's pixel rectangle and forwards the local pixel coordinate to the
// fragment stage. Fragment stage evaluates the rounded-box signed distance
// field, computes anti-aliased fill and border coverage, and outputs
// pre-multiplied alpha (pipeline blend = One, OneMinusSrcAlpha).

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
) -> VsOut {
    // Triangle-strip unit quad: vid 0→(0,0) 1→(1,0) 2→(0,1) 3→(1,1).
    let corner = vec2<f32>(f32(vid & 1u), f32((vid >> 1u) & 1u));
    let pixel_pos = inst_pos + corner * inst_size;

    // Pixel space (origin top-left, y-down) → clip space (origin center, y-up).
    let clip_x = pixel_pos.x / globals.viewport.x * 2.0 - 1.0;
    let clip_y = 1.0 - pixel_pos.y / globals.viewport.y * 2.0;

    var out: VsOut;
    out.clip = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    // Local pixel coord relative to rect center — SDF is symmetric, so this
    // is what the fragment stage feeds into sd_rounded_box.
    out.local = (corner - vec2<f32>(0.5, 0.5)) * inst_size;
    out.half_size = inst_size * 0.5;
    out.radius = inst_radius;
    out.border_width = inst_border_width;
    out.fill = inst_fill;
    out.border = inst_border;
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

    // Coverage of the outer shape (1 inside, 0 outside, smooth at edge).
    let outer_vis = 1.0 - smoothstep(-aa, aa, d);
    // Coverage of the inner (fill) region.
    let inner_vis = 1.0 - smoothstep(-aa, aa, inner_d);
    // Border region = outer minus inner.
    let border_vis = outer_vis - inner_vis;

    // Pre-multiplied alpha. Pipeline blend factors must be (One,
    // OneMinusSrcAlpha) for this to composite correctly.
    let fa = inner_vis * in.fill.a;
    let ba = border_vis * in.border.a;
    let rgb_pm = in.fill.rgb * fa + in.border.rgb * ba;
    return vec4<f32>(rgb_pm, fa + ba);
}
