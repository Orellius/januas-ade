//! Januas ADE — binary entry point.
//!
//! Purpose: own the `winit` event loop, the application state, and the wiring
//! that hands events to the renderer and the spawned shell.
//! Public surface: just `main`. The `App` type stays private to the binary.
//! Why this file (vs inlining elsewhere): renderer + pty + terminal stay
//! winit-free at their public APIs; this binary is the only place that needs
//! all three wired together.
//! Not responsibilities: GPU device management (renderer), VT parsing
//! (terminal), shell IO (pty).
//! Test strategy: covered by the end-to-end smoke test at slice close
//! (`cargo run` + type `ls` + observe directory listing in the window).

use std::sync::Arc;

use alacritty_terminal::grid::Dimensions as _;
use alacritty_terminal::index::{Column, Line};
use anyhow::{Context as _, Result};
use januas_pty::PtyShell;
use januas_renderer::Renderer;
use januas_terminal::Terminal;
use tracing::{info, warn};
use tracing_subscriber::EnvFilter;
use winit::{
    application::ApplicationHandler,
    event::{ElementState, KeyEvent, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    keyboard::{Key, NamedKey},
    window::{Window, WindowId},
};

const COLS: u16 = 100;
const ROWS: u16 = 32;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    info!("Januas ADE v0.0.0 — S3 boot");

    let event_loop = EventLoop::new().context("event_loop init failed")?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = App::default();
    event_loop
        .run_app(&mut app)
        .context("event_loop run failed")?;
    Ok(())
}

#[derive(Default)]
struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pty: Option<PtyShell>,
    terminal: Option<Terminal>,
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
        let renderer = match Renderer::new(Arc::clone(&window)) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = ?e, "renderer init failed");
                event_loop.exit();
                return;
            }
        };
        let pty = match PtyShell::spawn(COLS, ROWS) {
            Ok(p) => p,
            Err(e) => {
                warn!(error = ?e, "pty spawn failed");
                event_loop.exit();
                return;
            }
        };
        let terminal = Terminal::new(COLS, ROWS);
        info!("renderer + pty + terminal ready");

        self.renderer = Some(renderer);
        self.pty = Some(pty);
        self.terminal = Some(terminal);
        self.window = Some(window);
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
        let Some(terminal) = self.terminal.as_mut() else {
            return;
        };
        let Some(pty) = self.pty.as_ref() else {
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
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        logical_key,
                        text,
                        state: ElementState::Pressed,
                        ..
                    },
                ..
            } => {
                let bytes = key_to_bytes(&logical_key, text.as_deref());
                if !bytes.is_empty() {
                    if let Err(e) = pty.write(&bytes) {
                        warn!(error = ?e, "pty write failed");
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                let mut fed_anything = false;
                while let Ok(chunk) = pty.bytes.try_recv() {
                    terminal.feed(&chunk);
                    fed_anything = true;
                }
                if fed_anything {
                    renderer.set_text(&snapshot_grid(terminal));
                }
                if let Err(e) = renderer.render() {
                    // Rate-limit error logging via tracing::error — these are real
                    // failures (transient Occluded/Outdated/Timeout are swallowed
                    // inside Renderer::render and return Ok(())).
                    tracing::error!(error = ?e, "render failed (fatal)");
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}

/// Translate a winit key event into bytes for the shell.
///
/// v0.1 of S3 covers: printable text via `KeyEvent.text`, Enter (CR),
/// Backspace (DEL), Tab, Escape, and arrow keys (ANSI sequences).
fn key_to_bytes(key: &Key, text: Option<&str>) -> Vec<u8> {
    match key {
        Key::Named(NamedKey::Enter) => b"\r".to_vec(),
        Key::Named(NamedKey::Backspace) => b"\x7f".to_vec(),
        Key::Named(NamedKey::Tab) => b"\t".to_vec(),
        Key::Named(NamedKey::Escape) => b"\x1b".to_vec(),
        Key::Named(NamedKey::ArrowUp) => b"\x1b[A".to_vec(),
        Key::Named(NamedKey::ArrowDown) => b"\x1b[B".to_vec(),
        Key::Named(NamedKey::ArrowRight) => b"\x1b[C".to_vec(),
        Key::Named(NamedKey::ArrowLeft) => b"\x1b[D".to_vec(),
        _ => text.map(|t| t.as_bytes().to_vec()).unwrap_or_default(),
    }
}

/// Snapshot the visible grid into a single string. Each row becomes a line.
/// Trailing whitespace on each row is trimmed to keep `set_text` shaping cheap.
fn snapshot_grid(terminal: &Terminal) -> String {
    terminal.with_term(|term| {
        let grid = term.grid();
        let rows = grid.screen_lines();
        let cols = grid.columns();
        let mut out = String::with_capacity(rows * (cols + 1));
        for line in 0..rows {
            #[allow(
                clippy::cast_possible_wrap,
                clippy::cast_possible_truncation,
                reason = "line is bounded by screen_lines (32) — far below i32::MAX"
            )]
            let line_idx = Line(line as i32);
            let mut row = String::with_capacity(cols);
            for col in 0..cols {
                let cell = &grid[line_idx][Column(col)];
                row.push(cell.c);
            }
            let trimmed = row.trim_end();
            out.push_str(trimmed);
            out.push('\n');
        }
        out
    })
}
