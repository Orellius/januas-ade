//! Januas UI primitives — layout (S5.5b) + pointer/hover state (S5.5c).
//!
//! Purpose: own the pixel-coordinate layout pass that turns a tree of
//! [`Stack`] / [`Node`] descriptions into a flat [`LayoutFrame`] of absolute
//! [`Rect`]s and [`TextRun`]s for the renderer to draw. Children pack along a
//! row/column main axis with a configurable `gap` and optional `flex` weight;
//! padding and cross-axis alignment let panels nest without re-implementing
//! placement math at every call site.
//! Public surface: [`Stack`], [`Node`], [`NodeKind`], [`CrossAlign`],
//! [`Direction`], [`EdgeInsets`], [`RectStyle`], [`TextStyle`],
//! [`LayoutFrame`], [`layout`], [`estimate_text_size`].
//! Why this crate (vs a `renderer` module): renderer owns GPU pipelines;
//! layout is GPU-free pure math that S5.5c (pointer state) and S5.5d (home
//! scene) compose on top of. Keeping them separate also keeps `renderer`
//! free of opinion about how primitives compose.
//! Not responsibilities: text shaping (`cosmic-text` lives in renderer),
//! GPU dispatch (renderer), event handling (`crates/app/`), pointer state
//! machine (lands in this crate at S5.5c but is its own module).
//! Test strategy: `tests/layout.rs` covers the placement invariants (sibling
//! non-overlap, padding inset, flex distribution, cross-axis alignment).
//! Visual correctness is gated by the `home_compose` example at slice close.

pub use januas_renderer::{Rect, TextRun};

mod pointer;

pub use pointer::{HitZone, NodeId, PointerState, hit_test};

/// Stack main-axis direction.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Direction {
    /// Children pack horizontally; main axis is X, cross axis is Y.
    Row,
    /// Children pack vertically; main axis is Y, cross axis is X.
    Column,
}

/// Cross-axis alignment policy for a stack's children.
///
/// `Stretch` (default) sizes each child to fill the stack's inner cross-axis
/// extent. `Start`, `Center`, and `End` respect the child's intrinsic
/// cross-axis size and anchor it to the leading edge, midpoint, or trailing
/// edge of the inner region.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Default)]
pub enum CrossAlign {
    /// Anchor child to the leading edge of the cross axis.
    Start,
    /// Center child along the cross axis.
    Center,
    /// Anchor child to the trailing edge of the cross axis.
    End,
    /// Size child to fill the cross axis. Default.
    #[default]
    Stretch,
}

/// Per-edge inset specification, in pixels.
#[derive(Copy, Clone, Debug, Default, PartialEq)]
pub struct EdgeInsets {
    /// Inset from the top edge in pixels.
    pub top: f32,
    /// Inset from the right edge in pixels.
    pub right: f32,
    /// Inset from the bottom edge in pixels.
    pub bottom: f32,
    /// Inset from the left edge in pixels.
    pub left: f32,
}

impl EdgeInsets {
    /// All four edges share the same inset.
    #[must_use]
    pub const fn all(value: f32) -> Self {
        Self {
            top: value,
            right: value,
            bottom: value,
            left: value,
        }
    }

    /// Vertical insets on top + bottom, horizontal on left + right.
    #[must_use]
    pub const fn symmetric(vertical: f32, horizontal: f32) -> Self {
        Self {
            top: vertical,
            right: horizontal,
            bottom: vertical,
            left: horizontal,
        }
    }
}

/// Visual style for a [`NodeKind::Rect`] or a stack background.
///
/// Colors are linear-space RGBA in `[0.0, 1.0]` (mirrors
/// [`januas_renderer::Rect`]). `border_width == 0.0` disables the border.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct RectStyle {
    /// Linear-space RGBA fill color.
    pub fill: [f32; 4],
    /// Linear-space RGBA border color; ignored when `border_width == 0.0`.
    pub border: [f32; 4],
    /// Inset border thickness in pixels; `0.0` for a borderless rect.
    pub border_width: f32,
    /// Corner radius in pixels.
    pub radius: f32,
    /// Linear-space RGBA shadow color. Alpha `0.0` disables the shadow.
    pub shadow_color: [f32; 4],
    /// Shadow offset in pixels; positive y casts the shadow downward.
    pub shadow_offset: [f32; 2],
    /// Gaussian blur sigma in pixels.
    pub shadow_blur: f32,
}

impl RectStyle {
    /// Flat fill, no border, no rounding, no shadow.
    #[must_use]
    pub const fn fill(color: [f32; 4]) -> Self {
        Self {
            fill: color,
            border: [0.0; 4],
            border_width: 0.0,
            radius: 0.0,
            shadow_color: [0.0; 4],
            shadow_offset: [0.0; 2],
            shadow_blur: 0.0,
        }
    }

    /// Builder: attach a soft drop shadow.
    #[must_use]
    pub const fn with_shadow(mut self, color: [f32; 4], offset: [f32; 2], blur: f32) -> Self {
        self.shadow_color = color;
        self.shadow_offset = offset;
        self.shadow_blur = blur;
        self
    }
}

/// Visual style for a [`NodeKind::Text`] span.
///
/// `color` is sRGB rgba (mirrors `glyphon::Color::rgba`). `font_size` and
/// `line_height` are in surface pixels.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct TextStyle {
    /// Font size in surface pixels.
    pub font_size: f32,
    /// Line height in surface pixels.
    pub line_height: f32,
    /// sRGB rgba — matches `glyphon::Color::rgba`.
    pub color: [u8; 4],
}

/// One node in a layout tree. Carries an intrinsic main-axis size plus an
/// optional `flex` weight that lets the layout pass distribute leftover
/// space proportionally across siblings.
#[derive(Clone, Debug)]
pub struct Node {
    /// Intrinsic size in pixels. Used for the main-axis when `flex` is `None`,
    /// and for the cross-axis under [`CrossAlign::Start`] / `Center` / `End`.
    pub size: [f32; 2],
    /// When set, the child grows along the main axis to fill leftover space
    /// proportionally to other flex siblings; its intrinsic main-axis size is
    /// ignored. `None` means the child uses its intrinsic main-axis size.
    pub flex: Option<f32>,
    /// The drawable payload — rect, text span, or nested stack.
    pub kind: NodeKind,
    /// Optional hit-test identity. When set, the layout pass emits a
    /// [`HitZone`] covering this node's resolved bounds, so the pointer
    /// state machine can map cursor coordinates back to this node.
    pub id: Option<NodeId>,
}

/// Discriminated payload for a [`Node`].
#[derive(Clone, Debug)]
pub enum NodeKind {
    /// A rounded rectangle drawn through the renderer's rect pipeline.
    Rect(RectStyle),
    /// A positioned text span drawn through glyphon.
    Text(TextStyle, String),
    /// A nested layout subtree.
    Stack(Stack),
}

impl Node {
    /// Convenience constructor for a rect node with no flex.
    #[must_use]
    pub const fn rect(size: [f32; 2], style: RectStyle) -> Self {
        Self {
            size,
            flex: None,
            kind: NodeKind::Rect(style),
            id: None,
        }
    }

    /// Convenience constructor for a text node sized from
    /// [`estimate_text_size`].
    #[must_use]
    pub fn text(content: impl Into<String>, style: TextStyle) -> Self {
        let content: String = content.into();
        let size = estimate_text_size(&content, style.font_size, style.line_height);
        Self {
            size,
            flex: None,
            kind: NodeKind::Text(style, content),
            id: None,
        }
    }

    /// Convenience constructor for a nested stack node sized to its
    /// intrinsic content (the caller still owns sizing — pass `size` of
    /// the stack's expected outer extent).
    #[must_use]
    pub const fn stack(size: [f32; 2], stack: Stack) -> Self {
        Self {
            size,
            flex: None,
            kind: NodeKind::Stack(stack),
            id: None,
        }
    }

    /// Builder: mark this node as a flex child with the given weight.
    #[must_use]
    pub const fn with_flex(mut self, weight: f32) -> Self {
        self.flex = Some(weight);
        self
    }

    /// Builder: tag this node with a hit-test [`NodeId`]. The layout pass
    /// emits a [`HitZone`] at the node's resolved bounds so pointer events
    /// can map back to this node.
    #[must_use]
    pub const fn with_id(mut self, id: NodeId) -> Self {
        self.id = Some(id);
        self
    }
}

/// A row or column of [`Node`]s with gap, padding, and optional background.
#[derive(Clone, Debug)]
pub struct Stack {
    /// Main-axis direction.
    pub direction: Direction,
    /// Gap between consecutive children along the main axis, in pixels.
    pub gap: f32,
    /// Inner inset applied before child placement.
    pub padding: EdgeInsets,
    /// Cross-axis alignment policy for children.
    pub cross_align: CrossAlign,
    /// Drawn at the stack's outer bounds before any children. `None` leaves
    /// the surface beneath the stack untouched.
    pub background: Option<RectStyle>,
    /// Children placed in declaration order along the main axis.
    pub children: Vec<Node>,
}

impl Stack {
    /// Empty horizontal stack.
    #[must_use]
    pub fn row() -> Self {
        Self {
            direction: Direction::Row,
            gap: 0.0,
            padding: EdgeInsets::default(),
            cross_align: CrossAlign::default(),
            background: None,
            children: Vec::new(),
        }
    }

    /// Empty vertical stack.
    #[must_use]
    pub fn column() -> Self {
        Self {
            direction: Direction::Column,
            gap: 0.0,
            padding: EdgeInsets::default(),
            cross_align: CrossAlign::default(),
            background: None,
            children: Vec::new(),
        }
    }

    /// Builder: set the main-axis gap between siblings.
    #[must_use]
    pub const fn with_gap(mut self, gap: f32) -> Self {
        self.gap = gap;
        self
    }

    /// Builder: set the inner padding insets.
    #[must_use]
    pub const fn with_padding(mut self, padding: EdgeInsets) -> Self {
        self.padding = padding;
        self
    }

    /// Builder: set the cross-axis alignment policy.
    #[must_use]
    pub const fn with_cross_align(mut self, align: CrossAlign) -> Self {
        self.cross_align = align;
        self
    }

    /// Builder: draw the given rect at the stack's outer bounds.
    #[must_use]
    pub const fn with_background(mut self, style: RectStyle) -> Self {
        self.background = Some(style);
        self
    }

    /// Builder: replace the child list.
    #[must_use]
    pub fn with_children(mut self, children: Vec<Node>) -> Self {
        self.children = children;
        self
    }
}

/// Output of a [`layout`] pass: a flat list of absolutely-positioned rects
/// and text runs, ready to feed [`januas_renderer::Renderer::set_rects`] and
/// [`januas_renderer::Renderer::set_text_runs`].
#[derive(Clone, Debug, Default)]
pub struct LayoutFrame {
    /// Absolutely-positioned rects in declaration order.
    pub rects: Vec<Rect>,
    /// Absolutely-positioned text runs in declaration order.
    pub texts: Vec<TextRun>,
    /// Hit-test zones in declaration order — later entries paint on top of
    /// earlier ones, so reverse-iterate to find the topmost hit.
    pub hit_zones: Vec<HitZone>,
}

impl LayoutFrame {
    /// Multiply every position, size, font metric, and hit-zone bound by
    /// `scale`. Used to convert a logical-pixel layout into the physical
    /// pixels the renderer surface uses on a `HiDPI` display.
    pub fn scale_by(&mut self, scale: f32) {
        for r in &mut self.rects {
            r.pos[0] *= scale;
            r.pos[1] *= scale;
            r.size[0] *= scale;
            r.size[1] *= scale;
            r.radius *= scale;
            r.border_width *= scale;
            r.shadow_offset[0] *= scale;
            r.shadow_offset[1] *= scale;
            r.shadow_blur *= scale;
        }
        for t in &mut self.texts {
            t.pos[0] *= scale;
            t.pos[1] *= scale;
            t.font_size *= scale;
            t.line_height *= scale;
        }
        for z in &mut self.hit_zones {
            z.pos[0] *= scale;
            z.pos[1] *= scale;
            z.size[0] *= scale;
            z.size[1] *= scale;
        }
    }
}

/// Compute absolute pixel positions for every primitive under `root`.
///
/// The root stack occupies the full viewport. Recursion drills into nested
/// stacks; rects and texts emit into the returned [`LayoutFrame`]. Cross-axis
/// behavior follows the stack's [`CrossAlign`]; main-axis flex children
/// share leftover space proportionally to their weight.
#[must_use]
pub fn layout(root: &Stack, viewport: [f32; 2]) -> LayoutFrame {
    let mut frame = LayoutFrame::default();
    place_stack(&mut frame, root, [0.0, 0.0], viewport);
    frame
}

/// Approximate text bounds in pixels for a monospace-ish glyph run.
///
/// Width = max-line-chars × `font_size` × 0.6. Height = line-count ×
/// `line_height`. Good enough for v0.1 layout placement; real shaping-based
/// measurement lives in S5.5d when the home scene needs pixel-accurate
/// alignment against `~/Desktop/claude-html/januas-home-20260516-0947.html`.
#[must_use]
pub fn estimate_text_size(content: &str, font_size: f32, line_height: f32) -> [f32; 2] {
    const APPROX_CHAR_WIDTH_RATIO: f32 = 0.6;
    let mut max_chars: usize = 0;
    let mut lines: usize = 0;
    for line in content.lines() {
        lines += 1;
        let n = line.chars().count();
        if n > max_chars {
            max_chars = n;
        }
    }
    if lines == 0 {
        lines = 1;
    }
    #[allow(
        clippy::cast_precision_loss,
        reason = "char counts and line counts fit comfortably in f32"
    )]
    let w = max_chars as f32 * font_size * APPROX_CHAR_WIDTH_RATIO;
    #[allow(clippy::cast_precision_loss)]
    let h = lines as f32 * line_height;
    [w, h]
}

fn place_stack(frame: &mut LayoutFrame, stack: &Stack, origin: [f32; 2], size: [f32; 2]) {
    if let Some(style) = stack.background {
        frame.rects.push(Rect {
            pos: origin,
            size,
            radius: style.radius,
            border_width: style.border_width,
            fill: style.fill,
            border: style.border,
            shadow_color: style.shadow_color,
            shadow_offset: style.shadow_offset,
            shadow_blur: style.shadow_blur,
        });
    }

    let inner_origin = [
        origin[0] + stack.padding.left,
        origin[1] + stack.padding.top,
    ];
    let inner_size = [
        (size[0] - stack.padding.left - stack.padding.right).max(0.0),
        (size[1] - stack.padding.top - stack.padding.bottom).max(0.0),
    ];

    let (main_axis, cross_axis) = match stack.direction {
        Direction::Row => (0, 1),
        Direction::Column => (1, 0),
    };

    let n = stack.children.len();
    #[allow(
        clippy::cast_precision_loss,
        reason = "child counts fit comfortably in f32"
    )]
    let total_gap = stack.gap * (n.saturating_sub(1)) as f32;
    let mut intrinsic_main: f32 = total_gap;
    let mut total_flex: f32 = 0.0;
    for child in &stack.children {
        if let Some(weight) = child.flex {
            total_flex += weight.max(0.0);
        } else {
            intrinsic_main += child.size[main_axis];
        }
    }
    let leftover = (inner_size[main_axis] - intrinsic_main).max(0.0);

    let mut cursor = inner_origin[main_axis];
    for (i, child) in stack.children.iter().enumerate() {
        let main_extent = match child.flex {
            Some(weight) if total_flex > 0.0 => leftover * (weight.max(0.0) / total_flex),
            _ => child.size[main_axis],
        };
        let cross_extent = match stack.cross_align {
            CrossAlign::Stretch => inner_size[cross_axis],
            CrossAlign::Start | CrossAlign::Center | CrossAlign::End => child.size[cross_axis],
        };
        let cross_offset = match stack.cross_align {
            CrossAlign::Start | CrossAlign::Stretch => 0.0,
            CrossAlign::Center => ((inner_size[cross_axis] - cross_extent) * 0.5).max(0.0),
            CrossAlign::End => (inner_size[cross_axis] - cross_extent).max(0.0),
        };

        let mut child_pos = [0.0_f32; 2];
        child_pos[main_axis] = cursor;
        child_pos[cross_axis] = inner_origin[cross_axis] + cross_offset;
        let mut child_size = [0.0_f32; 2];
        child_size[main_axis] = main_extent;
        child_size[cross_axis] = cross_extent;

        place_node(frame, child, child_pos, child_size);

        cursor += main_extent;
        if i + 1 < n {
            cursor += stack.gap;
        }
    }
}

fn place_node(frame: &mut LayoutFrame, node: &Node, pos: [f32; 2], size: [f32; 2]) {
    if let Some(id) = node.id {
        frame.hit_zones.push(HitZone { id, pos, size });
    }
    match &node.kind {
        NodeKind::Rect(style) => {
            frame.rects.push(Rect {
                pos,
                size,
                radius: style.radius,
                border_width: style.border_width,
                fill: style.fill,
                border: style.border,
                shadow_color: style.shadow_color,
                shadow_offset: style.shadow_offset,
                shadow_blur: style.shadow_blur,
            });
        }
        NodeKind::Text(style, content) => {
            frame.texts.push(TextRun {
                pos,
                content: content.clone(),
                font_size: style.font_size,
                line_height: style.line_height,
                color: style.color,
            });
        }
        NodeKind::Stack(inner) => {
            place_stack(frame, inner, pos, size);
        }
    }
}
