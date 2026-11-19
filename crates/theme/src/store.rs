//! The [`ThemeStore`]: which of the bundled themes the app draws with right now.
//!
//! The choice has two inputs — the `theme` setting and the appearance the window
//! reports — and one output, the active [`Theme`]. The setting is read through
//! [`ThemeSetting`] and kept in step by an observer on the settings store, so a change to
//! `settings.json` lands without anyone having to ask. The window appearance has no
//! application-level observer in GPUI: it is announced per window, so the first step that
//! owns a window forwards it to [`ThemeStore::set_system_appearance`]. Views repaint by
//! observing this global (`cx.observe_global::<ThemeStore>`), which fires whenever either
//! input changes.

use std::sync::Arc;

use gpui::{App, Global, Subscription};
use settings::Setting;

use crate::{ActiveTheme, Appearance, Theme, ThemeError, ThemeRegistry};

/// The `theme` setting as the theme crate reads it: which appearance to draw, or `system`
/// to follow the window. A `Setting` view of its own so this crate reads the one key it
/// cares about and nothing else in the settings.
pub struct ThemeSetting(pub settings::Theme);

impl settings::Setting for ThemeSetting {
    fn from_settings(settings: &settings::Settings) -> Self {
        Self(settings.theme)
    }
}

/// The bundled themes plus the two inputs the active one is chosen from.
pub struct ThemeStore {
    registry: ThemeRegistry,
    selection: settings::Theme,
    system_appearance: Appearance,
    /// Held so the store keeps following the `theme` setting. Dropping it — a second
    /// [`init`], or the app quitting — is what stops the old store listening.
    _settings_subscription: Subscription,
}

impl Global for ThemeStore {}

impl ThemeStore {
    /// Record the appearance the window reports and let everything observing this global
    /// redraw. `system` is the only selection that cares; `light` and `dark` draw the
    /// theme they name whatever the desktop looks like.
    ///
    /// The first step that owns a window forwards `cx.observe_window_appearance` here,
    /// with `cx.window_appearance().into()`.
    pub fn set_system_appearance(cx: &mut App, appearance: Appearance) {
        cx.global_mut::<ThemeStore>().system_appearance = appearance;
    }

    /// What this store draws right now: the theme the selection asks for, or the one for
    /// the window appearance when the selection says `system`.
    fn active(&self) -> &Arc<Theme> {
        let appearance = match self.selection {
            settings::Theme::Light => Appearance::Light,
            settings::Theme::Dark => Appearance::Dark,
            settings::Theme::System => self.system_appearance,
        };
        self.registry.for_appearance(appearance)
    }
}

/// Install the [`ThemeStore`] global; from then on the active theme is read through
/// [`ActiveTheme::theme`] (`cx.theme()`).
///
/// Called once at startup, after `settings`, in the init order `main.rs` runs
/// (`settings → theme → ui → workspace → feature crates`).
///
/// # Errors
///
/// [`ThemeError`] when a bundled theme cannot be parsed. An app cannot draw without
/// colors, so the caller shows this instead of starting with the wrong ones.
pub fn init(cx: &mut App) -> Result<(), ThemeError> {
    let registry = ThemeRegistry::bundled()?;
    ThemeSetting::register(cx);
    let selection = ThemeSetting::get_global(cx).0;
    let system_appearance = Appearance::from(cx.window_appearance());
    let settings_subscription = cx.observe_global::<settings::SettingsStore>(|cx| {
        let selection = ThemeSetting::get_global(cx).0;
        cx.global_mut::<ThemeStore>().selection = selection;
    });
    cx.set_global(ThemeStore {
        registry,
        selection,
        system_appearance,
        _settings_subscription: settings_subscription,
    });
    Ok(())
}

impl ActiveTheme for App {
    fn theme(&self) -> &Arc<Theme> {
        self.global::<ThemeStore>().active()
    }
}

#[cfg(test)]
#[path = "store_tests.rs"]
mod tests;
