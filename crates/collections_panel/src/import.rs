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
//! - The read is *bounded* — at most `ImportLimits.max_bytes` + 1 bytes (50 MB by
//!   default, REQ-IO-06) — and a document nesting past the depth cap is refused before
//!   the parse. A FIFO or device file that reports any size streams for free no more.
//! - Imported `event[]` scripts are preserved verbatim and **never executed** (there is
//!   no scripting runtime); the report lists them as preserved-and-inert.
//! - Nothing in the file can steer where this import reads or writes: the only paths
//!   touched are the workspace root and the item placements
//!   [`workspace_store::save_item`] derives from item *names* — which it slugs,
//!   confines to the root and never lets follow a symlink (REQ-WSP-03). Names are
//!   shortened and Windows-reserved ones adjusted first, so every slug is a file name
//!   the filesystems accept.
//! - The file never chooses identity or modelled fields: every `extra` key that names a
//!   modelled field is dropped before the save (a flattened extra would overwrite that
//!   field — `settings` could turn TLS off, `id` could collide with an existing item),
//!   and every item is written under a fresh id this build minted.
//! - An imported `secret` variable keeps no value in the model
//!   (`api_model::VariableKind::Secret`), so no workspace file can carry one
//!   (REQ-VAR-SHARE-02); a request whose credential arrived as a literal is reported by
//!   name so the user can move it to one.
//! - The item count is capped ([`MAX_IMPORTED_ITEMS`]): an untrusted file may claim any
//!   size of tree.
//!
//! The writes go through [`workspace_store::save_item`] — one file per item, the tree
//! comes back through S42a's watcher — and each container lands childless first and is
//! written once more with its `order` after every child landed, so an `order` array
//! never names a file that is not there (the S40a duplicate's shape). A write that fails
//! takes everything this import wrote back out again — files and the slug directories it
//! created — and reports the reason.
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
//!
//! `Decision:` the modelled-key list is one union for every `extra` map, not one list
//! per type: a key that names a modelled field is never safe in a flattened extra, and
//! the fix's rule is "strip every modelled field name". The cost is honest over-stripping
//! — a request's `description`, unmapped here, is dropped with a per-key report line —
//! traded against any future type ever being written under a name the file chose.
//! `format_version` is the one carve-out: the importer sets it deliberately.
//!
//! `Decision:` a name over [`MAX_NAME_CHARS`] characters is shortened (and a
//! Windows-reserved name gets " (imported)" appended) rather than left to fail at the
//! filesystem: the file name is the one thing the workspace format derives from untrusted
//! input, and both changes are reported. `workspace_store`'s slug rules do the rest
//! (`../`, absolute paths and punctuation fold to plain slugs on their own).
//!
//! `Decision:` "literal credential" means a value that is not one well-formed `{{name}}`
//! reference: a value that merely contains one (`Bearer {{token}}`) still carries its
//! literal surroundings and is reported. The report names the request, folder or
//! collection and never the value.
//!
//! `Decision:` a preserved-verbatim request is stored under a `postman_raw` wrapper key
//! (fix 2's MEDIUM): the top level keeps only `name` and the id this build mints, so
//! `parse_request` can never read the item back as a typed request — a payload shaped
//! like one (`type`, `method`, `url`, `settings`, …) stays inert data under the wrapper,
//! exactly what the report calls it.
//!
//! `Decision:` the rollback claims a directory as created only when it could verify it
//! was not there before the write — every ancestor on the way, `collections/` included
//! (a `create_dir_all` makes the whole chain). A directory that cannot be listed is kept
//! as it is and the reason is reported, success or failure.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use api_model::{
    Auth, Body, Child, Collection, ImportError, ImportLimits, ImportProblem, ImportReport, ItemId,
    KeyValue, Request, import_collection,
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

#[cfg(test)]
#[path = "graphql_io_tests.rs"]
mod graphql_io_tests;

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

/// How many items one import may write (fix 1): the tree's own scale (REQ-NFR-02's
/// 10,000 requests) is what this build draws at 60 fps, and an untrusted file may claim
/// any number of them.
const MAX_IMPORTED_ITEMS: usize = 10_000;

/// How long an imported name may be before it is shortened (fix 1): long enough for any
/// real name, short enough that the slug derived from it is a file name every filesystem
/// accepts.
const MAX_NAME_CHARS: usize = 100;

/// The bare file names Windows reserves whatever the extension (fix 1): a name folding
/// to one of these would be a file no Windows checkout can hold.
const RESERVED_DEVICE_NAMES: [&str; 22] = [
    "con", "prn", "aux", "nul", "com1", "com2", "com3", "com4", "com5", "com6", "com7", "com8",
    "com9", "lpt1", "lpt2", "lpt3", "lpt4", "lpt5", "lpt6", "lpt7", "lpt8", "lpt9",
];

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
    let Some(window_handle) = workspace::action_window(cx) else {
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
    let bytes = read_bounded(source, &limits)?;
    let (mut collection, mut report) =
        import_collection(&bytes, &limits).map_err(|error| import_error_message(source, &error))?;
    if count_items(&collection) > MAX_IMPORTED_ITEMS {
        return Err(too_many_items_message(source));
    }
    harden(&mut collection, &mut report.problems);
    let collection_name = collection.name.clone();
    let imported = write_imported_items(root, &collection, &mut report.problems)?;
    Ok(ImportOutcome {
        collection_name,
        imported,
        report,
    })
}

/// Read at most [`ImportLimits::max_bytes`] + 1 bytes and refuse what came in over the
/// cap (REQ-IO-06). The read is bounded rather than sized-then-read: a FIFO or a device
/// reports any length (or none) and would otherwise stream forever.
fn read_bounded(source: &Path, limits: &ImportLimits) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(source).map_err(|error| unreadable_message(source, &error))?;
    let mut bytes = Vec::new();
    file.take(limits.max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| unreadable_message(source, &error))?;
    if bytes.len() > limits.max_bytes {
        return Err(too_large_message(source));
    }
    Ok(bytes)
}

/// How many items the imported tree holds: the collection and everything inside it.
fn count_items(collection: &Collection) -> usize {
    fn count(children: &[Child]) -> usize {
        children
            .iter()
            .map(|child| match child {
                Child::Folder(folder) => 1 + count(&folder.children),
                Child::Request(_) => 1,
            })
            .sum()
    }
    1 + count(&collection.children)
}

/// Write the imported collection and everything in it through
/// [`workspace_store::save_item`], returning the item names in the order they landed.
/// A failure takes everything this run wrote back out (the S40a duplicate's rollback):
/// a failed import leaves no half-collection behind. What the writes could not verify
/// is reported either way (fix 2).
fn write_imported_items(
    root: &Path,
    collection: &Collection,
    problems: &mut Vec<ImportProblem>,
) -> Result<Vec<String>, String> {
    let mut written = Written::default();
    let mut body = collection.clone();
    body.order = Vec::new();
    body.children = Vec::new();
    let container = Item {
        body: ItemBody::Collection(body),
        dir: PathBuf::from(COLLECTIONS_DIR),
        path: None,
    };
    let outcome = write_container(root, container, &collection.children, &mut written);
    match outcome {
        Ok(_) => {
            problems.append(&mut written.notes);
            Ok(written.names)
        }
        Err(error) => {
            let mut message = with_rollback(root, error, &mut written);
            for note in std::mem::take(&mut written.notes) {
                message.push_str(&format!(" — {}: {}", note.path, note.detail));
            }
            Err(message)
        }
    }
}

/// Write one container — the collection or a folder — and everything inside it. The
/// container lands childless first (its `order` must never name a file that is not
/// there), then its children go into its `<slug>` directory, and only then is it written
/// once more with the ids that actually landed (the S40a duplicate's shape). Both saves
/// are tracked, so a failure rolls both back.
fn write_container(
    root: &Path,
    mut container: Item,
    children: &[Child],
    written: &mut Written,
) -> Result<ItemId, String> {
    let name = container.name();
    let id = container.id().ok_or_else(|| {
        format!("Could not write “{name}” into the workspace: the import gave it no id")
    })?;
    let saved = save_tracked(root, &container, written)?;
    written.names.push(name.clone());
    let children_dir = saved.parent().map(Path::to_path_buf).ok_or_else(|| {
        format!("Could not write “{name}” into the workspace: its file has no directory")
    })?;
    let landed = write_children(root, &children_dir, children, written)?;
    match &mut container.body {
        ItemBody::Collection(collection) => collection.order = landed,
        ItemBody::Folder(folder) => folder.order = landed,
        // Handled: `write_container` is only ever called with a collection or folder.
        _ => {}
    }
    container.path = Some(saved);
    save_tracked(root, &container, written)?;
    Ok(id)
}

/// Write each child into `dir` and return the ids that landed, in order.
fn write_children(
    root: &Path,
    dir: &Path,
    children: &[Child],
    written: &mut Written,
) -> Result<Vec<ItemId>, String> {
    let mut landed = Vec::new();
    for child in children {
        landed.push(write_child(root, dir, child, written)?);
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
    written: &mut Written,
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
            write_container(root, container, &folder.children, written)
        }
        Child::Request(request) => {
            let mut body = request.clone();
            let id = match &mut body {
                Request::Http(http) => http.id,
                // A file's identity is never trusted for these kinds either: a fresh id,
                // exactly like Unsupported.
                typed @ (Request::GraphQl(_)
                | Request::WebSocket(_)
                | Request::SocketIo(_)
                | Request::Grpc(_)) => {
                    let id = ItemId::new();
                    typed.set_id(id);
                    id
                }
                Request::Unsupported(raw) => {
                    // Identity is ours to mint, never the file's (fix 1): a preserved
                    // request is written under a fresh id whatever its raw `id` said, so
                    // two items can never share one.
                    let Some(object) = raw.as_object_mut() else {
                        return Err(String::from(
                            "a preserved request is not a JSON object; it cannot become a workspace file",
                        ));
                    };
                    let id = ItemId::new();
                    object.insert(String::from("id"), Value::from(id.to_string()));
                    id
                }
            };
            let item = Item {
                body: ItemBody::Request(body),
                dir: dir.to_path_buf(),
                path: None,
            };
            let name = item.name();
            save_tracked(root, &item, written)?;
            written.names.push(name);
            Ok(id)
        }
    }
}

/// What one import has written so far: the files a failure takes back out, the
/// directories those writes created (taken out too), the names for the report, and what
/// it could not verify (also reported).
#[derive(Default)]
struct Written {
    files: Vec<PathBuf>,
    dirs: Vec<PathBuf>,
    names: Vec<String>,
    notes: Vec<ImportProblem>,
}

/// Save `item`, remembering everything a failure must take back out (fix 2): the file
/// it landed in and **every** directory on the way to it that this save created —
/// `create_dir_all` makes the whole missing chain, `collections/` included. A directory
/// that could not be listed before the write is never claimed (it is kept as it is) and
/// the reason is reported.
fn save_tracked(root: &Path, item: &Item, written: &mut Written) -> Result<PathBuf, String> {
    let name = item.name();
    // Before the save: which directories on the way to this item's file are already
    // there. The save creates the rest.
    let mut preexisting = Vec::new();
    let mut walked = PathBuf::new();
    for component in item.dir.components() {
        walked.push(component);
        match std::fs::symlink_metadata(root.join(&walked)) {
            Ok(_) => preexisting.push(walked.clone()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                note_unlistable(written, &walked, &error);
                // "Keep it": an unverifiable directory counts as one to leave alone.
                preexisting.push(walked.clone());
            }
        }
    }
    let listing = match std::fs::read_dir(root.join(&item.dir)) {
        Ok(entries) => Some(
            entries
                .flatten()
                .map(|entry| item.dir.join(entry.file_name()))
                .collect::<Vec<_>>(),
        ),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Some(Vec::new()),
        Err(error) => {
            note_unlistable(written, &item.dir, &error);
            None
        }
    };
    let saved = workspace_store::save_item(root, item)
        .map_err(|error| write_failure_message(&name, &error))?;
    // Every directory the save created: the ancestors that were not there before, and
    // the slug directory its own listing does not show.
    if let Some(parent) = saved.parent() {
        let mut prefix = PathBuf::new();
        for component in parent.components() {
            prefix.push(component);
            let created = if prefix == parent && parent != item.dir.as_path() {
                listing
                    .as_ref()
                    .is_some_and(|listing| !listing.contains(&prefix))
                    && !preexisting.contains(&prefix)
            } else {
                !preexisting.contains(&prefix)
            };
            if created {
                written.dirs.push(prefix.clone());
            }
        }
    }
    written.files.push(saved.clone());
    Ok(saved)
}

/// The report line for a directory that could not be listed before its write (fix 2):
/// the rollback keeps it and the user is told why. One row per directory — a container's
/// two saves see the same one (fix 3).
fn note_unlistable(written: &mut Written, dir: &Path, error: &std::io::Error) {
    let path = dir.display().to_string();
    if written.notes.iter().any(|note| note.path == path) {
        return;
    }
    written.notes.push(ImportProblem {
        path,
        detail: format!("could not be listed before the write ({error}); it is kept as it is"),
    });
}

/// The message a write that could not land earns: the item and the store's reason.
fn write_failure_message(name: &str, error: &workspace_store::SaveError) -> String {
    format!("Could not write “{name}” into the workspace: {error}")
}

/// Undo a failed import (fix 4): every file this run wrote goes again, then the slug
/// directories it created that are now empty — a failed import leaves nothing behind.
/// Every cleanup failure reaches the user's message; none is swallowed (AGENTS.md › no
/// discarded fallible results).
fn with_rollback(root: &Path, mut message: String, written: &mut Written) -> String {
    let mut cleanups = Vec::new();
    for file in written.files.drain(..) {
        match std::fs::remove_file(root.join(&file)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => cleanups.push(format!("{}: {error}", file.display())),
        }
    }
    // Deepest first: a parent is empty only once its children are gone.
    let mut dirs = std::mem::take(&mut written.dirs);
    dirs.sort_by_key(|dir| std::cmp::Reverse(dir.components().count()));
    for dir in dirs {
        match std::fs::remove_dir(root.join(&dir)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            // Not empty: something that is not this import's is in there — nothing this
            // rollback owes.
            Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
            Err(error) => cleanups.push(format!("{}: {error}", dir.display())),
        }
    }
    if !cleanups.is_empty() {
        message.push_str(&format!(
            " — and the items already written could not be removed: {}",
            cleanups.join("; ")
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

// ------------------------------------------------------------------------------------
// Hardening the parsed tree (fix 1): the file never chooses identity or modelled fields.

/// Every name any modelled type serializes: an `extra` key equal to one of these would
/// overwrite a mapped field when the file is saved (`#[serde(flatten)]` writes the extras
/// after the struct's own fields), and the file must never choose a modelled field. One
/// list for every `extra` map — a key that names a modelled field is never safe in a
/// flattened extra, whatever type carries it, and each dropped key is reported.
/// `format_version` is absent on purpose: the importer sets it deliberately.
const MODELLED_KEYS: &[&str] = &[
    "id",
    "name",
    "description",
    "auth",
    "variables",
    "order",
    "children",
    "type",
    "method",
    "url",
    "params",
    "path_variables",
    "headers",
    "body",
    "settings",
    "examples",
    "key",
    "value",
    "enabled",
    "kind",
    "file",
    "content_type",
    "request",
    "response",
    "status",
    "mode",
    "fields",
    "language",
    "text",
    "username",
    "password",
    "token",
    "location",
    "timeout_ms",
    "follow_redirects",
    "max_redirects",
    "follow_original_method",
    "keep_authorization",
    "verify_tls",
    "encode_url",
    "remove_referer",
    "http_version",
    "disable_cookie_jar",
    "disabled_auto_headers",
    "query",
    "operation_name",
];

/// Make the imported tree safe to write (fix 1): every `extra` key that names a
/// modelled field is dropped, a preserved-verbatim request loses any `id` the file
/// carried (identity is minted at write time), names are made safe as file-name sources,
/// and requests carrying literal credentials are reported by name. Everything dropped
/// or changed is a report line.
fn harden(collection: &mut Collection, problems: &mut Vec<ImportProblem>) {
    sanitize_name(&mut collection.name, problems);
    strip_extra(&mut collection.extra, &collection.name, problems);
    for variable in &mut collection.variables {
        strip_extra(&mut variable.extra, &collection.name, problems);
    }
    harden_auth(&mut collection.auth, &collection.name, problems);
    if auth_carries_credential(&collection.auth) {
        problems.push(credential_problem(&collection.name));
    }
    for child in &mut collection.children {
        harden_child(child, problems);
    }
}

/// [`harden`] for one child of a collection or folder.
fn harden_child(child: &mut Child, problems: &mut Vec<ImportProblem>) {
    match child {
        Child::Folder(folder) => {
            sanitize_name(&mut folder.name, problems);
            strip_extra(&mut folder.extra, &folder.name, problems);
            harden_auth(&mut folder.auth, &folder.name, problems);
            if auth_carries_credential(&folder.auth) {
                problems.push(credential_problem(&folder.name));
            }
            for child in &mut folder.children {
                harden_child(child, problems);
            }
        }
        Child::Request(request) => harden_request(request, problems),
    }
}

/// [`harden`] for one request — and for the request an example carries, which is a
/// request too.
fn harden_request(request: &mut Request, problems: &mut Vec<ImportProblem>) {
    match request {
        Request::Http(http) => {
            sanitize_name(&mut http.name, problems);
            strip_extra(&mut http.extra, &http.name, problems);
            for row in http
                .params
                .iter_mut()
                .chain(&mut http.path_variables)
                .chain(&mut http.headers)
            {
                strip_extra(&mut row.extra, &http.name, problems);
            }
            harden_body(&mut http.body, &http.name, problems);
            harden_auth(&mut http.auth, &http.name, problems);
            if let Some(settings) = &mut http.settings {
                strip_extra(&mut settings.extra, &http.name, problems);
            }
            for example in &mut http.examples {
                strip_extra(&mut example.extra, &http.name, problems);
                strip_extra(&mut example.response.extra, &http.name, problems);
                for row in &mut example.response.headers {
                    strip_extra(&mut row.extra, &http.name, problems);
                }
                harden_request(&mut example.request, problems);
            }
            if carries_literal_credential(
                &http.auth,
                &http.url,
                http.params.iter().chain(&http.headers),
            ) {
                problems.push(credential_problem(&http.name));
            }
        }
        // Hardened like HTTP: its own extras and rows lose modelled keys, and a literal
        // credential — auth, header, URL, a credential-named key in the variables text
        // when that text is JSON, or anything in the extras the Postman export writes
        // back (`api_model` keeps unmapped auth, certificate and request keys there) —
        // is reported by name.
        Request::GraphQl(graphql) => {
            sanitize_name(&mut graphql.name, problems);
            strip_extra(&mut graphql.extra, &graphql.name, problems);
            for row in &mut graphql.headers {
                strip_extra(&mut row.extra, &graphql.name, problems);
            }
            harden_auth(&mut graphql.auth, &graphql.name, problems);
            if let Some(settings) = &mut graphql.settings {
                strip_extra(&mut settings.extra, &graphql.name, problems);
            }
            let variables_carry_credential = serde_json::from_str::<Value>(&graphql.variables)
                .is_ok_and(|variables| raw_carries_credential(&variables));
            if variables_carry_credential
                || extra_carries_credential(&graphql.extra)
                || carries_literal_credential(&graphql.auth, &graphql.url, graphql.headers.iter())
            {
                problems.push(credential_problem(&graphql.name));
            }
        }
        // Not hardened field by field yet: made inert exactly like Unsupported. A value
        // that does not serialize becomes `null`, which the writer refuses with a reason.
        Request::WebSocket(_) | Request::SocketIo(_) | Request::Grpc(_) => {
            *request = Request::Unsupported(serde_json::to_value(&*request).unwrap_or(Value::Null));
            harden_request(request, problems);
        }
        Request::Unsupported(raw) => {
            let name = raw.get("name").and_then(Value::as_str).map(str::to_owned);
            let label = name
                .clone()
                .unwrap_or_else(|| String::from("the preserved request"));
            if raw_carries_credential(raw) {
                problems.push(credential_problem(&label));
            }
            let Some(object) = raw.as_object_mut() else {
                // Handled: a preserved request that is not an object has no name to
                // shorten and no id key to drop; the writer refuses it with the reason.
                return;
            };
            if object.remove("id").is_some() {
                problems.push(ignored_key("the preserved request", "id"));
            }
            // A preserved item must never be readable as a typed request (fix 2's
            // MEDIUM): everything it carried goes under `postman_raw` — a key no
            // modelled type serializes — and the top level keeps only the name and the
            // id the writer mints. `parse_request` finds no `type` tag up there, so the
            // item reopens as what it is: inert data, never a sendable request.
            let preserved = std::mem::take(object);
            let mut top = serde_json::Map::new();
            if let Some(name) = name {
                let mut name = name;
                sanitize_name(&mut name, problems);
                top.insert(String::from("name"), Value::from(name));
            }
            top.insert(String::from("postman_raw"), Value::Object(preserved));
            *raw = Value::Object(top);
        }
    }
}

/// [`harden`] for a request body.
fn harden_body(body: &mut Body, label: &str, problems: &mut Vec<ImportProblem>) {
    match body {
        Body::FormData { fields, extra } => {
            strip_extra(extra, label, problems);
            for field in fields {
                strip_extra(&mut field.extra, label, problems);
            }
        }
        Body::UrlEncoded { fields, extra } => {
            strip_extra(extra, label, problems);
            for field in fields {
                strip_extra(&mut field.extra, label, problems);
            }
        }
        Body::Raw { extra, .. } | Body::Binary { extra, .. } => {
            strip_extra(extra, label, problems);
        }
        // A body we do not model keeps its shape verbatim: it has no modelled fields an
        // extra key could override.
        Body::None | Body::Unsupported(_) => {}
    }
}

/// [`harden`] for the auth at one level.
fn harden_auth(auth: &mut Auth, label: &str, problems: &mut Vec<ImportProblem>) {
    match auth {
        Auth::Basic { extra, .. } | Auth::Bearer { extra, .. } | Auth::ApiKey { extra, .. } => {
            strip_extra(extra, label, problems);
        }
        // A preserved auth has no modelled fields to override, and its shape is never
        // the file's to change either. The extended kinds' `extra` can never shadow a
        // typed field: `api_model` drops such a key when it writes the auth.
        Auth::Digest(_)
        | Auth::OAuth2(_)
        | Auth::AwsSigV4(_)
        | Auth::JwtBearer(_)
        | Auth::Inherit
        | Auth::None
        | Auth::Unsupported(_) => {}
    }
}

/// Drop every `extra` key that names a modelled field (fix 1) and say so in the report:
/// the key was ignored, and the mapped field stands as the import set it.
fn strip_extra(
    extra: &mut BTreeMap<String, Value>,
    label: &str,
    problems: &mut Vec<ImportProblem>,
) {
    extra.retain(|key, _| {
        if MODELLED_KEYS.contains(&key.as_str()) {
            problems.push(ignored_key(label, key));
            false
        } else {
            true
        }
    });
}

/// The report line for a key the import refused to let through.
fn ignored_key(label: &str, key: &str) -> ImportProblem {
    ImportProblem {
        path: format!("{label} · {key}"),
        detail: format!(
            "the “{key}” key was ignored: imported data cannot choose identity or modelled fields"
        ),
    }
}

/// Make `name` safe as a file name's source (fix 1): a name over [`MAX_NAME_CHARS`] is
/// shortened so its slug fits every filesystem, and a name Windows reserves (`CON`,
/// `NUL`, …) gets " (imported)" appended so its slug is not a device name. Both changes
/// are reported; anything else stays exactly as the file wrote it.
fn sanitize_name(name: &mut String, problems: &mut Vec<ImportProblem>) {
    if name.chars().count() > MAX_NAME_CHARS {
        problems.push(ImportProblem {
            path: name.clone(),
            detail: format!(
                "the name was shortened to {MAX_NAME_CHARS} characters so its file name fits"
            ),
        });
        *name = name.chars().take(MAX_NAME_CHARS).collect();
    }
    let folded = name
        .chars()
        .filter(|character| character.is_ascii_alphanumeric())
        .collect::<String>()
        .to_ascii_lowercase();
    if RESERVED_DEVICE_NAMES.contains(&folded.as_str()) {
        problems.push(ImportProblem {
            path: name.clone(),
            detail: String::from("the name is reserved as a file name on Windows and was adjusted"),
        });
        name.push_str(" (imported)");
    }
}

/// The names a key or query parameter must have to carry a credential (fix 2).
const CREDENTIAL_KEYS: &[&str] = &[
    "api_key",
    "apikey",
    "token",
    "access_token",
    "password",
    "passwd",
    "secret",
    "client_secret",
    "passphrase",
];

/// The headers that carry a credential in their value.
const CREDENTIAL_HEADERS: &[&str] = &[
    "authorization",
    "proxy-authorization",
    "cookie",
    "x-api-key",
    "x-auth-token",
];

/// Whether `value` is exactly one well-formed `{{name}}` reference — the only shape that
/// counts as templated (fix 2). A value that merely contains one still carries whatever
/// literal text sits around it (`Bearer {{token}}` is a literal `Bearer ` plus a
/// reference, and `{{` with no close is not a reference at all).
fn templated(value: &str) -> bool {
    let Some(inner) = value
        .trim()
        .strip_prefix("{{")
        .and_then(|rest| rest.strip_suffix("}}"))
    else {
        return false;
    };
    !inner.is_empty() && !inner.contains(['{', '}'])
}

/// A value that travels as written — not one well-formed variable reference.
fn literal(value: &str) -> bool {
    !value.trim().is_empty() && !templated(value)
}

/// [`literal`] for the optional values the model carries.
fn literal_opt(value: &Option<String>) -> bool {
    value.as_deref().is_some_and(literal)
}

/// The one report line for something carrying a literal credential: its name, never its
/// value (AGENTS.md › Security).
fn credential_problem(name: &str) -> ImportProblem {
    ImportProblem {
        path: name.to_owned(),
        detail: String::from(
            "a literal credential is imported as plain text — move it to a secret variable",
        ),
    }
}

/// Whether the auth at one level carries a literal credential (fix 2/3): the values of
/// a mapped kind, and any non-template value a preserved kind holds.
fn auth_carries_credential(auth: &Auth) -> bool {
    match auth {
        Auth::Basic { password, .. } => literal_opt(password),
        Auth::Bearer { token, .. } => literal_opt(token),
        Auth::ApiKey { value, .. } => literal_opt(value),
        Auth::Digest(digest) => {
            literal(&digest.password) || extra_carries_credential(&digest.extra)
        }
        Auth::OAuth2(oauth2) => {
            let grant_password = match &oauth2.grant {
                api_model::OAuth2Grant::Password { password, .. } => literal(password),
                _ => false,
            };
            grant_password
                || literal(&oauth2.client_secret)
                || extra_carries_credential(&oauth2.extra)
        }
        Auth::AwsSigV4(aws) => {
            literal(&aws.secret_key)
                || literal_opt(&aws.session_token)
                || extra_carries_credential(&aws.extra)
        }
        Auth::JwtBearer(jwt) => literal(&jwt.secret_or_key) || extra_carries_credential(&jwt.extra),
        // Preserved verbatim: whatever the file held is still in there.
        Auth::Unsupported(raw) => raw_auth_carries_credential(raw),
        Auth::Inherit | Auth::None => false,
    }
}

/// Whether a preserved auth holds a literal (fix 3): any non-template value beyond the
/// `type` tag that names its kind. `{"type":"oauth1"}` declares itself and carries
/// nothing; the hawk shape's `{"key":"token-…"}` carries one.
fn raw_auth_carries_credential(value: &Value) -> bool {
    match value {
        Value::Object(object) => object.iter().any(|(key, value)| {
            (key != "type" && value.as_str().is_some_and(literal))
                || raw_auth_carries_credential(value)
        }),
        Value::Array(items) => items.iter().any(raw_auth_carries_credential),
        _ => false,
    }
}

/// Whether a key/value row carries a literal credential — one implementation for the
/// typed tables' rows and a preserved `{key, value}` pair (fix 3).
fn row_carries_credential(key: &str, value: &Option<String>) -> bool {
    let name = key.trim().to_ascii_lowercase();
    (CREDENTIAL_KEYS.contains(&name.as_str()) || CREDENTIAL_HEADERS.contains(&name.as_str()))
        && literal_opt(value)
}

/// Whether `url` carries a literal credential in its `user:pass@` part or in a query
/// parameter named like one (fix 2). A URL this build cannot parse carries no such
/// check: the notice is advice, not a parser.
fn url_carries_credential(url: &str) -> bool {
    let Ok(parsed) = url::Url::parse(url) else {
        return false;
    };
    parsed.password().is_some_and(literal)
        || parsed.query_pairs().any(|(key, value)| {
            CREDENTIAL_KEYS.contains(&key.trim().to_ascii_lowercase().as_str()) && literal(&value)
        })
}

/// Whether a preserved JSON value holds a literal credential (fix 2/3): the same checks
/// the typed path makes, walked over the raw JSON — a credential-named field or
/// `{key, value}` pair (headers included) with a literal value, a URL string carrying
/// one in its `user:pass@` or its query, and an auth object holding one.
fn raw_carries_credential(value: &Value) -> bool {
    match value {
        Value::Object(object) => {
            let pair = object
                .get("key")
                .and_then(Value::as_str)
                .zip(object.get("value").and_then(Value::as_str))
                .is_some_and(|(key, value)| row_carries_credential(key, &Some(value.to_owned())));
            pair || object.iter().any(|(key, value)| match value.as_str() {
                // A URL — or a request that is one — is checked as a URL: the typed
                // path's check, not a copy of it.
                Some(text) if key == "url" || key == "request" => {
                    url_carries_credential(text)
                        || row_carries_credential(key, &Some(text.to_owned()))
                }
                Some(text) => row_carries_credential(key, &Some(text.to_owned())),
                None => match key.as_str() {
                    "auth" => raw_auth_carries_credential(value) || raw_carries_credential(value),
                    _ => raw_carries_credential(value),
                },
            })
        }
        Value::Array(items) => items.iter().any(raw_carries_credential),
        _ => false,
    }
}

/// Whether a request's preserved `extra` holds a literal credential at any depth — the
/// [`raw_carries_credential`] walk over the extras as one object, so a top-level key
/// keeps its name for the check. Shared with the export's scan: whatever an import
/// preserved is what the export writes back.
pub(crate) fn extra_carries_credential(extra: &BTreeMap<String, Value>) -> bool {
    let object: serde_json::Map<String, Value> = extra
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect();
    raw_carries_credential(&Value::Object(object))
}

/// Whether a request carries a literal credential (fix 2): its auth, a credential
/// header, its URL's `user:pass@` or a query parameter named like one. One check for
/// every typed request kind; the report names the request only and the value never
/// leaves it (AGENTS.md › Security).
fn carries_literal_credential<'a>(
    auth: &Auth,
    url: &str,
    mut rows: impl Iterator<Item = &'a KeyValue>,
) -> bool {
    auth_carries_credential(auth)
        || url_carries_credential(url)
        || rows.any(|row| row_carries_credential(&row.key, &row.value))
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

/// The message a collection over the item cap earns (fix 1): the file and the cap.
pub(crate) fn too_many_items_message(source: &Path) -> String {
    format!(
        "Could not import {}: the collection holds more than {MAX_IMPORTED_ITEMS} items.",
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
