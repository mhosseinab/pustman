//! `pustman` — the app itself: [`init`], the init order every UI crate plugs into, and the
//! two globals the shell wires once at startup ([`AppStore`] and [`StartupProblems`]).
//! `main.rs` is only the window opener (docs/pustman-v1-implementation-steps.md, S22e,
//! convention 4): a later wire step appends one line per crate to [`init_with_store`].

pub mod app_icon;
pub mod restore_wiring;

use std::sync::{Arc, Mutex};

use gpui::{App, AppContext, Entity, Global, WeakEntity};
use storage::LocalStore;
use ui::Severity;
use workspace::Workspace;

/// Where the app's local store lives for one run.
pub enum StoreLocation {
    /// `settings::paths::config_dir()/pustman.db` — where the binary keeps it (PRD §8.5).
    ConfigDir,
    /// A private in-memory database: what tests ask for, so they never touch the user's
    /// real config dir.
    InMemory,
    /// A store the caller opened and migrated itself: the test entry (S23w), so two
    /// simulated launches can share one database and prove that a relaunch restores.
    Shared(Arc<Mutex<LocalStore>>),
}

/// The app's local store, installed when its background open finishes and reachable from
/// the shell from then on. `storage` has no global of its own and the app holds the one
/// `Arc<Mutex<LocalStore>>` everything shares — the shape `http_engine`'s cookie
/// persistence already takes.
pub struct AppStore(pub Arc<Mutex<LocalStore>>);

impl Global for AppStore {}

/// The problems the init order ran into, and the workspace that shows them once the window
/// is open: AGENTS.md — a failure the user must know about is never only a log. Problems
/// arriving before the window (theme, keymap) and after it (the store's background open)
/// both land here and become error toasts.
#[derive(Default)]
pub struct StartupProblems {
    workspace: Option<WeakEntity<Workspace>>,
    pending: Vec<String>,
}

impl Global for StartupProblems {}

impl StartupProblems {
    /// Remember `problem` and show it as an error toast as soon as there is a workspace.
    pub fn push(cx: &mut App, problem: impl Into<String>) {
        cx.default_global::<Self>().pending.push(problem.into());
        Self::flush(cx);
    }

    /// Point the shell at the window whose workspace shows the problems, and show the ones
    /// that arrived while that window was opening.
    pub fn attach_workspace(workspace: &Entity<Workspace>, cx: &mut App) {
        cx.default_global::<Self>().workspace = Some(workspace.downgrade());
        Self::flush(cx);
    }

    /// Toast every pending problem on the workspace, once there is one.
    fn flush(cx: &mut App) {
        let (workspace, problems) = {
            let state = cx.default_global::<Self>();
            let Some(workspace) = state.workspace.clone() else {
                // No window yet: the problems wait for `attach_workspace`.
                return;
            };
            (workspace, std::mem::take(&mut state.pending))
        };
        for problem in problems {
            let shown = workspace.update(cx, |workspace, cx| {
                workspace.show_toast(problem, Severity::Error, None, cx);
            });
            if shown.is_err() {
                // The window closed mid-flush, so the rest has nowhere to show.
                return;
            }
        }
    }
}

/// The app's init order (AGENTS.md): `settings → theme → ui → text_input → pustman_actions keymap →
/// storage → workspace → command_palette → restore wiring`, one line per crate or wire step
/// so later ones append their own. This is the binary's entry point: its local store is the
/// one in the platform config dir.
pub fn init(cx: &mut App) {
    init_with_store(cx, StoreLocation::ConfigDir);
}

/// [`init`] with the store's location chosen: [`StoreLocation::InMemory`] is the test
/// entry, so no test writes to the user's real config dir.
pub fn init_with_store(cx: &mut App, store: StoreLocation) {
    init_settings(cx);
    init_theme(cx);
    ui::init(cx);
    text_input::init(cx);
    init_keymap(cx);
    open_local_store(cx, store);
    workspace::init(cx);
    command_palette::init(cx);
    restore_wiring::init(cx);
}

/// `settings`: the bundled defaults, then the user's `settings.json` watched by a
/// background task (`SettingsStore::watch` does the reading, so nothing blocks the
/// foreground — REQ-NFR-02). A merge that cannot apply keeps the last good settings and
/// says so here.
fn init_settings(cx: &mut App) {
    if let Err(error) = settings::SettingsStore::init(cx, None) {
        StartupProblems::push(cx, format!("settings could not be applied: {error}"));
    }
    settings::SettingsStore::watch(cx);
}

/// `theme`: the bundled light and dark themes. A parse failure is reported to the user,
/// even though nothing can draw without colors: the report is all this step can do.
fn init_theme(cx: &mut App) {
    if let Err(error) = theme::init(cx) {
        StartupProblems::push(
            cx,
            format!("the bundled themes could not be loaded: {error}"),
        );
    }
}

/// `pustman_actions`: the default keymap. Entries that do not bind are reported rather
/// than dropped in silence.
fn init_keymap(cx: &mut App) {
    let problems = pustman_actions::load_default_keymap(cx);
    if !problems.is_empty() {
        StartupProblems::push(
            cx,
            format!("the default keymap has problems: {}", problems.join("; ")),
        );
    }
}

/// `storage`: the local store opened and migrated off the foreground thread (REQ-NFR-02),
/// installed as [`AppStore`] when it lands. A store that will not open falls back to an
/// in-memory one so the app still runs — with an error toast, because nothing is saved
/// then.
fn open_local_store(cx: &mut App, location: StoreLocation) {
    cx.spawn(async move |cx| {
        let startup = cx
            .background_spawn(async move { open_store_off_foreground(location) })
            .await;
        cx.update(|cx| match startup {
            StoreStartup::Opened(store) => cx.set_global(AppStore(store)),
            StoreStartup::Fallback { store, reason } => {
                cx.set_global(AppStore(store));
                StartupProblems::push(
                    cx,
                    format!(
                        "the local store could not be opened ({reason}); this session runs on an \
                         in-memory store, so nothing is saved"
                    ),
                );
            }
            StoreStartup::Unavailable { reason } => {
                StartupProblems::push(
                    cx,
                    format!("no local store could be opened ({reason}); nothing is saved"),
                );
            }
        });
    })
    .detach();
}

/// What opening the store on the background executor produced.
enum StoreStartup {
    /// The store the caller asked for.
    Opened(Arc<Mutex<LocalStore>>),
    /// The requested store would not open or migrate; this in-memory store runs instead.
    Fallback {
        store: Arc<Mutex<LocalStore>>,
        reason: String,
    },
    /// Not even an in-memory store could be opened, so the app runs without one.
    Unavailable { reason: String },
}

/// Open and migrate the store for `location`, falling back to an in-memory store when it
/// fails. Runs on the background executor: `storage` is synchronous by design (ADR-0002)
/// and no blocking IO belongs on the foreground thread (AGENTS.md).
fn open_store_off_foreground(location: StoreLocation) -> StoreStartup {
    match open_and_migrate(location) {
        Ok(store) => StoreStartup::Opened(store),
        Err(error) => {
            let reason = error.to_string();
            match open_and_migrate(StoreLocation::InMemory) {
                Ok(store) => StoreStartup::Fallback { store, reason },
                Err(fallback) => StoreStartup::Unavailable {
                    reason: format!("{reason}; the in-memory fallback failed too: {fallback}"),
                },
            }
        }
    }
}

/// Open one store and bring its schema up to date. The database lives at
/// `settings::paths::config_dir()/pustman.db`; its directory is created here, on whatever
/// thread called.
fn open_and_migrate(location: StoreLocation) -> anyhow::Result<Arc<Mutex<LocalStore>>> {
    let mut store = match location {
        StoreLocation::ConfigDir => {
            let path = settings::paths::config_dir().join("pustman.db");
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            LocalStore::open(path)?
        }
        StoreLocation::InMemory => LocalStore::open_in_memory()?,
        // Already open and migrated by the caller that injected it.
        StoreLocation::Shared(shared) => return Ok(shared),
    };
    store.migrate()?;
    Ok(Arc::new(Mutex::new(store)))
}
