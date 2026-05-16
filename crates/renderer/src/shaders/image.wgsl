// image.wgsl — sampled-texture primitive with rounded-rect clipping (S5.5d).
//
// One pipeline, one draw call per visible image. Each instance is a
// (pos, size, radius, tint, layer) tuple. Vertex stage stamps a tight unit
// quad — no shadow halo for now — and forwards the local coord. Fragment
// stage samples the texture at the local UV, multiplies by tint, then masks
// the result against a rounded-box SDF so the corner radius (e.g. the logo
// frame's 14px) is anti-aliased without a separate mask pass. Output is
// pre-multiplied alpha to match the rect-pipeline blend setup
// (One, OneMinusSrcAlpha).

struct Globals {
    viewport: vec2<f32>,
};

@group(0) @binding(0) var<uniform> globals: Globals;
@group(1) @binding(0) var image_tex: texture_2d<f32>;
@group(1) @binding(1) var image_sampler: sampler;

struct VsOut {
    @builtin(position) clip: vec4<f32>,
    @location(0) local: vec2<f32>,
    @location(1) uv: vec2<f32>,
    @location(2) half_size: vec2<f32>,
    @location(3) radius: f32,
    @location(4) tint: vec4<f32>,
};

@vertex
fn vs_main(
    @builtin(vertex_index) vid: u32,
    @location(0) inst_pos: vec2<f32>,
    @location(1) inst_size: vec2<f32>,
    @location(2) inst_radius: f32,
    @location(3) inst_tint: vec4<f32>,
) -> VsOut {
    let corner = vec2<f32>(f32(vid & 1u), f32((vid >> 1u) & 1u));
    let pixel_pos = inst_pos + corner * inst_size;

    let clip_x = pixel_pos.x / globals.viewport.x * 2.0 - 1.0;
    let clip_y = 1.0 - pixel_pos.y / globals.viewport.y * 2.0;

    var out: VsOut;
    out.clip = vec4<f32>(clip_x, clip_y, 0.0, 1.0);
    out.local = (corner - vec2<f32>(0.5, 0.5)) * inst_size;
    out.uv = corner;
    out.half_size = inst_size * 0.5;
    out.radius = inst_radius;
    out.tint = inst_tint;
    return out;
}

fn sd_rounded_box(p: vec2<f32>, half_size: vec2<f32>, r: f32) -> f32 {
    let q = abs(p) - half_size + vec2<f32>(r, r);
    return min(max(q.x, q.y), 0.0) + length(max(q, vec2<f32>(0.0, 0.0))) - r;
}

@fragment
fn fs_main(in: VsOut) -> @location(0) vec4<f32> {
    let texel = textureSample(image_tex, image_sampler, in.uv);
    let tinted = texel * in.tint;

    let d = sd_rounded_box(in.local, in.half_size, in.radius);
    let aa = max(fwidth(d), 1e-4);
    let mask = 1.0 - smoothstep(-aa, aa, d);

    let a = tinted.a * mask;
    return vec4<f32>(tinted.rgb * a, a);
}
