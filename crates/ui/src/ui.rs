//! `ui` — Pustman's design system: the semantic tokens and the components every screen is
//! built from.
//!
//! Nothing here draws in raw numbers or raw colors. A screen asks for an intent —
//! [`Color::Muted`], [`DynamicSpacing::Base04`], [`TextSize::Small`], an [`ElevationIndex`] —
//! and this crate resolves it against the active theme ([`theme::ActiveTheme`]), the UI
//! density ([`UiDensity`]) and the user's font sizes. That is what keeps light and dark
//! correct and the product looking like one product
//! (`.agents/skills/rust-gpui-ui-ux/references/design-system.md`).
//!
//! Two doors in:
//! - [`prelude`] — `use ui::prelude::*;` is the import every view file starts with.
//! - [`init`] — called once at startup; it registers the font sizes typography reads,
//!   installs the density store and binds the keys the components answer to.
//!
//! The components themselves are values ([`Button`], [`IconButton`], [`Tab`], …) built with
//! a chainable builder and drawn through [`gpui::RenderOnce`]; see [`components`] for the
//! catalogue and the shared [`components::behavior`] traits.

pub mod actions;
pub mod color;
pub mod components;
pub mod elevation;
pub mod format;
pub mod icons;
pub mod prelude;
pub mod spacing;
#[cfg(any(test, feature = "test-support"))]
pub mod story;
#[cfg(any(test, feature = "test-support"))]
pub mod test_support;
pub mod typography;
pub mod units;

pub use actions::{Activate, Cancel, SelectNext, SelectPrevious};
pub use color::{Color, Severity};
pub use components::badge::Badge;
pub use components::behavior::{Clickable, Disableable, FixedWidth, Toggleable, VisibleOnHover};
pub use components::button::{
    Button, ButtonSize, ButtonStyle, ClickHandler, IconButton, IconButtonShape, TintColor,
};
pub use components::context_menu::{ContextMenu, floating_menu};
pub use components::divider::{Divider, DividerColor};
pub use components::key_binding::{KeyBinding, keystroke_label};
pub use components::label::{Headline, Label, LabelSize};
pub use components::list::VirtualList;
pub use components::modal::Modal;
pub use components::tab::Tab;
pub use components::tab_bar::{SEGMENTED_TRACK_SELECTOR, TabBar, TabBarEvent};
pub use components::table::{Table, TableColumn, TableRow};
pub use components::toast::{Toast, ToastEvent, Toasts};
pub use components::toggle::{Toggle, ToggleKind, ToggleState};
pub use components::tooltip::{Tooltip, TooltipBuilder};
pub use elevation::{ElevationIndex, StyledExt};
pub use format::{format_duration, format_size};
pub use icons::{Icon, IconAssets, IconName, IconSize};
pub use spacing::{DynamicSpacing, UiDensity, UiDensityStore};
pub use typography::{
    BUFFER_FONT_FAMILY, HeadlineSize, StyledTypography, TextSize, UI_FONT_FAMILY,
};
pub use units::{PIXELS_PER_REM, px_to_rems};

use gpui::{App, Div, Styled, div};
use settings::Setting;

use crate::components::context_menu::MENU_CONTEXT;
use crate::components::tab_bar::TAB_BAR_CONTEXT;
use crate::typography::FontSizesSetting;

/// A row: the elements sit on one line, centered on the cross axis. The design system's
/// horizontal stack — `references/gpui-fundamentals.md`; vertically, see [`v_flex`].
#[track_caller]
pub fn h_flex() -> Div {
    div().flex().flex_row().items_center()
}

/// A column: the elements stack top to bottom. The design system's vertical stack.
#[track_caller]
pub fn v_flex() -> Div {
    div().flex().flex_col()
}

/// The embedded icon set as a [`gpui::AssetSource`], for the app to install once at startup:
/// `gpui_platform::application().with_assets(ui::assets())`.
pub fn assets() -> IconAssets {
    IconAssets
}

/// Set the design system up. Called once from the app's init order, after `settings` and
/// `theme`: it registers the [`FontSizesSetting`] view [`TextSize::Ui`] and
/// [`TextSize::Editor`] resolve against, installs the app-wide [`UiDensityStore`] every
/// [`DynamicSpacing`] resolves against, and binds the [`TabBar`] and [`ContextMenu`] keys
/// — the arrows move along the bar or through the menu, Enter and Space confirm, Escape
/// closes a menu. Buttons and toggles bind
/// nothing: GPUI synthesizes one click on the focused control per Enter/Space
/// press-and-release, which cannot fire on key auto-repeat.
pub fn init(cx: &mut App) {
    FontSizesSetting::register(cx);
    cx.set_global(UiDensityStore::default());
    cx.bind_keys([
        gpui::KeyBinding::new("left", SelectPrevious, Some(TAB_BAR_CONTEXT)),
        gpui::KeyBinding::new("right", SelectNext, Some(TAB_BAR_CONTEXT)),
        gpui::KeyBinding::new("enter", Activate, Some(TAB_BAR_CONTEXT)),
        gpui::KeyBinding::new("space", Activate, Some(TAB_BAR_CONTEXT)),
        gpui::KeyBinding::new("up", SelectPrevious, Some(MENU_CONTEXT)),
        gpui::KeyBinding::new("down", SelectNext, Some(MENU_CONTEXT)),
        gpui::KeyBinding::new("enter", Activate, Some(MENU_CONTEXT)),
        gpui::KeyBinding::new("escape", Cancel, Some(MENU_CONTEXT)),
    ]);
}
