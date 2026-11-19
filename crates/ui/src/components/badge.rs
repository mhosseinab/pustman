//! A small labeled tag: a content type, an environment name, a status.
//!
//! The badge is text-first — the color repeats what the text says, it never carries the
//! meaning on its own (REQ-NFR-06).

use gpui::{App, IntoElement, ParentElement, RenderOnce, SharedString, Styled, Window};
use theme::ActiveTheme;

use crate::color::{Color, Severity};
use crate::components::label::{Label, LabelSize};
use crate::h_flex;
use crate::icons::{Icon, IconName, IconSize};
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;

/// A tag with a word in it.
#[derive(Clone, Debug, IntoElement)]
pub struct Badge {
    label: SharedString,
    severity: Option<Severity>,
    icon: Option<IconName>,
    color: Option<Color>,
}

impl Badge {
    /// A neutral badge: metadata about the thing it sits next to.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            severity: None,
            icon: None,
            color: None,
        }
    }

    /// Tint it with a severity (e.g. a red `Error` badge on a failed test).
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = Some(severity);
        self
    }

    /// A glyph before the label.
    pub fn icon(mut self, icon: IconName) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Tint the text with an intent of the caller's choosing.
    pub fn color(mut self, color: Color) -> Self {
        self.color = Some(color);
        self
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let colors = cx.theme().colors();
        let (background, border, text) = match self.severity {
            Some(severity) => (
                severity.background(cx),
                severity.border(cx),
                severity.color(cx),
            ),
            None => (
                colors.element_background,
                colors.border_variant,
                self.color
                    .map(|color| color.color(cx))
                    .unwrap_or(colors.text_muted),
            ),
        };
        h_flex()
            .font_ui(cx)
            .items_center()
            .gap(DynamicSpacing::Base01.rems(cx))
            .px(DynamicSpacing::Base02.rems(cx))
            .py(DynamicSpacing::Base01.rems(cx))
            .rounded_full()
            .border_1()
            .border_color(border)
            .bg(background)
            .text_color(text)
            .children(self.icon.map(|icon| Icon::new(icon).size(IconSize::XSmall)))
            .child(Label::new(self.label).size(LabelSize::XSmall))
    }
}
