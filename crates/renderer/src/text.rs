//! Multi-position text run — the input to
//! [`crate::Renderer::set_text_runs`].
//!
//! Purpose: a transport struct that lets layout-driven callers (the `ui`
//! crate, the home-screen scene) specify N positioned text spans for a frame
//! without exposing `cosmic-text` or `glyphon` types across the crate
//! boundary. Each run owns its own buffer inside the renderer; the renderer
//! reuses buffer slots across calls when the run count is stable.
//! Public surface: [`TextRun`].
//! Not responsibilities: text shaping (`cosmic-text`), atlas management
//! (`glyphon`), layout (`crates/ui/`).

/// One positioned text span. Top-left anchor; sRGB color; font size and line
/// height in surface pixels.
#[derive(Clone, Debug, PartialEq)]
pub struct TextRun {
    /// Top-left anchor in surface pixels.
    pub pos: [f32; 2],
    /// Glyph contents. Newlines wrap.
    pub content: String,
    /// Font size in surface pixels.
    pub font_size: f32,
    /// Line height in surface pixels.
    pub line_height: f32,
    /// sRGB rgba — matches `glyphon::Color::rgba`.
    pub color: [u8; 4],
}
