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
    buffer: Buffer,

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

        let mut buffer = Buffer::new(&mut font_system, Metrics::new(FONT_SIZE, LINE_HEIGHT));
        #[allow(clippy::cast_precision_loss)]
        let buf_w = width as f32;
        #[allow(clippy::cast_precision_loss)]
        let buf_h = height as f32;
        buffer.set_size(&mut font_system, Some(buf_w), Some(buf_h));
        buffer.set_text(
            &mut font_system,
            INITIAL_TEXT,
            &Attrs::new(),
            Shaping::Advanced,
            None,
        );
        buffer.shape_until_scroll(&mut font_system, false);

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
            buffer,
            text_dirty: true,
            frame_count: 0,
            window_start: Instant::now(),
            _window: window,
        })
    }

    /// Replace the buffer's text content. Marks the prepared vertices dirty.
    pub fn set_text(&mut self, content: &str) {
        self.buffer.set_text(
            &mut self.font_system,
            content,
            &Attrs::new(),
            Shaping::Advanced,
            None,
        );
        self.buffer.shape_until_scroll(&mut self.font_system, false);
        self.text_dirty = true;
    }

    /// Resize the GPU surface and text buffer. Zero-sized inputs are ignored.
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
        self.buffer
            .set_size(&mut self.font_system, Some(w), Some(h));
        self.text_dirty = true;
    }

    /// Render one frame: clear to black, draw the sample string, present.
    ///
    /// # Errors
    ///
    /// Returns an error if acquiring the next swapchain texture or preparing
    /// the text pipeline fails.
    pub fn render(&mut self) -> Result<()> {
        if self.text_dirty {
            self.viewport.update(
                &self.queue,
                Resolution {
                    width: self.config.width,
                    height: self.config.height,
                },
            );

            self.text_renderer
                .prepare(
                    &self.device,
                    &self.queue,
                    &mut self.font_system,
                    &mut self.atlas,
                    &self.viewport,
                    [TextArea {
                        buffer: &self.buffer,
                        left: 32.0,
                        top: 32.0,
                        scale: 1.0,
                        bounds: TextBounds {
                            left: 0,
                            top: 0,
                            #[allow(clippy::cast_possible_wrap)]
                            right: self.config.width as i32,
                            #[allow(clippy::cast_possible_wrap)]
                            bottom: self.config.height as i32,
                        },
                        default_color: GlyphColor::rgb(TEXT_R, TEXT_G, TEXT_B),
                        custom_glyphs: &[],
                    }],
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
