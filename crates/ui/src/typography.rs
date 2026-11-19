//! Text sizes and font families.
//!
//! Every size is expressed in rems through [`crate::units::px_to_rems`], so text
//! follows the user's `ui_font_size`/`buffer_font_size` and the display's scale factor
//! instead of one pixel count (`references/design-system.md` §3). The px values below are
//! the sizes at the default `1rem = 16px`.
//!
//! Font families come from the redesign pack (`01_specs/design_tokens.json`): Inter for
//! UI text, JetBrains Mono for URLs, headers and bodies. A machine without those fonts
//! falls back to the platform default through the text system; the pack's own fallbacks
//! (DejaVu Sans / DejaVu Sans Mono) are what a Linux install resolves to.

use gpui::{AbsoluteLength, App, Pixels, Rems, SharedString, Styled, px};

use crate::units::px_to_rems;
use settings::{Setting, Settings};

/// The two font sizes [`TextSize`] resolves against, in pixels — a narrow [`Setting`]
/// view of the settings so the design system reads the two keys it needs and nothing
/// else. Every reading crate registers its own view (the `settings` contract; `theme`'s
/// `ThemeSetting` is the same shape), and [`crate::init`] registers this one.
pub struct FontSizesSetting {
    /// The `ui_font_size` setting.
    pub ui_font_size: f32,
    /// The `buffer_font_size` setting.
    pub buffer_font_size: f32,
}

impl Setting for FontSizesSetting {
    fn from_settings(settings: &Settings) -> Self {
        // `as f32` is the widening conversion here — `f32: From<u32>` does not exist.
        // Only absurd font sizes (above 2^24 px) would round in it; no real value does.
        Self {
            ui_font_size: settings.ui_font_size as f32,
            buffer_font_size: settings.buffer_font_size as f32,
        }
    }
}

/// The UI font: labels, buttons, menus.
pub const UI_FONT_FAMILY: &str = "Inter";
/// The content font: URLs, header values, request and response bodies.
pub const BUFFER_FONT_FAMILY: &str = "JetBrains Mono";

/// A semantic text size. `Large`/`Default`/`Small`/`XSmall` are fixed sizes in the
/// design system; `Ui` and `Editor` are whatever the user set their fonts to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TextSize {
    /// Modal titles, empty-state headlines: 16 px at the default scale.
    Large,
    /// Body UI text: 14 px at the default scale.
    #[default]
    Default,
    /// Secondary labels, table cells, tab titles: 12 px at the default scale.
    Small,
    /// Badges, counts and keybinding hints: 10 px at the default scale.
    XSmall,
    /// The `ui_font_size` setting.
    Ui,
    /// The `buffer_font_size` setting, for URLs, headers and bodies.
    Editor,
}

impl TextSize {
    fn pixels(self, cx: &App) -> f32 {
        match self {
            Self::Large => 16.,
            Self::Default => 14.,
            Self::Small => 12.,
            Self::XSmall => 10.,
            Self::Ui => FontSizesSetting::get_global(cx).ui_font_size,
            Self::Editor => FontSizesSetting::get_global(cx).buffer_font_size,
        }
    }

    /// This size as rems.
    pub fn rems(self, cx: &App) -> Rems {
        px_to_rems(self.pixels(cx))
    }

    /// This size as the pixel count it resolves to at the current scale.
    pub fn pixels_absolute(self, cx: &App) -> Pixels {
        px(self.pixels(cx))
    }
}

/// The size of a [`crate::Headline`] — screen and section titles. The largest maps to the
/// redesign pack's 24 px workspace heading, the small ones to its 16 px section title.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum HeadlineSize {
    /// 14 px at the default scale.
    XSmall,
    /// 16 px at the default scale (the pack's section title).
    Small,
    /// 18 px at the default scale.
    #[default]
    Medium,
    /// 20 px at the default scale.
    Large,
    /// 24 px at the default scale (the pack's workspace heading).
    XLarge,
}

impl HeadlineSize {
    /// This size as rems.
    pub fn rems(self, _cx: &App) -> Rems {
        let px = match self {
            Self::XSmall => 14.,
            Self::Small => 16.,
            Self::Medium => 18.,
            Self::Large => 20.,
            Self::XLarge => 24.,
        };
        px_to_rems(px)
    }
}

/// Typography helpers on top of [`Styled`], so a component says what the text *is*
/// (`text_ui_sm`) rather than how big it should be.
pub trait StyledTypography: Styled + Sized {
    /// Use the UI font family.
    fn font_ui(self, _cx: &App) -> Self {
        self.font_family(SharedString::new_static(UI_FONT_FAMILY))
    }

    /// Use the content font family (URLs, header values, bodies).
    fn font_buffer(self, _cx: &App) -> Self {
        self.font_family(SharedString::new_static(BUFFER_FONT_FAMILY))
    }

    /// A [`TextSize`] of the caller's choosing.
    fn text_ui_size(self, size: TextSize, cx: &App) -> Self {
        self.text_size(size.rems(cx))
    }

    /// Section titles and empty-state headlines.
    fn text_ui_lg(self, cx: &App) -> Self {
        self.text_size(TextSize::Large.rems(cx))
    }

    /// Body UI text.
    fn text_ui(self, cx: &App) -> Self {
        self.text_size(TextSize::Default.rems(cx))
    }

    /// Secondary labels, table cells, tab titles.
    fn text_ui_sm(self, cx: &App) -> Self {
        self.text_size(TextSize::Small.rems(cx))
    }

    /// Badges, counts and keybinding hints.
    fn text_ui_xs(self, cx: &App) -> Self {
        self.text_size(TextSize::XSmall.rems(cx))
    }

    /// URLs, header values and request/response bodies.
    fn text_buffer(self, cx: &App) -> Self {
        self.text_size(TextSize::Editor.rems(cx))
    }

    /// A [`HeadlineSize`] heading.
    fn text_headline(self, size: HeadlineSize, cx: &App) -> Self {
        self.text_size(size.rems(cx))
    }
}

impl<E: Styled> StyledTypography for E {}

/// A [`TextSize`] as an absolute length, for elements that size themselves in text units.
pub fn text_size_length(size: TextSize, cx: &App) -> AbsoluteLength {
    size.rems(cx).into()
}

#[cfg(test)]
#[path = "typography_tests.rs"]
mod tests;
