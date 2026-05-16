//! S5.5h visual smoke — per-corner radii + radial gradient.
//!
//! Renders two reference rects to an offscreen texture and writes a PNG to
//! `target/bench_portal.png` for eyeball comparison against the mockup at
//! `~/Desktop/claude-html/januas-portals-20260516-2310.html`:
//!
//! - **Left rect** — portal-shape `[TL=100, TR=100, BR=6, BL=6]` against a
//!   teal fill with a thin cream border. Verifies the per-corner SDF.
//! - **Right rect** — fully-rounded teal-tinted body with the mockup's
//!   radial-gradient halo painted over it. Verifies the gradient fragment
//!   path and the ellipse-farthest-corner formulation.
//!
//! Run with `cargo run --release --example bench_portal -p januas-renderer`.
//! Exits 0 after writing the PNG; the path is logged.

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::{Context as _, Result};
use januas_renderer::{GradientStop, NO_GRADIENT, RadialGradient, Rect, RectPipeline};
use tracing::info;
use tracing_subscriber::EnvFilter;

const TEX_W: u32 = 640;
const TEX_H: u32 = 360;

// Surface clear color — locked Januas `#191d1e` in linear space (matches the
// renderer's main path).
const CLEAR_R: f64 = 0.009_961_1;
const CLEAR_G: f64 = 0.012_182_8;
const CLEAR_B: f64 = 0.013_120_3;

// Locked Januas tokens.
const ACCENT_LINEAR: [f32; 3] = [0.143_30, 0.466_47, 0.391_57]; // #6bb8a8 → linear
const CREAM_LINEAR: [f32; 3] = [0.722_38, 0.638_92, 0.482_28]; // #ddd2bb → linear

const PORTAL_W: f32 = 180.0;
const PORTAL_H: f32 = 280.0;
const PORTAL_X: f32 = 80.0;
const PORTAL_Y: f32 = 40.0;

const GRADIENT_RECT_X: f32 = 360.0;
const GRADIENT_RECT_Y: f32 = 40.0;

/// Mockup's halo gradient — five stops at the locked alpha curve.
/// Stops `[0%, 28%, 58%, 85%, 100%]` mirror
/// `~/Desktop/claude-html/januas-portals-20260516-2310.html` §`.portal-light`.
fn portal_halo() -> RadialGradient {
    let teal = ACCENT_LINEAR;
    let stop = |offset: f32, alpha: f32| GradientStop {
        offset,
        color: [teal[0], teal[1], teal[2], alpha],
    };
    RadialGradient {
        center: [0.5, 0.58],
        stops: [
            stop(0.00, 0.18),
            stop(0.28, 0.11),
            stop(0.58, 0.055),
            stop(0.85, 0.022),
            stop(1.00, 0.012),
        ],
    }
}

fn build_scene() -> (Vec<Rect>, Vec<RadialGradient>) {
    let halo = portal_halo();
    let gradients = vec![halo];

    let cream_border = [CREAM_LINEAR[0], CREAM_LINEAR[1], CREAM_LINEAR[2], 0.35];
    let panel_fill = [
        ACCENT_LINEAR[0] * 0.18,
        ACCENT_LINEAR[1] * 0.18,
        ACCENT_LINEAR[2] * 0.18,
        0.40,
    ];

    let portal_shape = Rect {
        pos: [PORTAL_X, PORTAL_Y],
        size: [PORTAL_W, PORTAL_H],
        radii: [100.0, 100.0, 6.0, 6.0],
        border_width: 1.0,
        fill: panel_fill,
        border: cream_border,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        shadow_blur: 0.0,
        gradient_index: NO_GRADIENT,
    };

    let gradient_rect = Rect {
        pos: [GRADIENT_RECT_X, GRADIENT_RECT_Y],
        size: [PORTAL_W, PORTAL_H],
        radii: [100.0, 100.0, 6.0, 6.0],
        border_width: 1.0,
        fill: panel_fill,
        border: cream_border,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        shadow_blur: 0.0,
        gradient_index: 0,
    };

    (vec![portal_shape, gradient_rect], gradients)
}

#[allow(
    clippy::too_many_lines,
    reason = "linear wgpu init → encode → readback → PNG; the dataflow IS the point"
)]
fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("S5.5h visual smoke — per-corner radii + radial gradient");

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
        label: Some("portal-smoke-device"),
        required_features: wgpu::Features::empty(),
        required_limits: wgpu::Limits::default(),
        memory_hints: wgpu::MemoryHints::default(),
        experimental_features: wgpu::ExperimentalFeatures::default(),
        trace: wgpu::Trace::Off,
    }))
    .context("device request failed")?;

    let format = wgpu::TextureFormat::Rgba8Unorm;
    let target = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("portal-target"),
        size: wgpu::Extent3d {
            width: TEX_W,
            height: TEX_H,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let view = target.create_view(&wgpu::TextureViewDescriptor::default());

    #[allow(clippy::cast_precision_loss)]
    let vp = [TEX_W as f32, TEX_H as f32];
    let mut pipeline = RectPipeline::new(&device, format, vp);
    let (rects, gradients) = build_scene();
    pipeline.set_gradients(&device, &queue, &gradients);
    pipeline.set_rects(&device, &queue, &rects);

    let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
        label: Some("portal-encoder"),
    });
    {
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("portal-pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view: &view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color {
                        r: CLEAR_R,
                        g: CLEAR_G,
                        b: CLEAR_B,
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

    // wgpu requires bytes_per_row to be a multiple of COPY_BYTES_PER_ROW_ALIGNMENT (256).
    let unpadded_bytes_per_row: u32 = TEX_W * 4;
    let align = wgpu::COPY_BYTES_PER_ROW_ALIGNMENT;
    let padded_bytes_per_row = unpadded_bytes_per_row.div_ceil(align) * align;
    let buffer_size = u64::from(padded_bytes_per_row) * u64::from(TEX_H);

    let readback = device.create_buffer(&wgpu::BufferDescriptor {
        label: Some("portal-readback"),
        size: buffer_size,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });

    encoder.copy_texture_to_buffer(
        wgpu::TexelCopyTextureInfo {
            texture: &target,
            mip_level: 0,
            origin: wgpu::Origin3d::ZERO,
            aspect: wgpu::TextureAspect::All,
        },
        wgpu::TexelCopyBufferInfo {
            buffer: &readback,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(padded_bytes_per_row),
                rows_per_image: Some(TEX_H),
            },
        },
        wgpu::Extent3d {
            width: TEX_W,
            height: TEX_H,
            depth_or_array_layers: 1,
        },
    );
    queue.submit(std::iter::once(encoder.finish()));

    let slice = readback.slice(..);
    slice.map_async(wgpu::MapMode::Read, |res| {
        if let Err(e) = res {
            eprintln!("readback map_async error: {e:?}");
        }
    });
    device
        .poll(wgpu::PollType::wait_indefinitely())
        .context("readback poll")?;

    let mapped = slice.get_mapped_range();
    let mut packed = Vec::with_capacity((unpadded_bytes_per_row * TEX_H) as usize);
    for y in 0..TEX_H {
        let row_start = (y * padded_bytes_per_row) as usize;
        let row_end = row_start + unpadded_bytes_per_row as usize;
        packed.extend_from_slice(&mapped[row_start..row_end]);
    }
    drop(mapped);
    readback.unmap();

    let target_dir = std::env::current_dir().context("cwd")?.join("target");
    std::fs::create_dir_all(&target_dir).context("create target dir")?;
    let out_path: PathBuf = target_dir.join("bench_portal.png");
    let file = File::create(&out_path).context("create png file")?;
    let w = BufWriter::new(file);
    let mut encoder = png::Encoder::new(w, TEX_W, TEX_H);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    let mut writer = encoder.write_header().context("png header")?;
    writer.write_image_data(&packed).context("png data")?;

    println!("bench_portal: wrote {}", out_path.display());
    Ok(())
}
