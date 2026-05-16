//! Minimal interactive smoke for the `ui` crate's layout + pointer primitives.
//!
//! Not the real home screen. Not a v6 mockup transcription. Real home-scene
//! composition lands at S5.5d; real product integration (home wired into the
//! `januas-ade` binary, replacing the boot-to-shell path) lands at S5.5e.
//!
//! This example exists only to prove the primitives compose end-to-end:
//! `Stack` + `Node` flow through `layout()` into the renderer, and
//! `PointerState` + `hit_test` toggle styles on a small set of `NodeId`s.
//!
//! Run: `cargo run --release --example home_compose -p januas-ui`.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use januas_renderer::Renderer;
use januas_ui::{
    CrossAlign, EdgeInsets, HitZone, Node, NodeId, NodeKind, PointerState, RectStyle, Stack,
    TextStyle, estimate_text_size, layout,
};
use tracing::warn;
use tracing_subscriber::EnvFilter;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalPosition,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

const DEFAULT_LOGICAL_W: f64 = 1280.0;
const DEFAULT_LOGICAL_H: f64 = 800.0;

// Three tiles in a row prove: layout centering, pointer hit-test, click
// drops the selection rail. Sizes are deliberately generous so visual
// asymmetry is obvious if the centering math breaks.
// Portal aspect — taller than wide, riffing on the Januas / Janus
// gateway metaphor. ~1.45 height-to-width feels doorway-ish without
// going pill-tall.
const TILE_W: f32 = 220.0;
const TILE_H: f32 = 320.0;
const TILE_GAP: f32 = 24.0;
// Squircle-feel radius — ~20% of the tile's shorter axis (iOS app-icon
// territory). Token-locked 8px stays the cap for functional surfaces
// (buttons, chips); cards earn their own larger radius.
const TILE_RADIUS: f32 = 28.0;

// Logo placeholder beside the wordmark — accent-filled squircle, mirrors
// the brand-mark slot. Real PNG at `~/Desktop/Januas/products/januas-ade/
// assets/januas-ade-logo.png` lands when texture support arrives (S5.5d).
const LOGO_SIZE: f32 = 36.0;
const LOGO_RADIUS: f32 = 10.0;
const WORDMARK_SIZE: f32 = 32.0;
const HEADER_GAP: f32 = 12.0;

const TILE_A: NodeId = 1;
const TILE_B: NodeId = 2;
const TILE_C: NodeId = 3;

fn srgb_u8_to_linear(c: u8) -> f32 {
    #[allow(clippy::cast_lossless, reason = "u8-to-f32 promotion is intentional")]
    let s = c as f32 / 255.0;
    if s <= 0.040_45 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

fn linear(rgb: [u8; 3], alpha: f32) -> [f32; 4] {
    [
        srgb_u8_to_linear(rgb[0]),
        srgb_u8_to_linear(rgb[1]),
        srgb_u8_to_linear(rgb[2]),
        alpha,
    ]
}

// Locked Januas tokens — see `~/Desktop/Januas/docs/design-tokens.md`.
const SURFACE_BASE: [u8; 3] = [0x19, 0x1d, 0x1e];
const SURFACE_1: [u8; 3] = [0x1e, 0x23, 0x24];
const SURFACE_2: [u8; 3] = [0x23, 0x2a, 0x2b];
const ACCENT: [u8; 3] = [0x6b, 0xb8, 0xa8];
const CREAM: [u8; 3] = [0xdd, 0xd2, 0xbb];
const CREAM_DIM: [u8; 3] = [0x97, 0x90, 0x7f];
const CREAM_FAINT: [u8; 3] = [0x58, 0x52, 0x49];

// Shadow disabled — fill differentiation + dim border do all the work.
const TILE_SHADOW_OFFSET: [f32; 2] = [0.0, 0.0];
const TILE_SHADOW_BLUR: f32 = 0.0;
const TILE_SHADOW_ALPHA: f32 = 0.0;
const TILE_BORDER_ALPHA: f32 = 0.16;

// Shadow color: Discord's near-black `#040405` sRGB → linear (≈ all zeros).
// On our dark surface this darkens slightly rather than the cream-tinted
// lightening that was reading as a glow.
const SHADOW_RGB: [u8; 3] = [0x04, 0x04, 0x05];

fn tile_style(rgb: [u8; 3]) -> RectStyle {
    RectStyle {
        fill: linear(rgb, 1.0),
        border: linear(CREAM_FAINT, TILE_BORDER_ALPHA),
        border_width: 1.0,
        radius: TILE_RADIUS,
        shadow_color: linear(SHADOW_RGB, TILE_SHADOW_ALPHA),
        shadow_offset: TILE_SHADOW_OFFSET,
        shadow_blur: TILE_SHADOW_BLUR,
    }
}

fn selected_style() -> RectStyle {
    RectStyle {
        fill: linear(SURFACE_2, 1.0),
        border: linear(ACCENT, 1.0),
        border_width: 1.0,
        radius: TILE_RADIUS,
        shadow_color: linear(SHADOW_RGB, TILE_SHADOW_ALPHA),
        shadow_offset: TILE_SHADOW_OFFSET,
        shadow_blur: TILE_SHADOW_BLUR,
    }
}

fn text_style(color: [u8; 3], font_size: f32) -> TextStyle {
    TextStyle {
        font_size,
        line_height: font_size + 4.0,
        color: [color[0], color[1], color[2], 0xff],
    }
}

fn tile(id: NodeId, label: &str, pointer: &PointerState, selected: NodeId) -> Node {
    let is_selected = selected == id;
    let is_hovered = pointer.hovered == Some(id);

    // Hover: bg-shift on surface only. Selected: surface-2 fill + accent
    // border (1px). Borders carry weight in this design system, so swapping
    // the border color is the click signal — no in-tile rail required.
    let bg = if is_selected {
        selected_style()
    } else if is_hovered {
        tile_style(SURFACE_2)
    } else {
        tile_style(SURFACE_1)
    };

    Node::stack(
        [TILE_W, TILE_H],
        Stack::row()
            .with_padding(EdgeInsets::all(16.0))
            .with_cross_align(CrossAlign::Center)
            .with_background(bg)
            .with_children(vec![Node::text(
                label,
                text_style(if is_selected { CREAM } else { CREAM_DIM }, 14.0),
            )]),
    )
    .with_id(id)
}

fn header() -> Node {
    let logo = Node::rect(
        [LOGO_SIZE, LOGO_SIZE],
        RectStyle {
            fill: linear(ACCENT, 1.0),
            border: [0.0; 4],
            border_width: 0.0,
            radius: LOGO_RADIUS,
            shadow_color: [0.0; 4],
            shadow_offset: [0.0; 2],
            shadow_blur: 0.0,
        },
    );
    let wordmark = Node::text("Januas", text_style(CREAM, WORDMARK_SIZE));
    let wordmark_w = estimate_text_size("Januas", WORDMARK_SIZE, WORDMARK_SIZE + 4.0)[0];

    Node::stack(
        [LOGO_SIZE + HEADER_GAP + wordmark_w, LOGO_SIZE],
        Stack::row()
            .with_gap(HEADER_GAP)
            .with_cross_align(CrossAlign::Center)
            .with_children(vec![logo, wordmark]),
    )
}

fn build_root(viewport: [f32; 2], pointer: &PointerState, selected: NodeId) -> Stack {
    let portals_row = Node::stack(
        [TILE_W.mul_add(3.0, TILE_GAP * 2.0), TILE_H],
        Stack::row().with_gap(TILE_GAP).with_children(vec![
            tile(TILE_A, "tile a", pointer, selected),
            tile(TILE_B, "tile b", pointer, selected),
            tile(TILE_C, "tile c", pointer, selected),
        ]),
    );

    Stack::column()
        .with_gap(28.0)
        .with_padding(EdgeInsets::all(40.0))
        .with_cross_align(CrossAlign::Center)
        .with_background(RectStyle::fill(linear(SURFACE_BASE, 1.0)))
        .with_children(vec![
            // Flex spacer pushes content toward vertical center.
            Node {
                size: [viewport[0], 0.0],
                flex: Some(1.0),
                kind: NodeKind::Rect(RectStyle::fill([0.0; 4])),
                id: None,
            },
            header(),
            portals_row,
            Node {
                size: [viewport[0], 0.0],
                flex: Some(1.0),
                kind: NodeKind::Rect(RectStyle::fill([0.0; 4])),
                id: None,
            },
        ])
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")),
        )
        .init();

    let event_loop = EventLoop::new().context("event_loop init failed")?;
    event_loop.set_control_flow(ControlFlow::Poll);

    let mut app = HomeApp::default();
    event_loop
        .run_app(&mut app)
        .context("event_loop run failed")?;
    Ok(())
}

#[derive(Default)]
struct HomeApp {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    pointer: PointerState,
    selected: NodeId,
    scale: f32,
    physical_size: [u32; 2],
    hit_zones: Vec<HitZone>,
}

impl HomeApp {
    fn rebuild_and_upload(&mut self) {
        let Some(renderer) = self.renderer.as_mut() else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        let logical_w = self.physical_size[0] as f32 / self.scale;
        #[allow(clippy::cast_precision_loss)]
        let logical_h = self.physical_size[1] as f32 / self.scale;
        let root = build_root([logical_w, logical_h], &self.pointer, self.selected);
        let mut frame = layout(&root, [logical_w, logical_h]);
        frame.scale_by(self.scale);
        renderer.set_rects(&frame.rects);
        renderer.set_text_runs(&frame.texts);
        self.hit_zones = frame.hit_zones;
    }
}

impl ApplicationHandler for HomeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Januas ADE — primitives smoke")
            .with_inner_size(winit::dpi::LogicalSize::new(
                DEFAULT_LOGICAL_W,
                DEFAULT_LOGICAL_H,
            ));
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

        let physical = window.inner_size();
        #[allow(clippy::cast_possible_truncation)]
        let scale = window.scale_factor() as f32;
        self.physical_size = [physical.width.max(1), physical.height.max(1)];
        self.scale = if scale > 0.0 { scale } else { 1.0 };

        self.renderer = Some(renderer);
        self.window = Some(window);
        self.rebuild_and_upload();
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        match event {
            WindowEvent::CloseRequested => event_loop.exit(),
            WindowEvent::Resized(size) => {
                if let Some(r) = self.renderer.as_mut() {
                    r.resize(size.width, size.height);
                }
                self.physical_size = [size.width.max(1), size.height.max(1)];
                self.rebuild_and_upload();
            }
            WindowEvent::ScaleFactorChanged { scale_factor, .. } => {
                #[allow(clippy::cast_possible_truncation)]
                let s = scale_factor as f32;
                self.scale = if s > 0.0 { s } else { 1.0 };
                self.rebuild_and_upload();
            }
            WindowEvent::CursorMoved {
                position: PhysicalPosition { x, y },
                ..
            } => {
                #[allow(clippy::cast_possible_truncation)]
                let px = x as f32;
                #[allow(clippy::cast_possible_truncation)]
                let py = y as f32;
                if self.pointer.move_to([px, py], &self.hit_zones) {
                    self.rebuild_and_upload();
                }
            }
            WindowEvent::CursorLeft { .. } => {
                if self.pointer.leave() {
                    self.rebuild_and_upload();
                }
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                if let Some(id) = self.pointer.hovered {
                    if self.selected != id {
                        self.selected = id;
                        self.rebuild_and_upload();
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                if let Some(r) = self.renderer.as_mut() {
                    if let Err(e) = r.render() {
                        tracing::error!(error = ?e, "render failed");
                    }
                }
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            _ => {}
        }
    }
}
