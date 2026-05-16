//! Pointer state machine + hit-test invariants (S5.5c).

use januas_ui::{
    EdgeInsets, HitZone, Node, NodeId, NodeKind, PointerState, RectStyle, Stack, hit_test, layout,
};

const fn red() -> RectStyle {
    RectStyle::fill([1.0, 0.0, 0.0, 1.0])
}

const A: NodeId = 10;
const B: NodeId = 20;
const PARENT: NodeId = 99;

#[test]
fn hit_test_returns_none_on_miss() {
    let zones = [HitZone {
        id: A,
        pos: [10.0, 10.0],
        size: [20.0, 20.0],
    }];
    assert_eq!(hit_test(&zones, [0.0, 0.0]), None);
    assert_eq!(hit_test(&zones, [40.0, 40.0]), None);
}

#[test]
fn hit_test_returns_hit_inside_bounds() {
    let zones = [HitZone {
        id: A,
        pos: [10.0, 10.0],
        size: [20.0, 20.0],
    }];
    assert_eq!(hit_test(&zones, [15.0, 15.0]), Some(A));
    // Edge inclusive at top-left, exclusive at bottom-right.
    assert_eq!(hit_test(&zones, [10.0, 10.0]), Some(A));
    assert_eq!(hit_test(&zones, [29.9, 29.9]), Some(A));
    assert_eq!(hit_test(&zones, [30.0, 30.0]), None);
}

#[test]
fn hit_test_picks_topmost_when_zones_overlap() {
    // A under B; reverse-iter wins B.
    let zones = [
        HitZone {
            id: A,
            pos: [0.0, 0.0],
            size: [100.0, 100.0],
        },
        HitZone {
            id: B,
            pos: [10.0, 10.0],
            size: [30.0, 30.0],
        },
    ];
    assert_eq!(hit_test(&zones, [20.0, 20.0]), Some(B));
    // Outside B, still inside A.
    assert_eq!(hit_test(&zones, [80.0, 80.0]), Some(A));
}

#[test]
fn pointer_move_to_reports_hover_change() {
    let zones = [HitZone {
        id: A,
        pos: [0.0, 0.0],
        size: [50.0, 50.0],
    }];
    let mut p = PointerState::default();
    assert!(p.move_to([10.0, 10.0], &zones));
    assert_eq!(p.hovered, Some(A));
    // Same zone — no change.
    assert!(!p.move_to([20.0, 20.0], &zones));
    // Off the zone — change to None.
    assert!(p.move_to([100.0, 100.0], &zones));
    assert_eq!(p.hovered, None);
}

#[test]
fn pointer_leave_clears_hover() {
    let zones = [HitZone {
        id: A,
        pos: [0.0, 0.0],
        size: [50.0, 50.0],
    }];
    let mut p = PointerState::default();
    p.move_to([10.0, 10.0], &zones);
    assert!(p.leave());
    assert_eq!(p.hovered, None);
    assert_eq!(p.position, None);
    // Leaving when already left — no change.
    assert!(!p.leave());
}

#[test]
fn layout_emits_hit_zone_for_nodes_with_id() {
    let root = Stack::row()
        .with_padding(EdgeInsets::all(10.0))
        .with_children(vec![
            Node::rect([20.0, 20.0], red()).with_id(A),
            Node::rect([20.0, 20.0], red()),
            Node::rect([20.0, 20.0], red()).with_id(B),
        ]);
    let frame = layout(&root, [100.0, 50.0]);
    assert_eq!(frame.hit_zones.len(), 2, "only id-tagged nodes emit zones");
    assert_eq!(frame.hit_zones[0].id, A);
    assert_eq!(frame.hit_zones[1].id, B);
}

#[test]
fn child_hit_zone_wins_over_parent() {
    // Parent stack tagged + interactive child inside. Reverse-iter should
    // return the child, not the parent, when the cursor is over the child.
    let inner = Stack::row().with_children(vec![Node::rect([10.0, 10.0], red()).with_id(A)]);
    let root =
        Stack::column().with_children(vec![Node::stack([100.0, 30.0], inner).with_id(PARENT)]);
    let frame = layout(&root, [100.0, 30.0]);
    // Cursor over the child rect — child wins.
    assert_eq!(hit_test(&frame.hit_zones, [5.0, 5.0]), Some(A));
    // Cursor over the parent but outside the child — parent wins.
    assert_eq!(hit_test(&frame.hit_zones, [50.0, 5.0]), Some(PARENT));
}

#[test]
fn rebuild_preserves_node_ids_across_layout_passes() {
    let build = |selected: NodeId| {
        Stack::row().with_children(vec![
            Node::rect([20.0, 20.0], red()).with_id(A),
            Node::rect([20.0, 20.0], red()).with_id(B),
            Node::rect([20.0, 20.0], red()).with_id(selected),
        ])
    };
    let f1 = layout(&build(99), [60.0, 20.0]);
    let f2 = layout(&build(77), [60.0, 20.0]);
    assert_eq!(f1.hit_zones[0].id, A);
    assert_eq!(f1.hit_zones[2].id, 99);
    assert_eq!(f2.hit_zones[2].id, 77);
    // First two zones are at identical positions across rebuilds.
    assert!((f1.hit_zones[0].pos[0] - f2.hit_zones[0].pos[0]).abs() < f32::EPSILON);
}

// Silence an unused-import warning when NodeKind is only referenced through
// constructors — used implicitly by Node::rect / Node::stack.
const _: fn() = || {
    let _: Option<NodeKind> = None;
};
