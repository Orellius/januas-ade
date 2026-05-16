//! Layout-pass anti-clunky assertions (S5.5g).
//!
//! Purpose: catch the failure modes called out in
//! `~/Desktop/Januas/docs/design-scaling.md` §8 before they reach the user.
//! Two cheap checks run after every `layout()` in debug builds — they
//! panic on violation so the offending Stack lights up in a backtrace
//! instead of producing clunky pixels.
//! Public surface: [`debug_assert_layout`], [`assert_within_viewport`],
//! [`assert_pixel_grid_snap`]. Padding-symmetry and hit-target checks need
//! node-level metadata `LayoutFrame` doesn't yet carry — deferred to a
//! later slice when an `Interactive` marker or a container-tree audit pass
//! arrives.
//! Why this file (vs inlining in `lib.rs`): keeps the assertion noise out
//! of the layout pass's hot loop; `cfg!(debug_assertions)` gating + a
//! release-mode no-op closes the perf gap. The doc-spec for each check
//! reads top-to-bottom here rather than scattered across helpers.
//! Not responsibilities: enforcing design-tokens at the token-value level
//! (those live in `tokens.rs`); runtime visual diff against the v6
//! mockup (manual eyeball pass).
//! Test strategy: `tests/debug_asserts.rs` builds a known-bad scene and
//! confirms each check panics with the expected message; a known-good
//! scene round-trips clean.

use crate::LayoutFrame;

/// Run every cheap layout invariant against `frame`. Panics on the first
/// failure in debug builds; no-op in release.
///
/// Callers wire this immediately after `crate::layout(...)`. `viewport`
/// is the logical-pixel viewport the layout was computed against; the
/// frame's bounds must stay within `[0, viewport.x] × [0, viewport.y]`.
pub fn debug_assert_layout(frame: &LayoutFrame, viewport: [f32; 2]) {
    if cfg!(debug_assertions) {
        assert_within_viewport(frame, viewport);
        assert_pixel_grid_snap(frame);
    }
}

/// Verify every primitive's bounds sit inside the viewport rectangle.
///
/// Catches the "flex spacer collapsed, children extended past the parent
/// boundary" failure mode (`design-scaling.md` §5) — once a child crosses
/// the viewport edge it's the loudest signal the layout is clunky.
///
/// # Panics
///
/// Panics with the offending bound + index when any primitive's bounds
/// fall outside `[0, viewport.x] × [0, viewport.y]`. The exact-zero
/// comparisons are intentional; logical pixels are integer-shaped by the
/// design-tokens layer (§2.5), so any drift is a bug, not noise.
pub fn assert_within_viewport(frame: &LayoutFrame, viewport: [f32; 2]) {
    assert!(
        !frame.overflow,
        "LayoutFrame.overflow == true: at least one Stack's intrinsic main-axis exceeded its inner size. \
         Per design-scaling.md §5, the affected scene must switch to scroll/clip before this assertion fires."
    );
    for (i, r) in frame.rects.iter().enumerate() {
        check_in_viewport("rect", i, r.pos, r.size, viewport);
    }
    for (i, t) in frame.texts.iter().enumerate() {
        // Text-run bounds are pos + (font_size × line count × line_height);
        // we only have `pos` + `font_size` + `line_height` on the run, so
        // approximate the height as line_height (single line) — close
        // enough for the viewport guard.
        check_in_viewport(
            "text",
            i,
            t.pos,
            [t.font_size * 0.6, t.line_height],
            viewport,
        );
    }
    for (i, img) in frame.images.iter().enumerate() {
        check_in_viewport("image", i, img.pos, img.size, viewport);
    }
    for (i, z) in frame.hit_zones.iter().enumerate() {
        check_in_viewport("hit_zone", i, z.pos, z.size, viewport);
    }
}

fn check_in_viewport(kind: &str, i: usize, pos: [f32; 2], size: [f32; 2], viewport: [f32; 2]) {
    let right = pos[0] + size[0];
    let bottom = pos[1] + size[1];
    assert!(
        pos[0] >= 0.0 && pos[1] >= 0.0 && right <= viewport[0] && bottom <= viewport[1],
        "{kind}[{i}] out of viewport: pos={pos:?} size={size:?} right={right} bottom={bottom} viewport={viewport:?}",
    );
}

/// Verify every box snaps to integer logical pixels.
///
/// Text runs are exempt — cosmic-text handles sub-pixel glyph
/// rasterization, and the boxes around a text run come from the parent
/// stack which IS box-aligned.
///
/// Per `design-scaling.md` §2.5: "After `scale_by`, every node's `x`,
/// `y`, `width`, `height` must be integer physical pixels. Fractional
/// logical values snap their box to integers at scale-multiplied
/// resolution; text rasterization handles sub-pixel glyph placement, but
/// boxes do not."
///
/// This function runs BEFORE `scale_by`, so the check is against integer
/// LOGICAL pixels. If your tokens use a half-pixel literal (e.g.,
/// `13.5px` for body), the token must round to an integer through the
/// layout pass — make `Node::size` integers even when the underlying
/// font metric is fractional.
///
/// # Panics
///
/// Panics with the offending coordinate when any box dimension carries a
/// fractional logical value.
pub fn assert_pixel_grid_snap(frame: &LayoutFrame) {
    for (i, r) in frame.rects.iter().enumerate() {
        check_integer("rect", i, "pos.x", r.pos[0]);
        check_integer("rect", i, "pos.y", r.pos[1]);
        check_integer("rect", i, "size.w", r.size[0]);
        check_integer("rect", i, "size.h", r.size[1]);
    }
    for (i, img) in frame.images.iter().enumerate() {
        check_integer("image", i, "pos.x", img.pos[0]);
        check_integer("image", i, "pos.y", img.pos[1]);
        check_integer("image", i, "size.w", img.size[0]);
        check_integer("image", i, "size.h", img.size[1]);
    }
    for (i, z) in frame.hit_zones.iter().enumerate() {
        check_integer("hit_zone", i, "pos.x", z.pos[0]);
        check_integer("hit_zone", i, "pos.y", z.pos[1]);
        check_integer("hit_zone", i, "size.w", z.size[0]);
        check_integer("hit_zone", i, "size.h", z.size[1]);
    }
}

fn check_integer(kind: &str, i: usize, field: &str, v: f32) {
    assert!(
        v.fract() == 0.0,
        "{kind}[{i}].{field} = {v} (not integer logical pixels — design-scaling.md §2.5)",
    );
}
