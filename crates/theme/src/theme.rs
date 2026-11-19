//! `theme` — the colors Pustman draws with.
//!
//! A [`Theme`] is a named set of typed colors ([`ThemeColors`] for the layers of the
//! window, [`StatusColors`] for the things a status can be) declared for one
//! [`Appearance`]. Two of them ship in `assets/themes/` — `pustman-light.json` and
//! `pustman-dark.json` — and [`ThemeRegistry`] parses them at startup. [`init`] installs
//! the [`ThemeStore`] global that holds them; after that every view reads colors through
//! [`ActiveTheme::theme`] (`cx.theme()`) and never picks a raw color of its own.
//!
//! Which theme is drawn follows the `theme` setting (REQ-UI-06): `light` and `dark` force
//! one, `system` takes the appearance the window reports. The values come from
//! `docs/postman_redesign_pack/01_specs/design_tokens.json` where the redesign pack
//! defines a role; see [`ThemeColors`] for the mapping.

mod colors;
mod contrast;
mod registry;
mod store;

pub use colors::{StatusColors, ThemeColors};
pub use contrast::{contrast_ratio, relative_luminance};
pub use registry::ThemeRegistry;
pub use store::{ThemeSetting, ThemeStore, init};

use std::sync::Arc;

use gpui::WindowAppearance;
use serde::Deserialize;

/// One bundled theme: its colors and the appearance it was written for.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Theme {
    name: String,
    appearance: Appearance,
    colors: ThemeColors,
    status: StatusColors,
}

impl Theme {
    /// The name a theme picker would show.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The appearance this theme was written for.
    pub fn appearance(&self) -> Appearance {
        self.appearance
    }

    /// The layers, borders, text and accents of this theme.
    pub fn colors(&self) -> &ThemeColors {
        &self.colors
    }

    /// The success/warning/error/info, HTTP class and method colors of this theme.
    pub fn status(&self) -> &StatusColors {
        &self.status
    }
}

/// Whether a theme is the light one or the dark one.
///
/// This is the appearance a theme is *written* for, and the appearance the operating
/// system reports; the `theme` setting (which may also say `system`) is a different
/// thing and lives in `settings::Theme`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    /// The theme for a light desktop.
    Light,
    /// The theme for a dark desktop.
    Dark,
}

impl From<WindowAppearance> for Appearance {
    /// The two vibrant macOS appearances are still light and dark underneath.
    fn from(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
        }
    }
}

/// Why the bundled themes could not be loaded.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum ThemeError {
    /// A file in `assets/themes/` is not a theme this crate can read. The message is the
    /// parser's own, kept for whoever has to fix the file; the app cannot draw without
    /// colors, so the caller shows this instead of starting with the wrong ones.
    #[error("could not parse theme {name}: {message}")]
    Parse {
        /// The file that did not parse, without its extension.
        name: String,
        /// The parser's own explanation.
        message: String,
    },
}

/// Access to the theme the app is drawing with.
///
/// Implemented for [`gpui::App`], so `cx.theme()` works anywhere a context derefs to the
/// app. A view that caches what it is drawing subscribes to the [`ThemeStore`] global and
/// redraws when it changes (an appearance change, or a new `theme` setting).
pub trait ActiveTheme {
    /// The theme the app draws with right now.
    fn theme(&self) -> &Arc<Theme>;
}
