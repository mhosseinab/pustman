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
//! - A key is declared **once** (fix 1), and what the file *declares* decides: a file
//!   that lists the same key twice — once plain with a value, once `secret`, value or no
//!   value — is merged into one `Secret` row, and no occurrence's plain value reaches a
//!   workspace file. The merge is reported, by key only.
//! - A `secret` value goes to the OS keychain **only** — never into a workspace file, a
//!   toast, a log or the Console (REQ-IO-02, REQ-VAR-SHARE-02). `workspace_store` strips
//!   secret values from written files on its own as well, and every message this flow
//!   builds names keys and counts, never values.
//! - No keychain, or no installation key for this workspace, is fail-closed: the import
//!   is refused before anything is written (S43w's "fail closed"), never completed with
//!   the values kept somewhere else.
//! - The environment's name reaches the toast sanitised and truncated (fix 5) — as
//!   does every variable key a message names (fix 3 of cycle 3): both are file text and
//!   a toast is one line.
//!
//! What it writes:
//! - An environment becomes one new `environments/<slug>.environment.json` under an id
//!   this build minted, each secret declared as a [`VariableKind::Secret`] row with no
//!   value — so `{{name}}` resolves through the keychain at send time (the send pipeline
//!   reads a secret only through a declared secret variable) and the file records the
//!   key only.
//! - Globals land in `pustman.json`'s `variables` field — the globals' home the
//!   environment editor established — merged by key (see `Decision:` below). Every
//!   replaced key is named in the result (capped, fix 3), a row that stops being a
//!   secret takes its keychain entry out with it (fix 3 of cycle 1) so no orphaned
//!   entry can outlive its row, and a save over a `pustman.json` that changed under the
//!   import is refused rather than clobbered (fix 2 of cycle 2).
//!
//! **The order is keychain first, file second** (fix 2): the entries land — each keeping
//! the value it replaces — before the workspace file exists, so the file watcher cannot
//! see the file while the writes are in flight and mark the keys checked-but-unresolved.
//! Every failure after the first write rolls the keychain plan back — exactly the
//! entries this run wrote, previous values restored — and says so (module docs below:
//! nothing is swallowed).
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
//! row exactly as the file wrote it, every other key stays as it was, and every replaced
//! key is named in the result (fix 3).
//!
//! `Decision:` a replaced row that *was* a secret and is not one any more (fix 3) has
//! its keychain entry removed in the same rollback-able plan as the imports' own writes —
//! a kept entry would be an orphan no scope declares. A row that stays a secret and
//! arrives with no value keeps its existing entry: the import sets only what it carries.
//!
//! `Decision:` a globals save made while the keychain writes are in flight (fix 2,
//! cycle 2) refuses the import with a clear message instead of writing over it: the
//! merge is computed from one copy of `pustman.json`, and the keychain plan is rolled
//! back on the refusal, so the import is all-or-nothing and the concurrent edit — the
//! environment editor's save, a `git pull` — survives untouched. Re-merging onto the
//! fresh copy was the alternative, but the plan's removals and the message's claims
//! were computed from the old copy and could not stay truthful.
//!
//! `Decision:` `api_model`'s "a secret with no value is dropped" parse note is replaced
//! (fix 1, cycle 2): the row is no longer dropped — it is declared, unset — so the key
//! gets its own, accurate note ("it lands unset and nothing is stored for it") instead
//! of the parse step's claim.
//!
//! `Decision:` keys are read through `api_model`'s own `string_field` (fix 1, cycle 3)
//! — the one reader `split_secrets` maps keys with — never a look-alike: a number or a
//! bool is a key, and any reader that disagrees about that leaves the plain twin's value
//! in the file or the keychain entry orphaned. Every key `split_secrets` read counts as
//! declared besides, so the two readers cannot drift apart silently.
//!
//! `Decision:` an imported `secret` value is declared in the file as a
//! [`VariableKind::Secret`] row with no value: `api_model`'s `split_secrets` returns the
//! values separately and keeps no row for them, and a name no scope declares is never
//! followed to the keychain at send time (`http_engine`'s `ReferencedSecrets`). The row
//! is minted here — its `enabled` carried from the file's own secret row (fix 5). The
//! workspace format records only `{key, kind, description}` for a secret
//! (`workspace_store`'s REQ-VAR-SHARE-02 shape), so a file that *disables* a secret is
//! reported: the flag is carried as far as the model allows and the report says it did
//! not stick. A secret's `description` does not survive `split_secrets` at all.
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

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use api_model::{
    Environment, ImportError, ImportLimits, ImportProblem, SecretValuesToStore, Variable,
    VariableKind, Workspace as WorkspaceDocument, import_environment as parse_environment,
    import_globals as parse_globals, string_field,
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

/// How many characters of an environment's name a toast shows (fix 5): the name is file
/// text and a toast is one line.
const MAX_TOAST_NAME_CHARS: usize = 80;

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

/// What to say when `pustman.json` changed while the import's keychain writes were in
/// flight (fix 2): the import is refused — and the rollback note that follows alone
/// says what of the keychain writes remains.
const GLOBALS_CHANGED_MESSAGE: &str =
    "the globals changed while this import was running — the import is refused; try again";

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
        Some(FileKind::Variable(kind)) => Routed::VariableFile(import_variable_file(
            root, source, kind, &bytes, &value, keychain,
        )),
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

/// What lands in the workspace: one new environment, or the globals' merged rows.
enum Target {
    Environment(Environment),
    Globals(Box<GlobalsPrepared>),
}

/// Parse one environment or globals file and write it where it belongs, putting its
/// secret values in the keychain first (module docs, fix 2). Any failure after the first
/// keychain write rolls the plan back — exactly the entries this run wrote — before it
/// is reported.
fn import_variable_file(
    root: &Path,
    source: &Path,
    kind: VariableFile,
    bytes: &[u8],
    raw: &Value,
    keychain: &Keychain,
) -> Result<Outcome, String> {
    let limits = ImportLimits::default();
    let (label, scope_id, variables, secrets, mut problems, environment) = match kind {
        VariableFile::Environment => {
            let (environment, secrets, report) = parse_environment(bytes, &limits)
                .map_err(|error| import_error_message(source, &error, "environment"))?;
            let label = format!("“{}”", toast_name(&environment.name));
            let scope_id = environment.id.to_string();
            (
                label,
                scope_id,
                environment.variables.clone(),
                secrets,
                report.problems,
                Some(environment),
            )
        }
        VariableFile::Globals => {
            let (variables, secrets, report) = parse_globals(bytes, &limits)
                .map_err(|error| import_error_message(source, &error, "globals"))?;
            (
                String::from("globals"),
                String::from(GLOBALS_SCOPE_ID),
                variables,
                secrets,
                report.problems,
                None,
            )
        }
    };
    // `split_secrets` reports a valueless secret as "dropped" — at that point it kept no
    // row and no value. The row *is* declared here (fix 1), so that note is replaced by
    // the key's own, accurate one in `dedupe`.
    problems.retain(|problem| {
        !problem
            .detail
            .starts_with("a secret with no value is dropped")
    });
    // One row per key (fix 1), and the values the keychain gets — decided by what the
    // file declares, never by whether a value survived. Every merge is a report line
    // naming the key only.
    let (rows, secret_values) = dedupe(variables, &secrets, raw, &mut problems);
    let variable_count = rows.len();
    let mut writes: Vec<SecretWrite> = secret_values
        .iter()
        .map(|(key, value)| SecretWrite::Set {
            key: key.clone(),
            value: value.clone(),
        })
        .collect();
    let (target, replaced, downgraded) = match environment {
        Some(mut environment) => {
            environment.variables = rows;
            (Target::Environment(environment), Vec::new(), Vec::new())
        }
        None => {
            let prepared = prepare_globals(root, &rows)?;
            writes.extend(
                prepared
                    .downgraded
                    .iter()
                    .map(|key| SecretWrite::Remove { key: key.clone() }),
            );
            let replaced = prepared.replaced.clone();
            let downgraded = prepared.downgraded.clone();
            (Target::Globals(Box::new(prepared)), replaced, downgraded)
        }
    };
    // The keychain lands first (fix 2): each entry keeps the value it replaces, so a
    // failure rolls every write of this run back. Nothing is written to the workspace
    // before every value can be stored.
    let plan = keychain.plan(source, root, &scope_id, &writes)?;
    let mut applied = Vec::new();
    if let Some(plan) = &plan {
        for change in &plan.changes {
            match AppliedChange::apply(change, plan.store) {
                Ok(applied_change) => applied.push(applied_change),
                Err(error) => {
                    let mut message = import_failure(
                        source,
                        &format!("a secret value could not be stored — {error}"),
                    );
                    rollback_with_note(plan.store, &applied, &mut message);
                    return Err(message);
                }
            }
        }
    }
    let written = match &target {
        Target::Environment(environment) => write_environment(root, environment),
        Target::Globals(prepared) => commit_globals(root, prepared),
    };
    if let Err(mut message) = written {
        // The file did not land — take the keychain plan back out and say so (fix 2).
        if let Some(plan) = &plan {
            rollback_with_note(plan.store, &applied, &mut message);
        }
        return Err(message);
    }
    let (message, severity) = success_message(
        &label,
        variable_count,
        secret_values.len(),
        &replaced,
        &downgraded,
        &problems,
    );
    Ok(Outcome { message, severity })
}

/// The rows the file will carry and the secret values the keychain gets, one row per key
/// (fix 1 / `Decision:`): a key any occurrence declares `secret` is one `Secret` row and
/// **no** occurrence's value reaches the file; other duplicated keys keep their first
/// row. Every merge — and every flag the workspace format cannot keep (fix 5) — is one
/// report line per key, naming it only, never a value.
fn dedupe(
    variables: Vec<Variable>,
    secrets: &SecretValuesToStore,
    raw: &Value,
    problems: &mut Vec<ImportProblem>,
) -> (Vec<Variable>, Vec<(String, String)>) {
    // What the file *declares* decides secret-ness (fix 1): `split_secrets` drops a
    // valueless secret from both its outputs, so a key's kind comes from the raw rows'
    // own `type` and never from whether a value survived.
    let mut declared: Vec<String> = Vec::new();
    let mut declared_keys: BTreeSet<String> = BTreeSet::new();
    let mut occurrences: BTreeMap<String, usize> = BTreeMap::new();
    for row in raw
        .get("values")
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .unwrap_or_default()
    {
        // The key is read exactly as `api_model`'s `string_field` reads it — a number or
        // a bool is a key too (fix 1) — so a twin the importer merges is a twin here.
        let Some(key) = row
            .as_object()
            .and_then(|object| string_field(object, "key"))
        else {
            continue;
        };
        *occurrences.entry(key.clone()).or_insert(0) += 1;
        if row.get("type").and_then(Value::as_str) == Some("secret")
            && declared_keys.insert(key.clone())
        {
            declared.push(key);
        }
    }
    // Every key `split_secrets` itself read is declared too (fix 1's belt): its reader
    // and the raw scan must never disagree about what a key is.
    for (key, _) in secrets {
        if declared_keys.insert(key.clone()) {
            declared.push(key.clone());
        }
    }
    // The keychain values: the first non-empty value per key — and which keys carry
    // differing values across their occurrences (fix 4).
    let mut secret_values: Vec<(String, String)> = Vec::new();
    let mut first_value: BTreeMap<String, String> = BTreeMap::new();
    let mut differing: BTreeSet<String> = BTreeSet::new();
    for (key, value) in secrets {
        match first_value.get(key) {
            Some(first) if first != value => {
                differing.insert(key.clone());
            }
            Some(_) => {}
            None => {
                first_value.insert(key.clone(), value.clone());
                secret_values.push((key.clone(), value.clone()));
            }
        }
    }
    let enabled_flags = secret_enabled_flags(raw);
    let mut notes: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
    for (key, count) in &occurrences {
        if *count > 1 {
            let reason = if declared_keys.contains(key) {
                "appears more than once and was merged as one secret with no value written to \
                 the file"
            } else {
                "appears more than once and the rows were merged"
            };
            notes.entry(key.clone()).or_default().push(reason);
        }
    }
    for key in &declared {
        if !first_value.contains_key(key) {
            notes.entry(key.clone()).or_default().push(
                "its secret occurrences carry no value; it lands unset and nothing is stored \
                 for it",
            );
        }
    }
    for key in &differing {
        notes.entry(key.clone()).or_default().push(
            "the duplicate secret occurrences carry different values and the first one was kept",
        );
    }
    for key in &declared {
        if !enabled_flags.get(key).copied().unwrap_or(true) {
            notes.entry(key.clone()).or_default().push(
                "its enabled flag is dropped because the workspace format keeps no enabled flag \
                 for a secret",
            );
        }
    }
    for (key, reasons) in notes {
        problems.push(ImportProblem {
            path: key,
            detail: reasons.join("; "),
        });
    }
    let mut rows: Vec<Variable> = Vec::new();
    let mut row_keys: BTreeSet<String> = BTreeSet::new();
    for variable in variables {
        if declared_keys.contains(&variable.key) {
            // The declared secret wins (fix 1): no plain occurrence's value reaches the
            // file, whatever the twin looked like.
            continue;
        }
        if !row_keys.insert(variable.key.clone()) {
            continue;
        }
        rows.push(variable);
    }
    for key in &declared {
        let enabled = enabled_flags.get(key).copied().unwrap_or(true);
        rows.push(secret_row(key, enabled));
    }
    (rows, secret_values)
}

/// The `enabled` flags the file carried for its `secret` rows (fix 5) — the same mapping
/// `api_model` gives a plain row, carried here because `split_secrets` keeps no row for a
/// secret to take it from. First occurrence wins, as the dedupe's does.
fn secret_enabled_flags(raw: &Value) -> BTreeMap<String, bool> {
    let mut flags = BTreeMap::new();
    let Some(rows) = raw.get("values").and_then(Value::as_array) else {
        return flags;
    };
    for row in rows {
        if row.get("type").and_then(Value::as_str) != Some("secret") {
            continue;
        }
        let Some(key) = row
            .as_object()
            .and_then(|object| string_field(object, "key"))
        else {
            continue;
        };
        let enabled = row.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        flags.entry(key).or_insert(enabled);
    }
    flags
}

/// The file's row for one imported secret value: the key, its kind and its `enabled`
/// flag, never a value (REQ-VAR-SHARE-02) — what makes `{{name}}` resolve through the
/// keychain at send time (module docs).
fn secret_row(key: &str, enabled: bool) -> Variable {
    Variable {
        key: key.to_owned(),
        value: None,
        kind: VariableKind::Secret,
        enabled,
        description: None,
        extra: BTreeMap::new(),
    }
}

/// The globals' new state: the item to write, the document it was merged from — what
/// the save re-checks against (fix 2) — and what the merge did to what was there
/// (fix 3): every replaced key, and the replaced keys that stopped being secrets (their
/// keychain entries leave in the same plan as the import's own writes).
struct GlobalsPrepared {
    item: Item,
    base_document: WorkspaceDocument,
    replaced: Vec<String>,
    downgraded: Vec<String>,
}

/// The workspace document `pustman.json` carries right now.
fn read_workspace_item(root: &Path) -> Result<Item, String> {
    let snapshot = workspace_store::open(root).map_err(|error| {
        import_failure_sourceless(&format!("the workspace could not be read — {error}"))
    })?;
    snapshot
        .items
        .values()
        .find(|item| matches!(item.body, ItemBody::Workspace(_)))
        .cloned()
        .ok_or_else(|| {
            import_failure_sourceless(
                "this workspace has no workspace file to write the globals into",
            )
        })
}

/// The workspace document inside `item` — what a change check compares (fix 2).
fn workspace_document(item: &Item) -> Result<WorkspaceDocument, String> {
    match &item.body {
        ItemBody::Workspace(document) => Ok(document.clone()),
        // Handled: `read_workspace_item` only ever returns the workspace document.
        _ => Err(import_failure_sourceless(
            "the workspace file could not be read back",
        )),
    }
}

/// Merge the imported rows into the globals `pustman.json` carries (module docs): an
/// imported key takes its row exactly as the file wrote it, every other key stays — and
/// every replacement is recorded, by key only.
fn prepare_globals(root: &Path, rows: &[Variable]) -> Result<GlobalsPrepared, String> {
    let mut item = read_workspace_item(root)?;
    let base_document = workspace_document(&item)?;
    // The rows as the environment editor reads them, so the merge speaks one variable
    // format (`env_editor`'s `EditTarget`).
    let existing = EditTarget::new(item.clone())
        .map_err(|error| {
            import_failure_sourceless(&format!("the globals could not be read — {error}"))
        })?
        .variables()
        .to_vec();
    let mut merged = existing;
    let mut replaced = Vec::new();
    let mut downgraded = Vec::new();
    for variable in rows {
        match merged
            .iter_mut()
            .find(|present| present.key == variable.key)
        {
            Some(present) => {
                replaced.push(variable.key.clone());
                if present.kind == VariableKind::Secret && variable.kind != VariableKind::Secret {
                    downgraded.push(variable.key.clone());
                }
                *present = variable.clone();
            }
            None => merged.push(variable.clone()),
        }
    }
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
    Ok(GlobalsPrepared {
        item,
        base_document,
        replaced,
        downgraded,
    })
}

/// Save the merged globals — refusing when `pustman.json` changed since the merge read
/// it (fix 2): the merge is based on one copy of the document, and saving over a newer
/// one would silently take a concurrent edit (the environment editor's save, a `git
/// pull`) with it. The caller rolls the keychain plan back on the refusal, so either way
/// the import is all or nothing. The re-check leaves only the microseconds inside
/// `workspace_store::save_item` itself — its own atomic write — uncovered.
fn commit_globals(root: &Path, prepared: &GlobalsPrepared) -> Result<(), String> {
    let fresh = read_workspace_item(root)?;
    let fresh_document = workspace_document(&fresh)?;
    if fresh_document != prepared.base_document {
        return Err(import_failure_sourceless(GLOBALS_CHANGED_MESSAGE));
    }
    workspace_store::save_item(root, &prepared.item)
        .map(|_| ())
        .map_err(|error| {
            import_failure_sourceless(&format!(
                "they could not be written into the workspace — {error}"
            ))
        })
}

/// Write one new environment — one `environments/<slug>.environment.json` under the id
/// the importer minted. A save that fails takes the directory it created back out when
/// it is empty again (the S45a rollback's promise: a failed import leaves nothing).
fn write_environment(root: &Path, environment: &Environment) -> Result<(), String> {
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
        Ok(_) => Ok(()),
        Err(error) => {
            let mut message = write_failure_message(&toast_name(&environment.name), &error);
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

/// The keychain and the installation key this import must write under (module docs): the
/// app-globals the app's wire installs (S43w), read once where the import starts.
struct Keychain {
    /// The OS keychain — `None` when the app's wire never connected one.
    store: Option<Arc<dyn SecretStore>>,
    /// The installation key and the workspace folder it was built for: the root check is
    /// what fails closed before anything is written.
    installed: Option<(String, PathBuf)>,
}

/// One keychain write the import performs. Carries a value, so it has no `Debug` and no
/// `Display`: nothing that can be formatted may hold a secret (AGENTS.md › Security).
enum SecretWrite {
    /// Store the value under its key.
    Set { key: String, value: String },
    /// Remove the entry under its key — a downgraded secret's orphan (fix 3).
    Remove { key: String },
}

/// The keychain writes one import performs — `None` when there is nothing to write and
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

    /// The changes these writes need, under the key the app reads them back with — or
    /// the message that refuses the import before anything is written (fail closed).
    fn plan<'a>(
        &'a self,
        source: &Path,
        root: &Path,
        scope_id: &str,
        writes: &[SecretWrite],
    ) -> Result<Option<SecretPlan<'a>>, String> {
        if writes.is_empty() {
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
        let changes = writes
            .iter()
            .map(|write| {
                match write {
                    SecretWrite::Set { key, value } => {
                        SecretChange::set(workspace_key, scope_id, key, value)
                    }
                    SecretWrite::Remove { key } => {
                        SecretChange::remove(workspace_key, scope_id, key)
                    }
                }
                .map_err(|error| {
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

/// Roll the plan back and append what happened to `message` (fix 2): everything this run
/// wrote went back out, or the keys whose entries could not be restored are named —
/// keys only, never values (AGENTS.md › Security).
fn rollback_with_note(
    store: &Arc<dyn SecretStore>,
    applied: &[AppliedChange],
    message: &mut String,
) {
    let unrestored = secret_values::rollback(store, applied);
    if unrestored.is_empty() {
        message.push_str(" — and the keychain entries this import wrote were rolled back");
    } else {
        message.push_str(&format!(
            " — and the keychain entries under {} could not be rolled back",
            unrestored
                .iter()
                .map(|key| toast_name(key.variable_key()))
                .collect::<Vec<_>>()
                .join("; ")
        ));
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
/// many secret values went to the keychain — then what the merge replaced (fix 3) and
/// what the import could not map, capped (an untrusted file decides their number). Never
/// a value (module docs).
fn success_message(
    label: &str,
    variables: usize,
    secrets: usize,
    replaced: &[String],
    downgraded: &[String],
    problems: &[ImportProblem],
) -> (String, Severity) {
    let stored = match secrets {
        0 => String::from("no secret values to store"),
        1 => String::from("1 secret value stored in the keychain"),
        count => format!("{count} secret values stored in the keychain"),
    };
    let mut message = format!("Imported {label} — {variables} variables, {stored}.");
    if !replaced.is_empty() {
        message.push_str(&format!(" — replaced: {}", capped_names(replaced)));
    }
    if !downgraded.is_empty() {
        message.push_str(&format!(
            " — no longer a secret: {} (the old keychain entry was removed)",
            capped_names(downgraded)
        ));
    }
    let mut severity = Severity::Success;
    if !downgraded.is_empty() {
        severity = Severity::Warning;
    }
    if !problems.is_empty() {
        let named = problems
            .iter()
            .take(MAX_NOTES_IN_MESSAGE)
            .map(|problem| format!("{} — {}", toast_name(&problem.path), problem.detail))
            .collect::<Vec<_>>()
            .join("; ");
        let more = problems.len().saturating_sub(MAX_NOTES_IN_MESSAGE);
        message.push_str(&format!(" — notes: {named}"));
        if more > 0 {
            message.push_str(&format!(" (and {more} more)"));
        }
        severity = Severity::Warning;
    }
    (message, severity)
}

/// At most [`MAX_NOTES_IN_MESSAGE`] names and then a count of the rest (fix 3): the
/// lists are unbounded — an untrusted file decides their length. Every name is
/// sanitised like [`toast_name`] (fix 3 of cycle 3): it is file text in a message.
fn capped_names(names: &[String]) -> String {
    let named = names
        .iter()
        .take(MAX_NOTES_IN_MESSAGE)
        .map(|name| toast_name(name))
        .collect::<Vec<_>>()
        .join(", ");
    let more = names.len().saturating_sub(MAX_NOTES_IN_MESSAGE);
    if more > 0 {
        format!("{named} (and {more} more)")
    } else {
        named
    }
}

/// The name as the toast shows it (fix 5): control characters become spaces and a long
/// name is truncated — a name is untrusted file text and a message is one line.
fn toast_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|character| {
            if character.is_control() {
                ' '
            } else {
                character
            }
        })
        .collect();
    if cleaned.chars().count() > MAX_TOAST_NAME_CHARS {
        let mut truncated: String = cleaned.chars().take(MAX_TOAST_NAME_CHARS).collect();
        truncated.push('…');
        return truncated;
    }
    cleaned
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

/// [`import_failure`] for a step whose subject is the globals (their writes name their
/// own object).
fn import_failure_sourceless(reason: &str) -> String {
    format!("Could not import the globals: {reason}.")
}

/// The message a write that could not land earns: the item and the store's reason —
/// `import`'s own wording, so both importers say one thing. The name is already
/// sanitised for the toast (fix 5).
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
