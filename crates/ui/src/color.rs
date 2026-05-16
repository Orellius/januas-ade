//! sRGB → linear color conversion — the boundary between the design-token
//! tables and the renderer's linear-space pipelines.
//!
//! Purpose: collapse the duplicated `srgb_u8_to_linear` helpers that appeared
//! in the S5.5a bench, the S5.5b layout example, and (now) the S5.5d v6 scene
//! into one canonical implementation. Token hex literals live in
//! [`crate::tokens`]; this module is the only place the actual sRGB curve is
//! evaluated, so a future codegen path from `~/Desktop/Januas/docs/design-tokens.md`
//! can target either layer cleanly.
//! Public surface: [`srgb_u8_to_linear`], [`linear`], [`linear_rgb`].
//! Not responsibilities: token tables (`crate::tokens`), the wgpu pipeline
//! that consumes the linear values (`januas-renderer`).

/// Convert one sRGB byte channel to a linear-space `f32` in `[0.0, 1.0]`.
///
/// Uses the standard piecewise approximation: the linear segment below the
/// breakpoint avoids the discontinuity that a plain power-of-2.4 introduces
/// at black, and the `((s + 0.055) / 1.055)^2.4` segment matches the IEC
/// 61966-2-1 reference within `f32` precision.
#[must_use]
pub fn srgb_u8_to_linear(c: u8) -> f32 {
    #[allow(clippy::cast_lossless, reason = "u8-to-f32 promotion is intentional")]
    let s = c as f32 / 255.0;
    if s <= 0.040_45 {
        s / 12.92
    } else {
        ((s + 0.055) / 1.055).powf(2.4)
    }
}

/// Convert an sRGB `[r, g, b]` byte triple plus a straight `alpha` in
/// `[0.0, 1.0]` to a linear-space RGBA `[f32; 4]`.
///
/// Alpha is not gamma-encoded in sRGB, so it passes through unchanged.
#[must_use]
pub fn linear(rgb: [u8; 3], alpha: f32) -> [f32; 4] {
    [
        srgb_u8_to_linear(rgb[0]),
        srgb_u8_to_linear(rgb[1]),
        srgb_u8_to_linear(rgb[2]),
        alpha,
    ]
}

/// Convert an sRGB `[r, g, b]` byte triple to a linear-space `[f32; 3]`.
///
/// Use when the renderer wants the rgb channels separate from the alpha — for
/// example when filling in a struct that already carries its own alpha field.
#[must_use]
pub fn linear_rgb(rgb: [u8; 3]) -> [f32; 3] {
    [
        srgb_u8_to_linear(rgb[0]),
        srgb_u8_to_linear(rgb[1]),
        srgb_u8_to_linear(rgb[2]),
    ]
}
