//! The "Set <name>" flow behind the `UnresolvedSecret` error panel (PRD §8.6): a secret
//! with no value anywhere is one the OS keychain holds nothing for, so the panel offers a
//! "Set <name>" button per missing name and each opens the variables editor
//! (`environment_picker::env_editor`) with that secret's masked field under the cursor.
//! The value is typed and saved *there* — the editor's own save writes it to the keychain
//! and its module docs hold the save order and every guarantee around it (S43a/S43b).
//! Nothing in this module reads, writes or even names a secret *value*: it names keys.
//!
//! Once the editor goes and the keychain says every missing name is set, the panel
//! offers **Resend**, which dispatches `request::Send` — after that, re-sending works
//! because the send's own keychain read finds what is now stored.
//!
//! `Decision:` "on save" is observed, never assumed. The editor emits no save event — its
//! `DismissEvent` also fires on Cancel and on Escape — so the flow watches the workspace's
//! modal layer and, when the editor goes, re-reads the keychain exactly the way the
//! editor writes it (`environment_picker::secret_values`, same workspace, scope and
//! key). Resend is offered only when every missing name reads back `Found`: an offer to
//! re-send into the same failure is not an offer. A name set through some other path is
//! seen by the same re-read once the flow has watched an editor — until then the request
//! editor's own Send button is the way to re-send.
//!
//! `Decision:` the flow is one app-global entity ([`SetSecretFlow`]) rather than state in
//! [`crate::ResponseView`]: the panel is a per-frame value and this step's Edits set does
//! not reach `ResponseView`, so nothing in the render tree can hold the subscription and
//! the keychain task (AGENTS.md › Async — dropping either cancels it). The entity keeps
//! both as fields and the render side reaches it read-only through the global. When the
//! re-check changes what the panel must show, [`gpui::App::refresh_windows`] — the signal
//! a theme change re-renders on — is what redraws the frozen view tree.
//!
//! `Decision:` `env_editor::Open` is a toggle: clicking Set while an open this flow asked
//! for is still in flight would toggle the freshly opened editor shut. The flow dispatches
//! the action only while no variables editor is on screen and otherwise just moves the row
//! to focus; the second click during the (millisecond) read is the editor's own toggle
//! semantics, and the button opens it again.

use std::collections::BTreeSet;

use environment_picker::env_editor::{EnvEditor, EnvEditorInputs, Open as OpenEditor};
use environment_picker::secret_values::{self, SecretState};
use gpui::{
    Action, AnyWindowHandle, App, Context, Entity, Focusable, Global, Subscription, Task, Window,
};
use pustman_actions::request::Send as SendRequest;
use ui::prelude::*;
use workspace::Workspace;

use crate::child_id;

/// The debug key the "Set <name>" button renders under — the test hook that lets
/// `VisualTestContext::debug_bounds` find one button per missing name (the
/// [`crate::error_panel::error_kind_selector`] shape), keyed by the variable name the
/// button names.
pub fn set_button_selector(name: &str) -> String {
    format!("set-secret-button-{name}")
}

/// The debug key the Resend button renders under while it is shown (see
/// [`set_button_selector`]).
pub const RESEND_BUTTON_SELECTOR: &str = "set-secret-button-resend";

/// What Resend is offered for: it re-sends the request, which runs the same failure
/// again unless every missing secret is set.
const RESEND_LABEL: &str = "Resend";

/// What a "Set <name>" button does, on the label's own terms.
fn set_label(name: &str) -> String {
    format!("Set {name}")
}

/// The row under the `UnresolvedSecret` panel's suggested fix: one "Set <name>" button
/// per missing name, and Resend once they are all set (module docs).
#[derive(Clone, Debug, IntoElement)]
pub struct SetSecretRow {
    /// The panel's stable id — every button's element id derives from it
    /// (`.agents/rules/ui-render.md`).
    id: ElementId,
    /// The missing names the failed send reported, in first-reference order.
    names: Vec<String>,
}

impl SetSecretRow {
    /// The row for `names` — the `UnresolvedSecret` error's own missing names.
    pub fn new(id: impl Into<ElementId>, names: Vec<String>) -> Self {
        Self {
            id: id.into(),
            names,
        }
    }
}

impl RenderOnce for SetSecretRow {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let resend_ready = resend_ready(&self.names, cx);
        let base = self.id;
        h_flex()
            .w_full()
            .flex_wrap()
            .gap(DynamicSpacing::Base03.rems(cx))
            .children(self.names.iter().map(|name| {
                let selector = set_button_selector(name);
                let button_name = name.clone();
                let row_names = self.names.clone();
                div().debug_selector(move || selector).child(
                    Button::new(child_id(&base, format!("set-{name}")), set_label(name))
                        .size(ButtonSize::Compact)
                        .tooltip(Tooltip::text(format!(
                            "Set {name}'s secret value in the variables editor"
                        )))
                        .on_click(move |_event, window, cx| {
                            open_editor_for(&button_name, &row_names, window, cx)
                        }),
                )
            }))
            .when(resend_ready, |this| {
                this.child(
                    div()
                        .debug_selector(|| RESEND_BUTTON_SELECTOR.to_owned())
                        .child(
                            Button::new(child_id(&base, "resend"), RESEND_LABEL)
                                .size(ButtonSize::Compact)
                                .tooltip(Tooltip::text("Send the request again"))
                                .on_click(|_event, window, cx| resend(window, cx)),
                        ),
                )
            })
    }
}

/// Open the variables editor with `focus`'s secret row under the cursor, watching all of
/// `missing` — the panel's missing names — for the save that makes Resend worth offering
/// (module docs). The editor itself is the one this crate knows how to open: the
/// `env_editor::Open` action its crate registers.
pub fn open_editor_for(focus: &str, missing: &[String], window: &mut Window, cx: &mut App) {
    let flow = flow(cx);
    flow.update(cx, |flow, cx| flow.open_editor(focus, missing, window, cx));
}

/// Re-send the request — what Resend offers once every missing name is set. The send
/// button's own action, dispatched so the request editor runs the same send it always
/// does (the `error_panel` "Open in Console" shape).
fn resend(window: &mut Window, cx: &mut App) {
    window.dispatch_action(SendRequest.boxed_clone(), cx);
}

/// The one flow, reached through an app-global (module docs). Created on the first Set
/// click — a render may only read it.
struct FlowHandle(Entity<SetSecretFlow>);

impl Global for FlowHandle {}

fn flow(cx: &mut App) -> Entity<SetSecretFlow> {
    if let Some(handle) = cx.try_global::<FlowHandle>() {
        return handle.0.clone();
    }
    let flow = cx.new(|_| SetSecretFlow::default());
    cx.set_global(FlowHandle(flow.clone()));
    flow
}

/// Whether every missing name `names` now has a keychain value the flow's re-read found:
/// the one condition Resend is offered under (module docs).
fn resend_ready(names: &[String], cx: &App) -> bool {
    !names.is_empty()
        && cx.try_global::<FlowHandle>().is_some_and(|handle| {
            names
                .iter()
                .all(|name| handle.0.read(cx).set.contains(name))
        })
}

/// The flow's state (module docs): who to watch, what the keychain said, and the two
/// pieces of async work whose cancellation would lose the flow.
#[derive(Default)]
struct SetSecretFlow {
    /// The panel's missing names this flow watches, in first-reference order.
    watched: Vec<String>,
    /// Names whose keychain entry the last re-read found (module docs).
    set: BTreeSet<String>,
    /// The name the next opened editor must put the cursor on.
    pending_focus: Option<String>,
    /// A variables editor this flow is watching is on screen.
    editor_open: bool,
    /// The window the panel was clicked in — where the editor opens and the focus lands.
    window_handle: Option<AnyWindowHandle>,
    /// The workspace observation that spots the editor opening and going (module docs).
    watch: Option<Subscription>,
    /// The keychain re-read in flight; replacing it cancels the old (AGENTS.md › Async).
    recheck: Option<Task<()>>,
}

impl SetSecretFlow {
    /// Ask for the editor on `focus`'s row, watching every name in `missing` (module
    /// docs). The editor is opened through its own action; this flow only aims the
    /// cursor and reads the outcome back.
    fn open_editor(
        &mut self,
        focus: &str,
        missing: &[String],
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(Some(workspace)) = window.root::<Workspace>() else {
            // The panel only draws under a workspace shell. A bare window has no editor
            // to open and no toast layer to say so (`save_body`'s documented shape).
            return;
        };
        for name in missing {
            if !self.watched.iter().any(|watched| watched == name) {
                self.watched.push(name.clone());
            }
        }
        self.window_handle = Some(window.window_handle());
        self.watch = Some(cx.observe(&workspace, Self::on_workspace_changed));
        if let Some(editor) = workspace.read(cx).modal_layer().active_modal::<EnvEditor>() {
            // Already on screen: aim the cursor instead of dispatching `env_editor::Open`
            // again, which would toggle the editor shut.
            self.editor_open = true;
            self.focus_secret(focus, &editor, &workspace, cx);
            return;
        }
        self.pending_focus = Some(focus.to_string());
        window.dispatch_action(OpenEditor.boxed_clone(), cx);
    }

    /// The workspace changed: either the editor landed — aim the cursor — or it went,
    /// which is when the keychain is worth re-reading (module docs).
    fn on_workspace_changed(&mut self, workspace: Entity<Workspace>, cx: &mut Context<Self>) {
        match workspace.read(cx).modal_layer().active_modal::<EnvEditor>() {
            Some(editor) => {
                self.editor_open = true;
                if let Some(name) = self.pending_focus.take() {
                    self.focus_secret(&name, &editor, &workspace, cx);
                }
            }
            None => {
                if self.editor_open {
                    self.editor_open = false;
                    self.recheck(cx);
                }
            }
        }
    }

    /// Put the cursor on `name`'s masked value field in `editor` (module docs). A name
    /// the editor has no secret row for is said out loud — the button promised this row
    /// (AGENTS.md: errors reach the user).
    fn focus_secret(
        &mut self,
        name: &str,
        editor: &Entity<EnvEditor>,
        workspace: &Entity<Workspace>,
        cx: &mut Context<Self>,
    ) {
        let Some(field) = editor.read(cx).secret_value_field(name) else {
            let message = format!(
                "`{name}` is not a secret variable in the scope the editor has open — set it there under its own name."
            );
            workspace.update(cx, |workspace, cx| {
                workspace.show_toast(message, Severity::Error, None, cx);
            });
            return;
        };
        let handle = field.read(cx).focus_handle(cx);
        if let Some(window_handle) = self.window_handle {
            cx.defer(move |cx| {
                if window_handle
                    .update(cx, |_, window, cx| window.focus(&handle, cx))
                    .is_err()
                {
                    // Handled: the window closed before the focus landed.
                }
            });
        }
    }

    /// Re-read the keychain for every watched name, exactly the way the editor's save
    /// wrote it (module docs), off the foreground. What it finds decides whether the
    /// panel offers Resend.
    fn recheck(&mut self, cx: &mut Context<Self>) {
        if self.watched.is_empty() {
            return;
        }
        let names = self.watched.clone();
        let Some(inputs) = cx.try_global::<EnvEditorInputs>().cloned() else {
            // Nothing an editor could have saved under: without the inputs the editor
            // refuses to open at all (its own toast says so).
            return;
        };
        let store = secret_values::store(cx);
        self.recheck = Some(cx.spawn(async move |this, cx| {
            let found = cx
                .background_spawn(async move {
                    names
                        .into_iter()
                        .filter(|name| {
                            matches!(
                                secret_values::read_secret(
                                    store.as_ref(),
                                    &inputs.workspace_id,
                                    inputs.target.scope_id(),
                                    name,
                                )
                                .state,
                                SecretState::Found(_)
                            )
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            if this
                .update(cx, |this, cx| {
                    this.set.extend(found);
                    // The panel reads this flow at render and the view tree drawing it is
                    // not this entity's to notify (module docs): the app-wide refresh is
                    // the signal that re-renders it.
                    cx.refresh_windows();
                })
                .is_err()
            {
                // Handled: the flow went away with its window; there is no panel left to
                // offer anything on.
            }
        }));
    }
}

#[cfg(test)]
#[path = "set_secret_tests.rs"]
mod tests;
