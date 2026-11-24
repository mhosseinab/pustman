//! `pustman` — the app itself: [`init`], the init order every UI crate plugs into, and the
//! two globals the shell wires once at startup ([`AppStore`] and [`StartupProblems`]).
//! `main.rs` is only the window opener (docs/pustman-v1-implementation-steps.md, S22e,
//! convention 4): a later wire step appends one line per crate to [`init_with_store`].

pub mod app_icon;
pub mod restore_wiring;
pub mod status_items;

#[cfg(test)]
#[path = "pustman_tests.rs"]
mod tests;

use std::sync::{Arc, Mutex};

use environment_picker::{quick_look, selector};
use gpui::{App, AppContext, Entity, EntityId, Global, Subscription, Task, WeakEntity};
use history_panel::HistoryList;
use secrets::{KeyringStore, SecretStore};
use settings::Setting;
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

/// The app's one cookie jar (REQ-HTTP-08): the [`Arc`] every send captures `Set-Cookie`
/// into and sends `Cookie` back from (S33p). [`init_http_client`] builds it with the
/// transport, [`install_cookie_wiring`] reloads and persists it once the local store
/// exists (REQ-WSP-06), and the Manage Cookies dialog lists it (issue #286) — all through
/// this one handle, because replacing the jar would leave the transport sending from the
/// old one. Cookie values are secrets (PRD §7): nothing here prints them.
pub struct AppCookieJar(pub Arc<Mutex<http_engine::CookieJar>>);

impl Global for AppCookieJar {}

/// The problems the app runs into — at startup or later — and the workspace that shows
/// them once the window is open: AGENTS.md — a failure the user must know about is never
/// only a log. Problems arriving before the window (theme, keymap) and after it (the
/// store's background open, a failed history prune) both land here and become error
/// toasts.
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
/// storage → workspace → console → command_palette → http client → response viewer → request editor →
/// environment picker → history panel → status bar readouts → restore wiring → collections panel → git branch item`, one line per crate or wire
/// step so later ones append their own.
/// This is the binary's entry point: its local store is the one in the platform config dir.
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
    console::init(cx);
    command_palette::init(cx);
    init_http_client(cx);
    response_viewer::init(cx);
    request_editor::init(cx);
    environment_picker::init(cx);
    history_panel::init(cx);
    init_history_panel_wiring(cx);
    status_items::init(cx);
    restore_wiring::init(cx);
    // `collections_panel` (S39w): every workspace window gets the Collections tree in its
    // left dock (REQ-UI-01) — `collections_panel::init` owns the wiring, the same shape
    // `console::init` registers the bottom-dock panel with.
    collections_panel::init(cx);
    // `git` (S44w): the status bar's branch item in every workspace window (REQ-COLLAB-03).
    git::init(cx);
}

/// `http_engine`: the client every request tab sends through, built on `ReqwestTransport`
/// over the tokio runtime `gpui_tokio` owns (S26w). Installing it is this app-level
/// wiring decision (`send`'s `Decision:`): `request_editor::init` has no transport to
/// build a client from, and tests install one backed by `FakeTransport` instead. Nothing
/// here can fail — `gpui_tokio::init` reports its own runtime build failure — so there is
/// no problem to push to [`StartupProblems`].
///
/// `console` (S32w): `console::watch` records every cancellation this client reports
/// (REQ-HTTP-12). Its [`Subscription`] lives in [`ConsoleWiring`] for the app's lifetime
/// — dropping a subscription stops the recording.
///
/// `cookies` (S33p): the jar [`AppCookieJar`] holds is built here, with the transport, and
/// handed to [`http_engine::ReqwestTransport::with_cookie_jar`] so every send applies it
/// and captures against it (REQ-HTTP-08). [`init_cookie_saving`] persists it after each
/// completed send and [`install_cookie_wiring`] reloads it when the store lands.
///
/// `history` (S33w): every send this client makes is recorded into the local store
/// (PRD §8.5, REQ-HIST-01). The store lands on a background task, so the recorder is
/// installed by [`init_history_recording`] when [`AppStore`] lands — exactly once per
/// client.
fn init_http_client(cx: &mut App) {
    gpui_tokio::init(cx);
    let jar = Arc::new(Mutex::new(http_engine::CookieJar::new()));
    cx.set_global(AppCookieJar(Arc::clone(&jar)));
    let transport =
        http_engine::ReqwestTransport::new(gpui_tokio::Tokio::handle(cx)).with_cookie_jar(jar);
    let client = cx.new(|_| http_engine::HttpClient::new(Arc::new(transport)));
    let subscription = console::watch(&client, cx);
    cx.default_global::<ConsoleWiring>()
        ._subscriptions
        .push(subscription);
    request_editor::send::set_http_client(client, cx);
    init_cookie_saving(cx);
    init_history_recording(cx);
}

/// The `console` wire (S32w): the [`Subscription`]s that keep the app's Console feed
/// alive — today the one `console::watch` returns for the app's one HTTP client. A
/// dropped subscription stops the recording, so they live here for the app's lifetime,
/// the same shape [`HistoryPanelWiring`] keeps its observers in.
#[derive(Default)]
struct ConsoleWiring {
    _subscriptions: Vec<Subscription>,
}

impl Global for ConsoleWiring {}

/// The id of the one workspace v1 has. Decision: v1 has a single workspace, so a stable
/// constant is its id; a multi-workspace build would derive this per window instead. It
/// is both `restore_wiring`'s save key (the tabs and layout one window restores) and the
/// scope the picker's per-workspace selection and the quick-look's secret keys live
/// under, so those can never diverge — one value, written once (issue #150).
pub const WORKSPACE_ID: &str = "default";

/// `environment_picker` (S34w): the inputs its openers read when `environment::SelectEnvironment`
/// and `environment::QuickLook` fire. Installed where [`AppStore`] lands rather than in
/// [`init_with_store`], because the store opens on a background task and is not there during
/// init (the same ordering `restore_wiring` watches `AppStore` for); a run with no store at
/// all (`StoreStartup::Unavailable`) installs nothing and keeps the openers' own "not wired"
/// toast, which is the report the user gets either way. No workspace folder is open yet
/// (S39+), so the environment list is empty and no environment is active — nothing is
/// fabricated — and the quick-look lists no variables and uses the OS keychain for the
/// secret rows it will have once a workspace supplies them.
fn install_environment_picker_inputs(cx: &mut App, store: &Arc<Mutex<LocalStore>>) {
    selector::install_inputs(
        cx,
        selector::SelectorInputs {
            workspace_id: WORKSPACE_ID.to_owned(),
            environments: Vec::new(),
            active: None,
            store: Arc::clone(store),
        },
    );
    let secret_store: Arc<dyn SecretStore> = Arc::new(KeyringStore::new());
    quick_look::install_inputs(
        cx,
        quick_look::QuickLookInputs {
            workspace_id: WORKSPACE_ID.to_owned(),
            environment: None,
            globals: Vec::new(),
            local_values: Vec::new(),
            secret_store,
        },
    );
}

/// The app-wide cookie wire (S33p): whether the jar [`init_http_client`] built has finished
/// its first load from `storage`, and the [`Subscription`] that persists it after every
/// completed send — one wiring for the app's lifetime, reset per jar.
#[derive(Default)]
struct CookieWiring {
    /// The initial `load_jar` has landed (or failed), so saves may run. Until then a save
    /// would write the still-empty jar over rows that were never read.
    load_finished: bool,
    /// The save observer is installed — once per app, however often a relaunch rebuilds the
    /// jar ([`init_http_client`] runs again and resets [`Self::load_finished`]).
    started: bool,
    /// The latest save, which the next one awaits before it snapshots the jar: saves land
    /// one after another, in the order the jar changed. Unordered writes could let an
    /// older snapshot — still holding a cookie a server has since cleared — land last and
    /// bring that cookie back on the next launch.
    save_chain: Option<Task<()>>,
    /// A save is waiting on the chain and has not snapshotted yet. It will write the jar as
    /// it is when it runs, so further notifications until then add nothing.
    save_queued: bool,
    _subscriptions: Vec<Subscription>,
}

impl Global for CookieWiring {}

/// `cookies` (S33p): the save side of the wire — one observer of the app's per-send Console
/// notification, for the app's lifetime. `Decision:` that notification is the app's
/// completed-send hook: `HttpClient`'s [`http_engine::SendEvent`] reports cancellations
/// only and the history recorder's row writes are internal to `http_engine`, while every
/// completed send — a response or a failure — lands one Console entry
/// (`request_editor::send`'s `finish_send`), and the History panel already live-reloads on
/// this same notification. The observer is installed here and armed only once the jar's
/// first load has landed ([`CookieWiring::load_finished`]); a send completed before that is
/// saved by the next one instead of over the not-yet-read rows.
fn init_cookie_saving(cx: &mut App) {
    let wiring = cx.default_global::<CookieWiring>();
    wiring.load_finished = false;
    if wiring.started {
        return;
    }
    wiring.started = true;
    let subscription = cx.observe_global::<console::ConsoleStore>(save_cookie_jar);
    cx.default_global::<CookieWiring>()
        ._subscriptions
        .push(subscription);
}

/// Persist the shared jar after a completed send (REQ-WSP-06): `save_jar_latest` snapshots
/// it inside its background task under the store's lock and writes there (ADR-0002), so
/// every writer of the jar is ordered (#287). A failed
/// write is a [`StartupProblems`] toast and a logged error: a lost save is never silent
/// (AGENTS.md — errors reach the user).
fn save_cookie_jar(cx: &mut App) {
    let Some(wiring) = cx.try_global::<CookieWiring>() else {
        return;
    };
    if !wiring.load_finished || wiring.save_queued {
        return;
    }
    let wiring = cx.global_mut::<CookieWiring>();
    wiring.save_queued = true;
    let previous = wiring.save_chain.take();
    let save = cx.spawn(async move |cx| {
        if let Some(previous) = previous {
            previous.await;
        }
        // Snapshot now, after every earlier save has landed, so this write carries the
        // jar's latest state and nothing older can land after it.
        let write = cx.update(|cx| {
            let wiring = cx.global_mut::<CookieWiring>();
            wiring.save_queued = false;
            // A relaunch between queueing and now resets the flag: the jar is new and not
            // loaded yet, and writing it would erase the stored rows.
            if !wiring.load_finished {
                return None;
            }
            let store = cx
                .try_global::<AppStore>()
                .map(|store| Arc::clone(&store.0))?;
            let jar = cx
                .try_global::<AppCookieJar>()
                .map(|jar| Arc::clone(&jar.0))?;
            // `save_jar_latest` snapshots inside its task under the store's lock, so this
            // write is also ordered with the Manage Cookies dialog's saves (#287).
            Some(http_engine::save_jar_latest(
                cx.background_executor(),
                store,
                WORKSPACE_ID.to_owned(),
                jar,
            ))
        });
        let Some(write) = write else {
            return;
        };
        if let Err(error) = write.await {
            // The text is `cookie_persistence`'s own fixed wording; a cookie value never
            // reaches it (PRD §7). A lost save is never silent (AGENTS.md).
            cx.update(|cx| {
                StartupProblems::push(cx, format!("your cookies could not be saved ({error})"));
            });
        }
    });
    cx.global_mut::<CookieWiring>().save_chain = Some(save);
}

/// The load side of the cookie wire (S33p): what the store-open path runs once the local
/// store exists — the jar's reload from `storage` (REQ-WSP-06) into the very [`Arc`] the
/// transport sends and captures through ([`AppCookieJar`]), then the Manage Cookies
/// dialog's inputs (issue #286).
fn install_cookie_wiring(cx: &mut App, store: &Arc<Mutex<LocalStore>>) {
    let Some(jar) = cx
        .try_global::<AppCookieJar>()
        .map(|jar| Arc::clone(&jar.0))
    else {
        // `init_http_client` installs the jar before any store can land (the
        // `install_history_recording` shape); without one there is nothing to load into.
        return;
    };
    let store = Arc::clone(store);
    let loaded = http_engine::load_jar(
        cx.background_executor(),
        Arc::clone(&store),
        WORKSPACE_ID.to_owned(),
    );
    cx.spawn(async move |cx| {
        match loaded.await {
            Ok(loaded_jar) => {
                cx.update(|_cx| {
                    // The same `Arc` the transport holds, so the contents change under it
                    // and no second jar can exist. `Decision:` loaded rows win over
                    // whatever a send captured before the load landed: the jar has no
                    // merge API, and `CookieJar::add` would narrow a `Domain=` cookie to
                    // a host-only one. The store opens at startup, before any request tab
                    // can send.
                    let mut jar_guard = jar.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
                    *jar_guard = loaded_jar;
                });
            }
            Err(error) => {
                // The text is `cookie_persistence`'s own fixed wording (PRD §7), surfaced
                // the way the store-open failure is — never only logged. Handled here so
                // the dialog and later saves still work on the jar there is.
                cx.update(|cx| {
                    StartupProblems::push(
                        cx,
                        format!("your saved cookies could not be read back ({error})"),
                    );
                });
            }
        }
        cx.update(|cx| {
            // A relaunch since this load started built a newer jar (#287): this load's jar
            // is no longer the app's, so it must neither enable saving for the newer, not
            // yet loaded jar nor point the dialog at a jar the transport no longer uses.
            let still_current = cx
                .try_global::<AppCookieJar>()
                .is_some_and(|current| Arc::ptr_eq(&current.0, &jar));
            if !still_current {
                return;
            }
            // Installed once the load has landed (the brief's order), so the dialog lists
            // the loaded jar — and installed on a failed load too: the store is there to
            // save through and the jar is the app's own either way.
            response_viewer::cookie_dialog::install_inputs(
                cx,
                response_viewer::cookie_dialog::CookieDialogInputs {
                    workspace_id: WORKSPACE_ID.to_owned(),
                    store,
                    jar,
                },
            );
            cx.default_global::<CookieWiring>().load_finished = true;
        });
    })
    .detach();
}

/// The app-wide history-recording wire (S33w): the [`http_engine::HistoryRecorder`] the
/// app's one HTTP client records every send through (REQ-HIST-01), installed once per
/// client once the local store it writes through exists. The observer lives here for the
/// app's lifetime — dropping a subscription stops the watching.
#[derive(Default)]
struct HistoryRecordingWiring {
    started: bool,
    /// The recorders installed so far and the client each went to: `set_history` runs
    /// exactly once per client however often [`AppStore`] is set, and a
    /// `history_retention_days` change updates every recorder here (issue #196).
    installed: Vec<InstalledRecorder>,
    _subscriptions: Vec<Subscription>,
}

/// One client's installed [`http_engine::HistoryRecorder`] (issue #196): the client id
/// keeps one recorder per client, and the recorder is what a settings change updates.
struct InstalledRecorder {
    client: EntityId,
    recorder: http_engine::HistoryRecorder,
}

impl Global for HistoryRecordingWiring {}

/// `history` (S33w): the app's one HTTP client records every send into the local store
/// (PRD §8.5, REQ-HIST-01). The store opens on a background task, so
/// [`install_history_recording`] runs when [`AppStore`] lands — and once here too, for
/// the store that landed before this wire registered: watched from both sides, neither
/// order loses a recording (the [`HistoryPanelWiring`] shape below).
fn init_history_recording(cx: &mut App) {
    if cx.default_global::<HistoryRecordingWiring>().started {
        return;
    }
    cx.default_global::<HistoryRecordingWiring>().started = true;
    // The retention window is the typed `history_retention_days` setting (REQ-HIST-04):
    // the recorder takes its value when it is built below and follows later changes
    // through [`update_history_retention`] (issue #196).
    settings::Settings::register(cx);
    let subscriptions = [
        cx.observe_global::<AppStore>(install_history_recording),
        cx.observe_global::<settings::SettingsStore>(update_history_retention),
    ];
    cx.default_global::<HistoryRecordingWiring>()
        ._subscriptions
        .extend(subscriptions);
    install_history_recording(cx);
}

/// Give the installed HTTP client its [`http_engine::HistoryRecorder`], exactly once per
/// client. A run with no store at all (`StoreStartup::Unavailable`) never gets here:
/// nothing is recorded and no new error is shown — the store's toast has already told
/// the user. The workspace id is the one the History panel reads.
fn install_history_recording(cx: &mut App) {
    let Some(store) = cx
        .try_global::<AppStore>()
        .map(|store| Arc::clone(&store.0))
    else {
        // No store yet to record into — the observer runs again when one lands.
        return;
    };
    let Some(client) = cx
        .try_global::<request_editor::send::RequestClient>()
        .map(|client| client.0.clone())
    else {
        // `init_http_client` installs the client before this wire runs; without one
        // there is nothing to record through.
        return;
    };
    if cx
        .default_global::<HistoryRecordingWiring>()
        .installed
        .iter()
        .any(|installed| installed.client == client.entity_id())
    {
        return;
    }
    let retention_days = settings::Settings::get_global(cx).history_retention_days;
    // The retention prune runs on the writer's background task (ADR-0002), where no
    // toast can be shown: its one fixed message crosses to the foreground on this
    // channel and becomes an error toast (AGENTS.md — errors reach the user).
    let (prune_failures, prune_failure_reports) = smol::channel::unbounded::<&'static str>();
    let recorder = http_engine::HistoryRecorder::new(store, WORKSPACE_ID)
        .with_retention_days(retention_days)
        .with_prune_failure_report(move |message| {
            if prune_failures.try_send(message).is_err() {
                // Handled: the app is shutting down and the reader below is gone — the
                // toast would have nowhere to show.
            }
        });
    client.update(cx, |client, _| client.set_history(recorder.clone()));
    // The reader, for the app's lifetime (detached like [`ConsoleWiring`]'s feed): the
    // app keeps exactly one per recorder, and a dropped task stops nothing while any
    // sender is alive.
    cx.spawn(async move |cx| {
        while let Ok(message) = prune_failure_reports.recv().await {
            cx.update(|cx| StartupProblems::push(cx, message));
        }
    })
    .detach();
    cx.default_global::<HistoryRecordingWiring>()
        .installed
        .push(InstalledRecorder {
            client: client.entity_id(),
            recorder,
        });
}

/// `history_retention_days` (REQ-HIST-04) follows its setting while the app runs (issue
/// #196): a change updates every installed recorder's retention window at once — the
/// shape `console::init` uses for `console_max_entries` — so the value is not frozen at
/// the moment the recorder was built.
fn update_history_retention(cx: &mut App) {
    let retention_days = settings::Settings::get_global(cx).history_retention_days;
    for installed in &cx.default_global::<HistoryRecordingWiring>().installed {
        installed.recorder.set_retention_days(retention_days);
    }
}

/// The app-wide history-panel wiring (S35w): the windows waiting for the local store the
/// panel reads through, and the windows that already got their panel.
#[derive(Default)]
struct HistoryPanelWiring {
    started: bool,
    pending: Vec<WeakEntity<Workspace>>,
    registered: Vec<EntityId>,
    _subscriptions: Vec<Subscription>,
}

impl Global for HistoryPanelWiring {}

/// `history_panel` (S35w): every [`Workspace`] window gets the History panel in its left
/// dock once the local store it reads through exists. `history_panel::init` only registers
/// the `history::Focus` action; building the panel needs the store, which lands on a
/// background task before or after the window — the ordering `restore_wiring` solves, in
/// the same shape: both watchers, one registration per workspace, deferred while the
/// window is on the update stack. `Decision:` `restore_wiring`'s pending list is
/// module-private and this step may not edit that file, so this wire keeps its own small
/// pending list rather than extending its global.
fn init_history_panel_wiring(cx: &mut App) {
    if cx.default_global::<HistoryPanelWiring>().started {
        return;
    }
    cx.default_global::<HistoryPanelWiring>().started = true;
    let subscriptions = vec![
        // "Both exist" is watched from both sides, so neither order loses a window: this
        // observer fires when the store lands after the window, and the deferred store
        // check covers the store that landed first.
        cx.observe_global::<AppStore>(register_pending_history_panels),
        cx.observe_new::<Workspace>(|_workspace, _window, cx| {
            let workspace = cx.entity();
            cx.default_global::<HistoryPanelWiring>()
                .pending
                .push(workspace.downgrade());
            // Deferred: the window is on the update stack while its workspace is being
            // created, and registration needs it free to reach into the window.
            cx.defer(register_pending_history_panels);
        }),
    ];
    cx.default_global::<HistoryPanelWiring>()
        ._subscriptions
        .extend(subscriptions);
}

/// Give every workspace waiting for a store its History panel (REQ-HIST-01), at most one
/// per workspace whichever watcher gets there first. A run with no store at all
/// (`StoreStartup::Unavailable`) never gets here: no panel, and the user already has the
/// store's toast.
fn register_pending_history_panels(cx: &mut App) {
    let Some(store) = cx
        .try_global::<AppStore>()
        .map(|store| Arc::clone(&store.0))
    else {
        // No store yet to read the history through — keep the pending windows waiting.
        return;
    };
    let workspaces = std::mem::take(&mut cx.default_global::<HistoryPanelWiring>().pending);
    for workspace in workspaces {
        let Some(workspace) = workspace.upgrade() else {
            // Gone with its window before the store landed.
            continue;
        };
        let workspace_id = workspace.entity_id();
        if cx
            .default_global::<HistoryPanelWiring>()
            .registered
            .contains(&workspace_id)
        {
            continue;
        }
        let registered = cx.with_window(workspace_id, |window, cx| {
            let panel = cx.new(|cx| HistoryList::new(Arc::clone(&store), WORKSPACE_ID, window, cx));
            workspace.update(cx, |workspace, cx| {
                workspace.register_panel(panel, window, cx);
            });
        });
        if registered.is_none() {
            // The window closed before there was a store to register against.
            continue;
        }
        // The registration lives as long as its workspace: when the workspace is
        // released with its window, its id goes with it (the `restore_wiring` shape —
        // issue #163).
        let release = cx.observe_release(&workspace, move |_workspace, cx| {
            cx.default_global::<HistoryPanelWiring>()
                .registered
                .retain(|registered| *registered != workspace_id);
        });
        cx.default_global::<HistoryPanelWiring>()
            ._subscriptions
            .push(release);
        cx.default_global::<HistoryPanelWiring>()
            .registered
            .push(workspace_id);
    }
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
/// installed as [`AppStore`] when it lands, with [`install_environment_picker_inputs`],
/// [`install_cookie_wiring`] and `workspace::WorkspaceOpen::install_local_store` (the
/// recent list and the launch reopen, REQ-WSP-05) right behind it. A store that will not
/// open falls back to an in-memory one so the app still runs — with an error toast,
/// because nothing is saved then.
fn open_local_store(cx: &mut App, location: StoreLocation) {
    cx.spawn(async move |cx| {
        let startup = cx
            .background_spawn(async move { open_store_off_foreground(location) })
            .await;
        cx.update(|cx| match startup {
            StoreStartup::Opened(store) => {
                cx.set_global(AppStore(Arc::clone(&store)));
                install_environment_picker_inputs(cx, &store);
                install_cookie_wiring(cx, &store);
                workspace::WorkspaceOpen::install_local_store(Arc::clone(&store), cx);
            }
            StoreStartup::Fallback { store, reason } => {
                cx.set_global(AppStore(Arc::clone(&store)));
                install_environment_picker_inputs(cx, &store);
                install_cookie_wiring(cx, &store);
                workspace::WorkspaceOpen::install_local_store(Arc::clone(&store), cx);
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
