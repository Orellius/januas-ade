//! Pointer state machine + hit testing (S5.5c).
//!
//! Purpose: take cursor events from the windowing layer, map them to the
//! [`HitZone`]s emitted by the layout pass, and expose the currently hovered
//! [`NodeId`] so the caller can re-style hovered/selected nodes when the
//! layout is next rebuilt. Selection (which tab is active) lives one layer
//! up — this module only tracks transient pointer state.
//! Public surface: [`NodeId`], [`HitZone`], [`PointerState`], [`hit_test`].
//! Not responsibilities: selection state (caller owns), focus chain (later
//! slice), keyboard navigation (later slice), drag (out of scope for v0.1).

/// Stable hit-test identifier carried on a [`crate::Node`].
pub type NodeId = u32;

/// One hit-test zone — an axis-aligned bounding box plus the [`NodeId`] of
/// the node that produced it. Emitted by [`crate::layout`] when a node has
/// `id: Some(_)`.
#[derive(Copy, Clone, Debug, PartialEq)]
pub struct HitZone {
    /// Identity of the node this zone belongs to.
    pub id: NodeId,
    /// Top-left corner in surface pixels.
    pub pos: [f32; 2],
    /// Width and height in surface pixels.
    pub size: [f32; 2],
}

/// Topmost-wins hit test over a slice of [`HitZone`]s.
///
/// Walks `hit_zones` in reverse declaration order so children placed after
/// their parents in the layout pass take precedence over the parent zone
/// that wraps them.
#[must_use]
pub fn hit_test(hit_zones: &[HitZone], pos: [f32; 2]) -> Option<NodeId> {
    for zone in hit_zones.iter().rev() {
        let right = zone.pos[0] + zone.size[0];
        let bottom = zone.pos[1] + zone.size[1];
        if pos[0] >= zone.pos[0] && pos[0] < right && pos[1] >= zone.pos[1] && pos[1] < bottom {
            return Some(zone.id);
        }
    }
    None
}

/// Transient pointer state — cursor position and the [`NodeId`] under it.
///
/// Updated via [`Self::move_to`] when the windowing layer reports a cursor
/// move, and [`Self::leave`] when the cursor leaves the surface. Selection
/// state (e.g. which tab is currently active) is the caller's concern.
#[derive(Default, Debug, Clone, PartialEq)]
pub struct PointerState {
    /// Current cursor position in surface pixels, or `None` when the cursor
    /// is off the surface.
    pub position: Option<[f32; 2]>,
    /// Identity of the node currently under the cursor, or `None`.
    pub hovered: Option<NodeId>,
}

impl PointerState {
    /// Update [`Self::position`] and [`Self::hovered`] from a cursor move.
    ///
    /// Returns `true` when [`Self::hovered`] changed — useful for the caller
    /// to decide whether to rebuild the frame.
    pub fn move_to(&mut self, pos: [f32; 2], hit_zones: &[HitZone]) -> bool {
        let new_hover = hit_test(hit_zones, pos);
        let changed = self.hovered != new_hover;
        self.position = Some(pos);
        self.hovered = new_hover;
        changed
    }

    /// Clear position and hover when the cursor leaves the surface. Returns
    /// `true` if hover changed.
    pub const fn leave(&mut self) -> bool {
        let changed = self.hovered.is_some();
        self.position = None;
        self.hovered = None;
        changed
    }
}
