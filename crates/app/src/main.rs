//! Januas ADE — binary entry point.
//!
//! Purpose: own the `winit` event loop, the application state, and the wiring
//! that hands events to the renderer. v0.0.0 + S1: opens a window and renders
//! black clear-color frames; richer surfaces land in later slices.
//! Public surface: just `main`. The `App` type stays private to the binary.
//! Why this file (vs inlining in `renderer`): the renderer crate owns the GPU
//! pipeline and stays winit-free at the public API; this binary is the only
//! place that needs windowing dependency.
//! Not responsibilities: GPU device management (renderer), terminal grid
//! model (future `terminal` crate), input semantics beyond close/resize.
//! Test strategy: covered by the end-to-end smoke test at slice close
//! (`cargo run` + observe window + close it).

use std::sync::Arc;

use anyhow::{Context as _, Result};
use januas_renderer::Renderer;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use winit::{
    application::ApplicationHandler,
    event::WindowEvent,
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Januas ADE v0.0.0 — S1 boot");

    let event_loop = EventLoop::new().context("event_loop init failed")?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    event_loop
        .run_app(&mut app)
        .context("event_loop run failed")?;
    Ok(())
}

/// Application state: window + renderer, both lazy-initialized on `resumed`.
#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Januas ADE")
            .with_inner_size(winit::dpi::LogicalSize::new(1280, 800));
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                warn!(error = ?e, "window creation failed");
                event_loop.exit();
                return;
            }
        };
        match Renderer::new(Arc::clone(&window)) {
            Ok(r) => {
                info!("renderer ready");
                self.renderer = Some(r);
                self.window = Some(window);
            }
            Err(e) => {
                warn!(error = ?e, "renderer init failed");
                event_loop.exit();
            }
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        match event {
            WindowEvent::CloseRequested => {
                info!("close requested, exiting");
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                renderer.resize(size.width, size.height);
            }
            WindowEvent::RedrawRequested => {
                if let Err(e) = renderer.render() {
                    warn!(error = ?e, "render failed");
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
