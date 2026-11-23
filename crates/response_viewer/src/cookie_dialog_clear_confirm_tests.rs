//! What the Manage Cookies dialog's Clear and delete remove, and that what they remove
//! stays removed (REQ-HTTP-08): Clear takes exactly the cookies the dialog listed — not
//! one a send put in the jar while the dialog was open — a delete takes exactly its own
//! (domain, path, name), and a delete persists even when the dialog closes before its
//! save ran. Driven through the controls' own focus handles and Enter, with a real
//! `CookieJar` and `storage::LocalStore`; every persistence assertion reloads the jar
//! with `load_jar`. Fixture values are obviously fake (`.agents/rules/tests.md`).

use std::sync::{Arc, Mutex};

use gpui::{DismissEvent, Entity, TestAppContext, VisualTestContext};
use http_engine::{CookieJar, load_jar};
use pustman_actions::cookies::Manage;
use storage::LocalStore;
use ui::test_support::{draw_window, init_test, press_key};
use url::Url;
use workspace::Workspace;

use super::{CookieDialog, CookieDialogInputs, install_inputs};

const WORKSPACE_ID: &str = "workspace-a";
const API_DOMAIN: &str = "api.example.test";

/// Obviously fake cookie values (`.agents/rules/tests.md`).
const SID_ROOT_VALUE: &str = "token-REDACTED-root";
const SID_API_VALUE: &str = "token-REDACTED-api";
const LATE_VALUE: &str = "token-REDACTED-late";

fn url(text: &str) -> Url {
    Url::parse(text).expect("the fixture URL parses")
}

/// Two cookies both named `sid` under one domain, on paths `/` and `/api`.
fn two_sid_jar() -> CookieJar {
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &url("https://api.example.test/"),
        &[format!("sid={SID_ROOT_VALUE}; Path=/")],
    );
    jar.store_response_cookies(
        &url("https://api.example.test/api"),
        &[format!("sid={SID_API_VALUE}; Path=/api")],
    );
    jar
}

fn open_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    Arc::new(Mutex::new(store))
}

fn dialog_window(cx: &mut TestAppContext) -> (Entity<Workspace>, &mut VisualTestContext) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
    (workspace, cx)
}

fn open_dialog(
    workspace: &Entity<Workspace>,
    store: Arc<Mutex<LocalStore>>,
    jar: Arc<Mutex<CookieJar>>,
    cx: &mut VisualTestContext,
) -> Entity<CookieDialog> {
    cx.update(|_window, cx| {
        install_inputs(
            cx,
            CookieDialogInputs {
                workspace_id: WORKSPACE_ID.to_string(),
                store,
                jar,
            },
        )
    });
    cx.dispatch_action(Manage);
    cx.run_until_parked();
    draw_window(cx);
    cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<CookieDialog>()
        })
    })
    .expect("dispatching cookies::Manage opens the dialog")
}

fn press_focused(focus_handle: gpui::FocusHandle, cx: &mut VisualTestContext) {
    cx.update(|window, cx| window.focus(&focus_handle, cx));
    draw_window(cx);
    press_key(cx, "enter");
    cx.run_until_parked();
    draw_window(cx);
}

/// Turn the domain header's Clear: it asks first.
fn ask_clear(dialog: &Entity<CookieDialog>, cx: &mut VisualTestContext) {
    let clear = cx
        .read(|cx| dialog.read_with(cx, |dialog, _| dialog.clear_focus_handle(API_DOMAIN)))
        .expect("the domain header has a Clear button");
    press_focused(clear, cx);
}

/// Confirm the open Clear prompt through its own confirm button.
fn confirm_clear(dialog: &Entity<CookieDialog>, cx: &mut VisualTestContext) {
    let confirm = cx
        .read(|cx| dialog.read_with(cx, |dialog, _| dialog.clear_confirm_focus_handle()))
        .expect("Clear asks for confirmation");
    press_focused(confirm, cx);
}

/// Path and name of every cookie the live jar holds, in (domain, path, name) order.
fn jar_names(jar: &Arc<Mutex<CookieJar>>) -> Vec<String> {
    jar.lock()
        .expect("the jar lock is healthy")
        .list()
        .into_iter()
        .map(|entry| format!("{}{}", entry.path, entry.name))
        .collect()
}

/// (path, name) of every cookie the store holds for `domain` after a reload.
async fn stored_identities(
    cx: &mut VisualTestContext,
    store: Arc<Mutex<LocalStore>>,
    domain: &str,
) -> Vec<(String, String)> {
    let background = cx.executor();
    let reloaded = load_jar(&background, store, WORKSPACE_ID.to_string())
        .await
        .expect("the jar reloads from the store");
    reloaded
        .list()
        .into_iter()
        .filter(|entry| entry.domain == domain)
        .map(|entry| (entry.path, entry.name))
        .collect()
}

#[gpui::test]
async fn a_cookie_set_after_the_dialog_opened_survives_clear(cx: &mut TestAppContext) {
    let store = open_store();
    let jar = Arc::new(Mutex::new(two_sid_jar()));
    let (workspace, cx) = dialog_window(cx);
    let dialog = open_dialog(&workspace, Arc::clone(&store), Arc::clone(&jar), cx);

    // A send lands while the dialog is open: the jar gains a cookie the rows never showed.
    jar.lock()
        .expect("the jar lock is healthy")
        .store_response_cookies(
            &url("https://api.example.test/"),
            &[format!("late={LATE_VALUE}; Path=/")],
        );

    let clear = cx
        .read(|cx| dialog.read_with(cx, |dialog, _| dialog.clear_focus_handle(API_DOMAIN)))
        .expect("the domain header has a Clear button");
    press_focused(clear, cx);
    confirm_clear(&dialog, cx);

    let in_jar: Vec<String> = jar
        .lock()
        .expect("the jar lock is healthy")
        .list()
        .into_iter()
        .map(|entry| entry.name)
        .collect();
    assert_eq!(
        in_jar,
        vec!["late".to_string()],
        "Clear removed the two listed cookies and nothing it never showed"
    );
    assert_eq!(
        stored_identities(cx, store, API_DOMAIN).await,
        vec![("/".to_string(), "late".to_string())],
        "the store holds the unlisted cookie and neither listed one"
    );
}

#[gpui::test]
async fn deleting_one_of_two_same_named_cookies_keeps_the_other(cx: &mut TestAppContext) {
    let store = open_store();
    let jar = Arc::new(Mutex::new(two_sid_jar()));
    let (workspace, cx) = dialog_window(cx);
    let dialog = open_dialog(&workspace, Arc::clone(&store), Arc::clone(&jar), cx);

    let delete = cx
        .read(|cx| {
            dialog.read_with(cx, |dialog, _| {
                dialog.delete_focus_handle(API_DOMAIN, "/api", "sid")
            })
        })
        .expect("the /api sid row has a delete button");
    press_focused(delete, cx);

    let rows = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()));
    let cookie_rows: Vec<(String, String)> = rows
        .iter()
        .filter(|row| !row.group_header)
        .map(|row| (row.path.clone(), row.name.clone()))
        .collect();
    assert_eq!(
        cookie_rows,
        vec![("/".to_string(), "sid".to_string())],
        "only the /api sid row went"
    );
    assert_eq!(
        stored_identities(cx, store, API_DOMAIN).await,
        vec![("/".to_string(), "sid".to_string())],
        "the store keeps the / sid"
    );
}

#[gpui::test]
async fn a_delete_persists_when_the_dialog_closes_before_its_save_ran(cx: &mut TestAppContext) {
    let store = open_store();
    let jar = Arc::new(Mutex::new(two_sid_jar()));
    let (workspace, cx) = dialog_window(cx);
    let dialog = open_dialog(&workspace, Arc::clone(&store), Arc::clone(&jar), cx);
    let entry = jar
        .lock()
        .expect("the jar lock is healthy")
        .list()
        .into_iter()
        .find(|entry| entry.path == "/api")
        .expect("the /api sid is in the jar");

    // The delete and the close land in one turn of the foreground: nothing has run the
    // delete's save yet when the dialog goes away.
    cx.update(|_window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.delete_cookie(&entry, cx);
            cx.emit(DismissEvent);
        })
    });
    let released = dialog.downgrade();
    drop(dialog);
    // The window redraws without the dialog — the frame that last drew it lets go — and
    // only then does the foreground get to run anything the dialog started.
    draw_window(cx);
    assert!(
        released.upgrade().is_none(),
        "closing released the dialog before its save got to run"
    );
    cx.run_until_parked();

    let still_open = cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<CookieDialog>()
        })
    });
    assert!(still_open.is_none(), "the dialog closed");
    assert_eq!(
        stored_identities(cx, store, API_DOMAIN).await,
        vec![("/".to_string(), "sid".to_string())],
        "the deleted cookie does not come back from the store"
    );
}

#[gpui::test]
fn clear_asks_for_confirmation_naming_the_domain_and_count(cx: &mut TestAppContext) {
    let jar = Arc::new(Mutex::new(two_sid_jar()));
    let (workspace, cx) = dialog_window(cx);
    let dialog = open_dialog(&workspace, open_store(), Arc::clone(&jar), cx);

    ask_clear(&dialog, cx);

    let prompt = cx
        .read(|cx| dialog.read_with(cx, |dialog, _| dialog.clear_confirm_prompt()))
        .expect("Clear shows a confirm prompt");
    assert!(
        prompt.contains(API_DOMAIN) && prompt.contains("2 cookies"),
        "the prompt names the domain and how many cookies go: {prompt}"
    );
    assert_eq!(
        jar_names(&jar),
        vec!["/sid".to_string(), "/apisid".to_string()],
        "asking removes nothing"
    );
}

#[gpui::test]
fn cancel_and_escape_keep_the_cookies(cx: &mut TestAppContext) {
    let jar = Arc::new(Mutex::new(two_sid_jar()));
    let (workspace, cx) = dialog_window(cx);
    let dialog = open_dialog(&workspace, open_store(), Arc::clone(&jar), cx);

    ask_clear(&dialog, cx);
    let cancel = cx
        .read(|cx| dialog.read_with(cx, |dialog, _| dialog.clear_cancel_focus_handle()))
        .expect("the prompt has a Cancel button");
    press_focused(cancel, cx);
    assert!(
        cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.clear_confirm_prompt()))
            .is_none(),
        "Cancel closes the prompt"
    );

    ask_clear(&dialog, cx);
    press_key(cx, "escape");
    cx.run_until_parked();
    draw_window(cx);
    assert!(
        cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.clear_confirm_prompt()))
            .is_none(),
        "Escape closes the prompt"
    );
    let still_open = cx.read(|cx| {
        workspace.read_with(cx, |workspace, _| {
            workspace.modal_layer().active_modal::<CookieDialog>()
        })
    });
    assert!(
        still_open.is_some(),
        "Escape on the prompt keeps the dialog open"
    );

    assert_eq!(
        jar_names(&jar),
        vec!["/sid".to_string(), "/apisid".to_string()],
        "neither Cancel nor Escape removed a cookie"
    );
    let rows = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.displayed_rows()));
    assert_eq!(
        rows.iter().filter(|row| !row.group_header).count(),
        2,
        "both cookie rows are still listed"
    );
}

#[gpui::test]
async fn confirm_removes_exactly_the_cookies_the_prompt_counted(cx: &mut TestAppContext) {
    let store = open_store();
    let jar = Arc::new(Mutex::new(two_sid_jar()));
    let (workspace, cx) = dialog_window(cx);
    let dialog = open_dialog(&workspace, Arc::clone(&store), Arc::clone(&jar), cx);

    ask_clear(&dialog, cx);
    // A send lands while the prompt is up: its cookie was never counted.
    jar.lock()
        .expect("the jar lock is healthy")
        .store_response_cookies(
            &url("https://api.example.test/"),
            &[format!("late={LATE_VALUE}; Path=/")],
        );
    confirm_clear(&dialog, cx);

    assert_eq!(
        jar_names(&jar),
        vec!["/late".to_string()],
        "confirm removed the two counted cookies and only them"
    );
    assert_eq!(
        stored_identities(cx, store, API_DOMAIN).await,
        vec![("/".to_string(), "late".to_string())],
        "the store agrees"
    );
}
