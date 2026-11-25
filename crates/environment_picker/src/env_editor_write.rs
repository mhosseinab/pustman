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
//! `Decision:` an edit goes to the layer the request resolved the value from: the local
//! value when the scope has one for that key (it is what the request sends, and it is
//! this machine's own), the shared value otherwise. A new variable is added as a
//! Default-kind row with the given shared value.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use api_model::{Variable, VariableKind};
use gpui::{App, Task, WeakEntity};
use storage::{LocalStore, LocalValue};
use ui::prelude::*;
use workspace::Workspace;

use super::{
    Collected, EditTarget, SavePlan, persist_scope, read_current, read_local_values,
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

/// One write: the variable's key, its new value and which value it sets. A key the
/// scope does not hold is added.
pub struct VariableWrite {
    pub key: String,
    pub value: String,
    pub layer: ValueLayer,
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
    cx.spawn(async move |cx| {
        let outcome = cx
            .background_executor()
            .spawn(async move {
                land_write(&workspace_root, &workspace_id, &target, store, &write)
                    .map(|landed| (landed, workspace_id))
            })
            .await;
        cx.update(|cx| match outcome {
            Ok((landed, workspace_id)) => {
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
    let added = existing.is_none();
    let mut local_writes = Vec::new();
    match existing {
        Some(variable) if variable.kind == VariableKind::Secret => {
            return Err(format!(
                "{} is a secret — set its value in the variables editor, where it goes to the OS keychain.",
                write.key
            ));
        }
        Some(variable) => match write.layer {
            ValueLayer::Shared => variable.value = Some(write.value.clone()),
            ValueLayer::Local => local_writes.push(LocalValue {
                workspace_id: workspace_id.to_owned(),
                scope_id: target.scope_id().to_owned(),
                key: write.key.clone(),
                value: write.value.clone(),
            }),
        },
        None => variables.push(Variable {
            key: write.key.clone(),
            value: Some(write.value.clone()),
            kind: VariableKind::Default,
            enabled: true,
            description: None,
            extra: Default::default(),
        }),
    }
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
