//! The text a screen is made of: [`Label`] for everything inline, [`Headline`] for
//! section and empty-state titles.
//!
//! Sizes are [`LabelSize`]s — intents, not pixels — and colors are [`Color`]s resolved
//! against the theme. A label with no color inherits its container's text color, so a
//! filled button can tint its own caption without the label fighting it.

use gpui::{
    App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window, div,
    prelude::FluentBuilder,
};

use crate::color::Color;
use crate::typography::{HeadlineSize, StyledTypography, TextSize};

/// How prominent a label is. Maps onto the design system's [`TextSize`]s.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum LabelSize {
    /// Section titles and empty-state headlines.
    Large,
    /// Body UI text.
    #[default]
    Default,
    /// Secondary labels, table cells, tab titles.
    Small,
    /// Badges, counts and keybinding hints.
    XSmall,
}

impl LabelSize {
    fn text_size(self) -> TextSize {
        match self {
            Self::Large => TextSize::Large,
            Self::Default => TextSize::Default,
            Self::Small => TextSize::Small,
            Self::XSmall => TextSize::XSmall,
        }
    }
}

/// A piece of text.
#[derive(Clone, Debug, IntoElement)]
pub struct Label {
    text: SharedString,
    size: LabelSize,
    color: Option<Color>,
    truncated: bool,
}

impl Label {
    /// A label in [`LabelSize::Default`], inheriting its container's text color.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            size: LabelSize::Default,
            color: None,
            truncated: false,
        }
    }

    /// Make it this prominent.
    pub fn size(mut self, size: LabelSize) -> Self {
        self.size = size;
        self
    }

    /// Tint it with this intent. Without this the container's color applies.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }

    /// Cut it off with an ellipsis when it does not fit.
    ///
    /// The caller puts the full value in a tooltip — a truncated value the user cannot
    /// read is a dead end (`references/ux-checklist.md`, "text truncates with tooltip").
    pub fn truncate(mut self) -> Self {
        self.truncated = true;
        self
    }
}

impl RenderOnce for Label {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .font_ui(cx)
            .text_size(self.size.text_size().rems(cx))
            .when_some(self.color, |this, color| this.text_color(color.color(cx)))
            .map(|this| {
                if self.truncated {
                    this.min_w_0().truncate()
                } else {
                    this
                }
            })
            .child(self.text)
    }
}

/// A title: a modal's heading, an empty state's headline.
#[derive(Clone, Debug, IntoElement)]
pub struct Headline {
    text: SharedString,
    size: HeadlineSize,
    color: Option<Color>,
}

impl Headline {
    /// A headline in [`HeadlineSize::Medium`].
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            size: HeadlineSize::Medium,
            color: None,
        }
    }

    /// Make it this big.
    pub fn size(mut self, size: HeadlineSize) -> Self {
        self.size = size;
        self
    }

    /// Tint it with this intent.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Headline {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        div()
            .font_ui(cx)
            .text_headline(self.size, cx)
            .when_some(self.color, |this, color| this.text_color(color.color(cx)))
            .child(self.text)
    }
}
