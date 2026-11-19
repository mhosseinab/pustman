//! A line between two things.
//!
//! The color is a choice between the two border tokens: [`DividerColor::Border`] for a
//! strong division, [`DividerColor::BorderVariant`] for a subtle one inside a panel.

use gpui::{App, IntoElement, RenderOnce, Styled, Window, div, px};
use theme::ActiveTheme;

/// How strong a divider draws.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DividerColor {
    /// The strong divider: the edge of a region.
    Border,
    /// The subtle one: between rows inside a panel.
    #[default]
    BorderVariant,
}

/// A one-pixel rule, horizontal or vertical. Borders do not scale with UI density
/// (`references/design-system.md` §4), so it is a fixed pixel line.
#[derive(Clone, Debug, IntoElement)]
pub struct Divider {
    horizontal: bool,
    color: DividerColor,
}

impl Divider {
    /// A rule across the width of what contains it.
    pub fn horizontal() -> Self {
        Self {
            horizontal: true,
            color: DividerColor::default(),
        }
    }

    /// A rule down the height of what contains it.
    pub fn vertical() -> Self {
        Self {
            horizontal: false,
            color: DividerColor::default(),
        }
    }

    /// How strong it draws.
    pub fn color(mut self, color: DividerColor) -> Self {
        self.color = color;
        self
    }

    /// The [`DividerColor::Border`] shorthand.
    pub fn strong(self) -> Self {
        self.color(DividerColor::Border)
    }
}

impl RenderOnce for Divider {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let color = match self.color {
            DividerColor::Border => cx.theme().colors().border,
            DividerColor::BorderVariant => cx.theme().colors().border_variant,
        };
        if self.horizontal {
            div().w_full().h(px(1.)).flex_none().bg(color)
        } else {
            div().h_full().w(px(1.)).flex_none().bg(color)
        }
    }
}
