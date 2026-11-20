//! The error panel (REQ-RESP-08, PRD §8.6): a failed send is not an empty response — it
//! says what kind of failure it was, what happened and what the user can do about it.
//!
//! The panel speaks with [`TransportError`]'s own words — [`TransportError::user_message`]
//! and [`TransportError::suggested_fix`] — and adds only the error's type label. The
//! "Open in Console" link appears only while `console::Focus` can be dispatched: the
//! Console is a later step, so today there is no link at all — never a stub.

use http_engine::TransportError;
use ui::prelude::*;

use crate::child_id;

/// The action the "Open in Console" link dispatches. Nothing registers it until the
/// Console exists, so the link's visibility is asked of the action registry every frame.
const CONSOLE_FOCUS_ACTION: &str = "console::Focus";

/// The debug key the panel's type label renders under — the test hook that lets
/// `VisualTestContext::debug_bounds` prove the panel is on screen (the `toast.rs`
/// pattern). It is anchored to the label, so a caller and its test share one string.
pub fn error_kind_selector(kind: &str) -> String {
    format!("error-panel-kind-{kind}")
}

/// The debug key the panel's message renders under (see [`error_kind_selector`]).
pub fn error_message_selector(message: &str) -> String {
    format!("error-panel-message-{message}")
}

/// The debug key the panel's suggested-fix line renders under (see [`error_kind_selector`]).
pub fn error_fix_selector(suggested_fix: &str) -> String {
    format!("error-panel-fix-{suggested_fix}")
}

/// The debug key the "Open in Console" link renders under while it is shown
/// (see [`error_kind_selector`]).
pub const OPEN_CONSOLE_SELECTOR: &str = "error-panel-open-console";

/// One failed send on screen: its type, message and suggested fix, tinted by severity.
#[derive(Clone, Debug, IntoElement)]
pub struct ErrorPanel {
    id: ElementId,
    kind: SharedString,
    message: SharedString,
    suggested_fix: SharedString,
    severity: Severity,
}

impl ErrorPanel {
    /// The panel for `error`, identified like every interactive component — by the
    /// caller's stable id, so two viewers in one frame never share element ids. The
    /// texts are computed here, never in `render`.
    pub fn new(id: impl Into<ElementId>, error: &TransportError) -> Self {
        Self {
            id: id.into(),
            kind: error_kind(error).into(),
            message: error.user_message().into(),
            suggested_fix: error.suggested_fix().into(),
            severity: severity(error),
        }
    }

    /// The icon that goes with the severity (the `Toast` mapping, one vocabulary).
    fn icon_name(&self) -> IconName {
        match self.severity {
            Severity::Error => IconName::CircleAlert,
            Severity::Warning => IconName::TriangleAlert,
            Severity::Success => IconName::CircleCheck,
            Severity::Info => IconName::Info,
        }
    }
}

impl RenderOnce for ErrorPanel {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Test hooks: `VisualTestContext::debug_bounds` finds the three texts and the
        // link under the keys the selector functions in this module hand out.
        let kind_selector = error_kind_selector(&self.kind);
        let message_selector = error_message_selector(&self.message);
        let fix_selector = error_fix_selector(&self.suggested_fix);
        // The Console is a later step: `console::Focus` is not registered yet, so the
        // link is hidden rather than stubbed (REQ-RESP-08).
        let console_focus = cx.build_action(CONSOLE_FOCUS_ACTION, None).is_ok();
        let icon_name = self.icon_name();
        let suggested_fix = format!("Suggested fix: {}", self.suggested_fix);

        v_flex()
            .size_full()
            .px(DynamicSpacing::Base06.rems(cx))
            .py(DynamicSpacing::Base04.rems(cx))
            .child(
                v_flex()
                    .w_full()
                    .gap(DynamicSpacing::Base03.rems(cx))
                    .px(DynamicSpacing::Base04.rems(cx))
                    .py(DynamicSpacing::Base03.rems(cx))
                    .rounded_md()
                    .border_1()
                    .bg(self.severity.background(cx))
                    .border_color(self.severity.border(cx))
                    .text_color(self.severity.color(cx))
                    .child(
                        h_flex()
                            .items_center()
                            .gap(DynamicSpacing::Base03.rems(cx))
                            .child(Icon::new(icon_name).size(IconSize::Small))
                            .child(
                                div()
                                    .id(child_id(&self.id, "kind"))
                                    .min_w_0()
                                    .tooltip(Tooltip::text(self.kind.clone()).builder())
                                    .debug_selector(move || kind_selector)
                                    .child(
                                        Label::new(self.kind.clone())
                                            .size(LabelSize::Small)
                                            .truncate(),
                                    ),
                            ),
                    )
                    // The message and the fix wrap, never truncate: both are the reason
                    // the send failed and must be readable in full without a tooltip.
                    .child(
                        div()
                            .min_w_0()
                            .debug_selector(move || message_selector)
                            .child(Label::new(self.message.clone())),
                    )
                    .child(
                        div()
                            .min_w_0()
                            .debug_selector(move || fix_selector)
                            .child(Label::new(suggested_fix).size(LabelSize::Small)),
                    )
                    .map(|this| {
                        if console_focus {
                            this.child(
                                div()
                                    .debug_selector(|| OPEN_CONSOLE_SELECTOR.to_owned())
                                    .child(
                                        Button::new(
                                            child_id(&self.id, "open-console"),
                                            "Open in Console",
                                        )
                                        .size(ButtonSize::Compact)
                                        .start_icon(Icon::new(IconName::ExternalLink))
                                        .tooltip(Tooltip::text("Show this failure in the Console"))
                                        .on_click(
                                            |_event, window, cx| {
                                                // The link renders only while the action
                                                // exists (above); dispatching it is the whole
                                                // handler — there is no no-op path to click.
                                                if let Ok(action) =
                                                    cx.build_action(CONSOLE_FOCUS_ACTION, None)
                                                {
                                                    window.dispatch_action(action, cx);
                                                }
                                            },
                                        ),
                                    ),
                            )
                        } else {
                            this
                        }
                    }),
            )
    }
}

/// How serious this failure is: PRD §8.6 marks a cancelled send as info level, every
/// other kind as an error.
fn severity(error: &TransportError) -> Severity {
    match error {
        TransportError::Cancelled => Severity::Info,
        _ => Severity::Error,
    }
}

/// The error's type label: its variant name. `http_engine` exposes no kind label of its
/// own and this step may not edit it; the match is exhaustive on purpose, so a new
/// variant fails to compile here instead of rendering an anonymous error.
fn error_kind(error: &TransportError) -> &'static str {
    match error {
        TransportError::InvalidUrl { .. } => "InvalidUrl",
        TransportError::Dns { .. } => "Dns",
        TransportError::Connect { .. } => "Connect",
        TransportError::Tls { .. } => "Tls",
        TransportError::Timeout { .. } => "Timeout",
        TransportError::TooManyRedirects { .. } => "TooManyRedirects",
        TransportError::Proxy { .. } => "Proxy",
        TransportError::Protocol { .. } => "Protocol",
        TransportError::Io { .. } => "Io",
        TransportError::FileNotFound { .. } => "FileNotFound",
        TransportError::UnresolvedSecret { .. } => "UnresolvedSecret",
        TransportError::Cancelled => "Cancelled",
    }
}

#[cfg(test)]
#[path = "error_panel_tests.rs"]
mod tests;
