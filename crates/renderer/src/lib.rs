//! Januas renderer — `wgpu` rendering pipeline.
//!
//! Purpose: own the GPU surface, device, queue, and frame loop that every
//! higher-level surface (terminal cells, wizard UI, sidebars) will draw into.
//! Public surface: [`Renderer::new`], [`Renderer::resize`], [`Renderer::render`].
//! Why this file (vs inlining in app): the GPU pipeline grows substantially in
//! S2 (glyph atlas) and S5 (multipane compositing); isolating it now keeps the
//! app crate as pure wiring.
//! Not responsibilities: window creation (lives in `app`), input handling
//! (lives in `app`), terminal grid model (lives in `terminal` crate at S3).
//! Test strategy: integration tests under `tests/` will assert headless wgpu
//! init succeeds on the CI matrix; perf benchmarks land alongside this slice.

#![doc(html_no_source)]

mod rect;
mod text;

pub use rect::{Rect, RectPipeline};
pub use text::TextRun;

use std::sync::Arc;
use std::time::Instant;

use anyhow::{Context as _, Result};
use glyphon::{
    Attrs, Buffer, Cache, Color as GlyphColor, FontSystem, Metrics, Resolution, Shaping,
    SwashCache, TextArea, TextAtlas, TextBounds, TextRenderer, Viewport,
};
use tracing::info;
use winit::window::Window;

/// Initial smoke string shown before any shell output arrives.
const INITIAL_TEXT: &str = "januas · spawning shell";
const FONT_SIZE: f32 = 16.0;
const LINE_HEIGHT: f32 = 20.0;
/// Default top-left inset for the legacy single-buffer [`Renderer::set_text`]
/// path. Matches the historical S2/S3 baseline.
const DEFAULT_TEXT_LEFT: f32 = 32.0;
const DEFAULT_TEXT_TOP: f32 = 32.0;
/// Rolling-window length (in frames) before the FPS counter logs.
const FPS_REPORT_INTERVAL_FRAMES: u32 = 600;

// Default-theme tokens — locked in `~/Desktop/Januas/docs/design-tokens.md`.
// Surface = #191d1e; text = cream #ddd2bb. Hex literals stay verbatim so the
// codegen path from `design-tokens.md` to a generated `tokens.rs` is obvious
// when it lands.
#[allow(clippy::cast_lossless, reason = "u8-to-f64 promotion is intentional")]
const SURFACE_R: f64 = 0x19_u8 as f64 / 255.0;
#[allow(clippy::cast_lossless, reason = "u8-to-f64 promotion is intentional")]
const SURFACE_G: f64 = 0x1d_u8 as f64 / 255.0;
#[allow(clippy::cast_lossless, reason = "u8-to-f64 promotion is intentional")]
const SURFACE_B: f64 = 0x1e_u8 as f64 / 255.0;
const TEXT_R: u8 = 0xdd;
const TEXT_G: u8 = 0xd2;
const TEXT_B: u8 = 0xbb;

/// GPU renderer owning the `wgpu` surface, device, queue, and text pipeline.
///
/// Constructed by the application after `winit` creates a window. Holds an
/// `Arc<Window>` so the surface's window outlives the renderer.
#[allow(
    clippy::struct_field_names,
    reason = "text_renderer is the glyphon name and renaming would obscure the binding"
)]
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,

    font_system: FontSystem,
    swash_cache: SwashCache,
    viewport: Viewport,
    atlas: TextAtlas,
    text_renderer: TextRenderer,
    /// Parallel arrays. `buffers[i]` is the shaped `cosmic-text` buffer for
    /// `runs[i]`; slots are reused across `set_text_runs` calls when the
    /// run count is stable.
    buffers: Vec<Buffer>,
    runs: Vec<TextRun>,

    rect_pipeline: RectPipeline,

    /// `true` when the prepared text vertex buffer is stale and `text_renderer.prepare`
    /// must be called before the next draw. Set on init and on resize.
    text_dirty: bool,

    frame_count: u32,
    window_start: Instant,

    /// Held to keep the surface's window alive for the renderer's lifetime.
    _window: Arc<Window>,
}

impl Renderer {
    /// Initialize the GPU surface, adapter, device, queue, and text pipeline.
    ///
    /// Blocks on the underlying async `wgpu` init via [`pollster`].
    ///
    /// # Errors
    ///
    /// Returns an error if the GPU instance cannot create a surface, no
    /// adapter is available, the device request fails, or the text pipeline
    /// fails to construct.
    #[allow(
        clippy::too_many_lines,
        reason = "linear surface + device + text + rect bring-up; splitting hurts readability more than it helps"
    )]
    pub fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            flags: wgpu::InstanceFlags::default(),
            memory_budget_thresholds: wgpu::MemoryBudgetThresholds::default(),
            backend_options: wgpu::BackendOptions::default(),
            display: None,
        });

        let surface = instance
            .create_surface(Arc::clone(&window))
            .context("wgpu create_surface failed")?;

        let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::HighPerformance,
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        }))
        .context("no compatible wgpu adapter")?;

        let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
            label: Some("januas-ade-device"),
            required_features: wgpu::Features::empty(),
            required_limits: wgpu::Limits::default(),
            memory_hints: wgpu::MemoryHints::default(),
            experimental_features: wgpu::ExperimentalFeatures::default(),
            trace: wgpu::Trace::Off,
        }))
        .context("wgpu device request failed")?;

        let caps = surface.get_capabilities(&adapter);
        let format = caps
            .formats
            .iter()
            .find(|f| f.is_srgb())
            .copied()
            .unwrap_or_else(|| caps.formats[0]);

        // Pick uncapped present mode for the 1000 fps perf gate. Production-style
        // vsync (FIFO) will return when an end-user-facing config layer arrives.
        let present_mode = if caps.present_modes.contains(&wgpu::PresentMode::Mailbox) {
            wgpu::PresentMode::Mailbox
        } else if caps.present_modes.contains(&wgpu::PresentMode::Immediate) {
            wgpu::PresentMode::Immediate
        } else {
            caps.present_modes[0]
        };
        info!(?present_mode, "surface present mode selected");

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode,
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let cache = Cache::new(&device);
        let viewport = Viewport::new(&device, &cache);
        let mut atlas = TextAtlas::new(&device, &queue, &cache, format);
        let text_renderer =
            TextRenderer::new(&mut atlas, &device, wgpu::MultisampleState::default(), None);

        #[allow(clippy::cast_precision_loss)]
        let buf_w = width as f32;
        #[allow(clippy::cast_precision_loss)]
        let buf_h = height as f32;

        let initial_run = TextRun {
            pos: [DEFAULT_TEXT_LEFT, DEFAULT_TEXT_TOP],
            content: INITIAL_TEXT.to_string(),
            font_size: FONT_SIZE,
            line_height: LINE_HEIGHT,
            color: [TEXT_R, TEXT_G, TEXT_B, 0xff],
        };
        let mut initial_buffer = Buffer::new(
            &mut font_system,
            Metrics::new(initial_run.font_size, initial_run.line_height),
        );
        initial_buffer.set_size(&mut font_system, Some(buf_w), Some(buf_h));
        initial_buffer.set_text(
            &mut font_system,
            &initial_run.content,
            &Attrs::new(),
            Shaping::Advanced,
            None,
        );
        initial_buffer.shape_until_scroll(&mut font_system, false);

        let rect_pipeline = RectPipeline::new(&device, format, [buf_w, buf_h]);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            font_system,
            swash_cache,
            viewport,
            atlas,
            text_renderer,
            buffers: vec![initial_buffer],
            runs: vec![initial_run],
            rect_pipeline,
            text_dirty: true,
            frame_count: 0,
            window_start: Instant::now(),
            _window: window,
        })
    }

    /// Replace the current rect scene. Pass `&[]` to clear. Cheap to call
    /// repeatedly with the same data, but callers should avoid that — the
    /// uniform/instance upload still costs a `queue.write_buffer`.
    pub fn set_rects(&mut self, rects: &[Rect]) {
        self.rect_pipeline
            .set_rects(&self.device, &self.queue, rects);
    }

    /// Replace the renderer's text content with a single span at the legacy
    /// `(32, 32)` inset. Wraps [`Self::set_text_runs`] for the shell-snapshot
    /// hot path; layout-driven callers should use `set_text_runs` directly.
    pub fn set_text(&mut self, content: &str) {
        // Fast path when we already have exactly one slot: avoid reallocating
        // the run Vec on every shell-snapshot frame.
        if self.buffers.len() == 1 && self.runs.len() == 1 {
            let buf = &mut self.buffers[0];
            buf.set_text(
                &mut self.font_system,
                content,
                &Attrs::new(),
                Shaping::Advanced,
                None,
            );
            buf.shape_until_scroll(&mut self.font_system, false);
            self.runs[0].content = content.to_string();
            self.text_dirty = true;
            return;
        }
        self.set_text_runs(&[TextRun {
            pos: [DEFAULT_TEXT_LEFT, DEFAULT_TEXT_TOP],
            content: content.to_string(),
            font_size: FONT_SIZE,
            line_height: LINE_HEIGHT,
            color: [TEXT_R, TEXT_G, TEXT_B, 0xff],
        }]);
    }

    /// Replace the renderer's text content with N positioned spans. Reuses
    /// existing `cosmic-text` buffer slots when the new count fits; grows or
    /// truncates the buffer Vec as needed.
    pub fn set_text_runs(&mut self, runs: &[TextRun]) {
        #[allow(clippy::cast_precision_loss)]
        let buf_w = self.config.width as f32;
        #[allow(clippy::cast_precision_loss)]
        let buf_h = self.config.height as f32;

        while self.buffers.len() < runs.len() {
            let placeholder = Metrics::new(FONT_SIZE, LINE_HEIGHT);
            self.buffers
                .push(Buffer::new(&mut self.font_system, placeholder));
        }
        self.buffers.truncate(runs.len());

        for (buf, run) in self.buffers.iter_mut().zip(runs.iter()) {
            buf.set_metrics(
                &mut self.font_system,
                Metrics::new(run.font_size, run.line_height),
            );
            buf.set_size(&mut self.font_system, Some(buf_w), Some(buf_h));
            buf.set_text(
                &mut self.font_system,
                &run.content,
                &Attrs::new(),
                Shaping::Advanced,
                None,
            );
            buf.shape_until_scroll(&mut self.font_system, false);
        }

        self.runs = runs.to_vec();
        self.text_dirty = true;
    }

    /// Resize the GPU surface, every text buffer, and the rect-pipeline
    /// viewport uniform. Zero-sized inputs are ignored.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
        #[allow(clippy::cast_precision_loss)]
        let w = width as f32;
        #[allow(clippy::cast_precision_loss)]
        let h = height as f32;
        for buf in &mut self.buffers {
            buf.set_size(&mut self.font_system, Some(w), Some(h));
        }
        self.rect_pipeline.resize(&self.queue, [w, h]);
        self.text_dirty = true;
    }

    /// Render one frame: clear to black, draw the sample string, present.
    ///
    /// # Errors
    ///
    /// Returns an error if acquiring the next swapchain texture or preparing
    /// the text pipeline fails.
    #[allow(
        clippy::too_many_lines,
        reason = "linear frame loop: prepare → acquire → encode → submit → present → trim → fps. Splitting fragments the dataflow."
    )]
    pub fn render(&mut self) -> Result<()> {
        if self.text_dirty {
            self.viewport.update(
                &self.queue,
                Resolution {
                    width: self.config.width,
                    height: self.config.height,
                },
            );

            #[allow(clippy::cast_possible_wrap)]
            let bounds = TextBounds {
                left: 0,
                top: 0,
                right: self.config.width as i32,
                bottom: self.config.height as i32,
            };
            let areas: Vec<TextArea<'_>> = self
                .buffers
                .iter()
                .zip(self.runs.iter())
                .map(|(buffer, run)| TextArea {
                    buffer,
                    left: run.pos[0],
                    top: run.pos[1],
                    scale: 1.0,
                    bounds,
                    default_color: GlyphColor::rgba(
                        run.color[0],
                        run.color[1],
                        run.color[2],
                        run.color[3],
                    ),
                    custom_glyphs: &[],
                })
                .collect();
            self.text_renderer
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut self.font_system,
                    &mut self.atlas,
                    &self.viewport,
                    areas,
                    &mut self.swash_cache,
                )
                .context("text prepare failed")?;

            self.text_dirty = false;
        }

        let frame = match self.surface.get_current_texture() {
            wgpu::CurrentSurfaceTexture::Success(t)
            | wgpu::CurrentSurfaceTexture::Suboptimal(t) => t,
            wgpu::CurrentSurfaceTexture::Occluded
            | wgpu::CurrentSurfaceTexture::Outdated
            | wgpu::CurrentSurfaceTexture::Timeout => {
                // Transient: window not visible, configuration stale, or compositor
                // told us to wait. Skip this frame silently; the event loop's next
                // RedrawRequested will re-attempt. Logging here at ANY level floods
                // the log because RedrawRequested fires every loop iteration.
                return Ok(());
            }
            other => anyhow::bail!("get_current_texture: {other:?}"),
        };
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("januas-ade-frame"),
            });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("januas-ade-clear-and-text"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color {
                            r: SURFACE_R,
                            g: SURFACE_G,
                            b: SURFACE_B,
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
            self.rect_pipeline.draw(&mut pass);
            self.text_renderer
                .render(&self.atlas, &self.viewport, &mut pass)
                .context("text render failed")?;
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();

        self.atlas.trim();

        self.frame_count += 1;
        if self.frame_count >= FPS_REPORT_INTERVAL_FRAMES {
            let elapsed = self.window_start.elapsed();
            #[allow(clippy::cast_precision_loss)]
            let fps = f64::from(self.frame_count) / elapsed.as_secs_f64();
            info!(
                frames = self.frame_count,
                secs = elapsed.as_secs_f64(),
                fps = fps,
                "fps sample"
            );
            self.frame_count = 0;
            self.window_start = Instant::now();
        }

        Ok(())
    }
}
