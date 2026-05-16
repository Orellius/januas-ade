//! S5.5d — Januas ADE home screen, composed against
//! `~/Desktop/claude-html/januas-home-20260516-0947.html` v6.
//!
//! Four sections stacked top-to-bottom: titlebar (traffic dots + mono title),
//! subway (workspace tabs + add), hero (centered logo / wordmark / meta-row
//! / tagline / Parallex mode-button), footer (mono hotkey chips). Real
//! cosmic-text measurement via `Renderer::measure_text` replaces the
//! monospace approximation; family selection (`Display` / `Body` / `Mono`)
//! lands the locked v0.1 type stack from
//! `~/Desktop/Januas/docs/design-tokens.md`. The brand mark PNG is uploaded
//! to the new image-texture pipeline and clipped to the 14px radius the
//! token spec calls out for the logo bitmap.
//!
//! Run: `cargo run --release --example home_compose -p januas-ui`.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use januas_renderer::{ImageId, Renderer, TextFamily};
use januas_ui::{
    CrossAlign, EdgeInsets, HitZone, ImageStyle, Node, NodeId, NodeKind, PointerState, RectStyle,
    Stack, TextStyle,
    color::linear,
    tokens::{self, fonts, palette, radii},
};
use tracing::warn;
use tracing_subscriber::EnvFilter;
#[cfg(target_os = "macos")]
use winit::platform::macos::WindowAttributesExtMacOS as _;
use winit::{
    application::ApplicationHandler,
    dpi::PhysicalPosition,
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::{ActiveEventLoop, ControlFlow, EventLoop},
    window::{Window, WindowId},
};

const DEFAULT_LOGICAL_W: f64 = 1280.0;
const DEFAULT_LOGICAL_H: f64 = 800.0;

// Section heights match the v6 CSS literals; only the hero is flex.
const TITLEBAR_H: f32 = 34.0;
const SUBWAY_H: f32 = 44.0;
const FOOTER_H: f32 = 40.0;

// Hover/active state IDs. `0` reserved as "unselected" — Personal lands as
// the default selection on boot.
const TAB_PERSONAL: NodeId = 1;
const TAB_DEV: NodeId = 2;
const TAB_OSS: NodeId = 3;
const TAB_FIVEM: NodeId = 4;
const TAB_SCRATCH: NodeId = 5;
const TAB_ADD: NodeId = 6;
const MODE_BTN: NodeId = 7;

/// Logical workspace presented in the subway. One per tab.
struct Workspace {
    id: NodeId,
    name: &'static str,
    dot: [u8; 3],
    count: &'static str,
}

const WORKSPACES: &[Workspace] = &[
    Workspace {
        id: TAB_PERSONAL,
        name: "Personal",
        dot: palette::ACCENT,
        count: "12",
    },
    Workspace {
        id: TAB_DEV,
        name: "Januas Dev",
        dot: palette::GREEN,
        count: "6",
    },
    Workspace {
        id: TAB_OSS,
        name: "OSS",
        dot: palette::BLUE,
        count: "3",
    },
    Workspace {
        id: TAB_FIVEM,
        name: "FiveM (orellius)",
        dot: palette::ORANGE,
        count: "2",
    },
    Workspace {
        id: TAB_SCRATCH,
        name: "Scratch",
        dot: palette::PINK,
        count: "0",
    },
];

/// One row in the footer hotkey strip. `kbd` is the chord glyphs, `label`
/// the action verb beside them.
struct Hotkey {
    kbd: &'static str,
    label: &'static str,
}

const HOTKEYS: &[Hotkey] = &[
    Hotkey {
        kbd: "⌘ N",
        label: "new terminal",
    },
    Hotkey {
        kbd: "⌘ D",
        label: "split right",
    },
    Hotkey {
        kbd: "⌘ K",
        label: "command palette",
    },
    Hotkey {
        kbd: "⌘ ,",
        label: "settings",
    },
    Hotkey {
        kbd: "⌘ /",
        label: "search",
    },
];

/// Static text strings the scene reads from before it can ask the renderer
/// for measurements. Kept here so the build pass measures once per string.
const META_ROW: &str = "v0.0.0  ·  slice s5.5d  ·  ~340 fps";
const TAGLINE: &str =
    "A workspace is a project, a layout, and the CLIs that go with them — wired in one pass.";
const WORDMARK: &str = "Januas";
const TITLE_TEXT: &str = "januas · personal";
const MODE_NAME: &str = "Parallex";
const MODE_SUB: &str = "project · grid · providers";
const MODE_KBD: &str = "⌘ T";
const MODE_ICON: &str = "▦";

const fn rect(size: [f32; 2], style: RectStyle) -> Node {
    Node::rect(size, style)
}

fn flat(color: [u8; 3], alpha: f32) -> RectStyle {
    RectStyle::fill(linear(color, alpha))
}

fn rounded(color: [u8; 3], alpha: f32, radius: f32) -> RectStyle {
    let mut s = RectStyle::fill(linear(color, alpha));
    s.radius = radius;
    s
}

fn outlined(
    fill_rgb: [u8; 3],
    fill_a: f32,
    border_rgb: [u8; 3],
    border_a: f32,
    radius: f32,
) -> RectStyle {
    RectStyle {
        fill: linear(fill_rgb, fill_a),
        border: linear(border_rgb, border_a),
        border_width: 1.0,
        radius,
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        shadow_blur: 0.0,
    }
}

fn circle(diameter: f32, color: [u8; 3]) -> Node {
    rect([diameter, diameter], rounded(color, 1.0, diameter * 0.5))
}

fn text_node(
    renderer: &mut Renderer,
    content: &str,
    family: TextFamily,
    font_size: f32,
    color: [u8; 3],
    alpha: u8,
) -> Node {
    let line_height = font_size + 4.0;
    let size = renderer.measure_text(family, content, font_size, line_height);
    Node::text_sized(
        content,
        TextStyle {
            font_size,
            line_height,
            color: [color[0], color[1], color[2], alpha],
            family,
        },
        size,
    )
}

const fn spacer(flex_weight: f32) -> Node {
    Node {
        size: [0.0; 2],
        flex: Some(flex_weight),
        kind: NodeKind::Rect(RectStyle::fill([0.0; 4])),
        id: None,
    }
}

fn divider(viewport_w: f32) -> Node {
    rect([viewport_w, 1.0], flat(palette::CREAM, tokens::CREAM_09_A))
}

// ===== Titlebar =====
//
// On macOS, winit's WindowAttributesExtMacOS hides the OS titlebar chrome
// and extends the content view under the standard window-control cluster
// (red/amber/green close/min/max dots). Those dots paint ON TOP of our
// titlebar at fixed positions starting ~11px from the left edge; the
// cluster spans roughly 63px. We leave 78px of empty reserve on the left
// (and a matching reserve on the right) so the center title sits visually
// centered against the OS-drawn dots without us re-painting them.

/// Width of the macOS native traffic-light cluster plus a little breathing
/// room. Token-shaped so the next titlebar surface can reuse it.
const MACOS_TRAFFIC_RESERVE: f32 = 78.0;

fn titlebar_center(renderer: &mut Renderer, logo: ImageId) -> Node {
    let logo_node = Node::image([14.0, 14.0], ImageStyle::new(logo).with_radius(3.0));
    let title = text_node(
        renderer,
        TITLE_TEXT,
        TextFamily::Mono,
        fonts::SMALL_PX,
        tokens::TEXT_DIM,
        0xff,
    );
    let title_w = title.size[0];

    let inner = Stack::row()
        .with_gap(8.0)
        .with_cross_align(CrossAlign::Center)
        .with_children(vec![logo_node, title]);
    Node::stack([14.0 + 8.0 + title_w, 14.0], inner)
}

fn titlebar(renderer: &mut Renderer, logo: ImageId) -> Node {
    let center = titlebar_center(renderer, logo);

    let row = Stack::row()
        .with_padding(EdgeInsets::symmetric(0.0, 12.0))
        .with_cross_align(CrossAlign::Center)
        .with_children(vec![
            // Left reserve — native macOS traffic-light buttons paint here.
            Node::rect([MACOS_TRAFFIC_RESERVE, 1.0], RectStyle::fill([0.0; 4])),
            spacer(1.0),
            center,
            spacer(1.0),
            // Symmetric right reserve so the title sits visually centered
            // against the native cluster on the left.
            Node::rect([MACOS_TRAFFIC_RESERVE, 1.0], RectStyle::fill([0.0; 4])),
        ]);
    Node::stack([0.0, TITLEBAR_H], row)
}

// ===== Subway =====

fn ws_tab(renderer: &mut Renderer, ws: &Workspace, hovered: bool, active: bool) -> Node {
    let bg = if active {
        rounded(tokens::SURFACE_PANEL, 1.0, radii::MD)
    } else if hovered {
        rounded(tokens::SURFACE_RAISED, 1.0, radii::MD)
    } else {
        rounded(tokens::SURFACE_BASE, 0.0, radii::MD)
    };

    let name_color = if active {
        tokens::TEXT
    } else {
        tokens::TEXT_DIM
    };
    let count_color = if active {
        tokens::TEXT_DIM
    } else {
        tokens::TEXT_FAINT
    };

    let name = text_node(
        renderer,
        ws.name,
        TextFamily::Body,
        fonts::BODY_PX,
        name_color,
        0xff,
    );
    let count = text_node(
        renderer,
        ws.count,
        TextFamily::Mono,
        fonts::FOOTER_PX,
        count_color,
        0xff,
    );
    let name_w = name.size[0];
    let count_w = count.size[0];

    // Active rail: 2px × 16px accent strip at the tab's left edge. Sized
    // intrinsically so CrossAlign::Center keeps it visually centered
    // vertically within the 28px-tall tab body.
    let rail: Node = if active {
        rect([2.0, 16.0], rounded(tokens::ACCENT, 1.0, 1.0))
    } else {
        rect([2.0, 16.0], flat([0; 3], 0.0))
    };

    let dot = circle(7.0, ws.dot);

    let content = Stack::row()
        .with_gap(9.0)
        .with_padding(EdgeInsets {
            top: 0.0,
            right: 12.0,
            bottom: 0.0,
            left: 10.0,
        })
        .with_cross_align(CrossAlign::Center)
        .with_background(bg)
        .with_children(vec![rail, dot, name, count]);

    let intrinsic_w = 2.0 + 9.0 + 7.0 + 9.0 + name_w + 9.0 + count_w + 10.0 + 12.0;
    Node::stack([intrinsic_w, 28.0], content).with_id(ws.id)
}

fn ws_add(hovered: bool) -> Node {
    let bg = if hovered {
        rounded(tokens::SURFACE_RAISED, 1.0, radii::MD)
    } else {
        rounded(tokens::SURFACE_BASE, 0.0, radii::MD)
    };
    let inner = Stack::row()
        .with_cross_align(CrossAlign::Center)
        .with_background(bg)
        .with_children(vec![spacer(1.0)]);
    Node::stack([28.0, 28.0], inner).with_id(TAB_ADD)
}

fn subway(renderer: &mut Renderer, pointer: &PointerState, selected: NodeId) -> Node {
    let mut children: Vec<Node> = Vec::with_capacity(WORKSPACES.len() + 2);
    for ws in WORKSPACES {
        let hovered = pointer.hovered == Some(ws.id);
        let active = selected == ws.id;
        children.push(ws_tab(renderer, ws, hovered, active));
    }
    children.push(spacer(1.0));
    children.push(ws_add(pointer.hovered == Some(TAB_ADD)));

    let row = Stack::row()
        .with_gap(2.0)
        .with_padding(EdgeInsets::symmetric(8.0, 12.0))
        .with_cross_align(CrossAlign::Center)
        .with_children(children);
    Node::stack([0.0, SUBWAY_H], row)
}

// ===== Hero =====

fn hero(renderer: &mut Renderer, logo: ImageId, pointer: &PointerState) -> Node {
    let logo_node = Node::image([64.0, 64.0], ImageStyle::new(logo).with_radius(radii::LOGO));

    let wordmark = text_node(
        renderer,
        WORDMARK,
        TextFamily::Display,
        fonts::WORDMARK_PX,
        tokens::TEXT,
        0xff,
    );
    let wordmark_h = wordmark.size[1];

    let meta = text_node(
        renderer,
        META_ROW,
        TextFamily::Mono,
        fonts::FOOTER_PX,
        tokens::TEXT_FAINT,
        0xff,
    );
    let meta_h = meta.size[1];

    let tagline_line = fonts::TAGLINE_PX * 1.5;
    let tagline_natural =
        renderer.measure_text(TextFamily::Body, TAGLINE, fonts::TAGLINE_PX, tagline_line);
    let tagline_max = 360.0;
    let tagline_w = tagline_natural[0].min(tagline_max);
    // The tagline wraps in HTML at 360px; with measure_text reporting the
    // single-line width we cap the layout box and let the shaper wrap inside
    // it. Two lines is the conservative bound for the v6 copy at 15px.
    let tagline_h = if tagline_natural[0] > tagline_max {
        tagline_line * 2.0
    } else {
        tagline_line
    };
    let tagline = Node::text_sized(
        TAGLINE,
        TextStyle {
            font_size: fonts::TAGLINE_PX,
            line_height: tagline_line,
            color: [
                tokens::TEXT_DIM[0],
                tokens::TEXT_DIM[1],
                tokens::TEXT_DIM[2],
                0xff,
            ],
            family: TextFamily::Body,
        },
        [tagline_w, tagline_h],
    );

    let mode = mode_button(renderer, pointer.hovered == Some(MODE_BTN));
    let mode_w = mode.size[0];

    let column = Stack::column()
        .with_gap(28.0)
        .with_cross_align(CrossAlign::Center)
        .with_children(vec![logo_node, wordmark, meta, tagline, mode]);

    // Intrinsic content height — the column packs from main-axis start; the
    // outer hero wrapper centers it vertically via flex spacers.
    let content_h = 64.0 + 28.0 + wordmark_h + 28.0 + meta_h + 28.0 + tagline_h + 28.0 + 56.0;
    let content = Node::stack([mode_w, content_h], column);

    let wrapper = Stack::column()
        .with_padding(EdgeInsets::symmetric(56.0, 40.0))
        .with_cross_align(CrossAlign::Center)
        .with_children(vec![spacer(1.0), content, spacer(1.0)]);
    Node {
        size: [0.0, 0.0],
        flex: Some(1.0),
        kind: NodeKind::Stack(wrapper),
        id: None,
    }
}

fn mode_button(renderer: &mut Renderer, hovered: bool) -> Node {
    let bg = if hovered {
        outlined(tokens::SURFACE_HOVER, 1.0, tokens::ACCENT, 1.0, radii::LG)
    } else {
        outlined(
            tokens::SURFACE_PANEL,
            1.0,
            tokens::TEXT,
            tokens::CREAM_09_A,
            radii::LG,
        )
    };

    let icon = text_node(
        renderer,
        MODE_ICON,
        TextFamily::Mono,
        15.0,
        tokens::ACCENT,
        0xff,
    );
    let icon_box = Node::stack(
        [28.0, 28.0],
        Stack::row()
            .with_cross_align(CrossAlign::Center)
            .with_children(vec![spacer(1.0), icon, spacer(1.0)]),
    );

    let name = text_node(
        renderer,
        MODE_NAME,
        TextFamily::Body,
        14.0,
        tokens::TEXT,
        0xff,
    );
    let sub = text_node(
        renderer,
        MODE_SUB,
        TextFamily::Mono,
        fonts::MODE_SUB_PX,
        tokens::TEXT_DIM,
        0xff,
    );
    let body_h = name.size[1] + 2.0 + sub.size[1];
    let body_w = name.size[0].max(sub.size[0]);
    let body = Node::stack(
        [body_w, body_h],
        Stack::column().with_gap(2.0).with_children(vec![name, sub]),
    );

    let kbd_label = text_node(
        renderer,
        MODE_KBD,
        TextFamily::Mono,
        fonts::KBD_PX,
        tokens::TEXT_DIM,
        0xff,
    );
    let kbd_w = kbd_label.size[0] + 16.0;
    let kbd = Node::stack(
        [kbd_w, kbd_label.size[1] + 6.0],
        Stack::row()
            .with_padding(EdgeInsets::symmetric(3.0, 8.0))
            .with_cross_align(CrossAlign::Center)
            .with_background(outlined(
                tokens::SURFACE_BASE,
                0.0,
                tokens::TEXT,
                tokens::CREAM_09_A,
                radii::SM,
            ))
            .with_children(vec![kbd_label]),
    );

    let row = Stack::row()
        .with_gap(14.0)
        .with_padding(EdgeInsets::symmetric(14.0, 16.0))
        .with_cross_align(CrossAlign::Center)
        .with_background(bg)
        .with_children(vec![icon_box, body, spacer(1.0), kbd]);

    let outer_w = 16.0 + 28.0 + 14.0 + body_w + 14.0 + 80.0 + 14.0 + kbd_w + 16.0;
    let outer_h = 14.0_f32.mul_add(2.0, 28.0);
    Node::stack([outer_w.max(360.0), outer_h], row).with_id(MODE_BTN)
}

// ===== Footer =====

fn footer_chip(renderer: &mut Renderer, chip: &Hotkey) -> Node {
    let kbd_label = text_node(
        renderer,
        chip.kbd,
        TextFamily::Mono,
        10.0,
        tokens::TEXT_DIM,
        0xff,
    );
    let kbd = Node::stack(
        [kbd_label.size[0] + 10.0, kbd_label.size[1] + 2.0],
        Stack::row()
            .with_padding(EdgeInsets::symmetric(1.0, 5.0))
            .with_cross_align(CrossAlign::Center)
            .with_background(outlined(
                tokens::SURFACE_RAISED,
                0.0,
                tokens::TEXT,
                tokens::CREAM_09_A,
                radii::SM,
            ))
            .with_children(vec![kbd_label]),
    );

    let label = text_node(
        renderer,
        chip.label,
        TextFamily::Mono,
        fonts::FOOTER_PX,
        tokens::TEXT_FAINT,
        0xff,
    );
    let kbd_w = kbd.size[0];
    let kbd_h = kbd.size[1];
    let label_w = label.size[0];
    let label_h = label.size[1];

    let row = Stack::row()
        .with_gap(7.0)
        .with_cross_align(CrossAlign::Center)
        .with_children(vec![kbd, label]);
    Node::stack([kbd_w + 7.0 + label_w, row_height(&[kbd_h, label_h])], row)
}

fn row_height(heights: &[f32]) -> f32 {
    let mut h: f32 = 0.0;
    for &v in heights {
        if v > h {
            h = v;
        }
    }
    h
}

fn footer(renderer: &mut Renderer) -> Node {
    let mut children: Vec<Node> = Vec::with_capacity(HOTKEYS.len());
    for chip in HOTKEYS {
        children.push(footer_chip(renderer, chip));
    }

    let row = Stack::row()
        .with_gap(22.0)
        .with_padding(EdgeInsets::all(14.0))
        .with_cross_align(CrossAlign::Center)
        .with_background(flat(tokens::SURFACE_RAISED, 1.0))
        .with_children(
            vec![spacer(1.0)]
                .into_iter()
                .chain(children)
                .chain(std::iter::once(spacer(1.0)))
                .collect(),
        );
    Node::stack([0.0, FOOTER_H], row)
}

// ===== Root =====

fn build_root(
    renderer: &mut Renderer,
    logo: ImageId,
    viewport: [f32; 2],
    pointer: &PointerState,
    selected: NodeId,
) -> Stack {
    let titlebar_n = titlebar(renderer, logo);
    let subway_n = subway(renderer, pointer, selected);
    let hero_n = hero(renderer, logo, pointer);
    let footer_n = footer(renderer);
    let divider_a = divider(viewport[0]);
    let divider_b = divider(viewport[0]);
    let divider_c = divider(viewport[0]);

    Stack::column()
        .with_background(flat(tokens::SURFACE_BASE, 1.0))
        .with_children(vec![
            titlebar_n, divider_a, subway_n, divider_b, hero_n, divider_c, footer_n,
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
    logo: Option<ImageId>,
}

const LOGO_PNG: &[u8] = include_bytes!("../../../assets/januas-ade-logo.png");

impl HomeApp {
    fn rebuild_and_upload(&mut self) {
        let (Some(renderer), Some(logo)) = (self.renderer.as_mut(), self.logo) else {
            return;
        };
        #[allow(clippy::cast_precision_loss)]
        let logical_w = self.physical_size[0] as f32 / self.scale;
        #[allow(clippy::cast_precision_loss)]
        let logical_h = self.physical_size[1] as f32 / self.scale;
        let root = build_root(
            renderer,
            logo,
            [logical_w, logical_h],
            &self.pointer,
            self.selected,
        );
        let mut frame = januas_ui::layout(&root, [logical_w, logical_h]);
        frame.scale_by(self.scale);
        renderer.set_rects(&frame.rects);
        renderer.set_text_runs(&frame.texts);
        renderer.set_images(&frame.images);
        self.hit_zones = frame.hit_zones;
    }
}

impl ApplicationHandler for HomeApp {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }
        let attrs = Window::default_attributes()
            .with_title("Januas ADE — home v6")
            .with_inner_size(winit::dpi::LogicalSize::new(
                DEFAULT_LOGICAL_W,
                DEFAULT_LOGICAL_H,
            ));
        // macOS unified-titlebar pattern (Warp / VS Code / Linear-desktop):
        // keep the titlebar PRESENT so AppKit keeps drawing the native
        // traffic-light buttons, but make its background transparent and
        // extend our content view up under it. `titlebar_hidden(true)`
        // hides the buttons too — wrong knob.
        #[cfg(target_os = "macos")]
        let attrs = attrs
            .with_titlebar_transparent(true)
            .with_fullsize_content_view(true)
            .with_title_hidden(true);
        let window = match event_loop.create_window(attrs) {
            Ok(w) => Arc::new(w),
            Err(e) => {
                warn!(error = ?e, "window creation failed");
                event_loop.exit();
                return;
            }
        };
        let mut renderer = match Renderer::new(Arc::clone(&window)) {
            Ok(r) => r,
            Err(e) => {
                warn!(error = ?e, "renderer init failed");
                event_loop.exit();
                return;
            }
        };

        let logo = match renderer.load_image_png(LOGO_PNG) {
            Ok(id) => id,
            Err(e) => {
                warn!(error = ?e, "logo PNG load failed");
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
        self.logo = Some(logo);
        self.selected = TAB_PERSONAL;
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

const fn is_tab(id: NodeId) -> bool {
    matches!(
        id,
        TAB_PERSONAL | TAB_DEV | TAB_OSS | TAB_FIVEM | TAB_SCRATCH
    )
}
