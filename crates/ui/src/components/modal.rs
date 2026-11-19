//! The frame around a dialog: title, content, footer.
//!
//! The dialog's *lifecycle* — where it sits, focus trap, returning focus to where it came
//! from — belongs to the workspace's modal layer (S22). What lives here is the frame every
//! dialog wears, so two dialogs cannot come out looking like two different products.

use gpui::{
    AnyElement, App, IntoElement, ParentElement, Rems, RenderOnce, SharedString, Styled, Window,
    div,
};

use crate::color::Color;
use crate::components::label::Headline;
use crate::elevation::StyledExt;
use crate::spacing::DynamicSpacing;
use crate::typography::{HeadlineSize, StyledTypography};
use crate::units::px_to_rems;
use crate::v_flex;

/// The widest a dialog grows: a width a form can still read in.
const DEFAULT_MAX_WIDTH_PX: f32 = 512.;

/// A dialog frame.
#[derive(IntoElement)]
pub struct Modal {
    title: SharedString,
    content: Option<AnyElement>,
    footer: Option<AnyElement>,
    max_width: Option<Rems>,
}

impl Modal {
    /// A dialog with `title` as its heading.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            content: None,
            footer: None,
            max_width: None,
        }
    }

    /// The dialog's body.
    pub fn content(mut self, content: impl IntoElement) -> Self {
        self.content = Some(content.into_any_element());
        self
    }

    /// The dialog's action row, right-aligned by convention.
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Cap the width at something other than [`DEFAULT_MAX_WIDTH_PX`].
    pub fn max_width(mut self, width: Rems) -> Self {
        self.max_width = Some(width);
        self
    }
}

impl RenderOnce for Modal {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let mut modal = v_flex()
            .elevation_3(cx)
            .font_ui(cx)
            .max_w(
                self.max_width
                    .unwrap_or_else(|| px_to_rems(DEFAULT_MAX_WIDTH_PX)),
            )
            .p(DynamicSpacing::Base12.rems(cx))
            .gap(DynamicSpacing::Base08.rems(cx))
            .child(
                Headline::new(self.title)
                    .size(HeadlineSize::Small)
                    .color(Color::Default),
            );
        if let Some(content) = self.content {
            modal = modal.child(div().w_full().min_w_0().child(content));
        }
        if let Some(footer) = self.footer {
            modal = modal.child(footer);
        }
        modal
    }
}
