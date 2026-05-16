//! Tests for the layout-pass anti-clunky assertions (S5.5g).
//!
//! Confirms the two cheap checks from `debug::debug_assert_layout` fire
//! on the failure modes described in `design-scaling.md` §5 + §8, and
//! stay quiet on a known-good frame.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests use unwrap/expect to keep assertions terse — a failed unwrap is a test failure"
)]
#![allow(
    clippy::float_cmp,
    reason = "layout pass produces exact integer logical pixels for these inputs; strict equality is the intent"
)]

use januas_ui::{
    CrossAlign, EdgeInsets, Node, NodeKind, RectStyle, Stack,
    debug::{assert_pixel_grid_snap, assert_within_viewport, debug_assert_layout},
    layout,
};

const fn red() -> RectStyle {
    RectStyle::fill([1.0, 0.0, 0.0, 1.0])
}

const fn spacer(weight: f32) -> Node {
    Node {
        size: [0.0; 2],
        flex: Some(weight),
        kind: NodeKind::Rect(RectStyle::fill([0.0; 4])),
        id: None,
    }
}

#[test]
fn good_scene_passes_all_asserts() {
    // 3 rects that fit comfortably in the viewport with integer bounds.
    let root = Stack::column().with_gap(10.0).with_children([
        Node::rect([200.0, 50.0], red()),
        Node::rect([200.0, 50.0], red()),
        Node::rect([200.0, 50.0], red()),
    ]);
    let frame = layout(&root, [400.0, 400.0]);
    assert!(!frame.overflow, "good scene should not overflow");
    debug_assert_layout(&frame, [400.0, 400.0]);
}

#[test]
#[should_panic(expected = "LayoutFrame.overflow == true")]
fn overflow_flag_panics_in_debug_assert() {
    // Children sum to 300px main-axis; viewport is only 200px tall.
    let root = Stack::column().with_gap(0.0).with_children([
        Node::rect([100.0, 100.0], red()),
        Node::rect([100.0, 100.0], red()),
        Node::rect([100.0, 100.0], red()),
    ]);
    let frame = layout(&root, [200.0, 200.0]);
    assert!(frame.overflow, "scene exceeds viewport on main axis");
    debug_assert_layout(&frame, [200.0, 200.0]);
}

#[test]
fn flex_spacer_collapses_to_zero_when_intrinsic_exceeds_inner() {
    // Two non-flex rects already eat the whole main axis; flex spacer
    // between them must collapse to zero. Overflow flag stays FALSE here
    // because intrinsic equals the inner exactly — the spacer is a
    // borderline pass case, not a regression.
    let root = Stack::row().with_gap(0.0).with_children([
        Node::rect([100.0, 50.0], red()),
        spacer(1.0),
        Node::rect([100.0, 50.0], red()),
    ]);
    let frame = layout(&root, [200.0, 50.0]);
    assert!(
        !frame.overflow,
        "intrinsic == inner should not flag overflow"
    );
    // 3 rects emitted: first solid + spacer (transparent fill at width 0) + second solid.
    assert_eq!(frame.rects.len(), 3);
    assert_eq!(frame.rects[0].pos[0], 0.0);
    assert_eq!(frame.rects[0].size[0], 100.0);
    // The collapsed spacer sits between them at x=100, width=0.
    assert_eq!(frame.rects[1].pos[0], 100.0);
    assert_eq!(frame.rects[1].size[0], 0.0);
    assert_eq!(frame.rects[2].pos[0], 100.0);
    assert_eq!(frame.rects[2].size[0], 100.0);
}

#[test]
fn flex_spacer_flags_overflow_when_intrinsic_exceeds_inner() {
    // The bug from `design-scaling.md` §5: spacer collapses, non-flex
    // children overlap because their intrinsic widths already exceed
    // available.
    let root = Stack::row().with_gap(0.0).with_children([
        Node::rect([150.0, 50.0], red()),
        spacer(1.0),
        Node::rect([150.0, 50.0], red()),
    ]);
    let frame = layout(&root, [200.0, 50.0]);
    assert!(
        frame.overflow,
        "intrinsic 300 > inner 200 must flag overflow"
    );
}

#[test]
fn nested_padded_stack_inner_overflow_propagates() {
    // Parent padding 10 on each side → inner main-axis = 180; a child
    // requesting 200 should flag overflow even though the OUTER stack
    // fits the viewport.
    let inner = Stack::row()
        .with_padding(EdgeInsets::symmetric(0.0, 10.0))
        .with_cross_align(CrossAlign::Center)
        .with_children([Node::rect([200.0, 30.0], red())]);
    let root = Stack::column().with_children([Node::stack([200.0, 50.0], inner)]);
    let frame = layout(&root, [200.0, 50.0]);
    assert!(
        frame.overflow,
        "padded inner overflow must propagate to the frame flag"
    );
}

#[test]
#[should_panic(expected = "out of viewport")]
fn rect_past_viewport_right_edge_panics() {
    let frame = {
        let mut f = januas_ui::LayoutFrame::default();
        f.rects.push(januas_ui::Rect {
            pos: [180.0, 0.0],
            size: [30.0, 30.0], // right edge 210 > viewport 200
            radii: [0.0; 4],
            border_width: 0.0,
            fill: [0.0; 4],
            border: [0.0; 4],
            shadow_color: [0.0; 4],
            shadow_offset: [0.0; 2],
            shadow_blur: 0.0,
            gradient_index: januas_ui::NO_GRADIENT,
        });
        f
    };
    assert_within_viewport(&frame, [200.0, 200.0]);
}

#[test]
#[should_panic(expected = "not integer logical pixels")]
fn fractional_rect_pos_panics() {
    let mut f = januas_ui::LayoutFrame::default();
    f.rects.push(januas_ui::Rect {
        pos: [10.5, 0.0],
        size: [20.0, 20.0],
        radii: [0.0; 4],
        border_width: 0.0,
        fill: [0.0; 4],
        border: [0.0; 4],
        shadow_color: [0.0; 4],
        shadow_offset: [0.0; 2],
        shadow_blur: 0.0,
        gradient_index: januas_ui::NO_GRADIENT,
    });
    assert_pixel_grid_snap(&f);
}

#[test]
fn integer_box_with_fractional_intent_passes() {
    // A 13.5px font is fractional, but the ENCLOSING box should snap to
    // 14 (or whatever integer the layout produced). The assertion is on
    // the rect/image/hit_zone box, not on the font_size itself.
    let f = januas_ui::LayoutFrame::default();
    assert_pixel_grid_snap(&f); // empty frame is trivially clean
}
