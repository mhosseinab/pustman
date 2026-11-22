//! An inline, contextual message: the fact, and — when there is something to do about
//! it — one small secondary action (the design system's Feedback family: `Banner`,
//! `references/components.md`).
//!
//! Where [`Toast`] says its piece and leaves, a banner stays where the fact belongs:
//! under the URL bar it names the cURL import that just happened. It is compact — one
//! type size for the message and for the action ([`LabelSize::Small`] on both sides:
//! the message's own size and [`ButtonSize::Default`]'s) — and its action is never the
//! primary button of its region.

use std::rc::Rc;

use gpui::{
    App, ClickEvent, ElementId, InteractiveElement, IntoElement, ParentElement, RenderOnce,
    SharedString, Styled, Window, div, prelude::FluentBuilder,
};
use theme::ActiveTheme;

use crate::color::{Color, Severity};
use crate::components::behavior::Clickable;
use crate::components::button::{Button, ButtonSize, ButtonStyle, ClickHandler};
use crate::components::label::{Label, LabelSize};
use crate::h_flex;
use crate::icons::{Icon, IconName, IconSize};
use crate::spacing::DynamicSpacing;
use crate::typography::StyledTypography;

/// One message, in place.
#[derive(Clone, IntoElement)]
pub struct Banner {
    id: ElementId,
    message: SharedString,
    severity: Option<Severity>,
    action: Option<(SharedString, ClickHandler)>,
}

impl Banner {
    /// A banner stating `message` where its owner places it.
    pub fn new(id: impl Into<ElementId>, message: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            message: message.into(),
            severity: None,
            action: None,
        }
    }

    /// Tint it with a severity — the tone of the fact it states.
    pub fn severity(mut self, severity: Severity) -> Self {
        self.severity = Some(severity);
        self
    }

    /// One small secondary action beside the message. A banner never hosts its
    /// region's primary button: it reports a fact, and offers the one way back.
    pub fn action(
        mut self,
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.action = Some((label.into(), Rc::new(on_click)));
        self
    }

    fn icon_name(&self) -> IconName {
        match self.severity {
            Some(Severity::Error) => IconName::CircleAlert,
            Some(Severity::Warning) => IconName::TriangleAlert,
            Some(Severity::Success) => IconName::CircleCheck,
            Some(Severity::Info) | None => IconName::Info,
        }
    }
}

impl RenderOnce for Banner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let icon_name = self.icon_name();
        let Banner {
            id,
            message,
            severity,
            action,
        } = self;
        // Test hooks: `VisualTestContext::debug_bounds` finds the message and the
        // action under the keys the two selector functions in this module hand out
        // (the `toast.rs` pattern).
        let message_selector = banner_message_selector(&message);
        let action_selector = banner_action_selector(&message);

        let banner = h_flex()
            .id(id.clone())
            .font_ui(cx)
            .items_center()
            .gap(DynamicSpacing::Base03.rems(cx))
            .px(DynamicSpacing::Base04.rems(cx))
            .py(DynamicSpacing::Base02.rems(cx))
            .rounded_md()
            .border_1()
            .min_w_0()
            .child(Icon::new(icon_name).size(IconSize::Small))
            // The message wraps, never truncates: it is the fact the banner states
            // and must be readable in full.
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .debug_selector(move || message_selector)
                    .child(Label::new(message).size(LabelSize::Small)),
            )
            .map(|this| match severity {
                Some(severity) => this
                    .bg(severity.background(cx))
                    .border_color(severity.border(cx))
                    .text_color(severity.color(cx)),
                None => this
                    .border_color(cx.theme().colors().border)
                    .text_color(Color::Default.color(cx)),
            });

        let Some((label, on_click)) = action else {
            return banner.into_any_element();
        };
        banner
            .child(
                div()
                    .flex_none()
                    .debug_selector(move || action_selector)
                    .child(
                        Button::new((id, "action"), label)
                            .style(ButtonStyle::Subtle)
                            .size(ButtonSize::Default)
                            .on_click(move |event, window, cx| on_click(event, window, cx)),
                    ),
            )
            .into_any_element()
    }
}

/// The debug key a [`Banner`]'s message renders under — the test hook that lets
/// `VisualTestContext::debug_bounds` prove one exact message is on screen (the
/// `toast.rs` pattern). It is anchored to the message so a caller and its test share
/// one string without knowing the element id the caller gave the banner.
pub fn banner_message_selector(message: &str) -> String {
    format!("banner-message-{message}")
}

/// The debug key a [`Banner`]'s action button renders under (see
/// [`banner_message_selector`]).
pub fn banner_action_selector(message: &str) -> String {
    format!("banner-action-{message}")
}
