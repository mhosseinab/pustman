//! The pre-send unresolved-variable warning (PRD REQ-VAR-07): when a send would go out
//! with `{{names}}` nothing resolves, a non-blocking bar above the URL bar lists the
//! names, offers "Send anyway" and can be dismissed. The check re-runs on every draft
//! edit and every send (`recheck_unresolved`), so the bar always names what the request
//! has *now* — a fixed variable takes it down and a new one brings it back. The warning
//! also travels with the send's outcome into the Console (`send`'s `log_warned_outcome`
//! via `console::feed`): one write, never a second one beside it.
//!
//! `Decision:` the bar's names come from `resolve_request` with the same empty
//! [`ScopeStack`] the send resolves with — wiring the editor's environment scopes is not
//! this step's job, and the check must agree with the very send it guards. Cmd/Ctrl-Enter
//! while the bar shows re-checks and shows the bar again; only the bar's own "Send anyway"
//! sends with unresolved names.

use gpui::{AnyElement, Context, Window};
use http_engine::{Warning, resolve_request};
use ui::prelude::*;
use variables::{DynamicValues, ScopeStack};

use crate::RequestEditor;

/// Test hook: the warning bar above the URL bar while a send would use unresolved
/// variables.
pub const UNRESOLVED_WARNING_SELECTOR: &str = "unresolved-warning";

/// Test hook: the bar's "Send anyway" button.
pub const SEND_ANYWAY_SELECTOR: &str = "send-anyway";

/// Test hook: the bar's dismiss button.
pub const DISMISS_WARNING_SELECTOR: &str = "dismiss-unresolved-warning";

/// How many names the bar spells out before it counts the rest.
const NAMES_SHOWN: usize = 3;

/// The names a send would use without a value (REQ-VAR-07), in first-appearance order
/// across the request. Names only — a warning never carries values.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnresolvedWarning {
    names: Vec<String>,
}

impl UnresolvedWarning {
    pub(super) fn new(names: Vec<String>) -> Self {
        Self { names }
    }

    /// Every name, in the order the request first uses them.
    pub fn names(&self) -> &[String] {
        &self.names
    }

    /// The bar's line: the first few names spelled out, then how many more it holds
    /// back (the full list is the tooltip).
    pub fn summary(&self) -> String {
        let shown: Vec<&str> = self
            .names
            .iter()
            .take(NAMES_SHOWN)
            .map(String::as_str)
            .collect();
        if self.names.len() <= NAMES_SHOWN {
            shown.join(", ")
        } else {
            format!(
                "{} and {} more",
                shown.join(", "),
                self.names.len() - NAMES_SHOWN
            )
        }
    }

    /// Every name spelled out — the tooltip over the bar's truncated line.
    pub fn full_list(&self) -> String {
        self.names.join(", ")
    }
}

/// The names [`resolve_request`] reported unresolved, when that is what it reported.
pub(super) fn unresolved_names(warnings: &[Warning]) -> Option<&[String]> {
    warnings.iter().find_map(|warning| match warning {
        Warning::UnresolvedVariables(names) => Some(names.as_slice()),
        Warning::TlsVerifyDisabled => None,
    })
}

impl RequestEditor {
    /// The warning bar's state — [`Some`] while a send would use unresolved variables
    /// (REQ-VAR-07).
    pub fn unresolved_warning(&self) -> Option<&UnresolvedWarning> {
        self.unresolved_warning.as_ref()
    }

    /// Re-run the pre-send check on the draft as it is now (REQ-VAR-07): an edit or a
    /// send re-checks, so the bar lists the names the request has after the change — or
    /// disappears once the change resolved them. A draft that does not resolve at all
    /// (an unparseable URL mid-edit, a variable cycle) has no warning to show: the send
    /// itself reports that failure when the user tries it.
    pub(super) fn recheck_unresolved(&mut self, cx: &mut Context<Self>) {
        let checked =
            match resolve_request(&self.draft, &ScopeStack::new(), &mut DynamicValues::new()) {
                Ok((_, warnings)) => {
                    unresolved_names(&warnings).map(|names| UnresolvedWarning::new(names.to_vec()))
                }
                // A request that cannot resolve has no names to warn about; `begin_send`
                // turns the failure into the inline error it deserves.
                Err(_) => None,
            };
        if checked != self.unresolved_warning {
            self.unresolved_warning = checked;
            cx.notify();
        }
    }

    /// Dismiss the bar without sending. A dismiss never sticks past the next check: the
    /// bar comes back if the names are still unresolved when the user edits or sends.
    pub(super) fn dismiss_unresolved_warning(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.unresolved_warning = None;
        cx.notify();
        // The button this click landed on is gone with the bar; a keyboard user's focus
        // must not disappear with it.
        window.focus(&self.focus_handle, cx);
    }

    /// The bar above the URL bar (REQ-VAR-07): the names a send would use without a
    /// value, truncated with the full list in a tooltip, then the two ways past it.
    /// Non-blocking: nothing else in the editor is disabled while it shows.
    pub(super) fn render_unresolved_warning(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let warning = self.unresolved_warning.as_ref()?;
        let full_list = SharedString::from(warning.full_list());
        Some(
            h_flex()
                .id(ElementId::Name(SharedString::new_static(
                    UNRESOLVED_WARNING_SELECTOR,
                )))
                .debug_selector(|| UNRESOLVED_WARNING_SELECTOR.to_string())
                .items_center()
                .gap(DynamicSpacing::Base03.rems(cx))
                .px(DynamicSpacing::Base03.rems(cx))
                .py(DynamicSpacing::Base02.rems(cx))
                .rounded_md()
                .border_1()
                .border_color(Severity::Warning.border(cx))
                .bg(Severity::Warning.background(cx))
                .tooltip(Tooltip::text(full_list).builder())
                .child(
                    Icon::new(IconName::TriangleAlert)
                        .size(IconSize::Small)
                        .color(Color::Warning),
                )
                .child(
                    Label::new(SharedString::from(format!(
                        "Unresolved variables: {}",
                        warning.summary()
                    )))
                    .size(LabelSize::Small)
                    .color(Color::Warning)
                    .truncate(),
                )
                .child(
                    div()
                        .debug_selector(|| SEND_ANYWAY_SELECTOR.to_string())
                        .child(
                            Button::new("send-anyway", SharedString::new_static("Send anyway"))
                                .size(ButtonSize::Medium)
                                .style(ButtonStyle::Outlined)
                                .tooltip(Tooltip::text(SharedString::new_static(
                                    "Send the request with these names left as they are",
                                )))
                                .on_click(cx.listener(|this, _event, window, cx| {
                                    this.send_anyway(window, cx)
                                })),
                        ),
                )
                .child(
                    div()
                        .debug_selector(|| DISMISS_WARNING_SELECTOR.to_string())
                        .child(
                            IconButton::new(
                                "dismiss-unresolved-warning",
                                IconName::X,
                                Tooltip::text(SharedString::new_static("Dismiss this warning")),
                            )
                            .on_click(cx.listener(
                                |this, _event, window, cx| {
                                    this.dismiss_unresolved_warning(window, cx)
                                },
                            )),
                        ),
                )
                .into_any_element(),
        )
    }
}

#[cfg(test)]
#[path = "unresolved_warning_tests.rs"]
mod tests;
