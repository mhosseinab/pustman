//! The single length conversion the design system needs. Everything is specified in
//! pixels — the numbers the redesign pack and `references/design-system.md` state — but
//! rendered in rems, so sizes follow the user's font-size settings and the display's
//! scale factor instead of one fixed pixel count.

use gpui::Rems;

/// How many pixels one `rem` spans at the default scale. Every "N px" size in this crate
/// is read against this number through [`px_to_rems`]: 14 px becomes 0.875 rem.
pub const PIXELS_PER_REM: f32 = 16.;

/// Express a pixel length in rems: `16 px` becomes `1 rem`, `12 px` becomes `0.75 rem`.
///
/// The parameter stays a plain `f32` on purpose: an `impl Into<f32>` parameter sends
/// every float literal at a call site through the compiler's numeric fallback and warns,
/// while this way `32.` simply means `32.0f32`.
pub fn px_to_rems(pixels: f32) -> Rems {
    Rems(pixels / PIXELS_PER_REM)
}
