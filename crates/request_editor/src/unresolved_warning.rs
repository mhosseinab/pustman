//! The pre-send unresolved-variable warning (PRD REQ-VAR-07): when a send would go out
//! with `{{names}}` nothing resolves, a non-blocking bar above the URL bar lists the
//! names, offers "Send anyway" and can be dismissed. The check re-runs on every draft
//! edit and every send (`recheck_unresolved`), so the bar always names what the request
//! has *now* — a fixed variable takes it down and a new one brings it back. The warning
//! also travels with the send's outcome into the Console (`send`'s `record_outcome` via
//! `console::feed`): one write, never a second one beside it.
//!
//! `Decision:` the bar's names come from `resolve_request` against the editor's own
//! scopes (`RequestEditor`'s `scopes`) — the very stack the send resolves with (S33e) —
//! minus the secret names the editor's [`SecretSources`](crate::variable_hover::SecretSources)
//! would resolve at send (#198):
//! `http_engine::prepare` fetches those from the keychain before it resolves, so
//! listing them would hold back a send that would resolve them. The rule is the one
//! `prepare` reads by — a secret-kind variable with no local value and a keychain key
//! — and a secret with no key stays on the bar, where `prepare` fails the send as an
//! unresolved secret. Nothing outside tests calls
//! `RequestEditor::set_scopes` until the M2 environment wire (S43w), so that stack is
//! empty in practice and every name is unresolved — the truth today. Cmd/Ctrl-Enter
//! while the bar shows re-checks and shows the bar again; only the bar's own "Send anyway"
//! sends with unresolved names.

use api_model::VariableKind;
use gpui::{AnyElement, Context, Window};
use http_engine::Warning;
use ui::prelude::*;

use crate::RequestEditor;
use crate::variable_hover::scope_kind_of;

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

/// The names the pre-send check reported unresolved, when that is what it reported.
pub(super) fn unresolved_names(warnings: &[Warning]) -> Option<&[String]> {
    warnings.iter().find_map(|warning| match warning {
        Warning::UnresolvedVariables(names) => Some(names.as_slice()),
        Warning::TlsVerifyDisabled
        | Warning::AuthIncomplete { .. }
        | Warning::AuthUnsupported { .. } => None,
    })
}

impl RequestEditor {
    /// The pre-send check's [`Warning`]s as the bar and the send's Console entry see
    /// them (REQ-VAR-07): the names [`Self::keychain_resolves`] are dropped — the send
    /// resolves them, so they are not unresolved (#198) — and a
    /// [`Warning::UnresolvedVariables`] the drop empties goes with them, so nothing
    /// warns about a send that had nothing to warn about.
    pub(super) fn drop_keychain_resolved(&self, warnings: Vec<Warning>) -> Vec<Warning> {
        warnings
            .into_iter()
            .filter_map(|warning| match warning {
                Warning::UnresolvedVariables(names) => {
                    let names: Vec<String> = names
                        .into_iter()
                        .filter(|name| !self.keychain_resolves(name))
                        .collect();
                    (!names.is_empty()).then_some(Warning::UnresolvedVariables(names))
                }
                other => Some(other),
            })
            .collect()
    }

    /// Whether the send's keychain resolves `name` although the editor's scopes cannot
    /// (#198): the winning variable is a secret with no local value and
    /// [`SecretSources`](crate::variable_hover::SecretSources) names its keychain entry. `http_engine::prepare` reads exactly these before it
    /// resolves, so they are resolved at send. Only the key is asked here, never the
    /// value: every keychain read can prompt the user (macOS). A key with nothing
    /// stored still passes — `prepare` then fails the send as an unresolved secret,
    /// which reaches the user.
    fn keychain_resolves(&self, name: &str) -> bool {
        let Some(binding) = self.scopes.find(name) else {
            return false;
        };
        binding.value.is_none()
            && binding.variable.kind == VariableKind::Secret
            && scope_kind_of(&self.scopes, name)
                .is_some_and(|kind| (self.secret_sources.key_of)(kind, name).is_some())
    }

    /// The warning bar's state — [`Some`] while a send would use unresolved variables
    /// (REQ-VAR-07).
    pub fn unresolved_warning(&self) -> Option<&UnresolvedWarning> {
        self.unresolved_warning.as_ref()
    }

    /// Re-run the pre-send check on the draft as it is now (REQ-VAR-07): an edit or a
    /// send re-checks, so the bar lists the names the request has after the change — or
    /// disappears once the change resolved them. The check resolves against the editor's
    /// scopes — the same stack the send uses (S33e) — and knows the keychain the send
    /// reads ([`Self::drop_keychain_resolved`], #198), so only names really unresolved
    /// raise the bar. A draft that does not resolve at all (an unparseable URL mid-edit,
    /// a variable cycle) has no warning to show: the send itself reports that failure
    /// when the user tries it.
    pub(super) fn recheck_unresolved(&mut self, cx: &mut Context<Self>) {
        let ancestors = self.auth_ancestors(cx);
        let checked = match self.presend_check(&ancestors) {
            Ok(warnings) => {
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

#[cfg(test)]
#[path = "unresolved_warning_keychain_tests.rs"]
mod keychain_tests;
