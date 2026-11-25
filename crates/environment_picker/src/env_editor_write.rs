//! One variable written from outside the editor (S48u6): the `{{var}}` popover's value
//! edit and its "Add to Development" / "Add to Globals". It goes through the editor's
//! own save path — the scope is re-read as it is now ([`read_current`]), the one row is
//! changed, and [`persist_scope`] lands it with the same staleness check, the same
//! local-store write and the same file write a Save does — and the installed
//! [`EnvEditorInputs`] follow it ([`refresh_installed_inputs`]), so the wire
//! re-resolves the open requests.
//!
//! `Decision:` a write never touches a secret. A secret's value has exactly one home,
//! the OS keychain, and the popover sends a secret to the editor's masked field instead
//! (`environment::EditSecret`); a write aimed at a secret's key is refused with a
//! message that names the key, never a value.
//!
//! `Decision:` (fix 1) the intent is explicit — [`VariableChange::Edit`] or
//! [`VariableChange::Add`] — and an add never overwrites: a key the scope already holds,
//! enabled or disabled, of any kind, is refused with a message saying what to do.
//!
//! `Decision:` an edit goes to the layer the request resolved the value from: the local
//! value when the scope has one for that key (it is what the request sends, and it is
//! this machine's own), the shared value otherwise. A new variable is added as a
//! Default-kind row with the given shared value.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Variable, VariableKind};
use gpui::{App, Global, Task, WeakEntity};
use storage::{LocalStore, LocalValue};
use ui::prelude::*;
use workspace::Workspace;

use super::{
    Collected, EditTarget, EnvEditor, SavePlan, persist_scope, read_current, read_local_values,
    refresh_installed_inputs,
};

/// Which of a variable's values a write sets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ValueLayer {
    /// The value in the scope's file, shared with everyone who has the workspace.
    Shared,
    /// This machine's local value, in `storage` (REQ-VAR-SHARE-01).
    Local,
}

/// What one write means to do (S48u6 fix 1): the intent is explicit, so an add can
/// never turn into an overwrite.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VariableChange {
    /// Set the value of a variable the scope holds and has enabled — the popover's field.
    /// A key the scope no longer holds (or holds disabled) is refused, never added.
    Edit { value: String, layer: ValueLayer },
    /// Add a variable the scope does not hold — "Add to <scope>". A key the scope holds
    /// in any state (enabled or disabled, any kind) is refused, never overwritten.
    Add { value: String },
}

/// One write: the variable's key and what to do with it.
pub struct VariableWrite {
    pub key: String,
    pub change: VariableChange,
}

/// Where a write lands: the scope and what saving it needs — the fields
/// [`EnvEditorInputs`](super::EnvEditorInputs) carries for the editor.
#[derive(Clone)]
pub struct WriteScope {
    pub workspace_id: String,
    pub workspace_root: PathBuf,
    pub target: EditTarget,
    pub store: Arc<Mutex<LocalStore>>,
}

/// Write one variable into `scope` off the foreground and say how it went in a toast on
/// `workspace` (module docs). Resolves to whether it landed. The task carries its own
/// report, so a caller that cannot keep it may detach it.
pub fn write_variable(
    scope: WriteScope,
    write: VariableWrite,
    workspace: WeakEntity<Workspace>,
    cx: &mut App,
) -> Task<bool> {
    let label = scope.target.label().to_owned();
    if workspace::read_only::is_read_only(cx) {
        toast(
            &workspace,
            workspace::read_only::SAVE_REFUSED_MESSAGE.to_string(),
            Severity::Error,
            cx,
        );
        return Task::ready(false);
    }
    let WriteScope {
        workspace_id,
        workspace_root,
        target,
        store,
    } = scope;
    let scope_id = target.scope_id().to_owned();
    let key = write.key.clone();
    let local_edit = match &write.change {
        VariableChange::Edit {
            value,
            layer: ValueLayer::Local,
        } => Some(value.clone()),
        _ => None,
    };
    let lock = scope_lock(&workspace_id, &scope_id, cx);
    cx.spawn(async move |cx| {
        let outcome = cx
            .background_executor()
            .spawn(async move {
                // One write per scope at a time (fix 1): each re-reads the scope and
                // re-checks its file, so a second write queued behind the first sees
                // what the first wrote instead of failing on it — or losing it.
                let _in_flight = lock
                    .lock()
                    .map_err(|_| String::from("an earlier write to this scope failed midway"))?;
                land_write(&workspace_root, &workspace_id, &target, store, &write)
                    .map(|landed| (landed, workspace_id))
            })
            .await;
        cx.update(|cx| match outcome {
            Ok((landed, workspace_id)) => {
                if let Some(value) = &local_edit {
                    refresh_open_editor(&workspace, &workspace_id, &scope_id, &key, value, cx);
                }
                refresh_installed_inputs(
                    &workspace_id,
                    &scope_id,
                    &landed.item,
                    &landed.file,
                    &landed.scope_locals,
                    cx,
                );
                let message = if landed.added {
                    format!("Added {key} to {label}.")
                } else {
                    format!("Saved {key} in {label}.")
                };
                toast(&workspace, message, Severity::Success, cx);
                true
            }
            Err(reason) => {
                toast(
                    &workspace,
                    format!("Could not save {key} in {label} — {reason}"),
                    Severity::Error,
                    cx,
                );
                false
            }
        })
    })
}

/// The per-scope locks that serialize writes (fix 1), keyed by workspace and scope id.
#[derive(Default)]
struct ScopeWriteLocks(HashMap<String, Arc<Mutex<()>>>);

impl Global for ScopeWriteLocks {}

/// The lock one scope's writes take, created on first use.
fn scope_lock(workspace_id: &str, scope_id: &str, cx: &mut App) -> Arc<Mutex<()>> {
    let locks = cx.default_global::<ScopeWriteLocks>();
    Arc::clone(
        locks
            .0
            .entry(format!("{workspace_id}/{scope_id}"))
            .or_default(),
    )
}

/// A local value landed while the variables editor is open on the same scope (fix 1):
/// its field takes the new value, so the editor's next Save — which writes every row's
/// local value — keeps the edit instead of writing the old one back. (A shared-value
/// write needs no help: the editor's Save re-checks the file and refuses a changed one.)
fn refresh_open_editor(
    workspace: &WeakEntity<Workspace>,
    workspace_id: &str,
    scope_id: &str,
    key: &str,
    value: &str,
    cx: &mut App,
) {
    let Some(workspace) = workspace.upgrade() else {
        return;
    };
    let Some(editor) = workspace.read(cx).modal_layer().active_modal::<EnvEditor>() else {
        return;
    };
    editor.update(cx, |editor, cx| {
        if editor.workspace_id == workspace_id && editor.target.scope_id() == scope_id {
            // `false` means the editor has no plain row under `key`; nothing to refresh.
            if !editor.set_local_value(key, value, cx) {
                // Handled: a renamed or removed row is the editor's own edit.
            }
        }
    });
}

/// What one landed write hands back to the foreground.
struct Landed {
    item: workspace_store::Item,
    file: PathBuf,
    /// The scope's local values after the write: what the installed inputs carry.
    scope_locals: Vec<LocalValue>,
    added: bool,
}

/// The write itself, off the foreground: re-read the scope, change one row, persist it
/// through the editor's save, read the scope's local values back.
fn land_write(
    workspace_root: &Path,
    workspace_id: &str,
    captured: &EditTarget,
    store: Arc<Mutex<LocalStore>>,
    write: &VariableWrite,
) -> Result<Landed, String> {
    let (target, fingerprint) = read_current(workspace_root, captured).map_err(String::from)?;
    let mut variables = target.variables().to_vec();
    let existing = variables
        .iter_mut()
        .find(|variable| variable.key == write.key);
    let mut local_writes = Vec::new();
    let added = match (&write.change, existing) {
        (VariableChange::Add { .. }, Some(variable)) => {
            return Err(existing_refusal(
                &write.key,
                target.label(),
                variable.enabled,
                variable.kind,
            ));
        }
        (VariableChange::Add { value }, None) => {
            variables.push(Variable {
                key: write.key.clone(),
                value: Some(value.clone()),
                kind: VariableKind::Default,
                enabled: true,
                description: None,
                extra: Default::default(),
            });
            true
        }
        (VariableChange::Edit { .. }, None) => {
            return Err(format!(
                "{} is no longer in {} — reopen the request to see its current variables.",
                write.key,
                target.label()
            ));
        }
        (VariableChange::Edit { .. }, Some(variable)) if variable.kind == VariableKind::Secret => {
            return Err(format!(
                "{} is a secret — set its value in the variables editor, where it goes to the OS keychain.",
                write.key
            ));
        }
        (VariableChange::Edit { .. }, Some(variable)) if !variable.enabled => {
            return Err(format!(
                "{} is disabled in {} — enable it in the environment editor.",
                write.key,
                target.label()
            ));
        }
        (VariableChange::Edit { value, layer }, Some(variable)) => {
            match layer {
                ValueLayer::Shared => variable.value = Some(value.clone()),
                ValueLayer::Local => local_writes.push(LocalValue {
                    workspace_id: workspace_id.to_owned(),
                    scope_id: target.scope_id().to_owned(),
                    key: write.key.clone(),
                    value: value.clone(),
                }),
            }
            false
        }
    };
    let item = target
        .saved_item(&variables)
        .map_err(|error| error.to_string())?;
    let plan = SavePlan {
        root: workspace_root.to_path_buf(),
        file_path: target.file_path().map(Path::to_path_buf),
        fingerprint,
        item: item.clone(),
        collected: Collected {
            variables,
            local_writes,
            local_clears: Vec::new(),
            secret_sets: Vec::new(),
            secret_removes: Vec::new(),
        },
        workspace_id: workspace_id.to_owned(),
        scope_id: target.scope_id().to_owned(),
        secret_store: None,
    };
    let written = persist_scope(plan, Arc::clone(&store)).map_err(|failure| failure.message())?;
    // The installed inputs carry the scope's whole local set, so it is read back; a read
    // that fails is said, and the write itself has landed either way.
    let scope_locals = read_local_values(&store, workspace_id)
        .map_err(|error| {
            format!("it was saved, but the local values could not be read back — {error}")
        })?
        .into_iter()
        .filter(|value| value.scope_id == target.scope_id())
        .collect();
    Ok(Landed {
        item,
        file: written.file,
        scope_locals,
        added,
    })
}

/// Why "Add to <scope>" refuses a key the scope already holds (fix 1): an add never
/// overwrites, and the message says what to do instead. It names the key and the scope,
/// never a value.
pub(crate) fn existing_refusal(
    key: &str,
    scope: &str,
    enabled: bool,
    kind: VariableKind,
) -> String {
    match (enabled, kind) {
        (false, VariableKind::Secret) => format!(
            "{key} exists in {scope} as a disabled secret — enable it and set its value in the environment editor."
        ),
        (false, VariableKind::Default) => {
            format!(
                "{key} exists in {scope} but is disabled — enable it in the environment editor."
            )
        }
        (true, _) => format!("{key} already exists in {scope} — edit its value instead."),
    }
}

/// Show `message` on `workspace`, deferred: the workspace may be on the update stack.
fn toast(workspace: &WeakEntity<Workspace>, message: String, severity: Severity, cx: &mut App) {
    let workspace = workspace.clone();
    cx.defer(move |cx| {
        if workspace
            .update(cx, |workspace, cx| {
                workspace.show_toast(message, severity, None, cx);
            })
            .is_err()
        {
            // Handled: the window closed; there is no one left to tell.
        }
    });
}

#[cfg(test)]
#[path = "env_editor_write_tests.rs"]
mod tests;
