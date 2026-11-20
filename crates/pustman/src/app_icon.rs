//! The app's identity towards the desktop: the id the compositor matches against the
//! installed `pustman.desktop` entry, and the window icon X11 window managers draw.
//! Wayland and macOS take the icon from the desktop entry / app bundle instead
//! (`scripts/install-desktop-entry.sh`, `scripts/bundle-macos.sh`).

use std::sync::Arc;

use image::{ImageFormat, RgbaImage};

/// The window's app id. Wayland compositors look up `<APP_ID>.desktop` for the icon and
/// name, so it must equal the basename `scripts/install-desktop-entry.sh` installs.
pub const APP_ID: &str = "pustman";

const ICON_PNG: &[u8] =
    include_bytes!("../../../assets/pustman-icon-pack/linux/hicolor/256x256/apps/app-icon.png");

/// The bundled 256×256 app icon, decoded for `WindowOptions::icon`.
pub fn window_icon() -> anyhow::Result<Arc<RgbaImage>> {
    let icon = image::load_from_memory_with_format(ICON_PNG, ImageFormat::Png)?;
    Ok(Arc::new(icon.into_rgba8()))
}

#[cfg(test)]
#[path = "app_icon_tests.rs"]
mod tests;
