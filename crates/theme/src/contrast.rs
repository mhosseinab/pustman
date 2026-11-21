//! WCAG contrast, so the bundled themes are checked against REQ-NFR-06 rather than
//! eyeballed. Both functions take opaque colors: a translucent token has no single
//! contrast ratio, because what it looks like depends on what sits under it.

use gpui::{Hsla, Rgba};

/// The WCAG relative luminance of an opaque color: `0.0` for black, `1.0` for white.
pub fn relative_luminance(color: Hsla) -> f64 {
    let Rgba { r, g, b, .. } = Rgba::from(color);
    0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b)
}

/// The WCAG contrast ratio between two opaque colors, `1.0` for a color against itself
/// and `21.0` for black against white. Body text needs at least `4.5`.
pub fn contrast_ratio(foreground: Hsla, background: Hsla) -> f64 {
    let foreground = relative_luminance(foreground);
    let background = relative_luminance(background);
    let (lighter, darker) = if foreground >= background {
        (foreground, background)
    } else {
        (background, foreground)
    };
    (lighter + 0.05) / (darker + 0.05)
}

/// One sRGB channel of [`relative_luminance`]: the linear part of the transfer function.
fn channel(value: f32) -> f64 {
    let value = f64::from(value);
    if value <= 0.04045 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

#[cfg(test)]
#[path = "contrast_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "contrast_non_text_tests.rs"]
mod non_text_tests;
