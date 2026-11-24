//! Importing a Postman collection (S45a; PRD §6.4 REQ-IO-01/02/06).
//!
//! `import_export::Import` asks for one file with the platform dialog, then everything
//! that follows — the size check, the read, the parse and every write — runs off the
//! foreground (AGENTS.md › no blocking IO on the foreground thread) and lands either as
//! the report modal (REQ-IO-01: imported items, preserved-but-inactive scripts,
//! unsupported auth, warnings) or as one error toast naming the file and the reason
//! (AGENTS.md › errors reach the user).
//!
//! **The imported file is untrusted data** (AGENTS.md › Security):
//! - Sizes and nesting depth are capped *before* the parse ([`api_model::ImportLimits`]:
//!   50 MB per file by default, REQ-IO-06) — the metadata is checked before a single
//!   byte is read.
//! - Imported `event[]` scripts are preserved verbatim and **never executed** (there is
//!   no scripting runtime); the report lists them as preserved-and-inert.
//! - Nothing in the file can steer where this import reads or writes: the only paths
//!   touched are the workspace root and the item placements
//!   [`workspace_store::save_item`] derives from item *names* — which it slugs,
//!   confines to the root and never lets follow a symlink (REQ-WSP-03).
//! - An imported `secret` variable keeps no value in the model
//!   (`api_model::VariableKind::Secret`), so no workspace file can carry one
//!   (REQ-VAR-SHARE-02).
//!
//! The writes go through [`workspace_store::save_item`] — one file per item, the tree
//! comes back through S42a's watcher — and each container lands childless first and is
//! written once more with its `order` after every child landed, so an `order` array
//! never names a file that is not there (the S40a duplicate's shape). A write that fails
//! takes the files this import wrote back out again (same shape's rollback) and reports
//! the reason.
//!
//! Two doors in:
//! - [`init`] — the app-level `import_export::Import` handler (the
//!   `request_editor::save` shape: registered on the app, so the command palette can
//!   dispatch it wherever the keyboard is).
//! - [`begin_import`] — the import of one chosen file, the dialog answer and the tests
//!   both land here.
//!
//! `Decision:` the report groups its problems by the importer's own wording ("script
//! preserved", "auth type"): `api_model::ImportReport` carries no category field and
//! this step may not change it. The report speaks the import's JSON paths (`item[2].request`)
//! for problems — names can repeat, paths cannot.
//!
//! `Decision:` the redesign pack's S14 *preview* state (destination, conflicts, preview
//! before writing) is not this step's flow: the step's spec is pick → import → report.
//! Duplicate names cannot overwrite anything — `save_item` slugs every name onto a free
//! file — and the report names exactly what was written.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use api_model::{
    Child, Collection, ImportError, ImportLimits, ImportReport, ItemId, Request, import_collection,
};
use gpui::{
    AnyWindowHandle, App, Context, DismissEvent, ElementId, Entity, EventEmitter, FocusHandle,
    Focusable, Global, PathPromptOptions, Render, Window, div,
};
use pustman_actions::import_export::Import;
use serde_json::Value;
use ui::prelude::*;
use workspace::{ModalView, Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

#[cfg(test)]
#[path = "import_tests.rs"]
mod tests;

/// The directory top-level collections live in (REQ-WSP-01), the same one the CRUD
/// creates use.
const COLLECTIONS_DIR: &str = "collections";

/// How many report rows the modal draws before the list scrolls (the report's lists are
/// unbounded — an untrusted file decides their length).
const MAX_REPORT_ROWS: usize = 10;

/// One report row's height, fixed like the tree's rows so [`VirtualList`] can virtualize
/// against it.
const REPORT_ROW_HEIGHT_PX: f32 = 24.;

/// What to say when the import is asked for with no workspace open.
const NO_WORKSPACE_MESSAGE: &str = "No workspace is open — open one to import a collection.";

/// What to say when the open workspace must not be written (REQ-WSP-04) — the same
/// message the tree's writes use.
const READ_ONLY_MESSAGE: &str =
    "This workspace is read-only: it declares a newer format version than this build writes.";

/// The app-wide wiring: the init guard (one registration per app).
#[derive(Default)]
struct ImportWiring {
    started: bool,
}

impl Global for ImportWiring {}

/// Wire `import_export::Import` (REQ-IO-01) once per app. Registered app-level
/// (`App::on_action`), so the action is dispatchable — and listed in the command
/// palette — wherever the keyboard is.
pub fn init(cx: &mut App) {
    if cx.default_global::<ImportWiring>().started {
        return;
    }
    cx.default_global::<ImportWiring>().started = true;
    cx.on_action(import_action);
}

/// `import_export::Import`: ask for the collection file, then import it. The action
/// dispatch runs inside the window's own update and a window cannot be updated from
/// there, so the question is deferred (the `command_palette` toggle's shape).
fn import_action(_: &Import, cx: &mut App) {
    let Some(window_handle) = cx.active_window() else {
        return;
    };
    cx.defer(move |cx| {
        let prompted = window_handle.update(cx, |root, _window, cx| {
            let Ok(workspace) = root.downcast::<Workspace>() else {
                return;
            };
            let Some(target) = writable_root(&workspace, cx) else {
                // Reported: the workspace is missing or read-only and already said why.
                return;
            };
            let prompt = cx.prompt_for_paths(PathPromptOptions {
                files: true,
                directories: false,
                multiple: false,
                prompt: Some(SharedString::new_static("Import a Postman collection")),
            });
            cx.spawn(async move |cx| {
                let chosen = match prompt.await {
                    Ok(Ok(paths)) => paths.and_then(|paths| paths.into_iter().next()),
                    Ok(Err(error)) => {
                        cx.update(|cx| {
                            land(
                                Err(format!("The file dialog could not be opened — {error}")),
                                window_handle,
                                cx,
                            );
                        });
                        return;
                    }
                    // The dialog went away with its window.
                    Err(_canceled) => return,
                };
                let Some(source) = chosen else {
                    // The user cancelled the dialog: nothing was asked of the workspace.
                    return;
                };
                cx.update(|cx| begin_import(target, source, window_handle, cx));
            })
            .detach();
        });
        if prompted.is_err() {
            // Handled: the window closed before the dialog could open; there is nobody
            // left to ask.
        }
    });
}

/// Import the Postman collection at `source` into the workspace at `root` (S45a). The
/// size check, the read, the parse and every write run off the foreground; what the
/// import produced — or why it could not — lands on `window_handle` as the report modal
/// or one error toast.
pub(crate) fn begin_import(
    root: PathBuf,
    source: PathBuf,
    window_handle: AnyWindowHandle,
    cx: &mut App,
) {
    cx.spawn(async move |cx| {
        let outcome = cx
            .background_executor()
            .spawn(async move { import_work(&root, &source) })
            .await;
        cx.update(|cx| land(outcome, window_handle, cx));
    })
    .detach();
}

/// The import of one file against one workspace root: untrusted input in, workspace
/// files and a report out. Pure disk work — it runs on a background task and knows
/// nothing about windows.
fn import_work(root: &Path, source: &Path) -> Result<ImportOutcome, String> {
    let limits = ImportLimits::default();
    // The cap runs against the file's metadata before a byte is read (REQ-IO-06): a file
    // over it is refused, never pulled into memory.
    let size = std::fs::metadata(source)
        .map_err(|error| unreadable_message(source, &error))?
        .len();
    if size > limits.max_bytes as u64 {
        return Err(too_large_message(source));
    }
    let bytes = std::fs::read(source).map_err(|error| unreadable_message(source, &error))?;
    let (collection, report) =
        import_collection(&bytes, &limits).map_err(|error| import_error_message(source, &error))?;
    let collection_name = collection.name.clone();
    let imported = write_imported_items(root, &collection)?;
    Ok(ImportOutcome {
        collection_name,
        imported,
        report,
    })
}

/// Write the imported collection and everything in it through
/// [`workspace_store::save_item`], returning the item names in the order they landed.
/// A failure removes every file this run wrote (the S40a duplicate's rollback): a failed
/// import leaves no half-collection behind.
fn write_imported_items(root: &Path, collection: &Collection) -> Result<Vec<String>, String> {
    let mut written = Vec::new();
    let mut names = Vec::new();
    let mut body = collection.clone();
    body.order = Vec::new();
    body.children = Vec::new();
    let container = Item {
        body: ItemBody::Collection(body),
        dir: PathBuf::from(COLLECTIONS_DIR),
        path: None,
    };
    let outcome = write_container(
        root,
        container,
        &collection.children,
        &mut written,
        &mut names,
    );
    if let Err(error) = outcome {
        return Err(with_rollback(root, error, &mut written));
    }
    Ok(names)
}

/// Write one container — the collection or a folder — and everything inside it. The
/// container lands childless first (its `order` must never name a file that is not
/// there), then its children go into its `<slug>` directory, and only then is it written
/// once more with the ids that actually landed (the S40a duplicate's shape).
fn write_container(
    root: &Path,
    mut container: Item,
    children: &[Child],
    written: &mut Vec<PathBuf>,
    names: &mut Vec<String>,
) -> Result<ItemId, String> {
    let name = container.name();
    let id = container.id().ok_or_else(|| {
        format!("Could not write “{name}” into the workspace: the import gave it no id")
    })?;
    let saved = save_item_tracked(root, &container, written, names)?;
    let children_dir = saved.parent().map(Path::to_path_buf).ok_or_else(|| {
        format!("Could not write “{name}” into the workspace: its file has no directory")
    })?;
    let landed = write_children(root, &children_dir, children, written, names)?;
    match &mut container.body {
        ItemBody::Collection(collection) => collection.order = landed,
        ItemBody::Folder(folder) => folder.order = landed,
        // Handled: `write_container` is only ever called with a collection or folder.
        _ => {}
    }
    container.path = Some(saved);
    workspace_store::save_item(root, &container)
        .map_err(|error| format!("Could not write “{name}” into the workspace: {error}"))?;
    Ok(id)
}

/// Write each child into `dir` and return the ids that landed, in order.
fn write_children(
    root: &Path,
    dir: &Path,
    children: &[Child],
    written: &mut Vec<PathBuf>,
    names: &mut Vec<String>,
) -> Result<Vec<ItemId>, String> {
    let mut landed = Vec::new();
    for child in children {
        landed.push(write_child(root, dir, child, written, names)?);
    }
    Ok(landed)
}

/// Write one imported child: a folder recurses through [`write_container`], a request is
/// one file. Returns the id its file carries, so the parent's `order` names exactly what
/// landed.
fn write_child(
    root: &Path,
    dir: &Path,
    child: &Child,
    written: &mut Vec<PathBuf>,
    names: &mut Vec<String>,
) -> Result<ItemId, String> {
    match child {
        Child::Folder(folder) => {
            let mut body = folder.clone();
            body.order = Vec::new();
            body.children = Vec::new();
            let container = Item {
                body: ItemBody::Folder(body),
                dir: dir.to_path_buf(),
                path: None,
            };
            write_container(root, container, &folder.children, written, names)
        }
        Child::Request(request) => {
            let mut body = request.clone();
            let id = match &mut body {
                Request::Http(http) => http.id,
                Request::Unsupported(raw) => preserved_id(raw)?,
            };
            let item = Item {
                body: ItemBody::Request(body),
                dir: dir.to_path_buf(),
                path: None,
            };
            save_item_tracked(root, &item, written, names)?;
            Ok(id)
        }
    }
}

/// The identity a preserved-verbatim request carries: its own `id` when it has one this
/// build can read, and a fresh one written into the raw JSON when it does not — the
/// duplicate's `reidentify` shape — so `open` reads the file back as an item and the
/// parent's `order` names the same id.
fn preserved_id(raw: &mut Value) -> Result<ItemId, String> {
    let Some(object) = raw.as_object_mut() else {
        return Err(String::from(
            "a preserved request is not a JSON object; it cannot become a workspace file",
        ));
    };
    if let Some(id) = object
        .get("id")
        .and_then(Value::as_str)
        .and_then(|id| id.parse::<ItemId>().ok())
    {
        return Ok(id);
    }
    let id = ItemId::new();
    object.insert(String::from("id"), Value::from(id.to_string()));
    Ok(id)
}

/// Save `item`, remembering the file it landed in (the rollback's list) and the name it
/// wrote (the report's list).
fn save_item_tracked(
    root: &Path,
    item: &Item,
    written: &mut Vec<PathBuf>,
    names: &mut Vec<String>,
) -> Result<PathBuf, String> {
    let name = item.name();
    let saved = workspace_store::save_item(root, item)
        .map_err(|error| format!("Could not write “{name}” into the workspace: {error}"))?;
    written.push(saved.clone());
    names.push(name);
    Ok(saved)
}

/// Undo a failed import: the files this run wrote go again (the S40a duplicate's
/// rollback). A file that cannot be removed is named in the user's message, never
/// swallowed (AGENTS.md › no discarded fallible results).
fn with_rollback(root: &Path, mut message: String, written: &mut Vec<PathBuf>) -> String {
    let mut cleanup_failure = None;
    for file in written.drain(..) {
        match std::fs::remove_file(root.join(&file)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => cleanup_failure = Some(format!("{}: {error}", file.display())),
        }
    }
    if let Some(cleanup) = cleanup_failure {
        message.push_str(&format!(
            " — and the items already written could not be removed: {cleanup}"
        ));
    }
    message
}

/// What an import produced: the collection's name, the item names that landed, and the
/// report of everything the importer preserved or could not map.
struct ImportOutcome {
    collection_name: String,
    imported: Vec<String>,
    report: ImportReport,
}

/// Show what an import produced: the report modal, or the one message the user gets for
/// a failure. Deferred — a window cannot be updated while it is on the update stack (the
/// `response_viewer` toast reach-through's shape).
fn land(outcome: Result<ImportOutcome, String>, window_handle: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| match outcome {
        Ok(done) => {
            let shown = window_handle.update(cx, |root, window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    return;
                };
                workspace.update(cx, |workspace, cx| {
                    workspace.toggle_modal(window, cx, |_, cx| ImportReportModal::new(done, cx));
                });
            });
            if shown.is_err() {
                // Handled: the window closed before the report could open; the items are
                // on disk and the tree shows them wherever the workspace opens next.
            }
        }
        Err(message) => {
            let shown = window_handle.update(cx, |root, _window, cx| {
                let Ok(workspace) = root.downcast::<Workspace>() else {
                    return;
                };
                workspace.update(cx, |workspace, cx| {
                    workspace.show_toast(message.clone(), Severity::Error, None, cx);
                });
            });
            if shown.is_err() {
                // The window is gone — there is no user left to reach, and the message
                // is all that survives of the failure.
                log::error!("{message}");
            }
        }
    });
}

/// The workspace root this import may write into, or the message that says why it may
/// not — the tree's `writable_root` checks, spoken from here because the import starts
/// above the tree. The same checks run again inside `workspace_store`'s writes, so a
/// snapshot that changed between the check and the write still cannot be written.
fn writable_root(workspace: &Entity<Workspace>, cx: &mut App) -> Option<PathBuf> {
    let Some(opener) = WorkspaceOpen::for_window(workspace.entity_id(), cx) else {
        toast(workspace, NO_WORKSPACE_MESSAGE, cx);
        return None;
    };
    let opened = opener
        .read(cx)
        .snapshot()
        .map(|snapshot| (snapshot.root.clone(), snapshot.read_only));
    let Some((root, read_only)) = opened else {
        toast(workspace, NO_WORKSPACE_MESSAGE, cx);
        return None;
    };
    if read_only {
        toast(workspace, READ_ONLY_MESSAGE, cx);
        return None;
    }
    Some(root)
}

/// Say why an import cannot start — the message is this flow's to the user, never only
/// a log (AGENTS.md › errors reach the user).
fn toast(workspace: &Entity<Workspace>, message: &str, cx: &mut App) {
    workspace.update(cx, |workspace, cx| {
        workspace.show_toast(
            SharedString::from(message.to_owned()),
            Severity::Error,
            None,
            cx,
        );
    });
}

/// The message a file over the import cap earns (REQ-IO-06): the file and the limit, so
/// the user knows what to change. Shared with the tests so both speak one message.
pub(crate) fn too_large_message(source: &Path) -> String {
    let limit_mb = ImportLimits::default().max_bytes / (1024 * 1024);
    format!(
        "Could not import {}: the file is over the {limit_mb} MB import limit.",
        file_label(source)
    )
}

/// The message a file that cannot be read earns: the file and the OS reason.
fn unreadable_message(source: &Path, error: &std::io::Error) -> String {
    format!(
        "Could not import {}: the file could not be read — {error}.",
        file_label(source)
    )
}

/// The message a file the importer refused earns: the file and what the importer found.
fn import_error_message(source: &Path, error: &ImportError) -> String {
    let label = file_label(source);
    match error {
        ImportError::TooLarge { .. } => too_large_message(source),
        ImportError::TooDeep { limit } => {
            format!("Could not import {label}: the JSON nests deeper than {limit} levels.")
        }
        ImportError::Invalid(reason) => {
            format!(
                "Could not import {label}: the file is not a valid Postman collection — {reason}."
            )
        }
        ImportError::MissingField(field) => {
            format!("Could not import {label}: the file is missing its {field} field.")
        }
    }
}

/// The file's own name — messages name the file the user picked, never a path from the
/// import.
fn file_label(source: &Path) -> String {
    source.file_name().map_or_else(
        || source.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}

/// One report line: a section heading or an entry under it.
#[derive(Clone)]
enum ReportLine {
    Heading(SharedString),
    Entry(SharedString),
}

/// The report a finished import shows (REQ-IO-01): what was imported, the scripts it
/// preserved verbatim and never executed, the auth it could not map, and its warnings —
/// one modal over the workspace (the `DeleteItemModal`'s frame).
pub(crate) struct ImportReportModal {
    /// The collection the import wrote.
    collection_name: SharedString,
    /// Sections' headings and entries in draw order — built once, because the report
    /// never changes while the modal is open.
    lines: Vec<ReportLine>,
    // Kept on the view so the modal layer can take keyboard focus on open and give it
    // back on dismiss.
    focus_handle: FocusHandle,
    /// The four REQ-IO-01 sections before they were flattened into [`Self::lines`], kept
    /// so the report's tests can pin the categories (they read the modal the user sees,
    /// not a copy of the flow).
    #[cfg(test)]
    sections: ReportSections,
}

/// A report's categorized problems and imported names ([`ImportReportModal`]'s tests).
#[cfg(test)]
struct ReportSections {
    imported: Vec<SharedString>,
    preserved_scripts: Vec<SharedString>,
    unsupported_auth: Vec<SharedString>,
    warnings: Vec<SharedString>,
}

impl ImportReportModal {
    fn new(outcome: ImportOutcome, cx: &mut Context<Self>) -> Self {
        let mut preserved_scripts = Vec::new();
        let mut unsupported_auth = Vec::new();
        let mut warnings = Vec::new();
        for problem in &outcome.report.problems {
            let line = SharedString::from(format!("{} — {}", problem.path, problem.detail));
            if problem.detail.contains("script preserved") {
                preserved_scripts.push(line);
            } else if problem.detail.contains("auth type") {
                unsupported_auth.push(line);
            } else {
                warnings.push(line);
            }
        }
        let imported = outcome
            .imported
            .into_iter()
            .map(SharedString::from)
            .collect::<Vec<_>>();
        let mut lines = Vec::new();
        push_section(&mut lines, "Imported", &imported);
        push_section(
            &mut lines,
            "Scripts preserved — never executed",
            &preserved_scripts,
        );
        push_section(&mut lines, "Unsupported auth", &unsupported_auth);
        push_section(&mut lines, "Warnings", &warnings);
        Self {
            collection_name: SharedString::from(outcome.collection_name),
            lines,
            focus_handle: cx.focus_handle(),
            #[cfg(test)]
            sections: ReportSections {
                imported,
                preserved_scripts,
                unsupported_auth,
                warnings,
            },
        }
    }

    /// The names the import wrote, in the order they landed.
    #[cfg(test)]
    pub(crate) fn imported(&self) -> &[SharedString] {
        &self.sections.imported
    }

    /// The scripts the import preserved verbatim and never executed (REQ-IO-06).
    #[cfg(test)]
    pub(crate) fn preserved_scripts(&self) -> &[SharedString] {
        &self.sections.preserved_scripts
    }

    /// The auth types the import preserved verbatim rather than mapped.
    #[cfg(test)]
    pub(crate) fn unsupported_auth(&self) -> &[SharedString] {
        &self.sections.unsupported_auth
    }

    /// Everything else the import preserved or could not map.
    #[cfg(test)]
    pub(crate) fn warnings(&self) -> &[SharedString] {
        &self.sections.warnings
    }

    /// Done: dismiss the report.
    fn dismiss(&mut self, cx: &mut Context<Self>) {
        cx.emit(DismissEvent);
    }
}

/// Add one section's heading and entries to `lines` — a section with nothing in it is
/// not shown at all.
fn push_section(lines: &mut Vec<ReportLine>, heading: &str, entries: &[SharedString]) {
    if entries.is_empty() {
        return;
    }
    lines.push(ReportLine::Heading(SharedString::from(format!(
        "{heading} ({})",
        entries.len()
    ))));
    for entry in entries {
        lines.push(ReportLine::Entry(entry.clone()));
    }
}

/// One report row: a heading, or an entry that truncates with its full text in a
/// tooltip (`references/ux-checklist.md`).
fn render_line(index: usize, line: &ReportLine) -> AnyElement {
    let row = h_flex()
        .h(px_to_rems(REPORT_ROW_HEIGHT_PX))
        .min_w_0()
        .items_center();
    match line {
        ReportLine::Heading(text) => row
            .child(
                Label::new(text.clone())
                    .size(LabelSize::XSmall)
                    .color(Color::Muted),
            )
            .into_any_element(),
        ReportLine::Entry(text) => row
            .id(ElementId::named_usize("import-report-entry", index))
            .tooltip(Tooltip::text(text.clone()).builder())
            .child(Label::new(text.clone()).size(LabelSize::Small).truncate())
            .into_any_element(),
    }
}

impl Render for ImportReportModal {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let lines = Arc::new(self.lines.clone());
        let visible_rows = self.lines.len().min(MAX_REPORT_ROWS);
        let list = VirtualList::new(
            ElementId::named_usize("import-report-list", 0),
            self.lines.len(),
            |index| ElementId::named_usize("import-report-line", index),
            move |index, _window, _cx| {
                lines.get(index).map_or_else(
                    // The list is told the line count of the same `lines` it draws keys
                    // from, so this is unreachable — and must not panic if it ever is not.
                    || div().into_any_element(),
                    |line| render_line(index, line),
                )
            },
        );
        div().track_focus(&self.focus_handle).child(
            Modal::new(format!("Imported “{}”", self.collection_name))
                .content(
                    v_flex().gap(DynamicSpacing::Base04.rems(cx)).child(
                        div()
                            .h(px_to_rems(REPORT_ROW_HEIGHT_PX * visible_rows as f32))
                            .child(list),
                    ),
                )
                .footer(
                    h_flex().justify_end().child(
                        Button::new("import-report-done", SharedString::new_static("Done"))
                            .style(ButtonStyle::Filled)
                            .on_click(cx.listener(|this, _event, _window, cx| this.dismiss(cx))),
                    ),
                ),
        )
    }
}

impl Focusable for ImportReportModal {
    /// The modal is one tab stop at the top: its Done button carries the tab order after
    /// it.
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl EventEmitter<DismissEvent> for ImportReportModal {}

impl ModalView for ImportReportModal {}
