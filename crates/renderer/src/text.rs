//! Multi-position text run — the input to
//! [`crate::Renderer::set_text_runs`].
//!
//! Purpose: a transport struct that lets layout-driven callers (the `ui`
//! crate, the home-screen scene) specify N positioned text spans for a frame
//! without exposing `cosmic-text` or `glyphon` types across the crate
//! boundary. Each run owns its own buffer inside the renderer; the renderer
//! reuses buffer slots across calls when the run count is stable.
//! Public surface: [`TextRun`], [`TextFamily`].
//! Not responsibilities: text shaping (`cosmic-text`), atlas management
//! (`glyphon`), layout (`crates/ui/`).

/// Generic font family — maps to a `cosmic_text::Family` variant inside the
/// renderer.
///
/// The three variants match the locked v0.1 type stack in
/// `~/Desktop/Januas/docs/design-tokens.md`: serif for wordmarks, sans for
/// body, monospace for counts / IDs / hotkeys / paths. Custom-named family
/// support is intentionally out of scope until a slice needs it.
#[derive(Copy, Clone, Debug, Default, PartialEq, Eq)]
pub enum TextFamily {
    /// `font.body` — default UI text. Sans-serif system font.
    #[default]
    Body,
    /// `font.display` — wordmarks and hero headlines. Serif system font.
    Display,
    /// `font.mono` — counts, IDs, hotkeys, paths.
    Mono,
}

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
    /// Generic family — maps to `cosmic_text::Family` inside the renderer.
    pub family: TextFamily,
}
