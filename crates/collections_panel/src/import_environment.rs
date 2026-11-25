//! Importing a Postman environment or globals file (S45b; PRD §6.4 REQ-IO-02/06).
//!
//! `import_export::Import` asks for one file with the platform dialog and the file's own
//! shape decides what it is: a Postman collection goes to S45a's importer unchanged, an
//! environment or globals file comes through here. Everything that follows — the size
//! check, the read, the parse and every write — runs off the foreground (AGENTS.md › no
//! blocking IO on the foreground thread) and lands as one toast: what was imported and
//! where its secrets went, or what went wrong and why (AGENTS.md › errors reach the
//! user).
//!
//! **The imported file is untrusted data** (AGENTS.md › Security):
//! - The read is *bounded* — at most `ImportLimits::max_bytes` + 1 bytes (50 MB by
//!   default, REQ-IO-06) — so a FIFO or device file that reports any size streams for
//!   free no more (the S45a bounded read's shape).
//! - The file never chooses identity or modelled fields: `api_model` mints the
//!   environment's id itself, maps only fields the model has and drops every unknown key
//!   before anything is written — there is no `extra` for a file key to land in.
//! - A `secret` value goes to the OS keychain **only** — never into a workspace file, a
//!   toast, a log or the Console (REQ-IO-02, REQ-VAR-SHARE-02). `workspace_store` strips
//!   secret values from written files on its own as well, and every message this flow
//!   builds names keys and counts, never values.
//! - No keychain, or no installation key for this workspace, is fail-closed: the import
//!   is refused before anything is written (S43w's "fail closed"), never completed with
//!   the values kept somewhere else.
//!
//! What it writes:
//! - An environment becomes one new `environments/<slug>.environment.json` under an id
//!   this build minted, each secret declared as a [`VariableKind::Secret`] row with no
//!   value — so `{{name}}` resolves through the keychain at send time (the send pipeline
//!   reads a secret only through a declared secret variable) and the file records the
//!   key only.
//! - Globals land in `pustman.json`'s `variables` field — the globals' home the
//!   environment editor established — merged by key (see `Decision:` below).
//!
//! Keychain entries are written under the same installation key the app reads them back
//! with (S43w): `pustman::scope_wiring::workspace_key`'s key is installed in
//! [`EnvEditorInputs`], and this write and the app's reads compose through the one
//! `secrets::SecretKey::new(workspace_key, scope_id, variable_key)` constructor
//! (`scope_wiring::secret_key` wraps it; [`SecretChange::set`] builds it here). The scope
//! id is the environment's own minted id, or [`GLOBALS_SCOPE_ID`] for the globals.
//!
//! Two doors in:
//! - [`init`] — the app-level `import_export::Import` handler. It answers the shared
//!   action ("Imports a collection or environment") with one dialog and routes the chosen
//!   file: a collection to [`crate::import::begin_import`] (S45a's second door), a
//!   variable file through here. It registers after `import`'s own handler and global
//!   action handlers run last-registered-first and stop by default, so this handler
//!   answers the action and `import`'s dialog stays registered as the fallback.
//! - [`begin_import_file`] — the import of one chosen file; the dialog answer and the
//!   tests both land here.
//!
//! `Decision:` globals import *merges* by key rather than replacing the globals list:
//! importing adds — an environment import always adds a new environment, never replaces
//! one — and a replacement would destroy the user's globals with nothing to undo
//! (AGENTS.md › destructive actions confirm or are undoable). An imported key takes its
//! row exactly as the file wrote it; every other key stays as it was.
//!
//! `Decision:` an imported `secret` value is declared in the file as a
//! [`VariableKind::Secret`] row with no value: `api_model`'s `split_secrets` returns the
//! values separately and keeps no row for them, and a name no scope declares is never
//! followed to the keychain at send time (`http_engine`'s `ReferencedSecrets`). The row
//! is minted here from the key alone — a secret's `enabled` and `description` do not
//! survive `split_secrets`.
//!
//! `Decision:` the dialog, the size check and the failure messages are this module's own
//! thin shells over the same `api_model`/`workspace_store` machinery `import.rs` drives:
//! `import.rs`'s helpers of that name are private and this step may not edit it (its
//! Edits set), so the two share the *rules* — the same `ImportLimits`, the same
//! `import::too_large_message`, the same dialog and rollback shapes — rather than the
//! code. A dedup can hoist them the next time `import.rs` opens.
//!
//! `Decision:` a globals file with no `_postman_variable_scope` marker imports as an
//! environment: Postman exports both kinds with the marker, and an environment is the
//! safe reading of a file without one (it adds one scope instead of touching globals).
//!
//! `Decision:` the file's own `name` names the environment — and its file, through
//! `workspace_store`'s slug. Like a collection's name it is mapped deliberately, and
//! `save_item` slugs, confines and de-duplicates it (`staging-2`, …) whatever it says;
//! a name too long for a file name fails the save with the reason in the toast.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use api_model::{
    Environment, ImportError, ImportLimits, ImportProblem, ImportReport, SecretValuesToStore,
    Variable, VariableKind, import_environment as parse_environment,
    import_globals as parse_globals,
};
use environment_picker::env_editor::{EditTarget, EnvEditorInputs};
use environment_picker::secret_values::{self, AppliedChange, SecretChange};
use gpui::{AnyWindowHandle, App, Entity, Global, PathPromptOptions};
use pustman_actions::import_export::Import;
use secrets::SecretStore;
use serde_json::Value;
use ui::prelude::*;
use workspace::{Workspace, WorkspaceOpen};
use workspace_store::{Item, ItemBody};

#[cfg(test)]
#[path = "import_environment_tests.rs"]
mod tests;

/// The directory environments live in (REQ-WSP-01) — the one `workspace_store` reads
/// `*.environment.json` out of.
const ENVIRONMENTS_DIR: &str = "environments";

/// The scope id the globals' keychain entries live under — the same one the environment
/// editor keys the globals by (`env_editor`'s `GLOBALS_SCOPE_ID`, private there).
const GLOBALS_SCOPE_ID: &str = "globals";

/// The field `pustman.json` carries the globals' shared values in — the environment
/// editor's decision (`env_editor`'s `GLOBALS_VARIABLES_FIELD`), reused verbatim.
const GLOBALS_VARIABLES_FIELD: &str = "variables";

/// How many of the import's notes one message names before it counts the rest (the notes
/// list is unbounded — an untrusted file decides its length).
const MAX_NOTES_IN_MESSAGE: usize = 3;

/// What to say when the import is asked for with no workspace open.
const NO_WORKSPACE_MESSAGE: &str = "No workspace is open — open one to import a Postman file.";

/// What to say when the open workspace must not be written (REQ-WSP-04) — the same
/// message the tree's writes use.
const READ_ONLY_MESSAGE: &str =
    "This workspace is read-only: it declares a newer format version than this build writes.";

/// What to say when the file's secret values cannot be keyed for this workspace (S43w's
/// fail-closed rule names the case, never anything about the values).
const NO_INSTALLATION_KEY_MESSAGE: &str = "this window's workspace is not the one Pustman holds \
     keychain keys for — try again once it has finished opening";

/// The app-wide wiring: the init guard (one registration per app).
#[derive(Default)]
struct EnvironmentImportWiring {
    started: bool,
}

impl Global for EnvironmentImportWiring {}

/// Wire `import_export::Import` (REQ-IO-02) once per app. Registered app-level
/// (`App::on_action`), so the action is dispatchable — and listed in the command
/// palette — wherever the keyboard is, and registered after `import::init` so this
/// handler answers the shared action (module docs).
pub fn init(cx: &mut App) {
    if cx.default_global::<EnvironmentImportWiring>().started {
        return;
    }
    cx.default_global::<EnvironmentImportWiring>().started = true;
    cx.on_action(import_action);
}

/// `import_export::Import`: ask for the Postman file, then import it. The action
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
                prompt: Some(SharedString::new_static(
                    "Import a Postman collection, environment or globals file",
                )),
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
                cx.update(|cx| begin_import_file(target, source, window_handle, cx));
            })
            .detach();
        });
        if prompted.is_err() {
            // Handled: the window closed before the dialog could open; there is nobody
            // left to ask.
        }
    });
}

/// Import the Postman file at `source` into the workspace at `root` (S45b): a collection
/// is handed to S45a's importer unchanged, an environment or globals file goes through
/// here. The size check, the read, the parse and every write run off the foreground;
/// what the import produced — or why it could not — lands on `window_handle` as one
/// toast.
pub(crate) fn begin_import_file(
    root: PathBuf,
    source: PathBuf,
    window_handle: AnyWindowHandle,
    cx: &mut App,
) {
    let keychain = Keychain::capture(cx);
    cx.spawn(async move |cx| {
        let routed = cx
            .background_executor()
            .spawn({
                let root = root.clone();
                let source = source.clone();
                async move { route_work(&root, &source, &keychain) }
            })
            .await;
        match routed {
            Routed::Collection => {
                cx.update(|cx| crate::import::begin_import(root, source, window_handle, cx));
            }
            Routed::VariableFile(outcome) => {
                cx.update(|cx| land(outcome, window_handle, cx));
            }
        }
    })
    .detach();
}

/// What one chosen file turned out to be.
enum Routed {
    /// A Postman collection: S45a's importer owns it, unchanged.
    Collection,
    /// A variable file: one outcome for the user — what landed, or why nothing did.
    VariableFile(Result<Outcome, String>),
}

/// Which kind of Postman file the chosen file is (module docs).
enum FileKind {
    /// A collection.
    Collection,
    /// An environment or globals export.
    Variable(VariableFile),
}

/// Which variable scope the file becomes.
enum VariableFile {
    /// A new environment.
    Environment,
    /// The workspace's globals.
    Globals,
}

/// The import of one file against one workspace root: untrusted input in, workspace
/// files, keychain entries and one outcome out. Pure work — it runs on a background task
/// and knows nothing about windows.
fn route_work(root: &Path, source: &Path, keychain: &Keychain) -> Routed {
    let limits = ImportLimits::default();
    let bytes = match read_bounded(source, &limits) {
        Ok(bytes) => bytes,
        Err(message) => return Routed::VariableFile(Err(message)),
    };
    let value: Value = match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(error) => {
            return Routed::VariableFile(Err(format!(
                "Could not import {}: the file is not valid JSON — {error}.",
                file_label(source)
            )));
        }
    };
    match file_kind(&value) {
        Some(FileKind::Collection) => Routed::Collection,
        Some(FileKind::Variable(kind)) => {
            Routed::VariableFile(import_variable_file(root, source, kind, &bytes, keychain))
        }
        None => Routed::VariableFile(Err(format!(
            "Could not import {}: the file is not a Postman collection, environment or \
             globals file.",
            file_label(source)
        ))),
    }
}

/// Which kind of Postman file `value` is, from the markers Postman exports carry (module
/// docs). The document was parsed whole, so nothing here trusts a shape it has not seen.
fn file_kind(value: &Value) -> Option<FileKind> {
    let object = value.as_object()?;
    match object
        .get("_postman_variable_scope")
        .and_then(Value::as_str)
    {
        Some("globals") => Some(FileKind::Variable(VariableFile::Globals)),
        Some(_) => Some(FileKind::Variable(VariableFile::Environment)),
        // No marker: a document of `values` and nothing else reads as an environment
        // (Decision:), and a document shaped like a collection goes to S45a's importer.
        None if object.contains_key("values")
            && !object.contains_key("info")
            && !object.contains_key("item") =>
        {
            Some(FileKind::Variable(VariableFile::Environment))
        }
        None if object.contains_key("info") || object.contains_key("item") => {
            Some(FileKind::Collection)
        }
        None => None,
    }
}

/// What one parsed variable file holds before any of it is written.
struct Parsed {
    /// What the message calls the import's destination — `“Staging”`, `globals`.
    label: String,
    /// The scope id its keychain entries live under (module docs).
    scope_id: String,
    /// How many variables the file will declare, secret rows included.
    variable_count: usize,
    /// What lands in the workspace.
    target: Target,
    /// The secret values for the keychain, and the keychain writes they need.
    secrets: SecretValuesToStore,
    /// Everything the importer preserved or could not map (names and keys, never values).
    report: ImportReport,
}

/// What lands in the workspace: one new environment, or the globals' merged rows.
enum Target {
    Environment(Environment),
    Globals(Vec<Variable>),
}

/// Parse one environment or globals file and write it where it belongs, putting its
/// secret values in the keychain on the way (module docs). Any failure rolls back
/// exactly what this import created — the file and the keychain entries — before it is
/// reported.
fn import_variable_file(
    root: &Path,
    source: &Path,
    kind: VariableFile,
    bytes: &[u8],
    keychain: &Keychain,
) -> Result<Outcome, String> {
    let limits = ImportLimits::default();
    let parsed = match kind {
        VariableFile::Environment => {
            let (mut environment, secrets, report) = parse_environment(bytes, &limits)
                .map_err(|error| import_error_message(source, &error, "environment"))?;
            let label = format!("“{}”", environment.name);
            let scope_id = environment.id.to_string();
            environment
                .variables
                .extend(secrets.iter().map(|(key, _)| secret_row(key)));
            let variable_count = environment.variables.len();
            Parsed {
                label,
                scope_id,
                variable_count,
                target: Target::Environment(environment),
                secrets,
                report,
            }
        }
        VariableFile::Globals => {
            let (mut variables, secrets, report) = parse_globals(bytes, &limits)
                .map_err(|error| import_error_message(source, &error, "globals"))?;
            variables.extend(secrets.iter().map(|(key, _)| secret_row(key)));
            let variable_count = variables.len();
            Parsed {
                label: String::from("globals"),
                scope_id: String::from(GLOBALS_SCOPE_ID),
                variable_count,
                target: Target::Globals(variables),
                secrets,
                report,
            }
        }
    };
    // The keychain plan is refused up front (fail closed): nothing is written before the
    // import knows every secret value can be stored.
    let plan = keychain.plan(source, root, &parsed.scope_id, &parsed.secrets)?;
    let written = match &parsed.target {
        Target::Environment(environment) => write_environment(root, environment)?,
        Target::Globals(variables) => write_globals(root, variables)?,
    };
    if let Some(plan) = plan {
        let mut applied = Vec::new();
        for change in &plan.changes {
            match AppliedChange::apply(change, plan.store) {
                Ok(applied_change) => applied.push(applied_change),
                Err(error) => {
                    // A failed import leaves nothing behind: the entries this run
                    // created go away (and the values they replaced come back), and the
                    // file goes with them.
                    let unrestored = secret_values::rollback(plan.store, &applied);
                    let mut message = import_failure(
                        source,
                        &format!("a secret value could not be stored — {error}"),
                    );
                    message.push_str(&undo(root, &written));
                    if !unrestored.is_empty() {
                        message.push_str(&format!(
                            " — and the values already stored under {} could not be restored",
                            unrestored
                                .iter()
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                                .join("; ")
                        ));
                    }
                    return Err(message);
                }
            }
        }
    }
    let (message, severity) = success_message(
        &parsed.label,
        parsed.variable_count,
        parsed.secrets.len(),
        &parsed.report.problems,
    );
    Ok(Outcome { message, severity })
}

/// The file's row for one imported secret value: the key and its kind, never a value
/// (REQ-VAR-SHARE-02) — what makes `{{name}}` resolve through the keychain at send time
/// (module docs).
fn secret_row(key: &str) -> Variable {
    Variable {
        key: key.to_owned(),
        value: None,
        kind: VariableKind::Secret,
        enabled: true,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// Write one new environment — one `environments/<slug>.environment.json` under the id
/// the importer minted — remembering whether its directory is this import's doing, so a
/// failure takes back exactly what it created (the S45a rollback's promise).
fn write_environment(root: &Path, environment: &Environment) -> Result<Written, String> {
    let created_dir = match std::fs::symlink_metadata(root.join(ENVIRONMENTS_DIR)) {
        Ok(_) => false,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => true,
        // Not verifiable: kept as it is, so never claimed as this import's (S45a's rule).
        Err(_) => false,
    };
    let item = Item {
        body: ItemBody::Environment(environment.clone()),
        dir: PathBuf::from(ENVIRONMENTS_DIR),
        path: None,
    };
    match workspace_store::save_item(root, &item) {
        Ok(file) => Ok(Written::Environment { file, created_dir }),
        Err(error) => {
            let mut message = write_failure_message(&environment.name, &error);
            if created_dir
                && let Err(cleanup) = std::fs::remove_dir(root.join(ENVIRONMENTS_DIR))
                && cleanup.kind() != std::io::ErrorKind::NotFound
                && cleanup.kind() != std::io::ErrorKind::DirectoryNotEmpty
            {
                message.push_str(&format!(
                    " — and {ENVIRONMENTS_DIR}/ could not be removed: {cleanup}"
                ));
            }
            Err(message)
        }
    }
}

/// Merge the imported rows into the globals `pustman.json` carries (module docs),
/// remembering the previous state so a failure puts it back untouched.
fn write_globals(root: &Path, variables: &[Variable]) -> Result<Written, String> {
    let snapshot = workspace_store::open(root).map_err(|error| {
        import_failure_sourceless(&format!("the workspace could not be read — {error}"))
    })?;
    let previous = snapshot
        .items
        .values()
        .find(|item| matches!(item.body, ItemBody::Workspace(_)))
        .cloned()
        .ok_or_else(|| {
            import_failure_sourceless(
                "this workspace has no workspace file to write the globals into",
            )
        })?;
    // The rows as the environment editor reads them, so the merge speaks one variable
    // format (`env_editor`'s `EditTarget`).
    let existing = EditTarget::new(previous.clone())
        .map_err(|error| {
            import_failure_sourceless(&format!("the globals could not be read — {error}"))
        })?
        .variables()
        .to_vec();
    let mut merged = existing;
    for variable in variables {
        match merged
            .iter_mut()
            .find(|present| present.key == variable.key)
        {
            Some(present) => *present = variable.clone(),
            None => merged.push(variable.clone()),
        }
    }
    let mut item = previous.clone();
    let ItemBody::Workspace(document) = &mut item.body else {
        return Err(import_failure_sourceless(
            "the workspace file could not be read back",
        ));
    };
    let value = serde_json::to_value(&merged).map_err(|error| {
        import_failure_sourceless(&format!(
            "the merged globals could not be serialized — {error}"
        ))
    })?;
    document
        .extra
        .insert(String::from(GLOBALS_VARIABLES_FIELD), value);
    workspace_store::save_item(root, &item).map_err(|error| {
        import_failure_sourceless(&format!("the globals could not be written — {error}"))
    })?;
    Ok(Written::Globals {
        previous: Box::new(previous),
    })
}

/// What one import wrote, so a failure can take back exactly that (module docs).
enum Written {
    /// The new environment file, and whether its directory is this import's doing.
    Environment { file: PathBuf, created_dir: bool },
    /// The globals' previous state — the workspace item before the merge.
    Globals { previous: Box<Item> },
}

/// Take back what the import wrote (the S45a rollback's promise): the file it created
/// and the directory that file's save created, or the globals' previous contents.
/// Returns what could not be undone, for the message — none of it is swallowed.
fn undo(root: &Path, written: &Written) -> String {
    let mut notes = Vec::new();
    match written {
        Written::Environment { file, created_dir } => {
            match std::fs::remove_file(root.join(file)) {
                Ok(()) => {}
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    notes.push(format!("{} could not be removed: {error}", file.display()))
                }
            }
            if *created_dir {
                match std::fs::remove_dir(root.join(ENVIRONMENTS_DIR)) {
                    Ok(()) => {}
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    // Not empty: something that is not this import's is in there —
                    // nothing this rollback owes.
                    Err(error) if error.kind() == std::io::ErrorKind::DirectoryNotEmpty => {}
                    Err(error) => {
                        notes.push(format!("{ENVIRONMENTS_DIR}/ could not be removed: {error}"));
                    }
                }
            }
        }
        Written::Globals { previous } => {
            if let Err(error) = workspace_store::save_item(root, previous.as_ref()) {
                notes.push(format!(
                    "the previous globals could not be written back: {error}"
                ));
            }
        }
    }
    if notes.is_empty() {
        String::new()
    } else {
        format!(" — and: {}", notes.join("; "))
    }
}

/// The keychain and the installation key this import must write under (module docs): the
/// app-globals the app's wire installs (S43w), read once where the import starts.
struct Keychain {
    /// The OS keychain — `None` when the app's wire never connected one.
    store: Option<Arc<dyn SecretStore>>,
    /// The installation key and the workspace folder it was built for: the root check is
    /// what fails closed before anything is written.
    installed: Option<(String, PathBuf)>,
}

/// The keychain writes one import performs — `None` when the file carries no secrets and
/// no keychain is needed at all.
struct SecretPlan<'a> {
    store: &'a Arc<dyn SecretStore>,
    changes: Vec<SecretChange>,
}

impl Keychain {
    /// What the app's wire installed: the store [`secret_values`] keeps secret values in
    /// and the key [`EnvEditorInputs`] carries — `pustman::scope_wiring::workspace_key`'s
    /// own key (module docs).
    fn capture(cx: &App) -> Self {
        let installed = cx
            .try_global::<EnvEditorInputs>()
            .map(|inputs| (inputs.workspace_id.clone(), inputs.workspace_root.clone()));
        Self {
            store: secret_values::store(cx),
            installed,
        }
    }

    /// The writes these secret values need, under the key the app reads them back with —
    /// or the message that refuses the import before anything is written (fail closed).
    fn plan<'a>(
        &'a self,
        source: &Path,
        root: &Path,
        scope_id: &str,
        secrets: &SecretValuesToStore,
    ) -> Result<Option<SecretPlan<'a>>, String> {
        if secrets.is_empty() {
            return Ok(None);
        }
        let store = self
            .store
            .as_ref()
            .ok_or_else(|| import_failure(source, secret_values::NOT_CONNECTED))?;
        let (workspace_key, installed_root) = self
            .installed
            .as_ref()
            .ok_or_else(|| import_failure(source, NO_INSTALLATION_KEY_MESSAGE))?;
        if !same_folder(root, installed_root) {
            return Err(import_failure(source, NO_INSTALLATION_KEY_MESSAGE));
        }
        let changes = secrets
            .iter()
            .map(|(key, value)| {
                SecretChange::set(workspace_key, scope_id, key, value).map_err(|error| {
                    import_failure(
                        source,
                        &format!("a secret value could not be stored — {error}"),
                    )
                })
            })
            .collect::<Result<Vec<_>, String>>()?;
        Ok(Some(SecretPlan { store, changes }))
    }
}

/// Whether two workspace folder paths name the same folder: canonical when both resolve,
/// as written otherwise.
fn same_folder(left: &Path, right: &Path) -> bool {
    let canonical =
        |path: &Path| std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    canonical(left) == canonical(right)
}

/// What one import has to say (module docs: never a value).
struct Outcome {
    message: String,
    severity: Severity,
}

/// What the user is told when an import landed: the destination, the row counts and how
/// many secret values went to the keychain — plus the import's notes, capped (an
/// untrusted file decides their number). Never a value (module docs).
fn success_message(
    label: &str,
    variables: usize,
    secrets: usize,
    problems: &[ImportProblem],
) -> (String, Severity) {
    let stored = match secrets {
        0 => String::from("no secret values to store"),
        1 => String::from("1 secret value stored in the keychain"),
        count => format!("{count} secret values stored in the keychain"),
    };
    let mut message = format!("Imported {label} — {variables} variables, {stored}.");
    if problems.is_empty() {
        return (message, Severity::Success);
    }
    let named = problems
        .iter()
        .take(MAX_NOTES_IN_MESSAGE)
        .map(|problem| format!("{} — {}", problem.path, problem.detail))
        .collect::<Vec<_>>()
        .join("; ");
    let more = problems.len().saturating_sub(MAX_NOTES_IN_MESSAGE);
    message.push_str(&format!(" — notes: {named}"));
    if more > 0 {
        message.push_str(&format!(" (and {more} more)"));
    }
    (message, Severity::Warning)
}

/// Show what an import produced: the one message the user gets. Deferred — a window
/// cannot be updated while it is on the update stack (the `response_viewer` toast
/// reach-through's shape).
fn land(outcome: Result<Outcome, String>, window_handle: AnyWindowHandle, cx: &mut App) {
    cx.defer(move |cx| {
        let (message, severity) = match outcome {
            Ok(done) => (done.message, done.severity),
            Err(message) => (message, Severity::Error),
        };
        let shown = window_handle.update(cx, |root, _window, cx| {
            let Ok(workspace) = root.downcast::<Workspace>() else {
                return;
            };
            workspace.update(cx, |workspace, cx| {
                workspace.show_toast(message.clone(), severity, None, cx);
            });
        });
        if shown.is_err() {
            // The window is gone — there is no user left to reach, and the message is
            // all that survives of the failure. It names keys and files, never values.
            log::error!("{message}");
        }
    });
}

/// The workspace root this import may write into, or the message that says why it may
/// not — `import::writable_root`'s checks, spoken from here because the import starts
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

/// Why an import cannot start — the message is this flow's to the user, never only a log
/// (AGENTS.md › errors reach the user).
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

/// Read at most [`ImportLimits::max_bytes`] + 1 bytes and refuse what came in over the
/// cap (REQ-IO-06) — the S45a bounded read's shape: bounded rather than sized-then-read,
/// because a FIFO or a device reports any length (or none) and would otherwise stream
/// for ever. The size message is `import`'s own, so both importers say one thing.
fn read_bounded(source: &Path, limits: &ImportLimits) -> Result<Vec<u8>, String> {
    use std::io::Read;
    let file = std::fs::File::open(source).map_err(|error| unreadable_message(source, &error))?;
    let mut bytes = Vec::new();
    file.take(limits.max_bytes as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| unreadable_message(source, &error))?;
    if bytes.len() > limits.max_bytes {
        return Err(crate::import::too_large_message(source));
    }
    Ok(bytes)
}

/// The message a file that cannot be read earns: the file and the OS reason.
fn unreadable_message(source: &Path, error: &std::io::Error) -> String {
    format!(
        "Could not import {}: the file could not be read — {error}.",
        file_label(source)
    )
}

/// The message a file the importer refused earns: the file and what the importer found.
/// `what` names the kind the file claimed, so the message speaks the user's word for it.
fn import_error_message(source: &Path, error: &ImportError, what: &str) -> String {
    let label = file_label(source);
    match error {
        ImportError::TooLarge { .. } => crate::import::too_large_message(source),
        ImportError::TooDeep { limit } => {
            format!("Could not import {label}: the JSON nests deeper than {limit} levels.")
        }
        ImportError::Invalid(reason) => {
            format!("Could not import {label}: the file is not a valid Postman {what} — {reason}.")
        }
        ImportError::MissingField(field) => {
            format!("Could not import {label}: the file is missing its {field} field.")
        }
    }
}

/// The message a failed step of an import earns: the file and the reason.
fn import_failure(source: &Path, reason: &str) -> String {
    format!("Could not import {}: {reason}.", file_label(source))
}

/// [`import_failure`] for a step whose reason stands alone (the globals' writes name
/// their own subject).
fn import_failure_sourceless(reason: &str) -> String {
    format!("Could not import the globals: {reason}.")
}

/// The message a write that could not land earns: the item and the store's reason —
/// `import`'s own wording, so both importers say one thing.
fn write_failure_message(name: &str, error: &workspace_store::SaveError) -> String {
    format!("Could not write “{name}” into the workspace: {error}")
}

/// The file's own name — messages name the file the user picked, never a path from the
/// import.
fn file_label(source: &Path) -> String {
    source.file_name().map_or_else(
        || source.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    )
}
