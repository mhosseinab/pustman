//! Exporting a collection as Postman v2.1 (S45c, REQ-IO-03).
//!
//! The row menu's Export asks where the file goes and writes it on a background task
//! from a fresh read of the workspace — never from the tree's in-memory snapshot, which
//! may carry local variable values that belong to this user alone (REQ-VAR-SHARE-01,
//! REQ-WSP-06) — and a secret's value is never exported even when a workspace file
//! carries one (REQ-VAR-SHARE-02; workspace files are untrusted, REQ-IO-06).
//!
//! Two doors in:
//! - [`CollectionsTree::begin_export`] — the row menu's Export (the entry is added in
//!   [`crate::crud`]): dialog, background write, then the report.
//! - [`export_work`] — read, assemble, export, write: the part a test pins down.
//!
//! The report is one toast (AGENTS.md › errors reach the user): success names the file
//! and lists every skipped item with its reason (REQ-IO-03), a failure names what went
//! wrong and why.
//!
//! `Decision:` an export writes nothing into the workspace, so it runs on its own
//! detached task rather than the CRUD one-write queue, and a read-only workspace
//! (REQ-WSP-04) may still be exported. The task is detached, not stored on the tree:
//! the file is one the user chose, and closing the panel must not cancel it
//! (AGENTS.md › Async).
//!
//! `Decision:` the target file gets a plain write — the same call
//! `response_viewer::save_body` makes for a file the user just chose. Crash-safe atomic
//! replaces are for `workspace_store`'s own files (ADR-0001).
//!
//! `Decision:` the skipped list is built here, naming each item, because
//! [`api_model::ExportReport`]'s warnings name none and REQ-IO-03 asks for a list of
//! the items; the walk flags exactly the children `export_collection` drops.
//!
//! `Decision:` an `order` entry no folder or request file backs is listed as skipped
//! too: an export that silently drops an ordered item would lie about the collection's
//! contents, and a merge can leave such an entry behind (REQ-COLLAB-02).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use api_model::{Child, ItemId, Request};
use gpui::{AsyncApp, Context, WeakEntity, Window};
use ui::prelude::*;
use workspace::Workspace;
use workspace_store::{ItemBody, WorkspaceSnapshot};

use crate::tree::CollectionsTree;

/// What one export did: the name it wrote the collection under (resolved from the fresh
/// read, so a rename queued ahead of the export is reflected) and every item it had to
/// leave out, each named with its reason (REQ-IO-03).
struct ExportOutcome {
    name: String,
    skipped: Vec<String>,
}

impl ExportOutcome {
    /// The success toast: where the export landed and, when anything was skipped, the
    /// whole list on one line.
    fn message(&self, path: &Path) -> String {
        if self.skipped.is_empty() {
            return format!("Exported “{}” to {}.", self.name, path.display());
        }
        let item_word = if self.skipped.len() == 1 {
            "item"
        } else {
            "items"
        };
        format!(
            "Exported “{}” to {} — skipped {} {item_word}: {}.",
            self.name,
            path.display(),
            self.skipped.len(),
            self.skipped.join("; ")
        )
    }
}

impl CollectionsTree {
    /// The row menu's Export (REQ-IO-03): ask where the Postman v2.1 file goes, then
    /// write it on a background task from a fresh read of the workspace. Success reports
    /// what it skipped (or that it skipped nothing); every failure is a toast naming
    /// what and why (AGENTS.md › errors reach the user).
    pub(crate) fn begin_export(&mut self, id: ItemId, window: &mut Window, cx: &mut Context<Self>) {
        let Some(root) = self.export_root(window, cx) else {
            return;
        };
        let name = match self.snapshot.items.get(&id).map(|item| &item.body) {
            Some(ItemBody::Collection(collection)) => collection.name.clone(),
            _ => {
                self.notify_user(
                    SharedString::new_static("That collection is no longer in this workspace."),
                    Severity::Error,
                    window,
                    cx,
                );
                return;
            }
        };
        // The dialog suggests Postman's own export naming; where it opens is the
        // process's working directory, the one `response_viewer::save_body` uses for a
        // file the user chose.
        let directory = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
        let suggested = suggested_file_name(&name);
        let chosen = cx.prompt_for_new_path(&directory, Some(&suggested));
        let workspace = window
            .root::<Workspace>()
            .and_then(|workspace| workspace)
            .map(|workspace| workspace.downgrade());
        cx.spawn(async move |_this, cx| {
            let path = match chosen.await {
                Ok(Ok(Some(path))) => path,
                // The user dismissed the dialog: their decision, not a failure.
                Ok(Ok(None)) => return,
                Ok(Err(error)) => {
                    report_to_user(
                        &workspace,
                        format!("Could not open the save dialog — {error}"),
                        Severity::Error,
                        cx,
                    );
                    return;
                }
                Err(_canceled) => {
                    report_to_user(
                        &workspace,
                        String::from("The save dialog did not return a file name"),
                        Severity::Error,
                        cx,
                    );
                    return;
                }
            };
            // The write names its own destination; `path` stays here for the report.
            let target = path.clone();
            let outcome = cx
                .background_spawn(async move { export_work(&root, id, &target) })
                .await;
            match outcome {
                Ok(outcome) => {
                    let severity = if outcome.skipped.is_empty() {
                        Severity::Success
                    } else {
                        Severity::Warning
                    };
                    report_to_user(&workspace, outcome.message(&path), severity, cx);
                }
                Err(error) => {
                    report_to_user(
                        &workspace,
                        format!("Could not export “{name}” — {error}"),
                        Severity::Error,
                        cx,
                    );
                }
            }
        })
        .detach();
    }

    /// The workspace root to export from, or [`None`] with a toast when no workspace is
    /// open. A read-only workspace (REQ-WSP-04) exports fine: an export writes nothing
    /// back into it.
    fn export_root(&self, window: &mut Window, cx: &mut Context<Self>) -> Option<PathBuf> {
        if self.snapshot.root.as_os_str().is_empty() {
            self.notify_user(
                SharedString::new_static(
                    "No workspace is open — open one to export its collections.",
                ),
                Severity::Error,
                window,
                cx,
            );
            return None;
        }
        Some(self.snapshot.root.clone())
    }
}

/// Read the workspace, assemble `id`'s collection from it and write it as Postman v2.1
/// to `target` — the part of the export a test pins down (the dialog and the toast live
/// in [`CollectionsTree::begin_export`]).
///
/// Everything comes from the one fresh [`workspace_store::open`] read and nothing from
/// the caller's memory: a local variable value lives only in the per-user store
/// (REQ-WSP-06) and a secret's value never exports at all (REQ-VAR-SHARE-02), so the
/// file the user shares carries neither.
fn export_work(root: &Path, id: ItemId, target: &Path) -> Result<ExportOutcome, String> {
    let snapshot = workspace_store::open(root)
        .map_err(|error| format!("could not read {}: {error}", root.display()))?;
    let mut collection = match snapshot.items.get(&id).map(|item| &item.body) {
        Some(ItemBody::Collection(collection)) => collection.clone(),
        _ => {
            return Err(String::from(
                "it is no longer a collection in this workspace",
            ));
        }
    };
    let mut skipped = Vec::new();
    // The root is its own visited entry: a cycle back to it must not nest it.
    let mut visited = BTreeSet::from([id]);
    collection.children =
        assemble_children(&snapshot, &collection.order, &mut visited, &mut skipped);
    // `ExportReport`'s warnings name no items; `skipped` above reports the same drops
    // with names, so the report itself adds nothing to say (`Decision:` above).
    let (bytes, _) = api_model::export_collection(&collection);
    std::fs::write(target, &bytes)
        .map_err(|error| format!("could not write {}: {error}", target.display()))?;
    Ok(ExportOutcome {
        name: collection.name.clone(),
        skipped,
    })
}

/// The children `order` names, in order, each assembled from the snapshot — the
/// in-memory tree `export_collection` walks. Every entry that cannot become a child is
/// named in `skipped`, and every id is taken once (`visited`, the tree's "exactly once"
/// rule), so a merge artifact claiming one child under two parents, or a cycle, can
/// neither duplicate nor loop.
fn assemble_children(
    snapshot: &WorkspaceSnapshot,
    order: &[ItemId],
    visited: &mut BTreeSet<ItemId>,
    skipped: &mut Vec<String>,
) -> Vec<Child> {
    let mut children = Vec::new();
    for id in order {
        if !visited.insert(*id) {
            continue;
        }
        let Some(item) = snapshot.items.get(id) else {
            skipped.push(unexportable_entry(&id.to_string()));
            continue;
        };
        match &item.body {
            ItemBody::Folder(folder) => {
                let mut folder = folder.clone();
                folder.children = assemble_children(snapshot, &folder.order, visited, skipped);
                children.push(Child::Folder(folder));
            }
            ItemBody::Request(request) => {
                // The one skip REQ-IO-03 names: v2.1 has no request shape for a kind
                // this build does not model. `export_collection` drops the same
                // children; this walk names them.
                if matches!(request, Request::Unsupported(_)) {
                    skipped.push(format!(
                        "“{}”: a request kind v2.1 cannot represent",
                        item.name()
                    ));
                }
                children.push(Child::Request(request.clone()));
            }
            _ => skipped.push(unexportable_entry(&item.name())),
        }
    }
    children
}

/// The warnings-list line for an `order` entry no folder or request file backs.
fn unexportable_entry(label: &str) -> String {
    format!("“{label}”: no folder or request file in the workspace")
}

/// The name the save dialog starts with: Postman's own export naming, with the two path
/// separators a name may carry replaced so the suggestion stays one file name.
fn suggested_file_name(name: &str) -> String {
    format!("{}.postman_collection.json", name.replace(['/', '\\'], "-"))
}

/// Put `message` in front of the user once the export's task has landed — a toast on the
/// workspace window, or the log when that window is already gone (`crud.rs`'s shape:
/// the message names items and paths only, never request content).
fn report_to_user(
    workspace: &Option<WeakEntity<Workspace>>,
    message: String,
    severity: Severity,
    cx: &mut AsyncApp,
) {
    let mut shown = false;
    if let Some(workspace) = workspace {
        shown = workspace
            .update(cx, |workspace, cx| {
                workspace.show_toast(message.clone(), severity, None, cx);
            })
            .is_ok();
    }
    if !shown {
        log::error!("{message}");
    }
}

#[cfg(test)]
#[path = "export_tests.rs"]
mod tests;
