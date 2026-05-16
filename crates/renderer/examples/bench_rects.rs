//! S5.5a perf gate — headless bench of the rounded-rect pipeline.
//!
//! Renders 100 rounded rects into an offscreen texture per frame and waits for
//! GPU completion before counting the frame. No window, no glyphon, no
//! `Renderer`. This isolates [`RectPipeline`] from macOS Core Animation
//! vsync, which caps window-surface fps near the display refresh rate
//! regardless of the chosen present mode. Slice gate: ≥1000 fps on 100 rects
//! at 1280×800.
//!
//! Run with `cargo run --release --example bench_rects`. Prints `frames`,
//! `secs`, and `fps`; exits 0.

use std::time::Instant;

use anyhow::{Context as _, Result};
use januas_renderer::{NO_GRADIENT, Rect, RectPipeline};
use tracing::info;
use tracing_subscriber::EnvFilter;

const FRAMES: u32 = 5_000;
const WARMUP: u32 = 200;
const TEX_W: u32 = 1280;
const TEX_H: u32 = 800;

const GRID_COLS: usize = 10;
const GRID_ROWS: usize = 10;
const TILE_W: f32 = 100.0;
const TILE_H: f32 = 60.0;
const TILE_GAP: f32 = 24.0;
const TILE_RADIUS: f32 = 8.0;
const TILE_BORDER: f32 = 1.0;
const ORIGIN_X: f32 = 32.0;
const ORIGIN_Y: f32 = 32.0;

// Locked Januas tokens — see `~/Desktop/Januas/docs/design-tokens.md`.
const ACCENT_SRGB: [u8; 3] = [0x6b, 0xb8, 0xa8];
const BORDER_SRGB: [u8; 3] = [0xdd, 0xd2, 0xbb];

fn srgb_u8_to_linear(c: u8) -> f32 {
    #[allow(clippy::cast_lossless, reason = "u8-to-f32 promotion is intentional")]
    let s = c as f32 / 255.0;
    if s <= 0.040_45 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn srgb_rgba_to_linear(rgb: [u8; 3], alpha: f32) -> [f32; 4] {
    [
        srgb_u8_to_linear(rgb[0]),
        srgb_u8_to_linear(rgb[1]),
        srgb_u8_to_linear(rgb[2]),
        alpha,
    ]
}

#[allow(
    clippy::cast_precision_loss,
    reason = "row/col counts are tiny; precision loss is irrelevant"
)]
fn build_grid() -> Vec<Rect> {
    let fill = srgb_rgba_to_linear(ACCENT_SRGB, 0.85);
    let border = srgb_rgba_to_linear(BORDER_SRGB, 0.35);
    let mut rects = Vec::with_capacity(GRID_COLS * GRID_ROWS);
    for r in 0..GRID_ROWS {
        for c in 0..GRID_COLS {
            let x = (c as f32).mul_add(TILE_W + TILE_GAP, ORIGIN_X);
            let y = (r as f32).mul_add(TILE_H + TILE_GAP, ORIGIN_Y);
            rects.push(Rect {
                pos: [x, y],
                size: [TILE_W, TILE_H],
                radii: [TILE_RADIUS; 4],
                border_width: TILE_BORDER,
                fill,
                border,
                shadow_color: [0.0; 4],
                shadow_offset: [0.0; 2],
                shadow_blur: 0.0,
                gradient_index: NO_GRADIENT,
            });
        }
    }
    rects
}

fn encode_frame(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &wgpu::TextureView,
    pipeline: &RectPipeline,
) {
    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("bench-frame"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("bench-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: 0.0098,
                        g: 0.0122,
                        b: 0.0131,
                        a: 1.0,
                    }),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            multiview_mask: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });
        pipeline.draw(&mut pass);
    }
    queue.submit(std::iter::once(encoder.finish()));
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!(
        target_fps = 1000,
        rects = GRID_COLS * GRID_ROWS,
        "S5.5a headless bench — RectPipeline only"
    );

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::PRIMARY,
        flags: wgpu::InstanceFlags::default(),
        memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
        backend_options: wgpu::BackendOptions::default(),
        display: None,
    });

    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::HighPerformance,
        compatible_surface: None,
        force_fallback_adapter: false,
    }))
    .context("no compatible adapter")?;

    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("bench-device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }))
    .context("device request failed")?;

    let format = wgpu::TextureFormat::Bgra8UnormSrgb;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("bench-target"),
        size: wgpu::Extent3d {
            width: TEX_W,
            height: TEX_H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    #[allow(clippy::cast_precision_loss)]
    let vp = [TEX_W as f32, TEX_H as f32];
    let mut pipeline = RectPipeline::new(&device, format, vp);
    pipeline.set_rects(&device, &queue, &build_grid());

    // Warm up driver / shader cache so the first measured frame is not
    // dominated by one-time JIT or descriptor-pool growth.
    for _ in 0..WARMUP {
        encode_frame(&device, &queue, &view, &pipeline);
    }
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("warmup poll")?;

    let start = Instant::now();
    for _ in 0..FRAMES {
        encode_frame(&device, &queue, &view, &pipeline);
    }
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("measure poll")?;
    let elapsed = start.elapsed();
    let secs = elapsed.as_secs_f64();
    let fps = f64::from(FRAMES) / secs;
    info!(frames = FRAMES, secs = secs, fps = fps, "bench complete");
    println!("bench_rects: {FRAMES} frames in {secs:.3}s → {fps:.0} fps (target ≥1000)");
    Ok(())
}
