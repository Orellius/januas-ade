//! Layout invariants — sibling non-overlap, padding inset, flex distribution.

use januas_ui::{CrossAlign, EdgeInsets, Node, Rect, RectStyle, Stack, layout};

const fn red() -> RectStyle {
    RectStyle::fill([1.0, 0.0, 0.0, 1.0])
}

fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    let a_right = a.pos[0] + a.size[0];
    let a_bottom = a.pos[1] + a.size[1];
    let b_right = b.pos[0] + b.size[0];
    let b_bottom = b.pos[1] + b.size[1];
    a.pos[0] < b_right && b.pos[0] < a_right && a.pos[1] < b_bottom && b.pos[1] < a_bottom
}

#[test]
fn row_siblings_do_not_overlap_with_gap() {
    let root = Stack::row().with_gap(10.0).with_children(vec![
        Node::rect([100.0, 50.0], red()),
        Node::rect([100.0, 50.0], red()),
        Node::rect([100.0, 50.0], red()),
    ]);
    let frame = layout(&root, [400.0, 50.0]);
    assert_eq!(frame.rects.len(), 3, "three rects laid out");
    for i in 0..frame.rects.len() {
        for j in i + 1..frame.rects.len() {
            assert!(
                !rects_overlap(&frame.rects[i], &frame.rects[j]),
                "row siblings {i} and {j} overlap"
            );
        }
    }
}

#[test]
fn column_siblings_do_not_overlap_with_gap() {
    let root = Stack::column().with_gap(8.0).with_children(vec![
        Node::rect([100.0, 50.0], red()),
        Node::rect([100.0, 50.0], red()),
    ]);
    let frame = layout(&root, [100.0, 200.0]);
    assert!(!rects_overlap(&frame.rects[0], &frame.rects[1]));
    // Gap exactly 8.0
    let gap = frame.rects[1].pos[1] - (frame.rects[0].pos[1] + frame.rects[0].size[1]);
    assert!(
        (gap - 8.0).abs() < f32::EPSILON,
        "gap should be 8.0 but is {gap}"
    );
}

#[test]
fn padding_insets_children() {
    let root = Stack::row()
        .with_padding(EdgeInsets {
            top: 5.0,
            right: 10.0,
            bottom: 5.0,
            left: 20.0,
        })
        .with_children(vec![Node::rect([100.0, 50.0], red())]);
    let frame = layout(&root, [200.0, 100.0]);
    let r = &frame.rects[0];
    assert!(
        (r.pos[0] - 20.0).abs() < f32::EPSILON,
        "left inset 20px; child x = {}",
        r.pos[0]
    );
    assert!(
        (r.pos[1] - 5.0).abs() < f32::EPSILON,
        "top inset 5px; child y = {}",
        r.pos[1]
    );
}

#[test]
fn flex_distributes_leftover_proportionally() {
    let root = Stack::row().with_children(vec![
        Node::rect([0.0, 50.0], red()).with_flex(1.0),
        Node::rect([0.0, 50.0], red()).with_flex(2.0),
        Node::rect([0.0, 50.0], red()).with_flex(1.0),
    ]);
    let frame = layout(&root, [400.0, 50.0]);
    let widths: Vec<f32> = frame.rects.iter().map(|r| r.size[0]).collect();
    // 400 / (1+2+1) = 100 per unit weight → 100, 200, 100
    assert!((widths[0] - 100.0).abs() < f32::EPSILON, "{widths:?}");
    assert!((widths[1] - 200.0).abs() < f32::EPSILON, "{widths:?}");
    assert!((widths[2] - 100.0).abs() < f32::EPSILON, "{widths:?}");
}

#[test]
fn nested_stack_places_children_within_parent() {
    let inner = Stack::row().with_gap(4.0).with_children(vec![
        Node::rect([20.0, 20.0], red()),
        Node::rect([20.0, 20.0], red()),
    ]);
    let root = Stack::column()
        .with_padding(EdgeInsets::all(10.0))
        .with_children(vec![Node::stack([200.0, 30.0], inner)]);
    let frame = layout(&root, [220.0, 60.0]);
    assert_eq!(frame.rects.len(), 2);
    // Outer padding 10 + inner stack origin 10,10 → first child at (10, 10)
    assert!(frame.rects[0].pos[0] >= 10.0);
    assert!(frame.rects[0].pos[1] >= 10.0);
    // Second child past first + gap
    let gap = frame.rects[1].pos[0] - (frame.rects[0].pos[0] + frame.rects[0].size[0]);
    assert!((gap - 4.0).abs() < f32::EPSILON, "{gap}");
}

#[test]
fn cross_align_center_offsets_smaller_child() {
    let root = Stack::row()
        .with_cross_align(CrossAlign::Center)
        .with_children(vec![Node::rect([40.0, 20.0], red())]);
    let frame = layout(&root, [100.0, 100.0]);
    let r = &frame.rects[0];
    // 100 viewport cross, 20 child cross → centered at y = (100 - 20) / 2 = 40
    assert!((r.pos[1] - 40.0).abs() < f32::EPSILON, "{r:?}");
}

#[test]
fn background_emits_a_rect_at_outer_bounds() {
    let root = Stack::row()
        .with_background(red())
        .with_children(vec![Node::rect([20.0, 20.0], red())]);
    let frame = layout(&root, [80.0, 40.0]);
    assert_eq!(frame.rects.len(), 2, "background + one child");
    let bg = &frame.rects[0];
    assert!(bg.pos[0].abs() < f32::EPSILON && bg.pos[1].abs() < f32::EPSILON);
    assert!((bg.size[0] - 80.0).abs() < f32::EPSILON);
    assert!((bg.size[1] - 40.0).abs() < f32::EPSILON);
}
