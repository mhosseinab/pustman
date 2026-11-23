//! A Manage Cookies save that fails reaches the user (AGENTS.md: errors reach the user):
//! inline in the dialog while it is open, and as a workspace toast when the dialog closed
//! before the failed save landed (S38 R3 review, MEDIUM 1). The store is made to fail by
//! poisoning its lock, which `cookie_persistence` reports as a fixed, value-free error.

use std::sync::{Arc, Mutex};

use gpui::{DismissEvent, Entity, TestAppContext, VisualTestContext};
use http_engine::{CookieEntry, CookieJar};
use pustman_actions::cookies::Manage;
use storage::LocalStore;
use ui::test_support::{draw_window, init_test};
use url::Url;
use workspace::Workspace;

use super::{CookieDialog, CookieDialogInputs, install_inputs};

const WORKSPACE_ID: &str = "workspace-a";
/// An obviously fake cookie value (`.agents/rules/tests.md`).
const SID_VALUE: &str = "token-REDACTED-sid";

fn jar_with_one_cookie() -> Arc<Mutex<CookieJar>> {
    let mut jar = CookieJar::new();
    jar.store_response_cookies(
        &Url::parse("https://api.example.test/").expect("the fixture URL parses"),
        &[format!("sid={SID_VALUE}; Path=/")],
    );
    Arc::new(Mutex::new(jar))
}

/// A store whose lock a panicking writer left poisoned: every save through it fails.
fn failing_store() -> Arc<Mutex<LocalStore>> {
    let mut store = LocalStore::open_in_memory().expect("the in-memory store opens");
    store.migrate().expect("the schema migrates");
    let store = Arc::new(Mutex::new(store));
    let poisoner = Arc::clone(&store);
    let joined = std::thread::spawn(move || {
        let _held = poisoner.lock().expect("the store lock is free");
        panic!("poison the store lock for the test");
    })
    .join();
    assert!(
        joined.is_err(),
        "the poisoning thread panicked while holding the lock"
    );
    store
}

fn open_dialog(
    cx: &mut TestAppContext,
    store: Arc<Mutex<LocalStore>>,
    jar: Arc<Mutex<CookieJar>>,
) -> (
    Entity<Workspace>,
    Entity<CookieDialog>,
    &mut VisualTestContext,
) {
    cx.update(|cx| {
        init_test(cx);
        crate::init(cx);
    });
    let (workspace, cx) = cx.add_window_view(Workspace::new);
    cx.update(|window, _cx| window.activate_window());
    draw_window(cx);
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
    let dialog = cx
        .read(|cx| {
            workspace.read_with(cx, |workspace, _| {
                workspace.modal_layer().active_modal::<CookieDialog>()
            })
        })
        .expect("dispatching cookies::Manage opens the dialog");
    (workspace, dialog, cx)
}

fn only_entry(jar: &Arc<Mutex<CookieJar>>) -> CookieEntry {
    jar.lock()
        .expect("the jar lock is healthy")
        .list()
        .into_iter()
        .next()
        .expect("the jar holds the fixture cookie")
}

fn toast_count(workspace: &Entity<Workspace>, cx: &mut VisualTestContext) -> usize {
    cx.read(|cx| workspace.read_with(cx, |workspace, cx| workspace.toasts().read(cx).len()))
}

#[gpui::test]
fn a_failed_save_shows_inline_while_the_dialog_is_open(cx: &mut TestAppContext) {
    let jar = jar_with_one_cookie();
    let (workspace, dialog, cx) = open_dialog(cx, failing_store(), Arc::clone(&jar));
    let entry = only_entry(&jar);

    cx.update(|_window, cx| dialog.update(cx, |dialog, cx| dialog.delete_cookie(&entry, cx)));
    cx.run_until_parked();

    let error = cx.read(|cx| dialog.read_with(cx, |dialog, _| dialog.error.clone()));
    assert!(
        error.is_some(),
        "the failed save is shown in the open dialog"
    );
    assert_eq!(
        toast_count(&workspace, cx),
        0,
        "an open dialog shows the failure itself, not as a toast"
    );
}

#[gpui::test]
fn a_failed_save_after_the_dialog_closed_shows_a_toast(cx: &mut TestAppContext) {
    let jar = jar_with_one_cookie();
    let (workspace, dialog, cx) = open_dialog(cx, failing_store(), Arc::clone(&jar));
    let entry = only_entry(&jar);

    // The delete and the close land in one turn: the save has not run when the dialog goes.
    cx.update(|_window, cx| {
        dialog.update(cx, |dialog, cx| {
            dialog.delete_cookie(&entry, cx);
            cx.emit(DismissEvent);
        })
    });
    let released = dialog.downgrade();
    drop(dialog);
    // The frame that last drew the dialog lets go of it only on the next draw.
    draw_window(cx);
    assert!(
        released.upgrade().is_none(),
        "the dialog is gone before its save ran"
    );
    cx.run_until_parked();

    assert_eq!(
        toast_count(&workspace, cx),
        1,
        "a save that failed after the dialog closed reaches the user as a toast"
    );
}
