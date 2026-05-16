//! S5.5b/c interactive smoke — compose a simplified home-screen frame from
//! the ui layout primitives, render it through the live `Renderer`, route
//! cursor + mouse events through a pointer state machine.
//!
//! Run with `cargo run --release --example home_compose -p januas-ui`. Opens
//! a 1280×800 logical-size window holding four vertical regions (titlebar /
//! subway tabs / hero / footer). Cursor over a workspace tab shifts its
//! background (the design-principles "bg-shift" hover rule); a left click
//! drops the 2px accent rail onto the clicked tab (the "active" selection
//! marker per `~/Desktop/Januas/docs/design-principles.md`).

use std::sync::Arc;

use anyhow::{Context as _, Result};
use januas_renderer::Renderer;
use januas_ui::{
    CrossAlign, EdgeInsets, HitZone, Node, NodeId, NodeKind, PointerState, RectStyle, Stack,
    TextStyle, layout,
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

const TITLEBAR_H: f32 = 34.0;
const SUBWAY_H: f32 = 44.0;
const FOOTER_H: f32 = 36.0;

const TAB_H: f32 = 28.0;
const TAB_GAP: f32 = 2.0;
const TAB_RADIUS: f32 = 6.0;

const MODE_BTN_H: f32 = 60.0;
const MODE_BTN_W: f32 = 360.0;
const MODE_BTN_RADIUS: f32 = 8.0;

const LOGO_SIZE: f32 = 64.0;
const LOGO_RADIUS: f32 = 14.0;

const TRAFFIC_DOT: f32 = 11.0;
const TRAFFIC_RADIUS: f32 = 5.5;
const TRAFFIC_GAP: f32 = 7.0;

const HK_CHIP_H: f32 = 18.0;
const HK_CHIP_RADIUS: f32 = 4.0;

// Hit-test ids — keep stable across rebuilds so PointerState comparisons hold.
const TAB_PERSONAL: NodeId = 1;
const TAB_JANUAS_DEV: NodeId = 2;
const TAB_OSS: NodeId = 3;
const TAB_FIVEM: NodeId = 4;
const TAB_ADD: NodeId = 5;
const MODE_BTN: NodeId = 10;

const fn is_tab(id: NodeId) -> bool {
    matches!(id, TAB_PERSONAL | TAB_JANUAS_DEV | TAB_OSS | TAB_FIVEM)
}

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
const SURFACE_3: [u8; 3] = [0x2a, 0x31, 0x32];
const ACCENT: [u8; 3] = [0x6b, 0xb8, 0xa8];
const CREAM: [u8; 3] = [0xdd, 0xd2, 0xbb];
const CREAM_DIM: [u8; 3] = [0x97, 0x90, 0x7f];
const CREAM_FAINT: [u8; 3] = [0x58, 0x52, 0x49];
const TRAFFIC_RED: [u8; 3] = [0xff, 0x5f, 0x56];
const TRAFFIC_AMBER: [u8; 3] = [0xff, 0xbd, 0x2e];
const TRAFFIC_GREEN: [u8; 3] = [0x27, 0xc9, 0x3f];

fn flat(rgb: [u8; 3], alpha: f32) -> RectStyle {
    RectStyle::fill(linear(rgb, alpha))
}

fn rounded(rgb: [u8; 3], radius: f32) -> RectStyle {
    RectStyle {
        fill: linear(rgb, 1.0),
        border: [0.0; 4],
        border_width: 0.0,
        radius,
    }
}

fn bordered(rgb: [u8; 3], border_rgba: [u8; 3], border_alpha: f32, radius: f32) -> RectStyle {
    RectStyle {
        fill: linear(rgb, 1.0),
        border: linear(border_rgba, border_alpha),
        border_width: 1.0,
        radius,
    }
}

fn text(color: [u8; 3], font_size: f32) -> TextStyle {
    TextStyle {
        font_size,
        line_height: font_size + 4.0,
        color: [color[0], color[1], color[2], 0xff],
    }
}

fn spacer(weight: f32) -> Node {
    Node {
        size: [0.0, 0.0],
        flex: Some(weight),
        kind: NodeKind::Rect(flat(SURFACE_BASE, 0.0)),
        id: None,
    }
}

fn titlebar() -> Stack {
    let title_style = text(CREAM_DIM, 11.5);
    Stack::row()
        .with_padding(EdgeInsets::symmetric(0.0, 12.0))
        .with_cross_align(CrossAlign::Center)
        .with_background(flat(SURFACE_BASE, 1.0))
        .with_children(vec![
            Node::stack(
                [TRAFFIC_DOT.mul_add(3.0, TRAFFIC_GAP * 2.0), TRAFFIC_DOT],
                Stack::row().with_gap(TRAFFIC_GAP).with_children(vec![
                    Node::rect(
                        [TRAFFIC_DOT, TRAFFIC_DOT],
                        rounded(TRAFFIC_RED, TRAFFIC_RADIUS),
                    ),
                    Node::rect(
                        [TRAFFIC_DOT, TRAFFIC_DOT],
                        rounded(TRAFFIC_AMBER, TRAFFIC_RADIUS),
                    ),
                    Node::rect(
                        [TRAFFIC_DOT, TRAFFIC_DOT],
                        rounded(TRAFFIC_GREEN, TRAFFIC_RADIUS),
                    ),
                ]),
            ),
            spacer(1.0),
            Node::text("januas · personal", title_style),
            spacer(1.0),
            Node::rect(
                [TRAFFIC_DOT.mul_add(3.0, TRAFFIC_GAP * 2.0), 1.0],
                flat(SURFACE_BASE, 0.0),
            ),
        ])
}

fn ws_tab(
    id: NodeId,
    name: &str,
    count: &str,
    dot_rgb: [u8; 3],
    pointer: &PointerState,
    selected: NodeId,
) -> Node {
    let is_selected = selected == id;
    let is_hovered = pointer.hovered == Some(id);

    let name_style = text(if is_selected { CREAM } else { CREAM_DIM }, 12.5);
    let count_style = text(CREAM_FAINT, 10.5);

    // Background follows the design-principles state machine:
    //   selected → surface-2 fill (selection chrome)
    //   hovered (and not selected) → surface-1 fill (the bg-shift hover rule)
    //   else → transparent
    let bg = if is_selected {
        rounded(SURFACE_2, TAB_RADIUS)
    } else if is_hovered {
        rounded(SURFACE_1, TAB_RADIUS)
    } else {
        flat(SURFACE_BASE, 0.0)
    };

    let mut children: Vec<Node> = Vec::with_capacity(5);
    if is_selected {
        // 2px accent rail at the left edge — the active-state marker per
        // design-principles.md.
        children.push(Node::rect([2.0, TAB_H - 12.0], rounded(ACCENT, 1.0)));
    }
    children.push(Node::rect([7.0, 7.0], rounded(dot_rgb, 3.5)));
    children.push(Node::text(name, name_style));
    children.push(spacer(1.0));
    children.push(Node::text(count, count_style));

    let tab_w = 110.0;
    Node::stack(
        [tab_w, TAB_H],
        Stack::row()
            .with_gap(8.0)
            .with_padding(EdgeInsets::symmetric(0.0, 10.0))
            .with_cross_align(CrossAlign::Center)
            .with_background(bg)
            .with_children(children),
    )
    .with_id(id)
}

fn subway(pointer: &PointerState, selected: NodeId) -> Stack {
    let add_hover = pointer.hovered == Some(TAB_ADD);
    let add_bg = if add_hover {
        bordered(SURFACE_1, CREAM, 0.14, TAB_RADIUS)
    } else {
        bordered(SURFACE_BASE, CREAM, 0.09, TAB_RADIUS)
    };
    Stack::row()
        .with_gap(TAB_GAP)
        .with_padding(EdgeInsets::symmetric(8.0, 12.0))
        .with_cross_align(CrossAlign::Center)
        .with_background(flat(SURFACE_BASE, 1.0))
        .with_children(vec![
            ws_tab(TAB_PERSONAL, "Personal", "12", ACCENT, pointer, selected),
            ws_tab(
                TAB_JANUAS_DEV,
                "Januas Dev",
                "6",
                [0x6f, 0xb8, 0x9c],
                pointer,
                selected,
            ),
            ws_tab(TAB_OSS, "OSS", "3", [0x6e, 0x9b, 0xd4], pointer, selected),
            ws_tab(
                TAB_FIVEM,
                "FiveM",
                "2",
                [0xe8, 0x93, 0x57],
                pointer,
                selected,
            ),
            spacer(1.0),
            Node::rect([TAB_H, TAB_H], add_bg).with_id(TAB_ADD),
        ])
}

fn hero(pointer: &PointerState) -> Stack {
    let wordmark_style = text(CREAM, 26.0);
    let meta_style = text(CREAM_FAINT, 10.5);
    let tagline_style = text(CREAM_DIM, 15.0);
    let mode_name_style = text(CREAM, 14.0);
    let mode_sub_style = text(CREAM_DIM, 12.0);
    let mode_kbd_style = text(CREAM_DIM, 11.0);

    let mode_hovered = pointer.hovered == Some(MODE_BTN);
    let mode_bg = if mode_hovered {
        // bg-shift to surface-3 + border-color to accent — matches v6
        // .mode-btn:hover.
        bordered(SURFACE_3, ACCENT, 1.0, MODE_BTN_RADIUS)
    } else {
        bordered(SURFACE_2, CREAM, 0.09, MODE_BTN_RADIUS)
    };

    let logo = Node::rect([LOGO_SIZE, LOGO_SIZE], rounded(SURFACE_2, LOGO_RADIUS));
    let wordmark = Node::text("Januas", wordmark_style);
    let meta = Node::text("v0.0.0  ·  slice s5.5c  ·  pointer state", meta_style);
    let tagline = Node::text(
        "A workspace is a project, a layout, and the CLIs that go with them.",
        tagline_style,
    );
    let mode_button = Node::stack(
        [MODE_BTN_W, MODE_BTN_H],
        Stack::row()
            .with_gap(14.0)
            .with_padding(EdgeInsets::symmetric(14.0, 16.0))
            .with_cross_align(CrossAlign::Center)
            .with_background(mode_bg)
            .with_children(vec![
                Node::text("▦", text(ACCENT, 15.0)),
                Node::stack(
                    [MODE_BTN_W - 100.0, 32.0],
                    Stack::column().with_gap(2.0).with_children(vec![
                        Node::text("Parallex", mode_name_style),
                        Node::text("project · grid · providers", mode_sub_style),
                    ]),
                ),
                spacer(1.0),
                Node::stack(
                    [44.0, HK_CHIP_H],
                    Stack::row()
                        .with_padding(EdgeInsets::symmetric(2.0, 8.0))
                        .with_cross_align(CrossAlign::Center)
                        .with_background(bordered(SURFACE_2, CREAM, 0.09, HK_CHIP_RADIUS))
                        .with_children(vec![Node::text("⌘ T", mode_kbd_style)]),
                ),
            ]),
    )
    .with_id(MODE_BTN);

    Stack::column()
        .with_gap(20.0)
        .with_padding(EdgeInsets::symmetric(56.0, 40.0))
        .with_cross_align(CrossAlign::Center)
        .with_background(flat(SURFACE_BASE, 1.0))
        .with_children(vec![logo, wordmark, meta, tagline, mode_button])
}

fn hk_chip(key: &str, label: &str) -> Node {
    let key_style = text(CREAM_DIM, 10.0);
    let label_style = text(CREAM_FAINT, 10.5);
    Node::stack(
        [120.0, HK_CHIP_H + 4.0],
        Stack::row()
            .with_gap(7.0)
            .with_cross_align(CrossAlign::Center)
            .with_children(vec![
                Node::stack(
                    [28.0, HK_CHIP_H],
                    Stack::row()
                        .with_padding(EdgeInsets::symmetric(1.0, 5.0))
                        .with_cross_align(CrossAlign::Center)
                        .with_background(bordered(SURFACE_BASE, CREAM, 0.09, HK_CHIP_RADIUS))
                        .with_children(vec![Node::text(key, key_style)]),
                ),
                Node::text(label, label_style),
            ]),
    )
}

fn footer() -> Stack {
    Stack::row()
        .with_gap(22.0)
        .with_padding(EdgeInsets::all(14.0))
        .with_cross_align(CrossAlign::Center)
        .with_background(flat(SURFACE_BASE, 1.0))
        .with_children(vec![
            spacer(1.0),
            hk_chip("⌘N", "new terminal"),
            hk_chip("⌘D", "split right"),
            hk_chip("⌘K", "command palette"),
            hk_chip("⌘,", "settings"),
            spacer(1.0),
        ])
}

fn build_home_frame(logical_viewport: [f32; 2], pointer: &PointerState, selected: NodeId) -> Stack {
    Stack::column().with_children(vec![
        Node::stack([logical_viewport[0], TITLEBAR_H], titlebar()),
        Node::stack(
            [logical_viewport[0], 1.0],
            Stack::row().with_background(flat(CREAM, 0.09)),
        ),
        Node::stack([logical_viewport[0], SUBWAY_H], subway(pointer, selected)),
        Node::stack(
            [logical_viewport[0], 1.0],
            Stack::row().with_background(flat(CREAM, 0.09)),
        ),
        Node {
            size: [logical_viewport[0], 0.0],
            flex: Some(1.0),
            kind: NodeKind::Stack(hero(pointer)),
            id: None,
        },
        Node::stack(
            [logical_viewport[0], 1.0],
            Stack::row().with_background(flat(CREAM, 0.09)),
        ),
        Node::stack([logical_viewport[0], FOOTER_H], footer()),
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
        let root = build_home_frame([logical_w, logical_h], &self.pointer, self.selected);
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
            .with_title("Januas ADE — home_compose")
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
        self.selected = TAB_PERSONAL;

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
                    if is_tab(id) && self.selected != id {
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
