//! The variable line under the active table (mockup section 1): every `{{variable}}`
//! the request uses, each with what it resolves to and where from ("Source:
//! Environment"), or — when nothing resolves it — what is missing and a "Set it…" that
//! opens the place to fix it.
//!
//! Security: a secret never shows its value here — a resolved secret is the mask
//! ([`MASK`]), whatever its local value is, and the line's strings are the only things
//! it renders (AGENTS.md › Security). The value of a secret is never read: the entry is
//! built from the binding's kind and whether a value or keychain key exists.
//!
//! `Decision:` "Set it…" runs existing paths only: a secret with no keychain value
//! dispatches `environment::EditSecret` (the S43c door that puts the cursor in its
//! masked field); any other name opens the variables editor through its own
//! `env_editor::Open` action, built by name ([`OPEN_VARIABLES_EDITOR`]) because
//! `environment_picker` depends on this crate and cannot be named from it. A build
//! that fails (the editor is not part of this app) is told to the user as a toast.
//!
//! `Decision:` the source is the winning scope's kind ("Environment", "Global", …),
//! not the environment's display name: the editor's [`variables::ScopeStack`] carries
//! kinds only, and naming the active environment would need the app's scope wire
//! (`crates/pustman`), outside this step.
//!
//! The line is recomputed when the draft or the scopes change
//! ([`RequestEditor::refresh_variable_line`]), never in `render`.

use api_model::{Auth, Body, HttpRequest, MASK, VariableKind};
use gpui::{AnyElement, Context, Window};
use pustman_actions::environment::EditSecret;
use ui::prelude::*;
use variables::{DynamicValues, ScopeKind, ScopeStack, spans};

use crate::RequestEditor;
use crate::variable_hover::{scope_kind_of, scope_label};

/// Test hook: the variable line.
pub const VARIABLE_LINE_SELECTOR: &str = "variable-line";

/// The variables editor's open action, owned by `environment_picker` (module docs).
pub const OPEN_VARIABLES_EDITOR: &str = "env_editor::Open";

/// What the line shows when the variables editor is not part of the app.
const NO_EDITOR_MESSAGE: &str =
    "The variables editor is not available — open a workspace to edit its variables.";

/// How an unresolved name is fixed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariableFix {
    /// Set the secret's value in the keychain (`environment::EditSecret`).
    EditSecret(String),
    /// Open the variables editor on the active scope (`env_editor::Open`).
    OpenVariablesEditor,
}

/// What one name shows on the line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// It resolves: `shown` is its value (the mask for a secret) and `source` the
    /// "Source: …" text.
    Resolved { shown: String, source: String },
    /// Nothing resolves it: `message` says what is missing; `fix` is "Set it…".
    Unresolved { message: String, fix: VariableFix },
}

/// One `{{name}}` of the request and what the line shows for it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VariableLineEntry {
    pub name: String,
    pub state: EntryState,
}

/// Every name the request's enabled parts use, in first-appearance order: the URL, the
/// query and path rows, the headers, the request's own auth fields and a raw body.
fn request_variable_names(draft: &HttpRequest) -> Vec<String> {
    let mut templates: Vec<&str> = vec![draft.url.as_str()];
    for row in draft
        .params
        .iter()
        .chain(&draft.path_variables)
        .chain(&draft.headers)
        .filter(|row| row.enabled)
    {
        templates.push(&row.key);
        templates.extend(row.value.as_deref());
    }
    match &draft.auth {
        Auth::Basic {
            username, password, ..
        } => templates.extend(username.as_deref().into_iter().chain(password.as_deref())),
        Auth::Bearer { token, .. } => templates.extend(token.as_deref()),
        Auth::ApiKey { key, value, .. } => {
            templates.extend(key.as_deref().into_iter().chain(value.as_deref()))
        }
        Auth::Inherit | Auth::None | Auth::Unsupported(_) => {}
    }
    if let Body::Raw { text, .. } = &draft.body {
        templates.push(text);
    }
    let empty = ScopeStack::new();
    let mut names: Vec<String> = Vec::new();
    for template in templates {
        for span in spans(template, &empty) {
            if !names.contains(&span.name) {
                names.push(span.name);
            }
        }
    }
    names
}

impl RequestEditor {
    /// The line as it shows now — what a test reads.
    pub fn variable_line(&self) -> &[VariableLineEntry] {
        &self.variable_line
    }

    /// Recompute the line from the draft and the scopes.
    pub(super) fn refresh_variable_line(&mut self, cx: &mut Context<Self>) {
        let entries: Vec<VariableLineEntry> = request_variable_names(&self.draft)
            .into_iter()
            .map(|name| {
                let state = self.entry_state(&name);
                VariableLineEntry { name, state }
            })
            .collect();
        if entries != self.variable_line {
            self.variable_line = entries;
            cx.notify();
        }
    }

    /// What `name` shows. A secret's value is never read — only whether one exists.
    fn entry_state(&self, name: &str) -> EntryState {
        if DynamicValues::is_dynamic_name(name) {
            return EntryState::Resolved {
                shown: String::from("generated at send"),
                source: String::from("Source: Dynamic"),
            };
        }
        let source = || format!("Source: {}", scope_label(scope_kind_of(&self.scopes, name)));
        match self.scopes.find(name) {
            Some(binding) if binding.variable.kind == VariableKind::Secret => {
                if binding.value.is_some() || self.keychain_resolves(name) {
                    EntryState::Resolved {
                        shown: MASK.to_string(),
                        source: source(),
                    }
                } else {
                    EntryState::Unresolved {
                        message: String::from("is a secret with no value in the keychain."),
                        fix: VariableFix::EditSecret(name.to_string()),
                    }
                }
            }
            Some(binding) => match binding.value {
                Some(value) => EntryState::Resolved {
                    shown: value.to_string(),
                    source: source(),
                },
                None => EntryState::Unresolved {
                    message: format!(
                        "has no value in {}.",
                        scope_label(scope_kind_of(&self.scopes, name))
                    ),
                    fix: VariableFix::OpenVariablesEditor,
                },
            },
            None => {
                let has_environment = self
                    .scopes
                    .scopes()
                    .iter()
                    .any(|scope| scope.kind == ScopeKind::Environment);
                EntryState::Unresolved {
                    message: String::from(if has_environment {
                        "is not defined in the active environment."
                    } else {
                        "is not defined in any scope."
                    }),
                    fix: VariableFix::OpenVariablesEditor,
                }
            }
        }
    }

    /// Run `fix` — the "Set it…" button's path.
    pub(crate) fn apply_variable_fix(
        &self,
        fix: &VariableFix,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match fix {
            VariableFix::EditSecret(name) => {
                window.dispatch_action(Box::new(EditSecret { name: name.clone() }), cx)
            }
            VariableFix::OpenVariablesEditor => {
                match cx.build_action(OPEN_VARIABLES_EDITOR, None) {
                    Ok(action) => window.dispatch_action(action, cx),
                    Err(_) => crate::save::show_toast(
                        window.window_handle(),
                        NO_EDITOR_MESSAGE,
                        Severity::Error,
                        cx,
                    ),
                }
            }
        }
    }

    /// The line under the active table; `None` when the request uses no variables.
    pub(super) fn render_variable_line(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        if self.variable_line.is_empty() {
            return None;
        }
        let mut line = h_flex()
            .id(SharedString::new_static(VARIABLE_LINE_SELECTOR))
            .debug_selector(|| VARIABLE_LINE_SELECTOR.to_string())
            .flex_wrap()
            .min_w_0()
            .gap_x(DynamicSpacing::Base12.rems(cx))
            .gap_y(DynamicSpacing::Base02.rems(cx))
            .py(DynamicSpacing::Base04.rems(cx));
        for entry in &self.variable_line {
            line = line.child(self.render_entry(entry, cx));
        }
        Some(line.into_any_element())
    }

    fn render_entry(&self, entry: &VariableLineEntry, cx: &mut Context<Self>) -> AnyElement {
        let resolved = matches!(entry.state, EntryState::Resolved { .. });
        let severity = if resolved {
            Severity::Success
        } else {
            Severity::Error
        };
        let chip = div()
            .flex_none()
            .px(DynamicSpacing::Base02.rems(cx))
            .rounded_sm()
            .bg(severity.background(cx))
            .when(!resolved, |chip| {
                chip.border_1()
                    .border_dashed()
                    .border_color(severity.border(cx))
            })
            .child(
                Label::new(SharedString::from(format!("{{{{{}}}}}", entry.name)))
                    .size(LabelSize::Small)
                    .color(if resolved {
                        Color::Success
                    } else {
                        Color::Error
                    }),
            );
        let row = h_flex()
            .id(SharedString::from(format!("variable-line-{}", entry.name)))
            .debug_selector({
                let selector = format!("variable-line-{}", entry.name);
                move || selector
            })
            .min_w_0()
            .max_w_full()
            .gap(DynamicSpacing::Base03.rems(cx))
            .child(chip);
        match &entry.state {
            EntryState::Resolved { shown, source } => {
                let is_secret = shown == MASK;
                let value = Label::new(SharedString::from(shown.clone()))
                    .size(LabelSize::Small)
                    .truncate();
                row.child(Label::new(SharedString::new_static("→")).size(LabelSize::Small))
                    .child(
                        div()
                            .id(SharedString::from(format!("variable-value-{}", entry.name)))
                            .min_w_0()
                            .font_buffer(cx)
                            .child(value)
                            // A plain value truncates with its full text in the tooltip; a
                            // secret is only ever the mask, so it gets none.
                            .when(!is_secret, |this| {
                                this.tooltip(Tooltip::text(shown.clone()).builder())
                            }),
                    )
                    .child(
                        Label::new(SharedString::from(source.clone()))
                            .size(LabelSize::Small)
                            .color(Color::Muted),
                    )
                    .into_any_element()
            }
            EntryState::Unresolved { message, fix } => {
                let fix = fix.clone();
                row.child(
                    Label::new(SharedString::from(message.clone()))
                        .size(LabelSize::Small)
                        .color(Color::Error),
                )
                .child(
                    div()
                        .debug_selector({
                            let selector = format!("variable-fix-{}", entry.name);
                            move || selector
                        })
                        .child(
                            Button::new(
                                SharedString::from(format!("variable-fix-{}", entry.name)),
                                SharedString::new_static("Set it…"),
                            )
                            .style(ButtonStyle::Transparent)
                            .label_color(Color::Accent)
                            .tooltip(Tooltip::text(match &fix {
                                VariableFix::EditSecret(_) => {
                                    "Set this secret's value in the keychain"
                                }
                                VariableFix::OpenVariablesEditor => {
                                    "Open the variables editor to define it"
                                }
                            }))
                            .on_click(cx.listener(
                                move |editor, _event, window, cx| {
                                    editor.apply_variable_fix(&fix, window, cx)
                                },
                            )),
                        ),
                )
                .into_any_element()
            }
        }
    }
}

#[cfg(test)]
#[path = "variable_line_tests.rs"]
mod tests;
