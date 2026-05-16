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
//! init succeeds on the CI matrix; perf benchmarks land alongside S2.

#![doc(html_no_source)]

use std::sync::Arc;

use anyhow::{Context as _, Result};
use winit::window::Window;

/// GPU renderer owning the `wgpu` surface, device, queue, and configuration.
///
/// Constructed by the application after `winit` creates a window. Holds an
/// `Arc<Window>` so the surface's window outlives the renderer.
pub struct Renderer {
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Held to keep the surface's window alive for the renderer's lifetime.
    _window: Arc<Window>,
}

impl Renderer {
    /// Initialize the GPU surface, adapter, device, and queue for the given window.
    ///
    /// Blocks on the underlying async `wgpu` init via [`pollster`].
    ///
    /// # Errors
    ///
    /// Returns an error if the GPU instance cannot create a surface, no
    /// adapter is available, or the device request fails.
    pub fn new(window: Arc<Window>) -> Result<Self> {
        let size = window.inner_size();
        let width = size.width.max(1);
        let height = size.height.max(1);

        let instance = wgpu::Instance::new(&wgpu::InstanceDescriptor {
            backends: wgpu::Backends::PRIMARY,
            ..Default::default()
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

        let config = wgpu::SurfaceConfiguration {
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
            format,
            width,
            height,
            present_mode: caps.present_modes[0],
            alpha_mode: caps.alpha_modes[0],
            view_formats: vec![],
            desired_maximum_frame_latency: 2,
        };
        surface.configure(&device, &config);

        Ok(Self {
            surface,
            device,
            queue,
            config,
            _window: window,
        })
    }

    /// Resize the GPU surface to new window dimensions. Zero-sized inputs are ignored.
    pub fn resize(&mut self, width: u32, height: u32) {
        if width == 0 || height == 0 {
            return;
        }
        self.config.width = width;
        self.config.height = height;
        self.surface.configure(&self.device, &self.config);
    }

    /// Render one black clear-color frame.
    ///
    /// # Errors
    ///
    /// Returns an error if acquiring the next swapchain texture fails.
    pub fn render(&mut self) -> Result<()> {
        let frame = self
            .surface
            .get_current_texture()
            .context("get_current_texture failed")?;
        let view = frame
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("januas-ade-frame"),
            });
        {
            let _pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("januas-ade-clear"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                        store: wgpu::StoreOp::Store,
                    },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });
        }
        self.queue.submit(std::iter::once(encoder.finish()));
        frame.present();
        Ok(())
    }
}
