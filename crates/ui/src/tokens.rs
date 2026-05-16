//! Januas default-theme tokens — the typed mirror of
//! `~/Desktop/Januas/docs/design-tokens.md`.
//!
//! Purpose: every UI surface in the ADE consumes its colors, type sizes, gaps,
//! and radii from this module rather than from inline hex literals scattered
//! through call sites. One change here, one change in the markdown table, no
//! drift. The doc is the spec; this is the implementation.
//! Public surface: [`palette`] (sRGB byte triples, Layer 1 — palette), the
//! semantic [`SURFACE_BASE`] / [`ACCENT`] / [`TEXT`] / ... constants
//! (Layer 2 — what components reference), and the [`fonts`], [`motion`],
//! [`radii`] sub-modules.
//! Why this file: collapses the 3 duplicated token lists (S5.5a bench, S5.5b
//! example, S5.5c interactive demo). The Layer 1 / Layer 2 split in the
//! markdown is preserved; Layer 3 (component overrides) is empty as of
//! 2026-05-16 and lives at call sites until a real component needs it.
//! Not responsibilities: rendering (`januas-renderer`), layout (`crate::lib`),
//! color-space math (`crate::color`).

/// Layer 1 — raw palette.
///
/// Components should consume the semantic constants (Layer 2) below, not
/// these. Kept `pub` so codegen + tests can reference them by their token
/// name without unfolding the alias chain.
pub mod palette {
    /// `p.surface.base` — locked dark base.
    pub const SURFACE_BASE: [u8; 3] = [0x19, 0x1d, 0x1e];
    /// `p.surface.1` — +1 tint, subtle raise.
    pub const SURFACE_1: [u8; 3] = [0x1e, 0x23, 0x24];
    /// `p.surface.2` — +2 tint, panels and cards.
    pub const SURFACE_2: [u8; 3] = [0x23, 0x2a, 0x2b];
    /// `p.surface.3` — +3 tint, hover surfaces.
    pub const SURFACE_3: [u8; 3] = [0x2a, 0x31, 0x32];

    /// `p.accent` — Janus teal. The single active-state color.
    pub const ACCENT: [u8; 3] = [0x6b, 0xb8, 0xa8];
    /// `p.accent.deep` — pressed state.
    pub const ACCENT_DEEP: [u8; 3] = [0x56, 0x9a, 0x8c];

    /// `p.cream` — primary text, marble pulled from the brand mark.
    pub const CREAM: [u8; 3] = [0xdd, 0xd2, 0xbb];
    /// `p.cream.dim` — secondary text, labels.
    pub const CREAM_DIM: [u8; 3] = [0x97, 0x90, 0x7f];
    /// `p.cream.faint` — tertiary, scaffolding.
    pub const CREAM_FAINT: [u8; 3] = [0x58, 0x52, 0x49];

    /// `p.blue` — workspace dot accent.
    pub const BLUE: [u8; 3] = [0x6e, 0x9b, 0xd4];
    /// `p.green` — workspace dot accent.
    pub const GREEN: [u8; 3] = [0x6f, 0xb8, 0x9c];
    /// `p.orange` — workspace dot accent.
    pub const ORANGE: [u8; 3] = [0xe8, 0x93, 0x57];
    /// `p.pink` — workspace dot accent.
    pub const PINK: [u8; 3] = [0xd8, 0x7c, 0xb5];
    /// `p.yellow` — workspace dot accent.
    pub const YELLOW: [u8; 3] = [0xd9, 0xbb, 0x66];

    /// `p.traffic.red` — macOS native traffic-light close dot.
    pub const TRAFFIC_RED: [u8; 3] = [0xff, 0x5f, 0x56];
    /// `p.traffic.amber` — macOS native traffic-light minimize dot.
    pub const TRAFFIC_AMBER: [u8; 3] = [0xff, 0xbd, 0x2e];
    /// `p.traffic.green` — macOS native traffic-light maximize dot.
    pub const TRAFFIC_GREEN: [u8; 3] = [0x27, 0xc9, 0x3f];
}

// ===== Layer 2 — semantic =====
//
// Components consume these. The aliasing is deliberate so the swap point for
// a future light theme is one section of this file, not every call site.

/// Window background.
pub const SURFACE_BASE: [u8; 3] = palette::SURFACE_BASE;
/// One step up; subway bar, footer.
pub const SURFACE_RAISED: [u8; 3] = palette::SURFACE_1;
/// Cards, mode buttons.
pub const SURFACE_PANEL: [u8; 3] = palette::SURFACE_2;
/// Card hover, list-row hover.
pub const SURFACE_HOVER: [u8; 3] = palette::SURFACE_3;

/// Active marker, button accents.
pub const ACCENT: [u8; 3] = palette::ACCENT;
/// Pressed state, deeper border.
pub const ACCENT_DEEP: [u8; 3] = palette::ACCENT_DEEP;
/// Text *on* an accent-filled surface (matches `surface.base`).
pub const ACCENT_ON: [u8; 3] = palette::SURFACE_BASE;

/// Primary text.
pub const TEXT: [u8; 3] = palette::CREAM;
/// Secondary text.
pub const TEXT_DIM: [u8; 3] = palette::CREAM_DIM;
/// Tertiary / dividers / scaffolding.
pub const TEXT_FAINT: [u8; 3] = palette::CREAM_FAINT;

/// `p.cream.05` straight-alpha — subtle fills.
pub const CREAM_05_A: f32 = 0.05;
/// `p.cream.09` straight-alpha — standard borders.
pub const CREAM_09_A: f32 = 0.09;
/// `p.cream.14` straight-alpha — border emphasis.
pub const CREAM_14_A: f32 = 0.14;

/// `p.accent.15` straight-alpha — soft tinted backgrounds.
pub const ACCENT_15_A: f32 = 0.15;
/// `p.accent.22` straight-alpha — hover-state soft tint.
pub const ACCENT_22_A: f32 = 0.22;

/// Locked v0.1 type scale, in surface pixels.
///
/// Mirrors the v6 mockup's `font-size` values verbatim. Line-heights stay
/// caller-driven — the layout pass needs them, and the design system
/// declares `1.5` only for the tagline.
pub mod fonts {
    /// Serif wordmark in the hero.
    pub const WORDMARK_PX: f32 = 26.0;
    /// Sans-serif tagline copy.
    pub const TAGLINE_PX: f32 = 15.0;
    /// Sans-serif body default — workspace tab name, mode-button name.
    pub const BODY_PX: f32 = 13.5;
    /// Mono mode-button sub-line.
    pub const MODE_SUB_PX: f32 = 12.0;
    /// Mono workspace-tab count, titlebar title.
    pub const SMALL_PX: f32 = 11.5;
    /// Mono kbd chip / mode-kbd / count badge.
    pub const KBD_PX: f32 = 11.0;
    /// Mono footer hotkey labels, meta-row.
    pub const FOOTER_PX: f32 = 10.5;
}

/// Locked v0.1 radius scale, in surface pixels.
///
/// The 12-and-up cap from `design-principles.md` applies only to functional
/// surfaces; the logo bitmap's 14px frame is a brand asset (see the
/// principles doc for the carve-out).
pub mod radii {
    /// `r.sm` — kbd chips, count badges.
    pub const SM: f32 = 4.0;
    /// `r.md` — tab buttons, small controls.
    pub const MD: f32 = 6.0;
    /// `r.lg` — cards, mode buttons.
    pub const LG: f32 = 8.0;
    /// Logo bitmap frame — brand asset, not a functional radius.
    pub const LOGO: f32 = 14.0;
}

/// Locked v0.1 motion tokens. Not yet consumed by the renderer (no animation
/// state lives in the layout pass at v0.1), but mirrored here so the
/// design-system surface area stays complete.
pub mod motion {
    use core::time::Duration;
    /// `t.fast` — hover state changes.
    pub const FAST: Duration = Duration::from_millis(140);
    /// `t.base` — card hover, panel transitions.
    pub const BASE: Duration = Duration::from_millis(220);
}

/// Width caps from `~/Desktop/Januas/docs/design-scaling.md` §3. Logical
/// pixels. Centering rule lives at the call site: `min(viewport − 2·gutter,
/// width.hero.max)`.
pub mod widths {
    /// Hero column max-width — ~50ch at the 13.5px body size.
    pub const HERO_MAX: f32 = 720.0;
    /// Paragraph max-width — WCAG 1.4.8 sweet spot.
    pub const PROSE_MAX: f32 = 640.0;
    /// Mode-button row max-width; centered when viewport allows.
    pub const MODE_BUTTON_ROW_MAX: f32 = 960.0;
    /// Minimum horizontal gutter when the hero stretches.
    pub const VIEWPORT_MIN_GUTTER: f32 = 24.0;
}

/// Locked window dimensions from `design-scaling.md` §6.
pub mod window {
    /// Default open size; matches the v6 mockup target.
    pub const SIZE_DEFAULT_W: f32 = 1280.0;
    /// Default open size; matches the v6 mockup target.
    pub const SIZE_DEFAULT_H: f32 = 800.0;
    /// Enforced floor; below this the home scene overlaps. Pass to winit
    /// via `WindowAttributes::with_min_inner_size(LogicalSize::new(..))`.
    pub const SIZE_MIN_W: f32 = 720.0;
    /// Enforced floor; see `SIZE_MIN_W`.
    pub const SIZE_MIN_H: f32 = 480.0;
}

/// Gaps between sections + tiles. Logical pixels.
pub mod gaps {
    /// Vertical divider between chrome sections (titlebar/subway/hero/footer).
    pub const SECTION: f32 = 1.0;
    /// Breathing room around the subway home/tabs vertical divider.
    pub const SUBWAY_DIVIDER: f32 = 8.0;
    /// Between adjacent tiles in a launcher row or preset grid.
    pub const TILE: f32 = 12.0;
}

/// Hit-target floors from `design-scaling.md` §4.
///
/// Any `Interactive` node must meet [`MIN`] on both axes, OR sit in a row
/// with `≥ TARGET` hit band, OR carry `≥ MIN` clearance from every other
/// interactive node.
pub mod hit {
    /// WCAG 2.5.8 floor for any `Interactive` node.
    pub const MIN: f32 = 24.0;
    /// Standard-control target (mode buttons, list rows, primary buttons).
    pub const TARGET: f32 = 28.0;
}
