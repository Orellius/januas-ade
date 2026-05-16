//! S5.5e — Januas ADE home screen, wired to a real module-instance set.
//!
//! Replaces S5.5d's mockup data with a `januas_modules::InstanceSet`. Home
//! is a dedicated button (left of a 1px divider) — not a tab; tabs are
//! running `ModuleInstance`s, dynamically spawned by clicking a launcher
//! button on the Home scene. Selecting a tab swaps the scene to that
//! module's stub view; selecting Home returns to the launcher.
//!
//! Run: `cargo run --release --example home_compose -p januas-ui`.

use std::sync::Arc;

use anyhow::{Context as _, Result};
use januas_modules::{InstanceId, InstanceSet, ModuleInstance, ModuleKind, Selection};
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

use januas_ui::tokens::window as window_tokens;

#[allow(
    clippy::cast_lossless,
    reason = "winit's LogicalSize takes f64; tokens are f32 logical pixels — the lift is safe"
)]
const DEFAULT_LOGICAL_W: f64 = window_tokens::SIZE_DEFAULT_W as f64;
#[allow(clippy::cast_lossless)]
const DEFAULT_LOGICAL_H: f64 = window_tokens::SIZE_DEFAULT_H as f64;
#[allow(clippy::cast_lossless)]
const MIN_LOGICAL_W: f64 = window_tokens::SIZE_MIN_W as f64;
#[allow(clippy::cast_lossless)]
const MIN_LOGICAL_H: f64 = window_tokens::SIZE_MIN_H as f64;

const TITLEBAR_H: f32 = 34.0;
const SUBWAY_H: f32 = 44.0;
const FOOTER_H: f32 = 40.0;

// NodeId allocation. Static IDs first; ranges for dynamic groups follow.
const HOME_BTN: NodeId = 1;
const TAB_ADD: NodeId = 2;
const LAUNCHER_BASE: NodeId = 100; // launcher_node_id(kind_index) = 100 + index
const TAB_BASE: NodeId = 1_000; // tab_node_id(instance) = 1000 + instance.id
const TAB_CLOSE_BASE: NodeId = 2_000; // tab_close_node_id(instance) = 2000 + instance.id

const MACOS_TRAFFIC_RESERVE: f32 = 78.0;

/// Breathing room on each side of the subway's vertical divider — reads
/// as "deliberate divider," not a stuck-on edge against the Home button.
const SUBWAY_DIVIDER_GAP: f32 = 8.0;

// Hero scene layout constants. Mirror the v6 mockup's CSS values; pulled
// up here so the launcher row, tagline, and spacers stop repeating literals.
const HERO_PAD_V: f32 = 56.0;
const HERO_PAD_H: f32 = 40.0;
const HERO_ITEM_GAP: f32 = 28.0;
const LAUNCHER_ITEM_GAP: f32 = 12.0;
const LAUNCHER_PAD_V: f32 = 14.0;
const LAUNCHER_PAD_H: f32 = 16.0;
const LAUNCHER_BODY_GAP: f32 = 14.0;
const LAUNCHER_MIN_W: f32 = 360.0;
const LAUNCHER_ICON_BOX: f32 = 28.0;
const TAGLINE_MAX_W: f32 = 420.0;
const TAGLINE_LINE_MULT: f32 = 1.5;
const STUB_ITEM_GAP: f32 = 14.0;
const LOGO_HERO_SIZE: f32 = 64.0;
const FOOTER_CHIP_GAP: f32 = 7.0;
const FOOTER_ROW_GAP: f32 = 22.0;

// Subway tab geometry. Constants drive both the layout AND the intrinsic
// width calculation, so a literal change in one place can't drift from the
// recompute.
const TAB_PAD_L: f32 = 10.0;
const TAB_PAD_R: f32 = 12.0;
const TAB_GAP: f32 = 9.0;
const TAB_RAIL_W: f32 = 2.0;
const TAB_RAIL_H: f32 = 16.0;
const TAB_DOT_DIAMETER: f32 = 7.0;
/// `×` close button outer box. Matches `add_tab_button`'s 28×28 and the
/// Home button's 28-tall band so the three subway controls share one
/// horizontal centerline. Above `tokens::hit::MIN` from `design-scaling.md`
/// §4 (24px WCAG 2.5.8 floor).
const TAB_CLOSE_BOX: f32 = 28.0;
const TAB_OUTER_H: f32 = 28.0;
/// Fixed chrome contribution to a tab's width: padding + rail + dot + close
/// box + the four inter-child gaps. The dynamic part is `name_w + kind_w`.
const TAB_CHROME_W: f32 =
    TAB_PAD_L + TAB_RAIL_W + TAB_DOT_DIAMETER + TAB_CLOSE_BOX + 4.0 * TAB_GAP + TAB_PAD_R;

/// Floor on the flexible trailing spacer inside a launcher button — keeps
/// the kbd-chip-shaped reserve from collapsing flush against the body when
/// `LAUNCHER_MIN_W` doesn't already absorb the slack.
const LAUNCHER_TRAILING_MIN: f32 = 100.0;

/// Pre-folded chrome width for a launcher button (everything except the body).
const LAUNCHER_CHROME_W: f32 =
    2.0 * LAUNCHER_PAD_H + LAUNCHER_ICON_BOX + 2.0 * LAUNCHER_BODY_GAP + LAUNCHER_TRAILING_MIN;
/// Pre-folded outer height for a launcher button.
const LAUNCHER_OUTER_H: f32 = 2.0 * LAUNCHER_PAD_V + LAUNCHER_ICON_BOX;
/// Hero column's fixed contribution to content height: logo + 4 inter-item gaps.
const HERO_FIXED_H: f32 = LOGO_HERO_SIZE + 4.0 * HERO_ITEM_GAP;
/// Stub scene's fixed contribution to content height: 2 inter-item gaps.
const STUB_GAPS_H: f32 = 2.0 * STUB_ITEM_GAP;

const LOGO_PNG: &[u8] = include_bytes!("../../../assets/januas-ade-logo.png");

const TAGLINE: &str = "Pick a module to spin up a workspace, or open one from the tab strip above.";
const WORDMARK: &str = "Januas";
const META_TEMPLATE: &str = "v0.0.0  ·  slice s5.5e  ·  ~340 fps";

/// One footer hotkey chip. `kbd` is the chord glyphs, `label` the action.
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
        kbd: "⌘ T",
        label: "new tab",
    },
    Hotkey {
        kbd: "⌘ W",
        label: "close tab",
    },
    Hotkey {
        kbd: "⌘ K",
        label: "command palette",
    },
    Hotkey {
        kbd: "⌘ ,",
        label: "settings",
    },
];

// ===== Helpers =====

fn flat(color: [u8; 3], alpha: f32) -> RectStyle {
    RectStyle::fill(linear(color, alpha))
}

fn rounded(color: [u8; 3], alpha: f32, radius: f32) -> RectStyle {
    RectStyle::fill(linear(color, alpha)).with_radius(radius)
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
        radii: [radius; 4],
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        shadow_blur: 0.0,
        gradient: None,
    }
}

/// Pair an sRGB triple with an alpha byte. Cheap helper that avoids the
/// hand-rolled `[c[0], c[1], c[2], alpha]` indexing tax at every call site.
const fn with_alpha(rgb: [u8; 3], alpha: u8) -> [u8; 4] {
    [rgb[0], rgb[1], rgb[2], alpha]
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
            color: with_alpha(color, alpha),
            family,
        },
        size,
    )
}

const fn spacer(weight: f32) -> Node {
    Node {
        size: [0.0; 2],
        flex: Some(weight),
        kind: NodeKind::Rect(RectStyle::fill([0.0; 4])),
        id: None,
    }
}

fn divider(viewport_w: f32) -> Node {
    Node::rect([viewport_w, 1.0], flat(palette::CREAM, tokens::CREAM_09_A))
}

const fn tab_node_id(id: InstanceId) -> NodeId {
    TAB_BASE + id.as_u32()
}

const fn tab_close_node_id(id: InstanceId) -> NodeId {
    TAB_CLOSE_BASE + id.as_u32()
}

const fn try_tab_id(node_id: NodeId) -> Option<InstanceId> {
    if node_id >= TAB_BASE && node_id < TAB_CLOSE_BASE {
        Some(InstanceId(node_id - TAB_BASE))
    } else {
        None
    }
}

const fn try_tab_close_id(node_id: NodeId) -> Option<InstanceId> {
    if node_id >= TAB_CLOSE_BASE {
        Some(InstanceId(node_id - TAB_CLOSE_BASE))
    } else {
        None
    }
}

const fn launcher_node_id(kind_index: usize) -> NodeId {
    #[allow(
        clippy::cast_possible_truncation,
        reason = "kind_index < ALL.len() ≪ u32::MAX"
    )]
    let idx = kind_index as u32;
    LAUNCHER_BASE + idx
}

fn try_launcher_kind(node_id: NodeId) -> Option<ModuleKind> {
    if (LAUNCHER_BASE..TAB_BASE).contains(&node_id) {
        let idx = (node_id - LAUNCHER_BASE) as usize;
        ModuleKind::ALL.get(idx).copied()
    } else {
        None
    }
}

fn title_for(selection: Selection, set: &InstanceSet) -> String {
    match selection {
        Selection::Home => "JANUAS · HOME".to_string(),
        Selection::Tab(id) => set.get(id).map_or_else(
            || "JANUAS".to_string(),
            |inst| format!("JANUAS · {}", inst.name.to_uppercase()),
        ),
    }
}

// ===== Titlebar =====

fn titlebar(renderer: &mut Renderer, logo: ImageId, title: &str) -> Node {
    let logo_node = Node::image([14.0, 14.0], ImageStyle::new(logo).with_radius(3.0));
    let title_node = text_node(
        renderer,
        title,
        TextFamily::Mono,
        fonts::SMALL_PX,
        tokens::TEXT_DIM,
        0xff,
    );
    let title_w = title_node.size[0];

    let center = Node::stack(
        [14.0 + 8.0 + title_w, 14.0],
        Stack::row()
            .with_gap(8.0)
            .with_cross_align(CrossAlign::Center)
            .with_children([logo_node, title_node]),
    );

    // Reserve at both ends for the macOS native traffic-light cluster
    // (overlays our content) so the title stays visually centered.
    let traffic_reserve = || Node::rect([MACOS_TRAFFIC_RESERVE, 1.0], RectStyle::fill([0.0; 4]));
    let row = Stack::row()
        .with_padding(EdgeInsets::symmetric(0.0, 12.0))
        .with_cross_align(CrossAlign::Center)
        .with_children([
            traffic_reserve(),
            spacer(1.0),
            center,
            spacer(1.0),
            traffic_reserve(),
        ]);
    Node::stack([0.0, TITLEBAR_H], row)
}

// ===== Subway: Home button + divider + dynamic tabs + add =====

/// Home button — accent-colored `⌂` glyph + "Home" label. The icon comes
/// from the system mono font (Unicode U+2302), so it ships sharp at any
/// scale through the existing cosmic-text atlas without a PNG render.
/// Matches the `▦`/`▮` style of the launcher icons.
fn home_button(renderer: &mut Renderer, hovered: bool, active: bool) -> Node {
    let bg = if active {
        rounded(tokens::SURFACE_PANEL, 1.0, radii::MD)
    } else if hovered {
        rounded(tokens::SURFACE_RAISED, 1.0, radii::MD)
    } else {
        rounded(tokens::SURFACE_BASE, 0.0, radii::MD)
    };

    let icon = text_node(renderer, "⌂", TextFamily::Mono, 15.0, tokens::ACCENT, 0xff);
    let icon_w = icon.size[0];
    let label = text_node(
        renderer,
        "Home",
        TextFamily::Body,
        fonts::BODY_PX,
        if active {
            tokens::TEXT
        } else {
            tokens::TEXT_DIM
        },
        0xff,
    );
    let label_w = label.size[0];

    let inner = Stack::row()
        .with_gap(8.0)
        .with_padding(EdgeInsets {
            top: 0.0,
            right: 12.0,
            bottom: 0.0,
            left: 10.0,
        })
        .with_cross_align(CrossAlign::Center)
        .with_background(bg)
        .with_children([icon, label]);
    Node::stack([10.0 + icon_w + 8.0 + label_w + 12.0, 28.0], inner).with_id(HOME_BTN)
}

fn tab_node(
    renderer: &mut Renderer,
    inst: &ModuleInstance,
    hovered: bool,
    active: bool,
    close_hovered: bool,
) -> Node {
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
    let kind_label = inst.kind.name();
    let kind_text = text_node(
        renderer,
        kind_label,
        TextFamily::Mono,
        fonts::FOOTER_PX,
        tokens::TEXT_FAINT,
        0xff,
    );
    let name = text_node(
        renderer,
        &inst.name,
        TextFamily::Body,
        fonts::BODY_PX,
        name_color,
        0xff,
    );
    let kind_w = kind_text.size[0];
    let name_w = name.size[0];

    let rail_style = if active {
        rounded(tokens::ACCENT, 1.0, 1.0)
    } else {
        flat([0; 3], 0.0)
    };
    let rail = Node::rect([TAB_RAIL_W, TAB_RAIL_H], rail_style);
    let dot = Node::rect(
        [TAB_DOT_DIAMETER, TAB_DOT_DIAMETER],
        rounded(inst.color, 1.0, TAB_DOT_DIAMETER * 0.5),
    );
    let close = close_button(renderer, inst.id, close_hovered);

    let content = Stack::row()
        .with_gap(TAB_GAP)
        .with_padding(EdgeInsets {
            top: 0.0,
            right: TAB_PAD_R,
            bottom: 0.0,
            left: TAB_PAD_L,
        })
        .with_cross_align(CrossAlign::Center)
        .with_background(bg)
        .with_children([rail, dot, name, kind_text, close]);

    let intrinsic_w = TAB_CHROME_W + name_w + kind_w;
    Node::stack([intrinsic_w, TAB_OUTER_H], content).with_id(tab_node_id(inst.id))
}

/// `×` close button — 28×28 rounded squircle, transparent at rest, with a
/// `SURFACE_RAISED` background on hover (matches `add_tab_button` and
/// `home_button` so all three subway controls share one horizontal
/// centerline). Painted after the tab body so its hit-zone wins under the
/// reverse-painter hit-test.
fn close_button(renderer: &mut Renderer, instance: InstanceId, hovered: bool) -> Node {
    let bg = if hovered {
        rounded(tokens::SURFACE_RAISED, 1.0, radii::MD)
    } else {
        rounded(tokens::SURFACE_BASE, 0.0, radii::MD)
    };
    let glyph_color = if hovered {
        tokens::TEXT
    } else {
        tokens::TEXT_FAINT
    };
    let glyph = text_node(renderer, "×", TextFamily::Mono, 13.0, glyph_color, 0xff);
    let [gw, gh] = glyph.size;
    let centered = Node::stack(
        [gw, gh],
        Stack::column()
            .with_cross_align(CrossAlign::Center)
            .with_children([spacer(1.0), glyph, spacer(1.0)]),
    );
    let inner = Stack::row()
        .with_cross_align(CrossAlign::Center)
        .with_background(bg)
        .with_children([spacer(1.0), centered, spacer(1.0)]);
    Node::stack([TAB_CLOSE_BOX, TAB_CLOSE_BOX], inner).with_id(tab_close_node_id(instance))
}

fn add_tab_button(renderer: &mut Renderer, hovered: bool) -> Node {
    let bg = if hovered {
        rounded(tokens::SURFACE_RAISED, 1.0, radii::MD)
    } else {
        rounded(tokens::SURFACE_BASE, 0.0, radii::MD)
    };
    let glyph_color = if hovered {
        tokens::TEXT_DIM
    } else {
        tokens::TEXT_FAINT
    };
    let glyph = text_node(renderer, "+", TextFamily::Mono, 15.0, glyph_color, 0xff);
    let [gw, gh] = glyph.size;
    let centered = Node::stack(
        [gw, gh],
        Stack::column()
            .with_cross_align(CrossAlign::Center)
            .with_children([spacer(1.0), glyph, spacer(1.0)]),
    );
    let inner = Stack::row()
        .with_cross_align(CrossAlign::Center)
        .with_background(bg)
        .with_children([spacer(1.0), centered, spacer(1.0)]);
    Node::stack([28.0, 28.0], inner).with_id(TAB_ADD)
}

fn subway(renderer: &mut Renderer, set: &InstanceSet, pointer: &PointerState) -> Node {
    let selection = set.selected();
    let home_active = selection == Selection::Home;
    let home_hovered = pointer.hovered == Some(HOME_BTN);
    let gap = || Node::rect([SUBWAY_DIVIDER_GAP, 1.0], RectStyle::fill([0.0; 4]));
    let v_divider = Node::rect([1.0, 20.0], flat(palette::CREAM, tokens::CREAM_05_A));

    // Materialize each section before chaining — `renderer` is `&mut` and
    // can only be in one closure at a time.
    let home = home_button(renderer, home_hovered, home_active);
    let tabs: Vec<Node> = set
        .ordered()
        .map(|inst| {
            let active = matches!(selection, Selection::Tab(id) if id == inst.id);
            let hovered = pointer.hovered == Some(tab_node_id(inst.id));
            let close_hovered = pointer.hovered == Some(tab_close_node_id(inst.id));
            tab_node(renderer, inst, hovered, active, close_hovered)
        })
        .collect();
    let add = add_tab_button(renderer, pointer.hovered == Some(TAB_ADD));

    let head = [home, gap(), v_divider, gap()];
    let tail = [add, spacer(1.0)];
    let children = head.into_iter().chain(tabs).chain(tail);

    let row = Stack::row()
        .with_gap(2.0)
        .with_padding(EdgeInsets::symmetric(8.0, 12.0))
        .with_cross_align(CrossAlign::Center)
        .with_children(children);
    Node::stack([0.0, SUBWAY_H], row)
}

// ===== Hero scenes =====

fn launcher_button(
    renderer: &mut Renderer,
    kind: ModuleKind,
    kind_index: usize,
    hovered: bool,
) -> Node {
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
        kind.icon(),
        TextFamily::Mono,
        15.0,
        tokens::ACCENT,
        0xff,
    );
    let icon_box = Node::stack(
        [LAUNCHER_ICON_BOX, LAUNCHER_ICON_BOX],
        Stack::row()
            .with_cross_align(CrossAlign::Center)
            .with_children([spacer(1.0), icon, spacer(1.0)]),
    );

    let name = text_node(
        renderer,
        kind.name(),
        TextFamily::Body,
        14.0,
        tokens::TEXT,
        0xff,
    );
    let sub = text_node(
        renderer,
        kind.description(),
        TextFamily::Mono,
        fonts::MODE_SUB_PX,
        tokens::TEXT_DIM,
        0xff,
    );
    let body_w = name.size[0].max(sub.size[0]);
    let body_h = name.size[1] + 2.0 + sub.size[1];
    let body = Node::stack(
        [body_w, body_h],
        Stack::column().with_gap(2.0).with_children([name, sub]),
    );

    let outer_w = (LAUNCHER_CHROME_W + body_w).max(LAUNCHER_MIN_W);

    let row = Stack::row()
        .with_gap(LAUNCHER_BODY_GAP)
        .with_padding(EdgeInsets::symmetric(LAUNCHER_PAD_V, LAUNCHER_PAD_H))
        .with_cross_align(CrossAlign::Center)
        .with_background(bg)
        .with_children([icon_box, body, spacer(1.0)]);
    Node::stack([outer_w, LAUNCHER_OUTER_H], row).with_id(launcher_node_id(kind_index))
}

fn hero_home(renderer: &mut Renderer, logo: ImageId, pointer: &PointerState) -> Node {
    let logo_node = Node::image(
        [LOGO_HERO_SIZE, LOGO_HERO_SIZE],
        ImageStyle::new(logo).with_radius(radii::LOGO),
    );
    let wordmark = text_node(
        renderer,
        WORDMARK,
        TextFamily::Display,
        fonts::WORDMARK_PX,
        tokens::TEXT,
        0xff,
    );
    let meta = text_node(
        renderer,
        META_TEMPLATE,
        TextFamily::Mono,
        fonts::FOOTER_PX,
        tokens::TEXT_FAINT,
        0xff,
    );

    let tagline = hero_tagline(renderer);
    let tagline_h = tagline.size[1];
    let wordmark_h = wordmark.size[1];
    let meta_h = meta.size[1];

    let launchers: Vec<Node> = ModuleKind::ALL
        .iter()
        .copied()
        .enumerate()
        .map(|(idx, kind)| {
            let hovered = pointer.hovered == Some(launcher_node_id(idx));
            launcher_button(renderer, kind, idx, hovered)
        })
        .collect();
    let launcher_h: f32 = launchers.iter().map(|n| n.size[1]).sum();
    let launcher_w: f32 = launchers.iter().map(|n| n.size[0]).fold(0.0, f32::max);
    #[allow(
        clippy::cast_precision_loss,
        reason = "ModuleKind::ALL is single-digit at v0.1"
    )]
    let launcher_gaps = LAUNCHER_ITEM_GAP * launchers.len().saturating_sub(1) as f32;
    let launcher_stack_h = launcher_h + launcher_gaps;
    let launcher_stack = Node::stack(
        [launcher_w, launcher_stack_h],
        Stack::column()
            .with_gap(LAUNCHER_ITEM_GAP)
            .with_children(launchers),
    );

    let column = Stack::column()
        .with_gap(HERO_ITEM_GAP)
        .with_cross_align(CrossAlign::Center)
        .with_children([logo_node, wordmark, meta, tagline, launcher_stack]);

    let content_h = HERO_FIXED_H + wordmark_h + meta_h + tagline_h + launcher_stack_h;
    let content = Node::stack([launcher_w, content_h], column);
    hero_wrapper(content)
}

fn hero_tagline(renderer: &mut Renderer) -> Node {
    let line_height = fonts::TAGLINE_PX * TAGLINE_LINE_MULT;
    let natural = renderer.measure_text(TextFamily::Body, TAGLINE, fonts::TAGLINE_PX, line_height);
    let wraps = natural[0] > TAGLINE_MAX_W;
    let width = natural[0].min(TAGLINE_MAX_W);
    let height = if wraps {
        line_height * 2.0
    } else {
        line_height
    };
    Node::text_sized(
        TAGLINE,
        TextStyle {
            font_size: fonts::TAGLINE_PX,
            line_height,
            color: with_alpha(tokens::TEXT_DIM, 0xff),
            family: TextFamily::Body,
        },
        [width, height],
    )
}

fn hero_wrapper(content: Node) -> Node {
    let wrapper = Stack::column()
        .with_padding(EdgeInsets::symmetric(HERO_PAD_V, HERO_PAD_H))
        .with_cross_align(CrossAlign::Center)
        .with_children([spacer(1.0), content, spacer(1.0)]);
    Node {
        size: [0.0; 2],
        flex: Some(1.0),
        kind: NodeKind::Stack(wrapper),
        id: None,
    }
}

fn hero_tab_stub(renderer: &mut Renderer, inst: &ModuleInstance) -> Node {
    let header = text_node(
        renderer,
        &format!("Module · {}", inst.kind.name()),
        TextFamily::Mono,
        fonts::FOOTER_PX,
        tokens::TEXT_FAINT,
        0xff,
    );
    let name = text_node(
        renderer,
        &inst.name,
        TextFamily::Display,
        fonts::WORDMARK_PX,
        tokens::TEXT,
        0xff,
    );
    let stub = text_node(
        renderer,
        "stub scene — Parallex grid lands at S6",
        TextFamily::Mono,
        fonts::MODE_SUB_PX,
        tokens::TEXT_DIM,
        0xff,
    );

    let items = [header, name, stub];
    let total_h: f32 = items.iter().map(|n| n.size[1]).sum();
    let max_w: f32 = items.iter().map(|n| n.size[0]).fold(150.0, f32::max);

    let content = Node::stack(
        [max_w, total_h + STUB_GAPS_H],
        Stack::column()
            .with_gap(STUB_ITEM_GAP)
            .with_cross_align(CrossAlign::Center)
            .with_children(items),
    );
    hero_wrapper(content)
}

fn hero(renderer: &mut Renderer, logo: ImageId, set: &InstanceSet, pointer: &PointerState) -> Node {
    match set.selected() {
        Selection::Home => hero_home(renderer, logo, pointer),
        Selection::Tab(id) => match set.get(id) {
            Some(inst) => hero_tab_stub(renderer, inst),
            None => hero_home(renderer, logo, pointer),
        },
    }
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
    let [label_w, label_h] = kbd_label.size;
    let kbd = Node::stack(
        [label_w + 10.0, label_h + 2.0],
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
            .with_children([kbd_label]),
    );
    let label = text_node(
        renderer,
        chip.label,
        TextFamily::Mono,
        fonts::FOOTER_PX,
        tokens::TEXT_FAINT,
        0xff,
    );
    let [k_w, k_h] = kbd.size;
    let [l_w, l_h] = label.size;
    Node::stack(
        [k_w + FOOTER_CHIP_GAP + l_w, k_h.max(l_h)],
        Stack::row()
            .with_gap(FOOTER_CHIP_GAP)
            .with_cross_align(CrossAlign::Center)
            .with_children([kbd, label]),
    )
}

fn footer(renderer: &mut Renderer) -> Node {
    let chips = HOTKEYS.iter().map(|chip| footer_chip(renderer, chip));
    let children = std::iter::once(spacer(1.0))
        .chain(chips)
        .chain(std::iter::once(spacer(1.0)));
    let row = Stack::row()
        .with_gap(FOOTER_ROW_GAP)
        .with_padding(EdgeInsets::all(14.0))
        .with_cross_align(CrossAlign::Center)
        .with_background(flat(tokens::SURFACE_RAISED, 1.0))
        .with_children(children);
    Node::stack([0.0, FOOTER_H], row)
}

// ===== Root =====

fn build_root(
    renderer: &mut Renderer,
    logo: ImageId,
    viewport: [f32; 2],
    set: &InstanceSet,
    pointer: &PointerState,
) -> Stack {
    let title = title_for(set.selected(), set);
    Stack::column()
        .with_background(flat(tokens::SURFACE_BASE, 1.0))
        .with_children([
            titlebar(renderer, logo, &title),
            divider(viewport[0]),
            subway(renderer, set, pointer),
            divider(viewport[0]),
            hero(renderer, logo, set, pointer),
            divider(viewport[0]),
            footer(renderer),
        ])
}

// ===== App wiring =====

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
    set: InstanceSet,
    scale: f32,
    physical_size: [u32; 2],
    hit_zones: Vec<HitZone>,
    logo: Option<ImageId>,
}

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
            &self.set,
            &self.pointer,
        );
        let mut frame = januas_ui::layout(&root, [logical_w, logical_h]);
        frame.scale_by(self.scale);
        renderer.set_rects(&frame.rects);
        renderer.set_text_runs(&frame.texts);
        renderer.set_images(&frame.images);
        self.hit_zones = frame.hit_zones;
    }

    fn handle_click(&mut self, id: NodeId) {
        let Some(action) = classify_click(id) else {
            return;
        };
        if self.apply(action) {
            self.rebuild_and_upload();
        }
    }

    /// Apply a click action to set state. Returns `true` when something
    /// actually changed (cheap-out on identity clicks so we don't repaint
    /// for no reason).
    fn apply(&mut self, action: ClickAction) -> bool {
        match action {
            ClickAction::SelectHome => {
                if self.set.selected() == Selection::Home {
                    return false;
                }
                let _ = self.set.select(Selection::Home);
                true
            }
            ClickAction::SpawnAndSelect(kind) => {
                let id = self.set.spawn(kind);
                let _ = self.set.select(Selection::Tab(id));
                true
            }
            ClickAction::SelectTab(id) => {
                if self.set.selected() == Selection::Tab(id) {
                    return false;
                }
                let _ = self.set.select(Selection::Tab(id));
                true
            }
            ClickAction::CloseTab(id) => self.set.close(id).is_ok(),
        }
    }
}

/// Discriminated meaning of a click on a hit-tested [`NodeId`].
///
/// Separates "what part of the UI was clicked" from "what should happen to
/// state" so the dispatch table reads as an exhaustive match instead of an
/// `if`-chain with `return`s.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum ClickAction {
    /// Return to the Home view.
    SelectHome,
    /// Spawn a fresh instance of the given module and select its new tab.
    /// Drives both the `+` button (defaulting to the first registered kind
    /// at v0.1) and the Home-scene launcher buttons.
    SpawnAndSelect(ModuleKind),
    /// Switch the selection to an existing tab.
    SelectTab(InstanceId),
    /// Close an existing tab. Selection snaps to Home if the closed tab
    /// was the active one (`InstanceSet::close` enforces that).
    CloseTab(InstanceId),
}

fn classify_click(id: NodeId) -> Option<ClickAction> {
    // Close zone wins over the tab zone via reverse-painter hit-test, but
    // belt-and-suspenders the discriminator here too so dispatch is
    // self-describing.
    if let Some(instance) = try_tab_close_id(id) {
        return Some(ClickAction::CloseTab(instance));
    }
    if id == HOME_BTN {
        Some(ClickAction::SelectHome)
    } else if id == TAB_ADD {
        // v0.1: only one module registered, so `+` is a one-click spawn.
        // S5.5e.5 will swap this for a ModuleKind dropdown menu.
        ModuleKind::ALL
            .first()
            .copied()
            .map(ClickAction::SpawnAndSelect)
    } else if let Some(kind) = try_launcher_kind(id) {
        Some(ClickAction::SpawnAndSelect(kind))
    } else {
        try_tab_id(id).map(ClickAction::SelectTab)
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
            ))
            // Floor from `design-scaling.md` §6. Below 720×480 the home
            // scene cannot fit chrome + hero + footer without overlap;
            // the OS window manager refuses drag-resize below this.
            .with_min_inner_size(winit::dpi::LogicalSize::new(MIN_LOGICAL_W, MIN_LOGICAL_H));
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
        self.set = InstanceSet::new();
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
                    self.handle_click(id);
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
